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

pub fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        s.chars().take(max_chars.saturating_sub(1)).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn thousands_groups_digits_with_commas() {
        assert_eq!(super::thousands(1234567), "1,234,567");
    }

    #[test]
    fn truncate_ends_shortened_text_with_an_ellipsis_within_max_chars() {
        assert_eq!(super::truncate("abcdef", 4), "abc…");
        assert_eq!(super::truncate("abcd", 4), "abcd", "text that fits is kept as is");
    }
}
