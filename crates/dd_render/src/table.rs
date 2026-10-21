//! GFM tables as a Unicode box fitted to the preview width.
//!
//! Cell text wraps inside its column. A long row must not become one line
//! wider than the pane: the preview widget would wrap that line and split the
//! vertical rules.

use comrak::nodes::{AstNode, NodeValue, TableAlignment};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

use crate::{prefix_spans, PreviewPalette, RenderCtx};

#[derive(Clone)]
struct Piece {
    text: String,
    style: Style,
    width: usize,
}

struct ParsedRow {
    header: bool,
    cells: Vec<Vec<Piece>>,
}

struct Cell {
    lines: Vec<Vec<Piece>>,
}

pub(super) fn append<'a>(
    node: &'a AstNode<'a>,
    ctx: &RenderCtx<'_>,
    out: &mut Vec<Line<'static>>,
    indent: usize,
    alignments: &[TableAlignment],
    columns: usize,
) {
    let mut rows = collect_rows(node, ctx, indent);
    if rows.is_empty() {
        return;
    }
    let n = columns.max(rows.iter().map(|r| r.cells.len()).max().unwrap_or(0));
    if n == 0 {
        return;
    }
    for row in &mut rows {
        row.cells.resize_with(n, Vec::new);
    }
    if !rows.iter().any(|r| r.header) {
        rows[0].header = true;
    }

    let available = usize::from(ctx.max_width).saturating_sub(indent.saturating_mul(2));
    let mut natural = vec![0usize; n];
    for row in &rows {
        for (i, cell) in row.cells.iter().enumerate() {
            natural[i] = natural[i].max(pieces_width(cell));
        }
    }

    if let Some((widths, pad)) = layout(&natural, available) {
        emit_grid(out, ctx.p, indent, alignments, &rows, &widths, pad);
    } else {
        emit_stacked(out, ctx.p, indent, &rows, available.max(1));
    }
}

fn collect_rows<'a>(node: &'a AstNode<'a>, ctx: &RenderCtx<'_>, indent: usize) -> Vec<ParsedRow> {
    let mut rows = Vec::new();
    for row in node.children() {
        let header = matches!(row.data.borrow().value, NodeValue::TableRow(true));
        let mut cells = Vec::new();
        for cell in row.children() {
            let mut spans = Vec::new();
            ctx.pending.borrow_mut().clear();
            super::collect_inlines(cell, ctx, &mut spans, None, indent);
            ctx.pending.borrow_mut().clear();
            cells.push(pieces_of(&spans));
        }
        rows.push(ParsedRow { header, cells });
    }
    rows
}

fn layout(natural: &[usize], available: usize) -> Option<(Vec<usize>, usize)> {
    let n = natural.len();
    if n == 0 || available == 0 {
        return None;
    }
    for pad in [1usize, 0] {
        let chrome = n * pad * 2 + n + 1;
        if chrome > available {
            continue;
        }
        let widths = fit_widths(natural, available - chrome);
        if grid_width(&widths, pad) <= available {
            return Some((widths, pad));
        }
    }
    None
}

fn fit_widths(natural: &[usize], budget: usize) -> Vec<usize> {
    let mut widths = natural.to_vec();
    let sum: usize = widths.iter().sum();
    if sum <= budget {
        return widths;
    }
    let mut overflow = sum - budget;
    while overflow > 0 {
        let Some(i) = widths
            .iter()
            .enumerate()
            .filter(|(_, w)| **w > 1)
            .max_by_key(|(_, w)| *w)
            .map(|(i, _)| i)
        else {
            break;
        };
        widths[i] -= 1;
        overflow -= 1;
    }
    widths
}

fn grid_width(widths: &[usize], pad: usize) -> usize {
    let n = widths.len();
    widths.iter().sum::<usize>() + n * pad * 2 + n + 1
}

fn emit_grid(
    out: &mut Vec<Line<'static>>,
    p: PreviewPalette,
    indent: usize,
    alignments: &[TableAlignment],
    rows: &[ParsedRow],
    widths: &[usize],
    pad: usize,
) {
    let wrapped: Vec<CellRow> = rows
        .iter()
        .map(|row| {
            let mut cells = Vec::with_capacity(widths.len());
            let mut height = 1usize;
            for (i, cell) in row.cells.iter().enumerate() {
                let lines = wrap_pieces(cell, widths[i]);
                height = height.max(lines.len());
                cells.push(Cell { lines });
            }
            CellRow {
                header: row.header,
                cells,
                height,
            }
        })
        .collect();
    let has_body = wrapped.iter().any(|r| !r.header);

    push_rule(out, p, indent, widths, pad, ('┌', '┬', '┐'));
    for row in &wrapped {
        for v in 0..row.height {
            let mut spans = prefix_spans(indent);
            spans.extend(content_spans(p, alignments, widths, pad, row, v));
            out.push(Line::from(spans));
        }
        if row.header && has_body {
            push_rule(out, p, indent, widths, pad, ('├', '┼', '┤'));
        }
    }
    push_rule(out, p, indent, widths, pad, ('└', '┴', '┘'));
}

struct CellRow {
    header: bool,
    cells: Vec<Cell>,
    height: usize,
}

fn push_rule(
    out: &mut Vec<Line<'static>>,
    p: PreviewPalette,
    indent: usize,
    widths: &[usize],
    pad: usize,
    edges: (char, char, char),
) {
    let (left, join, right) = edges;
    let mut s = String::new();
    s.push(left);
    for (i, w) in widths.iter().enumerate() {
        if i > 0 {
            s.push(join);
        }
        s.push_str(&"─".repeat(w + pad * 2));
    }
    s.push(right);
    let mut spans = prefix_spans(indent);
    spans.push(Span::styled(s, p.muted()));
    out.push(Line::from(spans));
}

fn content_spans(
    p: PreviewPalette,
    alignments: &[TableAlignment],
    widths: &[usize],
    pad: usize,
    row: &CellRow,
    visual: usize,
) -> Vec<Span<'static>> {
    let plain = p.text();
    let heading = p.heading();
    let border = p.muted();
    let mut spans = Vec::new();
    spans.push(Span::styled("│", border));
    for (i, w) in widths.iter().enumerate() {
        if pad > 0 {
            spans.push(Span::styled(" ".repeat(pad), plain));
        }
        let pieces = row
            .cells
            .get(i)
            .and_then(|c| c.lines.get(visual))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let (left, right) = side_pad(pieces, *w, align_at(alignments, i));
        if left > 0 {
            spans.push(Span::styled(" ".repeat(left), plain));
        }
        for piece in pieces {
            let style = if row.header {
                emphasize(piece.style, plain, heading)
            } else {
                piece.style
            };
            if !piece.text.is_empty() {
                spans.push(Span::styled(piece.text.clone(), style));
            }
        }
        if right > 0 {
            spans.push(Span::styled(" ".repeat(right), plain));
        }
        if pad > 0 {
            spans.push(Span::styled(" ".repeat(pad), plain));
        }
        spans.push(Span::styled("│", border));
    }
    spans
}

fn side_pad(pieces: &[Piece], width: usize, align: TableAlignment) -> (usize, usize) {
    let content = pieces.iter().map(|p| p.width).sum::<usize>();
    let pad = width.saturating_sub(content);
    match align {
        TableAlignment::Right => (pad, 0),
        TableAlignment::Center => (pad / 2, pad - pad / 2),
        TableAlignment::Left | TableAlignment::None => (0, pad),
    }
}

fn align_at(alignments: &[TableAlignment], index: usize) -> TableAlignment {
    alignments
        .get(index)
        .copied()
        .unwrap_or(TableAlignment::None)
}

fn emphasize(style: Style, plain: Style, heading: Style) -> Style {
    if style.fg == plain.fg {
        heading.add_modifier(style.add_modifier | Modifier::BOLD)
    } else {
        style.add_modifier(Modifier::BOLD)
    }
}

fn emit_stacked(
    out: &mut Vec<Line<'static>>,
    p: PreviewPalette,
    indent: usize,
    rows: &[ParsedRow],
    available: usize,
) {
    let plain = p.text();
    let heading = p.heading();
    let width = available.max(1);
    for (ri, row) in rows.iter().enumerate() {
        if ri > 0 {
            out.push(Line::from(""));
        }
        for (ci, cell) in row.cells.iter().enumerate() {
            if cell.is_empty() {
                continue;
            }
            let gutter = if ci == 0 {
                0
            } else {
                2.min(width.saturating_sub(1))
            };
            let wrap_w = width.saturating_sub(gutter).max(1);
            for line in wrap_pieces(cell, wrap_w) {
                let mut spans = prefix_spans(indent);
                if gutter > 0 {
                    spans.push(Span::raw(" ".repeat(gutter)));
                }
                for piece in line {
                    let style = if row.header {
                        emphasize(piece.style, plain, heading)
                    } else {
                        piece.style
                    };
                    if !piece.text.is_empty() {
                        spans.push(Span::styled(piece.text, style));
                    }
                }
                if spans.iter().any(|s| !s.content.trim().is_empty()) {
                    out.push(Line::from(spans));
                }
            }
        }
    }
}

fn pieces_of(spans: &[Span<'static>]) -> Vec<Piece> {
    let mut out = Vec::new();
    for span in spans {
        let mut word = String::new();
        let mut word_w = 0usize;
        for ch in span.content.chars() {
            if ch.is_whitespace() {
                push_word(&mut out, &mut word, &mut word_w, span.style);
                continue;
            }
            word.push(ch);
            word_w += char_width(ch);
        }
        push_word(&mut out, &mut word, &mut word_w, span.style);
    }
    out
}

fn push_word(out: &mut Vec<Piece>, word: &mut String, word_w: &mut usize, style: Style) {
    if word.is_empty() {
        return;
    }
    out.push(Piece {
        text: std::mem::take(word),
        style,
        width: *word_w,
    });
    *word_w = 0;
}

fn pieces_width(pieces: &[Piece]) -> usize {
    if pieces.is_empty() {
        0
    } else {
        pieces.iter().map(|p| p.width).sum::<usize>() + pieces.len() - 1
    }
}

fn wrap_pieces(pieces: &[Piece], width: usize) -> Vec<Vec<Piece>> {
    if pieces.is_empty() || width == 0 {
        return vec![Vec::new()];
    }
    let mut lines: Vec<Vec<Piece>> = Vec::new();
    let mut line: Vec<Piece> = Vec::new();
    let mut col = 0usize;
    for piece in pieces {
        for chunk in split_piece(piece, width) {
            if !line.is_empty() && col + 1 + chunk.width > width {
                lines.push(std::mem::take(&mut line));
                col = 0;
            }
            if !line.is_empty() {
                line.push(Piece {
                    text: " ".to_string(),
                    style: chunk.style,
                    width: 1,
                });
                col += 1;
            }
            col += chunk.width;
            line.push(chunk);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        vec![Vec::new()]
    } else {
        lines
    }
}

fn split_piece(piece: &Piece, width: usize) -> Vec<Piece> {
    if piece.width <= width {
        vec![piece.clone()]
    } else {
        split_word(&piece.text, piece.style, width.max(1))
    }
}

fn split_word(text: &str, style: Style, width: usize) -> Vec<Piece> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut buf_w = 0usize;
    for ch in text.chars() {
        let cw = char_width(ch);
        if cw > 0 && buf_w > 0 && buf_w + cw > width {
            out.push(Piece {
                text: std::mem::take(&mut buf),
                style,
                width: buf_w,
            });
            buf_w = 0;
        }
        buf.push(ch);
        buf_w += cw;
    }
    if !buf.is_empty() {
        out.push(Piece {
            text: buf,
            style,
            width: buf_w,
        });
    }
    if out.is_empty() {
        out.push(Piece {
            text: String::new(),
            style,
            width: 0,
        });
    }
    out
}

fn char_width(ch: char) -> usize {
    UnicodeWidthChar::width(ch).unwrap_or(0)
}
