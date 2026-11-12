use iced::advanced::widget::operation::{self, Outcome, Scrollable};
use iced::advanced::widget::{Id, Operation, operate};
use iced::widget::scrollable::AbsoluteOffset;
use iced::{Rectangle, Task, Vector};

pub const LIST_ID: &str = "list-rows";
pub const SELECTED_ROW_ID: &str = "selected-list-row";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollFrom {
    WhereTheListIs,
    TheTop,
}

pub fn show_selected_row<T: Send + 'static>(from: ScrollFrom) -> Task<T> {
    operate(ShowSelectedRow { from, list: None, selected_row: None })
}

#[derive(Clone, Copy)]
struct ListViewport {
    content_top: f32,
    height: f32,
    scrolled: f32,
}

#[derive(Clone, Copy)]
struct RowOnScreen {
    top: f32,
    height: f32,
}

struct ShowSelectedRow {
    from: ScrollFrom,
    list: Option<ListViewport>,
    selected_row: Option<RowOnScreen>,
}

fn offset_showing(row_top: f32, row_height: f32, scrolled: f32, viewport_height: f32) -> Option<f32> {
    let row_bottom = row_top + row_height;
    if row_top < scrolled {
        Some(row_top)
    } else if row_bottom > scrolled + viewport_height {
        Some(row_bottom - viewport_height)
    } else {
        None
    }
}

impl ShowSelectedRow {
    fn wanted_offset(&self) -> Option<f32> {
        let list = self.list?;
        let (scrolled, offset_if_the_row_is_in_view) = match self.from {
            ScrollFrom::WhereTheListIs => (list.scrolled, None),
            ScrollFrom::TheTop => (0.0, Some(0.0)),
        };
        self.selected_row
            .and_then(|row| offset_showing(row.top - list.content_top, row.height, scrolled, list.height))
            .or(offset_if_the_row_is_in_view)
    }
}

impl<T: Send + 'static> Operation<T> for ShowSelectedRow {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        if id == Some(&Id::new(LIST_ID)) {
            self.list =
                Some(ListViewport { content_top: content_bounds.y, height: bounds.height, scrolled: translation.y });
        }
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&Id::new(SELECTED_ROW_ID)) {
            self.selected_row = Some(RowOnScreen { top: bounds.y, height: bounds.height });
        }
    }

    fn finish(&self) -> Outcome<T> {
        match self.wanted_offset() {
            Some(y) => Outcome::Chain(Box::new(operation::scrollable::scroll_to(
                Id::new(LIST_ID),
                AbsoluteOffset { x: None, y: Some(y) },
            ))),
            None => Outcome::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: ListViewport = ListViewport { content_top: 100.0, height: 50.0, scrolled: 200.0 };

    fn wanted(from: ScrollFrom, row_top_in_list: Option<f32>) -> Option<f32> {
        let selected_row = row_top_in_list.map(|top| RowOnScreen { top: LIST.content_top + top, height: 10.0 });
        ShowSelectedRow { from, list: Some(LIST), selected_row }.wanted_offset()
    }

    #[test]
    fn the_list_scrolls_only_as_far_as_needed_to_show_a_row_outside_the_view() {
        let from = ScrollFrom::WhereTheListIs;
        assert_eq!(wanted(from, Some(220.0)), None, "a row in view stays where it is");
        assert_eq!(wanted(from, Some(30.0)), Some(30.0), "a row above comes to the top");
        assert_eq!(wanted(from, Some(400.0)), Some(360.0), "a row below comes to the bottom");
        assert_eq!(wanted(from, None), None, "nothing selected, nothing to show");
    }

    #[test]
    fn a_newly_listed_folder_starts_at_the_top_unless_its_selected_row_is_further_down() {
        let from = ScrollFrom::TheTop;
        assert_eq!(wanted(from, None), Some(0.0));
        assert_eq!(wanted(from, Some(20.0)), Some(0.0), "the row shows from the top already");
        assert_eq!(wanted(from, Some(400.0)), Some(360.0));
    }

    #[test]
    fn without_the_list_on_screen_nothing_scrolls() {
        let op = ShowSelectedRow { from: ScrollFrom::TheTop, list: None, selected_row: None };
        assert_eq!(op.wanted_offset(), None);
    }
}
