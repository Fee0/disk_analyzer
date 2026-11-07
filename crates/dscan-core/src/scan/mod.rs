use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use rayon::prelude::*;
use rustc_hash::FxHashSet;

mod shared_blocks;

use crate::hidden;
use crate::kind::Kind;
use crate::platform::{Current, DeviceId, DirEntry, EntryType, Platform, ScanBoundaries, ScanExtent};
use crate::tree::{self, DirNode, ScanStats, Synthetic, Tree, flags};
use shared_blocks::SharedBlockLedger;

#[derive(Clone, Debug)]
pub struct ScanOptions {
    pub root: PathBuf,
    pub whole_disk: bool,
    pub root_name: Option<String>,
    pub threads: Option<usize>,
}

impl ScanOptions {
    pub fn folder(root: impl Into<PathBuf>) -> Self {
        ScanOptions { root: root.into(), whole_disk: false, root_name: None, threads: None }
    }
    pub fn volume(mount: impl Into<PathBuf>, name: impl Into<String>) -> Self {
        ScanOptions { root: mount.into(), whole_disk: true, root_name: Some(name.into()), threads: None }
    }

    pub fn display_name(&self) -> String {
        self.root_name.clone().unwrap_or_else(|| match self.root.file_name() {
            Some(n) => n.to_string_lossy().into_owned(),
            None => self.root.display().to_string(),
        })
    }
}

#[derive(Debug, Default)]
pub struct Progress {
    files: AtomicU64,
    dirs: AtomicU64,
    bytes: AtomicU64,
    expected_bytes_or_zero: AtomicU64,
}

#[derive(Clone, Debug, Default)]
pub struct ProgressSnapshot {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub expected: Option<u64>,
}

impl Progress {
    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            files: self.files.load(Relaxed),
            dirs: self.dirs.load(Relaxed),
            bytes: self.bytes.load(Relaxed),
            expected: Some(self.expected_bytes_or_zero.load(Relaxed)).filter(|&bytes| bytes > 0),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<Control>);

#[derive(Debug, Default)]
struct Control {
    cancelled: AtomicBool,
    paused: AtomicBool,
    clock: Mutex<PauseClock>,
    wake: Condvar,
}

#[derive(Debug, Default)]
struct PauseClock {
    since: Option<Instant>,
    total: Duration,
}

impl CancelToken {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Relaxed);
        let _clock = self.0.clock.lock();
        self.0.wake.notify_all();
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Relaxed)
    }

    pub fn set_paused(&self, paused: bool) {
        let mut clock = self.0.clock.lock();
        if self.0.paused.swap(paused, Relaxed) == paused {
            return;
        }
        if paused {
            clock.since = Some(Instant::now());
        } else {
            let paused = clock.since.take().map_or(Duration::ZERO, |t| t.elapsed());
            clock.total += paused;
            self.0.wake.notify_all();
        }
    }
    pub fn is_paused(&self) -> bool {
        self.0.paused.load(Relaxed)
    }

    pub fn paused_for(&self) -> Duration {
        let clock = self.0.clock.lock();
        clock.total + clock.since.map_or(Duration::ZERO, |t| t.elapsed())
    }

    fn wait_while_paused(&self) {
        if !self.is_paused() {
            return;
        }
        let mut clock = self.0.clock.lock();
        while self.is_paused() && !self.is_cancelled() {
            self.0.wake.wait(&mut clock);
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("scan cancelled")]
    Cancelled,
    #[error("cannot read {path}: {source}")]
    Root { path: PathBuf, source: std::io::Error },
}

const SEEN_SHARDS: usize = 64;

struct ShardedSeenSet(Vec<Mutex<FxHashSet<(DeviceId, u64)>>>);

impl ShardedSeenSet {
    fn new() -> Self {
        ShardedSeenSet((0..SEEN_SHARDS).map(|_| Mutex::new(FxHashSet::default())).collect())
    }
    fn first_sight(&self, device: DeviceId, id: u64) -> bool {
        self.0[(id as usize) % SEEN_SHARDS].lock().insert((device, id))
    }
}

enum Duplicate {
    HardLink,
    PureClone,
}
const MAX_ERROR_SAMPLES: usize = 100;

struct Ctx<'a> {
    boundaries: ScanBoundaries,
    system_top_level_dirs: FxHashSet<&'static [u8]>,
    hard_linked_inodes: ShardedSeenSet,
    shared_data_streams: ShardedSeenSet,
    shared_blocks: SharedBlockLedger,
    errors: AtomicU64,
    error_samples: Mutex<Vec<PathBuf>>,
    progress: &'a Progress,
    cancel: &'a CancelToken,
}

impl Ctx<'_> {
    fn already_counted(&self, e: &DirEntry<'_>) -> Option<Duplicate> {
        if let Some(id) = e.hard_link_id
            && !self.hard_linked_inodes.first_sight(e.device, id)
        {
            return Some(Duplicate::HardLink);
        }
        match e.shared_data_stream_id {
            Some(stream) if !self.shared_data_streams.first_sight(e.device, stream) => Some(Duplicate::PureClone),
            _ => None,
        }
    }

    fn counted_allocation_and_flags(&self, e: &DirEntry<'_>) -> (u64, u8) {
        match self.already_counted(e) {
            Some(Duplicate::HardLink) => return (0, flags::HARDLINK_DUP),
            Some(Duplicate::PureClone) => return (0, flags::CLONE_DUP),
            None => {}
        }
        let counted_before = self.shared_blocks.claim_returning_bytes_counted_before(e.shared_blocks);
        let not_counted_yet = e.allocated.saturating_sub(counted_before);
        match (counted_before, not_counted_yet) {
            (0, _) => (e.allocated, 0),
            (_, 0) => (0, flags::CLONE_DUP),
            _ => (not_counted_yet, flags::SHARES_COUNTED_BLOCKS),
        }
    }

    fn may_enter_device(&self, parent: DeviceId, child: DeviceId) -> bool {
        child == parent || self.boundaries.other_devices_of_the_disk.contains(&child)
    }

    fn is_duplicate_view(&self, path: &[u8]) -> bool {
        self.boundaries.duplicate_views.contains(path)
    }

    fn device_inside(&self, path: &[u8], as_listed: DeviceId) -> DeviceId {
        self.boundaries.devices_mounted_on.get(path).copied().unwrap_or(as_listed)
    }

    fn record_error(&self, path: &[u8]) {
        self.errors.fetch_add(1, Relaxed);
        let mut s = self.error_samples.lock();
        if s.len() < MAX_ERROR_SAMPLES {
            s.push(PathBuf::from(Current::os_str_from_bytes(path).into_owned()));
        }
    }
}

const IO_BOUND_THREADS_PER_CORE: usize = 4;
const FALLBACK_CORE_COUNT: usize = 8;
const MIN_SCAN_THREADS: usize = 16;
const MAX_SCAN_THREADS: usize = 64;
const SCAN_THREAD_STACK_BYTES: usize = 16 << 20;

fn default_scan_threads() -> usize {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(FALLBACK_CORE_COUNT);
    (cores * IO_BOUND_THREADS_PER_CORE).clamp(MIN_SCAN_THREADS, MAX_SCAN_THREADS)
}

fn canonical_root(root: &Path) -> Result<PathBuf, ScanError> {
    Current::canonical_root(root).map_err(|source| ScanError::Root { path: root.to_path_buf(), source })
}

fn ensure_readable(root: &Path) -> Result<(), ScanError> {
    std::fs::read_dir(root).map(drop).map_err(|source| ScanError::Root { path: root.to_path_buf(), source })
}

fn vanished_while_scanning(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound
}

pub fn scan(opts: &ScanOptions, progress: &Progress, cancel: &CancelToken) -> Result<Tree, ScanError> {
    let started = Instant::now();
    let root = canonical_root(&opts.root)?;
    let root_dev = Current::device_of(&root).map_err(|source| ScanError::Root { path: root.clone(), source })?;
    ensure_readable(&root)?;

    let space = opts.whole_disk.then(|| hidden::space_info(&root).ok()).flatten();
    if let Some(space) = &space {
        progress.expected_bytes_or_zero.store(space.used, Relaxed);
    }
    let extent = if opts.whole_disk { ScanExtent::WholeDisk } else { ScanExtent::Folder };
    let boundaries = Current::scan_boundaries(&root, extent);
    let system_top_level_dirs = if opts.whole_disk && Current::is_system_root(&root) {
        Current::SYSTEM_FOLDERS_AT_SYSTEM_ROOT.iter().map(|name| name.as_bytes()).collect()
    } else {
        FxHashSet::default()
    };

    let ctx = Ctx {
        boundaries,
        system_top_level_dirs,
        hard_linked_inodes: ShardedSeenSet::new(),
        shared_data_streams: ShardedSeenSet::new(),
        shared_blocks: SharedBlockLedger::new(),
        errors: AtomicU64::new(0),
        error_samples: Mutex::new(Vec::new()),
        progress,
        cancel,
    };

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.threads.unwrap_or_else(default_scan_threads))
        .stack_size(SCAN_THREAD_STACK_BYTES)
        .thread_name(|i| format!("dscan-{i}"))
        .build()
        .expect("failed to build scan thread pool");

    let root_bytes = Current::path_bytes(&root).into_owned();
    let root_node = pool.install(|| scan_dir(&ctx, root_bytes, Box::default(), root_dev, 0));
    if cancel.is_cancelled() {
        return Err(ScanError::Cancelled);
    }

    let mut stats = ScanStats {
        files: progress.files.load(Relaxed),
        dirs: progress.dirs.load(Relaxed),
        errors: ctx.errors.load(Relaxed),
        error_samples: std::mem::take(&mut *ctx.error_samples.lock()),
        duration: started.elapsed().saturating_sub(cancel.paused_for()),
        hidden: 0,
        purgeable: 0,
    };

    let mut synthetic = Vec::new();
    if let Some(space) = space {
        let (hidden, purgeable) = space.split_unscanned(root_node.alloc);
        stats.hidden = hidden;
        stats.purgeable = purgeable;
        if purgeable > 0 {
            synthetic.push(Synthetic { name: "Purgeable space", kind: Kind::Purgeable, size: purgeable });
        }
        if hidden > 0 {
            synthetic.push(Synthetic { name: "Hidden space", kind: Kind::Hidden, size: hidden });
        }
    }
    Ok(tree::build(root_node, root, &opts.display_name(), opts.whole_disk, synthetic, stats))
}

fn join(parent: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(parent.len() + 1 + name.len());
    p.extend_from_slice(parent);
    if p.last() != Some(&Current::PATH_SEPARATOR) {
        p.push(Current::PATH_SEPARATOR);
    }
    p.extend_from_slice(name);
    p
}

fn scan_dir(ctx: &Ctx<'_>, path: Vec<u8>, name: Box<[u8]>, dev: DeviceId, depth: u32) -> DirNode {
    let forced = if depth == 1 && ctx.system_top_level_dirs.contains(&name[..]) {
        Some(Kind::System)
    } else {
        Kind::of_dir(&name)
    };
    let mut node = DirNode::new(name);
    ctx.cancel.wait_while_paused();
    if ctx.cancel.is_cancelled() {
        return node;
    }
    let path = &path[..];

    let mut subdirs: Vec<(Box<[u8]>, DeviceId)> = Vec::new();
    let mut file_bytes = 0u64;
    let res = Current::read_dir(path, |e| match e.entry_type {
        None => {
            ctx.record_error(&join(path, e.name));
            node.push_file(e.name, Kind::Other, flags::UNREADABLE, 0);
        }
        Some(EntryType::Directory) => subdirs.push((e.name.into(), e.device)),
        Some(EntryType::File | EntryType::Symlink) => {
            let (alloc, mut fl) = ctx.counted_allocation_and_flags(&e);
            let kind = if e.entry_type == Some(EntryType::Symlink) {
                fl |= flags::SYMLINK;
                Kind::Other
            } else {
                Kind::of_file(e.name)
            };
            file_bytes += alloc;
            node.push_file(e.name, kind, fl, alloc);
        }
    });
    match res {
        Ok(()) => {}
        Err(e) if vanished_while_scanning(&e) => {}
        Err(_) => {
            node.flags |= flags::UNREADABLE;
            ctx.record_error(path);
        }
    }

    let p = ctx.progress;
    p.files.fetch_add(node.files.len() as u64, Relaxed);
    p.bytes.fetch_add(file_bytes, Relaxed);
    p.dirs.fetch_add(1, Relaxed);

    let todo = subdirs_to_descend(ctx, path, dev, subdirs);
    node.dirs = if todo.len() <= 1 {
        todo.into_iter().map(|(cp, n, d)| scan_dir(ctx, cp, n, d, depth + 1)).collect()
    } else {
        todo.into_par_iter().map(|(cp, n, d)| scan_dir(ctx, cp, n, d, depth + 1)).collect()
    };
    node.finish(forced);
    node
}

fn subdirs_to_descend(
    ctx: &Ctx<'_>,
    path: &[u8],
    dev: DeviceId,
    subdirs: Vec<(Box<[u8]>, DeviceId)>,
) -> Vec<(Vec<u8>, Box<[u8]>, DeviceId)> {
    subdirs
        .into_iter()
        .map(|(child, listed_dev)| {
            let child_path = join(path, &child);
            let child_dev = ctx.device_inside(&child_path, listed_dev);
            (child_path, child, child_dev)
        })
        .filter(|&(_, _, child_dev)| ctx.may_enter_device(dev, child_dev))
        .filter(|(child_path, _, _)| !ctx.is_duplicate_view(child_path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(t: &Tree, path: &str) -> crate::NodeId {
        let mut cur = t.root();
        for part in path.split('/') {
            cur = t.children(cur).find(|&c| t.name(c) == part).unwrap_or_else(|| panic!("{part} missing"));
        }
        cur
    }

    fn scan_folder(root: &Path) -> Tree {
        scan(&ScanOptions::folder(root), &Progress::default(), &CancelToken::default()).unwrap()
    }

    #[test]
    fn scans_nested_folders_and_counts_a_hard_linked_file_once() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("a/b")).unwrap();
        std::fs::write(r.join("a/one.bin"), vec![7u8; 100_000]).unwrap();
        std::fs::write(r.join("a/b/two.bin"), vec![7u8; 50_000]).unwrap();
        std::fs::hard_link(r.join("a/one.bin"), r.join("link.bin")).unwrap();

        let progress = Progress::default();
        let t = scan(&ScanOptions::folder(r), &progress, &CancelToken::default()).unwrap();

        let names = [find(&t, "a/one.bin"), find(&t, "link.bin")];
        let sizes = names.map(|id| t.size(id));
        assert!(sizes.contains(&0) && sizes.iter().any(|&s| s >= 100_000), "exactly one hard link carries the size");
        let marked_as_copies = names.iter().filter(|&&id| t.node(id).flags() & flags::HARDLINK_DUP != 0).count();
        assert_eq!(marked_as_copies, 1, "the name met second is marked as the copy");
        assert_eq!(scan_folder(r).size(t.root()), t.size(t.root()));
        let a = find(&t, "a");
        let sum: u64 = t.children(a).map(|c| t.size(c)).sum();
        assert_eq!(sum, t.size(a), "a folder's size is the sum of its children");
        assert!(t.size(find(&t, "a/b/two.bin")) >= 50_000, "a file takes at least its length on disk");
        let root_as_the_platform_spells_it = Current::canonical_root(r).unwrap();
        assert_eq!(t.path(find(&t, "a/b/two.bin")), root_as_the_platform_spells_it.join("a").join("b").join("two.bin"));
        assert_eq!(t.stats.errors, 0);
        assert_eq!(progress.snapshot().files, t.node(t.root()).items() as u64);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed_sparse_files_count_their_allocation_and_unreadable_folders_are_flagged() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir(r.join("a")).unwrap();
        std::fs::write(r.join("a/one.bin"), vec![7u8; 100_000]).unwrap();
        std::os::unix::fs::symlink(r.join("a"), r.join("sym")).unwrap();
        std::fs::File::create(r.join("sparse.img")).unwrap().set_len(1 << 30).unwrap();
        std::fs::create_dir(r.join("locked")).unwrap();
        std::fs::set_permissions(r.join("locked"), std::fs::Permissions::from_mode(0o000)).unwrap();

        let t = scan_folder(r);
        std::fs::set_permissions(r.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(!t.node(find(&t, "sym")).is_dir());
        let sparse = find(&t, "sparse.img");
        assert!(t.size(sparse) < 1 << 20, "a sparse file allocates little");
        assert_eq!(t.stats.errors, 1, "run the tests as a normal user: root can read a mode 000 folder");
        assert!(t.node(find(&t, "locked")).flags() & flags::UNREADABLE != 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn counts_pure_clones_once() {
        use std::ffi::CString;
        use std::io::{Seek, Write};
        use std::os::unix::ffi::OsStrExt;
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        let c = |p: &str| CString::new(r.join(p).as_os_str().as_bytes()).unwrap();
        std::fs::create_dir(r.join("copies")).unwrap();
        std::fs::write(r.join("orig.bin"), vec![3u8; 1_000_000]).unwrap();
        for p in ["copies/pure.bin", "copies/edited.bin"] {
            assert_eq!(unsafe { libc::clonefile(c("orig.bin").as_ptr(), c(p).as_ptr(), 0) }, 0);
        }
        let mut fh = std::fs::OpenOptions::new().write(true).open(r.join("copies/edited.bin")).unwrap();
        fh.seek(std::io::SeekFrom::Start(0)).unwrap();
        fh.write_all(&[9u8; 4096]).unwrap();
        fh.sync_all().unwrap();
        drop(fh);

        let t = scan_folder(r);
        let (orig, pure, edited) = (find(&t, "orig.bin"), find(&t, "copies/pure.bin"), find(&t, "copies/edited.bin"));
        let alloc = |id| t.size(id);
        assert!(alloc(orig) >= 1_000_000 || alloc(pure) >= 1_000_000);
        assert_eq!(alloc(orig).min(alloc(pure)), 0, "pure clone counted once");
        assert!(t.node(orig).is_clone_copy() ^ t.node(pure).is_clone_copy());
        assert!(alloc(edited) >= 1_000_000, "edited clones count in full");
        let total = t.size(t.root());
        assert!((2_000_000..2_100_000).contains(&total), "two streams on disk, not three: {total}");
    }

    #[cfg(target_os = "linux")]
    fn reflink(from: &Path, to: &Path) -> bool {
        use std::os::fd::AsRawFd;
        const FICLONE: u32 = 0x4004_9409;
        let source = std::fs::File::open(from).unwrap();
        let clone = std::fs::File::create(to).unwrap();
        let cloned = unsafe { libc::ioctl(clone.as_raw_fd(), FICLONE as _, source.as_raw_fd()) } == 0;
        if !cloned {
            std::fs::remove_file(to).unwrap();
            std::fs::copy(from, to).unwrap();
        }
        cloned
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reflinked_copies_count_shared_blocks_once_where_the_file_system_shares_blocks_and_in_full_elsewhere() {
        use std::io::{Seek, Write};
        const LENGTH: u64 = 1 << 20;
        const EDITED: u64 = 16 << 10;
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir(r.join("copies")).unwrap();
        let data: Vec<u8> = (0..LENGTH).map(|i| (i * 31 % 251) as u8).collect();
        std::fs::write(r.join("orig.bin"), &data).unwrap();
        let shares_blocks = reflink(&r.join("orig.bin"), &r.join("copies/pure.bin"))
            & reflink(&r.join("orig.bin"), &r.join("copies/edited.bin"));
        let mut edited = std::fs::OpenOptions::new().write(true).open(r.join("copies/edited.bin")).unwrap();
        edited.seek(std::io::SeekFrom::Start(0)).unwrap();
        edited.write_all(&vec![9u8; EDITED as usize]).unwrap();
        edited.sync_all().unwrap();
        drop(edited);

        let t = scan_folder(r);
        let files = [find(&t, "orig.bin"), find(&t, "copies/pure.bin"), find(&t, "copies/edited.bin")];
        let total = t.size(t.root());
        if shares_blocks {
            println!("reflinks: the temp dir shares blocks");
            let one_copy_and_the_edit = LENGTH + EDITED..LENGTH + EDITED + LENGTH / 4;
            assert!(one_copy_and_the_edit.contains(&total), "one copy of the data plus the edited blocks: {total}");
            let marked =
                files.iter().filter(|&&id| t.node(id).is_clone_copy() || t.node(id).shares_blocks_counted_elsewhere());
            assert!(marked.count() >= 2, "the copies counted second are marked");
        } else {
            println!("reflinks: the temp dir can't share blocks, so every copy is its own");
            assert!(total >= 3 * LENGTH, "three full copies: {total}");
            assert!(
                files.iter().all(|&id| !t.node(id).is_clone_copy() && !t.node(id).shares_blocks_counted_elsewhere())
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn follows_symlinked_root() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir(r.join("real")).unwrap();
        std::fs::write(r.join("real/f.bin"), vec![1u8; 10_000]).unwrap();
        std::os::unix::fs::symlink(r.join("real"), r.join("link")).unwrap();
        let t = scan_folder(&r.join("link"));
        assert_eq!(t.stats.errors, 0);
        assert!(t.size(find(&t, "f.bin")) >= 10_000, "the target of the symlinked root is scanned");
    }

    #[test]
    fn pause_holds_the_scan_until_resumed() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..50 {
            let d = dir.path().join(format!("d{i}"));
            std::fs::create_dir(&d).unwrap();
            std::fs::write(d.join("f"), b"x").unwrap();
        }
        let (progress, ctl) = (Progress::default(), CancelToken::default());
        let held_for = Duration::from_millis(200);
        ctl.set_paused(true);
        let wall_clock = Instant::now();
        std::thread::scope(|s| {
            let scan = s.spawn(|| scan(&ScanOptions::folder(dir.path()), &progress, &ctl));
            std::thread::sleep(held_for);
            assert_eq!(progress.snapshot().dirs, 0, "nothing is read while paused");
            assert!(!scan.is_finished());
            ctl.set_paused(false);
            let t = scan.join().unwrap().unwrap();
            assert_eq!(t.stats.dirs, 51);
            assert!(t.stats.duration + held_for <= wall_clock.elapsed(), "paused time is not counted");
        });
    }

    #[test]
    fn cancel_wakes_a_paused_scan() {
        let dir = tempfile::tempdir().unwrap();
        let ctl = CancelToken::default();
        ctl.set_paused(true);
        std::thread::scope(|s| {
            let scan = s.spawn(|| scan(&ScanOptions::folder(dir.path()), &Progress::default(), &ctl));
            std::thread::sleep(Duration::from_millis(50));
            ctl.cancel();
            assert!(matches!(scan.join().unwrap(), Err(ScanError::Cancelled)));
        });
    }

    #[test]
    fn cancel_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let c = CancelToken::default();
        c.cancel();
        let r = scan(&ScanOptions::folder(dir.path()), &Progress::default(), &c);
        assert!(matches!(r, Err(ScanError::Cancelled)));
    }
}
