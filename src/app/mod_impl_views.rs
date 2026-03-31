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
                    .style(container::rounded_box),
                space::horizontal(),
            ]
            .padding([8, 12])
            .into(),
            None => container(text("")).into(),
        }
    }

    fn single_result_modal_view(&self) -> Element<'_, Message> {
        let detail = if let Some(item) = &self.draft_single_result {
            iced::widget::column![
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
            iced::widget::column![text("暂无检测结果")]
        };

        let modal = container(
            iced::widget::column![
                text("单个检测结果").size(24),
                detail,
                row![
                    button("关闭").on_press(Message::CloseSingleResultModal),
                    button("保存").on_press(Message::SaveSingleResult),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(12)
            .align_x(iced::Alignment::Center),
        )
        .padding(20)
        .style(container::rounded_box)
        .width(Length::Fixed(520.0));

        container(iced::widget::column![modal].height(Length::Fill).align_x(iced::Alignment::Center))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    fn import_modal_view(&self) -> Element<'_, Message> {
        let (modal_width, modal_height) = import_modal_dimensions(
            self.window_viewport_width,
            self.window_viewport_height,
        );

        let max_left = (self.window_viewport_width - modal_width).max(0.0);
        let max_top = (self.window_viewport_height - modal_height).max(0.0);
        let left = (max_left * 0.5 + self.import_modal_offset_x).clamp(0.0, max_left);
        let top = (max_top * 0.5 + self.import_modal_offset_y).clamp(0.0, max_top);

        let top_padding_drag_area = mouse_area(
            container(space::vertical().height(Length::Fixed(0.0)))
                .width(Length::Fill)
                .height(Length::Fixed(0.0)),
        )
        .on_enter(Message::ImportTitleEntered)
        .on_exit(Message::ImportTitleExited)
        .on_press(Message::ImportTitlePressed);

        let title_drag_area = mouse_area(container(iced::widget::column![
                row![
                    text("导入IP").size(24),
                    if self.import_modal_dragging {
                        text("拖动中").size(12).color(Color::from_rgb8(37, 99, 235))
                    } else {
                        text("可拖动").size(12).color(Color::from_rgb8(100, 110, 130))
                    },
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
                text("支持多行粘贴，点击“确定”后会自动解析可识别格式")
                    .size(13)
                    .color(Color::from_rgb8(100, 110, 130)),
            ]
            .spacing(4))
            .width(Length::Fill)
            .padding([12, 20])
            .style(move |_theme| {
                import_title_bar_style(self.import_modal_dragging, self.import_modal_title_hovered)
            }))
        .on_enter(Message::ImportTitleEntered)
        .on_exit(Message::ImportTitleExited)
        .on_press(Message::ImportTitlePressed);

        let modal = container(
            iced::widget::column![
                top_padding_drag_area,
                title_drag_area,
                container(
                    text(IMPORT_DIALOG_HINT)
                        .size(13)
                        .color(Color::from_rgb8(95, 105, 125)),
                )
                .width(Length::Fill)
                .padding([10, 12])
                .style(container::rounded_box),
                container(
                    text_editor(&self.import_content)
                        .on_action(Message::ImportContentAction)
                        .height(Length::Fill),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(6)
                .style(container::rounded_box),
                row![
                    button("从剪贴板导入").on_press(Message::StartImportFromClipboard),
                    button("取消").on_press(Message::CloseImportModal),
                    button("确定").style(button::primary).on_press(Message::StartImport),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(12)
            .align_x(iced::Alignment::Start),
        )
        .padding(20)
        .style(move |_theme| import_modal_card_style(self.import_modal_dragging))
        .width(Length::Fixed(modal_width))
        .height(Length::Fixed(modal_height));

        // 背景遮罩通过 mouse_area 吞掉点击，避免点击穿透到底层按钮导致重复 OpenImportModal。
        mouse_area(
            container(iced::widget::column![
                space::vertical().height(Length::Fixed(top)),
                row![
                    space::horizontal().width(Length::Fixed(left)),
                    modal,
                    space::horizontal(),
                ]
                .width(Length::Fill)
                .align_y(iced::Alignment::Start),
                space::vertical(),
            ])
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| modal_backdrop_style())
        )
            .on_press(Message::Noop)
            .into()
    }

    fn single_check_modal_view(&self) -> Element<'_, Message> {
        let modal = container(
            iced::widget::column![
                text("风控检查单个节点").size(24),
                text("输入 1 条节点信息").size(14),
                text("格式提示（任选一种）：socks5://user:pass@host:port ｜ host|port|user|pass ｜ socks5://host:port---user---pass ｜ ip:port user pass")
                    .size(12)
                    .color(iced::Color::from_rgb8(100, 110, 130)),
                text_editor(&self.single_check_content)
                    .on_action(Message::SingleCheckContentAction)
                    .height(Length::Fixed(180.0)),
                row![
                    button("取消").on_press(Message::CloseSingleCheckModal),
                    button("开始检测").on_press(Message::StartSingleCheck),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(12)
            .align_x(iced::Alignment::Center),
        )
        .padding(20)
        .style(container::rounded_box)
        .width(Length::Fixed(520.0));

        container(iced::widget::column![modal].height(Length::Fill).align_x(iced::Alignment::Center))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| app_background_style())
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

    fn clear_confirm_view(&self) -> Element<'_, Message> {
        let modal = container(
            iced::widget::column![
                text("确认清空列表吗？").size(22),
                text("该操作会删除本地保存的代理记录。"),
                row![
                    button("取消").on_press(Message::CancelClearList),
                    button("确认清空").on_press(Message::ConfirmClearList),
                ]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(12)
            .align_x(iced::Alignment::Center),
        )
        .padding(20)
        .style(container::rounded_box)
        .width(Length::Fixed(440.0));

        container(iced::widget::column![modal].height(Length::Fill).align_x(iced::Alignment::Center))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| app_background_style())
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }

}
