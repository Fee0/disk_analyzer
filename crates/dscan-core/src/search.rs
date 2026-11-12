use memchr::memmem;
use rayon::prelude::*;
use unicode_normalization::UnicodeNormalization;

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

fn decomposed_lowercase(text: &str) -> String {
    text.nfd().flat_map(char::to_lowercase).nfd().collect()
}

enum Needle {
    Ascii(Vec<u8>),
    Unicode(Box<memmem::Finder<'static>>),
}

impl Needle {
    fn new(query: &str) -> Needle {
        let folded = decomposed_lowercase(query);
        if folded.is_ascii() {
            Needle::Ascii(folded.into_bytes())
        } else {
            Needle::Unicode(Box::new(memmem::Finder::new(folded.as_bytes()).into_owned()))
        }
    }

    #[inline]
    fn is_in(&self, name: &[u8]) -> bool {
        match self {
            Needle::Ascii(lowercase) => contains_ascii_ignore_case(name, lowercase),
            Needle::Unicode(_) if name.is_ascii() => false,
            Needle::Unicode(folded) => {
                folded.find(decomposed_lowercase(&String::from_utf8_lossy(name)).as_bytes()).is_some()
            }
        }
    }
}

pub fn search(tree: &Tree, query: &str, cancel: &CancelToken) -> Option<SearchResult> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let needle = Needle::new(query);
    let mut match_flags = vec![0u8; tree.len()];
    match_flags.par_chunks_mut(NODES_PER_PARALLEL_CHUNK).enumerate().for_each(|(chunk, out)| {
        if cancel.is_cancelled() {
            return;
        }
        let base = chunk * NODES_PER_PARALLEL_CHUNK;
        for (j, f) in out.iter_mut().enumerate() {
            let id = NodeId((base + j) as u32);
            let n = tree.node(id);
            if id != tree.root() && !n.is_removed() && !n.is_synthetic() && needle.is_in(tree.name_bytes(id)) {
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
    use crate::tree::test_util::{T::*, descend, tree};

    #[test]
    fn finds_case_insensitively_and_marks_ancestors() {
        let t = tree(vec![
            D("Movies", vec![D("2015", vec![F("IMG_7114.MOV", 10)]), F("other.txt", 1)]),
            D("Docs", vec![F("a.pdf", 5)]),
        ]);
        let r = search(&t, " img_71 ", &CancelToken::default()).unwrap();
        assert_eq!(r.query, "img_71", "the query is stored trimmed");
        assert_eq!(r.matches, 1);
        let (movies, docs) = (descend(&t, &["Movies"]), descend(&t, &["Docs"]));
        assert!(r.is_relevant(movies) && !r.is_match(movies), "an ancestor of a match is relevant, not a match");
        assert!(!r.is_relevant(docs));
        assert!(r.is_relevant(t.root()));
        assert!(search(&t, "  ", &CancelToken::default()).is_none(), "a blank query gives no result");
        let c = CancelToken::default();
        c.cancel();
        assert!(search(&t, "img", &c).is_none(), "a cancelled search gives no result");
    }

    #[test]
    fn finds_names_with_umlauts_and_accents_whatever_their_case_and_however_they_are_composed() {
        let decomposed_as_macos_often_stores_it = "Cafe\u{301} Mu\u{308}nchen.txt";
        let t = tree(vec![
            F("ÜBERSICHT.pdf", 3),
            F("Größe.txt", 2),
            F(decomposed_as_macos_often_stores_it, 1),
            F("plain.txt", 1),
        ]);
        let matches = |query: &str| search(&t, query, &CancelToken::default()).unwrap().matches;
        assert_eq!(matches("übersicht"), 1, "a lowercase query finds an uppercase umlaut");
        assert_eq!(matches("Ü"), 2, "ÜBERSICHT and the decomposed ü of München");
        assert_eq!(matches("café"), 1, "a composed query finds a decomposed name");
        assert_eq!(matches("GRÖ"), 1);
        assert_eq!(matches("ж"), 0, "plain ASCII names never match a non-ASCII query");
        assert_eq!(matches("PLAIN"), 1, "ASCII queries keep working");
    }

    #[test]
    fn skips_contents_of_removed_folders() {
        let mut t = tree(vec![D("Movies", vec![D("2015", vec![F("IMG_1.MOV", 10)])]), F("img.txt", 1)]);
        let movies = descend(&t, &["Movies"]);
        assert!(t.remove(movies));
        let r = search(&t, "img", &CancelToken::default()).unwrap();
        assert_eq!(r.matches, 1, "only the live img.txt matches");
        assert!(!r.is_relevant(movies));
    }
}
