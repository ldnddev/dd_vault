//! Structured markdown for the desktop preview.
//!
//! The terminal still paints [`crate::render_markdown_annotated`]. This walk
//! keeps heading levels and inline runs so the window can set real font sizes.

use comrak::nodes::{AstNode, ListType, NodeValue};
use comrak::parse_document;

use crate::{expand_embeds, inline_plain, mermaid_lines, options, table_lines, EmbedResolver};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Heading,
    Paragraph,
    Quote,
    Item,
    Code,
    Rule,
    Image,
    Pre,
    Banner,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MdRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MdBlock {
    pub kind: BlockKind,
    pub level: u8,
    pub depth: u8,
    pub marker: String,
    pub text: String,
    pub runs: Vec<MdRun>,
}

#[derive(Clone)]
struct Face {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: String,
}

impl Face {
    fn plain() -> Self {
        Self {
            bold: false,
            italic: false,
            strike: false,
            code: false,
            link: String::new(),
        }
    }
}

struct Walk {
    width: u16,
    diagrams: u16,
}

/// Blocks in document order. Embeds expand the same way as the terminal preview.
pub fn render_markdown_blocks(
    src: &str,
    embed: Option<&EmbedResolver<'_>>,
    max_width: u16,
    max_diagram_rows: u16,
) -> Vec<MdBlock> {
    let expanded = expand_embeds(src, embed);
    let arena = comrak::Arena::new();
    let root = parse_document(&arena, &expanded, &options());
    let walk = Walk {
        width: max_width.max(8),
        diagrams: max_diagram_rows.max(1),
    };
    let mut out = Vec::new();
    walk_node(root, &walk, &mut out, 0);
    out
}

fn walk_node<'a>(node: &'a AstNode<'a>, walk: &Walk, out: &mut Vec<MdBlock>, depth: u8) {
    match &node.data.borrow().value {
        NodeValue::Document => {
            for child in node.children() {
                walk_node(child, walk, out, depth);
            }
        }
        NodeValue::FrontMatter(_) => {}
        NodeValue::Heading(heading) => {
            take_inlines(
                node,
                out,
                BlockKind::Heading,
                heading.level,
                depth,
                &mut String::new(),
            );
        }
        NodeValue::Paragraph => {
            take_inlines(
                node,
                out,
                BlockKind::Paragraph,
                0,
                depth,
                &mut String::new(),
            );
        }
        NodeValue::BlockQuote => {
            for child in node.children() {
                match &child.data.borrow().value {
                    NodeValue::Paragraph => {
                        take_inlines(child, out, BlockKind::Quote, 0, depth, &mut String::new());
                    }
                    _ => walk_node(child, walk, out, depth),
                }
            }
        }
        NodeValue::List(list) => {
            let ordered = list.list_type == ListType::Ordered;
            let mut n = list.start;
            for child in node.children() {
                match &child.data.borrow().value {
                    NodeValue::TaskItem(sym) => {
                        walk_item(child, walk, out, depth, Some(*sym), None)
                    }
                    NodeValue::Item(_) => {
                        let num = if ordered {
                            let current = n;
                            n = n.saturating_add(1);
                            Some(current)
                        } else {
                            None
                        };
                        walk_item(child, walk, out, depth, None, num);
                    }
                    _ => walk_node(child, walk, out, depth),
                }
            }
        }
        NodeValue::Item(_) => walk_item(node, walk, out, depth, None, None),
        NodeValue::TaskItem(sym) => walk_item(node, walk, out, depth, Some(*sym), None),
        NodeValue::CodeBlock(block) => {
            if crate::is_mermaid_info(&block.info) {
                let lines = mermaid_lines(&block.literal, walk.width, walk.diagrams);
                out.push(pre_block(&lines, depth));
            } else {
                out.push(code_block(&block.literal, depth));
            }
        }
        NodeValue::ThematicBreak => out.push(MdBlock {
            kind: BlockKind::Rule,
            level: 0,
            depth,
            marker: String::new(),
            text: String::new(),
            runs: Vec::new(),
        }),
        NodeValue::Table(_) => {
            let lines = table_lines(node, walk.width);
            if !lines.is_empty() {
                out.push(pre_block(&lines, depth));
            }
        }
        NodeValue::Alert(alert) => {
            let (label, tone) = banner_tone(alert);
            let title = alert.title.clone().unwrap_or_default();
            out.push(MdBlock {
                kind: BlockKind::Banner,
                level: tone,
                depth,
                marker: label.to_string(),
                text: title,
                runs: Vec::new(),
            });
            let next = depth.saturating_add(1);
            for child in node.children() {
                walk_node(child, walk, out, next);
            }
        }
        NodeValue::HtmlBlock(html) => {
            let text = html.literal.trim_end_matches(['\r', '\n']).to_string();
            if !text.trim().is_empty() {
                out.push(MdBlock {
                    kind: BlockKind::Pre,
                    level: 0,
                    depth,
                    marker: String::new(),
                    text,
                    runs: Vec::new(),
                });
            }
        }
        _ => {
            for child in node.children() {
                walk_node(child, walk, out, depth);
            }
        }
    }
}

fn walk_item<'a>(
    node: &'a AstNode<'a>,
    walk: &Walk,
    out: &mut Vec<MdBlock>,
    depth: u8,
    task: Option<Option<char>>,
    num: Option<usize>,
) {
    let mut marker = item_marker(task, num);
    let mut continued = String::new();
    let mut first = true;
    for child in node.children() {
        match &child.data.borrow().value {
            NodeValue::Paragraph => {
                let mark = if first {
                    first = false;
                    &mut marker
                } else {
                    &mut continued
                };
                take_inlines(child, out, BlockKind::Item, 0, depth, mark);
            }
            NodeValue::List(_) => walk_node(child, walk, out, depth.saturating_add(1)),
            _ => walk_node(child, walk, out, depth.saturating_add(1)),
        }
    }
}

fn item_marker(task: Option<Option<char>>, num: Option<usize>) -> String {
    match task {
        Some(Some(ch)) if ch == 'x' || ch == 'X' => "☑ ".to_string(),
        Some(_) => "☐ ".to_string(),
        None => match num {
            Some(n) => format!("{n}. "),
            None => "• ".to_string(),
        },
    }
}

struct Flow<'a> {
    out: &'a mut Vec<MdBlock>,
    kind: BlockKind,
    level: u8,
    depth: u8,
}

fn take_inlines<'a>(
    node: &'a AstNode<'a>,
    out: &mut Vec<MdBlock>,
    kind: BlockKind,
    level: u8,
    depth: u8,
    marker: &mut String,
) {
    let mut runs = Vec::new();
    let mut flow = Flow {
        out,
        kind,
        level,
        depth,
    };
    fill(node, &mut flow, &mut runs, &Face::plain(), marker);
    push_runs(flow.out, kind, level, depth, marker, &mut runs);
}

fn banner_tone(alert: &comrak::nodes::NodeAlert) -> (&'static str, u8) {
    use comrak::nodes::AlertType;
    match alert.alert_type {
        AlertType::Note => ("NOTE", 0),
        AlertType::Tip => ("TIP", 1),
        AlertType::Important => ("IMPORTANT", 2),
        AlertType::Warning => ("WARNING", 3),
        AlertType::Caution => ("CAUTION", 4),
    }
}

fn fill<'a>(
    node: &'a AstNode<'a>,
    flow: &mut Flow<'_>,
    runs: &mut Vec<MdRun>,
    face: &Face,
    marker: &mut String,
) {
    for child in node.children() {
        match &child.data.borrow().value {
            NodeValue::Text(text) => push_text(runs, text, face),
            NodeValue::SoftBreak | NodeValue::LineBreak => push_text(runs, " ", face),
            NodeValue::Code(code) => {
                let mut face = face.clone();
                face.code = true;
                push_text(runs, &code.literal, &face);
            }
            NodeValue::Emph => {
                let mut face = face.clone();
                face.italic = true;
                fill(child, flow, runs, &face, marker);
            }
            NodeValue::Strong => {
                let mut face = face.clone();
                face.bold = true;
                fill(child, flow, runs, &face, marker);
            }
            NodeValue::Strikethrough => {
                let mut face = face.clone();
                face.strike = true;
                fill(child, flow, runs, &face, marker);
            }
            NodeValue::Link(link) => {
                let mut face = face.clone();
                face.link = link.url.clone();
                let before = runs.len();
                fill(child, flow, runs, &face, marker);
                if before == runs.len() {
                    push_text(runs, &link.url, &face);
                }
            }
            NodeValue::Image(link) => {
                push_runs(flow.out, flow.kind, flow.level, flow.depth, marker, runs);
                let alt = inline_plain(child);
                let label = if alt.is_empty() {
                    link.url.clone()
                } else {
                    alt
                };
                flow.out.push(MdBlock {
                    kind: BlockKind::Image,
                    level: 0,
                    depth: flow.depth,
                    marker: label,
                    text: link.url.clone(),
                    runs: Vec::new(),
                });
            }
            NodeValue::WikiLink(link) => {
                let display = inline_plain(child);
                let label = if display.is_empty() {
                    link.url.clone()
                } else {
                    display
                };
                let mut face = face.clone();
                face.link = link.url.clone();
                push_text(runs, &label, &face);
            }
            _ => fill(child, flow, runs, face, marker),
        }
    }
}

fn push_text(runs: &mut Vec<MdRun>, text: &str, face: &Face) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut() {
        if last.bold == face.bold
            && last.italic == face.italic
            && last.strike == face.strike
            && last.code == face.code
            && last.link == face.link
        {
            last.text.push_str(text);
            return;
        }
    }
    runs.push(MdRun {
        text: text.to_string(),
        bold: face.bold,
        italic: face.italic,
        strike: face.strike,
        code: face.code,
        link: face.link.clone(),
    });
}

fn push_runs(
    out: &mut Vec<MdBlock>,
    kind: BlockKind,
    level: u8,
    depth: u8,
    marker: &mut String,
    runs: &mut Vec<MdRun>,
) {
    if runs.iter().all(|run| run.text.trim().is_empty()) {
        runs.clear();
        return;
    }
    out.push(MdBlock {
        kind,
        level,
        depth,
        marker: std::mem::take(marker),
        text: String::new(),
        runs: std::mem::take(runs),
    });
}

fn code_block(literal: &str, depth: u8) -> MdBlock {
    MdBlock {
        kind: BlockKind::Code,
        level: 0,
        depth,
        marker: String::new(),
        text: literal.trim_end_matches(['\r', '\n']).to_string(),
        runs: Vec::new(),
    }
}

fn pre_block(lines: &[String], depth: u8) -> MdBlock {
    MdBlock {
        kind: BlockKind::Pre,
        level: 0,
        depth,
        marker: String::new(),
        text: lines.join("\n"),
        runs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump_runs(block: &MdBlock) -> String {
        block.runs.iter().map(|run| run.text.as_str()).collect()
    }

    #[test]
    fn heading_level_and_inline_styles() {
        let blocks = render_markdown_blocks("# Hello\n\n**bold** and *em* `code`\n", None, 80, 16);
        assert!(blocks.len() >= 2, "{blocks:?}");
        assert_eq!(blocks[0].kind, BlockKind::Heading);
        assert_eq!(blocks[0].level, 1);
        assert_eq!(dump_runs(&blocks[0]), "Hello");
        let para = &blocks[1];
        assert_eq!(para.kind, BlockKind::Paragraph);
        assert!(para.runs.iter().any(|run| run.bold && run.text == "bold"));
        assert!(para.runs.iter().any(|run| run.italic && run.text == "em"));
        assert!(para.runs.iter().any(|run| run.code && run.text == "code"));
    }

    #[test]
    fn second_level_heading_and_links() {
        let blocks = render_markdown_blocks(
            "## Sub\n\nSee [a](https://ex.com) and [[Note]].\n",
            None,
            80,
            16,
        );
        assert_eq!(blocks[0].level, 2);
        let para = blocks
            .iter()
            .find(|block| block.kind == BlockKind::Paragraph);
        let para = para.expect("paragraph");
        assert!(para
            .runs
            .iter()
            .any(|run| run.link == "https://ex.com" && run.text == "a"));
        assert!(para
            .runs
            .iter()
            .any(|run| run.link == "Note" && run.text == "Note"));
    }

    #[test]
    fn skips_front_matter_and_keeps_tasks() {
        let blocks = render_markdown_blocks(
            "---\ntitle: hidden\n---\n- [x] done\n- [ ] todo\n",
            None,
            80,
            16,
        );
        let text = blocks
            .iter()
            .map(|block| format!("{}{}", block.marker, dump_runs(block)))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!text.contains("hidden"), "{text}");
        assert!(text.contains("done"), "{text}");
        assert!(text.contains('☑'), "{text}");
        assert!(text.contains('☐'), "{text}");
    }

    #[test]
    fn mermaid_block_keeps_boxes() {
        let blocks =
            render_markdown_blocks("```mermaid\nflowchart LR\n  A --> B\n```\n", None, 40, 16);
        let pre = blocks.iter().find(|block| block.kind == BlockKind::Pre);
        let pre = pre.expect("diagram");
        assert!(pre.text.contains('┌'), "{}", pre.text);
        assert!(pre.text.contains('A'), "{}", pre.text);
    }
}
