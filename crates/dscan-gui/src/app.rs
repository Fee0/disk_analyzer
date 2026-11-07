use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dscan_core::{
    CancelToken, Collection, DesktopError, Exclusions, Layout, LayoutParams, NodeId, PREVIEW_ACTION, Progress,
    ProgressSnapshot, REVEAL_ACTION, Refusal, Removal, ScanError, ScanOptions, SearchResult, Segment, TRASH_NAME,
    Transition, Tree, Volume, actions, desktop, format_size,
};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::canvas::Cache;
use iced::{Point, Subscription, Task, event, mouse};

use crate::animation::{Advance, ChartAnimation};
use crate::fmt::{thousands, truncate};

pub const SEARCH_ID: &str = "search";
pub const MAX_LIST_ROWS: usize = 400;
const DRAG_THRESHOLD: f32 = 4.0;
pub const HOLD_TO_DELETE: Duration = Duration::from_secs(3);
const HOLD_PROGRESS_REFRESH: Duration = Duration::from_millis(33);
const SCAN_PROGRESS_REFRESH: Duration = Duration::from_millis(100);

struct PendingRemoval {
    removal: Removal,
    items: Vec<NodeId>,
}

pub type PanicMessage = String;

fn panic_message(payload: Box<dyn Any + Send>) -> PanicMessage {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

async fn off_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, PanicMessage> {
    tokio::task::spawn_blocking(f).await.map_err(|e| match e.try_into_panic() {
        Ok(payload) => panic_message(payload),
        Err(e) => e.to_string(),
    })
}

fn drop_off_thread(results: Vec<Browser>) {
    if !results.is_empty() {
        std::thread::spawn(move || drop(results));
    }
}

pub struct App {
    pub screen: Screen,
    pub volumes: Option<Vec<Volume>>,
    pub full_disk_access: bool,
    pub error: Option<String>,
    pub parked_newest_first: Vec<Browser>,
    next_job: u64,
}

pub enum Screen {
    Start { scan: Option<ScanJob> },
    Browse(Box<Browser>),
}

type ScanResultSlot = Arc<parking_lot::Mutex<Option<Result<Tree, ScanError>>>>;

pub struct ScanJob {
    pub id: u64,
    pub opts: ScanOptions,
    pub progress: Arc<Progress>,
    pub cancel: CancelToken,
    pub snapshot: ProgressSnapshot,
    pub(crate) result_slot: ScanResultSlot,
}

impl Drop for ScanJob {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl ScanJob {
    pub fn scans_volume(&self, volume: &Volume) -> bool {
        self.opts.whole_disk && self.opts.root == volume.mount
    }
}

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

pub struct ListRow {
    pub id: NodeId,
    pub name: String,
    pub size: String,
    pub fraction_of_listed_folder: f32,
}

#[derive(Clone, Copy, PartialEq)]
struct ListSource {
    folder: NodeId,
    layout_revision: u64,
}

#[derive(Default)]
pub struct ListView {
    built_for: Option<ListSource>,
    pub rows: Vec<ListRow>,
    pub cut_off_rows_summary: Option<String>,
}

pub struct Browser {
    pub tree: Arc<Tree>,
    pub opts: ScanOptions,
    pub focus: NodeId,
    history: Vec<NodeId>,
    forward: Vec<NodeId>,
    pub layout: Layout,
    layout_revision: u64,
    pub chart_cache: Cache,
    pub highlight_cache: Cache,
    pub hovered: Option<Segment>,
    pub selected: Option<NodeId>,
    pub search_text: String,
    search_gen: u64,
    search_cancel: CancelToken,
    pub search: Option<Arc<SearchResult>>,
    pub list: ListView,
    pub collector: Collection,
    pub exclusions: Exclusions,
    pub animation: Option<ChartAnimation>,
    pub over_collector: bool,
    pub confirm_trash: bool,
    removal_waiting_for_tree: Option<PendingRemoval>,
    pub(crate) delete_hold_started: Option<Instant>,
    pub drag: Option<PendingDrag>,
    pub status: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Message {
    VolumesLoaded(Vec<Volume>, bool),
    ScanVolume(usize),
    OpenFolder,
    FolderPicked(Option<PathBuf>),
    OpenFdaSettings,
    RefreshVolumes,
    ScanProgressTick,
    AnimationFrame(Instant),
    CancelScan,
    TogglePause,
    ScanFinished { job_id: u64, crash: Option<PanicMessage> },
    Hover(Option<Segment>),
    Press(Segment),
    Activate(Segment),
    RowExit(NodeId),
    GoUp,
    Deselect,
    Navigate(NodeId),
    Back,
    Forward,
    SearchChanged(String),
    SearchFire(u64),
    SearchDone(u64, Option<Arc<SearchResult>>),
    ClearSearch,
    FocusSearch,
    Key(Key, keyboard::Modifiers),
    CursorMoved(Point),
    MouseReleased,
    CollectorEnter,
    CollectorExit,
    CollectSelected,
    Uncollect(NodeId),
    ClearCollector,
    AskTrash,
    CancelTrash,
    ConfirmTrash,
    DeleteHoldStart,
    DeleteHoldCancel,
    DeleteHoldTick,
    Rescan,
    GoHome,
    ResumeParked(usize),
    CloseParked(usize),
    DismissStatus,
    Preview,
    Reveal,
    DesktopActionFinished { failure: Option<String> },
}

impl App {
    pub fn new(initial: Option<PathBuf>) -> (Self, Task<Message>) {
        let mut app = App {
            screen: Screen::Start { scan: None },
            volumes: None,
            full_disk_access: true,
            error: None,
            parked_newest_first: Vec::new(),
            next_job: 1,
        };
        let load = Self::load_volumes();
        let task = match initial {
            Some(path) => Task::batch([load, app.start_scan(ScanOptions::folder(path))]),
            None => load,
        };
        (app, task)
    }

    fn load_volumes() -> Task<Message> {
        Task::perform(off_thread(|| (dscan_core::list_volumes(), dscan_core::has_full_disk_access())), |r| {
            let (volumes, fda) = r.unwrap_or_default();
            Message::VolumesLoaded(volumes, fda)
        })
    }

    pub fn title(&self) -> String {
        match &self.screen {
            Screen::Start { scan: Some(j) } => format!("Scanning {} — Disk Analyzer", j.opts.display_name()),
            Screen::Start { scan: None } => "Disk Analyzer".into(),
            Screen::Browse(b) => format!("{} — Disk Analyzer", b.tree.name(b.focus)),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match &self.screen {
            Screen::Start { scan: Some(job) } if !job.cancel.is_paused() => {
                iced::time::every(SCAN_PROGRESS_REFRESH).map(|_| Message::ScanProgressTick)
            }
            Screen::Start { .. } => Subscription::none(),
            Screen::Browse(b) => {
                let mut subscriptions = vec![event::listen_with(global_event)];
                if b.drag.is_some() {
                    subscriptions.push(event::listen_with(drag_event));
                }
                if b.animation.is_some() {
                    subscriptions.push(iced::window::frames().map(Message::AnimationFrame));
                }
                if b.delete_hold_started.is_some() {
                    subscriptions.push(iced::time::every(HOLD_PROGRESS_REFRESH).map(|_| Message::DeleteHoldTick));
                }
                Subscription::batch(subscriptions)
            }
        }
    }

    pub fn scan(&self) -> Option<&ScanJob> {
        match &self.screen {
            Screen::Start { scan } => scan.as_ref(),
            Screen::Browse(_) => None,
        }
    }

    fn scan_slot(&mut self) -> Option<&mut Option<ScanJob>> {
        match &mut self.screen {
            Screen::Start { scan } => Some(scan),
            Screen::Browse(_) => None,
        }
    }

    pub fn is_scanning(&self) -> bool {
        self.scan().is_some()
    }

    fn start_scan(&mut self, opts: ScanOptions) -> Task<Message> {
        let id = self.next_job;
        self.next_job += 1;
        let progress = Arc::new(Progress::default());
        let cancel = CancelToken::default();
        let result_slot: ScanResultSlot = Default::default();
        let work = {
            let (opts, progress, cancel, slot) = (opts.clone(), progress.clone(), cancel.clone(), result_slot.clone());
            off_thread(move || *slot.lock() = Some(dscan_core::scan(&opts, &progress, &cancel)))
        };
        self.error = None;
        let job = ScanJob { id, opts, progress, cancel, snapshot: ProgressSnapshot::default(), result_slot };
        if let Screen::Browse(b) = std::mem::replace(&mut self.screen, Screen::Start { scan: Some(job) }) {
            drop_off_thread(vec![*b]);
        }
        Task::perform(work, move |r| Message::ScanFinished { job_id: id, crash: r.err() })
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
                if let Some(settings) = dscan_core::FULL_DISK_ACCESS_SETTINGS {
                    let _ = std::process::Command::new("open").arg(settings).spawn();
                }
            }
            Message::RefreshVolumes => return Self::load_volumes(),
            Message::ScanProgressTick => {
                if let Some(Some(job)) = self.scan_slot() {
                    job.snapshot = job.progress.snapshot();
                }
            }
            Message::TogglePause => {
                if let Some(job) = self.scan() {
                    job.cancel.set_paused(!job.cancel.is_paused());
                }
            }
            Message::CancelScan => {
                if let Some(scan) = self.scan_slot() {
                    *scan = None;
                }
            }
            Message::ScanFinished { job_id, crash } => {
                let Some(job) = self.scan_slot().and_then(|scan| scan.take_if(|job| job.id == job_id)) else {
                    return Task::none();
                };
                let result = job.result_slot.lock().take();
                let opts = job.opts.clone();
                match result {
                    Some(Ok(tree)) => {
                        self.drop_parked_results_superseded_by(&tree);
                        self.screen = Screen::Browse(Box::new(Browser::new(tree, opts).with_opening_reveal()));
                    }
                    Some(Err(ScanError::Cancelled)) => {}
                    Some(Err(e)) => self.error = Some(e.to_string()),
                    None => {
                        let why = crash.unwrap_or_else(|| "no result".into());
                        self.error = Some(format!("The scan failed unexpectedly: {why}"));
                    }
                }
            }
            Message::GoHome if matches!(self.screen, Screen::Browse(_)) => {
                if let Screen::Browse(mut b) = std::mem::replace(&mut self.screen, Screen::Start { scan: None }) {
                    b.drag = None;
                    self.parked_newest_first.insert(0, *b);
                }
                self.error = None;
                return Self::load_volumes();
            }
            Message::ResumeParked(i) => {
                if matches!(self.screen, Screen::Start { scan: None }) && i < self.parked_newest_first.len() {
                    self.screen = Screen::Browse(Box::new(self.parked_newest_first.remove(i)));
                }
            }
            Message::CloseParked(i) => {
                if i < self.parked_newest_first.len() {
                    drop_off_thread(vec![self.parked_newest_first.remove(i)]);
                }
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

    fn drop_parked_results_superseded_by(&mut self, fresh: &Tree) {
        let scans_same_place =
            |b: &Browser| b.tree.root_path() == fresh.root_path() && b.tree.is_whole_disk() == fresh.is_whole_disk();
        let (stale, keep) = std::mem::take(&mut self.parked_newest_first).into_iter().partition(scans_same_place);
        self.parked_newest_first = keep;
        drop_off_thread(stale);
    }
}

fn is_cmd_shortcut_a_text_field_has_no_use_for(key: &Key, modifiers: keyboard::Modifiers) -> bool {
    modifiers.command() && matches!(key.as_ref(), Key::Character("f" | "[" | "]"))
}

fn global_event(event: iced::Event, status: event::Status, _window: iced::window::Id) -> Option<Message> {
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            let not_taken_by_focused_widget = status == event::Status::Ignored;
            (not_taken_by_focused_widget || is_cmd_shortcut_a_text_field_has_no_use_for(&key, modifiers))
                .then_some(Message::Key(key, modifiers))
        }
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Some(Message::MouseReleased),
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Back)) => Some(Message::Back),
        iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Forward)) => Some(Message::Forward),
        _ => None,
    }
}

fn drag_event(event: iced::Event, _status: event::Status, _window: iced::window::Id) -> Option<Message> {
    match event {
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => Some(Message::CursorMoved(position)),
        _ => None,
    }
}

fn shortcut_message(key: Key<&str>, cmd: bool) -> Option<Message> {
    Some(match key {
        Key::Named(Named::ArrowUp) if cmd => Message::GoUp,
        Key::Named(Named::Backspace) if !cmd => Message::Back,
        Key::Named(Named::BrowserBack) => Message::Back,
        Key::Named(Named::BrowserForward) => Message::Forward,
        Key::Character("[") if cmd => Message::Back,
        Key::Character("]") if cmd => Message::Forward,
        Key::Character("f") if cmd => Message::FocusSearch,
        Key::Character("c") if !cmd => Message::CollectSelected,
        Key::Named(Named::Space) if !cmd => Message::Preview,
        _ => return None,
    })
}

impl Browser {
    pub fn new(tree: Tree, opts: ScanOptions) -> Self {
        let root = tree.root();
        let mut b = Browser {
            tree: Arc::new(tree),
            opts,
            focus: root,
            history: Vec::new(),
            forward: Vec::new(),
            layout: Layout::default(),
            layout_revision: 0,
            chart_cache: Cache::new(),
            highlight_cache: Cache::new(),
            hovered: None,
            selected: None,
            search_text: String::new(),
            search_gen: 0,
            search_cancel: CancelToken::default(),
            search: None,
            list: ListView::default(),
            collector: Collection::default(),
            exclusions: Exclusions::default(),
            animation: None,
            over_collector: false,
            confirm_trash: false,
            removal_waiting_for_tree: None,
            delete_hold_started: None,
            drag: None,
            status: None,
        };
        b.relayout();
        b.sync_list();
        b
    }

    pub fn with_opening_reveal(mut self) -> Self {
        self.animation = Some(ChartAnimation::new(Transition::reveal(&self.layout)));
        self
    }

    pub fn can_back(&self) -> bool {
        !self.history.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    fn computed_layout(&self) -> Layout {
        dscan_core::sunburst(&self.tree, self.focus, LayoutParams::default(), self.search.as_deref(), &self.exclusions)
    }

    fn relayout(&mut self) {
        self.install_layout(self.computed_layout());
    }

    fn relayout_animated(&mut self) {
        let layout = self.computed_layout();
        self.animation = Some(ChartAnimation::new(Transition::between(&self.layout, &layout)));
        self.install_layout(layout);
    }

    fn install_layout(&mut self, layout: Layout) {
        self.layout = layout;
        self.layout_revision += 1;
        self.chart_cache.clear();
        self.highlight_cache.clear();
    }

    pub fn size(&self, id: NodeId) -> u64 {
        self.exclusions.size(&self.tree, id)
    }

    pub fn listed_folder(&self) -> NodeId {
        match self.selected {
            Some(s) if self.tree.is_strict_ancestor_of(self.focus, s) => self.tree.parent(s).unwrap_or(self.focus),
            _ => self.focus,
        }
    }

    fn visible_children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.exclusions.children(&self.tree, id).filter(|&c| self.search.as_ref().is_none_or(|s| s.is_relevant(c)))
    }

    fn sync_list(&mut self) {
        let subject = self.listed_folder();
        let source = Some(ListSource { folder: subject, layout_revision: self.layout_revision });
        if self.list.built_for == source {
            return;
        }
        let total = self.size(subject).max(1);
        let children: Vec<NodeId> = self.visible_children(subject).collect();
        let rows = children
            .iter()
            .take(MAX_LIST_ROWS)
            .map(|&id| {
                let node = self.tree.node(id);
                let size = self.size(id);
                let mut name = truncate(&self.tree.name(id), 44);
                if node.is_dir() {
                    name.push('/');
                }
                if node.is_unreadable() {
                    name.push_str("  (no access)");
                } else if node.is_clone_copy() {
                    name.push_str("  (clone)");
                } else if node.shares_blocks_counted_elsewhere() {
                    name.push_str("  (partly a clone)");
                } else if node.is_hardlink_copy() {
                    name.push_str("  (hard link)");
                }
                ListRow { id, name, size: format_size(size), fraction_of_listed_folder: size as f32 / total as f32 }
            })
            .collect();
        let cut_off_rows_summary = (children.len() > MAX_LIST_ROWS).then(|| {
            let rest: u64 = children[MAX_LIST_ROWS..].iter().map(|&c| self.size(c)).sum();
            format!("… {} more items · {}", thousands((children.len() - MAX_LIST_ROWS) as u64), format_size(rest))
        });
        self.list = ListView { built_for: source, rows, cut_off_rows_summary };
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
        self.relayout_animated();
    }

    fn select_and_reveal(&mut self, id: NodeId) {
        self.selected = Some(id);
        let visible = self.tree.is_strict_ancestor_of(self.focus, id) && self.layout.find(Segment::Node(id)).is_some();
        if !visible && let Some(p) = self.tree.parent(id) {
            self.set_focus(p, true);
        }
    }

    fn activate(&mut self, id: NodeId) {
        if self.tree.node(id).is_dir() {
            self.set_focus(id, true);
        } else {
            self.selected = Some(id);
        }
    }

    fn go_up(&mut self) {
        if let Some(p) = self.tree.parent(self.focus) {
            let old = self.focus;
            self.set_focus(p, true);
            self.selected = Some(old);
        }
    }

    fn is_still_reachable_history_entry(&self, id: NodeId) -> bool {
        self.tree.is_live(id) && id != self.focus
    }

    fn back(&mut self) {
        while let Some(prev) = self.history.pop() {
            if self.is_still_reachable_history_entry(prev) {
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
            if self.is_still_reachable_history_entry(next) {
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
                    self.select_and_reveal(c);
                }
            }
            _ => {}
        }
    }

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
            Message::SearchDone(generation, r.ok().flatten())
        })
    }

    fn collect(&mut self, id: NodeId) {
        match self.collector.add(&self.tree, id) {
            Ok(()) => self.collector_changed(),
            Err(Refusal::Protected) => {
                self.status = Some(format!(
                    "“{}” is a protected system location and can't be moved to the {TRASH_NAME}.",
                    truncate(&self.tree.name(id), 40)
                ))
            }
            Err(Refusal::NotActionable | Refusal::AlreadyCoveredByCollectedFolder) => {}
        }
    }

    fn collector_changed(&mut self) {
        self.exclusions = self.collector.exclusions(&self.tree);
        if self.selected.is_some_and(|s| self.collector.covers(&self.tree, s)) {
            self.selected = None;
        }
        self.hovered = None;
        self.relayout_animated();
    }

    fn hand_selection_to_desktop(
        &self,
        action_name: &'static str,
        action: fn(&Path) -> Result<(), DesktopError>,
    ) -> Task<Message> {
        let Some(id) = self.selected.filter(|&s| self.tree.is_actionable(s)) else { return Task::none() };
        let path = self.tree.path(id);
        let name = truncate(&self.tree.name(id), 40);
        Task::perform(off_thread(move || action(&path)), move |outcome| {
            let why = match outcome {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e.to_string()),
                Err(panic) => Some(panic),
            };
            Message::DesktopActionFinished {
                failure: why.map(|why| format!("{action_name} failed for “{name}”: {why}")),
            }
        })
    }

    fn wait_for_the_cancelled_search_to_report_done() -> Task<Message> {
        Task::none()
    }

    pub fn delete_hold_progress(&self) -> Option<f32> {
        self.delete_hold_started
            .map(|started| (started.elapsed().as_secs_f32() / HOLD_TO_DELETE.as_secs_f32()).min(1.0))
    }

    fn remove_collected(&mut self, removal: Removal) -> Task<Message> {
        self.removal_waiting_for_tree = Some(PendingRemoval { removal, items: self.collector.items().to_vec() });
        self.remove_waiting_batch()
    }

    fn remove_waiting_batch(&mut self) -> Task<Message> {
        self.search_cancel.cancel();
        let Some(tree) = Arc::get_mut(&mut self.tree) else {
            return Self::wait_for_the_cancelled_search_to_report_done();
        };
        let Some(PendingRemoval { removal, items }) = self.removal_waiting_for_tree.take() else {
            return Task::none();
        };
        let report = actions::remove_from_disk(tree, &items, removal);
        self.collector.prune(&self.tree);
        let items_phrase = format!("{} item{}", report.removed, if report.removed == 1 { "" } else { "s" });
        let mut msg = match removal {
            Removal::MoveToTrash => format!(
                "Moved {items_phrase} ({}) to the {TRASH_NAME}. Empty the {TRASH_NAME} to free the space.",
                format_size(report.bytes)
            ),
            Removal::DeletePermanently => {
                format!("Deleted {items_phrase} permanently, freeing {}.", format_size(report.bytes))
            }
        };
        if let Some((_, err)) = report.failed.first() {
            msg.push_str(&format!(" {} failed: {err}", report.failed.len()));
        }
        self.status = Some(msg);
        self.focus = self.tree.nearest_live(self.focus);
        self.selected = self.selected.filter(|&s| self.tree.is_live(s));
        self.search_gen += 1;
        self.collector_changed();
        self.run_search()
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let before = (self.hovered, self.selected);
        let task = self.handle(message);
        if (self.hovered, self.selected) != before {
            self.highlight_cache.clear();
        }
        self.sync_list();
        task
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Hover(h) => self.hovered = h,
            Message::RowExit(id) if self.hovered == Some(Segment::Node(id)) => {
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
            Message::AnimationFrame(now) => {
                if let Some(Advance::Finished) = self.animation.as_mut().map(|a| a.advance(now)) {
                    self.animation = None;
                }
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
                if self.removal_waiting_for_tree.is_some() {
                    return self.remove_waiting_batch();
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
                        if let Some(msg) = shortcut_message(k, cmd) {
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
                self.delete_hold_started = None;
                if let Some(d) = self.drag.take()
                    && d.dragging
                    && self.over_collector
                {
                    self.collect(d.item);
                }
            }
            Message::CollectorEnter => self.over_collector = true,
            Message::CollectorExit => self.over_collector = false,
            Message::CollectSelected => {
                if let Some(s) = self.selected {
                    self.collect(s);
                }
            }
            Message::Uncollect(id) => {
                self.collector.remove(id);
                self.collector_changed();
            }
            Message::ClearCollector => {
                self.collector.clear();
                self.confirm_trash = false;
                self.collector_changed();
            }
            Message::AskTrash => self.confirm_trash = !self.collector.is_empty(),
            Message::CancelTrash => self.confirm_trash = false,
            Message::ConfirmTrash => {
                self.confirm_trash = false;
                return self.remove_collected(Removal::MoveToTrash);
            }
            Message::DeleteHoldStart if !self.collector.is_empty() => self.delete_hold_started = Some(Instant::now()),
            Message::DeleteHoldCancel => self.delete_hold_started = None,
            Message::DeleteHoldTick
                if self.delete_hold_started.is_some_and(|started| started.elapsed() >= HOLD_TO_DELETE) =>
            {
                self.delete_hold_started = None;
                return self.remove_collected(Removal::DeletePermanently);
            }
            Message::DismissStatus => self.status = None,
            Message::Preview => return self.hand_selection_to_desktop(PREVIEW_ACTION, desktop::preview),
            Message::Reveal => return self.hand_selection_to_desktop(REVEAL_ACTION, desktop::reveal),
            Message::DesktopActionFinished { failure: Some(why) } => self.status = Some(why),
            _ => {}
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser(dir: &std::path::Path) -> Browser {
        for (p, size) in [("Movies/a.mov", 3000), ("Movies/b.mov", 2000), ("Music/c.mp3", 1000)] {
            let path = dir.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, vec![1u8; size]).unwrap();
        }
        let opts = ScanOptions::folder(dir);
        let tree = dscan_core::scan(&opts, &Progress::default(), &CancelToken::default()).unwrap();
        Browser::new(tree, opts)
    }

    fn child(b: &Browser, parent: NodeId, name: &str) -> NodeId {
        b.tree.children(parent).find(|&c| b.tree.name(c) == name).unwrap()
    }

    fn remove_focused_folder_as_trashing_does(b: &mut Browser) {
        Arc::get_mut(&mut b.tree).unwrap().remove(b.focus);
        b.focus = b.tree.nearest_live(b.focus);
    }

    fn abandon_waiting_trash_batch_so_the_real_trash_is_never_touched(b: &mut Browser) {
        b.removal_waiting_for_tree = None;
    }

    fn waiting_items(b: &Browser) -> Option<&[NodeId]> {
        b.removal_waiting_for_tree.as_ref().map(|pending| &pending.items[..])
    }

    #[test]
    fn back_skips_history_made_stale_by_removal() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let root = b.tree.root();
        let movies = child(&b, root, "Movies");
        let _ = b.update(Message::Navigate(movies));
        remove_focused_folder_as_trashing_does(&mut b);
        assert_eq!(b.focus, root);

        let _ = b.update(Message::Back);
        assert_eq!(b.focus, root);
        assert!(!b.can_back() && !b.can_forward(), "no phantom history entries");
    }

    fn pretend_held_for(b: &mut Browser, held: Duration) {
        b.delete_hold_started = Some(Instant::now() - held);
    }

    #[test]
    fn holding_delete_for_three_seconds_deletes_the_collected_items_permanently() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let movies = child(&b, b.tree.root(), "Movies");
        b.collector.add(&b.tree, movies).unwrap();

        let _ = b.update(Message::DeleteHoldStart);
        pretend_held_for(&mut b, Duration::from_secs(1));
        let _ = b.update(Message::DeleteHoldTick);
        assert!(dir.path().join("Movies").exists(), "nothing is deleted before three seconds");
        let progress = b.delete_hold_progress().unwrap();
        assert!((0.3..0.4).contains(&progress), "one of three seconds held: {progress}");

        pretend_held_for(&mut b, HOLD_TO_DELETE);
        let _ = b.update(Message::DeleteHoldTick);
        assert!(!dir.path().join("Movies").exists());
        assert!(!b.tree.is_live(movies) && b.collector.is_empty());
        assert!(b.delete_hold_started.is_none());
        assert!(b.status.as_deref().is_some_and(|s| s.starts_with("Deleted 1 item permanently")), "{:?}", b.status);
    }

    #[test]
    fn releasing_or_leaving_the_button_early_cancels_the_delete() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let movies = child(&b, b.tree.root(), "Movies");
        b.collector.add(&b.tree, movies).unwrap();
        for early_end in [Message::DeleteHoldCancel, Message::MouseReleased] {
            let _ = b.update(Message::DeleteHoldStart);
            let _ = b.update(early_end);
            assert!(b.delete_hold_progress().is_none());
            let _ = b.update(Message::DeleteHoldTick);
            assert!(dir.path().join("Movies").exists());
        }
    }

    #[test]
    fn delete_hold_needs_something_collected() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let _ = b.update(Message::DeleteHoldStart);
        assert!(b.delete_hold_progress().is_none());
    }

    #[test]
    fn deferred_trash_keeps_the_confirmed_batch() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let root = b.tree.root();
        let (movies, music) = (child(&b, root, "Movies"), child(&b, root, "Music"));
        b.collector.add(&b.tree, movies).unwrap();
        let running_search = Arc::clone(&b.tree);

        let _ = b.update(Message::ConfirmTrash);
        assert_eq!(waiting_items(&b), Some(&[movies][..]), "deferred while the search holds the tree");
        let _ = b.update(Message::Navigate(root));
        let _ = b.update(Message::Press(Segment::Node(music)));
        let _ = b.update(Message::CollectSelected);
        assert_eq!(b.collector.items(), &[movies, music]);
        assert_eq!(waiting_items(&b), Some(&[movies][..]), "later additions are not trashed");

        abandon_waiting_trash_batch_so_the_real_trash_is_never_touched(&mut b);
        drop(running_search);
    }

    #[test]
    fn mouse_side_buttons_walk_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let root = b.tree.root();
        let movies = child(&b, root, "Movies");
        let _ = b.update(Message::Navigate(movies));
        let press = |button| {
            let event = iced::Event::Mouse(mouse::Event::ButtonPressed(button));
            global_event(event, event::Status::Ignored, iced::window::Id::unique()).unwrap()
        };
        let _ = b.update(press(mouse::Button::Back));
        assert_eq!(b.focus, root);
        let _ = b.update(press(mouse::Button::Forward));
        assert_eq!(b.focus, movies);
    }

    #[test]
    fn space_previews_the_selection_unless_a_modifier_is_held() {
        assert!(matches!(shortcut_message(Key::Named(Named::Space), false), Some(Message::Preview)));
        assert!(shortcut_message(Key::Named(Named::Space), true).is_none());
    }

    #[test]
    fn a_failed_desktop_action_is_reported_in_the_status_bar() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let failure = Some("Quick Look failed for “a.mov”: not found".to_string());
        let _ = b.update(Message::DesktopActionFinished { failure: failure.clone() });
        assert_eq!(b.status, failure);
        let _ = b.update(Message::DismissStatus);
        let _ = b.update(Message::DesktopActionFinished { failure: None });
        assert_eq!(b.status, None, "success needs no message");
    }

    #[test]
    fn desktop_actions_need_a_real_selected_item() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        b.selected = None;
        assert!(b.hand_selection_to_desktop(PREVIEW_ACTION, |_| panic!("nothing is selected")).units() == 0);
        b.selected = Some(b.tree.root());
        assert!(b.hand_selection_to_desktop(REVEAL_ACTION, |_| panic!("the root is not an item")).units() == 0);
    }

    #[test]
    fn search_field_keeps_cmd_arrows() {
        let press = |key: Key, status| {
            let event = iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key: key.clone(),
                modified_key: key,
                physical_key: keyboard::key::Physical::Unidentified(keyboard::key::NativeCode::Unidentified),
                location: keyboard::Location::Standard,
                modifiers: keyboard::Modifiers::COMMAND,
                text: None,
                repeat: false,
            });
            global_event(event, status, iced::window::Id::unique()).is_some()
        };
        assert!(!press(Key::Named(Named::ArrowUp), event::Status::Captured));
        assert!(!press(Key::Named(Named::ArrowDown), event::Status::Captured));
        assert!(press(Key::Character("[".into()), event::Status::Captured));
        assert!(press(Key::Character("f".into()), event::Status::Captured));
        assert!(press(Key::Named(Named::ArrowUp), event::Status::Ignored));
    }
}
