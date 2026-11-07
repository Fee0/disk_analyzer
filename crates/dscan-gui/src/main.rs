#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod animation;
mod app;
mod fmt;
#[cfg(test)]
mod iced_inspect;
mod icon;
#[cfg(test)]
mod inspect_tests;
mod sunburst;
mod theme;
mod view;

use std::path::PathBuf;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn folder_to_scan_at_launch() -> Option<PathBuf> {
    std::env::args_os().nth(1).map(PathBuf::from).filter(|p| p.is_dir()).map(|p| p.canonicalize().unwrap_or(p))
}

fn main() -> iced::Result {
    let initial = folder_to_scan_at_launch();
    iced::application(move || app::App::new(initial.clone()), app::App::update, view::view)
        .title(app::App::title)
        .subscription(app::App::subscription)
        .theme(|_: &app::App| theme::app_theme())
        .window(iced::window::Settings {
            size: view::DEFAULT_WINDOW_SIZE,
            min_size: Some(view::MIN_WINDOW_SIZE),
            ..iced::window::Settings::default()
        })
        .antialiasing(true)
        .run()
}
