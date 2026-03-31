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
        IpCellDisplay::RiskRed => t.color(Color::from_rgb8(198, 40, 40)),
        IpCellDisplay::CleanGreen => t.color(Color::from_rgb8(22, 163, 74)),
        IpCellDisplay::PseudoGray => t.color(Color::from_rgb8(130, 130, 135)),
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
        IpCellDisplay::RiskRed => t.color(Color::from_rgb8(198, 40, 40)),
        IpCellDisplay::CleanGreen => t.color(Color::from_rgb8(22, 163, 74)),
        IpCellDisplay::PseudoGray => t.color(Color::from_rgb8(130, 130, 135)),
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
            .style(button::secondary)
            .padding([2, 6])
            .on_press(toggle_msg),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);

    let history_box: Element<'static, Message> = if expanded {
        if loading {
            container(text("加载中...").size(12))
                .padding([0, 6])
                .style(container::rounded_box)
                .into()
        } else if let Some(hist) = history_opt {
            if hist.is_empty() {
                container(text("暂无历史记录").size(12))
                    .padding([0, 6])
                    .style(container::rounded_box)
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
                        line_t.color(Color::from_rgb8(59, 130, 246))
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

                container(column(lines).spacing(4).align_x(iced::Alignment::Start))
                    .padding([0, 6])
                    .style(container::rounded_box)
                    .into()
            }
        } else {
            container(text("暂无历史记录").size(12))
                .padding([0, 6])
                .style(container::rounded_box)
                .into()
        }
    } else {
        container(text("")).height(Length::Shrink).into()
    };

    container(iced::widget::column![top, history_box].spacing(4).align_x(iced::Alignment::Center))
        .width(Length::FillPortion(portion))
        .padding(6)
        .into()
}

/// 普通单元格；`row_green` 为真时表示该行风控已检且无异常，用绿色字。
fn table_cell_data(label: String, portion: u16, row_green: bool) -> Element<'static, Message> {
    let copy = label.clone();
    let t = text(label).size(14);
    let t = if row_green {
        t.color(Color::from_rgb8(22, 163, 74))
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
            t.color(Color::from_rgb8(22, 163, 74))
        } else {
            t
        }
    };
    container(
        mouse_area(
            iced::widget::column![
                line(format!("行为风险：{behavior}")),
                line(format!("关联设备风险：{device}")),
                line(format!("恶意事件风险：{malware}")),
                line(format!("其他标签：{tags}")),
            ]
            .spacing(4)
            .align_x(iced::Alignment::Start),
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
    _window_id: iced::window::Id,
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
        iced::Event::Window(iced::window::Event::Opened { size, .. }) => {
            Some(Message::WindowOpened {
                width: size.width as f32,
                height: size.height as f32,
            })
        }
        iced::Event::Window(iced::window::Event::Resized(size)) => {
            Some(Message::WindowViewport {
                width: size.width as f32,
                height: size.height as f32,
            })
        }
        _ => None,
    }
}

fn window_event_to_viewport_message_with_import_drag(
    event: iced::Event,
    status: iced::event::Status,
    window_id: iced::window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
            Some(Message::ImportTitleMoved(position))
        }
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
            Some(Message::ImportTitleReleased)
        }
        _ => window_event_to_viewport_message(event, status, window_id),
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

fn table_row_style(alt: bool) -> container::Style {
    let bg = if alt {
        Color::from_rgb8(248, 250, 253)
    } else {
        Color::from_rgb8(255, 255, 255)
    };
    container::Style {
        background: Some(iced::Background::Color(bg)),
        text_color: None,
        border: Border {
            color: Color::from_rgb8(228, 234, 244),
            width: 1.0,
            radius: 8.0.into(),
        },
        shadow: Default::default(),
        snap: false,
    }
}

fn app_background_style() -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgb8(240, 246, 255))),
        text_color: None,
        border: Default::default(),
        shadow: Default::default(),
        snap: false,
    }
}

fn modal_backdrop_style() -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.28,
        })),
        text_color: None,
        border: Default::default(),
        shadow: Default::default(),
        snap: false,
    }
}

fn import_modal_dimensions(view_w: f32, view_h: f32) -> (f32, f32) {
    const IMPORT_MODAL_WIDTH: f32 = 920.0;
    const IMPORT_MODAL_HEIGHT: f32 = 540.0;
    const IMPORT_MODAL_MIN_W: f32 = 540.0;
    const IMPORT_MODAL_MIN_H: f32 = 320.0;
    const IMPORT_MODAL_MARGIN: f32 = 24.0;

    let modal_width = IMPORT_MODAL_WIDTH.min((view_w - IMPORT_MODAL_MARGIN * 2.0).max(IMPORT_MODAL_MIN_W));
    let modal_height =
        IMPORT_MODAL_HEIGHT.min((view_h - IMPORT_MODAL_MARGIN * 2.0).max(IMPORT_MODAL_MIN_H));
    (modal_width, modal_height)
}

fn import_title_bar_style(dragging: bool, hovered: bool) -> container::Style {
    let bg = if dragging {
        Color::from_rgb8(225, 238, 255)
    } else if hovered {
        Color::from_rgb8(235, 243, 255)
    } else {
        Color::from_rgb8(246, 249, 255)
    };
    container::Style {
        background: Some(iced::Background::Color(bg)),
        text_color: None,
        border: Border {
            color: if dragging {
                Color::from_rgb8(96, 165, 250)
            } else {
                Color::from_rgb8(220, 230, 245)
            },
            width: if dragging { 1.4 } else { 1.0 },
            radius: 10.0.into(),
        },
        shadow: Default::default(),
        snap: false,
    }
}

fn import_modal_card_style(dragging: bool) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(Color::from_rgb8(255, 255, 255))),
        text_color: None,
        border: Border {
            color: if dragging {
                Color::from_rgb8(147, 197, 253)
            } else {
                Color::from_rgb8(220, 230, 245)
            },
            width: if dragging { 1.4 } else { 1.0 },
            radius: 14.0.into(),
        },
        shadow: iced::Shadow {
            color: Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: if dragging { 0.20 } else { 0.14 },
            },
            offset: iced::Vector::new(0.0, if dragging { 8.0 } else { 4.0 }),
            blur_radius: if dragging { 28.0 } else { 18.0 },
        },
        snap: false,
    }
}
