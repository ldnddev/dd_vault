//! Syntect markdown highlighting as byte ranges on one visual row.

use std::ops::Range;
use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaintSpan {
    pub bytes: Range<usize>,
    pub color: (u8, u8, u8),
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub background: Option<(u8, u8, u8)>,
}

struct Syntect {
    syntaxes: SyntaxSet,
    themes: ThemeSet,
}

fn syntect() -> &'static Syntect {
    static CELL: OnceLock<Syntect> = OnceLock::new();
    CELL.get_or_init(|| Syntect {
        syntaxes: SyntaxSet::load_defaults_newlines(),
        themes: ThemeSet::load_defaults(),
    })
}

/// Highlight one display row. The ranges cover `line` with no gaps.
pub fn highlight_line(line: &str) -> Vec<PaintSpan> {
    if line.is_empty() {
        return Vec::new();
    }
    let st = syntect();
    let syntax = st
        .syntaxes
        .find_syntax_by_extension("md")
        .unwrap_or_else(|| st.syntaxes.find_syntax_plain_text());
    let Some(theme) = st
        .themes
        .themes
        .get("base16-ocean.dark")
        .or_else(|| st.themes.themes.values().next())
    else {
        return vec![plain(0..line.len())];
    };
    let mut highlighter = HighlightLines::new(syntax, theme);
    let with_nl = format!("{line}\n");
    let ranges = match highlighter.highlight_line(&with_nl, &st.syntaxes) {
        Ok(ranges) => ranges,
        Err(_) => return vec![plain(0..line.len())],
    };
    let mut spans = Vec::new();
    let mut at = 0usize;
    for (style, text) in ranges {
        let text = text.trim_end_matches('\n');
        if text.is_empty() {
            continue;
        }
        let end = at + text.len();
        if end > line.len() || &line[at..end] != text {
            return vec![plain(0..line.len())];
        }
        spans.push(PaintSpan {
            bytes: at..end,
            color: (style.foreground.r, style.foreground.g, style.foreground.b),
            bold: style.font_style.contains(FontStyle::BOLD),
            italic: style.font_style.contains(FontStyle::ITALIC),
            underline: style.font_style.contains(FontStyle::UNDERLINE),
            background: None,
        });
        at = end;
    }
    if at < line.len() {
        spans.push(plain(at..line.len()));
    }
    if spans.is_empty() {
        spans.push(plain(0..line.len()));
    }
    spans
}

fn plain(bytes: Range<usize>) -> PaintSpan {
    PaintSpan {
        bytes,
        color: (0xF5, 0xF6, 0xF7),
        bold: false,
        italic: false,
        underline: false,
        background: None,
    }
}

/// Paint `char_range` (indexes into `text.chars()`) over the existing spans.
pub fn with_background(
    text: &str,
    spans: &[PaintSpan],
    char_range: Range<usize>,
    background: (u8, u8, u8),
    color: Option<(u8, u8, u8)>,
) -> Vec<PaintSpan> {
    if char_range.start >= char_range.end {
        return spans.to_vec();
    }
    let bytes = char_bytes(text, char_range);
    if bytes.start >= bytes.end {
        return spans.to_vec();
    }
    let mut out = Vec::new();
    for span in spans {
        push_split(&mut out, span, bytes.clone(), background, color);
    }
    if out.is_empty() {
        let mut span = plain(bytes.clone());
        span.background = Some(background);
        if let Some(color) = color {
            span.color = color;
        }
        out.push(span);
    }
    out
}

fn push_split(
    out: &mut Vec<PaintSpan>,
    span: &PaintSpan,
    overlay: Range<usize>,
    background: (u8, u8, u8),
    color: Option<(u8, u8, u8)>,
) {
    let start = span.bytes.start;
    let end = span.bytes.end;
    if overlay.end <= start || overlay.start >= end {
        out.push(span.clone());
        return;
    }
    let left = start.max(overlay.start);
    let right = end.min(overlay.end);
    if start < left {
        let mut head = span.clone();
        head.bytes = start..left;
        out.push(head);
    }
    let mut mid = span.clone();
    mid.bytes = left..right;
    mid.background = Some(background);
    if let Some(color) = color {
        mid.color = color;
    }
    out.push(mid);
    if right < end {
        let mut tail = span.clone();
        tail.bytes = right..end;
        out.push(tail);
    }
}

fn char_bytes(text: &str, chars: Range<usize>) -> Range<usize> {
    let mut start = text.len();
    let mut end = text.len();
    for (i, (byte, _)) in text.char_indices().enumerate() {
        if i == chars.start {
            start = byte;
        }
        if i == chars.end {
            end = byte;
            return start..end;
        }
    }
    if chars.start == 0 {
        start = 0;
    }
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_spans_cover_the_line_without_gaps() {
        let line = "# Title and `code`";
        let spans = highlight_line(line);
        assert!(!spans.is_empty());
        assert_eq!(spans.first().unwrap().bytes.start, 0);
        assert_eq!(spans.last().unwrap().bytes.end, line.len());
        let mut at = 0;
        let mut got = String::new();
        for span in &spans {
            assert_eq!(span.bytes.start, at);
            got.push_str(&line[span.bytes.clone()]);
            at = span.bytes.end;
        }
        assert_eq!(got, line);
    }

    #[test]
    fn background_splits_a_span_on_a_character() {
        let text = "hello";
        let spans = vec![plain(0..text.len())];
        let painted = with_background(text, &spans, 1..2, (1, 2, 3), Some((9, 9, 9)));
        assert_eq!(painted.len(), 3);
        assert_eq!(painted[1].bytes, 1..2);
        assert_eq!(painted[1].background, Some((1, 2, 3)));
        assert_eq!(painted[1].color, (9, 9, 9));
        assert_eq!(&text[painted[0].bytes.clone()], "h");
        assert_eq!(&text[painted[2].bytes.clone()], "llo");
    }
}
