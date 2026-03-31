impl IpCheckApp {
    pub fn init() -> (Self, Command<Message>) {
        let service = build_service();
        let config = load_or_init_config().unwrap_or_default();
        let mut app = Self {
            service,
            api_token: config.api_token,
            show_import_modal: false,
            import_modal_offset_x: 0.0,
            import_modal_offset_y: 0.0,
            import_modal_dragging: false,
            import_modal_title_hovered: false,
            import_modal_drag_last: None,
            import_raw_input: String::new(),
            import_content: text_editor::Content::new(),
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

    pub fn title(&self) -> String {
        "IP质量检测工具".to_string()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        if self.show_import_modal {
            iced::event::listen_with(window_event_to_viewport_message_with_import_drag)
        } else {
            iced::event::listen_with(window_event_to_viewport_message)
        }
    }

    pub fn update(&mut self, message: Message) -> Command<Message> {
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
                            cmds.push(operation::scroll_to(
                                table_h_scroll_id_row(idx),
                                operation::AbsoluteOffset {
                                    x: Some(off.x),
                                    y: Some(off.y),
                                },
                            ));
                        }
                    }
                    TableHScrollSource::Row(i) => {
                        cmds.push(operation::scroll_to(
                            table_h_scroll_id_header(),
                            operation::AbsoluteOffset {
                                x: Some(off.x),
                                y: Some(off.y),
                            },
                        ));
                        for j in 0..self.proxies.len() {
                            if j != i {
                                cmds.push(operation::scroll_to(
                                    table_h_scroll_id_row(j),
                                    operation::AbsoluteOffset {
                                        x: Some(off.x),
                                        y: Some(off.y),
                                    },
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
                if self.show_import_modal {
                    return Command::none();
                }
                self.show_import_modal = true;
                self.import_modal_offset_x = 0.0;
                self.import_modal_offset_y = 0.0;
                self.import_modal_dragging = false;
                self.import_modal_title_hovered = false;
                self.import_modal_drag_last = None;
                self.import_raw_input.clear();
                self.import_content = text_editor::Content::new();
                Command::none()
            }
            Message::CloseImportModal => {
                self.show_import_modal = false;
                self.import_modal_dragging = false;
                self.import_modal_title_hovered = false;
                self.import_modal_drag_last = None;
                Command::none()
            }
            Message::ImportContentAction(action) => {
                self.import_content.perform(action);
                self.import_raw_input = self.import_content.text();
                Command::none()
            }
            Message::StartImportFromClipboard => {
                clipboard::read().map(Message::ImportClipboardLoaded)
            }
            Message::ImportClipboardLoaded(content) => {
                let text = content.unwrap_or_default();
                if text.trim().is_empty() {
                    return self.show_toast("剪贴板为空，无法导入");
                }
                self.import_raw_input = text;
                self.import_content = text_editor::Content::with_text(&self.import_raw_input);
                self.show_toast("已从剪贴板读取，可继续编辑后点击“确定”")
            }
            Message::StartImport => {
                let Some(service) = self.service.clone() else {
                    return self.show_toast("服务不可用");
                };
                let text = self.import_content.text();
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
            Message::ImportTitleEntered => {
                self.import_modal_title_hovered = true;
                Command::none()
            }
            Message::ImportTitleExited => {
                self.import_modal_title_hovered = false;
                Command::none()
            }
            Message::ImportTitlePressed => {
                self.import_modal_dragging = true;
                self.import_modal_drag_last = None;
                Command::none()
            }
            Message::ImportTitleReleased => {
                self.import_modal_dragging = false;
                self.import_modal_drag_last = None;
                Command::none()
            }
            Message::ImportTitleMoved(pos) => {
                if !self.show_import_modal || !self.import_modal_dragging {
                    return Command::none();
                }
                if let Some(last) = self.import_modal_drag_last {
                    let dx = pos.x - last.x;
                    let dy = pos.y - last.y;
                    if dx.abs() < 0.1 && dy.abs() < 0.1 {
                        return Command::none();
                    }
                    let (modal_width, modal_height) = import_modal_dimensions(
                        self.window_viewport_width,
                        self.window_viewport_height,
                    );
                    let max_left = (self.window_viewport_width - modal_width).max(0.0);
                    let max_top = (self.window_viewport_height - modal_height).max(0.0);
                    let half_x = max_left * 0.5;
                    let half_y = max_top * 0.5;

                    // 边缘阻尼：靠近边界时拖动阻力变大，避免“生硬撞墙”。
                    let next_x = self.import_modal_offset_x + dx;
                    let next_y = self.import_modal_offset_y + dy;
                    self.import_modal_offset_x = if next_x < -half_x {
                        -half_x + (next_x + half_x) * 0.35
                    } else if next_x > half_x {
                        half_x + (next_x - half_x) * 0.35
                    } else {
                        next_x
                    };
                    self.import_modal_offset_y = if next_y < -half_y {
                        -half_y + (next_y + half_y) * 0.35
                    } else if next_y > half_y {
                        half_y + (next_y - half_y) * 0.35
                    } else {
                        next_y
                    };
                }
                self.import_modal_drag_last = Some(pos);
                Command::none()
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
                        self.show_import_modal = false;
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
                    Command::none()
                } else {
                    Command::none()
                }
            }
        }
    }
}
