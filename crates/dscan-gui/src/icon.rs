use std::sync::LazyLock;

use iced::widget::svg::{self, Handle, Svg};
use iced::{Color, Theme};

fn black_line_icon_svg(paths: &str) -> Handle {
    let doc = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{paths}</svg>"#
    );
    Handle::from_memory(doc.into_bytes())
}

static HOME: LazyLock<Handle> =
    LazyLock::new(|| black_line_icon_svg(r#"<path d="M3 10.5 12 3l9 7.5"/><path d="M5.5 9v11.5h4.5v-6h4v6h4.5V9"/>"#));
static REFRESH: LazyLock<Handle> =
    LazyLock::new(|| black_line_icon_svg(r#"<path d="M20 12a8 8 0 1 1-2.34-5.66L20 8.5"/><path d="M20 3.5v5h-5"/>"#));

static CLOSE: LazyLock<Handle> = LazyLock::new(|| black_line_icon_svg(r#"<path d="M6 6l12 12M18 6 6 18"/>"#));

static CHEVRON_LEFT: LazyLock<Handle> = LazyLock::new(|| black_line_icon_svg(r#"<path d="M15 18 9 12l6-6"/>"#));
static CHEVRON_RIGHT: LazyLock<Handle> = LazyLock::new(|| black_line_icon_svg(r#"<path d="m9 18 6-6-6-6"/>"#));
static CHEVRON_UP: LazyLock<Handle> = LazyLock::new(|| black_line_icon_svg(r#"<path d="m6 15 6-6 6 6"/>"#));
static CHEVRON_DOWN: LazyLock<Handle> = LazyLock::new(|| black_line_icon_svg(r#"<path d="m6 9 6 6 6-6"/>"#));

static CHECK_CIRCLE: LazyLock<Handle> =
    LazyLock::new(|| black_line_icon_svg(r#"<circle cx="12" cy="12" r="9"/><path d="m8 12.5 2.8 2.8 5.7-5.8"/>"#));
static WARNING_TRIANGLE: LazyLock<Handle> = LazyLock::new(|| {
    black_line_icon_svg(
        r#"<path d="M10.3 4 2.5 17.5a2 2 0 0 0 1.7 3h15.6a2 2 0 0 0 1.7-3L13.7 4a2 2 0 0 0-3.4 0z"/><path d="M12 9.5v4M12 17h.01"/>"#,
    )
});
static ERROR_CIRCLE: LazyLock<Handle> =
    LazyLock::new(|| black_line_icon_svg(r#"<circle cx="12" cy="12" r="9"/><path d="M12 7.5v5.5M12 16.5h.01"/>"#));

static PAUSE: LazyLock<Handle> = LazyLock::new(|| black_line_icon_svg(r#"<path d="M9 5v14M15 5v14"/>"#));
static RESUME: LazyLock<Handle> = LazyLock::new(|| black_line_icon_svg(r#"<path d="M8 5.5v13l10-6.5z"/>"#));

static DRIVE: LazyLock<Handle> = LazyLock::new(|| {
    black_line_icon_svg(
        r#"<rect x="3" y="13" width="18" height="7" rx="2"/><path d="M5 13 7.5 5h9l2.5 8"/><path d="M7 16.5h.01"/>"#,
    )
});
static FOLDER: LazyLock<Handle> = LazyLock::new(|| {
    black_line_icon_svg(
        r#"<path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2.5h7.5A2.5 2.5 0 0 1 21 10v7.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5z"/>"#,
    )
});

fn tinted<'a>(black_icon: &Handle, size: f32, color: Color) -> Svg<'a, Theme> {
    Svg::new(black_icon.clone()).width(size).height(size).style(move |_, _| svg::Style { color: Some(color) })
}

pub fn home<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&HOME, size, color)
}

pub fn refresh<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&REFRESH, size, color)
}

pub fn close<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&CLOSE, size, color)
}

pub fn chevron_left<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&CHEVRON_LEFT, size, color)
}

pub fn chevron_right<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&CHEVRON_RIGHT, size, color)
}

pub fn chevron_up<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&CHEVRON_UP, size, color)
}

pub fn chevron_down<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&CHEVRON_DOWN, size, color)
}

pub fn check_circle<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&CHECK_CIRCLE, size, color)
}

pub fn warning_triangle<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&WARNING_TRIANGLE, size, color)
}

pub fn error_circle<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&ERROR_CIRCLE, size, color)
}

pub fn drive<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&DRIVE, size, color)
}

pub fn folder<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&FOLDER, size, color)
}

pub fn pause<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&PAUSE, size, color)
}

pub fn resume<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    tinted(&RESUME, size, color)
}

pub fn scan_root<'a>(whole_disk: bool, size: f32, color: Color) -> Svg<'a, Theme> {
    if whole_disk { drive(size, color) } else { folder(size, color) }
}
