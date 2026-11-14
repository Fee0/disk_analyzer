use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Shell, Widget, overlay, renderer};
use iced::keyboard::{self, Key, key::Named};
use iced::{Element, Event, Length, Pixels, Point, Rectangle, Size, Vector, mouse};

pub struct Dropdown<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer> {
    opener: Element<'a, Message, Theme, Renderer>,
    menu: Element<'a, Message, Theme, Renderer>,
    open: bool,
    on_dismiss: Message,
    gap: f32,
}

pub fn dropdown<'a, Message, Theme, Renderer>(
    opener: impl Into<Element<'a, Message, Theme, Renderer>>,
    menu: impl Into<Element<'a, Message, Theme, Renderer>>,
    open: bool,
    on_dismiss: Message,
) -> Dropdown<'a, Message, Theme, Renderer> {
    Dropdown { opener: opener.into(), menu: menu.into(), open, on_dismiss, gap: 0.0 }
}

impl<Message, Theme, Renderer> Dropdown<'_, Message, Theme, Renderer> {
    pub fn gap(mut self, gap: impl Into<Pixels>) -> Self {
        self.gap = gap.into().0;
        self
    }
}

const OPENER: usize = 0;

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Dropdown<'a, Message, Theme, Renderer>
where
    Message: Clone,
    Renderer: iced::advanced::Renderer,
{
    fn size(&self) -> Size<Length> {
        self.opener.as_widget().size()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.opener), Tree::new(&self.menu)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&[&self.opener, &self.menu]);
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) -> layout::Node {
        self.opener.as_widget_mut().layout(&mut tree.children[OPENER], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.opener.as_widget().draw(&tree.children[OPENER], renderer, theme, style, layout, cursor, viewport);
    }

    fn operate(&mut self, tree: &mut Tree, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.opener.as_widget_mut().operate(&mut tree.children[OPENER], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.opener.as_widget_mut().update(
            &mut tree.children[OPENER],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.opener.as_widget().mouse_interaction(&tree.children[OPENER], layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let [opener_tree, menu_tree] = tree.children.as_mut_slice() else { return None };
        if !self.open {
            return self.opener.as_widget_mut().overlay(opener_tree, layout, renderer, viewport, translation);
        }
        Some(overlay::Element::new(Box::new(Menu {
            menu: &mut self.menu,
            tree: menu_tree,
            opener: layout.bounds() + translation,
            gap: self.gap,
            on_dismiss: self.on_dismiss.clone(),
        })))
    }
}

struct Menu<'a, 'b, Message, Theme, Renderer> {
    menu: &'b mut Element<'a, Message, Theme, Renderer>,
    tree: &'b mut Tree,
    opener: Rectangle,
    gap: f32,
    on_dismiss: Message,
}

impl<Message, Theme, Renderer> Menu<'_, '_, Message, Theme, Renderer> {
    fn position(&self, size: Size, room: Size) -> Point {
        let below = self.opener.y + self.opener.height + self.gap;
        let above = self.opener.y - self.gap - size.height;
        let y = if below + size.height <= room.height { below } else { above.max(0.0) };
        let x = self.opener.x.min(room.width - size.width).max(0.0);
        Point::new(x, y)
    }
}

fn dismisses_the_menu(event: &Event, cursor: mouse::Cursor, menu: Rectangle) -> bool {
    match event {
        Event::Keyboard(keyboard::Event::KeyPressed { key: Key::Named(Named::Escape), .. }) => true,
        Event::Mouse(mouse::Event::ButtonPressed(_)) => !cursor.is_over(menu),
        _ => false,
    }
}

fn picks_from_the_menu(event: &Event, cursor: mouse::Cursor, menu: Rectangle, taken_by_the_menu: bool) -> bool {
    let released = matches!(event, Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)));
    released && taken_by_the_menu && cursor.is_over(menu)
}

fn would_reach_what_is_under_the_menu(event: &Event, cursor: mouse::Cursor, menu: Rectangle) -> bool {
    let clicks_or_scrolls = matches!(
        event,
        Event::Mouse(
            mouse::Event::ButtonPressed(_) | mouse::Event::ButtonReleased(_) | mouse::Event::WheelScrolled { .. }
        )
    );
    clicks_or_scrolls && cursor.is_over(menu)
}

impl<Message, Theme, Renderer> overlay::Overlay<Message, Theme, Renderer> for Menu<'_, '_, Message, Theme, Renderer>
where
    Message: Clone,
    Renderer: iced::advanced::Renderer,
{
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let node = self.menu.as_widget_mut().layout(self.tree, renderer, &layout::Limits::new(Size::ZERO, bounds));
        let position = self.position(node.size(), bounds);
        node.move_to(position)
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.menu.as_widget().draw(self.tree, renderer, theme, style, layout, cursor, &layout.bounds());
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.menu.as_widget_mut().operate(self.tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let bounds = layout.bounds();
        if dismisses_the_menu(event, cursor, bounds) {
            shell.publish(self.on_dismiss.clone());
            shell.capture_event();
            return;
        }
        self.menu.as_widget_mut().update(self.tree, event, layout, cursor, renderer, clipboard, shell, &bounds);
        if picks_from_the_menu(event, cursor, bounds, shell.is_event_captured()) {
            shell.publish(self.on_dismiss.clone());
        }
        if would_reach_what_is_under_the_menu(event, cursor, bounds) {
            shell.capture_event();
        }
    }

    fn mouse_interaction(&self, layout: Layout<'_>, cursor: mouse::Cursor, renderer: &Renderer) -> mouse::Interaction {
        let bounds = layout.bounds();
        match self.menu.as_widget().mouse_interaction(self.tree, layout, cursor, &bounds, renderer) {
            mouse::Interaction::None if cursor.is_over(bounds) => mouse::Interaction::Idle,
            interaction => interaction,
        }
    }
}

impl<'a, Message, Theme, Renderer> From<Dropdown<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    fn from(dropdown: Dropdown<'a, Message, Theme, Renderer>) -> Self {
        Element::new(dropdown)
    }
}
