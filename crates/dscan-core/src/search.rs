//! Name search over the whole tree.

use rayon::prelude::*;

use crate::scan::CancelToken;
use crate::tree::{NodeId, Tree};

const MATCH: u8 = 1;
const CONTAINS: u8 = 2;

/// Result of a search: which nodes match and which contain a match.
#[derive(Clone, Debug, Default)]
pub struct SearchResult {
    /// The trimmed query.
    pub query: String,
    flags: Vec<u8>,
    /// Number of matching (live) nodes.
    pub matches: usize,
}

impl SearchResult {
    /// The node's own name matches.
    #[inline]
    pub fn is_match(&self, id: NodeId) -> bool {
        self.flags.get(id.idx()).is_some_and(|f| f & MATCH != 0)
    }
    /// The node matches or has a matching descendant.
    #[inline]
    pub fn is_relevant(&self, id: NodeId) -> bool {
        self.flags.get(id.idx()).is_some_and(|f| f & (MATCH | CONTAINS) != 0)
    }
}

/// ASCII case-insensitive substring test. `needle` must be lowercase.
#[inline]
fn contains_ignore_case(hay: &[u8], needle: &[u8]) -> bool {
    if needle.len() > hay.len() {
        return false;
    }
    let (lo, up) = (needle[0], needle[0].to_ascii_uppercase());
    let last_start = hay.len() - needle.len();
    let mut from = 0;
    while from <= last_start {
        let Some(i) = memchr::memchr2(lo, up, &hay[from..=last_start]) else { return false };
        let at = from + i;
        if hay[at + 1..at + needle.len()].eq_ignore_ascii_case(&needle[1..]) {
            return true;
        }
        from = at + 1;
    }
    false
}

/// Case-insensitive substring search over all node names.
///
/// Returns `None` for a blank query or when `cancel` fires mid-search.
pub fn search(tree: &Tree, query: &str, cancel: &CancelToken) -> Option<SearchResult> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let needle = query.to_ascii_lowercase().into_bytes();
    let mut flags = vec![0u8; tree.len()];
    flags.par_chunks_mut(1 << 16).enumerate().for_each(|(chunk, out)| {
        if cancel.is_cancelled() {
            return;
        }
        let base = chunk << 16;
        for (j, f) in out.iter_mut().enumerate() {
            let id = NodeId((base + j) as u32);
            let n = tree.node(id);
            if id != tree.root()
                && !n.is_removed()
                && !n.is_synthetic()
                && contains_ignore_case(tree.name_bytes(id), &needle)
            {
                *f = MATCH;
            }
        }
    });
    if cancel.is_cancelled() {
        return None;
    }
    let matched: Vec<u32> = flags
        .par_iter()
        .enumerate()
        .filter(|(_, f)| **f & MATCH != 0)
        .map(|(i, _)| i as u32)
        .collect();
    // Mark ancestors, stopping at ones already marked.
    for &i in &matched {
        let mut cur = tree.parent(NodeId(i));
        while let Some(p) = cur {
            if flags[p.idx()] & CONTAINS != 0 {
                break;
            }
            flags[p.idx()] |= CONTAINS;
            cur = tree.parent(p);
        }
    }
    let matches = matched.len();
    Some(SearchResult { query: query.to_owned(), flags, matches })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Metric;
    use crate::tree::test_util::{T::*, tree};

    #[test]
    fn finds_and_marks_ancestors() {
        let t = tree(vec![
            D("Movies", vec![D("2015", vec![F("IMG_7114.MOV", 10)]), F("other.txt", 1)]),
            D("Docs", vec![F("a.pdf", 5)]),
        ]);
        let r = search(&t, " img_71 ", &CancelToken::default()).unwrap();
        assert_eq!(r.query, "img_71");
        assert_eq!(r.matches, 1);
        let movies = t.children(t.root(), Metric::Allocated).find(|&c| t.name(c) == "Movies").unwrap();
        let docs = t.children(t.root(), Metric::Allocated).find(|&c| t.name(c) == "Docs").unwrap();
        assert!(r.is_relevant(movies) && !r.is_match(movies));
        assert!(!r.is_relevant(docs));
        assert!(r.is_relevant(t.root()));
        assert!(search(&t, "  ", &CancelToken::default()).is_none());
        let c = CancelToken::default();
        c.cancel();
        assert!(search(&t, "img", &c).is_none());
    }
}
