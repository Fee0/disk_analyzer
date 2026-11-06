use rayon::prelude::*;

use crate::scan::CancelToken;
use crate::tree::{NodeId, Tree};

const NAME_MATCHES: u8 = 1;
const HAS_MATCHING_DESCENDANT: u8 = 2;
const NODES_PER_PARALLEL_CHUNK: usize = 1 << 16;

#[derive(Clone, Debug, Default)]
pub struct SearchResult {
    pub query: String,
    match_flags: Vec<u8>,
    pub matches: usize,
}

impl SearchResult {
    #[inline]
    pub fn is_match(&self, id: NodeId) -> bool {
        self.match_flags.get(id.idx()).is_some_and(|f| f & NAME_MATCHES != 0)
    }
    #[inline]
    pub fn is_relevant(&self, id: NodeId) -> bool {
        self.match_flags.get(id.idx()).is_some_and(|f| f & (NAME_MATCHES | HAS_MATCHING_DESCENDANT) != 0)
    }
}

#[inline]
fn contains_ascii_ignore_case(hay: &[u8], lowercase_needle: &[u8]) -> bool {
    if lowercase_needle.len() > hay.len() {
        return false;
    }
    let (first_lower, first_upper) = (lowercase_needle[0], lowercase_needle[0].to_ascii_uppercase());
    let rest = &lowercase_needle[1..];
    let last_start = hay.len() - lowercase_needle.len();
    let mut from = 0;
    while from <= last_start {
        let Some(i) = memchr::memchr2(first_lower, first_upper, &hay[from..=last_start]) else { return false };
        let at = from + i;
        if hay[at + 1..at + lowercase_needle.len()].eq_ignore_ascii_case(rest) {
            return true;
        }
        from = at + 1;
    }
    false
}

pub fn search(tree: &Tree, query: &str, cancel: &CancelToken) -> Option<SearchResult> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let lowercase_needle = query.to_ascii_lowercase().into_bytes();
    let mut match_flags = vec![0u8; tree.len()];
    match_flags.par_chunks_mut(NODES_PER_PARALLEL_CHUNK).enumerate().for_each(|(chunk, out)| {
        if cancel.is_cancelled() {
            return;
        }
        let base = chunk * NODES_PER_PARALLEL_CHUNK;
        for (j, f) in out.iter_mut().enumerate() {
            let id = NodeId((base + j) as u32);
            let n = tree.node(id);
            if id != tree.root()
                && !n.is_removed()
                && !n.is_synthetic()
                && contains_ascii_ignore_case(tree.name_bytes(id), &lowercase_needle)
            {
                *f = NAME_MATCHES;
            }
        }
    });
    if cancel.is_cancelled() {
        return None;
    }
    let matched: Vec<u32> =
        match_flags.par_iter().enumerate().filter(|(_, f)| **f & NAME_MATCHES != 0).map(|(i, _)| i as u32).collect();
    mark_ancestors_of_matches(tree, &mut match_flags, &matched);
    let matches = matched.len();
    Some(SearchResult { query: query.to_owned(), match_flags, matches })
}

fn mark_ancestors_of_matches(tree: &Tree, match_flags: &mut [u8], matched: &[u32]) {
    for &i in matched {
        let mut cur = tree.parent(NodeId(i));
        while let Some(p) = cur {
            let ancestors_already_marked_by_an_earlier_match = match_flags[p.idx()] & HAS_MATCHING_DESCENDANT != 0;
            if ancestors_already_marked_by_an_earlier_match {
                break;
            }
            match_flags[p.idx()] |= HAS_MATCHING_DESCENDANT;
            cur = tree.parent(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::Metric;
    use crate::tree::test_util::{T::*, tree};

    #[test]
    fn finds_case_insensitively_and_marks_ancestors() {
        let t = tree(vec![
            D("Movies", vec![D("2015", vec![F("IMG_7114.MOV", 10)]), F("other.txt", 1)]),
            D("Docs", vec![F("a.pdf", 5)]),
        ]);
        let r = search(&t, " img_71 ", &CancelToken::default()).unwrap();
        assert_eq!(r.query, "img_71", "the query is stored trimmed");
        assert_eq!(r.matches, 1);
        let movies = t.children(t.root(), Metric::Allocated).find(|&c| t.name(c) == "Movies").unwrap();
        let docs = t.children(t.root(), Metric::Allocated).find(|&c| t.name(c) == "Docs").unwrap();
        assert!(r.is_relevant(movies) && !r.is_match(movies), "an ancestor of a match is relevant, not a match");
        assert!(!r.is_relevant(docs));
        assert!(r.is_relevant(t.root()));
        assert!(search(&t, "  ", &CancelToken::default()).is_none(), "a blank query gives no result");
        let c = CancelToken::default();
        c.cancel();
        assert!(search(&t, "img", &c).is_none(), "a cancelled search gives no result");
    }

    #[test]
    fn skips_contents_of_removed_folders() {
        let mut t = tree(vec![D("Movies", vec![D("2015", vec![F("IMG_1.MOV", 10)])]), F("img.txt", 1)]);
        let movies = t.children(t.root(), Metric::Allocated).find(|&c| t.name(c) == "Movies").unwrap();
        assert!(t.remove(movies));
        let r = search(&t, "img", &CancelToken::default()).unwrap();
        assert_eq!(r.matches, 1, "only the live img.txt matches");
        assert!(!r.is_relevant(movies));
    }
}
