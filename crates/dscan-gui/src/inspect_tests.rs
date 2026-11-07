use std::path::Path;

use dscan_core::{CancelToken, Progress, ScanOptions};
use iced::Settings;
use iced_test::{Error, Simulator};

use crate::app::{App, Browser, Message, Screen};
use crate::iced_inspect as ui_inspect;
use crate::{theme, view};

const VIEW: (f32, f32) = (1280.0, 820.0);
const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/iced-inspect");
const A_CHART_SEGMENT: (f32, f32) = (440.0, 320.0);
const CLOSE_BUTTON_COLUMN_X: f32 = 900.0;

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
    for (p, size) in files {
        let path = root.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![0x5au8; *size]).unwrap();
    }
}

fn scanned_fixture(dir: &Path) -> Browser {
    fixture(dir);
    let opts = ScanOptions::folder(dir);
    let tree = dscan_core::scan(&opts, &Progress::default(), &CancelToken::default()).unwrap();
    Browser::new(tree, opts)
}

fn app_showing(browser: Browser) -> App {
    let (mut app, _) = App::new(None);
    app.screen = Screen::Browse(Box::new(browser));
    app
}

fn browser_app(dir: &Path) -> App {
    app_showing(scanned_fixture(dir))
}

const LONGER_THAN_ANY_CHART_ANIMATION: std::time::Duration = std::time::Duration::from_secs(5);

fn finish_chart_animation(app: &mut App) {
    let first_frame = std::time::Instant::now();
    let _ = app.update(Message::AnimationFrame(first_frame));
    let _ = app.update(Message::AnimationFrame(first_frame + LONGER_THAN_ANY_CHART_ANIMATION));
    assert!(browser(app).animation.is_none(), "the animation ends once its time is up");
}

fn shoot_chart_animation(app: &mut App, name: &str, kind: dscan_core::TransitionKind) -> Result<(), Error> {
    let animation = browser(app).animation.as_ref().expect("the change animates the chart");
    assert_eq!(animation.kind(), kind, "{name}");
    for percent in [25, 50, 75] {
        if let Screen::Browse(b) = &mut app.screen {
            b.animation.as_mut().unwrap().jump_to(f64::from(percent) / 100.0);
        }
        let mut ui = sim(app);
        println!(
            "shot: {}",
            ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, &format!("{name}-{percent}"))?.display()
        );
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

    let movies = descend(&browser(&app).tree, &["Movies"]);
    let _ = app.update(Message::Navigate(movies));
    shoot_chart_animation(&mut app, "anim-zoom-in", TransitionKind::ZoomIn)?;
    let _ = app.update(Message::Back);
    shoot_chart_animation(&mut app, "anim-zoom-out", TransitionKind::ZoomOut)?;

    let size_before = browser(&app).layout.total;
    let pictures = descend(&browser(&app).tree, &["Pictures"]);
    let _ = app.update(Message::Press(dscan_core::Segment::Node(pictures)));
    let _ = app.update(Message::CollectSelected);
    let b = browser(&app);
    assert_eq!(b.collector.items(), &[pictures]);
    assert_eq!(b.layout.total, size_before - b.tree.size(pictures), "the chart leaves out what is collected");
    assert!(b.layout.find(dscan_core::Segment::Node(pictures)).is_none(), "the collected folder has no arc");
    assert!(b.list.rows.iter().all(|r| r.id != pictures), "nor a row in the list");
    shoot_chart_animation(&mut app, "anim-collect", TransitionKind::Morph)?;

    let _ = app.update(Message::Uncollect(pictures));
    assert_eq!(browser(&app).layout.total, size_before, "uncollecting puts it back");
    shoot_chart_animation(&mut app, "anim-uncollect", TransitionKind::Morph)?;
    Ok(())
}

fn deliver_search_result_without_its_thread(app: &mut App, query: &str) {
    if let Screen::Browse(b) = &app.screen {
        let result = dscan_core::search(&b.tree, query, &Default::default()).map(std::sync::Arc::new);
        let _ = app.update(Message::SearchDone(1, result));
    }
}

fn sim(app: &App) -> Simulator<'_, Message> {
    Simulator::with_size(Settings::default(), VIEW, view::view(app))
}

#[test]
fn inspect_start() -> Result<(), Error> {
    let (app, _) = App::new(None);
    let mut ui = sim(&app);
    println!("{}", ui_inspect::report(&mut ui, VIEW));
    println!("shot: {}", ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, "start")?.display());
    Ok(())
}

#[test]
fn inspect_browse() -> Result<(), Error> {
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    {
        let mut ui = sim(&app);
        println!("{}", ui_inspect::report(&mut ui, VIEW));
        println!("shot: {}", ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, "browse")?.display());
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
    assert_eq!(browser(&app).search.as_ref().unwrap().matches, 3);
    {
        let mut ui = sim(&app);
        println!("{}", ui_inspect::report(&mut ui, VIEW));
        println!("shot: {}", ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, "browse-after")?.display());
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

fn descend(tree: &dscan_core::Tree, names: &[&str]) -> dscan_core::NodeId {
    names.iter().fold(tree.root(), |at, name| tree.children(at).find(|&c| tree.name(c) == *name).expect(name))
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
    let deepest = descend(&browser(&app).tree, &chain);
    let _ = app.update(Message::Navigate(deepest));

    let mut ui = sim(&app);
    println!("shot: {}", ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, "toolbar-aligned")?.display());
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
    let images = descend(&browser(&app).tree, &chain);
    let _ = app.update(Message::Navigate(images));

    let msgs: Vec<Message> = {
        let mut ui = sim(&app);
        println!("{}", ui_inspect::report(&mut ui, VIEW));
        println!("shot: {}", ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, "path-bar-deep")?.display());
        for hidden in ["Projects", "clients"] {
            assert!(ui.find(hidden).is_err(), "{hidden} is folded into the ellipsis");
        }
        ui.click("…")?;
        ui.into_messages().collect()
    };
    let clients = descend(&browser(&app).tree, &chain[..2]);
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
    println!("shot: {}", ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, "path-bar-back-hover")?.display());
    Ok(())
}

fn browser(app: &App) -> &Browser {
    match &app.screen {
        Screen::Browse(b) => b,
        _ => panic!("not browsing"),
    }
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
    let root = browser(&app).tree.root();

    key(&mut app, Key::Named(Named::ArrowRight), false);
    let b = browser(&app);
    let first = b.selected.unwrap();
    assert_eq!(b.tree.name(first), "Movies");

    key(&mut app, Key::Named(Named::ArrowRight), false);
    assert_eq!(browser(&app).tree.name(browser(&app).selected.unwrap()), "Downloads");
    key(&mut app, Key::Named(Named::ArrowLeft), false);
    assert_eq!(browser(&app).selected, Some(first));

    key(&mut app, Key::Named(Named::ArrowDown), false);
    let b = browser(&app);
    assert_eq!(b.tree.name(b.selected.unwrap()), "2015");
    assert_eq!(b.listed_folder(), first, "list follows the selection's parent");
    key(&mut app, Key::Named(Named::ArrowUp), false);
    assert_eq!(browser(&app).selected, Some(first));

    key(&mut app, Key::Named(Named::Enter), false);
    assert_eq!(browser(&app).focus, first, "Enter drills into a folder");
    key(&mut app, Key::Named(Named::Backspace), false);
    assert_eq!(browser(&app).focus, root, "Backspace goes back");
    key(&mut app, Key::Character("]".into()), true);
    assert_eq!(browser(&app).focus, first, "Cmd-] goes forward");
    key(&mut app, Key::Named(Named::ArrowUp), true);
    assert_eq!(browser(&app).focus, root, "Cmd-Up goes to the parent");

    key(&mut app, Key::Character("c".into()), false);
    assert_eq!(browser(&app).collector.items(), &[first], "C collects the selection");
    key(&mut app, Key::Named(Named::Escape), false);
    assert_eq!(browser(&app).selected, None);
}

#[test]
fn drag_to_collector() {
    use dscan_core::Segment;
    use iced::Point;
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    let b = browser(&app);
    let movies = b.tree.children(b.tree.root()).next().unwrap();

    let _ = app.update(Message::Press(Segment::Node(movies)));
    let _ = app.update(Message::CollectorEnter);
    let _ = app.update(Message::MouseReleased);
    assert!(browser(&app).collector.is_empty(), "a click without movement selects but does not collect");
    let _ = app.update(Message::CollectorExit);

    let _ = app.update(Message::Press(Segment::Node(movies)));
    let _ = app.update(Message::CursorMoved(Point::new(100.0, 100.0)));
    let _ = app.update(Message::CursorMoved(Point::new(140.0, 160.0)));
    assert!(browser(&app).drag.as_ref().unwrap().dragging, "moving past the threshold starts a drag");
    let _ = app.update(Message::CollectorEnter);
    let _ = app.update(Message::MouseReleased);
    assert_eq!(browser(&app).collector.items(), &[movies], "releasing a drag over the collector collects");
    assert!(browser(&app).drag.is_none(), "releasing ends the drag");

    let b = browser(&app);
    let child = b.tree.children(movies).next().unwrap();
    let _ = app.update(Message::Press(Segment::Node(child)));
    let _ = app.update(Message::CollectSelected);
    assert_eq!(browser(&app).collector.items(), &[movies], "collecting a child of a collected folder is a no-op");

    let _ = app.update(Message::ClearCollector);
    let _ = app.update(Message::CollectorExit);
    let _ = app.update(Message::Press(Segment::Node(movies)));
    let _ = app.update(Message::CursorMoved(Point::new(0.0, 0.0)));
    let _ = app.update(Message::CursorMoved(Point::new(50.0, 0.0)));
    let _ = app.update(Message::MouseReleased);
    assert!(browser(&app).collector.is_empty(), "a drag released outside the collector collects nothing");
}

#[test]
fn inspect_scanning() -> Result<(), Error> {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = App::new(None);
    let _scan_task_never_run = app.update(Message::FolderPicked(Some(dir.path().to_path_buf())));
    let Screen::Scanning(job) = &mut app.screen else { panic!("not scanning") };
    job.snapshot =
        dscan_core::ProgressSnapshot { files: 673_862, dirs: 163_187, bytes: 86_624_714_752, expected: None };
    let mut ui = sim(&app);
    let report = ui_inspect::report(&mut ui, VIEW);
    println!("{report}");
    assert!(report.contains("\"Files\"") && report.contains("\"673,862\"") && report.contains("\"163,187\""));
    println!("shot: {}", ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, "scanning")?.display());

    ui.click("Pause")?;
    let msgs: Vec<Message> = ui.into_messages().collect();
    assert!(matches!(msgs[..], [Message::TogglePause]), "{msgs:?}");
    let _ = app.update(Message::TogglePause);
    let Screen::Scanning(job) = &app.screen else { panic!("not scanning") };
    assert!(job.cancel.is_paused());
    let mut ui = sim(&app);
    let report = ui_inspect::report(&mut ui, VIEW);
    assert!(report.contains("\"Resume\"") && report.contains("Paused: "), "{report}");
    println!("shot: {}", ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, "paused")?.display());
    drop(ui);
    let _ = app.update(Message::TogglePause);
    let Screen::Scanning(job) = &app.screen else { panic!("not scanning") };
    assert!(!job.cancel.is_paused());
    Ok(())
}

#[test]
fn inspect_open_results() -> Result<(), Error> {
    use dscan_core::Tree;
    let scan = |p: &Path| -> (Tree, ScanOptions) {
        let opts = ScanOptions::folder(p);
        (dscan_core::scan(&opts, &Progress::default(), &CancelToken::default()).unwrap(), opts)
    };
    let open = |app: &mut App, (tree, opts): (Tree, ScanOptions)| {
        app.screen = Screen::Browse(Box::new(Browser::new(tree, opts)));
        let _ = app.update(Message::GoHome);
    };
    let dirs: Vec<_> = (0..6).map(|_| tempfile::tempdir().unwrap()).collect();
    for d in &dirs {
        fixture(d.path());
    }
    let (mut app, _) = App::new(None);
    app.volumes = Some(dscan_core::list_volumes());
    for d in &dirs {
        open(&mut app, scan(d.path()));
    }
    assert_eq!(app.parked_newest_first.len(), 6, "every result stays open");
    {
        let mut ui = sim(&app);
        println!("{}", ui_inspect::report(&mut ui, VIEW));
        println!("shot: {}", ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, "home-results")?.display());
    }

    let newest = dscan_core::canonical_path(dirs[5].path()).unwrap();
    assert_eq!(app.parked_newest_first[0].tree.root_path(), newest, "most recently viewed result is listed first");
    let name = app.parked_newest_first[0].tree.name(app.parked_newest_first[0].tree.root()).into_owned();
    let msgs: Vec<Message> = {
        let mut ui = sim(&app);
        ui.click(name.as_str())?;
        ui.into_messages().collect()
    };
    assert!(matches!(msgs[..], [Message::ResumeParked(0)]), "clicking a row resumes that result: {msgs:?}");
    for m in msgs {
        let _ = app.update(m);
    }
    assert_eq!(browser(&app).tree.root_path(), newest, "exactly the clicked result is resumed");
    assert_eq!(app.parked_newest_first.len(), 5);
    let _ = app.update(Message::GoHome);

    let msgs: Vec<Message> = {
        let mut ui = sim(&app);
        let row_of_newest = bounds_of_text(&mut ui, &name).center_y();
        ui_inspect::click_at(&mut ui, (CLOSE_BUTTON_COLUMN_X, row_of_newest));
        ui.into_messages().collect()
    };
    assert!(
        matches!(msgs[..], [Message::CloseParked(0)]),
        "the × inside a row closes that result instead of opening it: {msgs:?}"
    );

    let _ = app.update(Message::CloseParked(1));
    assert_eq!(app.parked_newest_first.len(), 5, "closing removes just that one result");
    assert!(
        app.parked_newest_first
            .iter()
            .all(|b| b.tree.root_path() != dscan_core::canonical_path(dirs[4].path()).unwrap()),
        "the closed result is gone"
    );

    let _ = app.update(Message::FolderPicked(Some(dirs[0].path().to_path_buf())));
    let Screen::Scanning(job) = &app.screen else { panic!("not scanning") };
    let job_id = job.id;
    *job.result_slot.lock() = Some(Ok(scan(dirs[0].path()).0));
    let _ = app.update(Message::ScanFinished { job_id, crash: None });
    assert!(matches!(app.screen, Screen::Browse(_)));
    assert_eq!(app.parked_newest_first.len(), 4, "a fresh scan of an open folder drops its stale result");
    Ok(())
}

#[test]
fn collector_fixed_height_fits_every_state_without_clipping() -> Result<(), Error> {
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    let root = browser(&app).tree.root();
    let movies = browser(&app).tree.children(root).next().unwrap();
    let _ = app.update(Message::Press(dscan_core::Segment::Node(movies)));
    let _ = app.update(Message::MouseReleased);

    let shot = |app: &App, name: &str| -> Result<(), Error> {
        let mut ui = sim(app);
        let report = ui_inspect::report(&mut ui, VIEW);
        assert!(!report.contains("clipped:"), "{name}: collector content is clipped\n{report}");
        println!("shot: {}", ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, name)?.display());
        Ok(())
    };
    shot(&app, "collector-empty")?;
    let _ = app.update(Message::CollectSelected);
    shot(&app, "collector-one")?;
    let kids: Vec<_> = browser(&app).tree.children(root).skip(1).collect();
    for id in kids {
        if let Screen::Browse(b) = &mut app.screen {
            b.collector.add(&b.tree, id).unwrap();
        }
    }
    assert!(browser(&app).collector.len() > 4);
    shot(&app, "collector-many")?;
    {
        let mut ui = sim(&app);
        let report = ui_inspect::report(&mut ui, VIEW);
        assert!(report.contains("\"Hold 3 s to delete permanently\""), "the hold button says how long to hold");
    }
    if let Screen::Browse(b) = &mut app.screen {
        b.delete_hold_started = Some(std::time::Instant::now() - std::time::Duration::from_millis(1500));
    }
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
    if let Screen::Browse(b) = &mut app.screen {
        b.delete_hold_started = None;
    }
    let _ = app.update(Message::AskTrash);
    shot(&app, "collector-confirm")?;
    let _ = app.update(Message::CancelTrash);
    Ok(())
}

fn collect_through_the_ui(app: &mut App, names: &[&str]) {
    for name in names {
        let id = descend(&browser(app).tree, &[name]);
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
    if let Screen::Scanning(job) = &mut scanning.screen {
        job.snapshot = dscan_core::ProgressSnapshot {
            files: 673_862,
            dirs: 163_187,
            bytes: 86_624_714_752,
            expected: Some(200_000_000_000),
        };
    }

    let mut collected_and_searching = browser_app(dir.path());
    let inner = descend(&browser(&collected_and_searching).tree, &[long_name, "inner"]);
    let _ = collected_and_searching.update(Message::Navigate(inner));
    collect_through_the_ui(&mut collected_and_searching, &["Movies", "Downloads", "Pictures"]);
    let _ = collected_and_searching.update(Message::SearchChanged("img".into()));
    deliver_search_result_without_its_thread(&mut collected_and_searching, "img");

    let mut holding_delete = browser_app(other_results[0].path());
    collect_through_the_ui(&mut holding_delete, &["Movies", "Downloads"]);
    let _ = holding_delete.update(Message::DeleteHoldStart);

    let mut long_folder_with_unreadable_parts = browser_app(dir.path());
    let long_folder = descend(&browser(&long_folder_with_unreadable_parts).tree, &[long_name]);
    let _ = long_folder_with_unreadable_parts.update(Message::Navigate(long_folder));
    if let Screen::Browse(b) = &mut long_folder_with_unreadable_parts.screen {
        std::sync::Arc::get_mut(&mut b.tree).expect("nothing else holds the fresh tree").stats.errors = 1_234;
    }

    let mut confirming_trash = browser_app(other_results[1].path());
    collect_through_the_ui(&mut confirming_trash, &["Movies"]);
    let _ = confirming_trash.update(Message::AskTrash);

    let screens = [
        ("start", &start),
        ("scanning", &scanning),
        ("collected", &collected_and_searching),
        ("holding", &holding_delete),
        ("long", &long_folder_with_unreadable_parts),
        ("confirm", &confirming_trash),
    ];
    let mut problems = Vec::new();
    for (name, app) in screens {
        for size in sizes {
            let mut ui = Simulator::with_size(Settings::default(), size, view::view(app));
            let shot_name = format!("squash-{name}-{}x{}", size.0, size.1);
            println!("shot: {}", ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, &shot_name)?.display());
            problems.extend(layout_problems(&mut ui, size).into_iter().map(|p| format!("{shot_name}: {p}")));
        }
    }
    assert!(problems.is_empty(), "text is cut off, hidden or overlapping:\n{}", problems.join("\n"));
    Ok(())
}

fn scanned_folder(path: &Path) -> Browser {
    let opts = ScanOptions::folder(path);
    Browser::new(dscan_core::scan(&opts, &Progress::default(), &CancelToken::default()).unwrap(), opts)
}
