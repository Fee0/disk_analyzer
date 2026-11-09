use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dscan_core::{
    Accounting, CancelToken, Collection, DesktopError, Exclusions, Layout, LayoutParams, NodeId, PREVIEW_ACTION,
    Progress, ProgressSnapshot, REVEAL_ACTION, Refusal, Removal, RemovalPlan, RemovalReport, ScanError, ScanOptions,
    SearchResult, Segment, TRASH_NAME, Transition, Tree, Volume, desktop, format_size,
};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::canvas::Cache;
use iced::{Point, Subscription, Task, event, mouse};

use crate::animation::{Advance, ChartAnimation};
use crate::fmt::{NAME_IN_A_MESSAGE_MAX_CHARS, count_phrase, items_phrase, truncate, with_folder_mark};

pub const APP_NAME: &str = "Disk Analyzer";
pub const SEARCH_ID: &str = "search";
const LIST_ROW_NAME_MAX_CHARS: usize = 44;
pub const MAX_LIST_ROWS: usize = 400;
const DRAG_THRESHOLD: f32 = 4.0;
pub const HOLD_TO_DELETE: Duration = Duration::from_secs(3);
const NOTICE_LIFETIME: Duration = Duration::from_secs(4);
const HOLD_PROGRESS_REFRESH: Duration = Duration::from_millis(33);
const SCAN_PROGRESS_REFRESH: Duration = Duration::from_millis(100);
const SEARCH_TYPING_PAUSE: Duration = Duration::from_millis(150);

pub type PanicMessage = String;

pub type BrowserId = u64;

fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

enum RemovalProgress {
    OnDisk,
    Finished(Result<Arc<RemovalReport>, PanicMessage>),
}

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
    pub scans: Vec<ScanJob>,
    stepped_back_from_newest_parked: bool,
}

pub enum Screen {
    Start,
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

    fn scans_same_place_as(&self, opts: &ScanOptions) -> bool {
        self.opts.whole_disk == opts.whole_disk && self.opts.root == opts.root
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

#[derive(Default)]
pub struct ListView {
    built_for: Option<NodeId>,
    pub rows: Vec<ListRow>,
    pub cut_off_rows_summary: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Done,
    Warning,
    Error,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchStatus {
    Shown,
    Due,
    Running,
}

pub struct Browser {
    pub id: BrowserId,
    pub tree: Arc<Tree>,
    pub opts: ScanOptions,
    pub focus: NodeId,
    history: Vec<NodeId>,
    forward: Vec<NodeId>,
    pub layout: Layout,
    pub chart_cache: Cache,
    pub highlight_cache: Cache,
    pub hovered: Option<Segment>,
    pub selected: Option<NodeId>,
    pub search_text: String,
    search_gen: u64,
    search_status: SearchStatus,
    search_cancel: CancelToken,
    pub search: Option<Arc<SearchResult>>,
    pub list: ListView,
    pub collector: Collection,
    pub exclusions: Exclusions,
    pub animation: Option<ChartAnimation>,
    pub over_collector: bool,
    pub collected_list_open: bool,
    pub confirm_trash: bool,
    removal: Option<RemovalProgress>,
    pub(crate) delete_hold_started: Option<Instant>,
    pub drag: Option<PendingDrag>,
    pub notice: Option<Notice>,
    notice_expires_at: Option<Instant>,
    notice_hovered: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    VolumesLoaded(Vec<Volume>, bool),
    ScanVolume(PathBuf),
    OpenFolder,
    FolderPicked(Option<PathBuf>),
    OpenFdaSettings,
    RefreshVolumes,
    ScanProgressTick,
    AnimationFrame(Instant),
    TogglePause(u64),
    StopScan(u64),
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
    SearchFire { generation: u64 },
    SearchDone { generation: u64, result: Option<Arc<SearchResult>> },
    ClearSearch,
    FocusSearch,
    Key(Key, keyboard::Modifiers),
    CursorMoved(Point),
    MouseReleased,
    CollectorEnter,
    CollectorExit,
    ToggleCollectedList,
    CollectSelected,
    Uncollect(NodeId),
    ClearCollector,
    AskTrash,
    CancelTrash,
    ConfirmTrash,
    DeleteHoldStart,
    DeleteHoldCancel,
    DeleteHoldTick,
    RemovalDone { outcome: Result<Arc<RemovalReport>, PanicMessage> },
    Rescan,
    GoHome,
    ResumeParked(BrowserId),
    CloseParked(BrowserId),
    DismissNotice,
    NoticeExpired,
    NoticeEnter,
    NoticeExit,
    Preview,
    Reveal,
    DesktopActionFailed(String),
    ForBrowser(BrowserId, Box<Message>),
}

impl App {
    pub fn new(initial: Option<PathBuf>) -> (Self, Task<Message>) {
        let mut app = App {
            screen: Screen::Start,
            volumes: None,
            full_disk_access: true,
            error: None,
            parked_newest_first: Vec::new(),
            scans: Vec::new(),
            stepped_back_from_newest_parked: false,
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
            Screen::Start if self.scans.is_empty() => APP_NAME.into(),
            Screen::Start => {
                let names: Vec<String> = self.scans.iter().map(|job| job.opts.display_name()).collect();
                format!("Scanning {} — {APP_NAME}", names.join(", "))
            }
            Screen::Browse(b) => format!("{} — {APP_NAME}", b.tree.name(b.focus)),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        match &self.screen {
            Screen::Start if self.scans.iter().any(|job| !job.cancel.is_paused()) => Subscription::batch([
                event::listen_with(global_event),
                iced::time::every(SCAN_PROGRESS_REFRESH).map(|_| Message::ScanProgressTick),
            ]),
            Screen::Start => event::listen_with(global_event),
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

    fn scan_job(&self, id: u64) -> Option<&ScanJob> {
        self.scans.iter().find(|job| job.id == id)
    }

    fn start_scan(&mut self, opts: ScanOptions) -> Task<Message> {
        let id = next_id();
        let progress = Arc::new(Progress::default());
        let cancel = CancelToken::default();
        let result_slot: ScanResultSlot = Default::default();
        let work = {
            let (opts, progress, cancel, slot) = (opts.clone(), progress.clone(), cancel.clone(), result_slot.clone());
            off_thread(move || *slot.lock() = Some(dscan_core::scan(&opts, &progress, &cancel)))
        };
        self.error = None;
        self.stepped_back_from_newest_parked = false;
        self.scans.retain(|job| !job.scans_same_place_as(&opts));
        self.scans.push(ScanJob { id, opts, progress, cancel, snapshot: ProgressSnapshot::default(), result_slot });
        if let Screen::Browse(b) = std::mem::replace(&mut self.screen, Screen::Start) {
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
            Message::ScanVolume(mount) => {
                let volume = self.volumes.iter().flatten().find(|v| v.mount == mount);
                if let Some(opts) = volume.map(|v| ScanOptions::volume(&v.mount, &v.name)) {
                    return self.start_scan(opts);
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
                for job in &mut self.scans {
                    job.snapshot = job.progress.snapshot();
                }
            }
            Message::TogglePause(id) => {
                if let Some(job) = self.scan_job(id) {
                    job.cancel.set_paused(!job.cancel.is_paused());
                }
            }
            Message::StopScan(id) => self.scans.retain(|job| job.id != id),
            Message::ScanFinished { job_id, crash } => {
                let Some(i) = self.scans.iter().position(|job| job.id == job_id) else {
                    return Task::none();
                };
                let job = self.scans.remove(i);
                let result = job.result_slot.lock().take();
                return match result {
                    Some(Ok(tree)) => self.open_or_park_scan_result(tree, job.opts.clone()),
                    Some(Err(ScanError::Cancelled)) => Task::none(),
                    Some(Err(e)) => self.report_scan_failure(e.to_string()),
                    None => {
                        let why = crash.unwrap_or_else(|| "no result".into());
                        self.report_scan_failure(format!("The scan failed unexpectedly: {why}"))
                    }
                };
            }
            Message::GoHome => return self.go_home(),
            Message::Back if matches!(&self.screen, Screen::Browse(b) if !b.can_back()) => return self.go_home(),
            Message::Forward if matches!(self.screen, Screen::Start) => {
                if self.stepped_back_from_newest_parked {
                    return self.resume_parked(0);
                }
            }
            Message::Key(key, mods) => match navigation_shortcut(key.as_ref(), mods.command()) {
                Some(navigation) => return self.update(navigation),
                None => return self.update_browser(Message::Key(key, mods)),
            },
            Message::ResumeParked(id) => {
                if let Some(i) = self.parked_position(id) {
                    return self.resume_parked(i);
                }
            }
            Message::ForBrowser(id, message) => return self.update_browser_by_id(id, *message),
            Message::CloseParked(id) => {
                if let Some(i) = self.parked_position(id) {
                    drop_off_thread(vec![self.parked_newest_first.remove(i)]);
                    self.stepped_back_from_newest_parked &= i != 0;
                }
            }
            Message::Rescan => {
                if let Screen::Browse(b) = &self.screen {
                    let opts = b.opts.clone();
                    return self.start_scan(opts);
                }
            }
            other => return self.update_browser(other),
        }
        Task::none()
    }

    fn parked_position(&self, id: BrowserId) -> Option<usize> {
        self.parked_newest_first.iter().position(|b| b.id == id)
    }

    fn update_browser_by_id(&mut self, id: BrowserId, message: Message) -> Task<Message> {
        if let Screen::Browse(b) = &mut self.screen
            && b.id == id
        {
            return b.update(message);
        }
        if let Some(parked) = self.parked_newest_first.iter_mut().find(|b| b.id == id) {
            parked.note_while_parked(message);
        }
        Task::none()
    }

    fn update_browser(&mut self, message: Message) -> Task<Message> {
        match &mut self.screen {
            Screen::Browse(b) => b.update(message),
            Screen::Start => Task::none(),
        }
    }

    fn go_home(&mut self) -> Task<Message> {
        let Screen::Browse(mut b) = std::mem::replace(&mut self.screen, Screen::Start) else {
            return Task::none();
        };
        b.park();
        self.parked_newest_first.insert(0, *b);
        self.stepped_back_from_newest_parked = true;
        self.error = None;
        Self::load_volumes()
    }

    fn resume_parked(&mut self, i: usize) -> Task<Message> {
        if !matches!(self.screen, Screen::Start) || i >= self.parked_newest_first.len() {
            return Task::none();
        }
        let mut resumed = Box::new(self.parked_newest_first.remove(i));
        let catch_up = resumed.resume();
        self.screen = Screen::Browse(resumed);
        self.stepped_back_from_newest_parked = false;
        catch_up
    }

    fn open_or_park_scan_result(&mut self, tree: Tree, opts: ScanOptions) -> Task<Message> {
        self.drop_parked_results_superseded_by(&tree);
        let finished = format!("Finished scanning {}. Open it from the start page.", opts.display_name());
        let browser = Browser::new(tree, opts);
        let notice = match &mut self.screen {
            Screen::Start if self.scans.is_empty() => {
                self.screen = Screen::Browse(Box::new(browser.with_opening_reveal()));
                return Task::none();
            }
            Screen::Start => Task::none(),
            Screen::Browse(b) => b.notify(NoticeKind::Done, finished),
        };
        self.parked_newest_first.insert(0, browser);
        self.stepped_back_from_newest_parked = false;
        notice
    }

    fn report_scan_failure(&mut self, why: String) -> Task<Message> {
        match &mut self.screen {
            Screen::Browse(b) => b.notify(NoticeKind::Error, why),
            Screen::Start => {
                self.error = Some(why);
                Task::none()
            }
        }
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
    modifiers.command() && matches!(key.as_ref(), Key::Character("f" | "r" | "[" | "]"))
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

fn navigation_shortcut(key: Key<&str>, cmd: bool) -> Option<Message> {
    Some(match key {
        Key::Named(Named::Backspace) if !cmd => Message::Back,
        Key::Named(Named::BrowserBack) => Message::Back,
        Key::Named(Named::BrowserForward) => Message::Forward,
        Key::Character("[") if cmd => Message::Back,
        Key::Character("]") if cmd => Message::Forward,
        Key::Named(Named::F5) | Key::Named(Named::BrowserRefresh) => Message::Rescan,
        Key::Character("r") if cmd => Message::Rescan,
        _ => return None,
    })
}

fn shortcut_message(key: Key<&str>, cmd: bool) -> Option<Message> {
    Some(match key {
        Key::Named(Named::ArrowUp) if cmd => Message::GoUp,
        Key::Character("f") if cmd => Message::FocusSearch,
        Key::Character("c") if !cmd => Message::CollectSelected,
        Key::Named(Named::Space) if !cmd => Message::Preview,
        _ => return None,
    })
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.search_cancel.cancel();
    }
}

impl Browser {
    pub fn new(tree: Tree, opts: ScanOptions) -> Self {
        let root = tree.root();
        let mut b = Browser {
            id: next_id(),
            tree: Arc::new(tree),
            opts,
            focus: root,
            history: Vec::new(),
            forward: Vec::new(),
            layout: Layout::default(),
            chart_cache: Cache::new(),
            highlight_cache: Cache::new(),
            hovered: None,
            selected: None,
            search_text: String::new(),
            search_gen: 0,
            search_status: SearchStatus::Shown,
            search_cancel: CancelToken::default(),
            search: None,
            list: ListView::default(),
            collector: Collection::default(),
            exclusions: Exclusions::default(),
            animation: None,
            over_collector: false,
            collected_list_open: false,
            confirm_trash: false,
            removal: None,
            delete_hold_started: None,
            drag: None,
            notice: None,
            notice_expires_at: None,
            notice_hovered: false,
        };
        b.relayout();
        b.sync_list();
        b
    }

    pub fn with_opening_reveal(mut self) -> Self {
        self.animation = Some(ChartAnimation::new(Transition::reveal(&self.layout)));
        self
    }

    pub fn active_drag(&self) -> Option<&PendingDrag> {
        self.drag.as_ref().filter(|d| d.dragging)
    }

    pub fn dropping_on_collector(&self) -> bool {
        self.over_collector && self.active_drag().is_some()
    }

    pub fn can_back(&self) -> bool {
        self.history.iter().any(|&id| self.is_still_reachable_history_entry(id))
    }

    pub fn can_forward(&self) -> bool {
        self.forward.iter().any(|&id| self.is_still_reachable_history_entry(id))
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
        self.list.built_for = None;
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

    fn visible_children_with_sizes(&self, id: NodeId) -> impl Iterator<Item = (NodeId, u64)> + '_ {
        let relevant = |c: NodeId| self.search.as_ref().is_none_or(|s| s.is_relevant(c));
        self.exclusions.children_with_sizes(&self.tree, id).filter(move |&(c, _)| relevant(c))
    }

    fn visible_children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.visible_children_with_sizes(id).map(|(c, _)| c)
    }

    fn sync_list(&mut self) {
        let subject = self.listed_folder();
        if self.list.built_for == Some(subject) {
            return;
        }
        let total = self.size(subject).max(1);
        let mut children = self.visible_children_with_sizes(subject);
        let rows = children.by_ref().take(MAX_LIST_ROWS).map(|(id, size)| self.list_row(id, size, total)).collect();
        let (cut_off, cut_off_bytes) =
            children.fold((0u64, 0u64), |(count, bytes), (_, size)| (count + 1, bytes + size));
        let cut_off_rows_summary = (cut_off > 0).then(|| {
            format!("… {} · {}", count_phrase(cut_off, "more item", "more items"), format_size(cut_off_bytes))
        });
        self.list = ListView { built_for: Some(subject), rows, cut_off_rows_summary };
    }

    fn list_row(&self, id: NodeId, size: u64, listed_folder_size: u64) -> ListRow {
        let node = self.tree.node(id);
        let mut name = with_folder_mark(truncate(&self.tree.name(id), LIST_ROW_NAME_MAX_CHARS), node.is_dir());
        name.push_str(match node.accounting() {
            Accounting::Counted => "",
            Accounting::Unreadable => "  (no access)",
            Accounting::CloneCopy => "  (clone)",
            Accounting::SharesBlocksCountedElsewhere => "  (partly a clone)",
            Accounting::HardLinkCopy => "  (hard link)",
        });
        ListRow {
            id,
            name,
            size: format_size(size),
            fraction_of_listed_folder: size as f32 / listed_folder_size as f32,
        }
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

    fn sibling_beside(&self, id: NodeId, after: bool) -> Option<NodeId> {
        let parent = self.tree.parent(id).unwrap_or(self.focus);
        let siblings: Vec<NodeId> = self.visible_children(parent).collect();
        let i = siblings.iter().position(|&s| s == id)?;
        let beside = if after { i.checked_add(1) } else { i.checked_sub(1) };
        Some(beside.and_then(|j| siblings.get(j).copied()).unwrap_or(id))
    }

    fn move_selection(&mut self, key: Named) {
        let Some(sel) = self.selected.filter(|&s| self.tree.is_strict_ancestor_of(self.focus, s)) else {
            let first = self.visible_children(self.focus).next();
            self.selected = first;
            return;
        };
        match key {
            Named::ArrowLeft | Named::ArrowRight => {
                if let Some(sibling) = self.sibling_beside(sel, key == Named::ArrowRight) {
                    self.selected = Some(sibling);
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

    fn has_query(&self) -> bool {
        !self.search_text.trim().is_empty()
    }

    fn run_search(&mut self) -> Task<Message> {
        let search = self.restart_search();
        if !self.has_query() {
            self.relayout();
        }
        search
    }

    fn restart_search(&mut self) -> Task<Message> {
        self.search_cancel.cancel();
        self.search_cancel = CancelToken::default();
        if !self.has_query() {
            self.search = None;
            self.search_status = SearchStatus::Shown;
            return Task::none();
        }
        self.search_status = SearchStatus::Running;
        let (tree, query, cancel) = (Arc::clone(&self.tree), self.search_text.clone(), self.search_cancel.clone());
        let generation = self.search_gen;
        Task::perform(off_thread(move || dscan_core::search(&tree, &query, &cancel).map(Arc::new)), move |r| {
            Message::SearchDone { generation, result: r.ok().flatten() }
        })
    }

    fn collect(&mut self, id: NodeId) {
        match self.collector.add(&self.tree, id) {
            Ok(()) => self.collector_changed(),
            Err(Refusal::Protected) => self.show_notice(
                NoticeKind::Warning,
                format!(
                    "“{}” is a protected system location and can't be moved to the {TRASH_NAME}.",
                    truncate(&self.tree.name(id), NAME_IN_A_MESSAGE_MAX_CHARS)
                ),
            ),
            Err(Refusal::NotActionable | Refusal::AlreadyCoveredByCollectedFolder) => {}
        }
    }

    fn notify(&mut self, kind: NoticeKind, text: String) -> Task<Message> {
        self.updating(|b| {
            b.show_notice(kind, text);
            Task::none()
        })
    }

    fn show_notice(&mut self, kind: NoticeKind, text: String) {
        self.notice = Some(Notice { kind, text });
        self.notice_expires_at = (!self.notice_hovered).then(|| Instant::now() + NOTICE_LIFETIME);
    }

    fn dismiss_notice(&mut self) {
        self.notice = None;
        self.notice_expires_at = None;
        self.notice_hovered = false;
    }

    fn check_the_notice_again_once_its_lifetime_is_up() -> Task<Message> {
        Task::perform(async { tokio::time::sleep(NOTICE_LIFETIME).await }, |_| Message::NoticeExpired)
    }

    fn collector_changed(&mut self) {
        self.collected_list_open &= !self.collector.is_empty();
        self.confirm_trash &= !self.collector.is_empty();
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
        let name = truncate(&self.tree.name(id), NAME_IN_A_MESSAGE_MAX_CHARS);
        Task::perform(off_thread(move || action(&path)), move |outcome| {
            let why = match outcome {
                Ok(Ok(())) => return None,
                Ok(Err(e)) => e.to_string(),
                Err(panic) => panic,
            };
            Some(Message::DesktopActionFailed(format!("{action_name} failed for “{name}”: {why}")))
        })
        .and_then(Task::done)
    }

    pub fn delete_hold_progress(&self) -> Option<f32> {
        self.delete_hold_started
            .map(|started| (started.elapsed().as_secs_f32() / HOLD_TO_DELETE.as_secs_f32()).min(1.0))
    }

    fn remove_collected(&mut self, removal: Removal) -> Task<Message> {
        let Some(plan) = self.start_removal(removal) else { return Task::none() };
        Task::perform(off_thread(move || Arc::new(plan.carry_out())), |outcome| Message::RemovalDone { outcome })
    }

    fn start_removal(&mut self, removal: Removal) -> Option<RemovalPlan> {
        if self.removal.is_some() {
            return None;
        }
        self.removal = Some(RemovalProgress::OnDisk);
        Some(RemovalPlan::new(&self.tree, self.collector.items(), removal))
    }

    fn removal_is_on_disk(&self) -> bool {
        matches!(self.removal, Some(RemovalProgress::OnDisk))
    }

    fn removal_left_the_disk(&mut self, outcome: Result<Arc<RemovalReport>, PanicMessage>) {
        self.removal = Some(RemovalProgress::Finished(outcome));
    }

    fn apply_finished_removal(&mut self) -> Task<Message> {
        match self.removal.take() {
            Some(RemovalProgress::Finished(Ok(report))) => self.apply_removal_report(report),
            Some(RemovalProgress::Finished(Err(panic))) => {
                self.show_notice(NoticeKind::Error, format!("Removing failed unexpectedly: {panic}"));
                Task::none()
            }
            still_on_disk => {
                self.removal = still_on_disk;
                Task::none()
            }
        }
    }

    fn apply_removal_report(&mut self, report: Arc<RemovalReport>) -> Task<Message> {
        self.search_cancel.cancel();
        let Some(tree) = Arc::get_mut(&mut self.tree) else {
            self.removal = Some(RemovalProgress::Finished(Ok(report)));
            return Task::none();
        };
        report.apply_to(tree);
        self.collector.prune(&self.tree);
        let items = items_phrase(report.removed());
        let mut msg = match report.removal {
            Removal::MoveToTrash => format!(
                "Moved {items} ({}) to the {TRASH_NAME}. Empty the {TRASH_NAME} to free the space.",
                format_size(report.bytes)
            ),
            Removal::DeletePermanently => {
                format!("Deleted {items} permanently, freeing {}.", format_size(report.bytes))
            }
        };
        let kind = match report.failed.first() {
            None => NoticeKind::Done,
            Some((_, err)) => {
                msg.push_str(&format!(" {} failed: {err}", report.failed.len()));
                if report.removed() == 0 { NoticeKind::Error } else { NoticeKind::Warning }
            }
        };
        self.show_notice(kind, msg);
        self.focus = self.tree.nearest_live(self.focus);
        self.selected = self.selected.filter(|&s| self.tree.is_live(s));
        self.search_gen += 1;
        let search = self.restart_search();
        self.collector_changed();
        search
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        self.updating(|b| b.handle(message))
    }

    fn let_go_of_the_pointer(&mut self) {
        self.drag = None;
        self.delete_hold_started = None;
        self.over_collector = false;
        self.notice_hovered = false;
        self.hovered = None;
        self.highlight_cache.clear();
    }

    fn park(&mut self) {
        self.let_go_of_the_pointer();
        self.chart_cache.clear();
        self.animation = None;
        if self.search_status == SearchStatus::Running {
            self.search_cancel.cancel();
            self.search_gen += 1;
            self.search_status = SearchStatus::Due;
        }
    }

    fn note_while_parked(&mut self, message: Message) {
        match message {
            Message::RemovalDone { outcome } if self.removal_is_on_disk() => self.removal_left_the_disk(outcome),
            Message::DesktopActionFailed(why) => self.show_notice(NoticeKind::Error, why),
            _ => {}
        }
    }

    fn resume(&mut self) -> Task<Message> {
        self.updating(|b| {
            if b.notice.is_some() {
                b.notice_expires_at = Some(Instant::now() + NOTICE_LIFETIME);
            }
            let removal_before_the_search_holds_the_tree = b.apply_finished_removal();
            let search = if b.search_status == SearchStatus::Due { b.run_search() } else { Task::none() };
            Task::batch([removal_before_the_search_holds_the_tree, search])
        })
    }

    fn updating(&mut self, change: impl FnOnce(&mut Self) -> Task<Message>) -> Task<Message> {
        let before = (self.hovered, self.selected);
        let deadline_before = self.notice_expires_at;
        let task = change(self);
        let task = Task::batch([task, self.apply_finished_removal()]);
        if (self.hovered, self.selected) != before {
            self.highlight_cache.clear();
        }
        self.sync_list();
        let task = match self.notice_expires_at {
            Some(deadline) if Some(deadline) != deadline_before => {
                Task::batch([task, Self::check_the_notice_again_once_its_lifetime_is_up()])
            }
            _ => task,
        };
        let id = self.id;
        task.map(move |message| Message::ForBrowser(id, Box::new(message)))
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
                self.search_status = SearchStatus::Due;
                let generation = self.search_gen;
                return Task::perform(async { tokio::time::sleep(SEARCH_TYPING_PAUSE).await }, move |_| {
                    Message::SearchFire { generation }
                });
            }
            Message::SearchFire { generation } if generation == self.search_gen => return self.run_search(),
            Message::SearchDone { generation, result } => {
                if generation == self.search_gen {
                    self.search = result;
                    self.search_status = SearchStatus::Shown;
                    self.relayout();
                }
            }
            Message::RemovalDone { outcome } if self.removal_is_on_disk() => self.removal_left_the_disk(outcome),
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
                        } else if self.collected_list_open {
                            self.collected_list_open = false;
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
            Message::ToggleCollectedList => {
                self.collected_list_open = !self.collected_list_open && !self.collector.is_empty();
            }
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
            Message::DismissNotice => self.dismiss_notice(),
            Message::NoticeExpired if self.notice_expires_at.is_some_and(|deadline| Instant::now() >= deadline) => {
                self.dismiss_notice()
            }
            Message::NoticeEnter => {
                self.notice_hovered = true;
                self.notice_expires_at = None;
            }
            Message::NoticeExit => {
                self.notice_hovered = false;
                if self.notice.is_some() {
                    self.notice_expires_at = Some(Instant::now() + NOTICE_LIFETIME);
                }
            }
            Message::Preview => return self.hand_selection_to_desktop(PREVIEW_ACTION, desktop::preview),
            Message::Reveal => return self.hand_selection_to_desktop(REVEAL_ACTION, desktop::reveal),
            Message::DesktopActionFailed(why) => self.show_notice(NoticeKind::Error, why),
            _ => {}
        }
        Task::none()
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::*;

    pub fn scan_result(opts: &ScanOptions) -> Result<Tree, ScanError> {
        dscan_core::scan(opts, &Progress::default(), &CancelToken::default())
    }

    pub fn scanned(opts: ScanOptions) -> Browser {
        Browser::new(scan_result(&opts).unwrap(), opts)
    }

    pub fn descend(tree: &Tree, names: &[&str]) -> NodeId {
        names.iter().fold(tree.root(), |at, name| tree.children(at).find(|&c| tree.name(c) == *name).expect(name))
    }

    pub fn app_showing(b: Browser) -> App {
        let (mut app, _) = App::new(None);
        app.screen = Screen::Browse(Box::new(b));
        app
    }

    pub fn on_screen(app: &App) -> &Browser {
        match &app.screen {
            Screen::Browse(b) => b,
            Screen::Start => panic!("a result is on screen"),
        }
    }

    pub fn on_screen_mut(app: &mut App) -> &mut Browser {
        match &mut app.screen {
            Screen::Browse(b) => b,
            Screen::Start => panic!("a result is on screen"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::{app_showing, descend, on_screen, on_screen_mut, scanned};
    use super::*;
    use crate::inspect_tests::write_files;

    fn browser(dir: &std::path::Path) -> Browser {
        write_files(dir, &[("Movies/a.mov", 3000), ("Movies/b.mov", 2000), ("Music/c.mp3", 1000)], 1);
        scanned(ScanOptions::folder(dir))
    }

    fn remove_focused_folder_as_trashing_does(b: &mut Browser) {
        Arc::get_mut(&mut b.tree).unwrap().remove(b.focus);
        b.focus = b.tree.nearest_live(b.focus);
    }

    fn carry_out_as_the_background_task_does(b: &mut Browser, plan: RemovalPlan) {
        let _ = b.update(Message::RemovalDone { outcome: Ok(Arc::new(plan.carry_out())) });
    }

    #[test]
    fn back_skips_history_made_stale_by_removal() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let root = b.tree.root();
        let movies = descend(&b.tree, &["Movies"]);
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
    fn holding_delete_for_three_seconds_starts_deleting_the_collected_items_permanently() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let movies = descend(&b.tree, &["Movies"]);
        b.collector.add(&b.tree, movies).unwrap();

        let _ = b.update(Message::DeleteHoldStart);
        pretend_held_for(&mut b, Duration::from_secs(1));
        let _ = b.update(Message::DeleteHoldTick);
        assert!(dir.path().join("Movies").exists(), "nothing is deleted before three seconds");
        let progress = b.delete_hold_progress().unwrap();
        assert!((0.3..0.4).contains(&progress), "one of three seconds held: {progress}");

        pretend_held_for(&mut b, HOLD_TO_DELETE);
        let deleting = b.update(Message::DeleteHoldTick);
        assert!(deleting.units() > 0 && b.delete_hold_started.is_none(), "three seconds start the deletion");
        assert!(dir.path().join("Movies").exists(), "the disk work runs off the UI thread");
        assert!(b.start_removal(Removal::DeletePermanently).is_none(), "one removal at a time");
    }

    #[test]
    fn a_finished_removal_updates_the_tree_and_says_what_was_freed() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let movies = descend(&b.tree, &["Movies"]);
        b.collector.add(&b.tree, movies).unwrap();

        let removal = b.start_removal(Removal::DeletePermanently).unwrap();
        carry_out_as_the_background_task_does(&mut b, removal);

        assert!(!dir.path().join("Movies").exists());
        assert!(!b.tree.is_live(movies) && b.collector.is_empty());
        assert!(
            b.notice
                .as_ref()
                .is_some_and(|n| n.kind == NoticeKind::Done && n.text.starts_with("Deleted 1 item permanently")),
            "{:?}",
            b.notice
        );
    }

    #[test]
    fn releasing_or_leaving_the_button_early_cancels_the_delete() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let movies = descend(&b.tree, &["Movies"]);
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
    fn a_removal_removes_only_what_was_collected_when_it_started_and_waits_for_a_running_search() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let (movies, music) = (descend(&b.tree, &["Movies"]), descend(&b.tree, &["Music"]));
        b.collector.add(&b.tree, movies).unwrap();

        let removal = b.start_removal(Removal::DeletePermanently).unwrap();
        let _ = b.update(Message::Press(Segment::Node(music)));
        let _ = b.update(Message::CollectSelected);
        let running_search = Arc::clone(&b.tree);
        carry_out_as_the_background_task_does(&mut b, removal);

        assert!(dir.path().join("Music").exists(), "later additions are not removed");
        assert!(b.tree.is_live(movies), "the tree changes only once the search lets go of it");
        drop(running_search);
        let _ = b.update(Message::SearchDone { generation: b.search_gen, result: None });
        assert!(!b.tree.is_live(movies));
        assert_eq!(b.collector.items(), &[music]);
    }

    fn to(b: BrowserId, message: Message) -> Message {
        Message::ForBrowser(b, Box::new(message))
    }

    #[test]
    fn a_removal_finishing_after_going_home_is_applied_on_returning_to_its_result() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let movies = descend(&b.tree, &["Movies"]);
        b.collector.add(&b.tree, movies).unwrap();
        let plan = b.start_removal(Removal::DeletePermanently).unwrap();
        let b_id = b.id;
        let mut app = app_showing(b);

        let _ = app.update(Message::GoHome);
        let _ = app.update(to(b_id, Message::RemovalDone { outcome: Ok(Arc::new(plan.carry_out())) }));
        let _ = app.update(to(b_id, Message::SearchDone { generation: 0, result: None }));
        let parked = &app.parked_newest_first[0];
        assert!(parked.tree.is_live(movies) && parked.notice.is_none(), "nothing changes while it is parked");
        let _ = resume_newest_parked(&mut app);

        let b = on_screen(&app);
        assert!(!b.tree.is_live(movies) && b.collector.is_empty());
        assert!(b.notice.as_ref().is_some_and(|n| n.kind == NoticeKind::Done), "{:?}", b.notice);
    }

    fn resume_newest_parked(app: &mut App) -> Task<Message> {
        app.update(Message::ResumeParked(app.parked_newest_first[0].id))
    }

    #[test]
    fn going_home_mid_hold_cancels_the_permanent_delete() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let movies = descend(&b.tree, &["Movies"]);
        b.collector.add(&b.tree, movies).unwrap();
        let _ = b.update(Message::DeleteHoldStart);
        let mut app = app_showing(b);

        let _ = app.update(Message::GoHome);
        let _ = resume_newest_parked(&mut app);
        let b = on_screen_mut(&mut app);
        assert!(b.delete_hold_progress().is_none(), "the hold ended when its button left the screen");
        b.delete_hold_started.get_or_insert(Instant::now() - HOLD_TO_DELETE);
        let _ = app.update(Message::DeleteHoldTick);
        assert!(dir.path().join("Movies").exists(), "nothing is deleted without a new, finished hold");
    }

    fn park_and_show(app: &mut App, other: Browser) {
        let _ = app.update(Message::GoHome);
        app.screen = Screen::Browse(Box::new(other));
    }

    #[test]
    fn a_search_typed_just_before_going_home_runs_on_return_and_never_in_another_result() {
        let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let mut a = browser(dir_a.path());
        let _ = a.update(Message::SearchChanged("mov".into()));
        let (a_id, fire) = (a.id, Message::SearchFire { generation: a.search_gen });
        let mut other = browser(dir_b.path());
        other.search_gen = a.search_gen;
        let mut app = app_showing(a);
        park_and_show(&mut app, other);

        assert_eq!(app.update(to(a_id, fire)).units(), 0, "a parked result starts no search");
        assert_eq!(on_screen(&app).search_status, SearchStatus::Shown, "nor does the result on screen");
        let _ = app.update(Message::GoHome);
        let searching = app.update(Message::ResumeParked(a_id));
        assert!(searching.units() > 0, "returning starts the search that was due");
        let a = on_screen(&app);
        let result = dscan_core::search(&a.tree, "mov", &CancelToken::default()).map(Arc::new);
        let _ = app.update(to(a_id, Message::SearchDone { generation: a.search_gen, result }));

        assert!(on_screen(&app).search.is_some(), "the result shows its own search");
        assert!(app.parked_newest_first[0].search.is_none(), "the other result never takes it");
    }

    #[test]
    fn going_home_cancels_a_running_search_and_returning_runs_it_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let _ = b.update(Message::SearchChanged("mov".into()));
        let _ = b.update(Message::SearchFire { generation: b.search_gen });
        let (running, stale) = (b.search_cancel.clone(), b.search_gen);
        let mut app = app_showing(b);

        let _ = app.update(Message::GoHome);
        assert!(running.is_cancelled(), "a parked result lets go of its search");
        let parked = &app.parked_newest_first[0];
        let late = dscan_core::search(&parked.tree, "mov", &CancelToken::default()).map(Arc::new);
        let _ = app.update(to(parked.id, Message::SearchDone { generation: stale, result: late }));
        assert!(app.parked_newest_first[0].search.is_none(), "the cancelled search's answer is dropped");

        assert!(resume_newest_parked(&mut app).units() > 0, "returning searches again");
        assert_eq!(on_screen(&app).search_status, SearchStatus::Running);
    }

    #[test]
    fn closing_a_result_cancels_its_search() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let _ = b.update(Message::SearchChanged("mov".into()));
        let _ = b.update(Message::SearchFire { generation: b.search_gen });
        let running = b.search_cancel.clone();
        drop(b);
        assert!(running.is_cancelled(), "a closed result's tree is not kept alive by its search");
    }

    #[test]
    fn a_desktop_action_failing_after_going_home_is_told_in_its_own_result() {
        let (dir_a, dir_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (a, other) = (browser(dir_a.path()), browser(dir_b.path()));
        let a_id = a.id;
        let mut app = app_showing(a);
        park_and_show(&mut app, other);

        let failed = Message::DesktopActionFailed("Reveal failed".into());
        assert_eq!(app.update(to(a_id, failed)).units(), 0, "a parked result starts no notice timer");
        assert!(on_screen(&app).notice.is_none(), "the result on screen is not told");
        assert!(app.parked_newest_first[0].notice.is_some(), "the result that asked is");
    }

    #[test]
    fn every_task_a_result_starts_answers_to_that_result() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let typing_pause = b.update(Message::SearchChanged("mov".into()));
        let answers = outputs(typing_pause);
        assert!(
            matches!(answers.as_slice(), [Message::ForBrowser(to, fire)]
                if *to == b.id && matches!(**fire, Message::SearchFire { .. })),
            "{answers:?}"
        );
    }

    fn outputs(task: Task<Message>) -> Vec<Message> {
        use iced_test::futures::futures::{StreamExt, future};
        use iced_test::runtime::{Action, task};
        let Some(actions) = task::into_stream(task) else { return Vec::new() };
        let only_outputs = actions.filter_map(|action| {
            future::ready(match action {
                Action::Output(message) => Some(message),
                _ => None,
            })
        });
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        runtime.block_on(only_outputs.collect())
    }

    #[test]
    fn a_notice_left_showing_when_going_home_gets_a_full_lifetime_on_return() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let _ = b.update(Message::DesktopActionFailed("shown".into()));
        let _ = b.update(Message::NoticeEnter);
        let mut app = app_showing(b);

        let _ = app.update(Message::GoHome);
        let timer = resume_newest_parked(&mut app);

        assert!(timer.units() > 0, "returning restarts the notice's timer");
        let b = on_screen(&app);
        assert!(
            b.notice_expires_at.is_some_and(|deadline| deadline > Instant::now()),
            "and it is not hovered any more"
        );
    }

    #[test]
    fn forward_is_offered_only_while_its_target_still_exists() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let movies = descend(&b.tree, &["Movies"]);
        let _ = b.update(Message::Navigate(movies));
        let _ = b.update(Message::Back);
        assert!(b.can_forward());
        Arc::get_mut(&mut b.tree).unwrap().remove(movies);
        assert!(!b.can_forward(), "a removed folder is no forward target");
    }

    #[test]
    fn mouse_side_buttons_walk_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let root = b.tree.root();
        let movies = descend(&b.tree, &["Movies"]);
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
    fn a_failed_desktop_action_is_reported_in_a_notice() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let failure = "Quick Look failed for “a.mov”: not found".to_string();
        let _ = b.update(Message::DesktopActionFailed(failure.clone()));
        assert_eq!(b.notice, Some(Notice { kind: NoticeKind::Error, text: failure }), "a failure shows as an error");
    }

    #[test]
    fn a_desktop_action_answers_only_when_it_fails() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        b.selected = Some(descend(&b.tree, &["Movies", "a.mov"]));
        let worked = b.hand_selection_to_desktop(PREVIEW_ACTION, |_| Ok(()));
        assert!(outputs(worked).is_empty(), "success needs no message");
        let failed = b.hand_selection_to_desktop(PREVIEW_ACTION, |_| Err(DesktopError::WouldRunAProgram));
        assert!(matches!(outputs(failed).as_slice(), [Message::DesktopActionFailed(why)] if why.contains("a.mov")));
    }

    fn pretend_the_notice_lifetime_passed(b: &mut Browser) {
        if let Some(deadline) = &mut b.notice_expires_at {
            *deadline = Instant::now() - Duration::from_millis(1);
        }
    }

    #[test]
    fn a_notice_disappears_after_its_lifetime_but_not_while_hovered() {
        let dir = tempfile::tempdir().unwrap();
        let mut b = browser(dir.path());
        let failed = |why: &str| Message::DesktopActionFailed(why.to_string());

        let timer = b.update(failed("first"));
        assert!(timer.units() > 0, "showing a notice starts its timer");
        let _ = b.update(Message::NoticeExpired);
        assert!(b.notice.is_some(), "a notice stays for its whole lifetime");

        let _ = b.update(Message::NoticeEnter);
        pretend_the_notice_lifetime_passed(&mut b);
        let _ = b.update(Message::NoticeExpired);
        assert!(b.notice.is_some(), "a hovered notice stays");
        let _ = b.update(failed("second"));
        pretend_the_notice_lifetime_passed(&mut b);
        let _ = b.update(Message::NoticeExpired);
        assert_eq!(
            b.notice.as_ref().map(|n| n.text.as_str()),
            Some("second"),
            "a notice replaced under the mouse stays"
        );

        let restarted = b.update(Message::NoticeExit);
        assert!(restarted.units() > 0, "leaving the notice starts its lifetime again");
        let _ = b.update(Message::NoticeExpired);
        assert!(b.notice.is_some(), "after the mouse leaves, the notice gets a full lifetime again");
        pretend_the_notice_lifetime_passed(&mut b);
        let _ = b.update(Message::NoticeExpired);
        assert_eq!(b.notice, None, "the notice goes once its time is up");
        assert_eq!(b.update(Message::NoticeExit).units(), 0, "no notice, no timer");
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
