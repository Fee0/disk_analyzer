use dscan_core::{Kind, NodeId, Segment, TRASH_NAME, format_size};
use iced::border;
use iced::widget::text::Wrapping;
use iced::widget::{
    Column, Space, Svg, button, canvas, center, column, container, mouse_area, pin, progress_bar, row, scrollable,
    stack, text, text_input, tooltip,
};
use iced::{Alignment, Color, Element, Fill, Length, Shrink, Theme};

use crate::animation::ChartAnimation;
use crate::app::{App, Browser, HOLD_TO_DELETE, ListRow, Message, SEARCH_ID, ScanJob, Screen};
use crate::fmt::{thousands, truncate};
use crate::icon;
use crate::sunburst::Sunburst;
use crate::theme;

pub fn view(app: &App) -> Element<'_, Message> {
    let content = match &app.screen {
        Screen::Start => start(app),
        Screen::Scanning(job) => scanning(job),
        Screen::Browse(b) => browse(b),
    };
    container(content).width(Fill).height(Fill).style(|_| container::background(theme::BG)).into()
}

fn rounded_box(bg: Color, radius: f32) -> container::Style {
    container::Style::default().background(bg).border(border::rounded(radius)).color(theme::TEXT)
}

fn dot<'a>(color: Color, size: f32) -> Element<'a, Message> {
    container(Space::new()).width(size).height(size).style(move |_| rounded_box(color, size / 2.0)).into()
}

const TOOLBAR_CONTROL_HEIGHT: f32 = 29.0;
const FRAME_INSET: f32 = 2.0;
const FRAME_RADIUS: f32 = 8.0;
const INNER_BUTTON_SIDE: f32 = TOOLBAR_CONTROL_HEIGHT - 2.0 * FRAME_INSET;

fn framed_group<'a>(content: impl Into<Element<'a, Message>>) -> container::Container<'a, Message> {
    container(content)
        .padding(FRAME_INSET)
        .height(TOOLBAR_CONTROL_HEIGHT)
        .align_y(iced::alignment::Vertical::Center)
        .style(|_| {
            rounded_box(theme::PANEL, FRAME_RADIUS)
                .border(border::rounded(FRAME_RADIUS).width(1).color(theme::PANEL_HI))
        })
}

fn quiet_button_style(text_when_idle: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: hovered.then(|| theme::PANEL_HI.into()),
            text_color: if hovered { theme::TEXT } else { text_when_idle },
            border: border::rounded(FRAME_RADIUS - FRAME_INSET),
            ..button::Style::default()
        }
    }
}

fn filled_white_text_button(bg: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => theme::mix(bg, Color::WHITE, 0.12),
            button::Status::Pressed => theme::mix(bg, Color::BLACK, 0.12),
            button::Status::Active => bg,
            button::Status::Disabled => bg.scale_alpha(0.5),
        };
        button::Style {
            background: Some(bg.into()),
            text_color: Color::WHITE,
            border: border::rounded(6),
            ..button::Style::default()
        }
    }
}

fn subtle_button<'a>(content: impl Into<Element<'a, Message>>, msg: Option<Message>) -> button::Button<'a, Message> {
    button(content)
        .padding([4, 8])
        .style(|t: &Theme, s| {
            let mut st = button::text(t, s);
            if matches!(s, button::Status::Hovered) {
                st.background = Some(theme::PANEL_HI.into());
            }
            st.text_color =
                if matches!(s, button::Status::Disabled) { theme::TEXT_DIM.scale_alpha(0.4) } else { theme::TEXT };
            st.border = st.border.rounded(6);
            st
        })
        .on_press_maybe(msg)
}

fn outlined_text_button(label: &str, msg: Message) -> Element<'_, Message> {
    button(text(label).size(13))
        .padding([6, 14])
        .style(|_: &Theme, s| {
            let bg = if matches!(s, button::Status::Hovered | button::Status::Pressed) {
                theme::PANEL_HI
            } else {
                theme::PANEL
            };
            button::Style {
                background: Some(bg.into()),
                text_color: theme::TEXT,
                border: border::rounded(FRAME_RADIUS).width(1).color(theme::PANEL_HI),
                ..button::Style::default()
            }
        })
        .on_press(msg)
        .into()
}

const TOOLTIP_MAX_WIDTH: f32 = 320.0;

fn with_tooltip<'a>(content: impl Into<Element<'a, Message>>, label: &'a str) -> Element<'a, Message> {
    let tip = container(text(label).size(12))
        .padding([4, 8])
        .max_width(TOOLTIP_MAX_WIDTH)
        .style(|_| rounded_box(theme::PANEL_HI, 6.0));
    tooltip(content, tip, tooltip::Position::Bottom).gap(4).into()
}

fn icon_button<'a>(
    icon: impl Into<Element<'a, Message>>,
    tooltip_label: &'a str,
    msg: Message,
) -> Element<'a, Message> {
    with_tooltip(subtle_button(icon, Some(msg)).padding(6), tooltip_label)
}

fn start(app: &App) -> Element<'_, Message> {
    let cards: Element<'_, Message> = match &app.volumes {
        None => text("Loading disks…").size(14).color(theme::TEXT_DIM).into(),
        Some(volumes) => {
            let mut cards = row![].spacing(16);
            for (i, v) in volumes.iter().enumerate() {
                let frac = if v.total > 0 { v.used as f32 / v.total as f32 } else { 0.0 };
                let card = column![
                    row![dot(theme::kind_color(Kind::Document), 12.0), text(v.name.clone()).size(20)]
                        .spacing(10)
                        .align_y(Alignment::Center),
                    text(v.mount.display().to_string()).size(12).color(theme::TEXT_DIM),
                    progress_bar(0.0..=1.0, frac).girth(8),
                    text(format!("{} used of {}", format_size(v.used), format_size(v.total))).size(13),
                    text(format!("{} available", format_size(v.available))).size(12).color(theme::TEXT_DIM),
                ]
                .spacing(10)
                .width(260);
                cards = cards.push(
                    button(card)
                        .padding(18)
                        .style(|t: &Theme, s| {
                            let bg = if matches!(s, button::Status::Hovered) { theme::PANEL_HI } else { theme::PANEL };
                            button::Style {
                                background: Some(bg.into()),
                                text_color: theme::TEXT,
                                border: border::rounded(12),
                                ..button::secondary(t, s)
                            }
                        })
                        .on_press(Message::ScanVolume(i)),
                );
            }
            cards.into()
        }
    };

    let disks_header = row![
        text("Disks").size(13).color(theme::TEXT_DIM),
        icon_button(icon::refresh(14.0, theme::TEXT_DIM), "Refresh disks", Message::RefreshVolumes),
    ]
    .spacing(4)
    .align_y(Alignment::Center);

    let mut col = column![
        text("Disk Analyzer").size(34),
        text("Choose a disk or a folder to see what takes up space.").size(15).color(theme::TEXT_DIM),
        Space::new().height(12),
        column![disks_header, cards].spacing(4).width(Shrink),
        Space::new().height(4),
        button(text("Open folder…").size(14))
            .padding([8, 16])
            .style(filled_white_text_button(theme::ACCENT))
            .on_press(Message::OpenFolder),
    ]
    .spacing(12)
    .align_x(Alignment::Center);

    if !app.parked_newest_first.is_empty() {
        col = col.push(Space::new().height(12)).push(open_results(app));
    }

    if !app.full_disk_access {
        let warn = theme::kind_color(Kind::Archive);
        col = col.push(Space::new().height(16)).push(
            container(
                row![
                    text("Without Full Disk Access some folders can't be read, so some space shows up as hidden. Grant it to your terminal (or this app), then restart it.")
                        .size(13)
                        .width(Length::Fixed(460.0)),
                    button(text("Open Settings").size(13)).style(filled_white_text_button(theme::ACCENT)).on_press(Message::OpenFdaSettings),
                ]
                .spacing(16)
                .align_y(Alignment::Center),
            )
            .padding(14)
            .style(move |_| rounded_box(warn.scale_alpha(0.15), 10.0).border(border::rounded(10).width(1).color(warn.scale_alpha(0.5)))),
        );
    }
    if let Some(err) = &app.error {
        col = col.push(text(err.clone()).color(theme::DANGER).size(14));
    }
    center(col).padding(24).into()
}

const RESULT_ROW_H: f32 = 56.0;
const RESULT_GAP: f32 = 6.0;
const RESULT_ROWS_BEFORE_SCROLLING: usize = 4;

fn open_results(app: &App) -> Element<'_, Message> {
    let mut list = Column::new().spacing(RESULT_GAP);
    for (i, b) in app.parked_newest_first.iter().enumerate() {
        let t = &b.tree;
        let root = t.root();
        let meta = format!("{} files", thousands(t.node(root).items() as u64));
        let content = row![
            dot(theme::kind_color(t.node(root).kind()), 10.0),
            column![
                text(truncate(&t.name(root), 40)).size(14).wrapping(Wrapping::None),
                text(truncate(&t.root_path().display().to_string(), 56))
                    .size(12)
                    .color(theme::TEXT_DIM)
                    .wrapping(Wrapping::None),
            ]
            .spacing(2)
            .width(Fill),
            column![
                text(format_size(t.size(root))).size(14),
                text(meta).size(12).color(theme::TEXT_DIM).wrapping(Wrapping::None),
            ]
            .spacing(2)
            .align_x(Alignment::End),
            icon_button(icon::close(12.0, theme::TEXT_DIM), "Close", Message::CloseParked(i)),
        ]
        .spacing(12)
        .height(Fill)
        .align_y(Alignment::Center);
        list = list.push(
            button(content)
                .padding([0, 12])
                .width(Fill)
                .height(RESULT_ROW_H)
                .style(|t: &Theme, s| {
                    let bg = if matches!(s, button::Status::Hovered) { theme::PANEL_HI } else { theme::PANEL };
                    button::Style {
                        background: Some(bg.into()),
                        text_color: theme::TEXT,
                        border: border::rounded(10),
                        ..button::secondary(t, s)
                    }
                })
                .on_press(Message::ResumeParked(i)),
        );
    }
    let shown = app.parked_newest_first.len().min(RESULT_ROWS_BEFORE_SCROLLING) as f32;
    let height = shown * RESULT_ROW_H + (shown - 1.0).max(0.0) * RESULT_GAP;
    let header = row![text("Open results").size(13).color(theme::TEXT_DIM)];
    column![header, scrollable(list).height(height).spacing(6)].spacing(8).width(600).into()
}

fn scanning(job: &ScanJob) -> Element<'_, Message> {
    let s = &job.snapshot;
    let stat = |label, n| {
        row![text(label).size(15).color(theme::TEXT_DIM), Space::new().width(Fill), text(thousands(n)).size(15)]
            .width(220)
    };
    let paused = job.cancel.is_paused();
    let title = if paused {
        format!("Paused: {}", job.opts.display_name())
    } else {
        format!("Scanning {}…", job.opts.display_name())
    };
    let mut col = column![
        text(title).size(26),
        Space::new().height(8),
        text(format_size(s.bytes)).size(44).color(theme::kind_color(Kind::Video)),
        column![stat("Files", s.files), stat("Folders", s.dirs)].spacing(4),
    ]
    .spacing(8)
    .align_x(Alignment::Center);
    if let Some(expected) = s.expected {
        let frac = (s.bytes as f32 / expected as f32).min(0.99);
        col = col.push(container(progress_bar(0.0..=1.0, frac).girth(6)).width(420));
    }
    let buttons = row![
        button(text(if paused { "Resume" } else { "Pause" }).size(14))
            .padding([8, 18])
            .style(filled_white_text_button(theme::ACCENT))
            .on_press(Message::TogglePause),
        button(text("Cancel").size(14))
            .padding([8, 18])
            .style(filled_white_text_button(theme::NEUTRAL_BUTTON))
            .on_press(Message::CancelScan),
    ]
    .spacing(10);
    col = col.push(Space::new().height(12)).push(buttons);
    center(col).into()
}

fn browse(b: &Browser) -> Element<'_, Message> {
    let main = column![toolbar(b), row![chart_panel(b), list_panel(b)].spacing(12).height(Fill), status_bar(b)]
        .spacing(10)
        .padding(12);

    match &b.drag {
        Some(d) if d.dragging => {
            let label = format!("{}  {}", truncate(&b.tree.name(d.item), 40), format_size(b.size(d.item)));
            let chip = container(text(label).size(13))
                .padding([6, 10])
                .style(|_| rounded_box(theme::ACCENT.scale_alpha(0.92), 8.0).color(Color::WHITE));
            stack![main, pin(chip).x(d.pos.x + 14.0).y(d.pos.y + 10.0)].into()
        }
        _ => main.into(),
    }
}

const SEARCH_PLACEHOLDER_WITH_SHORTCUT: &str =
    if cfg!(target_os = "macos") { "Search  ⌘F" } else { "Search  Ctrl+F" };

const BACK_TOOLTIP: &str = if cfg!(target_os = "macos") { "Back  ⌘[" } else { "Back  Ctrl+[" };
const FORWARD_TOOLTIP: &str = if cfg!(target_os = "macos") { "Forward  ⌘]" } else { "Forward  Ctrl+]" };

const NAV_ICON_SIZE: f32 = 16.0;
const DISABLED_ICON_STRENGTH: f32 = 0.4;

fn nav_icon_button<'a>(
    icon: fn(f32, Color) -> Svg<'a, Theme>,
    tooltip_label: &'a str,
    msg: Option<Message>,
) -> Element<'a, Message> {
    let tint =
        if msg.is_some() { theme::TEXT } else { theme::mix(theme::PANEL, theme::TEXT_DIM, DISABLED_ICON_STRENGTH) };
    let button = button(center(icon(NAV_ICON_SIZE, tint)))
        .width(INNER_BUTTON_SIDE)
        .height(INNER_BUTTON_SIDE)
        .padding(0)
        .style(quiet_button_style(theme::TEXT))
        .on_press_maybe(msg);
    with_tooltip(button, tooltip_label)
}

fn toolbar(b: &Browser) -> Element<'_, Message> {
    let nav = framed_group(
        row![
            nav_icon_button(icon::home, "Start page", Some(Message::GoHome)),
            nav_icon_button(icon::chevron_left, BACK_TOOLTIP, b.can_back().then_some(Message::Back)),
            nav_icon_button(icon::chevron_right, FORWARD_TOOLTIP, b.can_forward().then_some(Message::Forward)),
        ]
        .spacing(2),
    );

    let search = text_input(SEARCH_PLACEHOLDER_WITH_SHORTCUT, &b.search_text)
        .id(SEARCH_ID)
        .on_input(Message::SearchChanged)
        .padding([6, 10])
        .size(13)
        .width(220)
        .style(framed_text_input);

    row![nav, path_bar(b), outlined_text_button("Rescan", Message::Rescan), search]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}

fn framed_text_input(t: &Theme, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });
    text_input::Style {
        background: theme::PANEL.into(),
        border: border::rounded(FRAME_RADIUS).width(1).color(if focused { theme::ACCENT } else { theme::PANEL_HI }),
        ..text_input::default(t, status)
    }
}

const PATH_ROOT_ICON_SIZE: f32 = 14.0;
const PATH_SEPARATOR_SIZE: f32 = 12.0;
const PATH_SEPARATOR_STRENGTH: f32 = 0.6;
const CRUMB_NAME_MAX_CHARS: usize = 28;
const BREADCRUMBS_AFTER_ELLIPSIS: usize = 5;

fn path_bar(b: &Browser) -> Element<'_, Message> {
    let root_icon = if b.opts.whole_disk { icon::drive } else { icon::folder };
    let content = row![
        container(root_icon(PATH_ROOT_ICON_SIZE, theme::TEXT_DIM)).padding([0, 6]),
        root_and_last_ancestors_breadcrumbs(b),
    ]
    .align_y(Alignment::Center);
    framed_group(content).padding([FRAME_INSET, 2.0 * FRAME_INSET]).width(Fill).clip(true).into()
}

fn path_separator<'a>() -> Element<'a, Message> {
    icon::chevron_right(PATH_SEPARATOR_SIZE, theme::mix(theme::PANEL, theme::TEXT_DIM, PATH_SEPARATOR_STRENGTH)).into()
}

fn crumb<'a>(label: String, msg: Option<Message>) -> Element<'a, Message> {
    let idle_text = if msg.is_some() { theme::TEXT_DIM } else { theme::TEXT };
    button(text(label).size(13).wrapping(Wrapping::None))
        .padding([3, 6])
        .style(quiet_button_style(idle_text))
        .on_press_maybe(msg)
        .into()
}

fn root_and_last_ancestors_breadcrumbs(b: &Browser) -> Element<'_, Message> {
    let mut root_to_focus: Vec<NodeId> = b.tree.ancestors(b.focus).collect();
    root_to_focus.reverse();
    let crumb_for = |id: NodeId| {
        crumb(truncate(&b.tree.name(id), CRUMB_NAME_MAX_CHARS), (id != b.focus).then_some(Message::Navigate(id)))
    };

    let first_after_ellipsis = root_to_focus.len().saturating_sub(BREADCRUMBS_AFTER_ELLIPSIS).max(1);
    let mut crumbs = row![crumb_for(root_to_focus[0])].align_y(Alignment::Center);
    if first_after_ellipsis > 1 {
        let deepest_hidden = root_to_focus[first_after_ellipsis - 1];
        crumbs = crumbs.push(path_separator()).push(crumb("…".to_string(), Some(Message::Navigate(deepest_hidden))));
    }
    for &id in &root_to_focus[first_after_ellipsis..] {
        crumbs = crumbs.push(path_separator()).push(crumb_for(id));
    }
    crumbs.into()
}

fn chart_panel(b: &Browser) -> Element<'_, Message> {
    let chart = canvas(Sunburst {
        tree: &b.tree,
        layout: &b.layout,
        chart_cache: &b.chart_cache,
        highlight_cache: &b.highlight_cache,
        hovered: b.hovered,
        selected: b.selected,
        moving: b.animation.as_ref().map(ChartAnimation::current_frame),
    })
    .width(Fill)
    .height(Fill);

    column![chart, collector(b)].spacing(10).width(Length::FillPortion(3)).into()
}

const COLLECTOR_FIXED_HEIGHT: f32 = 72.0;
const COLLECTED_CHIPS_SHOWN: usize = 2;
const COLLECTED_CHIP_NAME_CHARS: usize = 16;
const HOLD_BUTTON_WIDTH: f32 = 228.0;
const HOLD_BUTTON_HEIGHT: f32 = 29.0;

fn hold_to_delete_label(progress: Option<f32>) -> String {
    match progress {
        None => format!("Hold {} s to delete permanently", HOLD_TO_DELETE.as_secs()),
        Some(done) => format!("Keep holding… {:.1} s", HOLD_TO_DELETE.as_secs_f32() * (1.0 - done)),
    }
}

fn hold_to_delete_button(b: &Browser) -> Element<'_, Message> {
    let progress = b.delete_hold_progress();
    let held_part = container(Space::new())
        .width(HOLD_BUTTON_WIDTH * progress.unwrap_or(0.0))
        .height(Fill)
        .style(|_| rounded_box(theme::DANGER, 6.0));
    let label = center(text(hold_to_delete_label(progress)).size(13).color(Color::WHITE));
    let face = container(stack![held_part, label].width(Fill).height(Fill))
        .width(HOLD_BUTTON_WIDTH)
        .height(HOLD_BUTTON_HEIGHT)
        .style(|_| {
            rounded_box(theme::mix(theme::DANGER, theme::PANEL, 0.65), 6.0)
                .border(border::rounded(6).width(1).color(theme::DANGER))
        });
    mouse_area(face)
        .on_press(Message::DeleteHoldStart)
        .on_release(Message::DeleteHoldCancel)
        .on_exit(Message::DeleteHoldCancel)
        .interaction(iced::mouse::Interaction::Pointer)
        .into()
}

fn collector(b: &Browser) -> Element<'_, Message> {
    let dragging = b.drag.as_ref().is_some_and(|d| d.dragging);
    let hot = dragging && b.over_collector;
    let ring_color = if hot { theme::ACCENT } else { theme::TEXT_DIM };
    let collected = &b.collector;

    let icon = container(dot(if collected.is_empty() { theme::PANEL_HI } else { theme::DANGER }, 18.0))
        .padding(8)
        .style(move |_| {
            container::Style::default().border(border::rounded(20).width(2).color(ring_color.scale_alpha(0.7)))
        });

    let body: Element<'_, Message> = if b.confirm_trash {
        row![
            text(format!(
                "Move {} item(s), {}, to the {TRASH_NAME}?",
                collected.len(),
                format_size(collected.size(&b.tree))
            ))
            .size(14),
            Space::new().width(Fill),
            button(text("Cancel").size(13))
                .style(filled_white_text_button(theme::NEUTRAL_BUTTON))
                .on_press(Message::CancelTrash),
            button(text(format!("Move to {TRASH_NAME}")).size(13))
                .style(filled_white_text_button(theme::DANGER))
                .on_press(Message::ConfirmTrash),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    } else if collected.is_empty() {
        text(if dragging { "Drop here to collect" } else { "Drag items here (or press C) to collect them" })
            .size(14)
            .color(if hot { theme::TEXT } else { theme::TEXT_DIM })
            .into()
    } else {
        let mut items = row![].spacing(6).align_y(Alignment::Center);
        for &id in collected.items().iter().take(COLLECTED_CHIPS_SHOWN) {
            items = items.push(
                container(
                    row![
                        dot(theme::kind_color(b.tree.node(id).kind()), 8.0),
                        text(truncate(&b.tree.name(id), COLLECTED_CHIP_NAME_CHARS)).size(12),
                        button(text("×").size(12)).padding([0, 4]).style(button::text).on_press(Message::Uncollect(id)),
                    ]
                    .spacing(5)
                    .align_y(Alignment::Center),
                )
                .padding([2, 8])
                .style(|_| rounded_box(theme::PANEL_HI, 10.0)),
            );
        }
        if collected.len() > COLLECTED_CHIPS_SHOWN {
            let hidden = collected.len() - COLLECTED_CHIPS_SHOWN;
            items = items.push(text(format!("+{hidden} more")).size(12).color(theme::TEXT_DIM));
        }
        row![
            container(
                column![
                    text(format!("{} collected · {}", collected.len(), format_size(collected.size(&b.tree)))).size(14),
                    items
                ]
                .spacing(4)
            )
            .width(Fill)
            .clip(true),
            button(text("Clear").size(13))
                .style(filled_white_text_button(theme::NEUTRAL_BUTTON))
                .on_press(Message::ClearCollector),
            button(text(format!("Move to {TRASH_NAME}…")).size(13))
                .style(filled_white_text_button(theme::DANGER))
                .on_press(Message::AskTrash),
            hold_to_delete_button(b),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    };

    let zone = container(row![icon, body].spacing(12).height(Fill).align_y(Alignment::Center))
        .padding(10)
        .width(Fill)
        .height(COLLECTOR_FIXED_HEIGHT)
        .style(move |_| {
            let bg = if hot { theme::ACCENT.scale_alpha(0.18) } else { theme::PANEL };
            let edge = if dragging { 1.5 } else { 0.0 };
            rounded_box(bg, 12.0).border(border::rounded(12).width(edge).color(theme::ACCENT.scale_alpha(0.8)))
        });
    mouse_area(zone).on_enter(Message::CollectorEnter).on_exit(Message::CollectorExit).into()
}

fn list_panel(b: &Browser) -> Element<'_, Message> {
    let subject = b.listed_folder();
    let t = &b.tree;

    let mut header = column![
        row![
            dot(theme::kind_color(t.node(subject).kind()), 14.0),
            text(truncate(&t.name(subject), 34)).size(22).wrapping(Wrapping::None),
            Space::new().width(Fill),
            text(format_size(b.size(subject))).size(22),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(4);
    if let Some(s) = &b.search {
        let search_summary = format!("{} matches for “{}”", thousands(s.matches as u64), s.query);
        header = header.push(text(search_summary).size(12).color(theme::TEXT_DIM).wrapping(Wrapping::None));
    }

    let mut rows = Column::new().spacing(1);
    for r in &b.list.rows {
        rows = rows.push(list_row(b, r));
    }
    if let Some(more) = &b.list.cut_off_rows_summary {
        rows = rows.push(container(text(more.as_str()).size(12).color(theme::TEXT_DIM)).padding([6, 10]));
    }
    if b.list.rows.is_empty() && t.node(subject).is_dir() {
        rows = rows.push(container(text("Empty").size(13).color(theme::TEXT_DIM)).padding(10));
    }

    container(column![header, scrollable(rows).height(Fill).spacing(4)].spacing(12))
        .padding(14)
        .width(Length::FillPortion(2))
        .height(Fill)
        .style(|_| rounded_box(theme::PANEL, 10.0))
        .into()
}

fn list_row<'a>(b: &'a Browser, r: &'a ListRow) -> Element<'a, Message> {
    let id = r.id;
    let n = b.tree.node(id);
    let kind_color = theme::kind_color(n.kind());
    let dim = n.is_synthetic() || n.is_unreadable();

    let bar = container(Space::new())
        .width(Length::Fixed((r.fraction_of_listed_folder * 60.0).max(1.0)))
        .height(3)
        .style(move |_| container::background(kind_color.scale_alpha(0.7)));

    let mut content = row![
        dot(kind_color, 8.0),
        text(r.name.as_str())
            .size(13)
            .color(if dim { theme::TEXT_DIM } else { theme::TEXT })
            .wrapping(Wrapping::None)
            .width(Fill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if b.collector.contains(id) {
        content = content.push(text("in collector").size(11).color(theme::DANGER));
    }
    let content = content
        .push(container(bar).width(64).align_x(Alignment::End))
        .push(text(r.size.as_str()).size(13).width(Length::Fixed(78.0)).align_x(Alignment::End));

    let bg = if b.selected == Some(id) {
        Some(theme::ACCENT.scale_alpha(0.35))
    } else if b.hovered == Some(Segment::Node(id)) {
        Some(theme::PANEL_HI)
    } else if b.search.as_ref().is_some_and(|s| s.is_match(id)) {
        Some(theme::ACCENT.scale_alpha(0.12))
    } else {
        None
    };
    let cell = container(content).padding([4, 8]).width(Fill).style(move |_| {
        let style = container::Style::default().border(border::rounded(6));
        match bg {
            Some(c) => style.background(c),
            None => style,
        }
    });
    mouse_area(cell)
        .on_enter(Message::Hover(Some(Segment::Node(id))))
        .on_exit(Message::RowExit(id))
        .on_press(Message::Press(Segment::Node(id)))
        .on_double_click(Message::Activate(Segment::Node(id)))
        .into()
}

fn status_bar(b: &Browser) -> Element<'_, Message> {
    let t = &b.tree;
    let info = if let Some(s) = &b.status {
        row![
            text(s.as_str()).size(12).color(theme::TEXT),
            button(text("×").size(12)).padding([0, 6]).style(button::text).on_press(Message::DismissStatus)
        ]
        .spacing(6)
        .align_y(Alignment::Center)
    } else {
        let mut parts = format!(
            "{} files in {} folders scanned in {:.1} s",
            thousands(t.stats.files),
            thousands(t.stats.dirs),
            t.stats.duration.as_secs_f32()
        );
        if t.is_whole_disk() {
            parts.push_str(&format!(
                " · hidden {} · purgeable {}",
                format_size(t.stats.hidden),
                format_size(t.stats.purgeable)
            ));
        }
        row![text(parts).size(12).color(theme::TEXT_DIM)]
    };
    let mut bar = row![info].spacing(12).align_y(Alignment::Center).height(Shrink);
    if t.stats.errors > 0 {
        let mut unreadable = row![
            text(format!("{} folders couldn't be read", thousands(t.stats.errors)))
                .size(12)
                .color(theme::kind_color(Kind::Archive)),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if dscan_core::FULL_DISK_ACCESS_SETTINGS.is_some() {
            unreadable =
                unreadable.push(subtle_button(text("Full Disk Access…").size(12), Some(Message::OpenFdaSettings)));
        }
        bar = bar.push(Space::new().width(Fill)).push(unreadable);
    }
    bar.into()
}
