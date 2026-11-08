use std::sync::LazyLock;

use dscan_core::{
    Accounting, COMMAND_KEY, NodeId, PREVIEW_ACTION, RESCAN_SHORTCUT, REVEAL_ACTION, Remedy, Segment, SpaceExplanation,
    TRASH_NAME, Volume, format_size,
};
use iced::border;
use iced::widget::text::Wrapping;
use iced::widget::{
    Column, Space, Svg, button, canvas, center, column, container, mouse_area, pin, progress_bar, responsive, row,
    scrollable, stack, text, text_input, tooltip,
};
use iced::{Alignment, Color, Element, Fill, Length, Theme};

use crate::animation::ChartAnimation;
use crate::app::{App, Browser, HOLD_TO_DELETE, ListRow, Message, Notice, NoticeKind, SEARCH_ID, ScanJob, Screen};
use crate::depth::{self, Elevation};
use crate::fmt::{fit_width, fit_width_keeping_both_ends, generous_text_width, items_phrase, thousands, truncate};
use crate::icon;
use crate::sunburst::Sunburst;
use crate::theme;

pub const DEFAULT_WINDOW_SIZE: iced::Size = iced::Size::new(1280.0, 820.0);
pub const MIN_WINDOW_SIZE: iced::Size = iced::Size::new(860.0, 560.0);

pub fn view(app: &App) -> Element<'_, Message> {
    let content = match &app.screen {
        Screen::Start { .. } => start(app),
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

const FILLED_BUTTON_RADIUS: f32 = 6.0;
const HOVER_TINT: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.02);
const PRESS_TINT: Color = Color::from_rgba(0.0, 0.0, 0.0, 0.18);

fn framed_group<'a>(content: impl Into<Element<'a, Message>>) -> container::Container<'a, Message> {
    container(content).padding(FRAME_INSET).height(TOOLBAR_CONTROL_HEIGHT).align_y(iced::alignment::Vertical::Center)
}

fn raised_group<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    depth::casting_shadow(
        framed_group(content).style(|_| depth::surface(theme::PANEL_HI, FRAME_RADIUS)),
        FRAME_RADIUS,
        Elevation::Control,
    )
    .into()
}

fn quiet_button_style(text_when_idle: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let tint = match status {
            button::Status::Hovered => Some(HOVER_TINT),
            button::Status::Pressed => Some(PRESS_TINT),
            button::Status::Active | button::Status::Disabled => None,
        };
        button::Style {
            background: tint.map(Into::into),
            text_color: if tint.is_some() { theme::TEXT } else { text_when_idle },
            border: border::rounded(FRAME_RADIUS - FRAME_INSET),
            ..button::Style::default()
        }
    }
}

fn filled_button<'a>(
    content: impl Into<Element<'a, Message>>,
    bg: Color,
    shape: impl FnOnce(button::Button<'a, Message>) -> button::Button<'a, Message>,
) -> Element<'a, Message> {
    depth::casting_shadow(
        shape(button(content).style(depth::raised_button(bg, Color::WHITE, FILLED_BUTTON_RADIUS))),
        FILLED_BUTTON_RADIUS,
        Elevation::Control,
    )
    .into()
}

fn outlined_text_button(label: &str, msg: Message) -> Element<'_, Message> {
    depth::casting_shadow(
        button(text(label).size(13).wrapping(Wrapping::None))
            .padding([6, 14])
            .style(depth::raised_button(theme::PANEL_HI, theme::TEXT, FRAME_RADIUS))
            .on_press(msg),
        FRAME_RADIUS,
        Elevation::Control,
    )
    .into()
}

const TOOLTIP_MAX_WIDTH: f32 = 320.0;
const TOOLTIP_RADIUS: f32 = 6.0;

fn tooltip_box<'a>(label: impl text::IntoFragment<'a>) -> container::Container<'a, Message> {
    depth::casting_shadow(
        container(text(label).size(12))
            .padding([4, 8])
            .max_width(TOOLTIP_MAX_WIDTH)
            .style(|_| depth::surface(theme::PANEL_HI, TOOLTIP_RADIUS)),
        TOOLTIP_RADIUS,
        Elevation::Floating,
    )
}

const TOOLTIP_GAP: f32 = 4.0;

fn tooltip_at<'a>(
    content: impl Into<Element<'a, Message>>,
    label: impl text::IntoFragment<'a>,
    position: tooltip::Position,
) -> Element<'a, Message> {
    tooltip(content, tooltip_box(label), position).gap(TOOLTIP_GAP).into()
}

fn with_tooltip<'a>(content: impl Into<Element<'a, Message>>, label: &'a str) -> Element<'a, Message> {
    tooltip_at(content, label, tooltip::Position::Bottom)
}

fn icon_button<'a>(
    icon: impl Into<Element<'a, Message>>,
    tooltip_label: &'a str,
    msg: Message,
) -> Element<'a, Message> {
    let button = button(icon).padding(ICON_BUTTON_PADDING).style(quiet_button_style(theme::TEXT)).on_press(msg);
    with_tooltip(button, tooltip_label)
}

const START_PADDING: f32 = 24.0;
const START_BLOCK_MAX_WIDTH: f32 = 720.0;
const START_SECTION_GAP: f32 = 14.0;
const DISK_LIST_PADDING: f32 = 4.0;
const DISK_LIST_RADIUS: f32 = 12.0;
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
const SCAN_BUTTON_WIDTH: f32 = 76.0;
const SCAN_BUTTON_TEXT_SIZE: f32 = 14.0;
const FILLING_UP: f32 = 0.75;
const NEARLY_FULL: f32 = 0.9;
const SCAN_SHOWN_COMPLETE_ONLY_WHEN_DONE: f32 = 0.99;
const PAUSED_SCAN_STRENGTH: f32 = 0.5;
const SCAN_FOLDER_LABEL: &str = "Scan Folder…";
const SCAN_LABEL: &str = "Scan";
const PAUSE_LABEL: &str = "Pause";
const RESUME_LABEL: &str = "Resume";

fn start(app: &App) -> Element<'_, Message> {
    responsive(move |viewport| {
        let width = (viewport.width - 2.0 * START_PADDING).min(START_BLOCK_MAX_WIDTH);
        scrollable(center(start_content(app, width)).padding(START_PADDING))
            .width(Fill)
            .height(viewport.height)
            .style(depth::slim_scrollbar)
            .into()
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
        let warn = theme::WARNING;
        col = col.push(Space::new().height(START_SECTION_GAP)).push(
            container(
                row![
                    text("Without Full Disk Access some folders can't be read, so some space shows up as hidden. Grant it to your terminal (or this app), then restart it.")
                        .size(13)
                        .width(Fill),
                    filled_button(text("Open Settings").size(13).wrapping(Wrapping::None), theme::ACCENT, |b| b.on_press(Message::OpenFdaSettings)),
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
    let scan = app.scan();
    let text_width = disk_text_width(width);
    let mut scan_shown = false;
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
                    icon: icon::scan_root(true, DISK_ICON_SIZE, theme::TEXT_DIM),
                    name: v.name.clone(),
                    details: format!("{} · {}", format_size(v.total), volume_details(v)),
                };
                rows.push(match scan.filter(|job| job.scans_volume(v)) {
                    Some(job) => {
                        scan_shown = true;
                        scanning_row(identity, job, text_width)
                    }
                    None => volume_row(identity, i, v, text_width),
                });
            }
        }
    }
    if let Some(job) = scan.filter(|_| !scan_shown) {
        let identity = DiskIdentity {
            icon: icon::scan_root(job.opts.whole_disk, DISK_ICON_SIZE, theme::TEXT_DIM),
            name: job.opts.display_name(),
            details: job.opts.root.display().to_string(),
        };
        rows.push(scanning_row(identity, job, text_width));
    }
    depth::casting_shadow(
        container(Column::with_children(rows).width(Fill))
            .padding(DISK_LIST_PADDING)
            .width(Fill)
            .style(|_| depth::surface(theme::PANEL, DISK_LIST_RADIUS)),
        DISK_LIST_RADIUS,
        Elevation::Panel,
    )
    .into()
}

fn disk_text_width(list_width: f32) -> f32 {
    list_width
        - 2.0 * DISK_LIST_PADDING
        - 2.0 * DISK_ROW_PADDING_X
        - DISK_ICON_SIZE
        - 3.0 * DISK_ROW_SPACING
        - METER_WIDTH
        - SCAN_BUTTON_WIDTH
}

struct DiskIdentity<'a> {
    icon: Svg<'a, Theme>,
    name: String,
    details: String,
}

struct DiskMeter<'a> {
    fraction: f32,
    color: Color,
    label: String,
    label_color: Color,
    controls: Element<'a, Message>,
}

fn disk_row<'a>(identity: DiskIdentity<'a>, meter: DiskMeter<'a>, text_width: f32) -> Element<'a, Message> {
    let line_color = meter.color;
    let line = progress_bar(0.0..=1.0, meter.fraction)
        .length(METER_WIDTH)
        .girth(METER_HEIGHT)
        .style(move |_| depth::meter(line_color, METER_HEIGHT / 2.0));
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
        column![line, text(meter.label).size(METER_LABEL_SIZE).color(meter.label_color).wrapping(Wrapping::None)]
            .spacing(METER_LABEL_GAP)
            .width(METER_WIDTH)
            .align_x(Alignment::End),
        container(meter.controls).width(SCAN_BUTTON_WIDTH).align_x(Alignment::End),
    ]
    .spacing(DISK_ROW_SPACING)
    .align_y(Alignment::Center)
    .padding([0.0, DISK_ROW_PADDING_X])
    .height(DISK_ROW_HEIGHT)
    .into()
}

fn fullness_color(used_fraction: f32) -> Color {
    if used_fraction >= NEARLY_FULL {
        theme::DANGER
    } else if used_fraction >= FILLING_UP {
        theme::WARNING
    } else {
        theme::SUCCESS
    }
}

fn scan_button<'a>(label: &'a str, msg: Message) -> Element<'a, Message> {
    filled_button(
        text(label).size(SCAN_BUTTON_TEXT_SIZE).width(Fill).align_x(Alignment::Center).wrapping(Wrapping::None),
        theme::NEUTRAL_BUTTON,
        |b| b.padding([6.0, 0.0]).width(SCAN_BUTTON_WIDTH).on_press(msg),
    )
}

fn volume_row<'a>(identity: DiskIdentity<'a>, index: usize, v: &Volume, text_width: f32) -> Element<'a, Message> {
    let used_fraction = if v.total > 0 { v.used as f32 / v.total as f32 } else { 0.0 };
    let color = fullness_color(used_fraction);
    let meter = DiskMeter {
        fraction: used_fraction,
        color,
        label: format!("{} free", format_size(v.available)),
        label_color: color,
        controls: scan_button(SCAN_LABEL, Message::ScanVolume(index)),
    };
    disk_row(identity, meter, text_width)
}

fn scanning_row<'a>(identity: DiskIdentity<'a>, job: &ScanJob, text_width: f32) -> Element<'a, Message> {
    let s = &job.snapshot;
    let fraction =
        s.expected.map_or(0.0, |expected| (s.bytes as f32 / expected as f32).min(SCAN_SHOWN_COMPLETE_ONLY_WHEN_DONE));
    let (color, label, pause_or_resume) = if job.cancel.is_paused() {
        (
            theme::mix(theme::BG, theme::ACCENT, PAUSED_SCAN_STRENGTH),
            format!("Paused · {}", format_size(s.bytes)),
            RESUME_LABEL,
        )
    } else {
        (theme::ACCENT, format!("{} · {} files", format_size(s.bytes), thousands(s.files)), PAUSE_LABEL)
    };
    let controls = scan_button(pause_or_resume, Message::TogglePause);
    let meter = DiskMeter { fraction, color, label, label_color: theme::TEXT, controls };
    disk_row(identity, meter, text_width)
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
const RESULT_CARD_RADIUS: f32 = 10.0;
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
        list = list.push(depth::casting_shadow(
            button(content)
                .padding([0.0, RESULT_ROW_PADDING_X])
                .width(Fill)
                .height(RESULT_ROW_H)
                .style(depth::raised_button(theme::PANEL, theme::TEXT, RESULT_CARD_RADIUS))
                .on_press(Message::ResumeParked(i)),
            RESULT_CARD_RADIUS,
            Elevation::Control,
        ));
    }
    scrollable(list).height(Fill).spacing(RESULT_LIST_SCROLLBAR_GAP).style(depth::slim_scrollbar).into()
}

const PANEL_GAP: f32 = 12.0;
const CHART_SIDE_PORTION: u16 = 3;
const LIST_SIDE_PORTION: u16 = 2;
pub const TOOLBAR_LIST_SIDE_ID: &str = "toolbar-list-side";
pub const LIST_PANEL_ID: &str = "list-panel";
pub const PATH_BAR_ID: &str = "path-bar";

fn browse(b: &Browser) -> Element<'_, Message> {
    let main = column![toolbar(b), row![chart_panel(b), list_panel(b)].spacing(PANEL_GAP).height(Fill)]
        .spacing(10)
        .padding(12);

    match b.active_drag() {
        Some(d) => {
            let label = format!("{}  {}", truncate(&b.tree.name(d.item), 40), format_size(b.size(d.item)));
            let chip = container(text(label).size(13)).padding([6, 10]).style(|_| {
                rounded_box(theme::ACCENT.scale_alpha(0.92), 8.0).color(Color::WHITE).shadow(floating_shadow())
            });
            stack![main, pin(chip).x(d.pos.x + 14.0).y(d.pos.y + 10.0)].into()
        }
        None => main.into(),
    }
}

fn with_command_shortcut(label: &str, key: &str) -> String {
    format!("{label}  {COMMAND_KEY}{key}")
}

static SEARCH_PLACEHOLDER_WITH_SHORTCUT: LazyLock<String> = LazyLock::new(|| with_command_shortcut("Search", "F"));
static BACK_TOOLTIP: LazyLock<String> = LazyLock::new(|| with_command_shortcut("Back", "["));
static FORWARD_TOOLTIP: LazyLock<String> = LazyLock::new(|| with_command_shortcut("Forward", "]"));
static RESCAN_TOOLTIP: LazyLock<String> = LazyLock::new(|| format!("Scan again  {RESCAN_SHORTCUT}"));

const NAV_ICON_SIZE: f32 = 16.0;
const DISABLED_ICON_STRENGTH: f32 = 0.4;

fn nav_icon_button<'a>(
    icon: fn(f32, Color) -> Svg<'a, Theme>,
    tooltip_label: &'a str,
    msg: Option<Message>,
) -> Element<'a, Message> {
    let tint =
        if msg.is_some() { theme::TEXT } else { theme::mix(theme::PANEL_HI, theme::TEXT_DIM, DISABLED_ICON_STRENGTH) };
    let button = button(center(icon(NAV_ICON_SIZE, tint)))
        .width(INNER_BUTTON_SIDE)
        .height(INNER_BUTTON_SIDE)
        .padding(0)
        .style(quiet_button_style(theme::TEXT))
        .on_press_maybe(msg);
    with_tooltip(button, tooltip_label)
}

fn toolbar(b: &Browser) -> Element<'_, Message> {
    let nav = raised_group(
        row![
            nav_icon_button(icon::home, "Start page", Some(Message::GoHome)),
            nav_icon_button(icon::chevron_left, &BACK_TOOLTIP, Some(Message::Back)),
            nav_icon_button(icon::chevron_right, &FORWARD_TOOLTIP, b.can_forward().then_some(Message::Forward)),
        ]
        .spacing(2),
    );

    let search = text_input(&SEARCH_PLACEHOLDER_WITH_SHORTCUT, &b.search_text)
        .id(SEARCH_ID)
        .on_input(Message::SearchChanged)
        .padding([6, 10])
        .size(13)
        .width(Fill)
        .style(depth::well_input(FRAME_RADIUS));

    let chart_side =
        row![nav, path_bar(b)].spacing(8).align_y(Alignment::Center).width(Length::FillPortion(CHART_SIDE_PORTION));
    let list_side = container(row![rescan_button(), search].spacing(8).align_y(Alignment::Center))
        .id(TOOLBAR_LIST_SIDE_ID)
        .width(Length::FillPortion(LIST_SIDE_PORTION));
    row![chart_side, list_side].spacing(PANEL_GAP).align_y(Alignment::Center).into()
}

fn rescan_button<'a>() -> Element<'a, Message> {
    with_tooltip(outlined_text_button("Rescan", Message::Rescan), &RESCAN_TOOLTIP)
}

const PATH_ROOT_ICON_SIZE: f32 = 14.0;
const PATH_SEPARATOR_SIZE: f32 = 12.0;
const PATH_SEPARATOR_STRENGTH: f32 = 0.6;
const CRUMB_NAME_MAX_CHARS: usize = 28;
const BREADCRUMBS_AFTER_ELLIPSIS: usize = 5;

fn path_bar(b: &Browser) -> Element<'_, Message> {
    let content = row![
        container(icon::scan_root(b.opts.whole_disk, PATH_ROOT_ICON_SIZE, theme::TEXT_DIM)).padding([0, 6]),
        responsive(move |available| container(breadcrumbs_that_fit(b, available.width))
            .height(Fill)
            .align_y(Alignment::Center)
            .into()),
    ]
    .align_y(Alignment::Center);
    framed_group(content)
        .id(PATH_BAR_ID)
        .padding([FRAME_INSET, 2.0 * FRAME_INSET])
        .width(Fill)
        .clip(true)
        .style(|_| depth::well(theme::WELL, FRAME_RADIUS))
        .into()
}

fn path_separator<'a>() -> Element<'a, Message> {
    icon::chevron_right(PATH_SEPARATOR_SIZE, theme::mix(theme::WELL, theme::TEXT_DIM, PATH_SEPARATOR_STRENGTH)).into()
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

    let chart_above_the_collector =
        column![chart, Space::new().height(COLLECTOR_FIXED_HEIGHT)].spacing(CHART_COLLECTOR_GAP);
    let collector_floating_over_the_chart =
        responsive(move |size| column![Space::new().height(Fill), collector(b, size)].into());
    let mut layers = stack![chart_above_the_collector, collector_floating_over_the_chart];
    if let Some(message) = &b.notice {
        layers = layers
            .push(container(notice_banner(message)).width(Fill).padding(NOTICE_MARGIN).align_x(Alignment::Center));
    }
    layers.width(Length::FillPortion(CHART_SIDE_PORTION)).height(Fill).into()
}

const NOTICE_MARGIN: f32 = 8.0;
const NOTICE_MAX_WIDTH: f32 = 560.0;
const NOTICE_TEXT_SIZE: f32 = 14.0;
const NOTICE_LINE_SPACING: f32 = 1.5;
const NOTICE_PADDING: [f32; 2] = [8.0, 12.0];
const NOTICE_SPACING: f32 = 10.0;
const NOTICE_RADIUS: f32 = 10.0;
const NOTICE_DISMISS_ICON: f32 = 14.0;
const NOTICE_DISMISS_PADDING: f32 = 6.0;
pub const NOTICE_DISMISS_BUTTON_SIDE: f32 = NOTICE_DISMISS_ICON + 2.0 * NOTICE_DISMISS_PADDING;
const NOTICE_SIGN_SIZE: f32 = 20.0;
const NOTICE_BORDER_TINT: f32 = 0.45;
const DISMISS_NOTICE_TOOLTIP: &str = "Dismiss";

fn notice_color(kind: NoticeKind) -> Color {
    match kind {
        NoticeKind::Done => theme::SUCCESS,
        NoticeKind::Warning => theme::WARNING,
        NoticeKind::Error => theme::DANGER,
    }
}

fn notice_sign<'a>(kind: NoticeKind) -> Element<'a, Message> {
    let color = notice_color(kind);
    match kind {
        NoticeKind::Done => icon::check_circle(NOTICE_SIGN_SIZE, color).into(),
        NoticeKind::Warning => icon::warning_triangle(NOTICE_SIGN_SIZE, color).into(),
        NoticeKind::Error => icon::error_circle(NOTICE_SIGN_SIZE, color).into(),
    }
}

fn dismiss_notice_button<'a>() -> Element<'a, Message> {
    let dismiss = button(icon::close(NOTICE_DISMISS_ICON, theme::TEXT))
        .padding(NOTICE_DISMISS_PADDING)
        .width(NOTICE_DISMISS_BUTTON_SIDE)
        .height(NOTICE_DISMISS_BUTTON_SIDE)
        .style(|_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: hovered.then(|| theme::NEUTRAL_BUTTON.into()),
                border: border::rounded(FRAME_RADIUS - FRAME_INSET),
                ..button::Style::default()
            }
        })
        .on_press(Message::DismissNotice);
    with_tooltip(dismiss, DISMISS_NOTICE_TOOLTIP)
}

fn notice_banner(notice: &Notice) -> Element<'_, Message> {
    let tint = theme::mix(theme::NEUTRAL_BUTTON, notice_color(notice.kind), NOTICE_BORDER_TINT);
    let banner = container(
        row![
            notice_sign(notice.kind),
            text(notice.text.as_str()).size(NOTICE_TEXT_SIZE).line_height(NOTICE_LINE_SPACING).width(Fill),
            dismiss_notice_button(),
        ]
        .spacing(NOTICE_SPACING)
        .align_y(Alignment::Center),
    )
    .padding(NOTICE_PADDING)
    .max_width(NOTICE_MAX_WIDTH)
    .style(move |_| {
        rounded_box(theme::PANEL_HI, NOTICE_RADIUS)
            .border(border::rounded(NOTICE_RADIUS).width(1).color(tint))
            .shadow(floating_shadow())
    });
    mouse_area(banner)
        .on_enter(Message::NoticeEnter)
        .on_exit(Message::NoticeExit)
        .interaction(iced::mouse::Interaction::Idle)
        .into()
}

const CHART_COLLECTOR_GAP: f32 = 10.0;

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
const COLLECTOR_RADIUS: f32 = 12.0;
const COLLECTOR_CHEVRON_SIZE: f32 = 14.0;
const COLLECTOR_CHEVRON_SPACING: f32 = 6.0;
const COLLECTOR_CHEVRON_WIDTH: f32 = COLLECTOR_CHEVRON_SIZE + COLLECTOR_CHEVRON_SPACING;
const COLLECTED_LIST_PADDING: f32 = 6.0;
const COLLECTED_LIST_ROOM_LEFT_ABOVE: f32 = 12.0;
const COLLECTED_ROW_TEXT_SIZE: f32 = 13.0;
const COLLECTED_ROW_REMOVE_ICON: f32 = 12.0;
const COLLECTED_ROW_REMOVE_PADDING: f32 = 5.0;
const COLLECTED_ROW_REMOVE_WIDTH: f32 = COLLECTED_ROW_REMOVE_ICON + 2.0 * COLLECTED_ROW_REMOVE_PADDING;
const FLOATING_SHADOW_BLUR: f32 = 24.0;
const FLOATING_SHADOW_ALPHA: f32 = 0.5;
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
    let held_part = container(Space::new()).width(width * progress.unwrap_or(0.0)).height(Fill).style(|_| {
        container::Style::default().background(depth::lit_from_above(theme::DANGER)).border(border::rounded(6))
    });
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
    let button = depth::casting_shadow(
        mouse_area(face)
            .on_press(Message::DeleteHoldStart)
            .on_release(Message::DeleteHoldCancel)
            .on_exit(Message::DeleteHoldCancel)
            .interaction(iced::mouse::Interaction::Pointer),
        FILLED_BUTTON_RADIUS,
        Elevation::Control,
    );
    if fit == CollectorFit::Full {
        button.into()
    } else {
        tooltip_at(button, hold_to_delete_explanation(), tooltip::Position::Top)
    }
}

fn collector(b: &Browser, layer: iced::Size) -> Element<'_, Message> {
    let dragging = b.active_drag().is_some();
    let hot = b.dropping_on_collector();
    let list_open = b.collected_list_open;

    let bar = container(responsive(move |size| collector_content(b, size.width)))
        .padding(COLLECTOR_PADDING)
        .width(Fill)
        .height(COLLECTOR_FIXED_HEIGHT);
    let bar = if b.collector.is_empty() {
        mouse_area(bar)
    } else {
        mouse_area(bar).on_press(Message::ToggleCollectedList).interaction(iced::mouse::Interaction::Pointer)
    };

    let mut panel = Column::new().width(Fill);
    if list_open {
        let list_height = layer.height - COLLECTOR_FIXED_HEIGHT - COLLECTED_LIST_ROOM_LEFT_ABOVE;
        panel = panel
            .push(collected_list(b, iced::Size::new(layer.width, list_height)))
            .push(container(separator()).padding([0.0, COLLECTOR_PADDING]));
    }
    let zone = container(panel.push(bar)).width(Fill).style(move |_| {
        let bg = if hot { theme::mix(theme::PANEL, theme::ACCENT, 0.25) } else { theme::PANEL };
        let style = depth::surface(bg, COLLECTOR_RADIUS);
        if dragging {
            style.border(border::rounded(COLLECTOR_RADIUS).width(1.5).color(theme::ACCENT.scale_alpha(0.8)))
        } else {
            style
        }
    });
    let elevation = if list_open { Elevation::Floating } else { Elevation::Panel };
    depth::casting_shadow(
        mouse_area(zone).on_enter(Message::CollectorEnter).on_exit(Message::CollectorExit),
        COLLECTOR_RADIUS,
        elevation,
    )
    .into()
}

fn floating_shadow() -> iced::Shadow {
    iced::Shadow {
        color: Color::BLACK.scale_alpha(FLOATING_SHADOW_ALPHA),
        offset: iced::Vector::ZERO,
        blur_radius: FLOATING_SHADOW_BLUR,
    }
}

fn collected_list(b: &Browser, room: iced::Size) -> Element<'_, Message> {
    let row_width = room.width - 2.0 * COLLECTED_LIST_PADDING - DEFAULT_SCROLLBAR_WIDTH - LIST_SCROLLBAR_GAP;
    let mut rows = Column::new().spacing(1);
    for &id in b.collector.items() {
        rows = rows.push(collected_row(b, id, row_width));
    }
    let list = container(scrollable(rows).spacing(LIST_SCROLLBAR_GAP).style(depth::slim_scrollbar))
        .padding(COLLECTED_LIST_PADDING)
        .width(Fill)
        .max_height(room.height.max(0.0));
    mouse_area(list).interaction(iced::mouse::Interaction::Idle).into()
}

fn collected_row(b: &Browser, id: NodeId, row_width: f32) -> Element<'_, Message> {
    let node = b.tree.node(id);
    let mut name = b.tree.name(id).into_owned();
    if node.is_dir() {
        name.push('/');
    }
    let name_width = row_width
        - 2.0 * LIST_ROW_PADDING_X
        - LIST_ROW_DOT
        - 3.0 * LIST_ROW_SPACING
        - LIST_SIZE_COLUMN_WIDTH
        - COLLECTED_ROW_REMOVE_WIDTH;
    let remove = button(icon::close(COLLECTED_ROW_REMOVE_ICON, theme::TEXT_DIM))
        .padding(COLLECTED_ROW_REMOVE_PADDING)
        .style(quiet_button_style(theme::TEXT_DIM))
        .on_press(Message::Uncollect(id));
    let content = row![
        dot(theme::kind_color(node.kind()), LIST_ROW_DOT),
        clipped(
            text(fit_width(&name, name_width, COLLECTED_ROW_TEXT_SIZE))
                .size(COLLECTED_ROW_TEXT_SIZE)
                .wrapping(Wrapping::None)
        ),
        text(format_size(b.tree.size(id)))
            .size(COLLECTED_ROW_TEXT_SIZE)
            .color(theme::TEXT_DIM)
            .width(LIST_SIZE_COLUMN_WIDTH)
            .align_x(Alignment::End)
            .wrapping(Wrapping::None),
        remove,
    ]
    .spacing(LIST_ROW_SPACING)
    .align_y(Alignment::Center);
    container(content).padding([2.0, LIST_ROW_PADDING_X]).width(Fill).into()
}

fn collector_chevron<'a>(b: &Browser) -> Element<'a, Message> {
    if b.collected_list_open {
        icon::chevron_down(COLLECTOR_CHEVRON_SIZE, theme::TEXT_DIM).into()
    } else {
        icon::chevron_up(COLLECTOR_CHEVRON_SIZE, theme::TEXT_DIM).into()
    }
}

fn with_chevron<'a>(b: &Browser, label: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    row![label.into(), collector_chevron(b)].spacing(COLLECTOR_CHEVRON_SPACING).align_y(Alignment::Center).into()
}

fn collector_ring<'a>(b: &Browser) -> Element<'a, Message> {
    let ring_color = if b.dropping_on_collector() { theme::ACCENT } else { theme::TEXT_DIM };
    let fill = if b.collector.is_empty() { theme::PANEL_HI } else { theme::DANGER };
    container(dot(fill, COLLECTOR_RING_DOT))
        .padding(COLLECTOR_RING_PADDING)
        .style(move |_| {
            depth::well(theme::WELL, COLLECTOR_RING_WIDTH / 2.0)
                .border(border::rounded(COLLECTOR_RING_WIDTH / 2.0).width(2).color(ring_color.scale_alpha(0.7)))
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
    let dragging = b.active_drag().is_some();
    let hot = b.dropping_on_collector();
    text(if dragging { "Drop here to collect" } else { "Drag items here (or press C) to collect them" })
        .size(COLLECTOR_TEXT_SIZE)
        .color(if hot { theme::TEXT } else { theme::TEXT_DIM })
        .into()
}

fn collector_button<'a>(label: String, bg: Color, msg: Message) -> Element<'a, Message> {
    filled_button(text(label).size(COLLECTOR_BUTTON_TEXT_SIZE).wrapping(Wrapping::None), bg, |b| b.on_press(msg))
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
        CollectorFit::Full => generous_text_width(&full_summary, COLLECTOR_TEXT_SIZE) + COLLECTOR_CHEVRON_WIDTH,
        _ => (generous_text_width(&count, COLLECTOR_TEXT_SIZE) + COLLECTOR_CHEVRON_WIDTH)
            .max(generous_text_width(&size, COLLECTOR_TEXT_SIZE)),
    };
    let fit = CollectorFit::WIDEST_FIRST
        .into_iter()
        .find(|&fit| fit.ring_width() + summary_needs(fit) + buttons(fit) <= width)
        .unwrap_or(CollectorFit::CompactWithoutRing);
    let summary_width = width - fit.ring_width() - buttons(fit);

    let label_width = summary_width - COLLECTOR_CHEVRON_WIDTH;
    let summary: Element<'_, Message> = if fit == CollectorFit::Full {
        with_chevron(
            b,
            text(fit_width(&full_summary, label_width, COLLECTOR_TEXT_SIZE))
                .size(COLLECTOR_TEXT_SIZE)
                .wrapping(Wrapping::None),
        )
    } else {
        column![
            with_chevron(
                b,
                text(fit_width(&count, label_width, COLLECTOR_TEXT_SIZE))
                    .size(COLLECTOR_TEXT_SIZE)
                    .wrapping(Wrapping::None)
            ),
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

const LIST_PANEL_PADDING: f32 = 14.0;
const LIST_PANEL_RADIUS: f32 = 10.0;
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
const LIST_ROW_RADIUS: f32 = 6.0;
const SELECTED_ROW_ACCENT: f32 = 0.6;
const SEARCH_MATCH_TINT: f32 = 0.12;
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
    depth::casting_shadow(
        container(responsive(move |size| list_panel_content(b, size)))
            .id(LIST_PANEL_ID)
            .padding(LIST_PANEL_PADDING)
            .width(Fill)
            .height(Fill)
            .style(|_| depth::surface(theme::PANEL, LIST_PANEL_RADIUS)),
        LIST_PANEL_RADIUS,
        Elevation::Panel,
    )
    .width(Length::FillPortion(LIST_SIDE_PORTION))
    .into()
}

fn list_panel_content(b: &Browser, panel_size: iced::Size) -> Element<'_, Message> {
    let width = panel_size.width;
    let subject = b.listed_folder();
    let t = &b.tree;

    let mut header = column![name_and_size_title(b, subject, width, LIST_HEADER_TITLE)].spacing(4);
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

    let mut panel =
        column![header, scrollable(rows).height(Fill).spacing(LIST_SCROLLBAR_GAP).style(depth::slim_scrollbar)]
            .spacing(12);
    if let Some(details) = selection_details(b, panel_size) {
        panel = panel.push(column![separator(), details].spacing(12));
    }
    panel.into()
}

const DETAILS_TITLE_TEXT_SIZE: f32 = 14.0;
const DETAILS_TEXT_SIZE: f32 = 12.0;
const DETAILS_SPACING: f32 = 8.0;
const BULLET: &str = "•";
const GROOVE_SHADOW_ALPHA: f32 = 0.35;
const GROOVE_LIGHT_ALPHA: f32 = 0.05;
const BULLET_SPACING: f32 = 6.0;
const EXPLANATION_SHARE_OF_THE_PANEL: f32 = 0.5;
const PREVIEW_SHORTCUT: &str = "Space";
const OPEN_FULL_DISK_ACCESS_LABEL: &str = "Open Full Disk Access Settings";

fn separator<'a>() -> Element<'a, Message> {
    let line =
        |color: Color| container(Space::new()).width(Fill).height(1).style(move |_| container::background(color));
    column![line(Color::BLACK.scale_alpha(GROOVE_SHADOW_ALPHA)), line(Color::WHITE.scale_alpha(GROOVE_LIGHT_ALPHA))]
        .into()
}

fn selection_details(b: &Browser, panel: iced::Size) -> Option<Element<'_, Message>> {
    let id = b.selected?;
    if let Some(explanation) = dscan_core::hidden::explain(&b.tree, id) {
        return Some(unscanned_space_explanation(b, id, explanation, panel));
    }
    b.tree.is_actionable(id).then(|| selection_actions(b, id, panel.width))
}

struct TitleStyle {
    text_size: f32,
    dot: f32,
    spacing: f32,
}

const LIST_HEADER_TITLE: TitleStyle =
    TitleStyle { text_size: LIST_HEADER_TEXT_SIZE, dot: LIST_HEADER_DOT, spacing: LIST_HEADER_SPACING };
const DETAILS_TITLE: TitleStyle =
    TitleStyle { text_size: DETAILS_TITLE_TEXT_SIZE, dot: LIST_ROW_DOT, spacing: LIST_ROW_SPACING };

fn name_and_size_title(b: &Browser, id: NodeId, width: f32, style: TitleStyle) -> Element<'_, Message> {
    let size = format_size(b.size(id));
    let name_width = width - style.dot - 2.0 * style.spacing - generous_text_width(&size, style.text_size);
    row![
        dot(theme::kind_color(b.tree.node(id).kind()), style.dot),
        clipped(
            text(fit_width(&b.tree.name(id), name_width, style.text_size))
                .size(style.text_size)
                .wrapping(Wrapping::None)
        ),
        text(size).size(style.text_size).wrapping(Wrapping::None),
    ]
    .spacing(style.spacing)
    .align_y(Alignment::Center)
    .into()
}

fn selection_actions(b: &Browser, id: NodeId, width: f32) -> Element<'_, Message> {
    let preview =
        tooltip_at(outlined_text_button(PREVIEW_ACTION, Message::Preview), PREVIEW_SHORTCUT, tooltip::Position::Top);
    let actions = row![preview, outlined_text_button(REVEAL_ACTION, Message::Reveal)].spacing(DETAILS_SPACING);
    column![name_and_size_title(b, id, width, DETAILS_TITLE), actions].spacing(DETAILS_SPACING).into()
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
    if let Some(unreadable) = dscan_core::hidden::unreadable_items_behind(&b.tree, id) {
        let note = format!("{} folders couldn't be read in this scan.", thousands(unreadable));
        body = body.push(details_text(note).color(theme::WARNING));
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
    let body = container(scrollable(body).spacing(LIST_SCROLLBAR_GAP).style(depth::slim_scrollbar))
        .max_height(panel.height * EXPLANATION_SHARE_OF_THE_PANEL);
    column![name_and_size_title(b, id, panel.width, DETAILS_TITLE), body].spacing(DETAILS_SPACING).into()
}

fn list_row<'a>(b: &'a Browser, r: &'a ListRow, columns: &ListColumns) -> Element<'a, Message> {
    let id = r.id;
    let n = b.tree.node(id);
    let kind_color = theme::kind_color(n.kind());
    let collected = b.collector.contains(id);
    let name_color = if collected && !columns.shows_bar {
        theme::DANGER
    } else if n.is_synthetic() || n.accounting() == Accounting::Unreadable {
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

    let selected = b.selected == Some(id);
    let tint = if b.hovered == Some(Segment::Node(id)) {
        Some(HOVER_TINT)
    } else if b.search.as_ref().is_some_and(|s| s.is_match(id)) {
        Some(theme::ACCENT.scale_alpha(SEARCH_MATCH_TINT))
    } else {
        None
    };
    let cell = container(content).padding([4.0, LIST_ROW_PADDING_X]).width(Fill).style(move |_| {
        if selected {
            return depth::surface(theme::mix(theme::PANEL, theme::ACCENT, SELECTED_ROW_ACCENT), LIST_ROW_RADIUS);
        }
        let style = container::Style::default().border(border::rounded(LIST_ROW_RADIUS));
        match tint {
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
