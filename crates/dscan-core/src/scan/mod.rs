//! Parallel file system scanner.

pub mod bulk;
pub mod volumes;

use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use std::time::Instant;

use parking_lot::Mutex;
use rayon::prelude::*;
use rustc_hash::FxHashSet;

use crate::hidden;
use crate::kind::Kind;
use crate::tree::{self, DirNode, ScanStats, Synthetic, Tree, flags};

/// What to scan.
#[derive(Clone, Debug)]
pub struct ScanOptions {
    pub root: PathBuf,
    /// Whole-volume mode: follow APFS firmlinks into the sibling volumes of
    /// the same container and add hidden/purgeable space segments.
    pub whole_disk: bool,
    /// Display name for the root (e.g. the volume name).
    pub root_name: Option<String>,
    /// Worker threads; `None` picks a default suited to I/O-bound scanning.
    pub threads: Option<usize>,
}

impl ScanOptions {
    pub fn folder(root: impl Into<PathBuf>) -> Self {
        ScanOptions { root: root.into(), whole_disk: false, root_name: None, threads: None }
    }
    pub fn volume(mount: impl Into<PathBuf>, name: impl Into<String>) -> Self {
        ScanOptions { root: mount.into(), whole_disk: true, root_name: Some(name.into()), threads: None }
    }

    /// Name shown for the scan root: the volume name or the folder name.
    pub fn display_name(&self) -> String {
        self.root_name.clone().unwrap_or_else(|| match self.root.file_name() {
            Some(n) => n.to_string_lossy().into_owned(),
            None => self.root.display().to_string(),
        })
    }
}

/// Live progress counters, readable from another thread while scanning.
#[derive(Debug, Default)]
pub struct Progress {
    files: AtomicU64,
    dirs: AtomicU64,
    bytes: AtomicU64,
    /// Bytes the scan is expected to find (whole-disk mode), 0 if unknown.
    expected: AtomicU64,
    current: Mutex<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ProgressSnapshot {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub expected: Option<u64>,
    pub current: String,
}

impl Progress {
    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            files: self.files.load(Relaxed),
            dirs: self.dirs.load(Relaxed),
            bytes: self.bytes.load(Relaxed),
            expected: Some(self.expected.load(Relaxed)).filter(|&e| e > 0),
            current: self.current.lock().clone(),
        }
    }
}

/// Cooperative cancellation flag.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Relaxed)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("scan cancelled")]
    Cancelled,
    #[error("cannot read {path}: {source}")]
    Root { path: PathBuf, source: std::io::Error },
}

const HARDLINK_SHARDS: usize = 64;
const MAX_ERROR_SAMPLES: usize = 100;

struct Ctx<'a> {
    /// Devices we may cross into (sibling volumes of the APFS container in
    /// whole-disk mode; empty otherwise).
    allowed_devs: FxHashSet<i32>,
    /// Paths reachable twice through APFS firmlinks (whole-disk mode only).
    skip_paths: FxHashSet<Vec<u8>>,
    system_top: FxHashSet<&'static [u8]>,
    hardlinks: Vec<Mutex<FxHashSet<(i32, u64)>>>,
    errors: AtomicU64,
    error_samples: Mutex<Vec<PathBuf>>,
    progress: &'a Progress,
    cancel: &'a CancelToken,
}

impl Ctx<'_> {
    /// True the first time a hard-linked inode is seen.
    fn first_sight(&self, dev: i32, ino: u64) -> bool {
        self.hardlinks[(ino as usize) % HARDLINK_SHARDS].lock().insert((dev, ino))
    }

    fn record_error(&self, path: &[u8]) {
        self.errors.fetch_add(1, Relaxed);
        let mut s = self.error_samples.lock();
        if s.len() < MAX_ERROR_SAMPLES {
            s.push(PathBuf::from(OsStr::from_bytes(path)));
        }
    }
}

fn dev_of(path: &Path) -> std::io::Result<i32> {
    std::fs::symlink_metadata(path).map(|m| m.dev() as i32)
}

/// Scan `opts.root` and build a [`Tree`].
pub fn scan(opts: &ScanOptions, progress: &Progress, cancel: &CancelToken) -> Result<Tree, ScanError> {
    let started = Instant::now();
    // Resolve symlinks (e.g. /tmp -> /private/tmp): the walk never follows
    // them, so the root must be the real directory.
    let root = std::fs::canonicalize(&opts.root).map_err(|source| ScanError::Root { path: opts.root.clone(), source })?;
    let root_dev = dev_of(&root).map_err(|source| ScanError::Root { path: root.clone(), source })?;
    // Fail early if the root itself is unreadable.
    if let Err(source) = std::fs::read_dir(&root) {
        return Err(ScanError::Root { path: root, source });
    }

    let space = opts.whole_disk.then(|| hidden::space_info(&root).ok()).flatten();
    if let Some(space) = &space {
        progress.expected.store(space.used, Relaxed);
    }
    let (allowed_devs, skip_paths) = if opts.whole_disk {
        (volumes::container_devices(&root), volumes::firmlink_skip_paths())
    } else {
        (FxHashSet::default(), FxHashSet::default())
    };
    let mut system_top: FxHashSet<&'static [u8]> = FxHashSet::default();
    if opts.whole_disk && root == Path::new("/") {
        system_top.extend([&b"System"[..], b"usr", b"bin", b"sbin", b"private", b"cores", b"etc", b"var"]);
    }

    let ctx = Ctx {
        allowed_devs,
        skip_paths,
        system_top,
        hardlinks: (0..HARDLINK_SHARDS).map(|_| Mutex::new(FxHashSet::default())).collect(),
        errors: AtomicU64::new(0),
        error_samples: Mutex::new(Vec::new()),
        progress,
        cancel,
    };

    let threads = opts.threads.unwrap_or_else(|| {
        // Scanning is bound by kernel metadata lookups, not CPU: many more
        // threads than cores keeps APFS busy (≈40 is best on a 10-core M-series).
        (std::thread::available_parallelism().map(|n| n.get()).unwrap_or(8) * 4).clamp(16, 64)
    });
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .stack_size(16 << 20)
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
        duration: started.elapsed(),
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
    let forced = if depth == 1 && ctx.system_top.contains(&name[..]) { Some(Kind::System) } else { Kind::of_dir(&name) };
    let mut node = DirNode::new(name);
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
        // The kernel could not fetch this entry's attributes: keep it
        // visible as unreadable and count it like an unreadable folder.
        _ if e.error != 0 => {
            ctx.record_error(&join(path, e.name));
            node.push_file(e.name, Kind::Other, flags::UNREADABLE, 0, 0);
        }
        bulk::VDIR => subdirs.push((e.name.into(), e.dev)),
        _ => {
            let (mut alloc, mut logical, mut fl) = (e.alloc, e.logical, 0u8);
            if e.objtype == bulk::VREG && e.links > 1 && !ctx.first_sight(e.dev, e.ino) {
                alloc = 0;
                logical = 0;
                fl |= flags::HARDLINK_DUP;
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
        // Vanished while scanning (temp dirs): not a permission problem.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            node.flags |= flags::UNREADABLE;
            ctx.record_error(path);
        }
    }

    let p = ctx.progress;
    p.files.fetch_add(node.files.len() as u64, Relaxed);
    p.bytes.fetch_add(file_bytes, Relaxed);
    if p.dirs.fetch_add(1, Relaxed).is_multiple_of(128)
        && let Some(mut cur) = p.current.try_lock()
    {
        *cur = String::from_utf8_lossy(path).into_owned();
    }

    // Decide what to descend into first, so the parallel collect knows its
    // exact length.
    let todo: Vec<(Vec<u8>, Box<[u8]>, i32)> = subdirs
        .into_iter()
        // Never cross into another file system (except allowed APFS siblings).
        .filter(|(_, child_dev)| *child_dev == dev || ctx.allowed_devs.contains(child_dev))
        .map(|(child, child_dev)| (join(path, &child), child, child_dev))
        // Skip what is already reachable through an APFS firmlink.
        .filter(|(child_path, _, _)| !ctx.skip_paths.contains(child_path))
        .collect();
    node.dirs = if todo.len() <= 1 {
        todo.into_iter().map(|(cp, n, d)| scan_dir(ctx, cp, n, d, depth + 1)).collect()
    } else {
        todo.into_par_iter().map(|(cp, n, d)| scan_dir(ctx, cp, n, d, depth + 1)).collect()
    };
    node.finish(forced);
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Metric;
    use std::os::unix::fs::PermissionsExt;

    fn find(t: &Tree, path: &str) -> crate::NodeId {
        let mut cur = t.root();
        for part in path.split('/') {
            cur = t.children(cur, Metric::Allocated).find(|&c| t.name(c) == part).unwrap_or_else(|| panic!("{part} missing"));
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
        // Sparse file: large logical size, little allocation.
        let f = std::fs::File::create(r.join("sparse.img")).unwrap();
        f.set_len(1 << 30).unwrap();
        // Unreadable directory.
        std::fs::create_dir(r.join("locked")).unwrap();
        std::fs::set_permissions(r.join("locked"), std::fs::Permissions::from_mode(0o000)).unwrap();

        let progress = Progress::default();
        let t = scan(&ScanOptions::folder(r), &progress, &CancelToken::default()).unwrap();
        std::fs::set_permissions(r.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();

        let one = find(&t, "a/one.bin");
        let link = find(&t, "link.bin");
        // Exactly one of the two hard links carries the size.
        let sizes = [t.size(one, Metric::Allocated), t.size(link, Metric::Allocated)];
        assert!(sizes.contains(&0) && sizes.iter().any(|&s| s >= 100_000));

        let sparse = find(&t, "sparse.img");
        assert_eq!(t.size(sparse, Metric::Logical), 1 << 30);
        assert!(t.size(sparse, Metric::Allocated) < 1 << 20);

        let sym = find(&t, "sym");
        assert!(!t.node(sym).is_dir());

        assert_eq!(t.stats.errors, 1);
        assert!(t.node(find(&t, "locked")).flags() & flags::UNREADABLE != 0);

        // Totals are the sum of the parts.
        let a = find(&t, "a");
        let sum: u64 = t.children(a, Metric::Allocated).map(|c| t.size(c, Metric::Allocated)).sum();
        assert_eq!(sum, t.size(a, Metric::Allocated));
        // The root is canonicalized (the temp dir lives under the /var symlink).
        assert_eq!(t.path(find(&t, "a/b/two.bin")), r.canonicalize().unwrap().join("a/b/two.bin"));
        assert_eq!(progress.snapshot().files, t.node(t.root()).items() as u64);
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
    fn cancel_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let c = CancelToken::default();
        c.cancel();
        let r = scan(&ScanOptions::folder(dir.path()), &Progress::default(), &c);
        assert!(matches!(r, Err(ScanError::Cancelled)));
    }
}
