use crate::age::{AgeBucket, Day};
use crate::search::{SearchResult, is_relevant_under};
use crate::tree::{NodeId, Tree};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColorBy {
    #[default]
    Kind,
    Age,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Paint<'a> {
    pub color_by: ColorBy,
    pub search: Option<&'a SearchResult>,
    pub aged_at_least: Option<AgeBucket>,
}

impl ColorBy {
    pub fn shown_age(self, tree: &Tree, id: NodeId) -> Option<AgeBucket> {
        let has_an_age = !tree.node(id).is_synthetic();
        (self == ColorBy::Age && has_an_age).then(|| tree.age(id))
    }

    pub(crate) fn shown_age_of_newest(self, tree: &Tree, newest: Option<Day>) -> Option<AgeBucket> {
        (self == ColorBy::Age).then(|| AgeBucket::of(newest, tree.stats.scanned_on))
    }
}

impl Paint<'_> {
    pub(crate) fn lights(&self, tree: &Tree, id: NodeId) -> bool {
        let old_enough = || self.aged_at_least.is_none() || tree.age(id).stays_lit(self.aged_at_least);
        is_relevant_under(self.search, id) && old_enough()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_util::{T::*, descend, tree_scanned_on};

    #[test]
    fn a_node_stays_lit_only_when_it_matches_the_search_and_is_old_enough() {
        let today = Day::from_days_since_1970(20_000);
        let t = tree_scanned_on(
            today,
            vec![
                Aged("old-report", 10, Day::from_days_since_1970(18_000)),
                Aged("new-report", 10, today),
                Aged("old-photo", 10, Day::from_days_since_1970(18_000)),
                F("undated-report", 10),
            ],
        );
        let lit = |paint: Paint<'_>| -> Vec<&str> {
            ["old-report", "new-report", "old-photo", "undated-report"]
                .into_iter()
                .filter(|name| paint.lights(&t, descend(&t, &[name])))
                .collect()
        };
        assert_eq!(lit(Paint::default()).len(), 4, "nothing is dimmed without a search or an age");

        let reports = crate::search::search(&t, "report", &Default::default()).unwrap();
        let older_than_two_years = Some(AgeBucket::UnderFiveYears);
        assert_eq!(lit(Paint { aged_at_least: older_than_two_years, ..Paint::default() }), ["old-report", "old-photo"]);
        let both = Paint { search: Some(&reports), aged_at_least: older_than_two_years, ..Paint::default() };
        assert_eq!(lit(both), ["old-report"]);
    }
}
