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
    line_insert_str(text, cursor, &ch.to_string());
}

/// Insert `s` at the caret. `cursor` advances by the number of characters.
pub fn line_insert_str(text: &mut String, cursor: &mut usize, s: &str) {
    if s.is_empty() {
        return;
    }
    let at = line_clamp(text, *cursor);
    let byte = byte_at(text, at);
    text.insert_str(byte, s);
    *cursor = at + s.chars().count();
}

/// Clipboard or bracketed-paste text. Newlines stay when `multiline` is set.
/// A carriage return becomes a newline in that case. Other controls, including
/// Escape, are dropped so the paste cannot run as keystrokes.
pub fn sanitize_paste(raw: &str, multiline: bool) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if multiline {
                    out.push('\n');
                }
            }
            '\n' => {
                if multiline {
                    out.push('\n');
                }
            }
            '\t' => out.push(if multiline { '\t' } else { ' ' }),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Hex field paste: keep `#` and hex digits, and write letters in uppercase.
pub fn sanitize_hex_paste(raw: &str) -> String {
    sanitize_paste(raw, false)
        .chars()
        .filter(|c| c.is_ascii_hexdigit() || *c == '#')
        .map(|c| c.to_ascii_uppercase())
        .collect()
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
    fn paste_keeps_newlines_and_drops_controls() {
        assert_eq!(sanitize_paste("a\r\nb\r\u{1b}c\td", true), "a\nb\nc\td");
        assert_eq!(sanitize_paste("a\r\nb\tc", false), "ab c");
        assert_eq!(sanitize_hex_paste("zz #ab\ncd"), "#ABCD");
        let mut text = "hi".to_string();
        let mut cursor = 1;
        line_insert_str(&mut text, &mut cursor, "XY");
        assert_eq!((text.as_str(), cursor), ("hXYi", 3));
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
