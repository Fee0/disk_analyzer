//! Application state, messages, update logic and subscriptions.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dscan_core::{
    CancelToken, Collection, Layout, LayoutParams, Metric, NodeId, Progress, ProgressSnapshot, ScanError,
    ScanOptions, SearchResult, Segment, Tree, Volume, actions, format_size,
};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::canvas::Cache;
use iced::{Point, Subscription, Task, event, mouse};

use crate::fmt::{thousands, truncate};
use crate::quicklook::QuickLook;

pub const SEARCH_ID: &str = "search";
/// Rows rendered in the file list before collapsing the rest.
pub const MAX_ROWS: usize = 400;
const DRAG_THRESHOLD: f32 = 4.0;

/// Run blocking work on its own thread and await the result.
fn off_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> impl Future<Output = Option<T>> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    async move { rx.await.ok() }
}

pub struct App {
    pub screen: Screen,
    /// `None` while the volume list is loading.
    pub volumes: Option<Vec<Volume>>,
    pub full_disk_access: bool,
    pub error: Option<String>,
    next_job: u64,
}

pub enum Screen {
    Start,
    Scanning(ScanJob),
    Browse(Box<Browser>),
}

/// Shared slot the scan thread fills; taken once on `ScanFinished`.
type ScanSlot = Arc<parking_lot::Mutex<Option<Result<Tree, ScanError>>>>;

pub struct ScanJob {
    pub id: u64,
    pub opts: ScanOptions,
    pub progress: Arc<Progress>,
    pub cancel: CancelToken,
    pub started: Instant,
    pub snapshot: ProgressSnapshot,
    slot: ScanSlot,
}

/// Pointer pressed on an item: may turn into a drag.
pub struct PendingDrag {
    pub item: NodeId,
    pub origin: Option<Point>,
    pub dragging: bool,
    pub pos: Point,
}

impl PendingDrag {
    fn new(item: NodeId) -> Self {
        PendingDrag { item, origin: None, dragging: false, pos: Point::ORIGIN }
    }
}

/// One pre-formatted row of the file list.
pub struct Row {
    pub id: NodeId,
    pub name: String,
    pub size: String,
    /// Share of the listed folder, 0..=1.
    pub share: f32,
}

/// The file list, rebuilt only when its folder or the layout changes.
#[derive(Default)]
pub struct ListView {
    key: Option<(NodeId, u64)>,
    pub rows: Vec<Row>,
    /// "… N more items · X" when rows were cut off.
    pub more: Option<String>,
}

pub struct Browser {
    /// Shared with background searches; mutated only when not shared.
    pub tree: Arc<Tree>,
    pub opts: ScanOptions,
    pub metric: Metric,
    pub focus: NodeId,
    history: Vec<NodeId>,
    forward: Vec<NodeId>,
    pub layout: Layout,
    /// Bumped whenever `layout` is recomputed.
    layout_rev: u64,
    /// Static chart geometry.
    pub cache: Cache,
    /// Selection outline and hover highlight.
    pub highlight: Cache,
    pub hovered: Option<Segment>,
    pub selected: Option<NodeId>,
    pub search_text: String,
    search_gen: u64,
    search_cancel: CancelToken,
    pub search: Option<Arc<SearchResult>>,
    pub list: ListView,
    pub collector: Collection,
    pub over_collector: bool,
    pub confirm_trash: bool,
    /// Trash confirmed while a search still held the tree.
    trash_pending: bool,
    pub drag: Option<PendingDrag>,
    pub status: Option<String>,
    quicklook: QuickLook,
}

#[derive(Debug, Clone)]
pub enum Message {
    // Start screen
    VolumesLoaded(Vec<Volume>, bool),
    ScanVolume(usize),
    OpenFolder,
    FolderPicked(Option<PathBuf>),
    OpenFdaSettings,
    RefreshVolumes,
    // Scanning
    Tick,
    CancelScan,
    ScanFinished(u64),
    // Browsing: chart and list share these
    Hover(Option<Segment>),
    Press(Segment),
    Activate(Segment),
    /// Pointer left a list row (clears its hover if still current).
    RowExit(NodeId),
    GoUp,
    Deselect,
    // Navigation
    Navigate(NodeId),
    Back,
    Forward,
    SetMetric(Metric),
    // Search
    SearchChanged(String),
    SearchFire(u64),
    SearchDone(u64, Option<Arc<SearchResult>>),
    ClearSearch,
    FocusSearch,
    // Global input
    Key(Key, keyboard::Modifiers),
    CursorMoved(Point),
    MouseReleased,
    // Collector
    CollectorEnter,
    CollectorExit,
    CollectSelected,
    Uncollect(NodeId),
    ClearCollector,
    AskTrash,
    CancelTrash,
    ConfirmTrash,
    // Misc
    QuickLook,
    Rescan,
    NewScan,
    DismissStatus,
}

impl App {
    pub fn new(initial: Option<PathBuf>) -> (Self, Task<Message>) {
        let mut app = App { screen: Screen::Start, volumes: None, full_disk_access: true, error: None, next_job: 1 };
        let load = Self::load_volumes();
        let task = match initial {
            Some(path) => Task::batch([load, app.start_scan(ScanOptions::folder(path))]),
            None => load,
        };
        (app, task)
    }

    /// Volume info involves `statfs` and Foundation lookups that can stall
    /// (e.g. spun-down disks), so fetch it off the UI thread.
    fn load_volumes() -> Task<Message> {
        Task::perform(off_thread(|| (dscan_core::list_volumes(), dscan_core::has_full_disk_access())), |r| {
            let (volumes, fda) = r.unwrap_or_default();
            Message::VolumesLoaded(volumes, fda)
        })
    }

    pub fn title(&self) -> String {
        match &self.screen {
            Screen::Start => "Disk Analyzer".into(),
            Screen::Scanning(j) => format!("Scanning {} — Disk Analyzer", j.opts.display_name()),
            Screen::Browse(b) => format!("{} — Disk Analyzer", b.tree.name(b.focus)),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match &self.screen {
            Screen::Start => Subscription::none(),
            Screen::Scanning(_) => iced::time::every(Duration::from_millis(100)).map(|_| Message::Tick),
            Screen::Browse(b) => {
                let base = event::listen_with(global_event);
                if b.drag.is_some() { Subscription::batch([base, event::listen_with(drag_event)]) } else { base }
            }
        }
    }

    /// Leave the browser, freeing its tree off the UI thread (it can be
    /// hundreds of MB).
    fn leave_browser(&mut self) {
        if let Screen::Browse(old) = std::mem::replace(&mut self.screen, Screen::Start) {
            std::thread::spawn(move || drop(old));
        }
    }

    fn start_scan(&mut self, opts: ScanOptions) -> Task<Message> {
        self.leave_browser();
        let id = self.next_job;
        self.next_job += 1;
        let progress = Arc::new(Progress::default());
        let cancel = CancelToken::default();
        let slot: ScanSlot = Default::default();
        let work = {
            let (opts, progress, cancel, slot) = (opts.clone(), progress.clone(), cancel.clone(), slot.clone());
            off_thread(move || *slot.lock() = Some(dscan_core::scan(&opts, &progress, &cancel)))
        };
        self.error = None;
        self.screen = Screen::Scanning(ScanJob {
            id,
            opts,
            progress,
            cancel,
            started: Instant::now(),
            snapshot: ProgressSnapshot::default(),
            slot,
        });
        Task::perform(work, move |_| Message::ScanFinished(id))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::VolumesLoaded(volumes, fda) => {
                self.volumes = Some(volumes);
                self.full_disk_access = fda;
            }
            Message::ScanVolume(i) => {
                if let Some(v) = self.volumes.as_ref().and_then(|v| v.get(i)) {
                    return self.start_scan(ScanOptions::volume(&v.mount, &v.name));
                }
            }
            Message::OpenFolder => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Choose a folder to analyze")
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    Message::FolderPicked,
                );
            }
            Message::FolderPicked(Some(path)) => return self.start_scan(ScanOptions::folder(path)),
            Message::OpenFdaSettings => {
                let _ = std::process::Command::new("open")
                    .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
                    .spawn();
            }
            Message::RefreshVolumes => return Self::load_volumes(),
            Message::Tick => {
                if let Screen::Scanning(job) = &mut self.screen {
                    job.snapshot = job.progress.snapshot();
                }
            }
            Message::CancelScan => {
                if let Screen::Scanning(job) = &self.screen {
                    job.cancel.cancel();
                }
            }
            Message::ScanFinished(id) => {
                let Screen::Scanning(job) = &mut self.screen else { return Task::none() };
                if job.id != id {
                    return Task::none();
                }
                let result = job.slot.lock().take();
                let opts = job.opts.clone();
                self.screen = Screen::Start;
                match result {
                    Some(Ok(tree)) => self.screen = Screen::Browse(Box::new(Browser::new(tree, opts))),
                    Some(Err(ScanError::Cancelled)) | None => {}
                    Some(Err(e)) => self.error = Some(e.to_string()),
                }
            }
            Message::NewScan => {
                self.leave_browser();
                return Self::load_volumes();
            }
            Message::Rescan => {
                if let Screen::Browse(b) = &self.screen {
                    let opts = b.opts.clone();
                    return self.start_scan(opts);
                }
            }
            other => {
                if let Screen::Browse(b) = &mut self.screen {
                    return b.update(other);
                }
            }
        }
        Task::none()
    }
}

fn global_event(event: iced::Event, status: event::Status, _window: iced::window::Id) -> Option<Message> {
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            // Let focused widgets (the search field) keep their keys, but
            // allow Cmd shortcuts through.
            (status == event::Status::Ignored || modifiers.command()).then_some(Message::Key(key, modifiers))
        }
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Some(Message::MouseReleased),
        _ => None,
    }
}

fn drag_event(event: iced::Event, _status: event::Status, _window: iced::window::Id) -> Option<Message> {
    match event {
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => Some(Message::CursorMoved(position)),
        _ => None,
    }
}

/// Shortcuts that map directly onto an existing message.
fn shortcut(key: Key<&str>, cmd: bool) -> Option<Message> {
    Some(match key {
        Key::Named(Named::ArrowUp) if cmd => Message::GoUp,
        Key::Named(Named::Backspace) if !cmd => Message::Back,
        Key::Character("[") if cmd => Message::Back,
        Key::Character("]") if cmd => Message::Forward,
        Key::Character("f") if cmd => Message::FocusSearch,
        Key::Character("c") if !cmd => Message::CollectSelected,
        Key::Named(Named::Space) => Message::QuickLook,
        _ => return None,
    })
}

impl Browser {
    pub fn new(tree: Tree, opts: ScanOptions) -> Self {
        let root = tree.root();
        let mut b = Browser {
            tree: Arc::new(tree),
            opts,
            metric: Metric::Allocated,
            focus: root,
            history: Vec::new(),
            forward: Vec::new(),
            layout: Layout::default(),
            layout_rev: 0,
            cache: Cache::new(),
            highlight: Cache::new(),
            hovered: None,
            selected: None,
            search_text: String::new(),
            search_gen: 0,
            search_cancel: CancelToken::default(),
            search: None,
            list: ListView::default(),
            collector: Collection::default(),
            over_collector: false,
            confirm_trash: false,
            trash_pending: false,
            drag: None,
            status: None,
            quicklook: QuickLook::default(),
        };
        b.relayout();
        b.sync_list();
        b
    }

    pub fn can_back(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    fn relayout(&mut self) {
        self.layout =
            dscan_core::sunburst(&self.tree, self.focus, self.metric, LayoutParams::default(), self.search.as_deref());
        self.layout_rev += 1;
        self.cache.clear();
        self.highlight.clear();
    }

    /// Directory whose children are listed on the right.
    pub fn list_subject(&self) -> NodeId {
        match self.selected {
            Some(s) if self.tree.is_strict_ancestor_of(self.focus, s) => self.tree.parent(s).unwrap_or(self.focus),
            _ => self.focus,
        }
    }

    /// Children of `id` in display order, restricted to search matches.
    fn visible_children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.tree.children(id, self.metric).filter(|&c| self.search.as_ref().is_none_or(|s| s.is_relevant(c)))
    }

    /// Rebuild the list rows if their folder or the layout changed.
    fn sync_list(&mut self) {
        let subject = self.list_subject();
        let key = Some((subject, self.layout_rev));
        if self.list.key == key {
            return;
        }
        let total = self.tree.size(subject, self.metric).max(1);
        let children: Vec<NodeId> = self.visible_children(subject).collect();
        let rows = children
            .iter()
            .take(MAX_ROWS)
            .map(|&id| {
                let node = self.tree.node(id);
                let size = node.size(self.metric);
                let mut name = truncate(&self.tree.name(id), 44);
                if node.is_dir() {
                    name.push('/');
                }
                if node.is_unreadable() {
                    name.push_str("  (no access)");
                }
                Row { id, name, size: format_size(size), share: size as f32 / total as f32 }
            })
            .collect();
        let more = (children.len() > MAX_ROWS).then(|| {
            let rest: u64 = children[MAX_ROWS..].iter().map(|&c| self.tree.size(c, self.metric)).sum();
            format!("… {} more items · {}", thousands((children.len() - MAX_ROWS) as u64), format_size(rest))
        });
        self.list = ListView { key, rows, more };
    }

    fn set_focus(&mut self, id: NodeId, record: bool) {
        if !self.tree.node(id).is_dir() || id == self.focus || !self.tree.is_live(id) {
            return;
        }
        if record {
            self.history.push(self.focus);
            self.forward.clear();
        }
        self.focus = id;
        if self.selected.is_some_and(|sel| !self.tree.is_strict_ancestor_of(id, sel)) {
            self.selected = None;
        }
        self.hovered = None;
        self.relayout();
    }

    /// Select `id`, refocusing on its parent if it is not visible.
    fn select(&mut self, id: NodeId) {
        self.selected = Some(id);
        let visible = self.tree.is_strict_ancestor_of(self.focus, id) && self.layout.find(Segment::Node(id)).is_some();
        if !visible && let Some(p) = self.tree.parent(id) {
            self.set_focus(p, true);
        }
    }

    fn activate(&mut self, id: NodeId) {
        if self.tree.node(id).is_dir() {
            self.set_focus(id, true);
        } else if self.tree.is_actionable(id) {
            self.selected = Some(id);
            self.quicklook.toggle(Some(&self.tree.path(id)));
        }
    }

    fn go_up(&mut self) {
        if let Some(p) = self.tree.parent(self.focus) {
            let old = self.focus;
            self.set_focus(p, true);
            self.selected = Some(old);
        }
    }

    fn back(&mut self) {
        while let Some(prev) = self.history.pop() {
            if self.tree.is_live(prev) {
                self.forward.push(self.focus);
                let old = self.focus;
                self.set_focus(prev, false);
                if self.tree.is_ancestor_of(prev, old) {
                    self.selected = Some(old);
                }
                return;
            }
        }
    }

    fn forward(&mut self) {
        while let Some(next) = self.forward.pop() {
            if self.tree.is_live(next) {
                self.history.push(self.focus);
                self.set_focus(next, false);
                return;
            }
        }
    }

    fn move_selection(&mut self, key: Named) {
        let Some(sel) = self.selected.filter(|&s| self.tree.is_strict_ancestor_of(self.focus, s)) else {
            let first = self.visible_children(self.focus).next();
            self.selected = first;
            return;
        };
        match key {
            Named::ArrowLeft | Named::ArrowRight => {
                let parent = self.tree.parent(sel).unwrap_or(self.focus);
                let sibs: Vec<NodeId> = self.visible_children(parent).collect();
                if let Some(i) = sibs.iter().position(|&s| s == sel) {
                    let j = if key == Named::ArrowRight { (i + 1).min(sibs.len() - 1) } else { i.saturating_sub(1) };
                    self.selected = Some(sibs[j]);
                }
            }
            Named::ArrowUp => {
                if let Some(p) = self.tree.parent(sel).filter(|&p| p != self.focus) {
                    self.selected = Some(p);
                }
            }
            Named::ArrowDown => {
                let first = self.visible_children(sel).next();
                if let Some(c) = first {
                    self.select(c);
                }
            }
            _ => {}
        }
    }

    /// Start a background search, cancelling any running one; the result
    /// arrives as `SearchDone`.
    fn run_search(&mut self) -> Task<Message> {
        self.search_cancel.cancel();
        self.search_cancel = CancelToken::default();
        if self.search_text.trim().is_empty() {
            self.search = None;
            self.relayout();
            return Task::none();
        }
        let (tree, query, cancel) = (Arc::clone(&self.tree), self.search_text.clone(), self.search_cancel.clone());
        let generation = self.search_gen;
        Task::perform(off_thread(move || dscan_core::search(&tree, &query, &cancel).map(Arc::new)), move |r| {
            Message::SearchDone(generation, r.flatten())
        })
    }

    fn trash_collected(&mut self) -> Task<Message> {
        // A running search holds the tree: cancel it and finish once it
        // reports back (`SearchDone`).
        self.search_cancel.cancel();
        let Some(tree) = Arc::get_mut(&mut self.tree) else {
            self.trash_pending = true;
            return Task::none();
        };
        self.trash_pending = false;
        self.quicklook.close();
        let report = actions::move_to_trash(tree, &mut self.collector);
        let mut msg = format!(
            "Moved {} item{} to the Trash, freeing {}.",
            report.trashed,
            if report.trashed == 1 { "" } else { "s" },
            format_size(report.freed)
        );
        if let Some((_, err)) = report.failed.first() {
            msg.push_str(&format!(" {} failed: {err}", report.failed.len()));
        }
        self.status = Some(msg);
        self.focus = self.tree.nearest_live(self.focus);
        self.selected = self.selected.filter(|&s| self.tree.is_live(s));
        self.hovered = None;
        self.search_gen += 1;
        self.relayout();
        self.run_search()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let before = (self.hovered, self.selected);
        let task = self.handle(message);
        if (self.hovered, self.selected) != before {
            self.highlight.clear();
        }
        self.sync_list();
        task
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Hover(h) => self.hovered = h,
            Message::RowExit(id)
                if self.hovered == Some(Segment::Node(id)) => {
                    self.hovered = None;
                }
            Message::Press(Segment::Node(id)) => {
                self.selected = Some(id);
                self.drag = Some(PendingDrag::new(id));
            }
            Message::Press(Segment::Small { .. }) | Message::Deselect => self.selected = None,
            Message::Activate(Segment::Node(id)) => self.activate(id),
            Message::Activate(Segment::Small { parent, .. }) => self.set_focus(parent, true),
            Message::GoUp => self.go_up(),
            Message::Navigate(id) => self.set_focus(id, true),
            Message::Back => self.back(),
            Message::Forward => self.forward(),
            Message::SetMetric(m)
                if m != self.metric => {
                    self.metric = m;
                    self.relayout();
                }
            Message::SearchChanged(s) => {
                self.search_text = s;
                self.search_gen += 1;
                let generation = self.search_gen;
                return Task::perform(async { tokio::time::sleep(Duration::from_millis(150)).await }, move |_| {
                    Message::SearchFire(generation)
                });
            }
            Message::SearchFire(generation) if generation == self.search_gen => return self.run_search(),
            Message::SearchDone(generation, result) => {
                if generation == self.search_gen {
                    self.search = result;
                    self.relayout();
                }
                if self.trash_pending {
                    return self.trash_collected();
                }
            }
            Message::ClearSearch => {
                self.search_text.clear();
                self.search_gen += 1;
                return self.run_search();
            }
            Message::FocusSearch => return iced::widget::operation::focus(iced::widget::Id::new(SEARCH_ID)),
            Message::Key(key, mods) => {
                let cmd = mods.command();
                match key.as_ref() {
                    Key::Named(n @ (Named::ArrowLeft | Named::ArrowRight | Named::ArrowUp | Named::ArrowDown))
                        if !cmd =>
                    {
                        self.move_selection(n)
                    }
                    Key::Named(Named::Enter | Named::ArrowDown) => {
                        if let Some(s) = self.selected {
                            self.activate(s);
                        }
                    }
                    Key::Named(Named::Escape) => {
                        if self.confirm_trash {
                            self.confirm_trash = false;
                        } else if !self.search_text.is_empty() {
                            return self.handle(Message::ClearSearch);
                        } else {
                            self.selected = None;
                        }
                    }
                    k => {
                        if let Some(msg) = shortcut(k, cmd) {
                            return self.handle(msg);
                        }
                    }
                }
            }
            Message::CursorMoved(p) => {
                if let Some(d) = &mut self.drag {
                    d.pos = p;
                    match d.origin {
                        None => d.origin = Some(p),
                        Some(o) => d.dragging |= p.distance(o) > DRAG_THRESHOLD,
                    }
                }
            }
            Message::MouseReleased => {
                if let Some(d) = self.drag.take()
                    && d.dragging
                    && self.over_collector
                {
                    self.collector.add(&self.tree, d.item);
                }
            }
            Message::CollectorEnter => self.over_collector = true,
            Message::CollectorExit => self.over_collector = false,
            Message::CollectSelected => {
                if let Some(s) = self.selected {
                    self.collector.add(&self.tree, s);
                }
            }
            Message::Uncollect(id) => self.collector.remove(id),
            Message::ClearCollector => {
                self.collector.clear();
                self.confirm_trash = false;
            }
            Message::AskTrash => self.confirm_trash = !self.collector.is_empty(),
            Message::CancelTrash => self.confirm_trash = false,
            Message::ConfirmTrash => {
                self.confirm_trash = false;
                return self.trash_collected();
            }
            Message::QuickLook => {
                let path = self.selected.filter(|&s| self.tree.is_actionable(s)).map(|s| self.tree.path(s));
                self.quicklook.toggle(path.as_deref());
            }
            Message::DismissStatus => self.status = None,
            _ => {}
        }
        Task::none()
    }
}
