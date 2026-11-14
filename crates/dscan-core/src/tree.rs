use std::borrow::Cow;
use std::cmp::Reverse;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::age::{AgeBucket, Day};
use crate::kind::{Kind, KindSet};
use crate::platform::{Current, Platform};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

impl NodeId {
    pub const ROOT: NodeId = NodeId(0);
    #[inline]
    pub fn idx(self) -> usize {
        self.0 as usize
    }
}

pub(crate) mod flags {
    pub const DIR: u8 = 1;
    pub const REMOVED: u8 = 2;
    pub const HARDLINK_DUP: u8 = 4;
    pub const UNREADABLE: u8 = 8;
    pub const SYMLINK: u8 = 32;
    pub const CLONE_DUP: u8 = 64;
    pub const SHARES_COUNTED_BLOCKS: u8 = 128;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accounting {
    Counted,
    Unreadable,
    CloneCopy,
    SharesBlocksCountedElsewhere,
    HardLinkCopy,
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
    files_in_subtree: u32,
    alloc: u64,
}

impl Node {
    fn new(parent: u32, name: (u32, u16), kind: Kind, flags: u8, files_in_subtree: u32, alloc: u64) -> Node {
        Node {
            name_off: name.0,
            name_len: name.1,
            kind: kind as u8,
            flags,
            parent,
            first_child: 0,
            child_count: 0,
            files_in_subtree,
            alloc,
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
    #[inline]
    pub fn is_removed(&self) -> bool {
        self.flags & flags::REMOVED != 0
    }
    #[inline]
    pub fn is_synthetic(&self) -> bool {
        self.kind().is_synthetic()
    }
    pub fn accounting(&self) -> Accounting {
        let has = |flag: u8| self.flags & flag != 0;
        if has(flags::UNREADABLE) {
            Accounting::Unreadable
        } else if has(flags::CLONE_DUP) {
            Accounting::CloneCopy
        } else if has(flags::SHARES_COUNTED_BLOCKS) {
            Accounting::SharesBlocksCountedElsewhere
        } else if has(flags::HARDLINK_DUP) {
            Accounting::HardLinkCopy
        } else {
            Accounting::Counted
        }
    }
    #[inline]
    pub fn size(&self) -> u64 {
        self.alloc
    }
    #[inline]
    pub fn items(&self) -> u32 {
        self.files_in_subtree
    }
}

#[derive(Clone, Debug)]
pub struct ScanStats {
    pub files: u64,
    pub dirs: u64,
    pub errors: u64,
    pub duration: Duration,
    pub hidden: u64,
    pub purgeable: u64,
    pub scanned_on: Day,
}

#[cfg(test)]
impl Default for ScanStats {
    fn default() -> Self {
        const A_FIXED_SCAN_DAY: u16 = 20_000;
        ScanStats {
            files: 0,
            dirs: 0,
            errors: 0,
            duration: Duration::ZERO,
            hidden: 0,
            purgeable: 0,
            scanned_on: Day::from_days_since_1970(A_FIXED_SCAN_DAY),
        }
    }
}

pub struct Tree {
    nodes: Vec<Node>,
    newest: Vec<Option<Day>>,
    kinds_inside: Vec<KindSet>,
    names: Vec<u8>,
    children_largest_first: Vec<u32>,
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
    pub fn size(&self, id: NodeId) -> u64 {
        self.nodes[id.idx()].size()
    }
    #[inline]
    pub fn newest_change(&self, id: NodeId) -> Option<Day> {
        self.newest[id.idx()]
    }
    pub fn days_since_newest_change(&self, id: NodeId) -> Option<u32> {
        self.newest_change(id).map(|day| day.days_until(self.stats.scanned_on))
    }
    #[inline]
    pub fn age(&self, id: NodeId) -> AgeBucket {
        AgeBucket::of(self.newest_change(id), self.stats.scanned_on)
    }
    #[inline]
    pub(crate) fn kinds_inside(&self, id: NodeId) -> KindSet {
        self.kinds_inside[id.idx()]
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

    pub fn children(&self, id: NodeId) -> impl DoubleEndedIterator<Item = NodeId> + '_ {
        self.children_including_removed(id).filter(|&c| self.is_live(c))
    }

    fn children_including_removed(&self, id: NodeId) -> impl DoubleEndedIterator<Item = NodeId> + '_ {
        let n = &self.nodes[id.idx()];
        let start = n.first_child as usize;
        self.children_largest_first[start..start + n.child_count as usize].iter().map(|&c| NodeId(c))
    }

    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(Some(id), |&c| self.parent(c))
    }

    pub fn is_ancestor_of(&self, a: NodeId, b: NodeId) -> bool {
        let could_be_ancestor_since_ids_are_breadth_first = a <= b;
        could_be_ancestor_since_ids_are_breadth_first && self.ancestors(b).any(|x| x == a)
    }

    pub fn is_strict_ancestor_of(&self, a: NodeId, b: NodeId) -> bool {
        a != b && self.is_ancestor_of(a, b)
    }

    #[inline]
    pub fn is_live(&self, id: NodeId) -> bool {
        !self.nodes[id.idx()].is_removed()
    }

    pub fn nearest_live(&self, id: NodeId) -> NodeId {
        let mut cur = id;
        while !self.is_live(cur) {
            cur = self.parent(cur).unwrap_or(NodeId::ROOT);
        }
        cur
    }

    pub fn is_actionable(&self, id: NodeId) -> bool {
        id != NodeId::ROOT && !self.nodes[id.idx()].is_synthetic() && self.is_live(id)
    }

    pub fn path(&self, id: NodeId) -> PathBuf {
        let mut parts: Vec<&[u8]> =
            self.ancestors(id).take_while(|&a| a != NodeId::ROOT).map(|a| self.name_bytes(a)).collect();
        parts.reverse();
        let mut p = self.root_path.clone();
        for part in parts {
            p.push(Current::os_str_from_bytes(part));
        }
        p
    }

    pub fn remove(&mut self, id: NodeId) -> bool {
        if id == NodeId::ROOT || !self.is_live(id) {
            return false;
        }
        self.mark_subtree_removed(id);
        let shrunken_ancestors: Vec<NodeId> = self.ancestors(id).skip(1).collect();
        self.subtract_sizes_of_removed_from(&shrunken_ancestors, id);
        self.refresh_dates_and_kinds_from_live_children(&shrunken_ancestors);
        for &a in &shrunken_ancestors {
            if let Some(p) = self.parent(a) {
                self.move_shrunken_child_to_its_new_rank(p, a);
            }
        }
        true
    }

    fn mark_subtree_removed(&mut self, id: NodeId) {
        let mut stack = vec![id];
        while let Some(cur) = stack.pop() {
            self.nodes[cur.idx()].flags |= flags::REMOVED;
            stack.extend(self.children_including_removed(cur));
        }
    }

    fn subtract_sizes_of_removed_from(&mut self, ancestors: &[NodeId], removed: NodeId) {
        let n = &self.nodes[removed.idx()];
        let (alloc, files) = (n.alloc, n.files_in_subtree);
        for &a in ancestors {
            let n = &mut self.nodes[a.idx()];
            n.alloc = n.alloc.saturating_sub(alloc);
            n.files_in_subtree = n.files_in_subtree.saturating_sub(files);
        }
    }

    fn refresh_dates_and_kinds_from_live_children(&mut self, innermost_first: &[NodeId]) {
        for &folder in innermost_first {
            let newest = self.children(folder).filter_map(|c| self.newest_change(c)).max();
            self.newest[folder.idx()] = newest;
            self.kinds_inside[folder.idx()] = self.children(folder).map(|c| self.kinds_inside(c)).collect();
        }
    }

    fn move_shrunken_child_to_its_new_rank(&mut self, parent: NodeId, child: NodeId) {
        let Tree { nodes, children_largest_first: order, .. } = self;
        let n = &nodes[parent.idx()];
        let range = n.first_child as usize..(n.first_child + n.child_count) as usize;
        let slice = &mut order[range];
        let Some(i) = slice.iter().position(|&c| c == child.0) else { return };
        let size = nodes[child.idx()].size();
        let shift = slice[i + 1..].partition_point(|&c| nodes[c as usize].size() > size);
        slice[i..=i + shift].rotate_left(1);
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

#[derive(Clone, Copy, Debug)]
pub(crate) struct FileRec {
    pub name_off_in_dir_names: u32,
    pub name_len: u16,
    pub kind: Kind,
    pub flags: u8,
    pub alloc: u64,
}

#[derive(Debug, Default)]
pub(crate) struct DirNode {
    pub name: Box<[u8]>,
    pub names: Vec<u8>,
    pub files: Vec<FileRec>,
    pub file_days: Vec<Option<Day>>,
    pub dirs: Vec<DirNode>,
    pub newest: Option<Day>,
    pub kinds_inside: KindSet,
    pub one_kind_throughout: bool,
    pub alloc: u64,
    pub items: u32,
    pub dirs_in_subtree: u64,
    pub name_bytes_in_subtree: u64,
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

    pub fn push_file(&mut self, name: &[u8], kind: Kind, flags: u8, alloc: u64, modified: Option<Day>) {
        let name = clamp_name(name);
        let off = self.names.len() as u32;
        self.names.extend_from_slice(name);
        self.files.push(FileRec { name_off_in_dir_names: off, name_len: name.len() as u16, kind, flags, alloc });
        self.file_days.push(modified);
    }

    pub fn finish(&mut self, forced_kind: Option<Kind>) {
        let mut alloc_by_kind = [0u64; Kind::COUNT];
        let mut kinds_inside = KindSet::default();
        let (mut alloc, mut items) = (0u64, 0u32);
        let mut dirs_in_subtree = 1u64;
        let mut name_bytes_in_subtree = (self.name.len() + self.names.len()) as u64;
        for f in &self.files {
            alloc += f.alloc;
            items = items.saturating_add(1);
            alloc_by_kind[f.kind as usize] += f.alloc.max(1);
            kinds_inside = kinds_inside.union(KindSet::of(f.kind));
        }
        for d in &self.dirs {
            alloc += d.alloc;
            items = items.saturating_add(d.items);
            dirs_in_subtree += d.dirs_in_subtree;
            name_bytes_in_subtree += d.name_bytes_in_subtree;
            alloc_by_kind[d.kind as usize] += d.alloc;
            kinds_inside = kinds_inside.union(d.kinds_inside);
        }
        let dated_children = self.file_days.iter().copied().chain(self.dirs.iter().map(|d| d.newest));
        self.newest = dated_children.flatten().max();
        self.alloc = alloc;
        self.items = items;
        self.dirs_in_subtree = dirs_in_subtree;
        self.name_bytes_in_subtree = name_bytes_in_subtree;
        self.kind = forced_kind.unwrap_or_else(|| dominant_kind(&alloc_by_kind));
        self.one_kind_throughout = forced_kind.is_some();
        self.kinds_inside = forced_kind.map_or(kinds_inside, KindSet::of);
    }
}

fn dominant_kind(alloc_by_kind: &[u64; Kind::COUNT]) -> Kind {
    let (i, &largest) = alloc_by_kind.iter().enumerate().max_by_key(|&(_, v)| *v).unwrap();
    if largest == 0 { Kind::Other } else { Kind::ALL[i] }
}

type LargestFirstThenScanOrder = (Reverse<u64>, u32);

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
    for s in &synthetic {
        root.alloc += s.size;
    }

    let exact_node_count = root.items as usize + root.dirs_in_subtree as usize + synthetic.len();
    let exact_name_bytes =
        root.name_bytes_in_subtree as usize + root_name.len() + synthetic.iter().map(|s| s.name.len()).sum::<usize>();
    let mut nodes: Vec<Node> = Vec::with_capacity(exact_node_count);
    let mut newest: Vec<Option<Day>> = Vec::with_capacity(exact_node_count);
    let mut kinds_inside: Vec<KindSet> = Vec::with_capacity(exact_node_count);
    let mut names: Vec<u8> = Vec::with_capacity(exact_name_bytes);

    let push_name = |names: &mut Vec<u8>, s: &[u8]| -> (u32, u16) {
        let s = clamp_name(s);
        let off = names.len() as u32;
        names.extend_from_slice(s);
        (off, s.len() as u16)
    };

    let name = push_name(&mut names, root_name.as_bytes());
    nodes.push(Node::new(0, name, root.kind, root.flags, root.items, root.alloc));
    newest.push(root.newest);
    let root_one_kind = root.one_kind_throughout.then_some(root.kinds_inside);
    kinds_inside.push(
        root_one_kind
            .unwrap_or_else(|| synthetic.iter().map(|s| KindSet::of(s.kind)).chain([root.kinds_inside]).collect()),
    );

    let mut queue: std::collections::VecDeque<(u32, DirNode, Vec<Synthetic>, Option<KindSet>)> = Default::default();
    queue.push_back((0, root, synthetic, root_one_kind));

    let mut by_alloc: Vec<LargestFirstThenScanOrder> = Vec::new();

    while let Some((id, dir, synth, enclosing_one_kind)) = queue.pop_front() {
        let DirNode { names: dir_names, files, file_days, mut dirs, .. } = dir;
        let (nd, nf) = (dirs.len(), files.len());
        let total = nd + nf + synth.len();

        by_alloc.clear();
        let sizes =
            dirs.iter().map(|d| d.alloc).chain(files.iter().map(|f| f.alloc)).chain(synth.iter().map(|s| s.size));
        by_alloc.extend(sizes.enumerate().map(|(i, size)| (Reverse(size), i as u32)));
        by_alloc.sort_unstable();

        let first = nodes.len() as u32;
        nodes[id as usize].first_child = first;
        nodes[id as usize].child_count = total as u32;

        for (r, &(_, i)) in by_alloc.iter().enumerate() {
            let i = i as usize;
            let child_id = first + r as u32;
            if i < nd {
                let d = std::mem::take(&mut dirs[i]);
                let name = push_name(&mut names, &d.name);
                let one_kind = enclosing_one_kind.or_else(|| d.one_kind_throughout.then_some(d.kinds_inside));
                nodes.push(Node::new(id, name, d.kind, d.flags, d.items, d.alloc));
                newest.push(d.newest);
                kinds_inside.push(one_kind.unwrap_or(d.kinds_inside));
                queue.push_back((child_id, d, Vec::new(), one_kind));
            } else if i < nd + nf {
                let f = &files[i - nd];
                let start = f.name_off_in_dir_names as usize;
                let s = &dir_names[start..start + f.name_len as usize];
                let name = push_name(&mut names, s);
                nodes.push(Node::new(id, name, f.kind, f.flags, 1, f.alloc));
                newest.push(file_days[i - nd]);
                kinds_inside.push(enclosing_one_kind.unwrap_or(KindSet::of(f.kind)));
            } else {
                let s = &synth[i - nd - nf];
                let name = push_name(&mut names, s.name.as_bytes());
                nodes.push(Node::new(id, name, s.kind, 0, 0, s.size));
                newest.push(None);
                kinds_inside.push(enclosing_one_kind.unwrap_or(KindSet::of(s.kind)));
            }
        }
    }

    let laid_out_largest_first: Vec<u32> = (0..nodes.len() as u32).collect();
    Tree {
        nodes,
        newest,
        kinds_inside,
        names,
        children_largest_first: laid_out_largest_first,
        root_path,
        whole_disk,
        stats,
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::*;

    pub enum T {
        F(&'static str, u64),
        Aged(&'static str, u64, Day),
        D(&'static str, Vec<T>),
    }

    fn to_dir(name: &str, kids: Vec<T>) -> DirNode {
        let mut d = DirNode::new(name.as_bytes().into());
        for k in kids {
            match k {
                T::F(n, s) => d.push_file(n.as_bytes(), Kind::of_file(n.as_bytes()), 0, s, None),
                T::Aged(n, s, modified) => d.push_file(n.as_bytes(), Kind::of_file(n.as_bytes()), 0, s, Some(modified)),
                T::D(n, c) => d.dirs.push(to_dir(n, c)),
            }
        }
        d.finish(Kind::of_dir(name.as_bytes()));
        d
    }

    pub fn tree(kids: Vec<T>) -> Tree {
        tree_at("/r", kids)
    }

    pub fn tree_scanned_on(scanned_on: Day, kids: Vec<T>) -> Tree {
        let stats = ScanStats { scanned_on, ..ScanStats::default() };
        build(to_dir("root", kids), PathBuf::from("/r"), "root", false, vec![], stats)
    }

    pub fn descend(tree: &Tree, names: &[&str]) -> NodeId {
        names.iter().fold(tree.root(), |at, name| tree.children(at).find(|&c| tree.name(c) == *name).expect(name))
    }

    pub fn tree_at(root: &str, kids: Vec<T>) -> Tree {
        build(to_dir("root", kids), PathBuf::from(root), "root", false, vec![], ScanStats::default())
    }

    pub fn modified_on(path: &Path, days_since_1970: u16) {
        const AN_HOUR_INTO_THE_DAY: u64 = 3_600;
        let seconds = u64::from(days_since_1970) * crate::age::SECONDS_PER_DAY as u64 + AN_HOUR_INTO_THE_DAY;
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds)).unwrap();
    }

    pub fn scan_folder(root: &Path) -> Tree {
        crate::scan(&crate::ScanOptions::folder(root), &Default::default(), &Default::default()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_util::{T::*, descend, tree};
    use super::*;

    fn sample() -> Tree {
        tree(vec![
            F("small.txt", 10),
            D("a", vec![F("x.mov", 500), F("y.mov", 300), D("deep", vec![F("z.rs", 50)])]),
            D("b", vec![F("w.png", 100)]),
        ])
    }

    #[test]
    fn sizes_sum_up_and_children_are_listed_largest_first() {
        let t = sample();
        assert_eq!(t.size(t.root()), 960);
        let kids: Vec<_> = t.children(t.root()).map(|c| t.name(c).into_owned()).collect();
        assert_eq!(kids, vec!["a", "b", "small.txt"]);
        let a = t.children(t.root()).next().unwrap();
        assert_eq!(t.size(a), 850);
        assert_eq!(t.node(a).items(), 3, "items counts the files in the subtree, not folders");
        assert_eq!(t.node(a).kind(), Kind::Video, "a folder takes the kind holding most of its bytes");
        assert_eq!(t.node(t.root()).items(), 5);
    }

    #[test]
    fn equal_sized_children_keep_the_scanner_order_for_a_deterministic_layout() {
        let t = tree(vec![F("b", 5), F("a", 5), F("c", 5)]);
        let kids: Vec<_> = t.children(t.root()).map(|c| t.name(c).into_owned()).collect();
        assert_eq!(kids, vec!["b", "a", "c"], "ties break on scan order");
    }

    #[test]
    fn ids_are_breadth_first_so_parents_precede_children() {
        let t = sample();
        for id in t.ids() {
            if let Some(p) = t.parent(id) {
                assert!(p.0 < id.0, "parent {p:?} must have a smaller id than {id:?}");
            }
        }
    }

    #[test]
    fn paths_depths_and_ancestry() {
        let t = sample();
        let (a, z) = (descend(&t, &["a"]), descend(&t, &["a", "deep", "z.rs"]));
        assert_eq!(t.path(z), PathBuf::from("/r/a/deep/z.rs"));
        assert_eq!(t.path(t.root()), PathBuf::from("/r"));
        assert_eq!(t.depth(z), 3);
        assert!(t.is_ancestor_of(a, z));
        assert!(!t.is_ancestor_of(z, a));
        assert!(t.is_ancestor_of(a, a), "a node counts as its own ancestor");
        assert!(!t.is_strict_ancestor_of(a, a));
    }

    #[test]
    fn remove_shrinks_ancestors_and_moves_them_to_their_new_rank() {
        let mut t = sample();
        let a = t.children(t.root()).next().unwrap();
        let x = t.children(a).next().unwrap();
        let y = t.children(a).nth(1).unwrap();
        assert!(!t.remove(t.root()), "the root cannot be removed");
        assert!(t.remove(x));
        assert!(!t.remove(x), "removing twice is refused");
        assert!(t.remove(y));
        assert_eq!(t.size(a), 50);
        assert_eq!(t.size(t.root()), 160);
        assert_eq!(t.node(t.root()).items(), 3);
        let kids: Vec<_> = t.children(t.root()).map(|c| t.name(c).into_owned()).collect();
        assert_eq!(kids, vec!["b", "a", "small.txt"], "a shrank below b");
        assert_eq!(t.children(a).count(), 1);
    }

    #[test]
    fn a_folder_is_as_new_as_the_newest_file_anywhere_inside_it_and_removal_dates_it_again() {
        let day = Day::from_days_since_1970;
        let mut t = super::test_util::tree_scanned_on(
            day(20_000),
            vec![
                D("a", vec![Aged("x", 10, day(19_000)), D("deep", vec![Aged("z", 5, day(19_990))])]),
                D("empty", vec![]),
                F("undated", 1),
            ],
        );
        let (a, deep, z) = (descend(&t, &["a"]), descend(&t, &["a", "deep"]), descend(&t, &["a", "deep", "z"]));
        assert_eq!(t.newest_change(a), Some(day(19_990)), "the date comes up from the deepest file");
        assert_eq!(t.newest_change(t.root()), Some(day(19_990)));
        assert_eq!(t.newest_change(descend(&t, &["empty"])), None, "a folder with no files has no date");
        assert_eq!(t.days_since_newest_change(a), Some(10));
        assert_eq!(t.age(a), AgeBucket::UnderAMonth);
        assert_eq!(t.age(descend(&t, &["undated"])), AgeBucket::Unknown);

        assert!(t.remove(z));
        assert_eq!(t.newest_change(deep), None, "nothing dated is left in deep");
        assert_eq!(t.newest_change(a), Some(day(19_000)), "a is now as new as its remaining file");
        assert_eq!(t.age(a), AgeBucket::UnderFiveYears, "1000 days old");
    }

    #[test]
    fn a_folder_holds_the_kinds_of_everything_inside_and_a_bundle_is_its_own_kind_throughout() {
        let mut t = tree(vec![
            D(
                "Apps",
                vec![
                    D("Safari.app", vec![F("Safari", 100), D("Web.framework", vec![F("Web.dylib", 60)])]),
                    F("a.txt", 1),
                ],
            ),
            D("Movies", vec![F("a.mov", 10)]),
        ]);
        let at = |path: &[&str]| descend(&t, path);
        let (apps, safari, web) =
            (at(&["Apps"]), at(&["Apps", "Safari.app"]), at(&["Apps", "Safari.app", "Web.framework"]));
        let inside_apps = t.kinds_inside(apps);
        assert!(inside_apps.contains(Kind::App) && inside_apps.contains(Kind::Document));
        assert!(!inside_apps.contains(Kind::Library), "what is inside a bundle counts as the bundle");
        assert_eq!(t.kinds_inside(safari).only(), Some(Kind::App));
        assert_eq!(t.kinds_inside(web).only(), Some(Kind::App), "the outermost bundle decides");
        assert_eq!(t.kinds_inside(at(&["Apps", "Safari.app", "Safari"])), KindSet::of(Kind::App));
        assert_eq!(t.kinds_inside(at(&["Movies", "a.mov"])).only(), Some(Kind::Video), "a loose file is its own kind");

        let movies = at(&["Movies"]);
        assert!(t.remove(web));
        assert_eq!(t.kinds_inside(safari).only(), Some(Kind::App), "a bundle stays its own kind as its insides go");
        assert!(t.remove(safari));
        assert_eq!(t.kinds_inside(apps), KindSet::of(Kind::Document), "removal takes the app's kind away");
        assert!(t.remove(descend(&t, &["Movies", "a.mov"])));
        assert_eq!(t.kinds_inside(movies), KindSet::default(), "an emptied folder holds no kind");
        assert!(!t.kinds_inside(t.root()).contains(Kind::Video));
    }

    #[test]
    fn a_node_stays_32_bytes_because_its_date_is_kept_beside_it() {
        assert_eq!(std::mem::size_of::<Node>(), 32);
        assert_eq!(std::mem::size_of::<FileRec>(), 16);
        assert_eq!(std::mem::size_of::<Option<Day>>(), 2);
    }

    #[test]
    fn removing_a_folder_marks_its_whole_subtree_removed() {
        let mut t = sample();
        let (a, z) = (descend(&t, &["a"]), descend(&t, &["a", "deep", "z.rs"]));
        assert!(t.remove(a));
        assert!(t.node(z).is_removed(), "descendants of a removed folder are removed too");
        assert_eq!(t.nearest_live(z), t.root());
    }
}
