//! Headless UI inspection of the screens (iced_test Simulator).

use std::path::Path;

use dscan_core::{CancelToken, Progress, ScanOptions};
use iced::Settings;
use iced_test::{Error, Simulator};

use crate::app::{App, Browser, Message, Screen};
use crate::iced_inspect as ui_inspect;
use crate::theme;

const VIEW: (f32, f32) = (1280.0, 820.0);
const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/iced-inspect");

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
    // Hover + press a segment in the chart, click a list row.
    let msgs: Vec<Message> = {
        let mut ui = sim(&app);
        ui_inspect::hover(&mut ui, (440.0, 320.0));
        ui_inspect::click_at(&mut ui, (440.0, 320.0));
        ui.click("Pictures/")?;
        ui.into_messages().collect()
    };
    println!("messages: {msgs:?}");
    for m in msgs {
        let _ = app.update(m);
    }
    let _ = app.update(Message::MouseReleased);
    // Search
    let _ = app.update(Message::SearchChanged("img".into()));
    // The search itself runs on a thread; deliver its result directly.
    if let Screen::Browse(b) = &app.screen {
        let r = dscan_core::search(&b.tree, "img", &Default::default()).map(std::sync::Arc::new);
        let _ = app.update(Message::SearchDone(1, r));
    }
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
    assert_eq!(b.list_subject(), first, "list follows the selection's parent");
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

    // A click without movement selects but does not collect.
    let _ = app.update(Message::Press(Segment::Node(movies)));
    let _ = app.update(Message::CollectorEnter);
    let _ = app.update(Message::MouseReleased);
    assert!(browser(&app).collector.is_empty());
    let _ = app.update(Message::CollectorExit);

    // Press, move past the threshold, enter the collector, release.
    let _ = app.update(Message::Press(Segment::Node(movies)));
    let _ = app.update(Message::CursorMoved(Point::new(100.0, 100.0)));
    let _ = app.update(Message::CursorMoved(Point::new(140.0, 160.0)));
    assert!(browser(&app).drag.as_ref().unwrap().dragging);
    let _ = app.update(Message::CollectorEnter);
    let _ = app.update(Message::MouseReleased);
    assert_eq!(browser(&app).collector.items(), &[movies]);
    assert!(browser(&app).drag.is_none());

    // Collecting a child of a collected folder is a no-op; collecting the
    // parent replaces collected children.
    let b = browser(&app);
    let child = b.tree.children(movies, b.metric).next().unwrap();
    let _ = app.update(Message::Press(Segment::Node(child)));
    let _ = app.update(Message::CollectSelected);
    assert_eq!(browser(&app).collector.items(), &[movies]);

    // Dragging while the drop zone is not hovered collects nothing.
    let _ = app.update(Message::ClearCollector);
    let _ = app.update(Message::CollectorExit);
    let _ = app.update(Message::Press(Segment::Node(movies)));
    let _ = app.update(Message::CursorMoved(Point::new(0.0, 0.0)));
    let _ = app.update(Message::CursorMoved(Point::new(50.0, 0.0)));
    let _ = app.update(Message::MouseReleased);
    assert!(browser(&app).collector.is_empty());
}

#[test]
fn metric_toggle_relayouts() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = browser_app(dir.path());
    let _ = app.update(Message::SetMetric(dscan_core::Metric::Logical));
    let b = browser(&app);
    assert_eq!(b.layout.total, b.tree.size(b.tree.root(), dscan_core::Metric::Logical));
}
