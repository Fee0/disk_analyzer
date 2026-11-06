//! Headless inspection helpers for iced UIs, built on `iced_test`.
//!
//! Drop this file next to the code under test (e.g. `src/iced_inspect.rs`) and
//! declare it with `#[cfg(test)] mod iced_inspect;`. Works with `iced_test`
//! 0.14 (crates.io) and the 0.15-dev `master` branch: it only relies on
//! `Simulator::{find, point_at, simulate, snapshot}` and the selector
//! `Candidate` enum, which both versions share.
//!
//! What it gives you:
//! - [`nodes`] / [`tree`]: every widget the selector API can see, with kind,
//!   id, text, bounds, visibility, focus and scroll offset, as an indented tree.
//! - [`lint`]: layout red flags (hidden, clipped, off-viewport, zero-size,
//!   overlapping text, duplicate ids).
//! - [`gaps`]: regions with no visible widget, where invisible widgets
//!   (sliders, togglers, images, canvas, ...) most likely are.
//! - [`report`]: tree + lint + gaps in one string, ready to print.
//! - [`shot`]: always writes a PNG of the current frame and returns its path.
//! - [`click_at`] / [`drag`]: pointer input at raw coordinates, for widgets the
//!   selectors cannot target (sliders, togglers, radios, icon-only buttons, canvas).
#![allow(dead_code, clippy::collapsible_if, clippy::type_complexity)]

use iced_test::core::mouse;
use iced_test::core::theme;
use iced_test::core::{Event, Point, Rectangle};
use iced_test::selector::Candidate;
use iced_test::{Error, Simulator};

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// A widget as seen by iced's selector/operation API.
#[derive(Debug, Clone)]
pub struct Node {
    /// Nesting depth, inferred from traversal order + bounds containment.
    pub depth: usize,
    /// `container`, `scrollable`, `focusable`, `text_input`, `text` or `custom`.
    pub kind: &'static str,
    /// Custom widget id, if one was set with `.id(...)`.
    pub id: Option<String>,
    /// Text content. For a `text_input` this is the *placeholder* when empty.
    pub text: Option<String>,
    /// Layout bounds, in logical pixels.
    pub bounds: Rectangle,
    /// On-screen part of `bounds`; `None` when fully clipped or scrolled away.
    pub visible: Option<Rectangle>,
    /// Focus state, for focusable widgets (text inputs, editors, combo boxes).
    pub focused: Option<bool>,
    /// Scroll translation `(x, y)`, for scrollables.
    pub scroll: Option<(f32, f32)>,
    /// Whether the node lives inside a scrollable (being off-screen is then normal).
    pub in_scrollable: bool,
}

/// Collects every node the selector API can see, in depth-first order.
pub fn nodes<'a, Message, Theme, Renderer>(
    ui: &mut Simulator<'a, Message, Theme, Renderer>,
) -> Vec<Node>
where
    Message: 'a,
    Theme: theme::Base + 'a,
    Renderer: iced_test::core::Renderer + iced_test::core::renderer::Headless + 'a,
{
    let mut raw: Vec<Node> = Vec::new();

    // A selector that never matches visits the whole tree; we record as we go.
    let _ = ui.find(|candidate: Candidate<'_>| -> Option<()> {
        raw.push(to_node(&candidate));
        None
    });

    // Text inputs report twice (`text_input` + `focusable`): fold focus into the first.
    let mut merged: Vec<Node> = Vec::with_capacity(raw.len());
    for node in raw {
        if node.kind == "focusable" {
            if let Some(prev) = merged.last_mut() {
                if prev.kind == "text_input" && prev.bounds == node.bounds && prev.id == node.id {
                    prev.focused = node.focused;
                    continue;
                }
            }
        }
        merged.push(node);
    }

    // Infer depth: pre-order traversal, so a node's parent is the nearest
    // earlier frame that still accepts it. Scrollable content overflows the
    // scrollable's bounds, so inside one we also test visible bounds.
    struct Frame {
        bounds: Rectangle,
        clip: Option<Rectangle>,
        scroll: bool,
    }

    let accepts = |frame: &Frame, node: &Node| {
        let visible_in = |clip: &Rectangle| node.visible.is_none_or(|v| contains(clip, &v));
        if frame.scroll {
            visible_in(&frame.bounds)
        } else {
            contains(&frame.bounds, &node.bounds) && frame.clip.as_ref().is_none_or(visible_in)
        }
    };

    let mut stack: Vec<Frame> = Vec::new();
    for node in &mut merged {
        while let Some(top) = stack.last() {
            if accepts(top, node) {
                break;
            }
            stack.pop();
        }
        node.depth = stack.len();
        node.in_scrollable = stack.iter().any(|f| f.scroll);

        if matches!(node.kind, "container" | "scrollable" | "custom") {
            let clip = match stack.last() {
                Some(parent) if parent.scroll => Some(parent.bounds),
                Some(parent) => parent.clip,
                None => None,
            };
            stack.push(Frame {
                bounds: node.bounds,
                clip,
                scroll: node.kind == "scrollable",
            });
        }
    }

    merged
}

fn to_node(candidate: &Candidate<'_>) -> Node {
    let (kind, text, focused, scroll) = match candidate {
        Candidate::Container { .. } => ("container", None, None, None),
        Candidate::Scrollable { translation, .. } => {
            ("scrollable", None, None, Some((translation.x, translation.y)))
        }
        Candidate::Focusable { state, .. } => ("focusable", None, Some(state.is_focused()), None),
        Candidate::TextInput { state, .. } => ("text_input", Some(state.text().to_string()), None, None),
        Candidate::Text { content, .. } => ("text", Some(content.to_string()), None, None),
        Candidate::Custom { .. } => ("custom", None, None, None),
    };

    Node {
        depth: 0,
        kind,
        id: candidate.id().map(id_to_string),
        text,
        bounds: candidate.bounds(),
        visible: candidate.visible_bounds(),
        focused,
        scroll,
        in_scrollable: false,
    }
}

fn id_to_string(id: &iced_test::core::widget::Id) -> String {
    // `Id` has no Display; its Debug looks like `Id(Custom("name"))` or `Id(Unique(3))`.
    let debug = format!("{id:?}");
    match (debug.find('"'), debug.rfind('"')) {
        (Some(start), Some(end)) if end > start => debug[start + 1..end].to_owned(),
        _ => debug,
    }
}

fn contains(outer: &Rectangle, inner: &Rectangle) -> bool {
    const EPS: f32 = 0.5;
    inner.x >= outer.x - EPS
        && inner.y >= outer.y - EPS
        && inner.x + inner.width <= outer.x + outer.width + EPS
        && inner.y + inner.height <= outer.y + outer.height + EPS
}

fn rect(r: &Rectangle) -> String {
    format!("[{:.0},{:.0} {:.0}x{:.0}]", r.x, r.y, r.width, r.height)
}

/// Renders nodes as an indented, agent-readable tree.
pub fn tree(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        let _ = write!(out, "{}{}", "  ".repeat(node.depth), node.kind);
        if let Some(id) = &node.id {
            let _ = write!(out, " #{id}");
        }
        if let Some(text) = &node.text {
            let _ = write!(out, " {text:?}");
        }
        let _ = write!(out, " {}", rect(&node.bounds));
        match &node.visible {
            None => out.push_str(" HIDDEN"),
            Some(v) if (v.width * v.height) + 0.5 < node.bounds.width * node.bounds.height => {
                let _ = write!(out, " CLIPPED->{}", rect(v));
            }
            _ => {}
        }
        if let Some(focused) = node.focused {
            let _ = write!(out, " focused={focused}");
        }
        if let Some((x, y)) = node.scroll {
            let _ = write!(out, " scroll=({x:.0},{y:.0})");
        }
        out.push('\n');
    }
    out
}

/// Flags likely layout problems. `viewport` is the simulator size.
pub fn lint(nodes: &[Node], viewport: impl Into<iced_test::core::Size>) -> Vec<String> {
    let viewport = viewport.into();
    let screen = Rectangle::new(Point::ORIGIN, viewport);
    let mut issues = Vec::new();

    let label = |n: &Node| match (&n.id, &n.text) {
        (Some(id), _) => format!("{} #{id}", n.kind),
        (None, Some(text)) => format!("{} {text:?}", n.kind),
        _ => format!("{} {}", n.kind, rect(&n.bounds)),
    };

    let mut scrolled_out = 0;

    for n in nodes {
        let is_leaf = matches!(n.kind, "text" | "text_input");
        if is_leaf && (n.bounds.width < 1.0 || n.bounds.height < 1.0) {
            issues.push(format!("zero-size: {} has bounds {}", label(n), rect(&n.bounds)));
        }
        if n.in_scrollable {
            // Content outside a scrollable's viewport is expected; just count it.
            if is_leaf && n.visible.is_none() {
                scrolled_out += 1;
            }
            continue;
        }
        if !contains(&screen, &n.bounds) {
            issues.push(format!(
                "off-viewport: {} {} extends past the {:.0}x{:.0} viewport",
                label(n),
                rect(&n.bounds),
                viewport.width,
                viewport.height
            ));
        }
        if is_leaf {
            match &n.visible {
                None => issues.push(format!("hidden: {} is fully clipped", label(n))),
                Some(v) if v.width + 0.5 < n.bounds.width || v.height + 0.5 < n.bounds.height => {
                    issues.push(format!(
                        "clipped: {} shows only {} of {}",
                        label(n),
                        rect(v),
                        rect(&n.bounds)
                    ))
                }
                _ => {}
            }
        }
        if n.kind == "text" {
            if let Some(text) = &n.text {
                // Text is laid out inside its bounds, so overflow cut off by a
                // fixed-size parent does not show up as clipping. Estimate the
                // width the string needs (~0.35em per char at 16px, deliberately
                // low) and compare it with what the bounds can hold.
                let needed = text.chars().count() as f32 * 5.6;
                let lines = (n.bounds.height / 20.8).floor().max(1.0);
                let capacity = n.bounds.width * lines;
                if text.chars().count() > 8 && needed > capacity * 1.5 {
                    issues.push(format!(
                        "maybe-truncated: {} needs ~{:.0}px of line width but its bounds {} fit ~{:.0}px; check the screenshot",
                        label(n),
                        needed,
                        rect(&n.bounds),
                        capacity
                    ));
                }
            }
        }
    }

    if scrolled_out > 0 {
        issues.push(format!(
            "info: {scrolled_out} text node(s) are scrolled out of view inside scrollables (normal; scroll or enlarge to see them)"
        ));
    }

    // Overlapping text usually means a broken layout (stack/float aside).
    let texts: Vec<&Node> = nodes.iter().filter(|n| n.kind == "text" && n.visible.is_some()).collect();
    for (i, a) in texts.iter().enumerate() {
        for b in &texts[i + 1..] {
            if let Some(overlap) = a.bounds.intersection(&b.bounds) {
                if overlap.width > 1.0 && overlap.height > 1.0 {
                    issues.push(format!("overlap: {} and {} intersect at {}", label(a), label(b), rect(&overlap)));
                }
            }
        }
    }

    let mut seen: Vec<&str> = Vec::new();
    for n in nodes {
        if let Some(id) = &n.id {
            // text_input + focusable share an id legitimately; they are merged already.
            if seen.contains(&id.as_str()) {
                issues.push(format!("duplicate-id: #{id} appears more than once"));
            } else {
                seen.push(id);
            }
        }
    }

    issues
}

/// Finds empty-looking regions between visible siblings. iced's selectors
/// cannot see sliders, togglers (incl. their labels), radios, progress bars,
/// images, svgs, canvas, shaders or `space`, so a large unexplained gap in a
/// column/row usually *is* one of those. Returns `(region, hint)` pairs in
/// logical pixels; click or drag inside the region to interact with it.
pub fn gaps(nodes: &[Node]) -> Vec<(Rectangle, String)> {
    let mut found = Vec::new();

    for (i, parent) in nodes.iter().enumerate() {
        if !matches!(parent.kind, "container" | "custom") || parent.in_scrollable {
            continue;
        }

        // Direct children: following nodes exactly one level deeper, until we
        // leave the parent's subtree.
        let mut children: Vec<Rectangle> = Vec::new();
        for node in &nodes[i + 1..] {
            if node.depth <= parent.depth {
                break;
            }
            if node.depth == parent.depth + 1 && node.visible.is_some() {
                children.push(node.bounds);
            }
        }
        if children.len() < 2 {
            continue;
        }

        // A row if every child overlaps the first one vertically, else a column.
        let first = children[0];
        let is_row = children
            .iter()
            .all(|c| c.y < first.y + first.height && first.y < c.y + c.height);

        let (start, end): (fn(&Rectangle) -> f32, fn(&Rectangle) -> f32) = if is_row {
            (|r| r.x, |r| r.x + r.width)
        } else {
            (|r| r.y, |r| r.y + r.height)
        };

        children.sort_by(|a, b| start(a).total_cmp(&start(b)));

        let mut spans = Vec::new();
        let mut reach = end(&children[0]);
        for child in &children[1..] {
            spans.push((reach, start(child)));
            reach = reach.max(end(child));
        }

        // The smallest gap is usually the layout's `spacing`; flag gaps well above it.
        // With a single gap (e.g. `row![label, slider, label]` where only the
        // labels are visible) there is nothing to compare to, so use a fixed bar.
        let spacing = spans.iter().map(|(a, b)| b - a).fold(f32::INFINITY, f32::min).max(0.0);
        let single = spans.len() == 1;

        for (a, b) in spans {
            let size = b - a;
            let suspicious = if single { size >= 40.0 } else { size >= 20.0 && size > spacing * 2.0 + 8.0 };
            if suspicious {
                let region = if is_row {
                    Rectangle { x: a, y: parent.bounds.y, width: size, height: parent.bounds.height }
                } else {
                    Rectangle { x: parent.bounds.x, y: a, width: parent.bounds.width, height: size }
                };
                found.push((
                    region,
                    format!(
                        "{:.0}px {} gap inside container {} with no visible widget{}; likely a slider/toggler/radio/image/canvas/space",
                        size,
                        if is_row { "horizontal" } else { "vertical" },
                        rect(&parent.bounds),
                        if single { String::new() } else { format!(" (spacing looks like {spacing:.0}px)") }
                    ),
                ));
            }
        }
    }

    found
}

/// Tree + lint + gaps in one printable block.
pub fn report<'a, Message, Theme, Renderer>(
    ui: &mut Simulator<'a, Message, Theme, Renderer>,
    viewport: impl Into<iced_test::core::Size>,
) -> String
where
    Message: 'a,
    Theme: theme::Base + 'a,
    Renderer: iced_test::core::Renderer + iced_test::core::renderer::Headless + 'a,
{
    let viewport = viewport.into();
    let nodes = nodes(ui);
    let issues = lint(&nodes, viewport);

    let mut out = format!(
        "== widget tree ({} nodes, viewport {:.0}x{:.0}) ==\n",
        nodes.len(),
        viewport.width,
        viewport.height
    );
    out.push_str(&tree(&nodes));
    out.push_str(&format!("== lint ({} issues) ==\n", issues.len()));
    for issue in &issues {
        out.push_str("- ");
        out.push_str(issue);
        out.push('\n');
    }
    let gaps = gaps(&nodes);
    out.push_str(&format!("== unaccounted regions ({}) ==\n", gaps.len()));
    for (region, hint) in &gaps {
        out.push_str(&format!("- {} {hint}\n", rect(region)));
    }
    out
}

/// Renders the current frame and writes it as a PNG. Returns the file path.
///
/// The image is at 2x scale (a 400x300 viewport gives an 800x600 PNG). The
/// file name gets the renderer as suffix, e.g. `dir/name-wgpu.png`.
pub fn shot<'a, Message, Theme, Renderer>(
    ui: &mut Simulator<'a, Message, Theme, Renderer>,
    theme: &Theme,
    dir: impl AsRef<Path>,
    name: &str,
) -> Result<PathBuf, Error>
where
    Message: 'a,
    Theme: theme::Base + 'a,
    Renderer: iced_test::core::Renderer + iced_test::core::renderer::Headless + 'a,
{
    let dir = dir.as_ref();
    fs::create_dir_all(dir)?;

    // `matches_image` only writes when no file exists, so clear old shots first.
    let prefix = format!("{name}-");
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let file = path.file_name().and_then(|f| f.to_str()).unwrap_or_default();
        if file.starts_with(&prefix) && file.ends_with(".png") {
            fs::remove_file(&path)?;
        }
    }

    let snapshot = ui.snapshot(theme)?;
    let _ = snapshot.matches_image(dir.join(format!("{name}.png")))?;

    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let file = path.file_name().and_then(|f| f.to_str()).unwrap_or_default();
        if file.starts_with(&prefix) && file.ends_with(".png") {
            return Ok(path);
        }
    }

    Err(Error::IOFailed(std::sync::Arc::new(std::io::Error::other(
        "snapshot was not written",
    ))))
}

/// Left-clicks at a raw position (logical pixels).
pub fn click_at<'a, Message, Theme, Renderer>(
    ui: &mut Simulator<'a, Message, Theme, Renderer>,
    position: impl Into<Point>,
) where
    Message: 'a,
    Theme: theme::Base + 'a,
    Renderer: iced_test::core::Renderer + iced_test::core::renderer::Headless + 'a,
{
    let position = position.into();
    ui.point_at(position);
    let _ = ui.simulate([
        Event::Mouse(mouse::Event::CursorMoved { position }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
    ]);
}

/// Presses at `from`, moves to `to` in `steps` increments, then releases.
/// Use for sliders, pane splits, scrollbars.
pub fn drag<'a, Message, Theme, Renderer>(
    ui: &mut Simulator<'a, Message, Theme, Renderer>,
    from: impl Into<Point>,
    to: impl Into<Point>,
    steps: usize,
) where
    Message: 'a,
    Theme: theme::Base + 'a,
    Renderer: iced_test::core::Renderer + iced_test::core::renderer::Headless + 'a,
{
    let (from, to) = (from.into(), to.into());
    let steps = steps.max(1);

    ui.point_at(from);
    let _ = ui.simulate([
        Event::Mouse(mouse::Event::CursorMoved { position: from }),
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
    ]);

    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let position = Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        ui.point_at(position);
        let _ = ui.simulate([Event::Mouse(mouse::Event::CursorMoved { position })]);
    }

    let _ = ui.simulate([Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))]);
}

/// Moves the pointer without clicking, so hover styles show up in [`shot`].
pub fn hover<'a, Message, Theme, Renderer>(
    ui: &mut Simulator<'a, Message, Theme, Renderer>,
    position: impl Into<Point>,
) where
    Message: 'a,
    Theme: theme::Base + 'a,
    Renderer: iced_test::core::Renderer + iced_test::core::renderer::Headless + 'a,
{
    let position = position.into();
    ui.point_at(position);
    let _ = ui.simulate([Event::Mouse(mouse::Event::CursorMoved { position })]);
}
