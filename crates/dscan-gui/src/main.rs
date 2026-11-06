//! Disk Analyzer: a DaisyDisk-style disk usage explorer for macOS.

mod app;
mod fmt;
mod icon;
mod quicklook;
mod sunburst;
mod theme;
mod view;
#[cfg(test)]
mod iced_inspect;
#[cfg(test)]
mod inspect_tests;

use std::path::PathBuf;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> iced::Result {
    // `disk-analyzer <folder>` scans that folder right away.
    let initial: Option<PathBuf> = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .map(|p| p.canonicalize().unwrap_or(p));

    iced::application(move || app::App::new(initial.clone()), app::App::update, view::view)
        .title(app::App::title)
        .subscription(app::App::subscription)
        .theme(|_: &app::App| theme::app_theme())
        .window_size((1280.0, 820.0))
        .antialiasing(true)
        .run()
}
