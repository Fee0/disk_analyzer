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

pub fn items_phrase(count: usize) -> String {
    if count == 1 { "1 item".to_string() } else { format!("{} items", thousands(count as u64)) }
}

pub fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        s.chars().take(max_chars.saturating_sub(1)).collect::<String>() + "…"
    }
}

const GENEROUS_CHAR_WIDTH_PER_TEXT_SIZE: f32 = 0.55;

pub fn generous_text_width(s: &str, text_size: f32) -> f32 {
    s.chars().count() as f32 * text_size * GENEROUS_CHAR_WIDTH_PER_TEXT_SIZE
}

fn chars_that_fit(width: f32, text_size: f32) -> usize {
    (width / (text_size * GENEROUS_CHAR_WIDTH_PER_TEXT_SIZE)).floor().max(1.0) as usize
}

pub fn fit_width(s: &str, width: f32, text_size: f32) -> String {
    truncate(s, chars_that_fit(width, text_size))
}

pub fn fit_width_keeping_both_ends(s: &str, width: f32, text_size: f32) -> String {
    let fits = chars_that_fit(width, text_size);
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= fits {
        return s.to_string();
    }
    let kept = fits.saturating_sub(1);
    let head = kept / 2;
    let tail = kept - head;
    chars[..head].iter().chain(['…'].iter()).chain(chars[chars.len() - tail..].iter()).collect()
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

    #[test]
    fn fit_width_keeps_text_that_fits_and_ellipsizes_text_that_does_not() {
        let name = "a-folder-with-a-long-name";
        let room_for_it = super::generous_text_width(name, 13.0) + 1.0;
        assert_eq!(super::fit_width(name, room_for_it, 13.0), name);
        let half = super::fit_width(name, room_for_it / 2.0, 13.0);
        assert!(half.ends_with('…') && half.chars().count() <= name.chars().count() / 2, "{half}");
        assert_eq!(super::fit_width(name, 0.0, 13.0), "…", "no room still shows that something is there");
    }

    #[test]
    fn fit_width_keeping_both_ends_shows_where_a_path_starts_and_which_folder_it_ends_in() {
        let path = r"C:\Users\someone\AppData\Local\Temp\project";
        let room_for_twenty_chars = super::generous_text_width("x", 12.0) * 20.0 + 1.0;
        let fitted = super::fit_width_keeping_both_ends(path, room_for_twenty_chars, 12.0);
        assert_eq!(fitted, r"C:\Users\…mp\project");
        assert_eq!(super::fit_width_keeping_both_ends(path, 1000.0, 12.0), path, "a path that fits is kept whole");
    }
}
