//! Markdown preview for the desktop window.
//!
//! The comrak walk stays in `dd_render`. This module wraps those lines, swaps
//! image markers for bitmaps, and keeps the reading-view motion pure.

use std::ops::Range;
use std::path::{Path, PathBuf};

use dd_edit::wrap_row_starts;
use dd_render::{
    PackedPreviewColors, PreviewLink, looks_like_image, palette_from_packed,
    render_markdown_annotated,
};
use dd_vault_core::{read_prefix_nofollow, read_vault_bytes, read_vault_text};

use crate::colors::GuiTheme;

const EMBED_LINES: usize = 80;
const IMAGE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewColors {
    pub text: u32,
    pub muted: u32,
    pub heading: u32,
    pub focus: u32,
    pub link: u32,
    pub code: u32,
    pub success: u32,
    pub warning: u32,
    pub error: u32,
    pub info: u32,
    pub background: u32,
}

impl PreviewColors {
    pub fn from_theme(theme: &GuiTheme) -> Self {
        Self {
            text: theme.text_primary,
            muted: theme.text_secondary,
            heading: theme.text_labels,
            focus: theme.text_active_focus,
            link: theme.links,
            code: theme.files,
            success: theme.success,
            warning: theme.warning,
            error: theme.error,
            info: theme.info,
            background: theme.body_background,
        }
    }

    pub fn key(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        for color in [
            self.text,
            self.muted,
            self.heading,
            self.focus,
            self.link,
            self.code,
            self.success,
            self.warning,
            self.error,
            self.info,
            self.background,
        ] {
            hash = hash
                .wrapping_mul(0x100_0000_01b3)
                .wrapping_add(u64::from(color));
        }
        hash
    }

    fn palette(&self) -> dd_render::PreviewPalette {
        palette_from_packed(PackedPreviewColors {
            text: self.text,
            muted: self.muted,
            heading: self.heading,
            focus: self.focus,
            link: self.link,
            code: self.code,
            success: self.success,
            warning: self.warning,
            error: self.error,
            info: self.info,
            background: self.background,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewSpan {
    pub bytes: Range<usize>,
    pub color: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewImage {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLine {
    pub plain: String,
    pub spans: Vec<PreviewSpan>,
    pub image: Option<PreviewImage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewDoc {
    pub lines: Vec<SourceLine>,
    pub links: Vec<PreviewLink>,
}

impl PreviewDoc {
    #[cfg(test)]
    fn plain(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.plain.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

type EmbedFn<'a> = &'a dyn Fn(&str) -> Option<String>;
type ImageFn<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayRow {
    pub text: String,
    pub src_line: usize,
    pub src_col: usize,
    pub spans: Vec<PreviewSpan>,
    pub image: Option<PreviewImage>,
}

pub struct PreviewRequest<'a> {
    pub src: &'a str,
    pub cols: usize,
    pub colors: &'a PreviewColors,
    pub image_rows: u16,
    pub diagram_rows: u16,
    pub embed: Option<EmbedFn<'a>>,
    pub read_image: Option<ImageFn<'a>>,
}

pub struct PreviewKey<'a> {
    pub rel: &'a Option<String>,
    pub src: &'a str,
    pub cols: usize,
    pub image_rows: u16,
    pub diagram_rows: u16,
    pub palette: u64,
    pub epoch: u64,
}

pub struct PreviewCache {
    pub rel: Option<String>,
    pub src: String,
    pub cols: usize,
    pub image_rows: u16,
    pub diagram_rows: u16,
    pub palette: u64,
    pub epoch: u64,
    pub doc: PreviewDoc,
}

impl PreviewCache {
    pub fn matches(&self, key: &PreviewKey<'_>) -> bool {
        &self.rel == key.rel
            && self.src == key.src
            && self.cols == key.cols
            && self.image_rows == key.image_rows
            && self.diagram_rows == key.diagram_rows
            && self.palette == key.palette
            && self.epoch == key.epoch
    }
}

/// First 80 lines of an embed. A longer note keeps an ellipsis line.
pub fn cap_embed(raw: &str) -> String {
    let mut lines: Vec<&str> = raw.lines().take(EMBED_LINES).collect();
    if raw.lines().count() > EMBED_LINES {
        lines.push("…");
    }
    lines.join("\n")
}

pub fn embed_body(
    root: &Path,
    target: &str,
    find_file: &dyn Fn(&str) -> Option<PathBuf>,
) -> Option<String> {
    let name = target.split('#').next()?.split('|').next()?.trim();
    if name.is_empty() || looks_like_image(name) {
        return None;
    }
    let found = find_file(name)?;
    let raw = read_vault_text(root, &found, 2_000_000).ok()?;
    Some(cap_embed(&raw))
}

pub fn load_image_bytes(
    root: &Path,
    note_rel: Option<&Path>,
    url: &str,
    find_file: &dyn Fn(&str) -> Option<PathBuf>,
) -> Option<Vec<u8>> {
    if let Some(path) = resolve_image_path(root, note_rel, url) {
        return read_prefix_nofollow(&path, IMAGE_BYTES).ok();
    }
    let base = Path::new(url).file_name()?.to_string_lossy();
    let found = find_file(base.as_ref())?;
    read_vault_bytes(root, &found, IMAGE_BYTES).ok()
}

pub fn build_preview(request: &PreviewRequest<'_>) -> PreviewDoc {
    let cols = request.cols.clamp(1, u16::MAX as usize) as u16;
    let expanded = expand_embeds(request.src, request.embed);
    let images = markdown_images(&expanded);
    let mut queue = Vec::new();
    for (_label, url) in images {
        queue.push(decode_image(&url, request.read_image));
    }
    let mut queue = queue.into_iter();
    let (text, links) = render_markdown_annotated(
        request.src,
        request.colors.palette(),
        request.embed,
        None,
        cols,
        request.image_rows.max(1),
        request.diagram_rows.max(1),
    );
    let fallback = request.colors.text;
    let mut lines = Vec::new();
    for line in &text.lines {
        let mut plain = String::new();
        let mut spans = Vec::new();
        for span in &line.spans {
            let piece = span.content.to_string();
            if piece.is_empty() {
                continue;
            }
            let start = plain.len();
            plain.push_str(&piece);
            let (color, bold, italic, underline) = dd_render::style_parts(span.style);
            let color = color
                .map(|(r, g, b)| (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
                .unwrap_or(fallback);
            spans.push(PreviewSpan {
                bytes: start..plain.len(),
                color,
                bold,
                italic,
                underline,
            });
        }
        let markers = plain.matches("[image: ").count();
        let mut image = None;
        for _ in 0..markers {
            if let Some(next) = queue.next()
                && image.is_none()
                && markers == 1
                && image_label(&plain).is_some()
            {
                image = next;
            }
        }
        lines.push(SourceLine {
            plain,
            spans,
            image,
        });
    }
    PreviewDoc { lines, links }
}

fn decode_image(url: &str, read_image: Option<ImageFn<'_>>) -> Option<PreviewImage> {
    let bytes = read_image.and_then(|read| read(url))?;
    let (width, height, rgba) = dd_render::decode_rgba(&bytes).ok()?;
    let png = dd_render::encode_png_rgba(width, height, &rgba).ok()?;
    Some(PreviewImage { png, width, height })
}

pub fn display_rows(doc: &PreviewDoc, cols: usize) -> Vec<DisplayRow> {
    let cols = cols.max(1);
    let mut out = Vec::new();
    for (src_line, line) in doc.lines.iter().enumerate() {
        if line.image.is_some() {
            out.push(DisplayRow {
                text: line.plain.clone(),
                src_line,
                src_col: 0,
                spans: line.spans.clone(),
                image: line.image.clone(),
            });
            continue;
        }
        let chars: Vec<char> = line.plain.chars().collect();
        if chars.is_empty() {
            out.push(DisplayRow {
                text: String::new(),
                src_line,
                src_col: 0,
                spans: Vec::new(),
                image: None,
            });
            continue;
        }
        let starts = wrap_row_starts(&line.plain, cols);
        for (i, &start) in starts.iter().enumerate() {
            let end = starts.get(i + 1).copied().unwrap_or(chars.len());
            let text: String = chars[start..end].iter().collect();
            let byte_start = chars[..start].iter().collect::<String>().len();
            let byte_end = byte_start + text.len();
            out.push(DisplayRow {
                text: text.clone(),
                src_line,
                src_col: start,
                spans: slice_spans(&line.spans, byte_start, byte_end),
                image: None,
            });
        }
    }
    if out.is_empty() {
        out.push(DisplayRow {
            text: String::new(),
            src_line: 0,
            src_col: 0,
            spans: Vec::new(),
            image: None,
        });
    }
    out
}

fn slice_spans(spans: &[PreviewSpan], start: usize, end: usize) -> Vec<PreviewSpan> {
    let mut out = Vec::new();
    for span in spans {
        let lo = span.bytes.start.max(start);
        let hi = span.bytes.end.min(end);
        if hi <= lo {
            continue;
        }
        out.push(PreviewSpan {
            bytes: (lo - start)..(hi - start),
            color: span.color,
            bold: span.bold,
            italic: span.italic,
            underline: span.underline,
        });
    }
    out
}

pub fn link_at_row(
    doc: &PreviewDoc,
    row: &DisplayRow,
    local_col: usize,
) -> Option<(String, Option<String>)> {
    let col = row.src_col.saturating_add(local_col);
    link_covering(doc, row.src_line, col, 1)
}

pub fn link_covering(
    doc: &PreviewDoc,
    line: usize,
    col: usize,
    span: usize,
) -> Option<(String, Option<String>)> {
    let end = col.saturating_add(span.max(1));
    if let Some(hit) = doc
        .links
        .iter()
        .find(|link| link.line == line && link.start < end && link.end > col)
    {
        return Some(split_target(&hit.target));
    }
    let plain = doc.lines.get(line)?.plain.as_str();
    let chars = plain.chars().count();
    let mut i = col.min(chars);
    let stop = end.min(chars);
    while i < stop {
        if let Some(link) = dd_vault_core::link_at(plain, i) {
            return Some((link.dst_raw, link.dst_heading));
        }
        i += 1;
    }
    None
}

fn split_target(raw: &str) -> (String, Option<String>) {
    match raw.split_once('#') {
        Some((dst, heading)) if !dst.is_empty() && !heading.is_empty() => {
            (dst.to_string(), Some(heading.to_string()))
        }
        _ => (raw.to_string(), None),
    }
}

pub fn move_clamped(row: usize, len: usize, delta: isize) -> usize {
    let len = len.max(1);
    let cur = row.min(len - 1) as isize;
    (cur + delta).clamp(0, (len as isize) - 1) as usize
}

pub fn reveal_cursor(row: usize, scroll: usize, view: usize) -> usize {
    let view = view.max(1);
    if row < scroll {
        row
    } else if row >= scroll + view {
        row.saturating_add(1).saturating_sub(view)
    } else {
        scroll
    }
}

pub fn yank_text(lines: &[String], row: usize, visual: Option<usize>) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let last = lines.len() - 1;
    let row = row.min(last);
    let (start, end) = match visual {
        Some(anchor) => {
            let anchor = anchor.min(last);
            (anchor.min(row), anchor.max(row))
        }
        None => (row, row),
    };
    lines[start..=end].join("\n")
}

fn image_label(plain: &str) -> Option<&str> {
    let start = plain.find("[image: ")?;
    let rest = &plain[start + "[image: ".len()..];
    let end = rest.find(']')?;
    let label = rest[..end].trim();
    if label.is_empty() || !rest[end + 1..].trim().is_empty() {
        return None;
    }
    let before: String = plain[..start]
        .chars()
        .filter(|ch| !matches!(ch, ' ' | '│' | '•' | '☑' | '☐'))
        .collect();
    if before.is_empty() { Some(label) } else { None }
}

fn expand_embeds(src: &str, embed: Option<EmbedFn<'_>>) -> String {
    let Some(resolve) = embed else {
        return src.to_string();
    };
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("![[") {
        out.push_str(&rest[..start]);
        rest = &rest[start + 3..];
        if let Some(end) = rest.find("]]") {
            let target = rest[..end].trim();
            rest = &rest[end + 2..];
            if looks_like_image(target) {
                let name = target.split('|').next().unwrap_or(target).trim();
                out.push_str("![");
                out.push_str(name);
                out.push_str("](");
                out.push_str(name);
                out.push(')');
            } else {
                match resolve(target) {
                    Some(body) => {
                        out.push('\n');
                        out.push_str(&body);
                        if !body.ends_with('\n') {
                            out.push('\n');
                        }
                    }
                    None => {
                        out.push_str("*[missing embed: ");
                        out.push_str(target);
                        out.push_str("]*");
                    }
                }
            }
        } else {
            out.push_str("![[");
            out.push_str(rest);
            return out;
        }
    }
    out.push_str(rest);
    out
}

fn markdown_images(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut fence = false;
    for line in src.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            fence = !fence;
            continue;
        }
        if !fence {
            scan_images(line, &mut out);
        }
    }
    out
}

fn scan_images(line: &str, out: &mut Vec<(String, String)>) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut code = false;
    while i < chars.len() {
        if chars[i] == '`' {
            code = !code;
            i += 1;
            continue;
        }
        if !code
            && chars[i] == '!'
            && i + 1 < chars.len()
            && chars[i + 1] == '['
            && let Some((alt, url, next)) = parse_md_image(&chars, i + 2)
        {
            let label = if alt.is_empty() { url.clone() } else { alt };
            out.push((label, url));
            i = next;
            continue;
        }
        i += 1;
    }
}

fn parse_md_image(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    let mut i = start;
    while i + 1 < chars.len() {
        if chars[i] == ']' && chars[i + 1] == '(' {
            let alt: String = chars[start..i].iter().collect();
            let url_at = i + 2;
            let end = chars[url_at..].iter().position(|ch| *ch == ')')?;
            let url: String = chars[url_at..url_at + end].iter().collect();
            return Some((
                alt.trim().to_string(),
                url.trim().to_string(),
                url_at + end + 1,
            ));
        }
        i += 1;
    }
    None
}

fn resolve_image_path(root: &Path, note_rel: Option<&Path>, url: &str) -> Option<PathBuf> {
    let url = url.trim();
    if url.is_empty()
        || url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("data:")
    {
        return None;
    }
    let mut candidates = vec![root.join(url)];
    if let Some(note) = note_rel
        && let Some(parent) = note.parent()
    {
        candidates.push(root.join(parent).join(url));
    }
    candidates
        .into_iter()
        .find_map(|path| file_inside(root, &path))
}

fn file_inside(root: &Path, path: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    if path.starts_with(&root) && path.is_file() {
        Some(path)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors() -> PreviewColors {
        PreviewColors {
            text: 0xF5F6F7,
            muted: 0x9EA3AA,
            heading: 0xFFAF46,
            focus: 0x64B4F5,
            link: 0xFFA087,
            code: 0xFFAF46,
            success: 0x82E0AA,
            warning: 0xF5C469,
            error: 0xE57373,
            info: 0x5DADE2,
            background: 0x2A2D31,
        }
    }

    fn render(src: &str) -> PreviewDoc {
        build_preview(&PreviewRequest {
            src,
            cols: 48,
            colors: &colors(),
            image_rows: 8,
            diagram_rows: 24,
            embed: None,
            read_image: None,
        })
    }

    #[test]
    fn headings_tasks_front_matter_and_mermaid_boxes() {
        let doc = render(
            "---\ntitle: hidden\n---\n# Hello\n\n- [x] done\n\n```mermaid\nflowchart LR\n  A --> B\n```\n",
        );
        let plain = doc.plain();
        assert!(plain.contains("Hello"), "{plain}");
        assert!(!plain.contains("hidden"), "{plain}");
        assert!(plain.contains('☑'), "{plain}");
        assert!(plain.contains('┌'), "{plain}");
    }

    #[test]
    fn embeds_stop_at_eighty_lines() {
        let body = (0..100)
            .map(|i| format!("L{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let capped = cap_embed(&body);
        assert!(capped.contains("L79"));
        assert!(!capped.contains("L80"));
        assert!(capped.ends_with('…'));
        let doc = build_preview(&PreviewRequest {
            src: "![[other]]\n",
            cols: 40,
            colors: &colors(),
            image_rows: 4,
            diagram_rows: 16,
            embed: Some(&|target| {
                assert_eq!(target, "other");
                Some(capped.clone())
            }),
            read_image: None,
        });
        let plain = doc.plain();
        assert!(plain.contains("L0"), "{plain}");
        assert!(plain.contains('…'), "{plain}");
        assert!(!plain.contains("L90"), "{plain}");
    }

    #[test]
    fn an_image_line_becomes_a_bitmap() {
        let png = dd_render::encode_png_rgba(1, 1, &[255, 0, 0, 255]).expect("png");
        let doc = build_preview(&PreviewRequest {
            src: "![plot](pic.png)\n",
            cols: 40,
            colors: &colors(),
            image_rows: 4,
            diagram_rows: 16,
            embed: None,
            read_image: Some(&|url| {
                assert_eq!(url, "pic.png");
                Some(png.clone())
            }),
        });
        assert!(
            doc.lines.iter().any(|line| line.image.is_some()),
            "{:?}",
            doc.lines.iter().map(|line| &line.plain).collect::<Vec<_>>()
        );
    }

    #[test]
    fn reading_yank_covers_the_visual_range() {
        let lines = vec!["Title".into(), "Alpha".into(), "Beta".into()];
        assert_eq!(yank_text(&lines, 1, None), "Alpha");
        assert_eq!(yank_text(&lines, 2, Some(0)), "Title\nAlpha\nBeta");
        assert_eq!(move_clamped(0, 3, -1), 0);
        assert_eq!(move_clamped(1, 3, 5), 2);
        assert_eq!(reveal_cursor(8, 0, 4), 5);
        assert_eq!(reveal_cursor(1, 3, 4), 1);
    }

    #[test]
    fn wrapped_rows_keep_the_source_column() {
        let doc = render(&format!("[[note]] {}", "word ".repeat(30)));
        let rows = display_rows(&doc, 12);
        assert!(rows.len() > 1, "{}", rows.len());
        let hit = rows.iter().find(|row| row.text.contains("[[")).unwrap();
        let col = hit.text.find("[[").unwrap();
        assert_eq!(link_at_row(&doc, hit, col).unwrap().0, "note");
    }
}
