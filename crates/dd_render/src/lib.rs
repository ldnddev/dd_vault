//! comrak AST → ratatui `Text` for the markdown preview pane.

use comrak::nodes::{AstNode, NodeValue};
use comrak::{parse_document, Arena, Options};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

mod term_image;

pub use term_image::{decode_rgba, encode_png_rgba, looks_like_image, rgba_to_half_block};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid rgba buffer")]
    InvalidRgba,
    #[error(transparent)]
    Image(#[from] image::ImageError),
}

#[derive(Clone, Copy, Debug)]
pub struct PreviewPalette {
    pub text: Color,
    pub muted: Color,
    pub heading: Color,
    pub focus: Color,
    pub link: Color,
    pub code: Color,
    pub quote: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,
    pub background: Color,
}

impl PreviewPalette {
    fn text(self) -> Style {
        Style::default().fg(self.text)
    }
    fn muted(self) -> Style {
        Style::default().fg(self.muted)
    }
    fn heading(self) -> Style {
        Style::default()
            .fg(self.heading)
            .add_modifier(Modifier::BOLD)
    }
    fn link(self) -> Style {
        Style::default()
            .fg(self.link)
            .add_modifier(Modifier::UNDERLINED)
    }
    fn code(self) -> Style {
        Style::default().fg(self.code)
    }
    fn quote(self) -> Style {
        Style::default().fg(self.quote)
    }
}

/// Optional resolver for `![[note]]` embeds. Return markdown body or `None`.
pub type EmbedResolver<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// Optional resolver for markdown / wikilink images. Return encoded file bytes.
pub type ImageResolver<'a> = dyn Fn(&str) -> Option<Vec<u8>> + 'a;

struct RenderCtx<'a> {
    p: PreviewPalette,
    image: Option<&'a ImageResolver<'a>>,
    max_width: u16,
    max_image_rows: u16,
}

pub fn render_markdown(
    src: &str,
    palette: PreviewPalette,
    embed: Option<&EmbedResolver<'_>>,
) -> Text<'static> {
    render_markdown_ex(src, palette, embed, None, 80, 12)
}

pub fn render_markdown_ex(
    src: &str,
    palette: PreviewPalette,
    embed: Option<&EmbedResolver<'_>>,
    image: Option<&ImageResolver<'_>>,
    max_width: u16,
    max_image_rows: u16,
) -> Text<'static> {
    let expanded = expand_embeds(src, embed);
    let arena = Arena::new();
    let root = parse_document(&arena, &expanded, &options());
    let mut out = Vec::new();
    let ctx = RenderCtx {
        p: palette,
        image,
        max_width: max_width.max(1),
        max_image_rows: max_image_rows.max(1),
    };
    walk_block(root, &ctx, &mut out, 0);
    while out
        .last()
        .is_some_and(|l| l.spans.is_empty() || line_is_blank(l))
    {
        out.pop();
    }
    Text::from(out)
}

fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.alerts = true;
    options.extension.front_matter_delimiter = Some("---".into());
    options.extension.wikilinks_title_after_pipe = true;
    options
}

fn expand_embeds(src: &str, embed: Option<&EmbedResolver<'_>>) -> String {
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

fn line_is_blank(line: &Line<'_>) -> bool {
    line.spans.iter().all(|s| s.content.trim().is_empty())
}

fn walk_block<'a>(
    node: &'a AstNode<'a>,
    ctx: &RenderCtx<'_>,
    out: &mut Vec<Line<'static>>,
    indent: usize,
) {
    let p = ctx.p;
    match &node.data.borrow().value {
        NodeValue::Document => {
            for c in node.children() {
                walk_block(c, ctx, out, indent);
            }
        }
        NodeValue::FrontMatter(_) => {}
        NodeValue::Heading(h) => {
            let hashes = "#".repeat(h.level as usize);
            let mut spans = prefix_spans(indent);
            spans.push(Span::styled(format!("{hashes} "), p.heading()));
            collect_inlines(node, ctx, &mut spans, None, indent);
            out.push(Line::from(spans));
            out.push(blank());
        }
        NodeValue::Paragraph => {
            let mut spans = prefix_spans(indent);
            collect_inlines(node, ctx, &mut spans, Some(out), indent);
            if !spans_are_blank(&spans) {
                out.push(Line::from(spans));
            }
            out.push(blank());
        }
        NodeValue::BlockQuote => {
            for c in node.children() {
                walk_quote(c, ctx, out, indent);
            }
            out.push(blank());
        }
        NodeValue::List(_) => {
            for c in node.children() {
                match &c.data.borrow().value {
                    NodeValue::TaskItem(sym) => walk_item(c, ctx, out, indent, Some(*sym)),
                    NodeValue::Item(_) => walk_item(c, ctx, out, indent, None),
                    _ => walk_block(c, ctx, out, indent),
                }
            }
            out.push(blank());
        }
        NodeValue::Item(_) => walk_item(node, ctx, out, indent, None),
        NodeValue::TaskItem(sym) => walk_item(node, ctx, out, indent, Some(*sym)),
        NodeValue::CodeBlock(cb) => {
            let info = cb.info.trim();
            if !info.is_empty() {
                let mut spans = prefix_spans(indent);
                spans.push(Span::styled(format!("```{info}"), p.muted()));
                out.push(Line::from(spans));
            }
            for line in cb.literal.replace('\r', "").split('\n') {
                let mut spans = prefix_spans(indent);
                spans.push(Span::styled("  ", p.muted()));
                spans.push(Span::styled(line.to_string(), p.code()));
                out.push(Line::from(spans));
            }
            out.push(blank());
        }
        NodeValue::ThematicBreak => {
            out.push(Line::from(Span::styled("─".repeat(24), p.muted())));
            out.push(blank());
        }
        NodeValue::Table(_) => {
            walk_table(node, ctx, out, indent);
            out.push(blank());
        }
        NodeValue::Alert(alert) => {
            let (label, style) = alert_style(alert, p);
            let mut spans = prefix_spans(indent);
            spans.push(Span::styled(
                format!("▌ {label} "),
                style.add_modifier(Modifier::BOLD),
            ));
            if let Some(title) = alert_title(alert) {
                spans.push(Span::styled(title, style));
            }
            out.push(Line::from(spans));
            for c in node.children() {
                walk_block(c, ctx, out, indent + 1);
            }
            out.push(blank());
        }
        _ => {
            for c in node.children() {
                walk_block(c, ctx, out, indent);
            }
        }
    }
}

fn walk_quote<'a>(
    node: &'a AstNode<'a>,
    ctx: &RenderCtx<'_>,
    out: &mut Vec<Line<'static>>,
    indent: usize,
) {
    match &node.data.borrow().value {
        NodeValue::Paragraph => {
            let mut spans = prefix_spans(indent);
            spans.push(Span::styled("│ ", ctx.p.quote()));
            collect_inlines(node, ctx, &mut spans, Some(out), indent);
            if !spans_are_blank(&spans) {
                out.push(Line::from(spans));
            }
        }
        NodeValue::Alert(alert) => {
            walk_block(node, ctx, out, indent);
            let _ = alert;
        }
        _ => walk_block(node, ctx, out, indent),
    }
}

fn walk_item<'a>(
    node: &'a AstNode<'a>,
    ctx: &RenderCtx<'_>,
    out: &mut Vec<Line<'static>>,
    indent: usize,
    task: Option<Option<char>>,
) {
    let p = ctx.p;
    let bullet = match task {
        Some(Some(c)) if c == 'x' || c == 'X' => ("☑ ".to_string(), Style::default().fg(p.success)),
        Some(_) => ("☐ ".to_string(), p.muted()),
        None => ("• ".to_string(), p.text()),
    };
    let mut first = true;
    for c in node.children() {
        match &c.data.borrow().value {
            NodeValue::Paragraph => {
                let mut spans = prefix_spans(indent);
                if first {
                    spans.push(Span::styled(bullet.0.clone(), bullet.1));
                    first = false;
                } else {
                    spans.push(Span::raw("    "));
                }
                collect_inlines(c, ctx, &mut spans, Some(out), indent);
                if !spans_are_blank(&spans) {
                    out.push(Line::from(spans));
                }
            }
            NodeValue::List(_) => walk_block(c, ctx, out, indent + 1),
            _ => walk_block(c, ctx, out, indent + 1),
        }
    }
}

fn walk_table<'a>(
    node: &'a AstNode<'a>,
    ctx: &RenderCtx<'_>,
    out: &mut Vec<Line<'static>>,
    indent: usize,
) {
    for row in node.children() {
        let mut cells = Vec::new();
        for cell in row.children() {
            let mut spans = Vec::new();
            collect_inlines(cell, ctx, &mut spans, None, indent);
            let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
            cells.push(text);
        }
        let mut line = prefix_spans(indent);
        line.push(Span::styled(
            format!("│ {} │", cells.join(" │ ")),
            ctx.p.text(),
        ));
        out.push(Line::from(line));
    }
}

fn collect_inlines<'a>(
    node: &'a AstNode<'a>,
    ctx: &RenderCtx<'_>,
    spans: &mut Vec<Span<'static>>,
    mut out: Option<&mut Vec<Line<'static>>>,
    indent: usize,
) {
    let p = ctx.p;
    for c in node.children() {
        match &c.data.borrow().value {
            NodeValue::Text(t) => spans.push(Span::styled(t.clone(), p.text())),
            NodeValue::SoftBreak | NodeValue::LineBreak => spans.push(Span::raw(" ")),
            NodeValue::Code(code) => {
                spans.push(Span::styled(format!("`{}`", code.literal), p.code()));
            }
            NodeValue::Emph => {
                let start = spans.len();
                collect_inlines(c, ctx, spans, None, indent);
                for s in &mut spans[start..] {
                    s.style = s.style.add_modifier(Modifier::ITALIC);
                }
            }
            NodeValue::Strong => {
                let start = spans.len();
                collect_inlines(c, ctx, spans, None, indent);
                for s in &mut spans[start..] {
                    s.style = s.style.add_modifier(Modifier::BOLD);
                }
            }
            NodeValue::Strikethrough => {
                let start = spans.len();
                collect_inlines(c, ctx, spans, None, indent);
                for s in &mut spans[start..] {
                    s.style = s.style.add_modifier(Modifier::CROSSED_OUT);
                }
            }
            NodeValue::Link(link) => {
                let start = spans.len();
                collect_inlines(c, ctx, spans, None, indent);
                if start == spans.len() {
                    spans.push(Span::styled(link.url.clone(), p.link()));
                } else {
                    for s in &mut spans[start..] {
                        s.style = p.link();
                    }
                }
            }
            NodeValue::Image(link) => {
                let alt = inline_plain(c);
                let label = if alt.is_empty() {
                    link.url.clone()
                } else {
                    alt
                };
                if let (Some(out), Some(resolve)) = (out.as_mut(), ctx.image) {
                    if !spans_are_blank(spans) {
                        out.push(Line::from(std::mem::take(spans)));
                        *spans = prefix_spans(indent);
                    } else {
                        spans.clear();
                    }
                    emit_image(out, resolve, &link.url, &label, ctx, indent);
                } else {
                    spans.push(Span::styled(format!("[image: {label}]"), p.link()));
                }
            }
            NodeValue::WikiLink(w) => {
                let display = inline_plain(c);
                let label = if display.is_empty() {
                    w.url.clone()
                } else {
                    display
                };
                spans.push(Span::styled(format!("[[{label}]]"), p.link()));
            }
            _ => collect_inlines(c, ctx, spans, None, indent),
        }
    }
}

fn emit_image(
    out: &mut Vec<Line<'static>>,
    resolve: &ImageResolver<'_>,
    url: &str,
    label: &str,
    ctx: &RenderCtx<'_>,
    indent: usize,
) {
    let rendered = resolve(url)
        .and_then(|bytes| decode_rgba(&bytes).ok())
        .map(|(w, h, rgba)| {
            rgba_to_half_block(
                &rgba,
                w,
                h,
                ctx.max_width.saturating_sub((indent as u16) * 2),
                ctx.max_image_rows,
                ctx.p.background,
            )
        })
        .filter(|lines| !lines.is_empty());
    match rendered {
        Some(lines) => {
            for line in lines {
                if indent == 0 {
                    out.push(line);
                } else {
                    let mut spans = prefix_spans(indent);
                    spans.extend(line.spans);
                    out.push(Line::from(spans));
                }
            }
        }
        None => {
            let mut spans = prefix_spans(indent);
            spans.push(Span::styled(format!("[image: {label}]"), ctx.p.link()));
            out.push(Line::from(spans));
        }
    }
}

fn spans_are_blank(spans: &[Span<'_>]) -> bool {
    spans.iter().all(|s| s.content.trim().is_empty())
}

fn inline_plain<'a>(node: &'a AstNode<'a>) -> String {
    let mut s = String::new();
    for c in node.children() {
        match &c.data.borrow().value {
            NodeValue::Text(t) => s.push_str(t),
            NodeValue::Code(code) => s.push_str(&code.literal),
            _ => s.push_str(&inline_plain(c)),
        }
    }
    s
}

fn prefix_spans(indent: usize) -> Vec<Span<'static>> {
    if indent == 0 {
        Vec::new()
    } else {
        vec![Span::raw("  ".repeat(indent))]
    }
}

fn blank() -> Line<'static> {
    Line::from("")
}

fn alert_style(alert: &comrak::nodes::NodeAlert, p: PreviewPalette) -> (&'static str, Style) {
    use comrak::nodes::AlertType;
    match alert.alert_type {
        AlertType::Note => ("NOTE", Style::default().fg(p.info)),
        AlertType::Tip => ("TIP", Style::default().fg(p.success)),
        AlertType::Important => ("IMPORTANT", Style::default().fg(p.focus)),
        AlertType::Warning => ("WARNING", Style::default().fg(p.warning)),
        AlertType::Caution => ("CAUTION", Style::default().fg(p.error)),
    }
}

fn alert_title(alert: &comrak::nodes::NodeAlert) -> Option<String> {
    alert.title.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pal() -> PreviewPalette {
        PreviewPalette {
            text: Color::White,
            muted: Color::Gray,
            heading: Color::Cyan,
            focus: Color::Blue,
            link: Color::Magenta,
            code: Color::Yellow,
            quote: Color::Gray,
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            info: Color::Cyan,
            background: Color::Black,
        }
    }

    fn dump(src: &str) -> String {
        let t = render_markdown(src, pal(), None);
        t.lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_heading_and_paragraph() {
        let d = dump("# Hello\n\nWorld");
        assert!(d.contains("Hello"), "{d}");
        assert!(d.contains("World"), "{d}");
    }

    #[test]
    fn renders_wikilink() {
        let d = dump("See [[Inbox]] please.");
        assert!(d.contains("Inbox"), "{d}");
    }

    #[test]
    fn renders_task_list() {
        let d = dump("- [x] done\n- [ ] todo");
        assert!(d.contains("done"), "{d}");
        assert!(d.contains("todo"), "{d}");
        assert!(d.contains('☑') || d.contains('☐') || d.contains('['), "{d}");
    }

    #[test]
    fn renders_callout() {
        let d = dump("> [!NOTE]\n> Hello callout");
        assert!(d.contains("NOTE") || d.contains("Hello callout"), "{d}");
        assert!(d.contains("Hello callout"), "{d}");
    }

    #[test]
    fn skips_frontmatter() {
        let d = dump("---\ntitle: X\n---\n# Body");
        assert!(d.contains("Body"), "{d}");
        assert!(!d.contains("title: X"), "{d}");
    }

    #[test]
    fn expands_embed() {
        let resolve = |name: &str| {
            if name == "other" {
                Some("# Other\nHi".into())
            } else {
                None
            }
        };
        let t = render_markdown("Before\n\n![[other]]\n\nAfter", pal(), Some(&resolve));
        let d: String = t
            .lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(d.contains("Other"), "{d}");
        assert!(d.contains("After"), "{d}");
    }

    #[test]
    fn markdown_image_uses_half_block() {
        let mut rgba = vec![0u8; 4];
        rgba[0] = 255;
        rgba[3] = 255;
        let png = encode_png_rgba(1, 1, &rgba).expect("png");
        let resolve = |url: &str| {
            if url == "red.png" {
                Some(png.clone())
            } else {
                None
            }
        };
        let t = render_markdown_ex("![](red.png)", pal(), None, Some(&resolve), 8, 4);
        let has_block = t.lines.iter().any(|l| {
            l.spans
                .iter()
                .any(|s| s.content.as_ref() == "▀" && s.style.fg == Some(Color::Rgb(255, 0, 0)))
        });
        assert!(has_block, "{t:?}");
    }

    #[test]
    fn missing_image_keeps_placeholder() {
        let d = dump("![](nope.png)");
        assert!(d.contains("[image:"), "{d}");
    }

    #[test]
    fn wikilink_image_embed_becomes_markdown_image() {
        let d = dump("![[shot.png]]");
        assert!(d.contains("[image:") || d.contains("shot.png"), "{d}");
        assert!(!d.contains("missing embed"), "{d}");
    }
}
