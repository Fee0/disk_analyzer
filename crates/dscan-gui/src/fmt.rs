//! Small text formatting helpers shared by the views and the chart.

/// `1234567` -> `"1,234,567"`.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Cut `s` to at most `max` characters, ending in `…` when shortened.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats() {
        assert_eq!(super::thousands(1234567), "1,234,567");
        assert_eq!(super::truncate("abcdef", 4), "abc…");
    }
}
