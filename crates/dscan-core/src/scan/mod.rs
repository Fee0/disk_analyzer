pub mod bulk;
pub mod volumes;

use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use rayon::prelude::*;
use rustc_hash::FxHashSet;

use crate::hidden;
use crate::kind::Kind;
use crate::tree::{self, DirNode, ScanStats, Synthetic, Tree, flags};

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

struct ShardedSeenSet(Vec<Mutex<FxHashSet<(i32, u64)>>>);

impl ShardedSeenSet {
    fn new() -> Self {
        ShardedSeenSet((0..SEEN_SHARDS).map(|_| Mutex::new(FxHashSet::default())).collect())
    }
    fn first_sight(&self, dev: i32, id: u64) -> bool {
        self.0[(id as usize) % SEEN_SHARDS].lock().insert((dev, id))
    }
}

enum Duplicate {
    HardLink,
    PureClone,
}
const MAX_ERROR_SAMPLES: usize = 100;

struct Ctx<'a> {
    sibling_volume_devices: FxHashSet<i32>,
    firmlinked_duplicate_paths: FxHashSet<Vec<u8>>,
    system_top_level_dirs: FxHashSet<&'static [u8]>,
    hard_linked_inodes: ShardedSeenSet,
    shared_data_streams: ShardedSeenSet,
    errors: AtomicU64,
    error_samples: Mutex<Vec<PathBuf>>,
    progress: &'a Progress,
    cancel: &'a CancelToken,
}

impl Ctx<'_> {
    fn already_counted(&self, e: &bulk::RawEntry<'_>) -> Option<Duplicate> {
        if e.objtype != bulk::VREG {
            return None;
        }
        if e.links > 1 && !self.hard_linked_inodes.first_sight(e.dev, e.ino) {
            return Some(Duplicate::HardLink);
        }
        match e.data_stream {
            Some(stream) if e.may_share_blocks && !self.shared_data_streams.first_sight(e.dev, stream) => {
                Some(Duplicate::PureClone)
            }
            _ => None,
        }
    }

    fn may_enter_device(&self, parent_dev: i32, child_dev: i32) -> bool {
        child_dev == parent_dev || self.sibling_volume_devices.contains(&child_dev)
    }

    fn is_firmlinked_duplicate(&self, path: &[u8]) -> bool {
        self.firmlinked_duplicate_paths.contains(path)
    }

    fn record_error(&self, path: &[u8]) {
        self.errors.fetch_add(1, Relaxed);
        let mut s = self.error_samples.lock();
        if s.len() < MAX_ERROR_SAMPLES {
            s.push(PathBuf::from(OsStr::from_bytes(path)));
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

fn dev_of(path: &Path) -> std::io::Result<i32> {
    std::fs::symlink_metadata(path).map(|m| m.dev() as i32)
}

fn canonical_root(root: &Path) -> Result<PathBuf, ScanError> {
    std::fs::canonicalize(root).map_err(|source| ScanError::Root { path: root.to_path_buf(), source })
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
    let root_dev = dev_of(&root).map_err(|source| ScanError::Root { path: root.clone(), source })?;
    ensure_readable(&root)?;

    let space = opts.whole_disk.then(|| hidden::space_info(&root).ok()).flatten();
    if let Some(space) = &space {
        progress.expected_bytes_or_zero.store(space.used, Relaxed);
    }
    let (sibling_volume_devices, firmlinked_duplicate_paths) = if opts.whole_disk {
        (volumes::container_devices(&root), volumes::firmlink_skip_paths())
    } else {
        (FxHashSet::default(), FxHashSet::default())
    };
    let mut system_top_level_dirs: FxHashSet<&'static [u8]> = FxHashSet::default();
    if opts.whole_disk && root == Path::new("/") {
        system_top_level_dirs.extend([&b"System"[..], b"usr", b"bin", b"sbin", b"private", b"cores", b"etc", b"var"]);
    }

    let ctx = Ctx {
        sibling_volume_devices,
        firmlinked_duplicate_paths,
        system_top_level_dirs,
        hard_linked_inodes: ShardedSeenSet::new(),
        shared_data_streams: ShardedSeenSet::new(),
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

    let root_bytes = root.as_os_str().as_bytes().to_vec();
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
    if p.last() != Some(&b'/') {
        p.push(b'/');
    }
    p.extend_from_slice(name);
    p
}

fn scan_dir(ctx: &Ctx<'_>, path: Vec<u8>, name: Box<[u8]>, dev: i32, depth: u32) -> DirNode {
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
    let Ok(cpath) = CString::new(path) else {
        node.flags |= flags::UNREADABLE;
        return node;
    };
    let path = cpath.as_bytes();

    let mut subdirs: Vec<(Box<[u8]>, i32)> = Vec::new();
    let mut file_bytes = 0u64;
    let res = bulk::read_dir(&cpath, |e| match e.objtype {
        _ if e.attributes_unavailable() => {
            ctx.record_error(&join(path, e.name));
            node.push_file(e.name, Kind::Other, flags::UNREADABLE, 0, 0);
        }
        bulk::VDIR => subdirs.push((e.name.into(), e.dev)),
        _ => {
            let (mut alloc, mut logical, mut fl) = (e.alloc, e.logical, 0u8);
            match ctx.already_counted(&e) {
                Some(Duplicate::HardLink) => {
                    alloc = 0;
                    logical = 0;
                    fl |= flags::HARDLINK_DUP;
                }
                Some(Duplicate::PureClone) => {
                    alloc = 0;
                    fl |= flags::CLONE_DUP;
                }
                None => {}
            }
            let kind = if e.objtype == bulk::VLNK {
                fl |= flags::SYMLINK;
                Kind::Other
            } else {
                Kind::of_file(e.name)
            };
            file_bytes += alloc;
            node.push_file(e.name, kind, fl, alloc, logical);
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
    dev: i32,
    subdirs: Vec<(Box<[u8]>, i32)>,
) -> Vec<(Vec<u8>, Box<[u8]>, i32)> {
    subdirs
        .into_iter()
        .filter(|&(_, child_dev)| ctx.may_enter_device(dev, child_dev))
        .map(|(child, child_dev)| (join(path, &child), child, child_dev))
        .filter(|(child_path, _, _)| !ctx.is_firmlinked_duplicate(child_path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Metric;
    use std::os::unix::fs::PermissionsExt;

    fn find(t: &Tree, path: &str) -> crate::NodeId {
        let mut cur = t.root();
        for part in path.split('/') {
            cur = t
                .children(cur, Metric::Allocated)
                .find(|&c| t.name(c) == part)
                .unwrap_or_else(|| panic!("{part} missing"));
        }
        cur
    }

    #[test]
    fn scans_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("a/b")).unwrap();
        std::fs::write(r.join("a/one.bin"), vec![7u8; 100_000]).unwrap();
        std::fs::write(r.join("a/b/two.bin"), vec![7u8; 50_000]).unwrap();
        std::fs::hard_link(r.join("a/one.bin"), r.join("link.bin")).unwrap();
        std::os::unix::fs::symlink(r.join("a"), r.join("sym")).unwrap();
        let sparse_file = std::fs::File::create(r.join("sparse.img")).unwrap();
        sparse_file.set_len(1 << 30).unwrap();
        std::fs::create_dir(r.join("locked")).unwrap();
        std::fs::set_permissions(r.join("locked"), std::fs::Permissions::from_mode(0o000)).unwrap();

        let progress = Progress::default();
        let t = scan(&ScanOptions::folder(r), &progress, &CancelToken::default()).unwrap();
        std::fs::set_permissions(r.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();

        let one = find(&t, "a/one.bin");
        let link = find(&t, "link.bin");
        let sizes = [t.size(one, Metric::Allocated), t.size(link, Metric::Allocated)];
        assert!(sizes.contains(&0) && sizes.iter().any(|&s| s >= 100_000), "exactly one hard link carries the size");

        let sparse = find(&t, "sparse.img");
        assert_eq!(t.size(sparse, Metric::Logical), 1 << 30);
        assert!(t.size(sparse, Metric::Allocated) < 1 << 20, "a sparse file allocates little");

        let sym = find(&t, "sym");
        assert!(!t.node(sym).is_dir());

        assert_eq!(t.stats.errors, 1);
        assert!(t.node(find(&t, "locked")).flags() & flags::UNREADABLE != 0);

        let a = find(&t, "a");
        let sum: u64 = t.children(a, Metric::Allocated).map(|c| t.size(c, Metric::Allocated)).sum();
        assert_eq!(sum, t.size(a, Metric::Allocated), "a folder's size is the sum of its children");
        let temp_dir_behind_var_symlink = r.canonicalize().unwrap();
        assert_eq!(t.path(find(&t, "a/b/two.bin")), temp_dir_behind_var_symlink.join("a/b/two.bin"));
        assert_eq!(progress.snapshot().files, t.node(t.root()).items() as u64);
    }

    #[test]
    fn counts_pure_clones_once() {
        use std::io::{Seek, Write};
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

        let t = scan(&ScanOptions::folder(r), &Progress::default(), &CancelToken::default()).unwrap();
        let (orig, pure, edited) = (find(&t, "orig.bin"), find(&t, "copies/pure.bin"), find(&t, "copies/edited.bin"));
        let alloc = |id| t.size(id, Metric::Allocated);
        assert!(alloc(orig) >= 1_000_000 || alloc(pure) >= 1_000_000);
        assert_eq!(alloc(orig).min(alloc(pure)), 0, "pure clone counted once");
        assert!(t.node(orig).is_clone_copy() ^ t.node(pure).is_clone_copy());
        assert!(alloc(edited) >= 1_000_000, "edited clones count in full");
        for id in [orig, pure, edited] {
            assert_eq!(t.size(id, Metric::Logical), 1_000_000, "logical size is the apparent size of every copy");
        }
        let total = t.size(t.root(), Metric::Allocated);
        assert!((2_000_000..2_100_000).contains(&total), "two streams on disk, not three: {total}");
    }

    #[test]
    fn follows_symlinked_root() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir(r.join("real")).unwrap();
        std::fs::write(r.join("real/f.bin"), vec![1u8; 10_000]).unwrap();
        std::os::unix::fs::symlink(r.join("real"), r.join("link")).unwrap();
        let t = scan(&ScanOptions::folder(r.join("link")), &Progress::default(), &CancelToken::default()).unwrap();
        assert_eq!(t.stats.errors, 0);
        assert_eq!(t.size(find(&t, "f.bin"), Metric::Logical), 10_000);
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
        ctl.set_paused(true);
        std::thread::scope(|s| {
            let scan = s.spawn(|| scan(&ScanOptions::folder(dir.path()), &progress, &ctl));
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(progress.snapshot().dirs, 0, "nothing is read while paused");
            assert!(!scan.is_finished());
            ctl.set_paused(false);
            let t = scan.join().unwrap().unwrap();
            assert_eq!(t.stats.dirs, 51);
            assert!(t.stats.duration < Duration::from_millis(150), "paused time is not counted");
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
