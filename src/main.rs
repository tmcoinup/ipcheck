mod app;
mod config;
mod domain;
mod repository;
mod service;

use app::IpCheckApp;
use iced::{Application, Font, Settings, window};
use std::process::Command;
use tracing_subscriber::EnvFilter;

fn main() -> iced::Result {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_names(true)
        .init();

    tracing::info!("starting ipcheck desktop app");
    let font_name = pick_font_family();
    tracing::info!(font = font_name, "selected ui font");

    // 初始尺寸仅为创建窗口用；启动完成后最大化在 app 内仅调用一次（首次 Loaded，或无库时首次 WindowOpened）。
    let mut window_settings = window::Settings {
        size: iced::Size::new(1920.0, 1080.0),
        position: window::Position::Centered,
        min_size: Some(iced::Size::new(1024.0, 768.0)),
        resizable: true,
        decorations: true,
        transparent: false,
        ..window::Settings::default()
    };
    #[cfg(target_os = "linux")]
    {
        // Linux(GNOME/KDE) 的 Dock 绑定优先使用 WM_CLASS / app_id；
        // 必须与 desktop 文件名 `ipcheck.desktop` 的 basename 一致，才能合并为同一图标。
        window_settings.platform_specific.application_id = "ipcheck".to_string();
    }

    IpCheckApp::run(Settings {
        id: Some("ipcheck".to_string()),
        window: window_settings,
        antialiasing: true,
        default_font: Font::with_name(font_name),
        default_text_size: iced::Pixels(14.0),
        ..Settings::default()
    })
}

fn pick_font_family() -> &'static str {
    if matches_font("Microsoft YaHei") || matches_font("微软雅黑") {
        "Microsoft YaHei"
    } else if matches_font("Noto Sans CJK SC") {
        "Noto Sans CJK SC"
    } else if matches_font("WenQuanYi Micro Hei") || matches_font("文泉驿微米黑") {
        "WenQuanYi Micro Hei"
    } else {
        "sans-serif"
    }
}

fn matches_font(name: &str) -> bool {
    let output = Command::new("fc-match")
        .args(["-f", "%{family}\n", name])
        .output();
    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let matched = String::from_utf8_lossy(&output.stdout).to_lowercase();
    let needle = name.to_lowercase();
    matched.contains(&needle)
}
