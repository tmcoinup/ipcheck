use iced::clipboard;
use iced::theme::Palette;
use iced::widget::{
    button, column, container, mouse_area, row, scrollable, space, stack, svg, text, text_editor,
};
use iced::widget::scrollable::AbsoluteOffset;
use iced::widget::operation;
use iced::{Border, Color, Element, Length, Subscription, Task, Theme};
use std::process::{Command as ProcCommand, Stdio};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

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

fn table_h_scroll_id_header() -> iced::widget::Id {
    iced::widget::Id::new("ipcheck_tbl_h_hdr")
}

fn table_h_scroll_id_row(idx: usize) -> iced::widget::Id {
    iced::widget::Id::from(format!("ipcheck_tbl_h_{idx}"))
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
    show_import_modal: bool,
    import_modal_offset_x: f32,
    import_modal_offset_y: f32,
    import_modal_dragging: bool,
    import_modal_title_hovered: bool,
    import_modal_drag_last: Option<iced::Point>,
    import_raw_input: String,
    import_content: text_editor::Content,
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
    /// 应用启动后加载本地快照完成（含 token、代理列表、风控结果）。
    Loaded(Result<LoadedData, String>),
    /// 打开导入代理弹窗。
    OpenImportModal,
    /// 关闭导入代理弹窗。
    CloseImportModal,
    /// 导入编辑器内容变更。
    ImportContentAction(text_editor::Action),
    /// 从系统剪贴板读取导入文本。
    StartImportFromClipboard,
    /// 剪贴板读取回调（None 表示读取失败或无内容）。
    ImportClipboardLoaded(Option<String>),
    /// 开始解析并导入代理文本。
    StartImport,
    /// 鼠标进入导入弹窗标题栏（用于拖拽态样式）。
    ImportTitleEntered,
    /// 鼠标离开导入弹窗标题栏。
    ImportTitleExited,
    /// 按下导入弹窗标题栏，开始拖动。
    ImportTitlePressed,
    /// 释放导入弹窗标题栏，结束拖动。
    ImportTitleReleased,
    /// 拖动导入弹窗时，更新当前鼠标位置。
    ImportTitleMoved(iced::Point),
    /// 打开单条检测弹窗。
    OpenSingleCheckModal,
    /// 关闭单条检测弹窗。
    CloseSingleCheckModal,
    /// 单条检测输入编辑器内容变更。
    SingleCheckContentAction(text_editor::Action),
    /// 兜底：`iced 0.12` 下 `text_editor` 可能无法正确绑定 `Ctrl+A`，
    /// 这里在应用层捕获键盘事件后强制全选。
    SingleCheckCtrlASelectAll,
    /// 导入任务完成（返回最新代理列表）。
    Imported(Result<Vec<ProxyEntry>, String>),
    /// 批量查询所有代理真实 IP。
    QueryAllRealIp,
    /// 批量真实 IP 查询完成。
    RealIpDone(Result<Vec<(i64, String, String)>, String>),
    /// 查询单个代理真实 IP。
    QueryOneRealIp(i64),
    /// 单条真实 IP 查询完成（错误分支附带代理 id）。
    QueryOneRealIpDone(Result<(i64, String, String), (i64, String)>),
    /// 展开/收起某条代理的真实 IP 历史。
    ToggleRealIpHistory(i64),
    /// 真实 IP 历史加载完成。
    RealIpHistoryDone(Result<(i64, Vec<RealIpHistoryEntry>), (i64, String)>),
    /// 对单个代理执行风控检测。
    RiskCheckOne(i64),
    /// 单条风控检测完成。
    RiskCheckOneDone(Result<CheckResult, String>),
    /// 批量执行风控检测。
    CheckAll,
    /// 批量风控检测完成。
    CheckedAll(Result<CheckProxyBatchOutcome, String>),
    /// 启动单条即时检测（弹窗输入）。
    StartSingleCheck,
    /// 单条即时检测完成。
    StartSingleCheckDone(Result<CheckResult, String>),
    /// 将单条检测结果持久化到数据库。
    SaveSingleResult,
    /// 单条检测结果保存完成。
    SaveSingleResultDone(Result<(), String>),
    /// 关闭单条检测结果弹窗。
    CloseSingleResultModal,
    /// 删除一条代理记录。
    DeleteOne(i64),
    /// 删除代理完成（返回已删除 id）。
    DeletedOne(Result<i64, String>),
    /// 打开“清空列表”确认弹窗。
    AskClearList,
    /// 取消清空列表。
    CancelClearList,
    /// 确认清空列表并执行。
    ConfirmClearList,
    /// 清空列表完成。
    ClearedList(Result<(), String>),
    /// Toast 自动过期。
    ToastExpired(u64),
    /// 主窗口标题栏点击事件（用于双击行为）。
    TitleBarPressed,
    /// 主窗口首次创建完成（更新视口；无数据库时在此完成一次最大化）。
    WindowOpened { width: f32, height: f32 },
    /// 主窗口尺寸变化，刷新视口宽高。
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
    Command::none()
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

type Command<Message> = Task<Message>;

