use std::ops::Range;

use iced::widget::canvas::{self, Frame, Geometry, Path};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::app::Message;
use crate::theme;

pub struct Meter {
    pub fraction: f32,
    pub color: Color,
}

fn bar(frame: &mut Frame, span: Range<f32>, height: f32, color: Color) {
    if span.end <= span.start {
        return;
    }
    let path = Path::rounded_rectangle(
        Point::new(span.start, 0.0),
        Size::new(span.end - span.start, height),
        (height / 2.0).into(),
    );
    frame.fill(&path, color);
}

impl canvas::Program<Message> for Meter {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let size = bounds.size();
        let mut frame = Frame::new(renderer, size);
        bar(&mut frame, 0.0..size.width, size.height, theme::BG);
        bar(&mut frame, 0.0..self.fraction.clamp(0.0, 1.0) * size.width, size.height, self.color);
        vec![frame.into_geometry()]
    }
}
