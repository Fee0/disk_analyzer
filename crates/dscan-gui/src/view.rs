use dscan_core::{
    Kind, NodeId, PREVIEW_ACTION, REVEAL_ACTION, Remedy, Segment, SpaceExplanation, TRASH_NAME, Volume, format_size,
};
use iced::border;
use iced::widget::text::Wrapping;
use iced::widget::{
    Column, Space, Svg, button, canvas, center, column, container, mouse_area, pin, responsive, row, scrollable, stack,
    text, text_input, tooltip,
};
use iced::{Alignment, Color, Element, Fill, Length, Theme};

use crate::animation::ChartAnimation;
use crate::app::{App, Browser, HOLD_TO_DELETE, ListRow, Message, SEARCH_ID, ScanJob, Screen};
use crate::fmt::{fit_width, fit_width_keeping_both_ends, generous_text_width, thousands, truncate};
use crate::icon;
use crate::meter::Meter;
use crate::sunburst::Sunburst;
use crate::theme;

pub const DEFAULT_WINDOW_SIZE: iced::Size = iced::Size::new(1280.0, 820.0);
pub const MIN_WINDOW_SIZE: iced::Size = iced::Size::new(860.0, 560.0);

pub fn view(app: &App) -> Element<'_, Message> {
    let content = match &app.screen {
        Screen::Start => start(app),
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
        .padding([4.0, SUBTLE_BUTTON_PADDING_X])
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
    button(text(label).size(13).wrapping(Wrapping::None))
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

fn tooltip_box<'a>(label: impl text::IntoFragment<'a>) -> container::Container<'a, Message> {
    container(text(label).size(12))
        .padding([4, 8])
        .max_width(TOOLTIP_MAX_WIDTH)
        .style(|_| rounded_box(theme::PANEL_HI, 6.0))
}

fn with_tooltip<'a>(content: impl Into<Element<'a, Message>>, label: &'a str) -> Element<'a, Message> {
    tooltip(content, tooltip_box(label), tooltip::Position::Bottom).gap(4).into()
}

fn icon_button<'a>(
    icon: impl Into<Element<'a, Message>>,
    tooltip_label: &'a str,
    msg: Message,
) -> Element<'a, Message> {
    with_tooltip(subtle_button(icon, Some(msg)).padding(ICON_BUTTON_PADDING), tooltip_label)
}

const START_PADDING: f32 = 24.0;
const START_BLOCK_MAX_WIDTH: f32 = 720.0;
const START_SECTION_GAP: f32 = 14.0;
const DISK_LIST_PADDING: f32 = 4.0;
const DISK_LIST_RADIUS: f32 = 12.0;
const DISK_ROW_RADIUS: f32 = DISK_LIST_RADIUS - DISK_LIST_PADDING;
const DISK_ROW_HEIGHT: f32 = 76.0;
const DISK_ROW_PADDING_X: f32 = 18.0;
const DISK_ROW_SPACING: f32 = 18.0;
const DISK_ICON_SIZE: f32 = 34.0;
const DISK_NAME_SIZE: f32 = 17.0;
const DISK_DETAILS_SIZE: f32 = 13.0;
const DISK_TEXT_SPACING: f32 = 4.0;
const METER_WIDTH: f32 = 200.0;
const METER_HEIGHT: f32 = 6.0;
const METER_LABEL_SIZE: f32 = 13.0;
const METER_LABEL_GAP: f32 = 10.0;
const SCAN_CONTROLS_WIDTH: f32 = 116.0;
const SCAN_BUTTON_WIDTH: f32 = 76.0;
const SCAN_BUTTON_TEXT_SIZE: f32 = 14.0;
const SCAN_CONTROLS_SPACING: f32 = 6.0;
const PAUSE_ICON_SIZE: f32 = 14.0;
const ALTERNATE_ROW_SHADE: f32 = 0.45;
const FILLING_UP: f32 = 0.75;
const NEARLY_FULL: f32 = 0.9;
const SCAN_SHOWN_COMPLETE_ONLY_WHEN_DONE: f32 = 0.99;
const PAUSED_SCAN_STRENGTH: f32 = 0.5;
const SCAN_FOLDER_LABEL: &str = "Scan Folder…";
const SCAN_LABEL: &str = "Scan";
const STOP_LABEL: &str = "Stop";

fn start(app: &App) -> Element<'_, Message> {
    responsive(move |viewport| {
        let width = (viewport.width - 2.0 * START_PADDING).min(START_BLOCK_MAX_WIDTH);
        scrollable(center(start_content(app, width)).padding(START_PADDING)).width(Fill).height(viewport.height).into()
    })
    .into()
}

fn start_content(app: &App, width: f32) -> Column<'_, Message> {
    let footer = row![
        outlined_text_button(SCAN_FOLDER_LABEL, Message::OpenFolder),
        Space::new().width(Fill),
        icon_button(icon::refresh(14.0, theme::TEXT_DIM), "Refresh disks", Message::RefreshVolumes),
    ]
    .align_y(Alignment::Center);

    let mut col = column![disk_list(app, width), footer].spacing(START_SECTION_GAP).width(width);

    if !app.parked_newest_first.is_empty() {
        col = col.push(Space::new().height(START_SECTION_GAP)).push(open_results(app));
    }

    if !app.full_disk_access {
        let warn = theme::kind_color(Kind::Archive);
        col = col.push(Space::new().height(START_SECTION_GAP)).push(
            container(
                row![
                    text("Without Full Disk Access some folders can't be read, so some space shows up as hidden. Grant it to your terminal (or this app), then restart it.")
                        .size(13)
                        .width(Fill),
                    button(text("Open Settings").size(13).wrapping(Wrapping::None)).style(filled_white_text_button(theme::ACCENT)).on_press(Message::OpenFdaSettings),
                ]
                .spacing(16)
                .align_y(Alignment::Center),
            )
            .padding(14)
            .width(Fill)
            .style(move |_| rounded_box(warn.scale_alpha(0.15), 10.0).border(border::rounded(10).width(1).color(warn.scale_alpha(0.5)))),
        );
    }
    if let Some(err) = &app.error {
        col = col.push(text(err.clone()).color(theme::DANGER).size(14));
    }
    col
}

fn disk_list(app: &App, width: f32) -> Element<'_, Message> {
    let scan = app.scan.as_ref();
    let text_width = disk_text_width(width);
    let mut rows: Vec<Element<'_, Message>> = Vec::new();
    match &app.volumes {
        None => rows.push(
            container(text("Loading disks…").size(14).color(theme::TEXT_DIM))
                .height(DISK_ROW_HEIGHT)
                .padding([0.0, DISK_ROW_PADDING_X])
                .align_y(Alignment::Center)
                .into(),
        ),
        Some(volumes) => {
            for (i, v) in volumes.iter().enumerate() {
                let identity = DiskIdentity {
                    icon: icon::drive(DISK_ICON_SIZE, theme::TEXT_DIM),
                    name: v.name.clone(),
                    details: format!("{} · {}", format_size(v.total), volume_details(v)),
                };
                rows.push(match scan.filter(|job| job.scans_volume(v)) {
                    Some(job) => scanning_row(identity, job, text_width),
                    None => volume_row(identity, i, v, text_width),
                });
            }
        }
    }
    let scans_a_listed_volume = |job: &ScanJob| app.volumes.iter().flatten().any(|v| job.scans_volume(v));
    if let Some(job) = scan.filter(|job| !scans_a_listed_volume(job)) {
        let identity = DiskIdentity {
            icon: if job.opts.whole_disk {
                icon::drive(DISK_ICON_SIZE, theme::TEXT_DIM)
            } else {
                icon::folder(DISK_ICON_SIZE, theme::TEXT_DIM)
            },
            name: job.opts.display_name(),
            details: job.opts.root.display().to_string(),
        };
        rows.push(scanning_row(identity, job, text_width));
    }
    let shaded_rows = rows.into_iter().enumerate().map(|(n, content)| {
        let shade =
            if n % 2 == 1 { theme::mix(theme::PANEL, theme::PANEL_HI, ALTERNATE_ROW_SHADE) } else { theme::PANEL };
        container(content).width(Fill).style(move |_| rounded_box(shade, DISK_ROW_RADIUS)).into()
    });
    container(Column::with_children(shaded_rows))
        .padding(DISK_LIST_PADDING)
        .width(Fill)
        .style(|_| rounded_box(theme::PANEL, DISK_LIST_RADIUS))
        .into()
}

fn disk_text_width(list_width: f32) -> f32 {
    list_width
        - 2.0 * DISK_LIST_PADDING
        - 2.0 * DISK_ROW_PADDING_X
        - DISK_ICON_SIZE
        - 3.0 * DISK_ROW_SPACING
        - METER_WIDTH
        - SCAN_CONTROLS_WIDTH
}

struct DiskIdentity<'a> {
    icon: Svg<'a, Theme>,
    name: String,
    details: String,
}

fn disk_row<'a>(
    identity: DiskIdentity<'a>,
    meter: Meter,
    label: text::Text<'a, Theme>,
    controls: impl Into<Element<'a, Message>>,
    text_width: f32,
) -> Element<'a, Message> {
    row![
        identity.icon,
        column![
            text(fit_width(&identity.name, text_width, DISK_NAME_SIZE)).size(DISK_NAME_SIZE).wrapping(Wrapping::None),
            text(fit_width_keeping_both_ends(&identity.details, text_width, DISK_DETAILS_SIZE))
                .size(DISK_DETAILS_SIZE)
                .color(theme::TEXT_DIM)
                .wrapping(Wrapping::None),
        ]
        .spacing(DISK_TEXT_SPACING)
        .width(Fill)
        .clip(true),
        column![
            canvas(meter).width(METER_WIDTH).height(METER_HEIGHT),
            label.size(METER_LABEL_SIZE).wrapping(Wrapping::None)
        ]
        .spacing(METER_LABEL_GAP)
        .width(METER_WIDTH)
        .align_x(Alignment::End),
        container(controls).width(SCAN_CONTROLS_WIDTH).align_x(Alignment::End),
    ]
    .spacing(DISK_ROW_SPACING)
    .align_y(Alignment::Center)
    .padding([0.0, DISK_ROW_PADDING_X])
    .height(DISK_ROW_HEIGHT)
    .into()
}

fn fullness_color(used_fraction: f32) -> Color {
    let kind = if used_fraction >= NEARLY_FULL {
        Kind::DiskImage
    } else if used_fraction >= FILLING_UP {
        Kind::Archive
    } else {
        Kind::Video
    };
    theme::kind_color(kind)
}

fn scan_button<'a>(label: &'a str, msg: Message) -> Element<'a, Message> {
    button(text(label).size(SCAN_BUTTON_TEXT_SIZE).width(Fill).align_x(Alignment::Center).wrapping(Wrapping::None))
        .padding([6.0, 0.0])
        .width(SCAN_BUTTON_WIDTH)
        .style(filled_white_text_button(theme::NEUTRAL_BUTTON))
        .on_press(msg)
        .into()
}

fn volume_row<'a>(identity: DiskIdentity<'a>, index: usize, v: &Volume, text_width: f32) -> Element<'a, Message> {
    let used_fraction = if v.total > 0 { v.used as f32 / v.total as f32 } else { 0.0 };
    let color = fullness_color(used_fraction);
    let label = text(format!("{} free", format_size(v.available))).color(color);
    let meter = Meter { fraction: used_fraction, color };
    disk_row(identity, meter, label, scan_button(SCAN_LABEL, Message::ScanVolume(index)), text_width)
}

fn scanning_row<'a>(identity: DiskIdentity<'a>, job: &ScanJob, text_width: f32) -> Element<'a, Message> {
    let s = &job.snapshot;
    let fraction =
        s.expected.map_or(0.0, |expected| (s.bytes as f32 / expected as f32).min(SCAN_SHOWN_COMPLETE_ONLY_WHEN_DONE));
    let paused = job.cancel.is_paused();
    let (meter, label) = if paused {
        let dimmed = theme::mix(theme::BG, theme::ACCENT, PAUSED_SCAN_STRENGTH);
        (Meter { fraction, color: dimmed }, format!("Paused · {}", format_size(s.bytes)))
    } else {
        (Meter { fraction, color: theme::ACCENT }, format!("{} · {} files", format_size(s.bytes), thousands(s.files)))
    };
    let (pause_icon, pause_tooltip) = if paused {
        (icon::resume(PAUSE_ICON_SIZE, theme::TEXT), "Resume")
    } else {
        (icon::pause(PAUSE_ICON_SIZE, theme::TEXT), "Pause")
    };
    let controls = row![
        icon_button(pause_icon, pause_tooltip, Message::TogglePause),
        scan_button(STOP_LABEL, Message::CancelScan)
    ]
    .spacing(SCAN_CONTROLS_SPACING)
    .align_y(Alignment::Center);
    disk_row(identity, meter, text(label).color(theme::TEXT), controls, text_width)
}

const NETWORK_VOLUME_NOTE: &str = "network";

fn volume_details(v: &Volume) -> String {
    let mut details = v.mount.display().to_string();
    for fact in [v.file_system.as_str(), if v.is_network { NETWORK_VOLUME_NOTE } else { "" }] {
        if !fact.is_empty() {
            details.push_str(" · ");
            details.push_str(fact);
        }
    }
    details
}

const RESULT_ROW_H: f32 = 56.0;
const RESULT_GAP: f32 = 6.0;
const RESULT_ROWS_BEFORE_SCROLLING: usize = 4;
const RESULT_ROW_PADDING_X: f32 = 12.0;
const RESULT_ROW_SPACING: f32 = 12.0;
const RESULT_DOT: f32 = 10.0;
const RESULT_SIZE_COLUMN_WIDTH: f32 = 84.0;
const RESULT_CLOSE_ICON: f32 = 12.0;
const ICON_BUTTON_PADDING: f32 = 6.0;
const DEFAULT_SCROLLBAR_WIDTH: f32 = 10.0;
const RESULT_LIST_SCROLLBAR_GAP: f32 = 6.0;

fn open_results(app: &App) -> Element<'_, Message> {
    let shown = app.parked_newest_first.len().min(RESULT_ROWS_BEFORE_SCROLLING) as f32;
    let height = shown * RESULT_ROW_H + (shown - 1.0).max(0.0) * RESULT_GAP;
    let header = row![text("Open results").size(13).color(theme::TEXT_DIM)];
    let list = container(responsive(move |size| open_result_rows(app, size.width))).height(height);
    column![header, list].spacing(8).width(Fill).max_width(START_BLOCK_MAX_WIDTH).into()
}

fn open_result_name_width(list_width: f32, scrolls: bool) -> f32 {
    let close_button = RESULT_CLOSE_ICON + 2.0 * ICON_BUTTON_PADDING;
    let scrollbar = if scrolls { DEFAULT_SCROLLBAR_WIDTH + RESULT_LIST_SCROLLBAR_GAP } else { 0.0 };
    let fixed = 2.0 * RESULT_ROW_PADDING_X
        + RESULT_DOT
        + 3.0 * RESULT_ROW_SPACING
        + RESULT_SIZE_COLUMN_WIDTH
        + close_button
        + scrollbar;
    list_width - fixed
}

fn open_result_rows(app: &App, list_width: f32) -> Element<'_, Message> {
    let scrolls = app.parked_newest_first.len() > RESULT_ROWS_BEFORE_SCROLLING;
    let name_width = open_result_name_width(list_width, scrolls);
    let mut list = Column::new().spacing(RESULT_GAP);
    for (i, b) in app.parked_newest_first.iter().enumerate() {
        let t = &b.tree;
        let root = t.root();
        let meta = format!("{} files", thousands(t.node(root).items() as u64));
        let content = row![
            dot(theme::kind_color(t.node(root).kind()), RESULT_DOT),
            column![
                text(fit_width(&t.name(root), name_width, 14.0)).size(14).wrapping(Wrapping::None),
                text(fit_width_keeping_both_ends(&t.root_path().display().to_string(), name_width, 12.0))
                    .size(12)
                    .color(theme::TEXT_DIM)
                    .wrapping(Wrapping::None),
            ]
            .spacing(2)
            .width(Fill)
            .clip(true),
            column![
                text(format_size(t.size(root))).size(14).wrapping(Wrapping::None),
                text(meta).size(12).color(theme::TEXT_DIM).wrapping(Wrapping::None),
            ]
            .spacing(2)
            .width(RESULT_SIZE_COLUMN_WIDTH)
            .align_x(Alignment::End),
            icon_button(icon::close(RESULT_CLOSE_ICON, theme::TEXT_DIM), "Close", Message::CloseParked(i)),
        ]
        .spacing(RESULT_ROW_SPACING)
        .height(Fill)
        .align_y(Alignment::Center);
        list = list.push(
            button(content)
                .padding([0.0, RESULT_ROW_PADDING_X])
                .width(Fill)
                .height(RESULT_ROW_H)
                .style(|t: &Theme, s| {
                    let bg = if matches!(s, button::Status::Hovered) { theme::PANEL_HI } else { theme::PANEL };
                    button::Style {
                        background: Some(bg.into()),
                        text_color: if matches!(s, button::Status::Disabled) { theme::TEXT_DIM } else { theme::TEXT },
                        border: border::rounded(10),
                        ..button::secondary(t, s)
                    }
                })
                .on_press_maybe((!app.is_scanning()).then_some(Message::ResumeParked(i))),
        );
    }
    scrollable(list).height(Fill).spacing(RESULT_LIST_SCROLLBAR_GAP).into()
}

const PANEL_GAP: f32 = 12.0;
const CHART_SIDE_PORTION: u16 = 3;
const LIST_SIDE_PORTION: u16 = 2;
pub const TOOLBAR_LIST_SIDE_ID: &str = "toolbar-list-side";
pub const LIST_PANEL_ID: &str = "list-panel";
pub const PATH_BAR_ID: &str = "path-bar";

fn browse(b: &Browser) -> Element<'_, Message> {
    let main = column![toolbar(b), row![chart_panel(b), list_panel(b)].spacing(PANEL_GAP).height(Fill), status_bar(b)]
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
        .width(Fill)
        .style(framed_text_input);

    let chart_side =
        row![nav, path_bar(b)].spacing(8).align_y(Alignment::Center).width(Length::FillPortion(CHART_SIDE_PORTION));
    let list_side =
        container(row![outlined_text_button("Rescan", Message::Rescan), search].spacing(8).align_y(Alignment::Center))
            .id(TOOLBAR_LIST_SIDE_ID)
            .width(Length::FillPortion(LIST_SIDE_PORTION));
    row![chart_side, list_side].spacing(PANEL_GAP).align_y(Alignment::Center).into()
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
        responsive(move |available| container(breadcrumbs_that_fit(b, available.width))
            .height(Fill)
            .align_y(Alignment::Center)
            .into()),
    ]
    .align_y(Alignment::Center);
    framed_group(content).id(PATH_BAR_ID).padding([FRAME_INSET, 2.0 * FRAME_INSET]).width(Fill).clip(true).into()
}

fn path_separator<'a>() -> Element<'a, Message> {
    icon::chevron_right(PATH_SEPARATOR_SIZE, theme::mix(theme::PANEL, theme::TEXT_DIM, PATH_SEPARATOR_STRENGTH)).into()
}

const CRUMB_TEXT_SIZE: f32 = 13.0;
const CRUMB_PADDING_X: f32 = 6.0;
const ELLIPSIS: &str = "…";

fn crumb<'a>(label: String, msg: Option<Message>) -> Element<'a, Message> {
    let idle_text = if msg.is_some() { theme::TEXT_DIM } else { theme::TEXT };
    button(text(label).size(CRUMB_TEXT_SIZE).wrapping(Wrapping::None))
        .padding([3.0, CRUMB_PADDING_X])
        .style(quiet_button_style(idle_text))
        .on_press_maybe(msg)
        .into()
}

fn generous_crumb_width(label: &str) -> f32 {
    generous_text_width(label, CRUMB_TEXT_SIZE) + 2.0 * CRUMB_PADDING_X
}

fn crumbs_after_the_root_that_fit(root_to_focus_labels: &[String], available_width: f32) -> usize {
    let after_root = root_to_focus_labels.len() - 1;
    let width_with_last = |shown: usize| {
        let ellipsis = if shown < after_root { PATH_SEPARATOR_SIZE + generous_crumb_width(ELLIPSIS) } else { 0.0 };
        let tail: f32 = root_to_focus_labels[root_to_focus_labels.len() - shown..]
            .iter()
            .map(|label| PATH_SEPARATOR_SIZE + generous_crumb_width(label))
            .sum();
        generous_crumb_width(&root_to_focus_labels[0]) + ellipsis + tail
    };
    let most = after_root.min(BREADCRUMBS_AFTER_ELLIPSIS);
    (1..=most).rev().find(|&shown| width_with_last(shown) <= available_width).unwrap_or(most.min(1))
}

fn breadcrumbs_that_fit(b: &Browser, available_width: f32) -> Element<'_, Message> {
    let mut root_to_focus: Vec<NodeId> = b.tree.ancestors(b.focus).collect();
    root_to_focus.reverse();
    let labels: Vec<String> =
        root_to_focus.iter().map(|&id| truncate(&b.tree.name(id), CRUMB_NAME_MAX_CHARS)).collect();
    let link_unless_focus = |id: NodeId| (id != b.focus).then_some(Message::Navigate(id));

    let first_after_ellipsis = root_to_focus.len() - crumbs_after_the_root_that_fit(&labels, available_width);
    let mut shown = vec![(labels[0].clone(), link_unless_focus(root_to_focus[0]))];
    if first_after_ellipsis > 1 {
        let deepest_hidden = root_to_focus[first_after_ellipsis - 1];
        shown.push((ELLIPSIS.to_string(), Some(Message::Navigate(deepest_hidden))));
    }
    for (index, &id) in root_to_focus.iter().enumerate().skip(first_after_ellipsis) {
        shown.push((labels[index].clone(), link_unless_focus(id)));
    }
    let before_current: f32 =
        shown[..shown.len() - 1].iter().map(|(label, _)| generous_crumb_width(label) + PATH_SEPARATOR_SIZE).sum();
    if let Some((current, _)) = shown.last_mut() {
        *current = fit_width(current, available_width - before_current - 2.0 * CRUMB_PADDING_X, CRUMB_TEXT_SIZE);
    }

    let mut crumbs = row![].align_y(Alignment::Center);
    for (index, (label, msg)) in shown.into_iter().enumerate() {
        if index > 0 {
            crumbs = crumbs.push(path_separator());
        }
        crumbs = crumbs.push(crumb(label, msg));
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

    column![chart, collector(b)].spacing(10).width(Length::FillPortion(CHART_SIDE_PORTION)).into()
}

const COLLECTOR_FIXED_HEIGHT: f32 = 72.0;
const COLLECTOR_PADDING: f32 = 10.0;
const COLLECTOR_SPACING: f32 = 12.0;
const COLLECTOR_BUTTON_SPACING: f32 = 8.0;
const COLLECTOR_RING_DOT: f32 = 18.0;
const COLLECTOR_RING_PADDING: f32 = 8.0;
const COLLECTOR_RING_WIDTH: f32 = COLLECTOR_RING_DOT + 2.0 * COLLECTOR_RING_PADDING;
const COLLECTOR_TEXT_SIZE: f32 = 14.0;
const COLLECTOR_BUTTON_TEXT_SIZE: f32 = 13.0;
const DEFAULT_BUTTON_PADDING_X: f32 = 10.0;
const COLLECTED_CHIPS_SHOWN: usize = 2;
const COLLECTED_CHIP_NAME_CHARS: usize = 16;
const CHIP_TEXT_SIZE: f32 = 12.0;
const CHIP_DOT: f32 = 8.0;
const CHIP_PADDING_X: f32 = 8.0;
const CHIP_INNER_SPACING: f32 = 5.0;
const CHIP_REMOVE_PADDING_X: f32 = 4.0;
const CHIP_GAP: f32 = 6.0;
const HOLD_BUTTON_FULL_WIDTH: f32 = 228.0;
const HOLD_BUTTON_COMPACT_WIDTH: f32 = 120.0;
const HOLD_BUTTON_HEIGHT: f32 = 29.0;
const LINES_THAT_FIT_IN_THE_COLLECTOR: f32 = 2.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CollectorFit {
    Full,
    Compact,
    CompactWithoutRing,
}

impl CollectorFit {
    const WIDEST_FIRST: [Self; 3] = [Self::Full, Self::Compact, Self::CompactWithoutRing];

    fn ring_width(self) -> f32 {
        if self == Self::CompactWithoutRing { 0.0 } else { COLLECTOR_RING_WIDTH + COLLECTOR_SPACING }
    }

    fn hold_button_width(self) -> f32 {
        if self == Self::Full { HOLD_BUTTON_FULL_WIDTH } else { HOLD_BUTTON_COMPACT_WIDTH }
    }
}

fn generous_button_width(label: &str, text_size: f32) -> f32 {
    generous_text_width(label, text_size) + 2.0 * DEFAULT_BUTTON_PADDING_X
}

fn items_phrase(count: usize) -> String {
    if count == 1 { "1 item".to_string() } else { format!("{} items", thousands(count as u64)) }
}

fn hold_to_delete_label(progress: Option<f32>, fit: CollectorFit) -> String {
    let seconds_left = |done: f32| HOLD_TO_DELETE.as_secs_f32() * (1.0 - done);
    match (progress, fit) {
        (None, CollectorFit::Full) => hold_to_delete_explanation(),
        (None, _) => "Hold to delete".to_string(),
        (Some(done), CollectorFit::Full) => format!("Keep holding… {:.1} s", seconds_left(done)),
        (Some(done), _) => format!("Holding… {:.1} s", seconds_left(done)),
    }
}

fn hold_to_delete_explanation() -> String {
    format!("Hold {} s to delete permanently", HOLD_TO_DELETE.as_secs())
}

fn hold_to_delete_button(b: &Browser, fit: CollectorFit) -> Element<'_, Message> {
    let progress = b.delete_hold_progress();
    let width = fit.hold_button_width();
    let held_part = container(Space::new())
        .width(width * progress.unwrap_or(0.0))
        .height(Fill)
        .style(|_| rounded_box(theme::DANGER, 6.0));
    let label = center(
        text(hold_to_delete_label(progress, fit))
            .size(COLLECTOR_BUTTON_TEXT_SIZE)
            .color(Color::WHITE)
            .wrapping(Wrapping::None),
    );
    let face = container(stack![held_part, label].width(Fill).height(Fill))
        .width(width)
        .height(HOLD_BUTTON_HEIGHT)
        .style(|_| {
            rounded_box(theme::mix(theme::DANGER, theme::PANEL, 0.65), 6.0)
                .border(border::rounded(6).width(1).color(theme::DANGER))
        });
    let button = mouse_area(face)
        .on_press(Message::DeleteHoldStart)
        .on_release(Message::DeleteHoldCancel)
        .on_exit(Message::DeleteHoldCancel)
        .interaction(iced::mouse::Interaction::Pointer);
    if fit == CollectorFit::Full {
        button.into()
    } else {
        tooltip(button, tooltip_box(hold_to_delete_explanation()), tooltip::Position::Top).gap(4).into()
    }
}

fn collector(b: &Browser) -> Element<'_, Message> {
    let dragging = b.drag.as_ref().is_some_and(|d| d.dragging);
    let hot = dragging && b.over_collector;
    let zone = container(responsive(move |size| collector_content(b, size.width)))
        .padding(COLLECTOR_PADDING)
        .width(Fill)
        .height(COLLECTOR_FIXED_HEIGHT)
        .style(move |_| {
            let bg = if hot { theme::ACCENT.scale_alpha(0.18) } else { theme::PANEL };
            let edge = if dragging { 1.5 } else { 0.0 };
            rounded_box(bg, 12.0).border(border::rounded(12).width(edge).color(theme::ACCENT.scale_alpha(0.8)))
        });
    mouse_area(zone).on_enter(Message::CollectorEnter).on_exit(Message::CollectorExit).into()
}

fn collector_ring<'a>(b: &Browser) -> Element<'a, Message> {
    let hot = b.over_collector && b.drag.as_ref().is_some_and(|d| d.dragging);
    let ring_color = if hot { theme::ACCENT } else { theme::TEXT_DIM };
    let fill = if b.collector.is_empty() { theme::PANEL_HI } else { theme::DANGER };
    container(dot(fill, COLLECTOR_RING_DOT))
        .padding(COLLECTOR_RING_PADDING)
        .style(move |_| {
            container::Style::default().border(border::rounded(20).width(2).color(ring_color.scale_alpha(0.7)))
        })
        .into()
}

fn collector_content(b: &Browser, width: f32) -> Element<'_, Message> {
    let (fit, body) = if b.confirm_trash {
        trash_confirmation(b, width)
    } else if b.collector.is_empty() {
        (CollectorFit::Full, empty_collector_hint(b))
    } else {
        collected_items(b, width)
    };
    let mut content = row![].spacing(COLLECTOR_SPACING).height(Fill).align_y(Alignment::Center);
    if fit != CollectorFit::CompactWithoutRing {
        content = content.push(collector_ring(b));
    }
    content.push(body).into()
}

fn empty_collector_hint<'a>(b: &Browser) -> Element<'a, Message> {
    let dragging = b.drag.as_ref().is_some_and(|d| d.dragging);
    let hot = dragging && b.over_collector;
    text(if dragging { "Drop here to collect" } else { "Drag items here (or press C) to collect them" })
        .size(COLLECTOR_TEXT_SIZE)
        .color(if hot { theme::TEXT } else { theme::TEXT_DIM })
        .into()
}

fn collector_button<'a>(label: String, bg: Color, msg: Message) -> Element<'a, Message> {
    button(text(label).size(COLLECTOR_BUTTON_TEXT_SIZE).wrapping(Wrapping::None))
        .style(filled_white_text_button(bg))
        .on_press(msg)
        .into()
}

fn trash_confirmation(b: &Browser, width: f32) -> (CollectorFit, Element<'_, Message>) {
    let items = items_phrase(b.collector.len());
    let size = format_size(b.collector.size(&b.tree));
    let confirm_label = format!("Move to {TRASH_NAME}");
    let buttons = generous_button_width("Cancel", COLLECTOR_BUTTON_TEXT_SIZE)
        + generous_button_width(&confirm_label, COLLECTOR_BUTTON_TEXT_SIZE)
        + 2.0 * COLLECTOR_BUTTON_SPACING;
    let full_question = format!("Move {items}, {size}, to the {TRASH_NAME}?");
    let fits = |question: &str, fit: CollectorFit| {
        let room = width - fit.ring_width() - buttons - COLLECTOR_BUTTON_SPACING;
        generous_text_width(question, COLLECTOR_TEXT_SIZE) <= LINES_THAT_FIT_IN_THE_COLLECTOR * room
    };
    let (fit, question) = if fits(&full_question, CollectorFit::Full) {
        (CollectorFit::Full, full_question)
    } else if fits(&full_question, CollectorFit::CompactWithoutRing) {
        (CollectorFit::CompactWithoutRing, full_question)
    } else {
        (CollectorFit::CompactWithoutRing, format!("Move {items}, {size}?"))
    };
    let body = row![
        text(question).size(COLLECTOR_TEXT_SIZE).width(Fill),
        collector_button("Cancel".to_string(), theme::NEUTRAL_BUTTON, Message::CancelTrash),
        collector_button(confirm_label, theme::DANGER, Message::ConfirmTrash),
    ]
    .spacing(COLLECTOR_BUTTON_SPACING)
    .align_y(Alignment::Center)
    .into();
    (fit, body)
}

fn collected_items(b: &Browser, width: f32) -> (CollectorFit, Element<'_, Message>) {
    let collected = &b.collector;
    let count = format!("{} collected", thousands(collected.len() as u64));
    let size = format_size(collected.size(&b.tree));
    let full_summary = format!("{count} · {size}");
    let trash_label = format!("Move to {TRASH_NAME}…");
    let buttons = |fit: CollectorFit| {
        generous_button_width("Clear", COLLECTOR_BUTTON_TEXT_SIZE)
            + generous_button_width(&trash_label, COLLECTOR_BUTTON_TEXT_SIZE)
            + fit.hold_button_width()
            + 3.0 * COLLECTOR_BUTTON_SPACING
    };
    let summary_needs = |fit: CollectorFit| match fit {
        CollectorFit::Full => generous_text_width(&full_summary, COLLECTOR_TEXT_SIZE),
        _ => generous_text_width(&count, COLLECTOR_TEXT_SIZE).max(generous_text_width(&size, COLLECTOR_TEXT_SIZE)),
    };
    let fit = CollectorFit::WIDEST_FIRST
        .into_iter()
        .find(|&fit| fit.ring_width() + summary_needs(fit) + buttons(fit) <= width)
        .unwrap_or(CollectorFit::CompactWithoutRing);
    let summary_width = width - fit.ring_width() - buttons(fit);

    let summary: Element<'_, Message> = if fit == CollectorFit::Full {
        column![
            text(fit_width(&full_summary, summary_width, COLLECTOR_TEXT_SIZE))
                .size(COLLECTOR_TEXT_SIZE)
                .wrapping(Wrapping::None),
            collected_chips(b, summary_width),
        ]
        .spacing(4)
        .into()
    } else {
        column![
            text(fit_width(&count, summary_width, COLLECTOR_TEXT_SIZE))
                .size(COLLECTOR_TEXT_SIZE)
                .wrapping(Wrapping::None),
            text(fit_width(&size, summary_width, COLLECTOR_BUTTON_TEXT_SIZE))
                .size(COLLECTOR_BUTTON_TEXT_SIZE)
                .color(theme::TEXT_DIM)
                .wrapping(Wrapping::None),
        ]
        .spacing(2)
        .into()
    };
    let body = row![
        container(summary).width(Fill).clip(true),
        collector_button("Clear".to_string(), theme::NEUTRAL_BUTTON, Message::ClearCollector),
        collector_button(trash_label, theme::DANGER, Message::AskTrash),
        hold_to_delete_button(b, fit),
    ]
    .spacing(COLLECTOR_BUTTON_SPACING)
    .align_y(Alignment::Center)
    .into();
    (fit, body)
}

fn generous_chip_width(name: &str) -> f32 {
    let remove_button = generous_text_width("×", CHIP_TEXT_SIZE) + 2.0 * CHIP_REMOVE_PADDING_X;
    2.0 * CHIP_PADDING_X
        + CHIP_DOT
        + 2.0 * CHIP_INNER_SPACING
        + generous_text_width(name, CHIP_TEXT_SIZE)
        + remove_button
        + CHIP_GAP
}

fn more_items_label(hidden: usize) -> String {
    format!("+{hidden} more")
}

fn collected_chips(b: &Browser, width: f32) -> Element<'_, Message> {
    let collected = b.collector.items();
    let mut chips = row![].spacing(CHIP_GAP).align_y(Alignment::Center);
    let mut used = 0.0;
    let mut shown = 0;
    for &id in collected.iter().take(COLLECTED_CHIPS_SHOWN) {
        let name = truncate(&b.tree.name(id), COLLECTED_CHIP_NAME_CHARS);
        let hidden_after_this = collected.len() - shown - 1;
        let more_label = if hidden_after_this > 0 {
            generous_text_width(&more_items_label(hidden_after_this), CHIP_TEXT_SIZE)
        } else {
            0.0
        };
        let chip_width = generous_chip_width(&name);
        if used + chip_width + more_label > width {
            break;
        }
        used += chip_width;
        shown += 1;
        chips = chips.push(
            container(
                row![
                    dot(theme::kind_color(b.tree.node(id).kind()), CHIP_DOT),
                    text(name).size(CHIP_TEXT_SIZE).wrapping(Wrapping::None),
                    button(text("×").size(CHIP_TEXT_SIZE))
                        .padding([0.0, CHIP_REMOVE_PADDING_X])
                        .style(button::text)
                        .on_press(Message::Uncollect(id)),
                ]
                .spacing(CHIP_INNER_SPACING)
                .align_y(Alignment::Center),
            )
            .padding([2.0, CHIP_PADDING_X])
            .style(|_| rounded_box(theme::PANEL_HI, 10.0)),
        );
    }
    if collected.len() > shown {
        let label = if shown == 0 { items_phrase(collected.len()) } else { more_items_label(collected.len() - shown) };
        chips = chips.push(text(label).size(CHIP_TEXT_SIZE).color(theme::TEXT_DIM).wrapping(Wrapping::None));
    }
    chips.into()
}

const LIST_PANEL_PADDING: f32 = 14.0;
const LIST_HEADER_TEXT_SIZE: f32 = 22.0;
const LIST_HEADER_DOT: f32 = 14.0;
const LIST_HEADER_SPACING: f32 = 10.0;
const LIST_SUMMARY_TEXT_SIZE: f32 = 12.0;
const LIST_ROW_TEXT_SIZE: f32 = 13.0;
const LIST_ROW_PADDING_X: f32 = 8.0;
const LIST_ROW_DOT: f32 = 8.0;
const LIST_ROW_SPACING: f32 = 8.0;
const LIST_BAR_SLOT_WIDTH: f32 = 64.0;
const LIST_BAR_MAX_WIDTH: f32 = 60.0;
const LIST_SIZE_COLUMN_WIDTH: f32 = 78.0;
const LIST_SCROLLBAR_GAP: f32 = 4.0;
const LIST_NAME_MIN_WIDTH_BESIDE_A_BAR: f32 = 120.0;

struct ListColumns {
    name_width: f32,
    shows_bar: bool,
}

impl ListColumns {
    fn for_width(list_width: f32) -> Self {
        let without_bar = 2.0 * LIST_ROW_PADDING_X
            + LIST_ROW_DOT
            + 2.0 * LIST_ROW_SPACING
            + LIST_SIZE_COLUMN_WIDTH
            + DEFAULT_SCROLLBAR_WIDTH
            + LIST_SCROLLBAR_GAP;
        let name_beside_bar = list_width - without_bar - LIST_BAR_SLOT_WIDTH - LIST_ROW_SPACING;
        if name_beside_bar >= LIST_NAME_MIN_WIDTH_BESIDE_A_BAR {
            Self { name_width: name_beside_bar, shows_bar: true }
        } else {
            Self { name_width: list_width - without_bar, shows_bar: false }
        }
    }
}

fn clipped<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).width(Fill).clip(true).into()
}

fn list_panel(b: &Browser) -> Element<'_, Message> {
    container(responsive(move |size| list_panel_content(b, size)))
        .id(LIST_PANEL_ID)
        .padding(LIST_PANEL_PADDING)
        .width(Length::FillPortion(LIST_SIDE_PORTION))
        .height(Fill)
        .style(|_| rounded_box(theme::PANEL, 10.0))
        .into()
}

fn list_panel_content(b: &Browser, panel_size: iced::Size) -> Element<'_, Message> {
    let width = panel_size.width;
    let subject = b.listed_folder();
    let t = &b.tree;

    let size = format_size(b.size(subject));
    let name_width =
        width - LIST_HEADER_DOT - 2.0 * LIST_HEADER_SPACING - generous_text_width(&size, LIST_HEADER_TEXT_SIZE);
    let mut header = column![
        row![
            dot(theme::kind_color(t.node(subject).kind()), LIST_HEADER_DOT),
            clipped(
                text(fit_width(&t.name(subject), name_width, LIST_HEADER_TEXT_SIZE))
                    .size(LIST_HEADER_TEXT_SIZE)
                    .wrapping(Wrapping::None)
            ),
            text(size).size(LIST_HEADER_TEXT_SIZE).wrapping(Wrapping::None),
        ]
        .spacing(LIST_HEADER_SPACING)
        .align_y(Alignment::Center),
    ]
    .spacing(4);
    if let Some(s) = &b.search {
        let search_summary = format!("{} matches for “{}”", thousands(s.matches as u64), s.query);
        header = header.push(
            text(fit_width(&search_summary, width, LIST_SUMMARY_TEXT_SIZE))
                .size(LIST_SUMMARY_TEXT_SIZE)
                .color(theme::TEXT_DIM)
                .wrapping(Wrapping::None),
        );
    }

    let columns = ListColumns::for_width(width);
    let mut rows = Column::new().spacing(1);
    for r in &b.list.rows {
        rows = rows.push(list_row(b, r, &columns));
    }
    if let Some(more) = &b.list.cut_off_rows_summary {
        rows = rows.push(container(text(more.as_str()).size(12).color(theme::TEXT_DIM)).padding([6, 10]));
    }
    if b.list.rows.is_empty() && t.node(subject).is_dir() {
        rows = rows.push(container(text("Empty").size(13).color(theme::TEXT_DIM)).padding(10));
    }

    let mut panel = column![header, scrollable(rows).height(Fill).spacing(LIST_SCROLLBAR_GAP)].spacing(12);
    if let Some(details) = selection_details(b, panel_size) {
        panel = panel.push(column![separator(), details].spacing(12));
    }
    panel.into()
}

const DETAILS_TITLE_TEXT_SIZE: f32 = 14.0;
const DETAILS_TEXT_SIZE: f32 = 12.0;
const DETAILS_SPACING: f32 = 8.0;
const BULLET: &str = "•";
const BULLET_SPACING: f32 = 6.0;
const EXPLANATION_SHARE_OF_THE_PANEL: f32 = 0.5;
const PREVIEW_SHORTCUT: &str = "Space";
const OPEN_FULL_DISK_ACCESS_LABEL: &str = "Open Full Disk Access Settings";

fn separator<'a>() -> Element<'a, Message> {
    container(Space::new()).width(Fill).height(1).style(|_| container::background(theme::PANEL_HI)).into()
}

fn selection_details(b: &Browser, panel: iced::Size) -> Option<Element<'_, Message>> {
    let id = b.selected?;
    if let Some(explanation) = dscan_core::hidden::explain(&b.tree, id) {
        return Some(unscanned_space_explanation(b, id, explanation, panel));
    }
    b.tree.is_actionable(id).then(|| selection_actions(b, id, panel.width))
}

fn selection_title(b: &Browser, id: NodeId, width: f32) -> Element<'_, Message> {
    let size = format_size(b.size(id));
    let name_width =
        width - LIST_ROW_DOT - 2.0 * LIST_ROW_SPACING - generous_text_width(&size, DETAILS_TITLE_TEXT_SIZE);
    row![
        dot(theme::kind_color(b.tree.node(id).kind()), LIST_ROW_DOT),
        clipped(
            text(fit_width(&b.tree.name(id), name_width, DETAILS_TITLE_TEXT_SIZE))
                .size(DETAILS_TITLE_TEXT_SIZE)
                .wrapping(Wrapping::None)
        ),
        text(size).size(DETAILS_TITLE_TEXT_SIZE).wrapping(Wrapping::None),
    ]
    .spacing(LIST_ROW_SPACING)
    .align_y(Alignment::Center)
    .into()
}

fn selection_actions(b: &Browser, id: NodeId, width: f32) -> Element<'_, Message> {
    let preview = tooltip(
        outlined_text_button(PREVIEW_ACTION, Message::Preview),
        tooltip_box(PREVIEW_SHORTCUT),
        tooltip::Position::Top,
    )
    .gap(4);
    let actions = row![preview, outlined_text_button(REVEAL_ACTION, Message::Reveal)].spacing(DETAILS_SPACING);
    column![selection_title(b, id, width), actions].spacing(DETAILS_SPACING).into()
}

fn remedy_button<'a>(remedy: Remedy) -> Option<Element<'a, Message>> {
    match remedy {
        Remedy::GrantFullDiskAccess => dscan_core::FULL_DISK_ACCESS_SETTINGS
            .map(|_| outlined_text_button(OPEN_FULL_DISK_ACCESS_LABEL, Message::OpenFdaSettings)),
    }
}

fn details_text<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a, Theme> {
    text(content).size(DETAILS_TEXT_SIZE)
}

fn unscanned_space_explanation(
    b: &Browser,
    id: NodeId,
    explanation: SpaceExplanation,
    panel: iced::Size,
) -> Element<'_, Message> {
    let mut body = column![details_text(explanation.summary).color(theme::TEXT_DIM)].spacing(DETAILS_SPACING);
    let unreadable = b.tree.stats.errors;
    if b.tree.node(id).kind() == Kind::Hidden && unreadable > 0 {
        let note = format!("{} folders couldn't be read in this scan.", thousands(unreadable));
        body = body.push(details_text(note).color(theme::kind_color(Kind::Archive)));
    }
    for cause in explanation.causes {
        let mut item =
            column![row![details_text(BULLET), details_text(cause.text).width(Fill)].spacing(BULLET_SPACING)]
                .spacing(BULLET_SPACING);
        if let Some(button) = cause.remedy.and_then(remedy_button) {
            item = item.push(button);
        }
        body = body.push(item);
    }
    let body = container(scrollable(body).spacing(LIST_SCROLLBAR_GAP))
        .max_height(panel.height * EXPLANATION_SHARE_OF_THE_PANEL);
    column![selection_title(b, id, panel.width), body].spacing(DETAILS_SPACING).into()
}

fn list_row<'a>(b: &'a Browser, r: &'a ListRow, columns: &ListColumns) -> Element<'a, Message> {
    let id = r.id;
    let n = b.tree.node(id);
    let kind_color = theme::kind_color(n.kind());
    let collected = b.collector.contains(id);
    let name_color = if collected && !columns.shows_bar {
        theme::DANGER
    } else if n.is_synthetic() || n.is_unreadable() {
        theme::TEXT_DIM
    } else {
        theme::TEXT
    };

    let mut content = row![
        dot(kind_color, LIST_ROW_DOT),
        clipped(
            text(fit_width(&r.name, columns.name_width, LIST_ROW_TEXT_SIZE))
                .size(LIST_ROW_TEXT_SIZE)
                .color(name_color)
                .wrapping(Wrapping::None)
        ),
    ]
    .spacing(LIST_ROW_SPACING)
    .align_y(Alignment::Center);
    if columns.shows_bar {
        let slot: Element<'a, Message> = if collected {
            text("collected").size(11).color(theme::DANGER).wrapping(Wrapping::None).into()
        } else {
            container(Space::new())
                .width(Length::Fixed((r.fraction_of_listed_folder * LIST_BAR_MAX_WIDTH).max(1.0)))
                .height(3)
                .style(move |_| container::background(kind_color.scale_alpha(0.7)))
                .into()
        };
        content = content.push(container(slot).width(LIST_BAR_SLOT_WIDTH).align_x(Alignment::End));
    }
    let content = content.push(
        text(r.size.as_str())
            .size(LIST_ROW_TEXT_SIZE)
            .width(LIST_SIZE_COLUMN_WIDTH)
            .align_x(Alignment::End)
            .wrapping(Wrapping::None),
    );

    let bg = if b.selected == Some(id) {
        Some(theme::ACCENT.scale_alpha(0.35))
    } else if b.hovered == Some(Segment::Node(id)) {
        Some(theme::PANEL_HI)
    } else if b.search.as_ref().is_some_and(|s| s.is_match(id)) {
        Some(theme::ACCENT.scale_alpha(0.12))
    } else {
        None
    };
    let cell = container(content).padding([4.0, LIST_ROW_PADDING_X]).width(Fill).style(move |_| {
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

const STATUS_BAR_HEIGHT: f32 = 24.0;
const STATUS_TEXT_SIZE: f32 = 12.0;
const STATUS_SPACING: f32 = 12.0;
const STATUS_INNER_SPACING: f32 = 6.0;
const STATUS_DISMISS_PADDING_X: f32 = 6.0;
const SUBTLE_BUTTON_PADDING_X: f32 = 8.0;
const FULL_DISK_ACCESS_BUTTON_LABEL: &str = "Full Disk Access…";

fn status_bar(b: &Browser) -> Element<'_, Message> {
    container(responsive(move |size| status_bar_content(b, size.width))).height(STATUS_BAR_HEIGHT).into()
}

fn status_bar_content(b: &Browser, width: f32) -> Element<'_, Message> {
    let t = &b.tree;
    if let Some(s) = &b.status {
        let dismiss_width = generous_text_width("×", STATUS_TEXT_SIZE) + 2.0 * STATUS_DISMISS_PADDING_X;
        let message_width = width - dismiss_width - STATUS_INNER_SPACING;
        return row![
            text(fit_width(s, message_width, STATUS_TEXT_SIZE))
                .size(STATUS_TEXT_SIZE)
                .color(theme::TEXT)
                .wrapping(Wrapping::None),
            button(text("×").size(STATUS_TEXT_SIZE))
                .padding([0.0, STATUS_DISMISS_PADDING_X])
                .style(button::text)
                .on_press(Message::DismissStatus),
        ]
        .spacing(STATUS_INNER_SPACING)
        .height(Fill)
        .align_y(Alignment::Center)
        .into();
    }

    let mut summary = format!(
        "{} files in {} folders scanned in {:.1} s",
        thousands(t.stats.files),
        thousands(t.stats.dirs),
        t.stats.duration.as_secs_f32()
    );
    if t.is_whole_disk() {
        summary.push_str(&format!(
            " · hidden {} · purgeable {}",
            format_size(t.stats.hidden),
            format_size(t.stats.purgeable)
        ));
    }
    let mut bar = row![].spacing(STATUS_SPACING).height(Fill).align_y(Alignment::Center);
    let mut unreadable_width = 0.0;
    let mut unreadable = row![].spacing(STATUS_INNER_SPACING).align_y(Alignment::Center);
    if t.stats.errors > 0 {
        let couldnt_read = format!("{} folders couldn't be read", thousands(t.stats.errors));
        unreadable_width += generous_text_width(&couldnt_read, STATUS_TEXT_SIZE) + STATUS_SPACING;
        unreadable = unreadable.push(
            text(couldnt_read).size(STATUS_TEXT_SIZE).color(theme::kind_color(Kind::Archive)).wrapping(Wrapping::None),
        );
        if dscan_core::FULL_DISK_ACCESS_SETTINGS.is_some() {
            unreadable_width += STATUS_INNER_SPACING
                + generous_text_width(FULL_DISK_ACCESS_BUTTON_LABEL, STATUS_TEXT_SIZE)
                + 2.0 * SUBTLE_BUTTON_PADDING_X;
            unreadable = unreadable.push(subtle_button(
                text(FULL_DISK_ACCESS_BUTTON_LABEL).size(STATUS_TEXT_SIZE).wrapping(Wrapping::None),
                Some(Message::OpenFdaSettings),
            ));
        }
    }
    bar = bar.push(clipped(
        text(fit_width(&summary, width - unreadable_width, STATUS_TEXT_SIZE))
            .size(STATUS_TEXT_SIZE)
            .color(theme::TEXT_DIM)
            .wrapping(Wrapping::None),
    ));
    if t.stats.errors > 0 {
        bar = bar.push(unreadable);
    }
    bar.into()
}
