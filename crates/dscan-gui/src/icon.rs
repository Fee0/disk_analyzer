//! Small line icons as inline SVG (no icon font needed). They are drawn in
//! black and tinted at render time.

use std::sync::LazyLock;

use iced::widget::svg::{self, Handle, Svg};
use iced::{Color, Theme};

fn handle(paths: &str) -> Handle {
    let doc = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{paths}</svg>"#
    );
    Handle::from_memory(doc.into_bytes())
}

static HOME: LazyLock<Handle> =
    LazyLock::new(|| handle(r#"<path d="M3 10.5 12 3l9 7.5"/><path d="M5.5 9v11.5h4.5v-6h4v6h4.5V9"/>"#));
static REFRESH: LazyLock<Handle> =
    LazyLock::new(|| handle(r#"<path d="M20 12a8 8 0 1 1-2.34-5.66L20 8.5"/><path d="M20 3.5v5h-5"/>"#));

static CLOSE: LazyLock<Handle> = LazyLock::new(|| handle(r#"<path d="M6 6l12 12M18 6 6 18"/>"#));

fn icon<'a>(h: &Handle, size: f32, color: Color) -> Svg<'a, Theme> {
    Svg::new(h.clone()).width(size).height(size).style(move |_, _| svg::Style { color: Some(color) })
}

pub fn home<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    icon(&HOME, size, color)
}

pub fn refresh<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    icon(&REFRESH, size, color)
}

pub fn close<'a>(size: f32, color: Color) -> Svg<'a, Theme> {
    icon(&CLOSE, size, color)
}
