//! Compact arena tree holding the scan result.
//!
//! Nodes are stored breadth-first, so a node's children occupy a contiguous
//! id range and every parent id is smaller than its children's ids. Per
//! metric, `order` holds a permutation of each child range sorted by size,
//! largest first.

use std::borrow::Cow;
use std::cmp::Reverse;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::kind::Kind;

/// Index of a node in the [`Tree`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

impl NodeId {
    pub const ROOT: NodeId = NodeId(0);
    #[inline]
    pub fn idx(self) -> usize {
        self.0 as usize
    }
}

/// Which size to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Metric {
    /// Bytes allocated on disk (what deleting frees). Hard links count once.
    #[default]
    Allocated,
    /// Logical file length (what Finder shows as "size").
    Logical,
}

impl Metric {
    const ALL: [Metric; 2] = [Metric::Allocated, Metric::Logical];
    fn slot(self) -> usize {
        self as usize
    }
}

pub(crate) mod flags {
    pub const DIR: u8 = 1;
    pub const REMOVED: u8 = 2;
    /// Second or later sighting of a hard-linked file; size counted as 0.
    pub const HARDLINK_DUP: u8 = 4;
    /// The directory could not be read (permissions…).
    pub const UNREADABLE: u8 = 8;
    /// Not a real file system object (hidden/purgeable space).
    pub const SYNTHETIC: u8 = 16;
    pub const SYMLINK: u8 = 32;
}

#[derive(Clone, Debug)]
pub struct Node {
    name_off: u32,
    name_len: u16,
    kind: u8,
    flags: u8,
    parent: u32,
    first_child: u32,
    child_count: u32,
    /// Number of files in this subtree (1 for a file).
    items: u32,
    alloc: u64,
    logical: u64,
}

impl Node {
    fn new(parent: u32, name: (u32, u16), kind: Kind, flags: u8, items: u32, alloc: u64, logical: u64) -> Node {
        Node {
            name_off: name.0,
            name_len: name.1,
            kind: kind as u8,
            flags,
            parent,
            first_child: 0,
            child_count: 0,
            items,
            alloc,
            logical,
        }
    }
    #[inline]
    pub fn kind(&self) -> Kind {
        Kind::from_u8(self.kind)
    }
    #[cfg(test)]
    pub(crate) fn flags(&self) -> u8 {
        self.flags
    }
    #[inline]
    pub fn is_dir(&self) -> bool {
        self.flags & flags::DIR != 0
    }
    /// The node or one of its ancestors was removed ([`Tree::remove`]
    /// marks the whole subtree).
    #[inline]
    pub fn is_removed(&self) -> bool {
        self.flags & flags::REMOVED != 0
    }
    #[inline]
    pub fn is_synthetic(&self) -> bool {
        self.flags & flags::SYNTHETIC != 0
    }
    /// A directory that could not be read.
    #[inline]
    pub fn is_unreadable(&self) -> bool {
        self.flags & flags::UNREADABLE != 0
    }
    #[inline]
    pub fn size(&self, m: Metric) -> u64 {
        match m {
            Metric::Allocated => self.alloc,
            Metric::Logical => self.logical,
        }
    }
    #[inline]
    pub fn items(&self) -> u32 {
        self.items
    }
}

/// Statistics gathered during a scan.
#[derive(Clone, Debug, Default)]
pub struct ScanStats {
    pub files: u64,
    pub dirs: u64,
    /// Directories that could not be read.
    pub errors: u64,
    /// A few unreadable paths, for display.
    pub error_samples: Vec<PathBuf>,
    pub duration: Duration,
    /// Bytes of the volume not reached by the scan (whole-disk mode only).
    pub hidden: u64,
    pub purgeable: u64,
}

pub struct Tree {
    nodes: Vec<Node>,
    names: Vec<u8>,
    order: [Vec<u32>; 2],
    root_path: PathBuf,
    whole_disk: bool,
    pub stats: ScanStats,
}

impl std::fmt::Debug for Tree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tree").field("root_path", &self.root_path).field("nodes", &self.nodes.len()).finish()
    }
}

impl Tree {
    #[inline]
    pub fn root(&self) -> NodeId {
        NodeId::ROOT
    }
    pub fn root_path(&self) -> &Path {
        &self.root_path
    }
    pub fn is_whole_disk(&self) -> bool {
        self.whole_disk
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    #[inline]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.idx()]
    }
    #[inline]
    pub fn size(&self, id: NodeId, m: Metric) -> u64 {
        self.nodes[id.idx()].size(m)
    }
    #[inline]
    pub fn name_bytes(&self, id: NodeId) -> &[u8] {
        let n = &self.nodes[id.idx()];
        &self.names[n.name_off as usize..n.name_off as usize + n.name_len as usize]
    }
    pub fn name(&self, id: NodeId) -> Cow<'_, str> {
        String::from_utf8_lossy(self.name_bytes(id))
    }
    #[inline]
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        if id == NodeId::ROOT { None } else { Some(NodeId(self.nodes[id.idx()].parent)) }
    }

    /// Live children of `id`, largest first according to `m`.
    pub fn children(&self, id: NodeId, m: Metric) -> impl DoubleEndedIterator<Item = NodeId> + '_ {
        let n = &self.nodes[id.idx()];
        let start = n.first_child as usize;
        let end = start + n.child_count as usize;
        self.order[m.slot()][start..end].iter().map(|&c| NodeId(c)).filter(|c| !self.nodes[c.idx()].is_removed())
    }

    /// All children of `id`, including removed ones, in arena order.
    fn all_children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let n = &self.nodes[id.idx()];
        let start = n.first_child as usize;
        self.order[Metric::Allocated.slot()][start..start + n.child_count as usize].iter().map(|&c| NodeId(c))
    }

    /// Iterator over `id` and all its ancestors up to the root.
    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(Some(id), |&c| self.parent(c))
    }

    /// Is `a` an ancestor of (or equal to) `b`?
    pub fn is_ancestor_of(&self, a: NodeId, b: NodeId) -> bool {
        // Breadth-first ids: ancestors always have smaller ids.
        a.0 <= b.0 && self.ancestors(b).any(|x| x == a)
    }

    /// Is `b` inside `a` (and not `a` itself)?
    pub fn is_strict_ancestor_of(&self, a: NodeId, b: NodeId) -> bool {
        a != b && self.is_ancestor_of(a, b)
    }

    /// Neither the node nor any ancestor has been removed.
    #[inline]
    pub fn is_live(&self, id: NodeId) -> bool {
        !self.nodes[id.idx()].is_removed()
    }

    /// `id` if live, otherwise its closest live ancestor.
    pub fn nearest_live(&self, id: NodeId) -> NodeId {
        let mut cur = id;
        while !self.is_live(cur) {
            cur = self.parent(cur).unwrap_or(NodeId::ROOT);
        }
        cur
    }

    /// A live, real file system object other than the root: something that
    /// can be previewed, collected or trashed.
    pub fn is_actionable(&self, id: NodeId) -> bool {
        id != NodeId::ROOT && !self.nodes[id.idx()].is_synthetic() && self.is_live(id)
    }

    /// Absolute file system path of a node.
    pub fn path(&self, id: NodeId) -> PathBuf {
        let mut parts: Vec<&[u8]> =
            self.ancestors(id).take_while(|&a| a != NodeId::ROOT).map(|a| self.name_bytes(a)).collect();
        parts.reverse();
        let mut p = self.root_path.clone();
        for part in parts {
            p.push(OsStr::from_bytes(part));
        }
        p
    }

    /// Mark `id` and its whole subtree removed and subtract its sizes from
    /// all ancestors. Returns false if it was already removed or is root.
    pub fn remove(&mut self, id: NodeId) -> bool {
        if id == NodeId::ROOT || !self.is_live(id) {
            return false;
        }
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            self.nodes[cur.idx()].flags |= flags::REMOVED;
            stack.extend(self.all_children(cur));
        }
        let n = &self.nodes[id.idx()];
        let (alloc, logical, items) = (n.alloc, n.logical, n.items);
        let ancestors: Vec<NodeId> = self.ancestors(id).skip(1).collect();
        for &a in &ancestors {
            let n = &mut self.nodes[a.idx()];
            n.alloc = n.alloc.saturating_sub(alloc);
            n.logical = n.logical.saturating_sub(logical);
            n.items = n.items.saturating_sub(items);
        }
        // Each ancestor shrank: move it to its new rank among its siblings.
        for &a in &ancestors {
            if let Some(p) = self.parent(a) {
                self.reposition(p, a);
            }
        }
        true
    }

    /// `child` got smaller: shift it later in `parent`'s sorted child ranges.
    fn reposition(&mut self, parent: NodeId, child: NodeId) {
        let Tree { nodes, order, .. } = self;
        let n = &nodes[parent.idx()];
        let range = n.first_child as usize..(n.first_child + n.child_count) as usize;
        for m in Metric::ALL {
            let slice = &mut order[m.slot()][range.clone()];
            let Some(i) = slice.iter().position(|&c| c == child.0) else { continue };
            let size = nodes[child.idx()].size(m);
            let shift = slice[i + 1..].partition_point(|&c| nodes[c as usize].size(m) > size);
            slice[i..=i + shift].rotate_left(1);
        }
    }

    #[cfg(test)]
    pub(crate) fn ids(&self) -> impl ExactSizeIterator<Item = NodeId> {
        (0..self.nodes.len() as u32).map(NodeId)
    }

    #[cfg(test)]
    pub(crate) fn depth(&self, id: NodeId) -> usize {
        self.ancestors(id).count() - 1
    }
}

// ---------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------

/// A file record produced by the scanner; its name lives in the owning
/// [`DirNode`]'s `names` buffer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FileRec {
    pub name_off: u32,
    pub name_len: u16,
    pub kind: Kind,
    pub flags: u8,
    pub alloc: u64,
    pub logical: u64,
}

/// A directory as produced (bottom-up) by the scanner.
#[derive(Debug, Default)]
pub(crate) struct DirNode {
    pub name: Box<[u8]>,
    pub names: Vec<u8>,
    pub files: Vec<FileRec>,
    pub dirs: Vec<DirNode>,
    pub alloc: u64,
    pub logical: u64,
    pub items: u32,
    /// Directories in this subtree, including this one.
    pub subdirs: u64,
    /// Name bytes in this subtree, including this directory's own name.
    pub name_bytes: u64,
    pub kind: Kind,
    pub flags: u8,
}

fn clamp_name(name: &[u8]) -> &[u8] {
    &name[..name.len().min(u16::MAX as usize)]
}

impl DirNode {
    pub fn new(name: Box<[u8]>) -> Self {
        DirNode { name, flags: flags::DIR, ..Default::default() }
    }

    pub fn push_file(&mut self, name: &[u8], kind: Kind, flags: u8, alloc: u64, logical: u64) {
        let name = clamp_name(name);
        let off = self.names.len() as u32;
        self.names.extend_from_slice(name);
        self.files.push(FileRec { name_off: off, name_len: name.len() as u16, kind, flags, alloc, logical });
    }

    /// Compute totals and the dominant kind from files and child dirs.
    pub fn finish(&mut self, forced_kind: Option<Kind>) {
        let mut by_kind = [0u64; Kind::COUNT];
        let (mut alloc, mut logical, mut items) = (0u64, 0u64, 0u32);
        let (mut subdirs, mut name_bytes) = (1u64, (self.name.len() + self.names.len()) as u64);
        for f in &self.files {
            alloc += f.alloc;
            logical += f.logical;
            items = items.saturating_add(1);
            by_kind[f.kind as usize] += f.alloc.max(1);
        }
        for d in &self.dirs {
            alloc += d.alloc;
            logical += d.logical;
            items = items.saturating_add(d.items);
            subdirs += d.subdirs;
            name_bytes += d.name_bytes;
            by_kind[d.kind as usize] += d.alloc;
        }
        self.alloc = alloc;
        self.logical = logical;
        self.items = items;
        self.subdirs = subdirs;
        self.name_bytes = name_bytes;
        self.kind = forced_kind.unwrap_or_else(|| {
            let (i, &v) = by_kind.iter().enumerate().max_by_key(|&(_, v)| *v).unwrap();
            if v == 0 { Kind::Other } else { Kind::ALL[i] }
        });
    }
}

/// Extra synthetic child of the root (hidden / purgeable space).
pub(crate) struct Synthetic {
    pub name: &'static str,
    pub kind: Kind,
    pub size: u64,
}

pub(crate) fn build(
    mut root: DirNode,
    root_path: PathBuf,
    root_name: &str,
    whole_disk: bool,
    synthetic: Vec<Synthetic>,
    stats: ScanStats,
) -> Tree {
    // Fold synthetic sizes into the root totals.
    for s in &synthetic {
        root.alloc += s.size;
        root.logical += s.size;
    }

    // Exact sizes: no reallocation while the scanner's tree is still alive.
    let node_count = root.items as usize + root.subdirs as usize + synthetic.len();
    let name_bytes = root.name_bytes as usize + root_name.len() + synthetic.iter().map(|s| s.name.len()).sum::<usize>();
    let mut nodes: Vec<Node> = Vec::with_capacity(node_count);
    let mut names: Vec<u8> = Vec::with_capacity(name_bytes);
    let mut logical_order: Vec<u32> = Vec::with_capacity(node_count);

    let push_name = |names: &mut Vec<u8>, s: &[u8]| -> (u32, u16) {
        let s = clamp_name(s);
        let off = names.len() as u32;
        names.extend_from_slice(s);
        (off, s.len() as u16)
    };

    let name = push_name(&mut names, root_name.as_bytes());
    nodes.push(Node::new(0, name, root.kind, root.flags, root.items, root.alloc, root.logical));
    logical_order.push(0);

    let mut queue: std::collections::VecDeque<(u32, DirNode, Vec<Synthetic>)> = Default::default();
    queue.push_back((0, root, synthetic));

    // Scratch buffers reused for every directory: sort keys and ranks.
    let mut by_alloc: Vec<(Reverse<u64>, u32)> = Vec::new();
    let mut by_logical: Vec<(Reverse<u64>, u32)> = Vec::new();
    let mut rank: Vec<u32> = Vec::new();

    while let Some((id, dir, synth)) = queue.pop_front() {
        let DirNode { names: dir_names, files, mut dirs, .. } = dir;
        let (nd, nf) = (dirs.len(), files.len());
        let total = nd + nf + synth.len();

        by_alloc.clear();
        by_logical.clear();
        let sizes = dirs
            .iter()
            .map(|d| (d.alloc, d.logical))
            .chain(files.iter().map(|f| (f.alloc, f.logical)))
            .chain(synth.iter().map(|s| (s.size, s.size)));
        for (i, (a, l)) in sizes.enumerate() {
            by_alloc.push((Reverse(a), i as u32));
            by_logical.push((Reverse(l), i as u32));
        }
        // Ties break on the index, so the order is deterministic.
        by_alloc.sort_unstable();
        by_logical.sort_unstable();

        let first = nodes.len() as u32;
        nodes[id as usize].first_child = first;
        nodes[id as usize].child_count = total as u32;
        rank.clear();
        rank.resize(total, 0);

        for (r, &(_, i)) in by_alloc.iter().enumerate() {
            let i = i as usize;
            rank[i] = r as u32;
            let child_id = first + r as u32;
            if i < nd {
                let d = std::mem::take(&mut dirs[i]);
                let name = push_name(&mut names, &d.name);
                nodes.push(Node::new(id, name, d.kind, d.flags, d.items, d.alloc, d.logical));
                queue.push_back((child_id, d, Vec::new()));
            } else if i < nd + nf {
                let f = &files[i - nd];
                let s = &dir_names[f.name_off as usize..f.name_off as usize + f.name_len as usize];
                let name = push_name(&mut names, s);
                nodes.push(Node::new(id, name, f.kind, f.flags, 1, f.alloc, f.logical));
            } else {
                let s = &synth[i - nd - nf];
                let name = push_name(&mut names, s.name.as_bytes());
                nodes.push(Node::new(id, name, s.kind, flags::SYNTHETIC, 0, s.size, s.size));
            }
        }
        logical_order.extend(by_logical.iter().map(|&(_, i)| first + rank[i as usize]));
    }

    let alloc_order: Vec<u32> = (0..nodes.len() as u32).collect();
    debug_assert_eq!(logical_order.len(), nodes.len());

    Tree { nodes, names, order: [alloc_order, logical_order], root_path, whole_disk, stats }
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::*;

    /// Build a tree from a nested description: (name, size) leaves and
    /// (name, children) dirs.
    pub enum T {
        F(&'static str, u64),
        D(&'static str, Vec<T>),
    }

    fn to_dir(name: &str, kids: Vec<T>) -> DirNode {
        let mut d = DirNode::new(name.as_bytes().into());
        for k in kids {
            match k {
                T::F(n, s) => d.push_file(n.as_bytes(), Kind::of_file(n.as_bytes()), 0, s, s / 2),
                T::D(n, c) => d.dirs.push(to_dir(n, c)),
            }
        }
        d.finish(None);
        d
    }

    pub fn tree(kids: Vec<T>) -> Tree {
        build(to_dir("root", kids), PathBuf::from("/r"), "root", false, vec![], ScanStats::default())
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::{T::*, tree};
    use super::*;

    fn sample() -> Tree {
        tree(vec![
            F("small.txt", 10),
            D("a", vec![F("x.mov", 500), F("y.mov", 300), D("deep", vec![F("z.rs", 50)])]),
            D("b", vec![F("w.png", 100)]),
        ])
    }

    #[test]
    fn sizes_sum_and_sorted() {
        let t = sample();
        assert_eq!(t.size(t.root(), Metric::Allocated), 960);
        assert_eq!(t.size(t.root(), Metric::Logical), 480);
        let kids: Vec<_> = t.children(t.root(), Metric::Allocated).map(|c| t.name(c).into_owned()).collect();
        assert_eq!(kids, vec!["a", "b", "small.txt"]);
        let a = t.children(t.root(), Metric::Allocated).next().unwrap();
        assert_eq!(t.size(a, Metric::Allocated), 850);
        assert_eq!(t.node(a).items(), 3);
        assert_eq!(t.node(a).kind(), Kind::Video);
        assert_eq!(t.node(t.root()).items(), 5);
    }

    #[test]
    fn parents_before_children_and_paths() {
        let t = sample();
        for id in t.ids() {
            if let Some(p) = t.parent(id) {
                assert!(p.0 < id.0);
            }
        }
        let a = t.children(t.root(), Metric::Allocated).next().unwrap();
        let deep = t.children(a, Metric::Allocated).find(|&c| t.name(c) == "deep").unwrap();
        let z = t.children(deep, Metric::Allocated).next().unwrap();
        assert_eq!(t.path(z), PathBuf::from("/r/a/deep/z.rs"));
        assert_eq!(t.path(t.root()), PathBuf::from("/r"));
        assert_eq!(t.depth(z), 3);
        assert!(t.is_ancestor_of(a, z));
        assert!(!t.is_ancestor_of(z, a));
    }

    #[test]
    fn remove_propagates_and_resorts() {
        let mut t = sample();
        let a = t.children(t.root(), Metric::Allocated).next().unwrap();
        let x = t.children(a, Metric::Allocated).next().unwrap();
        let y = t.children(a, Metric::Allocated).nth(1).unwrap();
        assert!(t.remove(x));
        assert!(!t.remove(x));
        assert!(t.remove(y));
        assert_eq!(t.size(a, Metric::Allocated), 50);
        assert_eq!(t.size(t.root(), Metric::Allocated), 160);
        assert_eq!(t.node(t.root()).items(), 3);
        let kids: Vec<_> = t.children(t.root(), Metric::Allocated).map(|c| t.name(c).into_owned()).collect();
        assert_eq!(kids, vec!["b", "a", "small.txt"]);
        assert_eq!(t.children(a, Metric::Allocated).count(), 1);
    }
}
