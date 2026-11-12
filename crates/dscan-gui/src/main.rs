#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod animation;
mod app;
mod collector;
mod depth;
mod fmt;
#[cfg(test)]
mod iced_inspect;
mod icon;
#[cfg(test)]
mod inspect_tests;
mod list_scroll;
mod sunburst;
mod theme;
mod view;

use std::path::PathBuf;

use iced::window;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const WINDOW_ICON_SIDE: u32 = 64;
const WINDOW_ICON_RGBA: &[u8] = include_bytes!("../../../assets/icon/window-icon-64x64.rgba");

fn window_icon() -> Option<window::Icon> {
    window::icon::from_rgba(WINDOW_ICON_RGBA.to_vec(), WINDOW_ICON_SIDE, WINDOW_ICON_SIDE).ok()
}

#[cfg(target_os = "linux")]
fn platform_window_settings() -> window::settings::PlatformSpecific {
    const DESKTOP_ENTRY_ID: &str = "disk-analyzer";
    window::settings::PlatformSpecific {
        application_id: DESKTOP_ENTRY_ID.to_owned(),
        ..window::settings::PlatformSpecific::default()
    }
}

#[cfg(not(target_os = "linux"))]
fn platform_window_settings() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific::default()
}

fn folder_to_scan_at_launch() -> Option<PathBuf> {
    std::env::args_os().nth(1).map(PathBuf::from).and_then(dscan_core::existing_folder)
}

fn main() -> iced::Result {
    let initial = folder_to_scan_at_launch();
    iced::application(move || app::App::new(initial.clone()), app::App::update, view::view)
        .title(app::App::title)
        .subscription(app::App::subscription)
        .theme(|_: &app::App| theme::app_theme())
        .window(window::Settings {
            size: view::DEFAULT_WINDOW_SIZE,
            min_size: Some(view::MIN_WINDOW_SIZE),
            icon: window_icon(),
            platform_specific: platform_window_settings(),
            ..window::Settings::default()
        })
        .antialiasing(true)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_window_icon_is_a_square_rgba_image() {
        assert_eq!(WINDOW_ICON_RGBA.len(), (WINDOW_ICON_SIDE * WINDOW_ICON_SIDE * 4) as usize);
        assert!(window_icon().is_some(), "iced rejected the bundled window icon");
    }
}
