use iced::clipboard;
use iced::theme::{self, Palette};
use iced::widget::{
    button, column, container, horizontal_space, mouse_area, row, scrollable, svg, text, text_editor,
};
use iced::widget::scrollable::AbsoluteOffset;
use iced::{Application, Border, Color, Command, Element, Length, Subscription, Theme, executor, window};
use std::process::{Command as ProcCommand, Stdio};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tracing::info;

use crate::config::app_config::load_or_init_config;
use crate::domain::models::{CheckResult, ProxyEntry, RealIpHistoryEntry};
use crate::repository::sqlite_repo::{AppRepository, SqliteRepository};
use crate::service::ip_service::{CheckProxyBatchOutcome, IpCheckService, resolve_db_path};

/// 多列 `FillPortion` 在 `scrollable` 内需要明确宽度；过窄会塌列，故设下限。
/// 实际宽度取 `max(本值, 窗口内容区宽度 - 边距)`，宽屏下可铺满。
const TABLE_SCROLL_MIN_WIDTH: f32 = 2400.0;
/// 表格区域左右 padding 等预留
const TABLE_VIEWPORT_WIDTH_TRIM: f32 = 32.0;
/// 序号列：约容纳 3 位数字
const TABLE_COL_INDEX_WIDTH: f32 = 42.0;
/// 操作列：三枚小按钮 + 间距，略大于内容宽度即可
const TABLE_COL_OPS_WIDTH: f32 = 198.0;
/// 左侧数据区（不含操作列）的最小宽度；已扣除已移除的「今日」列约宽。
const TABLE_LEFT_MIN_WIDTH: f32 = TABLE_SCROLL_MIN_WIDTH - TABLE_COL_OPS_WIDTH - 40.0;
/// 窗口内容区不大于 1920×1080 时，四类风险合并为一列多行展示
const COMPACT_VIEWPORT_MAX_W: f32 = 1920.0;
const COMPACT_VIEWPORT_MAX_H: f32 = 1080.0;
/// 合并列占用的 FillPortion（原 行为/关联/恶意/其他 各 2，合计 8）
const RISK_STACK_PORTION: u16 = 8;

/// 表格横向滚动同步：仅表头显示滚动条；数据行隐藏条，通过 `scroll_to` 对齐偏移。
const TABLE_H_SCROLL_EPS: f32 = 0.75;

/// 横向滚动事件来源（用于 `scroll_to` 同步时避免重复下发）。
#[derive(Debug, Clone, Copy)]
pub enum TableHScrollSource {
    Header,
    Row(usize),
}

fn table_h_scroll_id_header() -> scrollable::Id {
    scrollable::Id::new("ipcheck_tbl_h_hdr")
}

fn table_h_scroll_id_row(idx: usize) -> scrollable::Id {
    scrollable::Id::new(format!("ipcheck_tbl_h_{idx}"))
}

/// zenity 导入窗口顶部说明（类似 placeholder 提示）
const IMPORT_DIALOG_HINT: &str = "在下方编辑区粘贴或输入代理；支持格式示例：\n• socks5://username:password@host:port\n• host|port|username|password\n• socks5://host:port---username---password\n• ip:port username password\n\n编辑完成后点「确定」导入。";

fn is_risk_rate_limit_msg(s: &str) -> bool {
    s.contains("风控接口限速")
}

/// 独立子进程弹出限速说明（zenity / notify-send），不遮挡主窗口表格。
fn spawn_rate_limit_external_dialog(detail: &str) {
    let title = "百度风控接口限速";
    let detail_trim = detail
        .strip_prefix("风控接口限速:")
        .map(str::trim)
        .unwrap_or(detail);
    let body = format!(
        "说明：{detail_trim}\n\n表格内「风控」与工具栏「风控检查」已暂停；未完成的 IP 不会写入结果。重启应用后可再试。请遵守接口频次。"
    );
    let zenity = ProcCommand::new("zenity")
        .arg("--warning")
        .arg("--title")
        .arg(title)
        .arg("--text")
        .arg(&body)
        .arg("--width=520")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if zenity.is_ok() {
        return;
    }
    let _ = ProcCommand::new("notify-send")
        .arg("-u")
        .arg("critical")
        .arg(title)
        .arg(&body)
        .stdin(Stdio::null())
        .spawn();
}

#[derive(Clone)]
struct Toast {
    id: u64,
    text: String,
}

pub struct IpCheckApp {
    service: Option<IpCheckService<SqliteRepository>>,
    api_token: String,
    show_clear_confirm: bool,
    show_single_check_modal: bool,
    show_single_result_modal: bool,
    single_check_content: text_editor::Content,
    draft_single_result: Option<CheckResult>,
    proxies: Vec<ProxyEntry>,
    results: Vec<CheckResult>,
    real_ip_histories: HashMap<i64, Vec<RealIpHistoryEntry>>,
    expanded_real_ip_history: HashSet<i64>,
    loading_real_ip_history: HashSet<i64>,
    busy: bool,
    toast: Option<Toast>,
    toast_seq: u64,
    last_title_click_at: Option<Instant>,
    /// 主窗口内容区逻辑宽度，用于表格随窗口拉宽铺满。
    window_viewport_width: f32,
    /// 主窗口内容区高度，与宽度共同决定是否采用紧凑风险列布局。
    window_viewport_height: f32,
    /// 与表头/各行横向 `Scrollable` 同步的 X 偏移（像素），用于去重 `on_scroll`。
    table_h_scroll_x: f32,
    /// 启动完成后是否已下发过一次 `maximize`。
    startup_maximize_done: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Result<LoadedData, String>),
    OpenImportModal,
    ImportExternalDone(Result<String, String>),
    OpenSingleCheckModal,
    CloseSingleCheckModal,
    SingleCheckContentAction(text_editor::Action),
    /// 兜底：`iced 0.12` 下 `text_editor` 可能无法正确绑定 `Ctrl+A`，
    /// 这里在应用层捕获键盘事件后强制全选。
    SingleCheckCtrlASelectAll,
    Imported(Result<Vec<ProxyEntry>, String>),
    QueryAllRealIp,
    RealIpDone(Result<Vec<(i64, String, String)>, String>),
    QueryOneRealIp(i64),
    QueryOneRealIpDone(Result<(i64, String, String), (i64, String)>),
    ToggleRealIpHistory(i64),
    RealIpHistoryDone(Result<(i64, Vec<RealIpHistoryEntry>), (i64, String)>),
    RiskCheckOne(i64),
    RiskCheckOneDone(Result<CheckResult, String>),
    CheckAll,
    CheckedAll(Result<CheckProxyBatchOutcome, String>),
    StartSingleCheck,
    StartSingleCheckDone(Result<CheckResult, String>),
    SaveSingleResult,
    SaveSingleResultDone(Result<(), String>),
    CloseSingleResultModal,
    DeleteOne(i64),
    DeletedOne(Result<i64, String>),
    AskClearList,
    CancelClearList,
    ConfirmClearList,
    ClearedList(Result<(), String>),
    ToastExpired(u64),
    TitleBarPressed,
    /// 主窗口首次创建完成（更新视口；无数据库时在此完成一次最大化）。
    WindowOpened { width: f32, height: f32 },
    WindowViewport { width: f32, height: f32 },
    /// 点击表格单元格将内容写入剪贴板（普通文本无输入框样式）
    CopyCellToClipboard(String),
    /// 占位：忙碌时按钮仍响应但不做事
    Noop,
    /// 表头或某数据行横向滚动，同步其余 `Scrollable` 的偏移。
    TableHorizontalScroll {
        offset_x: f32,
        source: TableHScrollSource,
    },
}

fn cmd_maximize_main_window() -> Command<Message> {
    iced::window::maximize(iced::window::Id::MAIN, true)
}

/// 启动完成后仅一次：有数据库时在首次 `Loaded` 后调用；无数据库时在首次 `WindowOpened` 调用。
fn first_startup_maximize_once(app: &mut IpCheckApp) -> Command<Message> {
    if app.startup_maximize_done {
        return Command::none();
    }
    app.startup_maximize_done = true;
    cmd_maximize_main_window()
}

#[derive(Debug, Clone)]
pub struct LoadedData {
    token: String,
    proxies: Vec<ProxyEntry>,
    results: Vec<CheckResult>,
}

impl Application for IpCheckApp {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = ();

    fn new(_flags: ()) -> (Self, Command<Self::Message>) {
        let service = build_service();
        let config = load_or_init_config().unwrap_or_default();
        let mut app = Self {
            service,
            api_token: config.api_token,
            show_clear_confirm: false,
            show_single_check_modal: false,
            show_single_result_modal: false,
            single_check_content: text_editor::Content::new(),
            draft_single_result: None,
            proxies: Vec::new(),
            results: Vec::new(),
            real_ip_histories: HashMap::new(),
            expanded_real_ip_history: HashSet::new(),
            loading_real_ip_history: HashSet::new(),
            busy: false,
            toast: None,
            toast_seq: 0,
            last_title_click_at: None,
            window_viewport_width: 1920.0,
            window_viewport_height: 1080.0,
            table_h_scroll_x: 0.0,
            startup_maximize_done: false,
        };

        if app.service.is_none() {
            let cmd = app.show_toast("数据库初始化失败，请检查权限后重启");
            return (app, cmd);
        }

        let service = app.service.clone();
        let config_token = app.api_token.clone();
        let cmd = Command::perform(
            async move {
                let Some(service) = service else {
                    return Err::<LoadedData, String>("service unavailable".to_string());
                };
                if !config_token.trim().is_empty() {
                    service
                        .save_token(config_token.as_str())
                        .map_err(|e| e.to_string())?;
                }
                let snapshot = service.load_snapshot().map_err(|e| e.to_string())?;
                Ok::<LoadedData, String>(LoadedData {
                    token: snapshot.token,
                    proxies: snapshot.proxies,
                    results: snapshot.results,
                })
            },
            Message::Loaded,
        );
        (app, cmd)
    }

    fn title(&self) -> String {
        "IP质量检测工具".to_string()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        iced::event::listen_with(window_event_to_viewport_message)
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        info!(?message, "app received message");
        match message {
            Message::SingleCheckCtrlASelectAll => {
                if self.show_single_check_modal {
                    // iced 0.12 的 `Action` 不包含 `SelectAll`，用文档起止 motion 拼一个“全选”。
                    self.single_check_content
                        .perform(text_editor::Action::Move(
                            text_editor::Motion::DocumentStart,
                        ));
                    self.single_check_content
                        .perform(text_editor::Action::Select(
                            text_editor::Motion::DocumentEnd,
                        ));
                }
                Command::none()
            }
            Message::TableHorizontalScroll { offset_x, source } => {
                if (offset_x - self.table_h_scroll_x).abs() < TABLE_H_SCROLL_EPS {
                    return Command::none();
                }
                self.table_h_scroll_x = offset_x;
                let off = AbsoluteOffset {
                    x: offset_x,
                    y: 0.0,
                };
                let mut cmds: Vec<Command<Message>> = Vec::new();
                match source {
                    TableHScrollSource::Header => {
                        for idx in 0..self.proxies.len() {
                            cmds.push(scrollable::scroll_to(
                                table_h_scroll_id_row(idx),
                                off,
                            ));
                        }
                    }
                    TableHScrollSource::Row(i) => {
                        cmds.push(scrollable::scroll_to(table_h_scroll_id_header(), off));
                        for j in 0..self.proxies.len() {
                            if j != i {
                                cmds.push(scrollable::scroll_to(
                                    table_h_scroll_id_row(j),
                                    off,
                                ));
                            }
                        }
                    }
                }
                Command::batch(cmds)
            }
            Message::Loaded(result) => {
                let toast_cmd = match result {
                    Ok(data) => {
                        if self.api_token.trim().is_empty() && !data.token.trim().is_empty() {
                            self.api_token = data.token;
                        }
                        self.proxies = data.proxies;
                        self.results = data.results;
                        self.show_toast("已加载本地数据")
                    }
                    Err(err) => self.show_toast(&format!("加载失败: {err}")),
                };
                Command::batch([toast_cmd, first_startup_maximize_once(self)])
            }
            Message::OpenImportModal => {
                Command::perform(
                    async move { open_import_window() },
                    Message::ImportExternalDone,
                )
            }
            Message::ImportExternalDone(result) => {
                match result {
                    Ok(text) => {
                        let Some(service) = self.service.clone() else {
                            return self.show_toast("服务不可用");
                        };
                        self.busy = true;
                        Command::perform(
                            async move {
                                let parsed = service.parse_import_text(&text).map_err(|e| e.to_string())?;
                                service.save_imported_proxies(&parsed).map_err(|e| e.to_string())?;
                                let snapshot = service.load_snapshot().map_err(|e| e.to_string())?;
                                Ok::<Vec<ProxyEntry>, String>(snapshot.proxies)
                            },
                            Message::Imported,
                        )
                    }
                    Err(err) => {
                        if err == "__CANCELLED__" {
                            Command::none()
                        } else {
                            self.show_toast(&format!("打开导入窗口失败: {err}"))
                        }
                    }
                }
            }
            Message::OpenSingleCheckModal => {
                self.show_single_check_modal = true;
                Command::none()
            }
            Message::CloseSingleCheckModal => {
                self.show_single_check_modal = false;
                Command::none()
            }
            Message::SingleCheckContentAction(action) => {
                self.single_check_content.perform(action);
                Command::none()
            }
            Message::Imported(result) => {
                self.busy = false;
                match result {
                    Ok(items) => {
                        self.proxies = items;
                        self.show_toast("导入成功")
                    }
                    Err(err) => self.show_toast(&format!("导入失败: {err}")),
                }
            }
            Message::QueryAllRealIp => {
                if self.proxies.is_empty() {
                    return self.show_toast("请先导入 s5 列表");
                }
                let Some(service) = self.service.clone() else {
                    return self.show_toast("服务不可用");
                };
                let _ = service.clear_all_real_ip_rows();
                for p in &mut self.proxies {
                    p.last_real_ip = None;
                    p.updated_at = None;
                }
                let token = self.api_token.clone();
                let proxies = self.proxies.clone();
                self.busy = true;
                Command::perform(
                    async move {
                        let tasks = proxies.into_iter().map(|item| {
                            let service = service.clone();
                            let token = token.clone();
                            async move {
                                let id = item.id;
                                let host = item.host.clone();
                                match service.resolve_real_ip(item, token).await {
                                    Ok(v) => v,
                                    Err(_) => (
                                        id,
                                        host,
                                        chrono::Local::now()
                                            .format("%Y-%m-%d %H:%M:%S")
                                            .to_string(),
                                    ),
                                }
                            }
                        });
                        let joined = futures::future::join_all(tasks).await;
                        let mut out = Vec::new();
                        for item in joined {
                            out.push(item);
                        }
                        Ok::<Vec<(i64, String, String)>, String>(out)
                    },
                    Message::RealIpDone,
                )
            }
            Message::RealIpDone(result) => {
                self.busy = false;
                match result {
                    Ok(updated) => {
                        for (id, ip, updated_at) in updated {
                            if let Some(proxy) = self.proxies.iter_mut().find(|p| p.id == id) {
                                proxy.last_real_ip = Some(ip);
                                proxy.updated_at = Some(updated_at);

                                // 若用户已展开历史：仅在 IP 发生变化时追加到内存中，避免重复。
                                if self.expanded_real_ip_history.contains(&id) {
                                    let hist = self
                                        .real_ip_histories
                                        .entry(id)
                                        .or_insert_with(Vec::new);
                                    let ip_trim = proxy
                                        .last_real_ip
                                        .as_deref()
                                        .unwrap_or_default()
                                        .trim()
                                        .to_string();
                                    let last_ip_same = hist
                                        .last()
                                        .map(|x| x.real_ip.trim() == ip_trim)
                                        .unwrap_or(false);
                                    if !last_ip_same {
                                        hist.push(RealIpHistoryEntry {
                                            id: 0,
                                            proxy_id: id,
                                            real_ip: ip_trim,
                                            observed_at: proxy
                                                .updated_at
                                                .as_deref()
                                                .unwrap_or_default()
                                                .to_string(),
                                        });
                                    }
                                }
                            }
                        }
                        self.show_toast("查询IP完成")
                    }
                    Err(err) => self.show_toast(&format!("查询IP失败: {err}")),
                }
            }
            Message::ToggleRealIpHistory(proxy_id) => {
                if self.expanded_real_ip_history.contains(&proxy_id) {
                    self.expanded_real_ip_history.remove(&proxy_id);
                    return Command::none();
                }
                self.expanded_real_ip_history.insert(proxy_id);

                if self.real_ip_histories.contains_key(&proxy_id) || self.loading_real_ip_history.contains(&proxy_id) {
                    return Command::none();
                }

                let Some(service) = self.service.clone() else {
                    return self.show_toast("服务不可用");
                };
                self.loading_real_ip_history.insert(proxy_id);

                Command::perform(
                    async move {
                        let history = service
                            .get_real_ip_history(proxy_id)
                            .map_err(|e| (proxy_id, e.to_string()))?;
                        Ok::<(i64, Vec<RealIpHistoryEntry>), (i64, String)>((proxy_id, history))
                    },
                    |res| match res {
                        Ok(ok) => Message::RealIpHistoryDone(Ok(ok)),
                        Err((id, err)) => Message::RealIpHistoryDone(Err((id, err))),
                    },
                )
            }
            Message::RealIpHistoryDone(result) => {
                match result {
                    Ok((id, history)) => {
                        self.loading_real_ip_history.remove(&id);
                        self.real_ip_histories.insert(id, history);
                    }
                    Err((id, err)) => {
                        self.loading_real_ip_history.remove(&id);
                        let _ = self.show_toast(&format!("加载真实IP历史失败: {err}"));
                    }
                }
                Command::none()
            }
            Message::QueryOneRealIp(id) => {
                let Some(service) = self.service.clone() else {
                    return self.show_toast("服务不可用");
                };
                let Some(proxy) = self.proxies.iter().find(|x| x.id == id).cloned() else {
                    return self.show_toast("代理不存在");
                };
                let _ = service.clear_real_ip_row(id);
                if let Some(p) = self.proxies.iter_mut().find(|p| p.id == id) {
                    p.last_real_ip = None;
                    p.updated_at = None;
                }
                let token = self.api_token.clone();
                self.busy = true;
                Command::perform(
                    async move {
                        let fallback_id = proxy.id;
                        let fallback_host = proxy.host.clone();
                        service
                            .resolve_real_ip(proxy, token)
                            .await
                            .map_err(|e| (fallback_id, format!("{e}; fallback={fallback_host}")))
                    },
                    Message::QueryOneRealIpDone,
                )
            }
            Message::QueryOneRealIpDone(result) => {
                self.busy = false;
                match result {
                    Ok((id, ip, updated_at)) => {
                        if let Some(proxy) = self.proxies.iter_mut().find(|p| p.id == id) {
                            proxy.last_real_ip = Some(ip);
                            proxy.updated_at = Some(updated_at);
                        }
                        self.show_toast("单条查询成功")
                    }
                    Err((id, err)) => {
                        if let Some(proxy) = self.proxies.iter_mut().find(|p| p.id == id) {
                            proxy.last_real_ip = Some(proxy.host.clone());
                            proxy.updated_at = Some(
                                chrono::Local::now()
                                    .format("%Y-%m-%d %H:%M:%S")
                                    .to_string(),
                            );
                        }
                        self.show_toast(&format!("单条查询失败，已回填代理IP: {err}"))
                    }
                }
            }
            Message::RiskCheckOne(id) => {
                let Some(service) = self.service.clone() else {
                    return self.show_toast("服务不可用");
                };
                let Some(proxy) = self.proxies.iter().find(|x| x.id == id).cloned() else {
                    return self.show_toast("代理不存在");
                };
                let _ = service.delete_results_for_proxy(id);
                self.results.retain(|r| r.proxy_id != id);
                let token = self.api_token.clone();
                self.busy = true;
                Command::perform(
                    async move { service.check_proxy_ip(proxy, token).await.map_err(|e| e.to_string()) },
                    Message::RiskCheckOneDone,
                )
            }
            Message::RiskCheckOneDone(result) => {
                self.busy = false;
                match result {
                    Ok(item) => {
                        if let Some(service) = self.service.clone() {
                            if let Err(e) = service.save_result(&item) {
                                tracing::warn!(
                                    proxy_id = item.proxy_id,
                                    error = %e,
                                    "风控结果写入数据库失败"
                                );
                            }
                        }
                        if let Some(p) = self.proxies.iter_mut().find(|p| p.id == item.proxy_id) {
                            p.last_real_ip = Some(item.real_ip.clone());
                            p.updated_at = Some(item.checked_at.clone());

                            if self.expanded_real_ip_history.contains(&item.proxy_id) {
                                let hist = self
                                    .real_ip_histories
                                    .entry(item.proxy_id)
                                    .or_insert_with(Vec::new);
                                let ip_trim = item.real_ip.trim().to_string();
                                let last_ip_same = hist
                                    .last()
                                    .map(|x| x.real_ip.trim() == ip_trim)
                                    .unwrap_or(false);
                                if !last_ip_same {
                                    hist.push(RealIpHistoryEntry {
                                        id: 0,
                                        proxy_id: item.proxy_id,
                                        real_ip: ip_trim,
                                        observed_at: item.checked_at.clone(),
                                    });
                                }
                            }
                        }
                        self.results.retain(|r| r.proxy_id != item.proxy_id);
                        self.results.insert(0, item);
                        self.show_toast("单条风控查询完成")
                    }
                    Err(err) => {
                        if is_risk_rate_limit_msg(&err) {
                            spawn_rate_limit_external_dialog(&err);
                            self.show_toast("本条代理风控限速（可换其他行再试），主窗口未遮挡")
                        } else {
                            self.show_toast(&format!("风控查询失败: {err}"))
                        }
                    }
                }
            }
            Message::CheckAll => {
                if self.proxies.is_empty() {
                    return self.show_toast("请先导入 s5 列表");
                }
                let Some(service) = self.service.clone() else {
                    return self.show_toast("服务不可用");
                };
                let token = self.api_token.clone();
                let proxies: Vec<ProxyEntry> = self.proxies.iter().cloned().collect();
                let ids: Vec<i64> = proxies.iter().map(|p| p.id).collect();
                for pid in &ids {
                    let _ = service.delete_results_for_proxy(*pid);
                }
                self.results.retain(|r| !ids.contains(&r.proxy_id));
                self.busy = true;
                Command::perform(
                    async move {
                        service
                            .check_proxies_sequential(proxies, token)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::CheckedAll,
                )
            }
            Message::StartSingleCheck => {
                let Some(service) = self.service.clone() else {
                    return self.show_toast("服务不可用");
                };
                let token = self.api_token.clone();
                let text = self.single_check_content.text();
                self.busy = true;
                Command::perform(
                    async move {
                        let parsed = service.parse_import_text(&text).map_err(|e| e.to_string())?;
                        let spec = parsed.into_iter().next().ok_or_else(|| "请填写节点信息".to_string())?;
                        let timed = tokio::time::timeout(
                            Duration::from_secs(25),
                            service.check_proxy_spec(spec, token),
                        )
                        .await;
                        let result = match timed {
                            Ok(inner) => inner.map_err(|e| e.to_string())?,
                            Err(_) => return Err::<CheckResult, String>("单个检测超时，请更换节点后重试".to_string()),
                        };
                        Ok::<CheckResult, String>(result)
                    },
                    Message::StartSingleCheckDone,
                )
            }
            Message::StartSingleCheckDone(result) => {
                self.busy = false;
                match result {
                    Ok(item) => {
                        self.show_single_check_modal = false;
                        self.draft_single_result = Some(item);
                        self.show_single_result_modal = true;
                        Command::none()
                    }
                    Err(err) => {
                        if is_risk_rate_limit_msg(&err) {
                            spawn_rate_limit_external_dialog(&err);
                            self.show_toast("本条风控限速（可换其他节点再试），主窗口未遮挡")
                        } else {
                            self.show_toast(&format!("单个检测失败: {err}"))
                        }
                    }
                }
            }
            Message::CheckedAll(result) => {
                self.busy = false;
                match result {
                    Ok(out) => {
                        let skipped = out.skipped_rate_limit;
                        for item in out.results.into_iter().rev() {
                            if let Some(p) = self.proxies.iter_mut().find(|p| p.id == item.proxy_id) {
                                p.last_real_ip = Some(item.real_ip.clone());
                                p.updated_at = Some(item.checked_at.clone());

                                if self.expanded_real_ip_history.contains(&item.proxy_id) {
                                    let hist = self
                                        .real_ip_histories
                                        .entry(item.proxy_id)
                                        .or_insert_with(Vec::new);
                                    let ip_trim = item.real_ip.trim().to_string();
                                    let last_ip_same = hist
                                        .last()
                                        .map(|x| x.real_ip.trim() == ip_trim)
                                        .unwrap_or(false);
                                    if !last_ip_same {
                                        hist.push(RealIpHistoryEntry {
                                            id: 0,
                                            proxy_id: item.proxy_id,
                                            real_ip: ip_trim,
                                            observed_at: item.checked_at.clone(),
                                        });
                                    }
                                }
                            }
                            self.results.insert(0, item);
                        }
                        if skipped > 0 {
                            let summary = format!(
                                "批量风控中，有 {skipped} 条因百度接口限速未写入（其余代理已检测）。不同出口可继续尝试其他行。"
                            );
                            spawn_rate_limit_external_dialog(&summary);
                            self.show_toast(&format!(
                                "批量完成：已跳过 {skipped} 条（限速），其余已写入"
                            ))
                        } else {
                            self.show_toast("全部检测完成")
                        }
                    }
                    Err(err) => {
                        if is_risk_rate_limit_msg(&err) {
                            spawn_rate_limit_external_dialog(&err);
                        }
                        self.show_toast(&format!("批量检测失败: {err}"))
                    }
                }
            }
            Message::SaveSingleResult => {
                let Some(service) = self.service.clone() else {
                    return self.show_toast("服务不可用");
                };
                let Some(draft) = self.draft_single_result.clone() else {
                    return self.show_toast("没有可保存的检测结果");
                };
                self.busy = true;
                Command::perform(
                    async move { service.save_result(&draft).map_err(|e| e.to_string()) },
                    Message::SaveSingleResultDone,
                )
            }
            Message::SaveSingleResultDone(result) => {
                self.busy = false;
                match result {
                    Ok(()) => {
                        if let Some(item) = self.draft_single_result.clone() {
                            // 保持与批量/单条风控一致：先移除同一 proxy_id 的旧结果再插入新结果。
                            self.results.retain(|r| r.proxy_id != item.proxy_id);
                            self.results.insert(0, item);
                        }
                        self.show_single_result_modal = false;
                        self.draft_single_result = None;
                        self.show_toast("保存单条检测结果成功")
                    }
                    Err(err) => self.show_toast(&format!("保存失败: {err}")),
                }
            }
            Message::CloseSingleResultModal => {
                self.show_single_result_modal = false;
                self.draft_single_result = None;
                Command::none()
            }
            Message::DeleteOne(id) => {
                let Some(service) = self.service.clone() else {
                    return self.show_toast("服务不可用");
                };
                self.busy = true;
                Command::perform(
                    async move {
                        service.delete_proxy(id).map_err(|e| e.to_string())?;
                        Ok::<i64, String>(id)
                    },
                    Message::DeletedOne,
                )
            }
            Message::DeletedOne(result) => {
                self.busy = false;
                match result {
                    Ok(id) => {
                        self.proxies.retain(|p| p.id != id);
                        self.show_toast("删除成功")
                    }
                    Err(err) => self.show_toast(&format!("删除失败: {err}")),
                }
            }
            Message::AskClearList => {
                self.show_clear_confirm = true;
                Command::none()
            }
            Message::CancelClearList => {
                self.show_clear_confirm = false;
                Command::none()
            }
            Message::ConfirmClearList => {
                let Some(service) = self.service.clone() else {
                    return self.show_toast("服务不可用");
                };
                self.busy = true;
                self.show_clear_confirm = false;
                Command::perform(
                    async move { service.clear_proxy_list().map_err(|e| e.to_string()) },
                    Message::ClearedList,
                )
            }
            Message::ClearedList(result) => {
                self.busy = false;
                match result {
                    Ok(()) => {
                        self.proxies.clear();
                        self.results.clear();
                        self.show_toast("IP 列表已清空")
                    }
                    Err(err) => self.show_toast(&format!("清空失败: {err}")),
                }
            }
            Message::ToastExpired(id) => {
                if let Some(toast) = &self.toast {
                    if toast.id == id {
                        self.toast = None;
                    }
                }
                Command::none()
            }
            Message::WindowOpened { width, height } => {
                self.window_viewport_width = width.max(400.0);
                self.window_viewport_height = height.max(300.0);
                if self.service.is_none() {
                    first_startup_maximize_once(self)
                } else {
                    Command::none()
                }
            }
            Message::WindowViewport { width, height } => {
                self.window_viewport_width = width.max(400.0);
                self.window_viewport_height = height.max(300.0);
                Command::none()
            }
            Message::Noop => Command::none(),
            Message::CopyCellToClipboard(s) => {
                let toast = self.show_toast("已复制到剪贴板");
                Command::batch([clipboard::write(s), toast])
            }
            Message::TitleBarPressed => {
                let now = Instant::now();
                let is_double = self
                    .last_title_click_at
                    .map(|prev| now.duration_since(prev) <= Duration::from_millis(280))
                    .unwrap_or(false);
                self.last_title_click_at = Some(now);
                if is_double {
                    window::toggle_maximize(window::Id::MAIN)
                } else {
                    window::drag(window::Id::MAIN)
                }
            }
        }
    }

    fn view(&self) -> Element<'_, Self::Message> {
        // 窗口使用系统标题栏（decorations），此处仅保留标题条用于拖拽，不再放置第二套最小化/最大化/关闭按钮。
        let top_bar = container(
            mouse_area(
                container(text("IP质量检测工具 · SOCKS5").size(18))
                    .width(Length::Fill)
                    .padding([12, 16]),
            )
            .on_press(Message::TitleBarPressed),
        )
        .width(Length::Fill);
        let toolbar = container(top_bar);

        let actions = row![
            button(
                row![svg(icon_import()).width(Length::Fixed(12.0)).height(Length::Fixed(12.0)), text("导入IP").size(13)]
                    .spacing(6)
                    .align_items(iced::Alignment::Center),
            )
                .style(theme::Button::Primary)
                .on_press(Message::OpenImportModal),
            button(
                row![svg(icon_check()).width(Length::Fixed(12.0)).height(Length::Fixed(12.0)), text("风控检查").size(13)]
                    .spacing(6)
                    .align_items(iced::Alignment::Center),
            )
                .style(theme::Button::Primary)
                .on_press(Message::CheckAll),
            button(
                row![svg(icon_trash()).width(Length::Fixed(12.0)).height(Length::Fixed(12.0)), text("清空列表").size(13)]
                    .spacing(6)
                    .align_items(iced::Alignment::Center),
            )
                .style(theme::Button::Destructive)
                .on_press(Message::AskClearList),
            button(
                row![svg(icon_search()).width(Length::Fixed(12.0)).height(Length::Fixed(12.0)), text("查询IP").size(13)]
                    .spacing(6)
                    .align_items(iced::Alignment::Center),
            )
                .style(theme::Button::Secondary)
                .on_press(Message::QueryAllRealIp),
            button(
                row![svg(icon_check()).width(Length::Fixed(12.0)).height(Length::Fixed(12.0)), text("风控检查单个").size(13)]
                    .spacing(6)
                    .align_items(iced::Alignment::Center),
            )
                .style(theme::Button::Secondary)
                .on_press(Message::OpenSingleCheckModal),
        ]
        .spacing(10)
        .align_items(iced::Alignment::Center);
        let compact = compact_risk_layout(self.window_viewport_width, self.window_viewport_height);
        let row_align = if compact {
            iced::Alignment::Start
        } else {
            iced::Alignment::Center
        };

        // 横向条只显示在「最后一行数据」的 Scrollable 底部，避免贴在表头下方像「在顶部」。
        let h_scroll_props_visible = iced::widget::scrollable::Properties::default();
        let h_scroll_props_hidden = iced::widget::scrollable::Properties::new()
            .width(0.0)
            .scroller_width(0.0);
        let has_table_body = !self.proxies.is_empty();
        let header_h_scroll_props = if has_table_body {
            h_scroll_props_hidden
        } else {
            h_scroll_props_visible
        };
        let table_header_row = {
            let header_left_cells: Element<'static, Message> = if compact {
                row![
                    table_cell_index_header(),
                    table_cell("IP".to_string(), 2),
                    table_cell("端口".to_string(), 1),
                    table_cell("用户名".to_string(), 1),
                    table_cell("密码".to_string(), 1),
                    table_cell("协议".to_string(), 1),
                    table_cell("导入时间".to_string(), 2),
                    table_cell("真实IP".to_string(), 2),
                    table_cell("归属地".to_string(), 2),
                    table_cell("运营商".to_string(), 1),
                    table_cell("应用场景".to_string(), 1),
                    table_cell("风险详情".to_string(), RISK_STACK_PORTION),
                ]
                .spacing(6)
                .align_items(row_align)
                .into()
            } else {
                row![
                    table_cell_index_header(),
                    table_cell("IP".to_string(), 2),
                    table_cell("端口".to_string(), 1),
                    table_cell("用户名".to_string(), 1),
                    table_cell("密码".to_string(), 1),
                    table_cell("协议".to_string(), 1),
                    table_cell("导入时间".to_string(), 2),
                    table_cell("真实IP".to_string(), 2),
                    table_cell("归属地".to_string(), 2),
                    table_cell("运营商".to_string(), 1),
                    table_cell("应用场景".to_string(), 1),
                    table_cell("行为风险".to_string(), 2),
                    table_cell("关联设备风险".to_string(), 2),
                    table_cell("恶意事件风险".to_string(), 2),
                    table_cell("其他标签".to_string(), 2),
                ]
                .spacing(6)
                .align_items(row_align)
                .into()
            };

            row![
                scrollable(
                    container(header_left_cells).width(Length::Fixed(TABLE_LEFT_MIN_WIDTH)),
                )
                .direction(iced::widget::scrollable::Direction::Horizontal(
                    header_h_scroll_props,
                ))
                .id(table_h_scroll_id_header())
                .on_scroll(|vp| Message::TableHorizontalScroll {
                    offset_x: vp.absolute_offset().x,
                    source: TableHScrollSource::Header,
                })
                .width(Length::Fill),
                table_cell_ops_header(),
            ]
            .spacing(0)
            .align_items(iced::Alignment::Center)
        };

        let busy = self.busy;
        let row_count = self.proxies.len();
        let rows = self
            .proxies
            .iter()
            .enumerate()
            .fold(column![table_header_row].spacing(6), |col, (idx, proxy)| {
                let result = self.latest_result_for(proxy.id);
                let real_ip_display = result
                    .map(|r| r.real_ip.clone())
                    .or_else(|| proxy.last_real_ip.clone())
                    .unwrap_or_else(|| "-".to_string());
                let location = result
                    .map(|r| format!("{}{}{}", r.base.country, r.base.province, r.base.city))
                    .unwrap_or_else(|| "-".to_string());
                let isp = result.map(|r| r.base.isp.clone()).unwrap_or_else(|| "-".to_string());
                let scene = result.map(|r| r.base.scene.clone()).unwrap_or_else(|| "-".to_string());
                let behavior = result
                    .map(|r| join_or_none(&r.overall.behavior_risks))
                    .unwrap_or_else(|| "无".to_string());
                let device = result
                    .map(|r| join_or_none(&r.overall.device_risks))
                    .unwrap_or_else(|| "无".to_string());
                let malware = result
                    .map(|r| join_or_none(&r.overall.malware_risks))
                    .unwrap_or_else(|| "无".to_string());
                let tags = result
                    .map(|r| join_or_none(&r.overall.other_tags))
                    .unwrap_or_else(|| "无".to_string());
                let ip_risk = result_indicates_ip_risk(result);
                let risk_clean_row = result.is_some() && !ip_risk;
                let pseudo_real_ip = {
                    let ip = real_ip_display.trim();
                    !ip.is_empty() && ip != "-" && ip == proxy.host.trim()
                };

                let q_msg = if busy {
                    Message::Noop
                } else {
                    Message::QueryOneRealIp(proxy.id)
                };
                let r_msg = if busy {
                    Message::Noop
                } else {
                    Message::RiskCheckOne(proxy.id)
                };
                let d_msg = if busy {
                    Message::Noop
                } else {
                    Message::DeleteOne(proxy.id)
                };

                let history_toggle_msg = if busy {
                    Message::Noop
                } else {
                    Message::ToggleRealIpHistory(proxy.id)
                };

                let real_ip_history_expanded = self.expanded_real_ip_history.contains(&proxy.id);
                let real_ip_history_loading = self.loading_real_ip_history.contains(&proxy.id);
                let real_ip_history_opt = self.real_ip_histories.get(&proxy.id);

                let op_row = row![
                    button(
                        row![
                            svg(icon_search()).width(Length::Fixed(11.0)).height(Length::Fixed(11.0)),
                            text("查询IP").size(11)
                        ]
                        .spacing(3)
                        .align_items(iced::Alignment::Center),
                    )
                        .style(theme::Button::Secondary)
                        .padding([3, 5])
                        .on_press(q_msg),
                    button(
                        row![
                            svg(icon_check()).width(Length::Fixed(11.0)).height(Length::Fixed(11.0)),
                            text("风控").size(11)
                        ]
                        .spacing(3)
                        .align_items(iced::Alignment::Center),
                    )
                        .style(theme::Button::Primary)
                        .padding([3, 5])
                        .on_press(r_msg),
                    button(
                        row![
                            svg(icon_trash()).width(Length::Fixed(11.0)).height(Length::Fixed(11.0)),
                            text("删除").size(11)
                        ]
                        .spacing(3)
                        .align_items(iced::Alignment::Center),
                    )
                        .style(theme::Button::Destructive)
                        .padding([3, 5])
                        .on_press(d_msg),
                ]
                .spacing(3)
                .align_items(iced::Alignment::Center)
                .width(Length::Shrink);

                let created_at = proxy
                    .created_at
                    .clone()
                    .unwrap_or_else(|| "-".to_string());

                let row_left_cells: Element<'static, Message> = if compact {
                    row![
                        table_cell_index_value((idx + 1).to_string()),
                        table_cell_ip_colored(
                            proxy.host.clone(),
                            2,
                            ip_cell_display_for_host(result, ip_risk),
                        ),
                        table_cell_data(proxy.port.to_string(), 1, risk_clean_row),
                        table_cell_data(proxy.username.clone(), 1, risk_clean_row),
                        table_cell_data(proxy.password.clone(), 1, risk_clean_row),
                        table_cell_data("socks5".to_string(), 1, risk_clean_row),
                        table_cell_data(created_at, 2, risk_clean_row),
                        table_cell_real_ip_history(
                            real_ip_display,
                            2,
                            ip_cell_display_for_real(result, ip_risk, pseudo_real_ip),
                            history_toggle_msg,
                            real_ip_history_expanded,
                            real_ip_history_loading,
                            real_ip_history_opt,
                        ),
                        table_cell_data(location, 2, risk_clean_row),
                        table_cell_data(isp, 1, risk_clean_row),
                        table_cell_data(scene, 1, risk_clean_row),
                        table_cell_risk_stack_compact(
                            behavior.clone(),
                            device.clone(),
                            malware.clone(),
                            tags.clone(),
                            risk_clean_row,
                        ),
                    ]
                    .spacing(6)
                    .align_items(row_align)
                    .into()
                } else {
                    row![
                        table_cell_index_value((idx + 1).to_string()),
                        table_cell_ip_colored(
                            proxy.host.clone(),
                            2,
                            ip_cell_display_for_host(result, ip_risk),
                        ),
                        table_cell_data(proxy.port.to_string(), 1, risk_clean_row),
                        table_cell_data(proxy.username.clone(), 1, risk_clean_row),
                        table_cell_data(proxy.password.clone(), 1, risk_clean_row),
                        table_cell_data("socks5".to_string(), 1, risk_clean_row),
                        table_cell_data(created_at, 2, risk_clean_row),
                        table_cell_real_ip_history(
                            real_ip_display,
                            2,
                            ip_cell_display_for_real(result, ip_risk, pseudo_real_ip),
                            history_toggle_msg,
                            real_ip_history_expanded,
                            real_ip_history_loading,
                            real_ip_history_opt,
                        ),
                        table_cell_data(location, 2, risk_clean_row),
                        table_cell_data(isp, 1, risk_clean_row),
                        table_cell_data(scene, 1, risk_clean_row),
                        table_cell_data(behavior, 2, risk_clean_row),
                        table_cell_data(device, 2, risk_clean_row),
                        table_cell_data(malware, 2, risk_clean_row),
                        table_cell_data(tags, 2, risk_clean_row),
                    ]
                    .spacing(6)
                    .align_items(row_align)
                    .into()
                };

                let is_last_data_row = idx + 1 == row_count;
                let row_h_scroll_props = if is_last_data_row {
                    h_scroll_props_visible
                } else {
                    h_scroll_props_hidden
                };
                let row_pair = row![
                    scrollable(
                        container(row_left_cells).width(Length::Fixed(TABLE_LEFT_MIN_WIDTH)),
                    )
                    .direction(iced::widget::scrollable::Direction::Horizontal(
                        row_h_scroll_props,
                    ))
                    .id(table_h_scroll_id_row(idx))
                    .on_scroll(move |vp| Message::TableHorizontalScroll {
                        offset_x: vp.absolute_offset().x,
                        source: TableHScrollSource::Row(idx),
                    })
                    .width(Length::Fill),
                    container(op_row)
                        .width(Length::Fixed(TABLE_COL_OPS_WIDTH))
                        .padding([4, 2]),
                ]
                .spacing(0)
                .align_items(iced::Alignment::Center);

                col.push(
                    container(column![row_pair])
                        .padding([6, 10])
                        .style(theme::Container::Custom(Box::new(TableRowStripe {
                            alt: idx % 2 == 1,
                        }))),
                )
            });

        let table_scroll_width =
            (self.window_viewport_width - TABLE_VIEWPORT_WIDTH_TRIM).max(TABLE_SCROLL_MIN_WIDTH);

        let table_scroll = scrollable(
            container(rows).width(Length::Fixed(table_scroll_width)),
        )
        .direction(iced::widget::scrollable::Direction::Vertical(
            iced::widget::scrollable::Properties::default(),
        ))
        .width(Length::Fill)
        .height(Length::Fill);

        let content = column![
            toolbar,
            container(actions).padding([0, 12, 0, 12]),
            container(
                table_scroll,
            )
                .width(Length::Fill)
                .height(Length::Fill)
                .padding([0, 12, 12, 12]),
            if self.busy {
                container(text("处理中，请稍候...").size(14)).padding([0, 12, 8, 12])
            } else {
                container(text(""))
            },
            self.toast_view(),
            if self.show_single_check_modal {
                self.single_check_modal_view()
            } else {
                container(text("")).into()
            },
            if self.show_single_result_modal {
                self.single_result_modal_view()
            } else {
                container(text("")).into()
            },
            if self.show_clear_confirm {
                self.clear_confirm_view()
            } else {
                container(text("")).into()
            },
        ]
        .spacing(10)
        .width(Length::Fill)
        .height(Length::Fill);

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::Container::Custom(Box::new(AppBackground)))
            .into()
    }

    fn theme(&self) -> Self::Theme {
        Theme::custom(
            "ipcheck-theme".to_string(),
            Palette {
                background: iced::Color::from_rgb8(245, 248, 255),
                text: iced::Color::from_rgb8(35, 35, 45),
                primary: iced::Color::from_rgb8(59, 130, 246),
                success: iced::Color::from_rgb8(22, 163, 74),
                danger: iced::Color::from_rgb8(220, 38, 38),
            },
        )
    }
}

impl IpCheckApp {
    fn latest_result_for(&self, proxy_id: i64) -> Option<&CheckResult> {
        self.results.iter().find(|item| item.proxy_id == proxy_id)
    }

    fn show_toast(&mut self, msg: &str) -> Command<Message> {
        self.toast_seq += 1;
        let id = self.toast_seq;
        self.toast = Some(Toast {
            id,
            text: msg.to_string(),
        });
        Command::perform(
            async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                id
            },
            Message::ToastExpired,
        )
    }

    fn toast_view(&self) -> Element<'_, Message> {
        match &self.toast {
            Some(toast) => row![
                container(text(&toast.text).size(14))
                    .padding([8, 12])
                    .style(theme::Container::Box),
                horizontal_space(),
            ]
            .padding([0, 12, 8, 12])
            .into(),
            None => container(text("")).into(),
        }
    }

    fn single_result_modal_view(&self) -> Element<'_, Message> {
        let detail = if let Some(item) = &self.draft_single_result {
            column![
                text(format!("真实IP: {}", item.real_ip)),
                text(format!(
                    "归属地: {}{}{}",
                    item.base.country, item.base.province, item.base.city
                )),
                text(format!("运营商: {}", item.base.isp)),
                text(format!("应用场景: {}", item.base.scene)),
                text(format!("行为风险: {}", join_or_none(&item.overall.behavior_risks))),
                text(format!("关联设备风险: {}", join_or_none(&item.overall.device_risks))),
                text(format!("恶意事件风险: {}", join_or_none(&item.overall.malware_risks))),
                text(format!("其他标签: {}", join_or_none(&item.overall.other_tags))),
            ]
            .spacing(8)
        } else {
            column![text("暂无检测结果")]
        };

        let modal = container(
            column![
                text("单个检测结果").size(24),
                detail,
                row![
                    button("关闭").on_press(Message::CloseSingleResultModal),
                    button("保存").on_press(Message::SaveSingleResult),
                ]
                .spacing(10)
                .align_items(iced::Alignment::Center),
            ]
            .spacing(12)
            .align_items(iced::Alignment::Center),
        )
        .padding(20)
        .style(theme::Container::Box)
        .width(Length::Fixed(520.0));

        container(column![modal].height(Length::Fill).align_items(iced::Alignment::Center))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::Container::Custom(Box::new(AppBackground)))
            .center_x()
            .center_y()
            .into()
    }

    fn single_check_modal_view(&self) -> Element<'_, Message> {
        let modal = container(
            column![
                text("风控检查单个节点").size(24),
                text("输入 1 条节点信息").size(14),
                text("格式提示（任选一种）：socks5://user:pass@host:port ｜ host|port|user|pass ｜ socks5://host:port---user---pass ｜ ip:port user pass")
                    .size(12)
                    .style(theme::Text::Color(iced::Color::from_rgb8(100, 110, 130))),
                text_editor(&self.single_check_content)
                    .on_action(Message::SingleCheckContentAction)
                    .height(Length::Fixed(180.0)),
                row![
                    button("取消").on_press(Message::CloseSingleCheckModal),
                    button("开始检测").on_press(Message::StartSingleCheck),
                ]
                .spacing(10)
                .align_items(iced::Alignment::Center),
            ]
            .spacing(12)
            .align_items(iced::Alignment::Center),
        )
        .padding(20)
        .style(theme::Container::Box)
        .width(Length::Fixed(520.0));

        container(column![modal].height(Length::Fill).align_items(iced::Alignment::Center))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::Container::Custom(Box::new(AppBackground)))
            .center_x()
            .center_y()
            .into()
    }

    fn clear_confirm_view(&self) -> Element<'_, Message> {
        let modal = container(
            column![
                text("确认清空列表吗？").size(22),
                text("该操作会删除本地保存的代理记录。"),
                row![
                    button("取消").on_press(Message::CancelClearList),
                    button("确认清空").on_press(Message::ConfirmClearList),
                ]
                .spacing(10)
                .align_items(iced::Alignment::Center),
            ]
            .spacing(12)
            .align_items(iced::Alignment::Center),
        )
        .padding(20)
        .style(theme::Container::Box)
        .width(Length::Fixed(440.0));

        container(column![modal].height(Length::Fill).align_items(iced::Alignment::Center))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::Container::Custom(Box::new(AppBackground)))
            .center_x()
            .center_y()
            .into()
    }

}

fn table_cell(label: String, portion: u16) -> Element<'static, Message> {
    let copy = label.clone();
    container(
        mouse_area(
            container(text(label).size(14))
                .width(Length::Fill)
                .padding(6),
        )
        .on_press(Message::CopyCellToClipboard(copy)),
    )
    .width(Length::FillPortion(portion))
    .padding(6)
    .into()
}

/// IP/真实 IP 列展示：有风险标红；已出风控且均无异常标绿；伪 IP 用灰（红优先）。
#[derive(Clone, Copy)]
enum IpCellDisplay {
    RiskRed,
    CleanGreen,
    PseudoGray,
    Default,
}

fn ip_cell_display_for_host(result: Option<&CheckResult>, ip_risk: bool) -> IpCellDisplay {
    if ip_risk {
        IpCellDisplay::RiskRed
    } else if result.is_some() && !result_indicates_ip_risk(result) {
        IpCellDisplay::CleanGreen
    } else {
        IpCellDisplay::Default
    }
}

fn ip_cell_display_for_real(
    result: Option<&CheckResult>,
    ip_risk: bool,
    pseudo_gray: bool,
) -> IpCellDisplay {
    if ip_risk {
        IpCellDisplay::RiskRed
    } else if pseudo_gray {
        IpCellDisplay::PseudoGray
    } else if result.is_some() && !result_indicates_ip_risk(result) {
        IpCellDisplay::CleanGreen
    } else {
        IpCellDisplay::Default
    }
}

fn table_cell_ip_colored(
    label: String,
    portion: u16,
    display: IpCellDisplay,
) -> Element<'static, Message> {
    let copy = label.clone();
    let t = text(label).size(14);
    let t = match display {
        IpCellDisplay::RiskRed => t.style(theme::Text::Color(Color::from_rgb8(198, 40, 40))),
        IpCellDisplay::CleanGreen => t.style(theme::Text::Color(Color::from_rgb8(22, 163, 74))),
        IpCellDisplay::PseudoGray => t.style(theme::Text::Color(Color::from_rgb8(130, 130, 135))),
        IpCellDisplay::Default => t,
    };
    container(
        mouse_area(
            container(t)
                .width(Length::Fill)
                .padding(6),
        )
        .on_press(Message::CopyCellToClipboard(copy)),
    )
    .width(Length::FillPortion(portion))
    .padding(6)
    .into()
}

fn table_cell_real_ip_history(
    label: String,
    portion: u16,
    display: IpCellDisplay,
    toggle_msg: Message,
    expanded: bool,
    loading: bool,
    history_opt: Option<&Vec<RealIpHistoryEntry>>,
) -> Element<'static, Message> {
    let label_trim = label.trim().to_string();
    let t = text(label.clone()).size(14);
    let t = match display {
        IpCellDisplay::RiskRed => t.style(theme::Text::Color(Color::from_rgb8(198, 40, 40))),
        IpCellDisplay::CleanGreen => t.style(theme::Text::Color(Color::from_rgb8(22, 163, 74))),
        IpCellDisplay::PseudoGray => t.style(theme::Text::Color(Color::from_rgb8(130, 130, 135))),
        IpCellDisplay::Default => t,
    };

    let top = row![
        mouse_area(
            container(t)
                .width(Length::Fill)
                .padding(6),
        )
        .on_press(Message::CopyCellToClipboard(label.clone())),
        button(if expanded { "收起" } else { "历史" })
            .style(theme::Button::Secondary)
            .padding([2, 6])
            .on_press(toggle_msg),
    ]
    .spacing(6)
    .align_items(iced::Alignment::Center);

    let history_box: Element<'static, Message> = if expanded {
        if loading {
            container(text("加载中...").size(12))
                .padding([0, 6, 0, 6])
                .style(theme::Container::Box)
                .into()
        } else if let Some(hist) = history_opt {
            if hist.is_empty() {
                container(text("暂无历史记录").size(12))
                    .padding([0, 6, 0, 6])
                    .style(theme::Container::Box)
                    .into()
            } else {
                let max_lines = 8usize;
                let mut lines: Vec<Element<'static, Message>> = Vec::new();
                for entry in hist.iter().take(max_lines) {
                    let is_current = entry.real_ip.trim() == label_trim;
                    let line_t = text(format!(
                        "{}  {}",
                        entry.real_ip.trim(),
                        entry.observed_at.trim()
                    ))
                    .size(12);
                    let line_t = if is_current {
                        line_t.style(theme::Text::Color(Color::from_rgb8(59, 130, 246)))
                    } else {
                        line_t
                    };
                    let entry_copy = entry.real_ip.clone();
                    let line_el = container(mouse_area(container(line_t).padding(4)).on_press(
                        Message::CopyCellToClipboard(entry_copy),
                    ));
                    lines.push(line_el.into());
                }
                if hist.len() > max_lines {
                    lines.push(
                        container(text("...").size(12))
                            .padding([4, 6])
                            .into(),
                    );
                }

                container(column(lines).spacing(4).align_items(iced::Alignment::Start))
                    .padding([0, 6, 0, 6])
                    .style(theme::Container::Box)
                    .into()
            }
        } else {
            container(text("暂无历史记录").size(12))
                .padding([0, 6, 0, 6])
                .style(theme::Container::Box)
                .into()
        }
    } else {
        container(text("")).height(Length::Shrink).into()
    };

    container(column![top, history_box].spacing(4).align_items(iced::Alignment::Center))
        .width(Length::FillPortion(portion))
        .padding(6)
        .into()
}

/// 普通单元格；`row_green` 为真时表示该行风控已检且无异常，用绿色字。
fn table_cell_data(label: String, portion: u16, row_green: bool) -> Element<'static, Message> {
    let copy = label.clone();
    let t = text(label).size(14);
    let t = if row_green {
        t.style(theme::Text::Color(Color::from_rgb8(22, 163, 74)))
    } else {
        t
    };
    container(
        mouse_area(
            container(t)
                .width(Length::Fill)
                .padding(6),
        )
        .on_press(Message::CopyCellToClipboard(copy)),
    )
    .width(Length::FillPortion(portion))
    .padding(6)
    .into()
}

fn compact_risk_layout(width: f32, height: f32) -> bool {
    width <= COMPACT_VIEWPORT_MAX_W && height <= COMPACT_VIEWPORT_MAX_H
}

/// 紧凑布局下四类风险纵向堆叠（默认「展开」在同一单元格内）。
fn table_cell_risk_stack_compact(
    behavior: String,
    device: String,
    malware: String,
    tags: String,
    row_green: bool,
) -> Element<'static, Message> {
    let block = format!(
        "行为风险：{behavior}\n关联设备风险：{device}\n恶意事件风险：{malware}\n其他标签：{tags}"
    );
    let copy = block.clone();
    let line = |s: String| {
        let t = text(s).size(12);
        if row_green {
            t.style(theme::Text::Color(Color::from_rgb8(22, 163, 74)))
        } else {
            t
        }
    };
    container(
        mouse_area(
            column![
                line(format!("行为风险：{behavior}")),
                line(format!("关联设备风险：{device}")),
                line(format!("恶意事件风险：{malware}")),
                line(format!("其他标签：{tags}")),
            ]
            .spacing(4)
            .align_items(iced::Alignment::Start),
        )
        .on_press(Message::CopyCellToClipboard(copy)),
    )
    .width(Length::FillPortion(RISK_STACK_PORTION))
    .padding(6)
    .into()
}

fn table_cell_index_header() -> Element<'static, Message> {
    table_cell_fixed_clickable("序号".to_string(), TABLE_COL_INDEX_WIDTH)
}

fn table_cell_index_value(label: String) -> Element<'static, Message> {
    table_cell_fixed_clickable(label, TABLE_COL_INDEX_WIDTH)
}

fn table_cell_ops_header() -> Element<'static, Message> {
    let copy = "操作".to_string();
    container(
        mouse_area(
            container(text("操作").size(14))
                .width(Length::Fill)
                .padding(6),
        )
        .on_press(Message::CopyCellToClipboard(copy)),
    )
    .width(Length::Fixed(TABLE_COL_OPS_WIDTH))
    .padding(6)
    .into()
}

fn table_cell_fixed_clickable(label: String, width: f32) -> Element<'static, Message> {
    let copy = label.clone();
    container(
        mouse_area(
            container(text(label).size(14))
                .width(Length::Fill)
                .padding(4),
        )
        .on_press(Message::CopyCellToClipboard(copy)),
    )
    .width(Length::Fixed(width))
    .padding(4)
    .into()
}

fn join_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "无".to_string()
    } else {
        items.join(",")
    }
}

fn result_indicates_ip_risk(result: Option<&CheckResult>) -> bool {
    let Some(r) = result else {
        return false;
    };
    let o = &r.overall;
    join_or_none(&o.behavior_risks) != "无"
        || join_or_none(&o.device_risks) != "无"
        || join_or_none(&o.malware_risks) != "无"
        || join_or_none(&o.other_tags) != "无"
}

fn icon_import() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/icons/import.svg"
    )))
}

fn icon_search() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/icons/search.svg"
    )))
}

fn icon_check() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/icons/check.svg"
    )))
}

fn icon_trash() -> svg::Handle {
    svg::Handle::from_memory(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/icons/trash.svg"
    )))
}

fn window_event_to_viewport_message(
    event: iced::Event,
    status: iced::event::Status,
) -> Option<Message> {
    match event {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            let ctrl = modifiers.contains(iced::keyboard::Modifiers::CTRL);
            let key_is_a = match key {
                iced::keyboard::Key::Character(c) => c.eq_ignore_ascii_case("a"),
                _ => false,
            };

            // 即使事件已被控件捕获，也希望能修复 iced 0.12 的 `Ctrl+A` 全选失效。
            if ctrl && key_is_a {
                return Some(Message::SingleCheckCtrlASelectAll);
            }
            if matches!(status, iced::event::Status::Captured) {
                None
            } else {
                None
            }
        }
        iced::Event::Window(_, iced::window::Event::Opened { size, .. }) => {
            Some(Message::WindowOpened {
                width: size.width as f32,
                height: size.height as f32,
            })
        }
        iced::Event::Window(_, iced::window::Event::Resized { width, height }) => {
            Some(Message::WindowViewport {
                width: width as f32,
                height: height as f32,
            })
        }
        _ => None,
    }
}

fn build_service() -> Option<IpCheckService<SqliteRepository>> {
    let path = match resolve_db_path() {
        Ok(path) => path,
        Err(_) => std::path::PathBuf::from("ipcheck.db"),
    };
    let repo = SqliteRepository::new(path).ok()?;
    if repo.init().is_err() {
        return None;
    }
    Some(IpCheckService::new(repo))
}

/// 部分 zenity 不支持 `--center`（exit 255 + stderr “not available”），检测到则去掉该参数再开一次。
fn zenity_stderr_suggests_unsupported_option(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("not available") || s.contains("unrecognized option")
}

fn run_zenity_import_text_info(path_str: &str, use_center: bool) -> Result<std::process::Output, String> {
    let text_label = "格式说明已预填在编辑区；可追加或粘贴代理行，点「确定」导入。";
    let mut cmd = ProcCommand::new("zenity");
    cmd.arg("--text-info").arg("--editable");
    if use_center {
        cmd.arg("--center");
    }
    cmd.arg("--title=导入IP")
        .arg(format!("--text={text_label}"))
        .arg("--filename")
        .arg(path_str)
        .arg("--width=900")
        .arg("--height=560")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = cmd.spawn().map_err(|e| e.to_string())?;
    child.wait_with_output().map_err(|e| e.to_string())
}

fn parse_zenity_import_output(output: std::process::Output) -> Result<String, String> {
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            return Err("__CANCELLED__".to_string());
        }
        return Ok(trimmed);
    }
    let code = output.status.code().unwrap_or(-1);
    if code == 1 {
        return Err("__CANCELLED__".to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Err(format!("zenity exit code {code}: {stderr}"))
}

fn open_import_window() -> Result<String, String> {
    use std::time::{SystemTime, UNIX_EPOCH};

    // `zenity --text-info --filename=/dev/stdin` 若未向子进程 stdin 写入，编辑区会一直空白。
    // 将格式说明写入临时文件并作为 --filename 传入，编辑区即可看到预填提示。
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ipcheck_zenity_import_{unique}.txt"));
    let initial = format!(
        "{IMPORT_DIALOG_HINT}\n\n# 在下方追加代理行即可；说明可删可留（非代理行会自动忽略）。\n\n"
    );
    std::fs::write(&path, initial).map_err(|e| e.to_string())?;

    let path_str = path.to_str().ok_or_else(|| "临时路径无效".to_string())?.to_string();

    let output = match run_zenity_import_text_info(&path_str, true) {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
    };

    let output = if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if code == 255 && zenity_stderr_suggests_unsupported_option(&stderr) {
            match run_zenity_import_text_info(&path_str, false) {
                Ok(o) => o,
                Err(e) => {
                    let _ = std::fs::remove_file(&path);
                    return Err(e);
                }
            }
        } else {
            let _ = std::fs::remove_file(&path);
            return parse_zenity_import_output(output);
        }
    } else {
        output
    };

    let _ = std::fs::remove_file(&path);
    parse_zenity_import_output(output)
}

/// 表格数据行斑马纹与圆角边框。
struct TableRowStripe {
    alt: bool,
}

impl container::StyleSheet for TableRowStripe {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> container::Appearance {
        let bg = if self.alt {
            Color::from_rgb8(248, 250, 253)
        } else {
            Color::from_rgb8(255, 255, 255)
        };
        container::Appearance {
            background: Some(iced::Background::Color(bg)),
            text_color: None,
            border: Border {
                color: Color::from_rgb8(228, 234, 244),
                width: 1.0,
                radius: 8.0.into(),
            },
            shadow: Default::default(),
        }
    }
}

struct AppBackground;

impl container::StyleSheet for AppBackground {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> container::Appearance {
        container::Appearance {
            background: Some(iced::Background::Color(iced::Color::from_rgb8(240, 246, 255))),
            text_color: None,
            border: Default::default(),
            shadow: Default::default(),
        }
    }
}
