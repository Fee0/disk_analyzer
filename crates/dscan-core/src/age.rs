use std::num::NonZeroU16;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::exclusions::Exclusions;
use crate::tree::{NodeId, Tree};

pub(crate) const SECONDS_PER_DAY: i64 = 86_400;
const LAST_STORABLE_DAY_SINCE_1970: u16 = u16::MAX - 1;
const DAYS_PER_WEEK: u32 = 7;
const DAYS_PER_MONTH: u32 = 30;
const DAYS_PER_YEAR: u32 = 365;
const DAYS_COUNTED_AS_DAYS: u32 = 14;
const DAYS_COUNTED_AS_WEEKS: u32 = 61;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day(NonZeroU16);

impl Day {
    pub fn from_unix_seconds(seconds: impl Into<i64>) -> Day {
        let seconds: i64 = seconds.into();
        let days_since_1970 = seconds.div_euclid(SECONDS_PER_DAY).clamp(0, LAST_STORABLE_DAY_SINCE_1970.into());
        Day::from_days_since_1970(days_since_1970 as u16)
    }

    pub fn from_days_since_1970(days: u16) -> Day {
        Day(NonZeroU16::MIN.saturating_add(days.min(LAST_STORABLE_DAY_SINCE_1970)))
    }

    pub fn today() -> Day {
        let since_1970 = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
        Day::from_unix_seconds(i64::try_from(since_1970.as_secs()).unwrap_or(i64::MAX))
    }

    #[cfg(test)]
    pub(crate) fn days_since_1970(self) -> u16 {
        self.0.get() - 1
    }

    pub fn days_until(self, later: Day) -> u32 {
        u32::from(later.0.get().saturating_sub(self.0.get()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AgeBucket {
    UnderAMonth,
    UnderSixMonths,
    UnderAYear,
    UnderTwoYears,
    UnderFiveYears,
    FiveYearsOrMore,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgeUnit {
    Month,
    Year,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AgeSpan {
    pub count: u32,
    pub unit: AgeUnit,
}

const MONTHS_PER_YEAR: u32 = 12;

impl AgeSpan {
    const fn new(count: u32, unit: AgeUnit) -> AgeSpan {
        AgeSpan { count, unit }
    }

    fn days(self) -> u32 {
        self.count
            * match self.unit {
                AgeUnit::Month => DAYS_PER_MONTH,
                AgeUnit::Year => DAYS_PER_YEAR,
            }
    }

    pub fn months(self) -> u32 {
        match self.unit {
            AgeUnit::Month => self.count,
            AgeUnit::Year => self.count * MONTHS_PER_YEAR,
        }
    }
}

const BOUNDARIES: [AgeSpan; 5] = [
    AgeSpan::new(1, AgeUnit::Month),
    AgeSpan::new(6, AgeUnit::Month),
    AgeSpan::new(1, AgeUnit::Year),
    AgeSpan::new(2, AgeUnit::Year),
    AgeSpan::new(5, AgeUnit::Year),
];

impl AgeBucket {
    pub const COUNT: usize = 7;
    pub const ALL: [AgeBucket; AgeBucket::COUNT] = [
        AgeBucket::UnderAMonth,
        AgeBucket::UnderSixMonths,
        AgeBucket::UnderAYear,
        AgeBucket::UnderTwoYears,
        AgeBucket::UnderFiveYears,
        AgeBucket::FiveYearsOrMore,
        AgeBucket::Unknown,
    ];

    pub fn of(modified: Option<Day>, today: Day) -> AgeBucket {
        let Some(modified) = modified else { return AgeBucket::Unknown };
        let age = modified.days_until(today);
        let younger_than_its_end = |bucket: &AgeBucket| bucket.until().is_none_or(|end| age < end.days());
        AgeBucket::ALL.into_iter().find(younger_than_its_end).unwrap_or(AgeBucket::FiveYearsOrMore)
    }

    pub fn from(self) -> Option<AgeSpan> {
        BOUNDARIES.get(self.index().checked_sub(1)?).copied()
    }

    pub fn until(self) -> Option<AgeSpan> {
        BOUNDARIES.get(self.index()).copied()
    }

    pub fn is_at_least(self, threshold: AgeBucket) -> bool {
        self != AgeBucket::Unknown && self >= threshold
    }

    pub fn stays_lit(self, aged_at_least: Option<AgeBucket>) -> bool {
        aged_at_least.is_none_or(|oldest| self.is_at_least(oldest))
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

pub fn describe_age(days: u32) -> String {
    let counted = |n: u32, unit: &str| if n == 1 { format!("1 {unit} ago") } else { format!("{n} {unit}s ago") };
    match days {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        d if d < DAYS_COUNTED_AS_DAYS => counted(d, "day"),
        d if d < DAYS_COUNTED_AS_WEEKS => counted(d / DAYS_PER_WEEK, "week"),
        d if d < DAYS_PER_YEAR => counted(d / DAYS_PER_MONTH, "month"),
        d => counted(d / DAYS_PER_YEAR, "year"),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AgeBreakdown {
    bytes: [u64; AgeBucket::COUNT],
}

impl AgeBreakdown {
    pub fn bytes(&self, bucket: AgeBucket) -> u64 {
        self.bytes[bucket.index()]
    }

    pub fn bytes_at_least(&self, threshold: AgeBucket) -> u64 {
        AgeBucket::ALL.iter().filter(|b| b.is_at_least(threshold)).map(|&b| self.bytes(b)).sum()
    }

    pub fn total(&self) -> u64 {
        self.bytes.iter().sum()
    }
}

pub fn age_breakdown(tree: &Tree, focus: NodeId, excluded: &Exclusions) -> AgeBreakdown {
    let mut breakdown = AgeBreakdown::default();
    let mut folders = vec![focus];
    while let Some(folder) = folders.pop() {
        for (child, size) in excluded.children_with_sizes(tree, folder) {
            let node = tree.node(child);
            if node.is_dir() {
                folders.push(child);
            } else if !node.is_synthetic() {
                breakdown.bytes[tree.age(child).index()] += size;
            }
        }
    }
    breakdown
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::test_util::{T::*, descend, tree_scanned_on};

    const TODAY: u16 = 20_000;

    fn day(days_ago: u16) -> Day {
        Day::from_days_since_1970(TODAY - days_ago)
    }

    #[test]
    fn a_day_counts_whole_days_since_1970_and_clamps_what_it_cannot_store() {
        assert_eq!(Day::from_unix_seconds(0).days_since_1970(), 0);
        assert_eq!(
            Day::from_unix_seconds(SECONDS_PER_DAY - 1).days_since_1970(),
            0,
            "the first day lasts until midnight"
        );
        assert_eq!(Day::from_unix_seconds(SECONDS_PER_DAY).days_since_1970(), 1);
        assert_eq!(Day::from_unix_seconds(-1).days_since_1970(), 0, "before 1970 counts as its first day");
        assert_eq!(Day::from_unix_seconds(i64::MAX).days_since_1970(), LAST_STORABLE_DAY_SINCE_1970);
        assert_eq!(Day::from_days_since_1970(u16::MAX).days_since_1970(), LAST_STORABLE_DAY_SINCE_1970);
        assert_eq!(day(10).days_until(day(0)), 10);
        assert_eq!(day(0).days_until(day(10)), 0, "a date after today is no age, not a negative one");
    }

    #[test]
    fn each_bucket_starts_on_its_boundary_day() {
        let bucket = |days_ago: u16| AgeBucket::of(Some(day(days_ago)), day(0));
        use AgeBucket::*;
        let boundaries = [
            (0, UnderAMonth),
            (29, UnderAMonth),
            (30, UnderSixMonths),
            (179, UnderSixMonths),
            (180, UnderAYear),
            (364, UnderAYear),
            (365, UnderTwoYears),
            (729, UnderTwoYears),
            (730, UnderFiveYears),
            (1824, UnderFiveYears),
            (1825, FiveYearsOrMore),
            (15_000, FiveYearsOrMore),
        ];
        for (days_ago, expected) in boundaries {
            assert_eq!(bucket(days_ago), expected, "{days_ago} days ago");
        }
        assert_eq!(AgeBucket::of(None, day(0)), Unknown);
        assert_eq!(AgeBucket::ALL.map(AgeBucket::index), [0, 1, 2, 3, 4, 5, 6], "ALL is in index order");
    }

    #[test]
    fn each_bucket_starts_where_the_one_before_it_ends() {
        use AgeBucket::*;
        let month = |count| AgeSpan { count, unit: AgeUnit::Month };
        let year = |count| AgeSpan { count, unit: AgeUnit::Year };
        assert_eq!((UnderAMonth.from(), UnderAMonth.until()), (None, Some(month(1))));
        assert_eq!((UnderAYear.from(), UnderAYear.until()), (Some(month(6)), Some(year(1))));
        assert_eq!((FiveYearsOrMore.from(), FiveYearsOrMore.until()), (Some(year(5)), None));
        assert_eq!((Unknown.from(), Unknown.until()), (None, None), "an unknown age has no range");
        for pair in AgeBucket::ALL[..AgeBucket::COUNT - 1].windows(2) {
            assert_eq!(pair[0].until(), pair[1].from(), "{pair:?} meet without a gap");
        }
        assert_eq!(year(1).months(), 12);
    }

    #[test]
    fn at_least_a_bucket_means_that_old_or_older_and_never_an_unknown_age() {
        use AgeBucket::*;
        assert!(UnderFiveYears.is_at_least(UnderTwoYears));
        assert!(UnderTwoYears.is_at_least(UnderTwoYears));
        assert!(!UnderAYear.is_at_least(UnderTwoYears));
        assert!(!Unknown.is_at_least(UnderAMonth), "an unknown age is not known to be old");
        assert!(Unknown.stays_lit(None), "nothing is dimmed without an age chosen");
        assert!(!UnderAYear.stays_lit(Some(UnderTwoYears)));
    }

    #[test]
    fn ages_read_in_the_largest_whole_unit() {
        let cases = [
            (0, "today"),
            (1, "yesterday"),
            (2, "2 days ago"),
            (13, "13 days ago"),
            (14, "2 weeks ago"),
            (60, "8 weeks ago"),
            (61, "2 months ago"),
            (364, "12 months ago"),
            (365, "1 year ago"),
            (1100, "3 years ago"),
        ];
        for (days, expected) in cases {
            assert_eq!(describe_age(days), expected);
        }
    }

    #[test]
    fn the_breakdown_counts_every_file_s_bytes_in_its_own_bucket_and_skips_excluded_ones() {
        let t = tree_scanned_on(
            day(0),
            vec![
                Aged("new", 100, day(3)),
                D("old", vec![Aged("older", 400, day(1000)), Aged("oldest", 200, day(4000))]),
                F("undated", 50),
            ],
        );
        let all = age_breakdown(&t, t.root(), &Exclusions::default());
        assert_eq!(all.bytes(AgeBucket::UnderAMonth), 100);
        assert_eq!(all.bytes(AgeBucket::UnderFiveYears), 400);
        assert_eq!(all.bytes(AgeBucket::FiveYearsOrMore), 200);
        assert_eq!(all.bytes(AgeBucket::Unknown), 50);
        assert_eq!(all.total(), t.size(t.root()), "every byte lands in one bucket");
        assert_eq!(all.bytes_at_least(AgeBucket::UnderTwoYears), 600, "unknown is not counted as old");

        let oldest = descend(&t, &["old", "oldest"]);
        let without_oldest = age_breakdown(&t, t.root(), &Exclusions::new(&t, &[oldest]));
        assert_eq!(without_oldest.bytes(AgeBucket::FiveYearsOrMore), 0);
        let inside_old = age_breakdown(&t, descend(&t, &["old"]), &Exclusions::default());
        assert_eq!(inside_old.total(), 600, "only the focused folder is broken down");
    }
}
