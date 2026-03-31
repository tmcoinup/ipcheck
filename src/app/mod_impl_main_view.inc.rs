impl IpCheckApp {
    pub fn view(&self) -> Element<'_, Message> {
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
                    .align_y(iced::Alignment::Center),
            )
                .style(button::primary)
                .on_press(if self.show_import_modal { Message::Noop } else { Message::OpenImportModal }),
            button(
                row![svg(icon_check()).width(Length::Fixed(12.0)).height(Length::Fixed(12.0)), text("风控检查").size(13)]
                    .spacing(6)
                    .align_y(iced::Alignment::Center),
            )
                .style(button::primary)
                .on_press(Message::CheckAll),
            button(
                row![svg(icon_trash()).width(Length::Fixed(12.0)).height(Length::Fixed(12.0)), text("清空列表").size(13)]
                    .spacing(6)
                    .align_y(iced::Alignment::Center),
            )
                .style(button::danger)
                .on_press(Message::AskClearList),
            button(
                row![svg(icon_search()).width(Length::Fixed(12.0)).height(Length::Fixed(12.0)), text("查询IP").size(13)]
                    .spacing(6)
                    .align_y(iced::Alignment::Center),
            )
                .style(button::secondary)
                .on_press(Message::QueryAllRealIp),
            button(
                row![svg(icon_check()).width(Length::Fixed(12.0)).height(Length::Fixed(12.0)), text("风控检查单个").size(13)]
                    .spacing(6)
                    .align_y(iced::Alignment::Center),
            )
                .style(button::secondary)
                .on_press(Message::OpenSingleCheckModal),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center);
        let compact = compact_risk_layout(self.window_viewport_width, self.window_viewport_height);
        let row_align = if compact {
            iced::Alignment::Start
        } else {
            iced::Alignment::Center
        };

        // 横向条只显示在「最后一行数据」的 Scrollable 底部，避免贴在表头下方像「在顶部」。
        let h_scroll_props_visible = iced::widget::scrollable::Scrollbar::default();
        let h_scroll_props_hidden = iced::widget::scrollable::Scrollbar::new()
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
                .align_y(row_align)
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
                .align_y(row_align)
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
            .align_y(iced::Alignment::Center)
        };

        let busy = self.busy;
        let row_count = self.proxies.len();
        let rows = self
            .proxies
            .iter()
            .enumerate()
            .fold(iced::widget::column![table_header_row].spacing(6), |col, (idx, proxy)| {
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
                        .align_y(iced::Alignment::Center),
                    )
                        .style(button::secondary)
                        .padding([3, 5])
                        .on_press(q_msg),
                    button(
                        row![
                            svg(icon_check()).width(Length::Fixed(11.0)).height(Length::Fixed(11.0)),
                            text("风控").size(11)
                        ]
                        .spacing(3)
                        .align_y(iced::Alignment::Center),
                    )
                        .style(button::primary)
                        .padding([3, 5])
                        .on_press(r_msg),
                    button(
                        row![
                            svg(icon_trash()).width(Length::Fixed(11.0)).height(Length::Fixed(11.0)),
                            text("删除").size(11)
                        ]
                        .spacing(3)
                        .align_y(iced::Alignment::Center),
                    )
                        .style(button::danger)
                        .padding([3, 5])
                        .on_press(d_msg),
                ]
                .spacing(3)
                .align_y(iced::Alignment::Center)
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
                    .align_y(row_align)
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
                    .align_y(row_align)
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
                .align_y(iced::Alignment::Center);

                col.push(
                    container(iced::widget::column![row_pair])
                        .padding([6, 10])
                        .style(move |_theme| table_row_style(idx % 2 == 1)),
                )
            });

        let table_scroll_width =
            (self.window_viewport_width - TABLE_VIEWPORT_WIDTH_TRIM).max(TABLE_SCROLL_MIN_WIDTH);

        let table_scroll = scrollable(
            container(rows).width(Length::Fixed(table_scroll_width)),
        )
        .direction(iced::widget::scrollable::Direction::Vertical(
            iced::widget::scrollable::Scrollbar::default(),
        ))
        .width(Length::Fill)
        .height(Length::Fill);
        let content = iced::widget::column![
            toolbar,
            container(actions).padding([0, 12]),
            container(table_scroll)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding([12, 12]),
            if self.busy {
                container(text("处理中，请稍候...").size(14)).padding([8, 12])
            } else {
                container(text(""))
            },
            self.toast_view(),
        ]
        .spacing(10)
        .width(Length::Fill)
        .height(Length::Fill);

        let base = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| app_background_style());

        let mut layers: Vec<Element<'_, Message>> = vec![base.into()];
        if self.show_import_modal {
            layers.push(self.import_modal_view());
        }
        if self.show_single_check_modal {
            layers.push(self.single_check_modal_view());
        }
        if self.show_single_result_modal {
            layers.push(self.single_result_modal_view());
        }
        if self.show_clear_confirm {
            layers.push(self.clear_confirm_view());
        }

        stack(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    pub fn theme(&self) -> Theme {
        Theme::custom(
            "ipcheck-theme".to_string(),
            Palette {
                background: iced::Color::from_rgb8(245, 248, 255),
                text: iced::Color::from_rgb8(35, 35, 45),
                primary: iced::Color::from_rgb8(59, 130, 246),
                success: iced::Color::from_rgb8(22, 163, 74),
                warning: iced::Color::from_rgb8(245, 158, 11),
                danger: iced::Color::from_rgb8(220, 38, 38),
            },
        )
    }
}
