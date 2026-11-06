use std::path::Path;

use dscan_core::{CancelToken, Progress, ScanOptions};
use iced::Settings;
use iced_test::{Error, Simulator};

use crate::app::{App, Browser, Message, Screen};
use crate::iced_inspect as ui_inspect;
use crate::theme;

const VIEW: (f32, f32) = (1280.0, 820.0);
const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/iced-inspect");
const A_CHART_SEGMENT: (f32, f32) = (440.0, 320.0);
const CLOSE_BUTTON_OF_NEWEST_RESULT: (f32, f32) = (900.0, 518.0);

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

fn browser_app(dir: &Path) -> App {
    fixture(dir);
    let opts = ScanOptions::folder(dir);
    let tree = dscan_core::scan(&opts, &Progress::default(), &CancelToken::default()).unwrap();
    let (mut app, _) = App::new(None);
    app.screen = Screen::Browse(Box::new(Browser::new(tree, opts)));
    app
}

fn deliver_search_result_without_its_thread(app: &mut App, query: &str) {
    if let Screen::Browse(b) = &app.screen {
        let result = dscan_core::search(&b.tree, query, &Default::default()).map(std::sync::Arc::new);
        let _ = app.update(Message::SearchDone(1, result));
    }
}

fn sim(app: &App) -> Simulator<'_, Message> {
    Simulator::with_size(Settings::default(), VIEW, crate::view::view(app))
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
    let movies = b.tree.children(b.tree.root(), b.metric).next().unwrap();

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
    let child = b.tree.children(movies, b.metric).next().unwrap();
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
fn metric_toggle_relayouts() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    let msgs: Vec<Message> = {
        let mut ui = sim(&app);
        ui.click("Logical").unwrap();
        ui.into_messages().collect()
    };
    assert!(
        matches!(msgs[..], [Message::SetMetric(dscan_core::Metric::Logical)]),
        "the toggle emits the metric it shows: {msgs:?}"
    );
    for m in msgs {
        let _ = app.update(m);
    }
    let b = browser(&app);
    assert_eq!(b.layout.total, b.tree.size(b.tree.root(), dscan_core::Metric::Logical));
    let mut ui = sim(&app);
    println!("shot: {}", ui_inspect::shot(&mut ui, &theme::app_theme(), OUT, "logical").unwrap().display());
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

    let newest = dirs[5].path().canonicalize().unwrap();
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
        ui_inspect::click_at(&mut ui, CLOSE_BUTTON_OF_NEWEST_RESULT);
        ui.into_messages().collect()
    };
    assert!(
        matches!(msgs[..], [Message::CloseParked(0)]),
        "the × inside a row closes that result instead of opening it: {msgs:?}"
    );

    let _ = app.update(Message::CloseParked(1));
    assert_eq!(app.parked_newest_first.len(), 5, "closing removes just that one result");
    assert!(
        app.parked_newest_first.iter().all(|b| b.tree.root_path() != dirs[4].path().canonicalize().unwrap()),
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
    let movies = browser(&app).tree.children(root, browser(&app).metric).next().unwrap();
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
    let kids: Vec<_> = browser(&app).tree.children(root, browser(&app).metric).skip(1).collect();
    for id in kids {
        if let Screen::Browse(b) = &mut app.screen {
            b.collector.add(&b.tree, id).unwrap();
        }
    }
    assert!(browser(&app).collector.len() > 4);
    shot(&app, "collector-many")?;
    let _ = app.update(Message::AskTrash);
    shot(&app, "collector-confirm")?;
    let _ = app.update(Message::CancelTrash);
    Ok(())
}
