//! Caret for a single-line field. `cursor` is a character index, `0..=len`.

pub fn line_len(text: &str) -> usize {
    text.chars().count()
}

pub fn line_clamp(text: &str, cursor: usize) -> usize {
    cursor.min(line_len(text))
}

fn byte_at(text: &str, cursor: usize) -> usize {
    match text.char_indices().nth(cursor) {
        Some((byte, _)) => byte,
        None => text.len(),
    }
}

pub fn line_insert(text: &mut String, cursor: &mut usize, ch: char) {
    let at = line_clamp(text, *cursor);
    let byte = byte_at(text, at);
    text.insert(byte, ch);
    *cursor = at + 1;
}

pub fn line_backspace(text: &mut String, cursor: &mut usize) {
    let at = line_clamp(text, *cursor);
    if at == 0 {
        *cursor = 0;
        return;
    }
    let start = byte_at(text, at - 1);
    let end = byte_at(text, at);
    text.replace_range(start..end, "");
    *cursor = at - 1;
}

pub fn line_delete(text: &mut String, cursor: &mut usize) {
    let at = line_clamp(text, *cursor);
    let n = line_len(text);
    if at >= n {
        *cursor = n;
        return;
    }
    let start = byte_at(text, at);
    let end = byte_at(text, at + 1);
    text.replace_range(start..end, "");
    *cursor = at;
}

pub fn line_left(cursor: &mut usize) {
    *cursor = cursor.saturating_sub(1);
}

pub fn line_right(text: &str, cursor: &mut usize) {
    let n = line_len(text);
    if *cursor < n {
        *cursor += 1;
    } else {
        *cursor = n;
    }
}

pub fn line_home(cursor: &mut usize) {
    *cursor = 0;
}

pub fn line_end(text: &str, cursor: &mut usize) {
    *cursor = line_len(text);
}

/// Column from the start of the editable text. Past the end stays at the end.
pub fn line_click(text: &str, col: usize) -> usize {
    col.min(line_len(text))
}

/// `(before, cell, after)`. The cell is the character under the caret, or a
/// block when the caret is at the end. A masked field shows bullets.
pub fn line_parts(text: &str, cursor: usize, mask: bool) -> (String, String, String) {
    let shown = if mask {
        "•".repeat(line_len(text))
    } else {
        text.to_string()
    };
    let at = cursor.min(line_len(&shown));
    let mut chars = shown.chars();
    let before: String = chars.by_ref().take(at).collect();
    match chars.next() {
        Some(ch) => (before, ch.to_string(), chars.collect()),
        None => (before, "█".to_string(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_backspace_and_delete_follow_the_caret() {
        let mut text = String::new();
        let mut cursor = 0;
        line_insert(&mut text, &mut cursor, 'a');
        line_insert(&mut text, &mut cursor, 'b');
        line_insert(&mut text, &mut cursor, 'c');
        assert_eq!(text, "abc");
        assert_eq!(cursor, 3);
        line_left(&mut cursor);
        line_left(&mut cursor);
        line_insert(&mut text, &mut cursor, 'X');
        assert_eq!((text.as_str(), cursor), ("aXbc", 2));
        line_backspace(&mut text, &mut cursor);
        assert_eq!((text.as_str(), cursor), ("abc", 1));
        line_delete(&mut text, &mut cursor);
        assert_eq!((text.as_str(), cursor), ("ac", 1));
        line_home(&mut cursor);
        line_backspace(&mut text, &mut cursor);
        assert_eq!((text.as_str(), cursor), ("ac", 0));
        line_end(&text, &mut cursor);
        line_delete(&mut text, &mut cursor);
        assert_eq!((text.as_str(), cursor), ("ac", 2));
    }

    #[test]
    fn click_and_unicode_stay_on_character_boundaries() {
        let mut text = "éé".to_string();
        let mut cursor = 0;
        line_insert(&mut text, &mut cursor, 'Z');
        assert_eq!(text, "Zéé");
        assert_eq!(line_click(&text, 99), 3);
        cursor = line_click(&text, 2);
        line_backspace(&mut text, &mut cursor);
        assert_eq!((text.as_str(), cursor), ("Zé", 1));
        let (before, cell, after) = line_parts("ab", 1, false);
        assert_eq!(
            (before.as_str(), cell.as_str(), after.as_str()),
            ("a", "b", "")
        );
        let (before, cell, after) = line_parts("ab", 2, false);
        assert_eq!(
            (before.as_str(), cell.as_str(), after.as_str()),
            ("ab", "█", "")
        );
        let (before, cell, after) = line_parts("ab", 1, true);
        assert_eq!(
            (before.as_str(), cell.as_str(), after.as_str()),
            ("•", "•", "")
        );
    }
}
