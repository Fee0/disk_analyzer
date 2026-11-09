use std::path::Path;

use dscan_core::ScanOptions;
use dscan_core::hidden::HIDDEN_SPACE_NAME;
use iced::Settings;
use iced_test::{Error, Simulator};

use crate::app::test_util::{app_showing, descend, on_screen, on_screen_mut, scan_result, scanned};
use crate::app::{App, Browser, Message, Notice, NoticeKind, Screen};
use crate::iced_inspect as ui_inspect;
use crate::{theme, view};

const VIEW: (f32, f32) = (view::DEFAULT_WINDOW_SIZE.width, view::DEFAULT_WINDOW_SIZE.height);
const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/iced-inspect");
const A_CHART_SEGMENT: (f32, f32) = (440.0, 320.0);
const CLOSE_BUTTON_COLUMN_X: f32 = 960.0;

pub(crate) fn write_files(root: &Path, files: &[(&str, usize)], fill: u8) {
    for &(relative, size) in files {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![fill; size]).unwrap();
    }
}

fn fixture(root: &Path) {
    let files: &[(&str, usize)] = &[
        ("Movies/2015/IMG_7114.MOV", 2_400_000),
        ("Movies/2015/IMG_0818.MOV", 1_350_000),
        ("Movies/2015/IMG_9410.MOV", 1_050_000),
        ("Movies/2016/clip.mp4", 900_000),
        ("Pictures/a.jpg", 600_000),
        ("Pictures/b.png", 300_000),
        ("Pictures/raw/c.dng", 700_000),
        ("Music/song.mp3", 400_000),
        ("Code/proj/src/main.rs", 80_000),
        ("Code/proj/target/debug/app", 900_000),
        ("Downloads/installer.dmg", 1_200_000),
        ("Downloads/archive.zip", 500_000),
        ("Documents/report.pdf", 250_000),
        ("notes.txt", 20_000),
    ];
    write_files(root, files, 0x5a);
}

fn scanned_fixture(dir: &Path) -> Browser {
    fixture(dir);
    scanned_folder(dir)
}

fn browser_app(dir: &Path) -> App {
    app_showing(scanned_fixture(dir))
}

const LONGER_THAN_ANY_CHART_ANIMATION: std::time::Duration = std::time::Duration::from_secs(5);

fn finish_chart_animation(app: &mut App) {
    let first_frame = std::time::Instant::now();
    let _ = app.update(Message::AnimationFrame(first_frame));
    let _ = app.update(Message::AnimationFrame(first_frame + LONGER_THAN_ANY_CHART_ANIMATION));
    assert!(on_screen(app).animation.is_none(), "the animation ends once its time is up");
}

fn shoot_chart_animation(app: &mut App, name: &str, kind: dscan_core::TransitionKind) -> Result<(), Error> {
    let animation = on_screen(app).animation.as_ref().expect("the change animates the chart");
    assert_eq!(animation.kind(), kind, "{name}");
    for percent in [25, 50, 75] {
        on_screen_mut(app).animation.as_mut().unwrap().jump_to(f64::from(percent) / 100.0);
        let mut ui = sim(app);
        shoot(&mut ui, &format!("{name}-{percent}"))?;
    }
    finish_chart_animation(app);
    Ok(())
}

#[test]
fn chart_reveals_zooms_and_lets_collected_items_shrink_away() -> Result<(), Error> {
    use dscan_core::TransitionKind;
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_showing(scanned_fixture(dir.path()).with_opening_reveal());
    shoot_chart_animation(&mut app, "anim-reveal", TransitionKind::Reveal)?;

    let movies = descend(&on_screen(&app).tree, &["Movies"]);
    let _ = app.update(Message::Navigate(movies));
    shoot_chart_animation(&mut app, "anim-zoom-in", TransitionKind::ZoomIn)?;
    let _ = app.update(Message::Back);
    shoot_chart_animation(&mut app, "anim-zoom-out", TransitionKind::ZoomOut)?;

    let size_before = on_screen(&app).layout.total;
    let pictures = descend(&on_screen(&app).tree, &["Pictures"]);
    let _ = app.update(Message::Press(dscan_core::Segment::Node(pictures)));
    let _ = app.update(Message::CollectSelected);
    let b = on_screen(&app);
    assert_eq!(b.collector.items(), &[pictures]);
    assert_eq!(b.layout.total, size_before - b.tree.size(pictures), "the chart leaves out what is collected");
    assert!(b.layout.find(dscan_core::Segment::Node(pictures)).is_none(), "the collected folder has no arc");
    assert!(b.list.rows.iter().all(|r| r.id != pictures), "nor a row in the list");
    shoot_chart_animation(&mut app, "anim-collect", TransitionKind::Morph)?;

    let _ = app.update(Message::Uncollect(pictures));
    assert_eq!(on_screen(&app).layout.total, size_before, "uncollecting puts it back");
    shoot_chart_animation(&mut app, "anim-uncollect", TransitionKind::Morph)?;
    Ok(())
}

fn deliver_search_result_without_its_thread(app: &mut App, query: &str) {
    if let Screen::Browse(b) = &app.screen {
        let result = dscan_core::search(&b.tree, query, &Default::default()).map(std::sync::Arc::new);
        let _ = app.update(Message::ForBrowser(b.id, Box::new(Message::SearchDone { generation: 1, result })));
    }
}

fn sim(app: &App) -> Simulator<'_, Message> {
    Simulator::with_size(Settings::default(), VIEW, view::view(app))
}

fn shoot(ui: &mut Simulator<'_, Message>, name: &str) -> Result<(), Error> {
    println!("shot: {}", ui_inspect::shot(ui, &theme::app_theme(), OUT, name)?.display());
    Ok(())
}

#[test]
fn inspect_start() -> Result<(), Error> {
    let (app, _) = App::new(None);
    let mut ui = sim(&app);
    println!("{}", ui_inspect::report(&mut ui, VIEW));
    shoot(&mut ui, "start-without-scans")?;
    Ok(())
}

#[test]
fn inspect_browse() -> Result<(), Error> {
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    {
        let mut ui = sim(&app);
        println!("{}", ui_inspect::report(&mut ui, VIEW));
        shoot(&mut ui, "browse")?;
        let file_count =
            |text: &str| text.strip_suffix(" files").is_some_and(|n| n.chars().all(|c| c.is_ascii_digit() || c == ','));
        assert!(
            !ui_inspect::nodes(&mut ui).iter().any(|n| n.text.as_deref().is_some_and(file_count)),
            "the folder header shows its size without a file count"
        );
    }
    let msgs: Vec<Message> = {
        let mut ui = sim(&app);
        ui_inspect::hover(&mut ui, A_CHART_SEGMENT);
        shoot(&mut ui, "chart-tooltip")?;
        ui_inspect::click_at(&mut ui, A_CHART_SEGMENT);
        ui.click("Pictures/")?;
        ui.into_messages().collect()
    };
    println!("messages: {msgs:?}");
    for m in msgs {
        let _ = app.update(m);
    }
    let _ = app.update(Message::MouseReleased);
    let _ = app.update(Message::SearchChanged("img".into()));
    deliver_search_result_without_its_thread(&mut app, "img");
    assert_eq!(on_screen(&app).search.as_ref().unwrap().matches, 3);
    {
        let mut ui = sim(&app);
        println!("{}", ui_inspect::report(&mut ui, VIEW));
        shoot(&mut ui, "browse-after")?;
    }
    Ok(())
}

#[test]
fn toolbar_shows_only_on_disk_sizes_with_a_rescan_text_button() -> Result<(), Error> {
    let dir = tempfile::tempdir().unwrap();
    let app = browser_app(dir.path());

    let mut ui = sim(&app);
    let texts: Vec<String> = ui_inspect::nodes(&mut ui).into_iter().filter_map(|n| n.text).collect();
    for size_choice in ["On disk", "Logical"] {
        assert!(
            !texts.iter().any(|t| t == size_choice),
            "sizes are always on disk, so there is no {size_choice:?} choice"
        );
    }
    ui.click("Rescan")?;
    let msgs: Vec<Message> = ui.into_messages().collect();
    assert!(matches!(msgs[..], [Message::Rescan]), "the Rescan button rescans: {msgs:?}");
    Ok(())
}

fn bounds_of_text(ui: &mut Simulator<'_, Message>, label: &str) -> iced::Rectangle {
    ui_inspect::nodes(ui)
        .into_iter()
        .find(|n| n.kind == "text" && n.text.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no text {label:?} on screen"))
        .bounds
}

#[test]
fn toolbar_lines_up_with_the_file_list_and_a_long_path_stops_before_it() -> Result<(), Error> {
    let dir = tempfile::tempdir().unwrap();
    let chain =
        ["a-rather-long-folder-name", "another-long-folder-name", "and-one-more-long-folder", "deeper", "deepest"];
    let deepest = chain.iter().fold(dir.path().to_path_buf(), |p, name| p.join(name));
    std::fs::create_dir_all(&deepest).unwrap();
    std::fs::write(deepest.join("file.bin"), vec![0u8; 50_000]).unwrap();
    let mut app = browser_app(dir.path());
    let deepest = descend(&on_screen(&app).tree, &chain);
    let _ = app.update(Message::Navigate(deepest));

    let mut ui = sim(&app);
    shoot(&mut ui, "toolbar-aligned")?;
    let nodes = ui_inspect::nodes(&mut ui);
    let bounds_of =
        |id: &str| nodes.iter().find(|n| n.id.as_deref() == Some(id)).unwrap_or_else(|| panic!("no {id}")).bounds;
    let (toolbar_list_side, list) = (bounds_of(view::TOOLBAR_LIST_SIDE_ID), bounds_of(view::LIST_PANEL_ID));
    assert_eq!(toolbar_list_side.x, list.x, "Rescan starts where the file list starts");
    assert_eq!(
        toolbar_list_side.x + toolbar_list_side.width,
        list.x + list.width,
        "search ends where the file list ends"
    );
    let path_bar = bounds_of(view::PATH_BAR_ID);
    assert!(path_bar.x + path_bar.width < list.x, "the path bar stops before the file list: {path_bar:?} vs {list:?}");
    let current = nodes
        .iter()
        .find(|n| n.text.as_deref() == Some("deepest"))
        .expect("the current folder is in the path bar")
        .bounds;
    assert!(
        current.x + current.width <= path_bar.x + path_bar.width,
        "the current folder is not cut off: {current:?} vs {path_bar:?}"
    );
    assert!(nodes.iter().any(|n| n.text.as_deref() == Some("…")), "folders that don't fit fold into the ellipsis");
    let search = nodes.iter().find(|n| n.kind == "text_input").expect("search field").bounds;
    assert_eq!(search.x + search.width, list.x + list.width, "the search field takes the rest of the width");
    Ok(())
}

#[test]
fn deep_path_bar_shows_root_ellipsis_and_last_folders_and_the_ellipsis_steps_into_the_hidden_part() -> Result<(), Error>
{
    let dir = tempfile::tempdir().unwrap();
    let chain = ["Projects", "clients", "acme", "website", "build", "assets", "images"];
    let deepest = chain.iter().fold(dir.path().to_path_buf(), |p, name| p.join(name));
    std::fs::create_dir_all(&deepest).unwrap();
    std::fs::write(deepest.join("hero.png"), vec![0u8; 50_000]).unwrap();
    let mut app = browser_app(dir.path());
    let images = descend(&on_screen(&app).tree, &chain);
    let _ = app.update(Message::Navigate(images));

    let msgs: Vec<Message> = {
        let mut ui = sim(&app);
        println!("{}", ui_inspect::report(&mut ui, VIEW));
        shoot(&mut ui, "path-bar-deep")?;
        for hidden in ["Projects", "clients"] {
            assert!(ui.find(hidden).is_err(), "{hidden} is folded into the ellipsis");
        }
        ui.click("…")?;
        ui.into_messages().collect()
    };
    let clients = descend(&on_screen(&app).tree, &chain[..2]);
    assert!(
        matches!(msgs[..], [Message::Navigate(id)] if id == clients),
        "the ellipsis opens the deepest folder it hides: {msgs:?}"
    );

    let mut ui = sim(&app);
    let back_button = ui_inspect::nodes(&mut ui)
        .into_iter()
        .find(|n| n.text.as_deref() == Some("acme"))
        .map(|acme| iced::Point::new(54.0, acme.bounds.center_y()))
        .unwrap();
    ui_inspect::hover(&mut ui, back_button);
    shoot(&mut ui, "path-bar-back-hover")?;
    Ok(())
}

fn key(app: &mut App, k: iced::keyboard::Key, cmd: bool) {
    let mods = if cmd { iced::keyboard::Modifiers::COMMAND } else { iced::keyboard::Modifiers::empty() };
    let _ = app.update(Message::Key(k, mods));
}

#[test]
fn keyboard_navigation() {
    use iced::keyboard::{Key, key::Named};
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    let root = on_screen(&app).tree.root();

    key(&mut app, Key::Named(Named::ArrowRight), false);
    let b = on_screen(&app);
    let first = b.selected.unwrap();
    assert_eq!(b.tree.name(first), "Movies");

    key(&mut app, Key::Named(Named::ArrowRight), false);
    assert_eq!(on_screen(&app).tree.name(on_screen(&app).selected.unwrap()), "Downloads");
    key(&mut app, Key::Named(Named::ArrowLeft), false);
    assert_eq!(on_screen(&app).selected, Some(first));

    key(&mut app, Key::Named(Named::ArrowDown), false);
    let b = on_screen(&app);
    assert_eq!(b.tree.name(b.selected.unwrap()), "2015");
    assert_eq!(b.listed_folder(), first, "list follows the selection's parent");
    key(&mut app, Key::Named(Named::ArrowUp), false);
    assert_eq!(on_screen(&app).selected, Some(first));

    key(&mut app, Key::Named(Named::Enter), false);
    assert_eq!(on_screen(&app).focus, first, "Enter drills into a folder");
    key(&mut app, Key::Named(Named::Backspace), false);
    assert_eq!(on_screen(&app).focus, root, "Backspace goes back");
    key(&mut app, Key::Character("]".into()), true);
    assert_eq!(on_screen(&app).focus, first, "Cmd-] goes forward");
    key(&mut app, Key::Named(Named::ArrowUp), true);
    assert_eq!(on_screen(&app).focus, root, "Cmd-Up goes to the parent");

    key(&mut app, Key::Character("c".into()), false);
    assert_eq!(on_screen(&app).collector.items(), &[first], "C collects the selection");
    key(&mut app, Key::Named(Named::Escape), false);
    assert_eq!(on_screen(&app).selected, None);
}

#[test]
fn the_start_page_is_one_step_back_from_the_results_root() -> Result<(), Error> {
    use iced::keyboard::Key;
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    let root = on_screen(&app).tree.root();
    let movies = descend(&on_screen(&app).tree, &["Movies"]);
    let _ = app.update(Message::Navigate(movies));

    let _ = app.update(Message::Back);
    assert_eq!(on_screen(&app).focus, root, "back walks the folder history first");
    let msgs: Vec<Message> = {
        let mut ui = sim(&app);
        let back_button = toolbar_icon_buttons_left_to_right(&mut ui)[1];
        ui_inspect::click_at(&mut ui, (back_button.center_x(), back_button.center_y()));
        ui.into_messages().collect()
    };
    assert!(matches!(msgs[..], [Message::Back]), "back stays clickable at the results root: {msgs:?}");
    for m in msgs {
        let _ = app.update(m);
    }
    assert!(matches!(app.screen, Screen::Start), "back from the root shows the start page");
    assert_eq!(app.parked_newest_first.len(), 1, "the results stay open");
    let _ = app.update(Message::Back);
    assert!(matches!(app.screen, Screen::Start), "nothing comes before the start page");

    let _ = app.update(Message::Forward);
    let b = on_screen(&app);
    assert_eq!(b.focus, root, "forward returns to the results where they were left");
    assert!(app.parked_newest_first.is_empty());
    let _ = app.update(Message::Forward);
    assert_eq!(on_screen(&app).focus, movies, "the results keep their own forward history");

    key(&mut app, Key::Character("[".into()), true);
    key(&mut app, Key::Character("[".into()), true);
    assert!(matches!(app.screen, Screen::Start), "Cmd-[ steps back to the start page too");
    key(&mut app, Key::Character("]".into()), true);
    assert_eq!(on_screen(&app).focus, root, "and Cmd-] steps forward into the results again");

    let _ = app.update(Message::GoHome);
    let _ = app.update(Message::CloseParked(app.parked_newest_first[0].id));
    let _ = app.update(Message::Forward);
    assert!(matches!(app.screen, Screen::Start), "closed results can't be stepped into");
    Ok(())
}

fn toolbar_icon_buttons_left_to_right(ui: &mut Simulator<'_, Message>) -> Vec<iced::Rectangle> {
    let toolbar_bottom = bounds_of_text(ui, "Rescan").y + TOOLBAR_DEPTH_BELOW_RESCAN_LABEL;
    let mut squares: Vec<iced::Rectangle> = ui_inspect::nodes(ui)
        .into_iter()
        .map(|n| n.bounds)
        .filter(|b| b.width == b.height && b.width >= MIN_ICON_BUTTON_SIDE && b.y < toolbar_bottom)
        .collect();
    squares.sort_by(|a, b| a.x.total_cmp(&b.x));
    squares.dedup_by(|a, b| a.x == b.x);
    assert!(squares.len() >= 3, "the toolbar has home, back and forward buttons: {squares:?}");
    squares
}

const TOOLBAR_DEPTH_BELOW_RESCAN_LABEL: f32 = 20.0;
const MIN_ICON_BUTTON_SIDE: f32 = 20.0;

#[test]
fn f5_or_cmd_r_scans_the_shown_results_again() {
    use iced::keyboard::{Key, key::Named};
    let dir = tempfile::tempdir().unwrap();
    for (shortcut, cmd) in [(Key::Named(Named::F5), false), (Key::Character("r".into()), true)] {
        let mut app = browser_app(dir.path());
        let shown_root = on_screen(&app).opts.root.clone();
        key(&mut app, shortcut.clone(), cmd);
        let job = app.scans.last().unwrap_or_else(|| panic!("{shortcut:?} starts a scan"));
        assert_eq!(job.opts.root, shown_root, "{shortcut:?} rescans what was shown");
    }
    let mut app = browser_app(dir.path());
    key(&mut app, Key::Character("r".into()), false);
    assert!(app.scans.is_empty(), "a plain R is not a rescan");
}

#[test]
fn drag_to_collector() {
    use dscan_core::Segment;
    use iced::Point;
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    let b = on_screen(&app);
    let movies = b.tree.children(b.tree.root()).next().unwrap();

    let _ = app.update(Message::Press(Segment::Node(movies)));
    let _ = app.update(Message::CollectorEnter);
    let _ = app.update(Message::MouseReleased);
    assert!(on_screen(&app).collector.is_empty(), "a click without movement selects but does not collect");
    let _ = app.update(Message::CollectorExit);

    let _ = app.update(Message::Press(Segment::Node(movies)));
    let _ = app.update(Message::CursorMoved(Point::new(100.0, 100.0)));
    let _ = app.update(Message::CursorMoved(Point::new(140.0, 160.0)));
    assert!(on_screen(&app).drag.as_ref().unwrap().dragging, "moving past the threshold starts a drag");
    let _ = app.update(Message::CollectorEnter);
    let _ = app.update(Message::MouseReleased);
    assert_eq!(on_screen(&app).collector.items(), &[movies], "releasing a drag over the collector collects");
    assert!(on_screen(&app).drag.is_none(), "releasing ends the drag");

    let b = on_screen(&app);
    let child = b.tree.children(movies).next().unwrap();
    let _ = app.update(Message::Press(Segment::Node(child)));
    let _ = app.update(Message::CollectSelected);
    assert_eq!(on_screen(&app).collector.items(), &[movies], "collecting a child of a collected folder is a no-op");

    let _ = app.update(Message::ClearCollector);
    let _ = app.update(Message::CollectorExit);
    let _ = app.update(Message::Press(Segment::Node(movies)));
    let _ = app.update(Message::CursorMoved(Point::new(0.0, 0.0)));
    let _ = app.update(Message::CursorMoved(Point::new(50.0, 0.0)));
    let _ = app.update(Message::MouseReleased);
    assert!(on_screen(&app).collector.is_empty(), "a drag released outside the collector collects nothing");
}

const TB: u64 = 1_000_000_000_000;

fn disk(name: &str, mount: &Path, used_share: f64) -> dscan_core::Volume {
    let total = 2 * TB;
    let used = (total as f64 * used_share) as u64;
    dscan_core::Volume {
        name: name.into(),
        mount: mount.to_path_buf(),
        total,
        used,
        available: total - used,
        is_root: false,
        file_system: "APFS".into(),
        is_network: false,
    }
}

fn scan_job(app: &mut App) -> &mut crate::app::ScanJob {
    app.scans.last_mut().expect("a scan is running")
}

fn count_of(texts: &[String], label: &str) -> usize {
    texts.iter().filter(|t| *t == label).count()
}

const MID_SCAN: dscan_core::ProgressSnapshot =
    dscan_core::ProgressSnapshot { files: 673_862, dirs: 163_187, bytes: 86_624_714_752, expected: Some(2 * TB) };

#[test]
fn scanning_a_disk_turns_its_scan_button_into_pause_and_resume() -> Result<(), Error> {
    let mounts = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let (mut app, _) = App::new(None);
    app.volumes = Some(vec![
        disk("Macintosh HD", mounts[0].path(), 0.37),
        disk("Backup", mounts[1].path(), 0.8),
        disk("Archive", mounts[2].path(), 0.96),
    ]);
    {
        let mut ui = sim(&app);
        println!("{}", ui_inspect::report(&mut ui, VIEW));
        shoot(&mut ui, "start-disks")?;
        assert_eq!(count_of(&texts_on_screen(&mut ui), "Scan"), 3, "every disk offers a scan");
    }

    let _scan_task_never_run = app.update(Message::ScanVolume(app.volumes.as_ref().unwrap()[0].mount.clone()));
    assert!(matches!(app.screen, Screen::Start) && app.scans.len() == 1, "the scan runs inside the disk list");
    assert_eq!(app.title(), "Scanning Macintosh HD — Disk Analyzer");
    scan_job(&mut app).snapshot = MID_SCAN;
    let progress_label = format!("{} · 673,862 files", dscan_core::format_size(MID_SCAN.bytes));
    for percent in [10, 40, 80] {
        let bytes = MID_SCAN.expected.unwrap() / 100 * percent;
        scan_job(&mut app).snapshot = dscan_core::ProgressSnapshot { bytes, ..MID_SCAN };
        let mut ui = sim(&app);
        shoot(&mut ui, &format!("scanning-{percent}"))?;
    }
    scan_job(&mut app).snapshot = MID_SCAN;
    let job_id = scan_job(&mut app).id;
    let mut ui = sim(&app);
    let texts = texts_on_screen(&mut ui);
    assert!(texts.contains(&progress_label), "the scanned size and file count replace the free space: {texts:?}");
    assert_eq!(count_of(&texts, "Pause"), 1, "the scanned disk's Scan button became Pause");
    assert_eq!(count_of(&texts, "Scan"), 2, "the other disks keep their scan button");
    assert!(texts.contains(&"Backup".to_string()) && texts.contains(&"Archive".to_string()), "the list stays");
    let scan_button = bounds_of_text(&mut ui, "Scan");
    let pause_button = bounds_of_text(&mut ui, "Pause");
    assert_eq!(
        (pause_button.x + pause_button.width, pause_button.width),
        (scan_button.x + scan_button.width, scan_button.width),
        "Pause takes the Scan button's place and size"
    );
    let stop = icon_button_right_of(&mut ui, pause_button).expect("a scanning row has a stop button beside Pause");
    ui_inspect::click_at(&mut ui, (stop.center_x(), stop.center_y()));
    ui.click("Pause")?;
    let msgs: Vec<Message> = ui.into_messages().collect();
    assert!(
        matches!(msgs[..], [Message::StopScan(stop), Message::TogglePause(pause)] if stop == job_id && pause == job_id),
        "{msgs:?}"
    );

    let _ = app.update(Message::TogglePause(job_id));
    assert!(scan_job(&mut app).cancel.is_paused());
    {
        let mut ui = sim(&app);
        let paused_label = format!("Paused · {}", dscan_core::format_size(MID_SCAN.bytes));
        let texts = texts_on_screen(&mut ui);
        assert!(texts.contains(&paused_label));
        assert_eq!(count_of(&texts, "Resume"), 1, "a paused scan offers Resume in the same place");
        shoot(&mut ui, "paused")?;
        ui.click("Resume")?;
        let msgs: Vec<Message> = ui.into_messages().collect();
        assert!(matches!(msgs[..], [Message::TogglePause(id)] if id == job_id), "{msgs:?}");
    }
    let _ = app.update(Message::TogglePause(job_id));
    assert!(!scan_job(&mut app).cancel.is_paused());

    let cancel = scan_job(&mut app).cancel.clone();
    let _ = app.update(Message::StopScan(job_id));
    assert!(cancel.is_cancelled() && app.scans.is_empty(), "stopping a scan cancels it and frees its row");
    assert_eq!(count_of(&texts_on_screen(&mut sim(&app)), "Scan"), 3, "the disk offers a scan again");
    Ok(())
}

fn icon_button_right_of(ui: &mut Simulator<'_, Message>, button: iced::Rectangle) -> Option<iced::Rectangle> {
    ui_inspect::nodes(ui)
        .into_iter()
        .map(|n| n.bounds)
        .filter(|b| b.width == b.height && b.x >= button.x + button.width)
        .filter(|b| (b.center_y() - button.center_y()).abs() < 1.0)
        .min_by(|a, b| a.x.total_cmp(&b.x))
}

#[test]
fn a_folder_scan_gets_its_own_row_and_runs_beside_a_disk_scan() -> Result<(), Error> {
    let mount = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    fixture(folder.path());
    let (mut app, _) = App::new(None);
    app.volumes = Some(vec![disk("Macintosh HD", mount.path(), 0.5)]);
    app.screen = Screen::Browse(Box::new(scanned_folder(folder.path())));
    let _ = app.update(Message::GoHome);

    let _scan_task_never_run = app.update(Message::FolderPicked(Some(folder.path().to_path_buf())));
    scan_job(&mut app).snapshot = dscan_core::ProgressSnapshot { expected: None, ..MID_SCAN };
    let folder_name = scan_job(&mut app).opts.display_name();
    {
        let mut ui = sim(&app);
        shoot(&mut ui, "scanning-folder")?;
        let texts = texts_on_screen(&mut ui);
        assert!(count_of(&texts, &folder_name) >= 1, "the folder is listed under the disks: {texts:?}");
        assert_eq!((count_of(&texts, "Pause"), count_of(&texts, "Scan")), (1, 1), "{texts:?}");
    }
    let msgs: Vec<Message> = {
        let mut ui = sim(&app);
        let open_result = ui_inspect::nodes(&mut ui)
            .into_iter()
            .filter(|n| n.text.as_deref() == Some(folder_name.as_str()))
            .map(|n| n.bounds)
            .max_by(|a, b| a.y.total_cmp(&b.y))
            .expect("the open result is listed below the disks");
        ui_inspect::click_at(&mut ui, (open_result.center_x(), open_result.center_y()));
        ui.into_messages().collect()
    };
    assert!(
        matches!(msgs[..], [Message::ResumeParked(id)] if id == app.parked_newest_first[0].id),
        "open results stay open-able during a scan: {msgs:?}"
    );

    let (folder_job, folder_cancel) = (scan_job(&mut app).id, scan_job(&mut app).cancel.clone());
    let _scan_task_never_run = app.update(Message::ScanVolume(app.volumes.as_ref().unwrap()[0].mount.clone()));
    assert!(!folder_cancel.is_cancelled(), "starting another scan leaves the folder scan running");
    assert_eq!(app.scans.len(), 2);
    assert_eq!(app.title(), format!("Scanning {folder_name}, Macintosh HD — Disk Analyzer"));
    scan_job(&mut app).snapshot = MID_SCAN;
    {
        let mut ui = sim(&app);
        shoot(&mut ui, "scanning-two")?;
        let texts = texts_on_screen(&mut ui);
        assert_eq!((count_of(&texts, "Pause"), count_of(&texts, "Scan")), (2, 0), "{texts:?}");
    }

    let disk_cancel = scan_job(&mut app).cancel.clone();
    let _ = app.update(Message::ResumeParked(app.parked_newest_first[0].id));
    assert!(matches!(app.screen, Screen::Browse(_)), "opening a result works while scans run");
    assert!(!disk_cancel.is_cancelled() && !folder_cancel.is_cancelled(), "scans keep running behind a result");

    finish_scan_of(&mut app, folder_job, folder.path());
    assert_eq!(app.scans.len(), 1, "the disk scan is still running");
    assert!(
        app.parked_newest_first.first().is_some_and(|b| b.opts.root == folder.path()),
        "a scan finishing behind a shown result waits under open results"
    );
    let notice = on_screen(&app).notice.as_ref().expect("the finished scan is announced");
    assert_eq!(notice.kind, crate::app::NoticeKind::Done);
    assert!(notice.text.contains(&folder_name), "{notice:?}");
    Ok(())
}

#[test]
fn the_last_running_scan_opens_its_result_and_earlier_ones_wait_under_open_results() {
    let folders = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
    let (mut app, _) = App::new(None);
    let ids: Vec<u64> = folders
        .iter()
        .map(|f| {
            fixture(f.path());
            let _scan_task_never_run = app.update(Message::FolderPicked(Some(f.path().to_path_buf())));
            scan_job(&mut app).id
        })
        .collect();

    finish_scan_of(&mut app, ids[1], folders[1].path());
    assert!(matches!(app.screen, Screen::Start), "the start page stays while another scan runs");
    assert_eq!(app.parked_newest_first.len(), 1);
    assert_eq!(app.parked_newest_first[0].opts.root, folders[1].path());

    finish_scan_of(&mut app, ids[0], folders[0].path());
    assert!(app.scans.is_empty());
    assert!(
        matches!(&app.screen, Screen::Browse(b) if b.opts.root == folders[0].path()),
        "the last scan to finish opens like a lone scan"
    );
    assert_eq!(app.parked_newest_first.len(), 1);
}

#[test]
fn scanning_the_same_place_again_replaces_its_running_scan() {
    let folder = tempfile::tempdir().unwrap();
    let (mut app, _) = App::new(None);
    let _scan_task_never_run = app.update(Message::FolderPicked(Some(folder.path().to_path_buf())));
    let first = scan_job(&mut app).cancel.clone();
    let _scan_task_never_run = app.update(Message::FolderPicked(Some(folder.path().to_path_buf())));
    assert!(first.is_cancelled(), "the earlier scan of the same folder stops");
    assert_eq!(app.scans.len(), 1);
}

fn finish_scan_of(app: &mut App, job_id: u64, folder: &Path) {
    let job = app.scans.iter().find(|job| job.id == job_id).expect("the scan is running");
    *job.result_slot.lock() = Some(scan_result(&ScanOptions::folder(folder)));
    let _ = app.update(Message::ScanFinished { job_id, crash: None });
}

#[test]
fn inspect_open_results() -> Result<(), Error> {
    let open = |app: &mut App, browser: Browser| {
        app.screen = Screen::Browse(Box::new(browser));
        let _ = app.update(Message::GoHome);
    };
    let dirs: Vec<_> = (0..6).map(|_| tempfile::tempdir().unwrap()).collect();
    for d in &dirs {
        fixture(d.path());
    }
    let (mut app, _) = App::new(None);
    app.volumes = Some(dscan_core::list_volumes());
    for d in &dirs {
        open(&mut app, scanned_folder(d.path()));
    }
    assert_eq!(app.parked_newest_first.len(), 6, "every result stays open");
    {
        let mut ui = sim(&app);
        println!("{}", ui_inspect::report(&mut ui, VIEW));
        shoot(&mut ui, "home-results")?;
    }

    let newest = dscan_core::canonical_path(dirs[5].path()).unwrap();
    assert_eq!(app.parked_newest_first[0].tree.root_path(), newest, "most recently viewed result is listed first");
    let name = app.parked_newest_first[0].tree.name(app.parked_newest_first[0].tree.root()).into_owned();
    let msgs: Vec<Message> = {
        let mut ui = sim(&app);
        ui.click(name.as_str())?;
        ui.into_messages().collect()
    };
    assert!(
        matches!(msgs[..], [Message::ResumeParked(id)] if id == app.parked_newest_first[0].id),
        "clicking a row resumes that result: {msgs:?}"
    );
    for m in msgs {
        let _ = app.update(m);
    }
    assert_eq!(on_screen(&app).tree.root_path(), newest, "exactly the clicked result is resumed");
    assert_eq!(app.parked_newest_first.len(), 5);
    let _ = app.update(Message::GoHome);

    let msgs: Vec<Message> = {
        let mut ui = sim(&app);
        let row_of_newest = bounds_of_text(&mut ui, &name).center_y();
        ui_inspect::click_at(&mut ui, (CLOSE_BUTTON_COLUMN_X, row_of_newest));
        ui.into_messages().collect()
    };
    assert!(
        matches!(msgs[..], [Message::CloseParked(id)] if id == app.parked_newest_first[0].id),
        "the × inside a row closes that result instead of opening it: {msgs:?}"
    );

    let _ = app.update(Message::CloseParked(app.parked_newest_first[1].id));
    assert_eq!(app.parked_newest_first.len(), 5, "closing removes just that one result");
    assert!(
        app.parked_newest_first
            .iter()
            .all(|b| b.tree.root_path() != dscan_core::canonical_path(dirs[4].path()).unwrap()),
        "the closed result is gone"
    );

    let _ = app.update(Message::FolderPicked(Some(dirs[0].path().to_path_buf())));
    let job = app.scans.last().expect("scanning");
    let job_id = job.id;
    let fresh_scan = scan_result(&ScanOptions::folder(dirs[0].path()));
    *job.result_slot.lock() = Some(fresh_scan);
    let _ = app.update(Message::ScanFinished { job_id, crash: None });
    assert!(matches!(app.screen, Screen::Browse(_)));
    assert_eq!(app.parked_newest_first.len(), 4, "a fresh scan of an open folder drops its stale result");
    Ok(())
}

#[test]
fn collector_fixed_height_fits_every_state_without_clipping() -> Result<(), Error> {
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    let root = on_screen(&app).tree.root();
    let movies = on_screen(&app).tree.children(root).next().unwrap();
    let _ = app.update(Message::Press(dscan_core::Segment::Node(movies)));
    let _ = app.update(Message::MouseReleased);

    let shot = |app: &App, name: &str| -> Result<(), Error> {
        let mut ui = sim(app);
        let report = ui_inspect::report(&mut ui, VIEW);
        assert!(!report.contains("clipped:"), "{name}: collector content is clipped\n{report}");
        shoot(&mut ui, name)?;
        Ok(())
    };
    shot(&app, "collector-empty")?;
    let _ = app.update(Message::CollectSelected);
    shot(&app, "collector-one")?;
    let kids: Vec<_> = on_screen(&app).tree.children(root).skip(1).collect();
    for id in kids {
        let _ = app.update(Message::Press(dscan_core::Segment::Node(id)));
        let _ = app.update(Message::MouseReleased);
        let _ = app.update(Message::CollectSelected);
    }
    assert!(on_screen(&app).collector.len() > 4);
    shot(&app, "collector-many")?;
    {
        let mut ui = sim(&app);
        let report = ui_inspect::report(&mut ui, VIEW);
        assert!(report.contains("\"Hold 3 s to delete permanently\""), "the hold button says how long to hold");
    }
    on_screen_mut(&mut app).delete_hold_started =
        Some(std::time::Instant::now() - std::time::Duration::from_millis(1500));
    {
        let mut ui = sim(&app);
        let seconds_left: f32 = ui_inspect::nodes(&mut ui)
            .into_iter()
            .find_map(|n| n.text?.strip_prefix("Keep holding… ")?.strip_suffix(" s")?.parse().ok())
            .expect("the hold button says to keep holding");
        assert!(
            (0.0..=1.5).contains(&seconds_left),
            "the label counts down from 3 s, and 1.5 s have passed at layout time: {seconds_left}"
        );
    }
    shot(&app, "collector-holding")?;
    on_screen_mut(&mut app).delete_hold_started = None;
    let _ = app.update(Message::AskTrash);
    shot(&app, "collector-confirm")?;
    let _ = app.update(Message::CancelTrash);
    Ok(())
}

fn list_panel_left(ui: &mut Simulator<'_, Message>) -> f32 {
    ui_inspect::nodes(ui)
        .into_iter()
        .find(|n| n.id.as_deref() == Some(view::LIST_PANEL_ID))
        .expect("the list panel is on screen")
        .bounds
        .x
}

fn collected_rows_over_the_chart(ui: &mut Simulator<'_, Message>, names: &[&str]) -> Vec<String> {
    let chart_side = list_panel_left(ui);
    texts_on_screen_left_of(ui, chart_side).into_iter().filter(|t| names.contains(&t.as_str())).collect()
}

fn texts_on_screen_left_of(ui: &mut Simulator<'_, Message>, x: f32) -> Vec<String> {
    ui_inspect::nodes(ui).into_iter().filter(|n| n.bounds.x < x).filter_map(|n| n.text).collect()
}

fn remove_button_beside(ui: &mut Simulator<'_, Message>, label: &str) -> (f32, f32) {
    let button = bounds_of_button_beside(ui, label);
    (button.center_x(), button.center_y())
}

fn bounds_of_button_beside(ui: &mut Simulator<'_, Message>, label: &str) -> iced::Rectangle {
    let nodes = ui_inspect::nodes(ui);
    let name_at = nodes
        .iter()
        .position(|n| n.text.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no text {label:?} on screen"));
    let name_right = nodes[name_at].bounds.x + nodes[name_at].bounds.width;
    nodes[name_at..]
        .iter()
        .find(|n| n.kind != "text" && n.bounds.x > name_right)
        .map(|n| n.bounds)
        .unwrap_or_else(|| panic!("no remove button beside {label:?}"))
}

fn clicks_only(msgs: impl Iterator<Item = Message>) -> Vec<Message> {
    msgs.filter(|m| {
        !matches!(
            m,
            Message::CollectorEnter
                | Message::CollectorExit
                | Message::NoticeEnter
                | Message::NoticeExit
                | Message::Hover(_)
        )
    })
    .collect()
}

#[test]
fn collected_items_are_listed_only_after_clicking_the_collector() -> Result<(), Error> {
    use iced::keyboard::{Key, key::Named};
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    collect_through_the_ui(&mut app, &["Movies", "Downloads", "Pictures"]);
    let names = ["Movies/", "Downloads/", "Pictures/"];

    let summary = {
        let mut ui = sim(&app);
        assert!(collected_rows_over_the_chart(&mut ui, &names).is_empty(), "the collector starts closed");
        let summary = ui_inspect::nodes(&mut ui)
            .into_iter()
            .find(|n| n.text.as_deref().is_some_and(|t| t.starts_with("3 collected")))
            .expect("the collector sums up what it holds")
            .bounds;
        ui_inspect::click_at(&mut ui, (summary.center_x(), summary.center_y()));
        let msgs = clicks_only(ui.into_messages());
        assert!(matches!(msgs[..], [Message::ToggleCollectedList]), "clicking the collector opens its list: {msgs:?}");
        summary
    };
    let _ = app.update(Message::ToggleCollectedList);

    {
        let mut ui = sim(&app);
        let report = ui_inspect::report(&mut ui, VIEW);
        println!("{report}");
        assert_eq!(
            collected_rows_over_the_chart(&mut ui, &names),
            names,
            "the open list shows every collected item, oldest first"
        );
        let chart_side = list_panel_left(&mut ui);
        assert!(
            texts_on_screen_left_of(&mut ui, chart_side).iter().any(|t| t == "1.7 MB"),
            "each row shows the item's size"
        );
        let still_in_place = ui_inspect::nodes(&mut ui)
            .into_iter()
            .find(|n| n.text.as_deref().is_some_and(|t| t.starts_with("3 collected")))
            .expect("the summary stays")
            .bounds;
        assert_eq!(still_in_place.y, summary.y, "opening the list grows it upward, the bar stays put");
        shoot(&mut ui, "collector-list-open")?;
    }

    let msgs: Vec<Message> = {
        let mut ui = sim(&app);
        let remove = remove_button_beside(&mut ui, "Downloads/");
        ui_inspect::click_at(&mut ui, remove);
        clicks_only(ui.into_messages())
    };
    let downloads = descend(&on_screen(&app).tree, &["Downloads"]);
    assert!(
        matches!(msgs[..], [Message::Uncollect(id)] if id == downloads),
        "a row's close button takes only that item out: {msgs:?}"
    );
    for m in msgs {
        let _ = app.update(m);
    }
    assert!(on_screen(&app).collector.list_open(), "removing one item keeps the list open");
    assert_eq!(collected_rows_over_the_chart(&mut sim(&app), &names), ["Movies/", "Pictures/"]);

    key(&mut app, Key::Named(Named::Escape), false);
    assert!(!on_screen(&app).collector.list_open(), "Escape closes the list");

    let _ = app.update(Message::ToggleCollectedList);
    let _ = app.update(Message::ClearCollector);
    assert!(!on_screen(&app).collector.list_open(), "an emptied collector closes its list");
    let _ = app.update(Message::ToggleCollectedList);
    assert!(!on_screen(&app).collector.list_open(), "an empty collector has no list to open");
    Ok(())
}

const LONG_FAILURE: &str = "Reveal in Explorer failed for “an-extremely-long-folder-name”: the system could not find the file specified, and nothing else could be opened in its place.";

#[test]
fn messages_float_over_the_chart_instead_of_a_status_line() -> Result<(), Error> {
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    {
        let mut ui = sim(&app);
        let texts = texts_on_screen(&mut ui);
        assert!(
            !texts.iter().any(|t| t.contains("scanned in") || t.contains(" files in ")),
            "no status line repeats the scan's counts: {texts:?}"
        );
    }

    let _ = app.update(Message::DesktopActionFailed(LONG_FAILURE.to_string()));
    let msgs = {
        let mut ui = sim(&app);
        let chart_side = list_panel_left(&mut ui);
        let message = bounds_of_text(&mut ui, LONG_FAILURE);
        assert!(message.x + message.width < chart_side, "the notice floats over the chart: {message:?}");
        assert!(message.y < VIEW.1 / 4.0, "the notice sits at the top of the chart: {message:?}");
        shoot(&mut ui, "notice-error")?;
        let dismiss = remove_button_beside(&mut ui, LONG_FAILURE);
        ui_inspect::click_at(&mut ui, dismiss);
        clicks_only(ui.into_messages())
    };
    assert!(matches!(msgs[..], [Message::DismissNotice]), "the notice's close button dismisses it: {msgs:?}");
    for m in msgs {
        let _ = app.update(m);
    }
    assert!(!texts_on_screen(&mut sim(&app)).iter().any(|t| t == LONG_FAILURE), "a dismissed notice is gone");

    let _ = app.update(Message::DesktopActionFailed(LONG_FAILURE.to_string()));
    let (hovering, clicking_the_text) = {
        let mut ui = sim(&app);
        let message = bounds_of_text(&mut ui, LONG_FAILURE);
        let on_the_text = (message.center_x(), message.center_y());
        ui_inspect::hover(&mut ui, on_the_text);
        let hovering: Vec<Message> = ui.into_messages().collect();
        let mut ui = sim(&app);
        ui_inspect::click_at(&mut ui, on_the_text);
        (hovering, clicks_only(ui.into_messages()))
    };
    assert!(
        hovering.iter().any(|m| matches!(m, Message::NoticeEnter)),
        "hovering the notice holds it open: {hovering:?}"
    );
    assert!(clicking_the_text.is_empty(), "a click on the notice doesn't reach the chart below: {clicking_the_text:?}");

    for (kind, label, text) in [
        (
            NoticeKind::Done,
            "notice-done",
            "Moved 3 items (9.0 MB) to the Recycle Bin. Empty the Recycle Bin to free the space.",
        ),
        (
            NoticeKind::Warning,
            "notice-warning",
            "“System32” is a protected system location and can't be moved to the Recycle Bin.",
        ),
    ] {
        on_screen_mut(&mut app).notice = Some(Notice { kind, text: text.to_string() });
        let mut ui = sim(&app);
        shoot(&mut ui, label)?;
        let dismiss = bounds_of_button_beside(&mut ui, text);
        assert!(
            dismiss.width >= view::NOTICE_DISMISS_BUTTON_SIDE && dismiss.height >= view::NOTICE_DISMISS_BUTTON_SIDE,
            "{label}: the long message leaves the close button its full size: {dismiss:?}"
        );
        ui_inspect::click_at(&mut ui, (dismiss.center_x(), dismiss.center_y()));
        let msgs = clicks_only(ui.into_messages());
        assert!(matches!(msgs[..], [Message::DismissNotice]), "{label}: the close button stays clickable: {msgs:?}");
    }
    Ok(())
}

fn collect_through_the_ui(app: &mut App, names: &[&str]) {
    for name in names {
        let id = descend(&on_screen(app).tree, &[name]);
        let _ = app.update(Message::Press(dscan_core::Segment::Node(id)));
        let _ = app.update(Message::MouseReleased);
        let _ = app.update(Message::CollectSelected);
    }
}

fn layout_problems(ui: &mut Simulator<'_, Message>, size: (f32, f32)) -> Vec<String> {
    let nodes = ui_inspect::nodes(ui);
    ui_inspect::lint(&nodes, size).into_iter().filter(|issue| !issue.starts_with("info:")).collect()
}

#[test]
fn every_screen_stays_readable_from_the_default_window_down_to_the_minimum_size() -> Result<(), Error> {
    let sizes = [
        (view::DEFAULT_WINDOW_SIZE.width, view::DEFAULT_WINDOW_SIZE.height),
        (1000.0, 700.0),
        (view::MIN_WINDOW_SIZE.width, view::MIN_WINDOW_SIZE.height),
    ];
    let dir = tempfile::tempdir().unwrap();
    let long_name = "an-extremely-long-folder-name-that-will-never-fit-anywhere-on-screen";
    let long = dir.path().join(long_name);
    fixture(&long.join("inner"));
    let other_results: Vec<_> = (0..3).map(|_| tempfile::tempdir().unwrap()).collect();

    let (mut start, _) = App::new(None);
    start.volumes = Some(dscan_core::list_volumes());
    start.full_disk_access = false;
    for d in &other_results {
        start.screen = Screen::Browse(Box::new(scanned_fixture(d.path())));
        let _ = start.update(Message::GoHome);
    }
    start.screen = Screen::Browse(Box::new(scanned_folder(&long)));
    let _ = start.update(Message::GoHome);

    let (mut scanning, _) = App::new(None);
    let _scan_task_never_run = scanning.update(Message::FolderPicked(Some(long.clone())));
    scanning.volumes = start.volumes.clone();
    scan_job(&mut scanning).snapshot = MID_SCAN;

    let mut collected_and_searching = browser_app(dir.path());
    let inner = descend(&on_screen(&collected_and_searching).tree, &[long_name, "inner"]);
    let _ = collected_and_searching.update(Message::Navigate(inner));
    collect_through_the_ui(&mut collected_and_searching, &["Movies", "Downloads", "Pictures"]);
    let _ = collected_and_searching.update(Message::SearchChanged("img".into()));
    deliver_search_result_without_its_thread(&mut collected_and_searching, "img");

    let mut holding_delete = browser_app(other_results[0].path());
    collect_through_the_ui(&mut holding_delete, &["Movies", "Downloads"]);
    let _ = holding_delete.update(Message::DeleteHoldStart);

    let mut long_folder_with_unreadable_parts = browser_app(dir.path());
    let long_folder = descend(&on_screen(&long_folder_with_unreadable_parts).tree, &[long_name]);
    let _ = long_folder_with_unreadable_parts.update(Message::Navigate(long_folder));
    if let Screen::Browse(b) = &mut long_folder_with_unreadable_parts.screen {
        std::sync::Arc::get_mut(&mut b.tree).expect("nothing else holds the fresh tree").stats.errors = 1_234;
    }

    let mut confirming_trash = browser_app(other_results[1].path());
    collect_through_the_ui(&mut confirming_trash, &["Movies"]);
    let _ = confirming_trash.update(Message::AskTrash);

    let mut collected_list_open = browser_app(other_results[2].path());
    collect_through_the_ui(
        &mut collected_list_open,
        &["Movies", "Downloads", "Pictures", "Music", "Code", "Documents"],
    );
    let _ = collected_list_open.update(Message::ToggleCollectedList);

    let mut long_notice = browser_app(dir.path());
    let _ = long_notice.update(Message::DesktopActionFailed(LONG_FAILURE.to_string()));

    let mut long_file_selected = browser_app(dir.path());
    let long_folder = descend(&on_screen(&long_file_selected).tree, &[long_name]);
    select(&mut long_file_selected, long_folder);

    let whole_disk = tempfile::tempdir().unwrap();
    let mut hidden_space_explained = whole_disk_scan_of(whole_disk.path());
    let hidden = descend(&on_screen(&hidden_space_explained).tree, &[HIDDEN_SPACE_NAME]);
    select(&mut hidden_space_explained, hidden);

    let screens = [
        ("start", &start),
        ("scanning", &scanning),
        ("collected", &collected_and_searching),
        ("holding", &holding_delete),
        ("long", &long_folder_with_unreadable_parts),
        ("confirm", &confirming_trash),
        ("collected-list", &collected_list_open),
        ("selected", &long_file_selected),
        ("notice", &long_notice),
        ("hidden", &hidden_space_explained),
    ];
    let mut problems = Vec::new();
    for (name, app) in screens {
        for size in sizes {
            let mut ui = Simulator::with_size(Settings::default(), size, view::view(app));
            let shot_name = format!("squash-{name}-{}x{}", size.0, size.1);
            shoot(&mut ui, &shot_name)?;
            problems.extend(layout_problems(&mut ui, size).into_iter().map(|p| format!("{shot_name}: {p}")));
        }
    }
    assert!(problems.is_empty(), "text is cut off, hidden or overlapping:\n{}", problems.join("\n"));
    Ok(())
}

fn scanned_folder(path: &Path) -> Browser {
    scanned(ScanOptions::folder(path))
}

fn select(app: &mut App, id: dscan_core::NodeId) {
    let _ = app.update(Message::Press(dscan_core::Segment::Node(id)));
    let _ = app.update(Message::MouseReleased);
}

fn texts_on_screen(ui: &mut Simulator<'_, Message>) -> Vec<String> {
    ui_inspect::nodes(ui).into_iter().filter_map(|n| n.text).collect()
}

fn whole_disk_scan_of(dir: &Path) -> App {
    fixture(dir);
    app_showing(scanned(ScanOptions::volume(dir, "Test Disk")))
}

#[test]
fn a_selected_file_offers_preview_and_reveal_and_the_buttons_only_send_messages() -> Result<(), Error> {
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    let notes = descend(&on_screen(&app).tree, &["notes.txt"]);
    select(&mut app, notes);
    {
        let mut ui = sim(&app);
        shoot(&mut ui, "selection-actions")?;
    }
    for (label, expected) in [(dscan_core::PREVIEW_ACTION, "Preview"), (dscan_core::REVEAL_ACTION, "Reveal")] {
        let mut ui = sim(&app);
        ui.click(label)?;
        let msgs: Vec<Message> = ui.into_messages().collect();
        let sent = msgs.iter().any(|m| match m {
            Message::Preview => expected == "Preview",
            Message::Reveal => expected == "Reveal",
            _ => false,
        });
        assert!(sent, "{label} sends {expected}, which the test never runs: {msgs:?}");
    }
    let _ = app.update(Message::Deselect);
    let mut ui = sim(&app);
    assert!(!texts_on_screen(&mut ui).iter().any(|t| t == dscan_core::REVEAL_ACTION), "no selection, no actions");
    Ok(())
}

#[test]
fn selecting_hidden_space_explains_it_and_offers_the_fixes_this_os_has() -> Result<(), Error> {
    let dir = tempfile::tempdir().unwrap();
    let mut app = whole_disk_scan_of(dir.path());
    let hidden = descend(&on_screen(&app).tree, &[HIDDEN_SPACE_NAME]);
    let tree = &mut on_screen_mut(&mut app).tree;
    std::sync::Arc::get_mut(tree).expect("nothing else holds the fresh tree").stats.errors = 12;
    select(&mut app, hidden);
    let explanation = dscan_core::hidden::explain(&on_screen(&app).tree, hidden).expect("hidden space is explained");

    let mut ui = sim(&app);
    shoot(&mut ui, "hidden-space-explained")?;
    let texts = texts_on_screen(&mut ui);
    assert!(texts.iter().any(|t| t == explanation.summary), "the summary is shown: {texts:?}");
    for cause in explanation.causes {
        assert!(texts.iter().any(|t| t == cause.text), "cause listed: {}", cause.text);
    }
    assert!(texts.iter().any(|t| t == "12 folders couldn't be read in this scan."), "the scan's own evidence");
    assert!(!texts.iter().any(|t| t == dscan_core::REVEAL_ACTION), "hidden space is not a file to show");
    let offers_settings = texts.iter().any(|t| t == "Open Full Disk Access Settings");
    assert_eq!(offers_settings, dscan_core::FULL_DISK_ACCESS_SETTINGS.is_some());
    Ok(())
}

#[test]
fn disk_cards_name_the_file_system_and_mark_network_shares() {
    let (mut app, _) = App::new(None);
    let share = dscan_core::Volume {
        file_system: "SMB".into(),
        is_network: true,
        ..disk("media", Path::new("/Volumes/media"), 0.25)
    };
    app.volumes = Some(vec![share]);
    let mut ui = sim(&app);
    let texts = texts_on_screen(&mut ui);
    let expected =
        format!("{} · {} · SMB · network", dscan_core::format_size(2 * TB), Path::new("/Volumes/media").display());
    assert!(texts.contains(&expected), "{texts:?}");
}
