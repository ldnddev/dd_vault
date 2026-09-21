use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Modal, Pane, PromptKind};
use crate::help::{build_help_text, build_theme_text, count_wrapped_lines};
use crate::toasts::render_toasts;
use crate::tree::VisibleRow;
use dd_vault_core::NodeKind;

pub fn draw(frame: &mut Frame, app: &mut App) {
    frame.render_widget(Block::default().style(app.theme.app_shell), frame.area());

    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_header(frame, app, root[0]);
    render_body(frame, app, root[1]);

    let footer = Paragraph::new(app.footer_hint(root[2].width)).style(app.theme.app_shell);
    frame.render_widget(footer, root[2]);

    if matches!(app.modal, Some(Modal::VaultPicker { .. })) {
        render_vault_picker(frame, app);
    } else if matches!(app.modal, Some(Modal::Prompt { .. })) {
        render_prompt(frame, app);
    } else if matches!(app.modal, Some(Modal::Confirm { .. })) {
        render_confirm(frame, app);
    } else if matches!(app.modal, Some(Modal::Finder { .. })) {
        render_finder(frame, app);
    } else if matches!(app.modal, Some(Modal::Notice { .. })) {
        render_notice(frame, app);
    } else if matches!(app.modal, Some(Modal::Palette { .. })) {
        render_palette(frame, app);
    }

    if app.show_help {
        render_scroll_modal(
            frame,
            app,
            "Key & Mouse bindings (F1 / Esc to close, j/k or arrows to scroll)",
            true,
        );
    }
    if app.show_theme {
        if let Some(editor) = app.theme_editor.clone() {
            render_theme_editor(frame, app, &editor);
        } else {
            render_scroll_modal(
                frame,
                app,
                "Theme (F2 / Esc to close, j/k or arrows to scroll)",
                false,
            );
        }
    }

    let lift = if !app.ai_layout_visible() {
        0
    } else {
        match app.ai.size {
            crate::ai::AiSize::Collapsed => 4,
            crate::ai::AiSize::Chat => 13,
            crate::ai::AiSize::Full => 0,
        }
    };
    render_toasts(app, frame, frame.area(), lift);
}

fn render_ai_overlay(frame: &mut Frame, app: &mut App, body: Rect) {
    use crate::ai::AiSize;
    let (w, h) = match app.ai.size {
        AiSize::Collapsed => (22u16, 3u16),
        AiSize::Chat => (40u16, 12u16),
        AiSize::Full => return,
    };
    let width = w.min(body.width.saturating_sub(1)).max(12);
    let height = h.min(body.height.saturating_sub(1)).max(3);
    let rect = Rect {
        x: body.x + body.width.saturating_sub(width + 1),
        y: body.y + body.height.saturating_sub(height),
        width,
        height,
    };
    render_ai_panel(frame, app, rect);
}

fn render_ai_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    use crate::ai::AiSize;
    if area.width < 8 || area.height < 3 {
        app.ai.area = Rect::default();
        return;
    }
    app.ai.area = area;
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(app.ai.title())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if app.ai.expanded() {
            app.theme.border_active
        } else {
            app.theme.border_default
        }))
        .title_style(
            Style::default()
                .fg(app.theme.modal_header)
                .add_modifier(Modifier::BOLD),
        )
        .style(
            Style::default()
                .bg(app.theme.modal_background)
                .fg(app.theme.modal_text),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    if app.ai.size == AiSize::Collapsed {
        let badge = format!("{}  <Space>ai", app.ai.badge());
        frame.render_widget(
            Paragraph::new(badge).style(Style::default().fg(app.theme.text_secondary)),
            inner,
        );
        return;
    }
    let mut lines: Vec<Line> = app
        .ai
        .transcript
        .iter()
        .flat_map(|t| t.lines().map(|l| Line::from(l.to_string())))
        .collect();
    if !app.ai.stream.is_empty() {
        for l in app.ai.stream.lines() {
            lines.push(Line::from(Span::styled(
                l.to_string(),
                Style::default().fg(app.theme.info),
            )));
        }
    }
    let input_h = 1u16;
    let text_h = inner.height.saturating_sub(input_h);
    let vis = text_h as usize;
    let max_scroll = lines.len().saturating_sub(vis.max(1));
    if app.ai.scroll as usize > max_scroll {
        app.ai.scroll = max_scroll as u16;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((app.ai.scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[0],
    );
    let hint = if app.ai.rx.is_some() {
        "Esc cancel".into()
    } else {
        format!("> {}█", app.ai.draft)
    };
    frame.render_widget(
        Paragraph::new(hint).style(
            Style::default()
                .fg(app.theme.input_text_focus)
                .bg(app.theme.modal_background),
        ),
        chunks[1],
    );
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let header_block = Block::default()
        .title(Span::styled(
            " dd_vault ",
            Style::default()
                .fg(app.theme.text_active_focus)
                .add_modifier(Modifier::BOLD),
        ))
        .title_top(
            Line::from(Span::styled(
                app.vault_header_label(),
                Style::default().fg(app.theme.text_secondary),
            ))
            .right_aligned(),
        )
        .borders(Borders::ALL)
        .border_style(app.theme.active_border)
        .style(app.theme.app_shell);
    frame.render_widget(header_block.clone(), area);
    if area.height >= 3 {
        let inner = header_block.inner(area);
        let quote = Paragraph::new(app.header_copy.as_str()).style(
            Style::default()
                .fg(app.theme.text_secondary)
                .bg(app.theme.base_background),
        );
        frame.render_widget(quote, inner);
    }
}

fn render_body(frame: &mut Frame, app: &mut App, area: Rect) {
    app.body_width = area.width;
    let show_tree = app.tree_layout_visible();
    let right;
    if show_tree {
        let auto = if area.width < 80 {
            (area.width / 3).clamp(12, 24)
        } else {
            (area.width / 4).clamp(18, 32)
        };
        let max_w = area.width.saturating_sub(20).max(12);
        let tree_width = if app.tree_split > 0 {
            app.tree_split.clamp(12, max_w)
        } else {
            auto.min(max_w)
        };
        let split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(tree_width), Constraint::Min(0)])
            .split(area);
        app.tree_area = split[0];
        right = split[1];
    } else {
        app.tree_area = Rect::default();
        right = area;
    }
    let ai_full = app.ai.size == crate::ai::AiSize::Full && app.ai_layout_visible();
    if app.preview_layout_visible() && right.height >= 10 && right.width > 0 {
        let auto = (right.height * 2 / 5).clamp(6, right.height.saturating_sub(5));
        let max_h = right.height.saturating_sub(4).max(4);
        let prev_h = if app.preview_split > 0 {
            app.preview_split.clamp(4, max_h)
        } else {
            auto.min(max_h)
        };
        let cols = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(prev_h)])
            .split(right);
        app.editor_area = cols[0];
        app.preview_area = cols[1];
    } else {
        app.editor_area = right;
        app.preview_area = Rect::default();
    }
    let tree_border = if app.pane == Pane::Tree {
        app.theme.border_active
    } else {
        app.theme.border_default
    };
    let editor_border = if app.pane == Pane::Editor {
        app.theme.border_active
    } else {
        app.theme.border_default
    };
    let preview_border = if app.pane == Pane::Preview {
        app.theme.border_active
    } else {
        app.theme.border_default
    };

    if app.tree_area.width > 0 {
        let count = app.tree.visible().len();
        let tree_title = tree_title(app, count);
        let tree_block = Block::default()
            .title(tree_title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(tree_border))
            .style(
                Style::default()
                    .fg(app.theme.text_primary)
                    .bg(app.theme.body_background),
            );
        let inner = tree_block.inner(app.tree_area);
        app.tree_inner = inner;
        frame.render_widget(tree_block, app.tree_area);
        let height = inner.height as usize;
        app.tree.ensure_visible(height);
        if app.vault.is_none() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "open a vault to begin",
                    Style::default().fg(app.theme.text_secondary),
                ))),
                inner,
            );
        } else {
            let rows = app.tree.visible();
            render_tree_rows(frame, app, inner, &rows);
            if rows.len() > height && height > 0 {
                paint_scrollbar(
                    frame,
                    inner,
                    app.tree.scroll,
                    rows.len(),
                    app.theme.scrollbar,
                    app.theme.scrollbar_hover,
                    app.theme.body_background,
                );
            }
        }
    } else {
        app.tree_inner = Rect::default();
    }

    if app.editor_area.width > 0 {
        render_editor(frame, app, editor_border);
    }
    if app.preview_area.height > 0 {
        if ai_full {
            render_ai_panel(frame, app, app.preview_area);
        } else {
            render_preview(frame, app, preview_border);
        }
    }
    if !ai_full && app.ai_layout_visible() {
        render_ai_overlay(frame, app, area);
    } else if !app.ai_layout_visible() {
        app.ai.area = Rect::default();
    }
}

fn render_theme_editor(frame: &mut Frame, app: &App, editor: &ldnddev_theme::ThemeEditor) {
    use ldnddev_theme::{theme_editor_rows, ThemeEditorRow};
    let area = centered_rect(80, 80, frame.area());
    frame.render_widget(Clear, area);
    let rows = theme_editor_rows(&editor.fields);
    let channel = ["R", "G", "B"][editor.channel.min(2)];
    let target = editor.save_target.label().to_uppercase();
    let mut lines = vec![
        Line::from(format!(
            "Source: {}   Schema: {}   Save: {target} (Tab)   Channel: {channel} ([/])",
            app.theme.source.label(),
            app.theme.version
        )),
        Line::from(if editor.editing_hex {
            format!("Hex: {}█   Y save   R reset   Esc revert", editor.hex_draft)
        } else {
            format!(
                "Hex: {}   Enter edit   +/- nudge   Y save   R reset   Esc revert",
                editor.hex_draft
            )
        }),
        Line::from(""),
    ];
    let view_h = area.height.saturating_sub(6) as usize;
    let start = rows
        .iter()
        .position(|row| match row {
            ThemeEditorRow::Color(idx) => *idx >= editor.scroll,
            ThemeEditorRow::Header(_) => false,
        })
        .unwrap_or(0);
    let start = if start > 0 && matches!(rows[start - 1], ThemeEditorRow::Header(_)) {
        start - 1
    } else {
        start
    };
    for row in rows.iter().skip(start).take(view_h.max(1)) {
        match row {
            ThemeEditorRow::Header(name) => lines.push(Line::from(Span::styled(
                (*name).to_string(),
                Style::default()
                    .fg(app.theme.modal_header)
                    .add_modifier(Modifier::BOLD),
            ))),
            ThemeEditorRow::Color(idx) => {
                let field = editor.fields[*idx];
                let hex = editor
                    .palette
                    .get(field.key)
                    .map(|c| c.to_hex())
                    .unwrap_or_else(|| "#000000".into());
                let cursor = if *idx == editor.selected { ">" } else { " " };
                let style = if *idx == editor.selected {
                    Style::default()
                        .fg(app.theme.text_active_focus)
                        .bg(app.theme.selected_background)
                } else {
                    Style::default().fg(app.theme.modal_text)
                };
                lines.push(Line::from(Span::styled(
                    format!("{cursor} {:<22} {hex}", field.key),
                    style,
                )));
            }
        }
    }
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title("F2 Theme editor")
                .borders(Borders::ALL)
                .border_style(app.theme.active_border)
                .title_style(
                    Style::default()
                        .fg(app.theme.modal_header)
                        .add_modifier(Modifier::BOLD),
                )
                .style(
                    Style::default()
                        .bg(app.theme.modal_background)
                        .fg(app.theme.modal_text),
                ),
        ),
        area,
    );
}

fn render_scroll_modal(frame: &mut Frame, app: &mut App, title: &str, is_help: bool) {
    let area = centered_rect(80, 80, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(title.to_string())
        .borders(Borders::ALL)
        .style(
            Style::default()
                .fg(app.theme.modal_text)
                .bg(app.theme.modal_background),
        )
        .border_style(Style::default().fg(app.theme.border_active))
        .title_style(
            Style::default()
                .fg(app.theme.modal_header)
                .add_modifier(Modifier::BOLD),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let body_w = inner.width.saturating_sub(2);
    let body_area = Rect {
        x: inner.x,
        y: inner.y,
        width: body_w,
        height: inner.height,
    };
    let text = if is_help {
        build_help_text(&app.theme, body_w as usize)
    } else {
        build_theme_text(&app.theme, &app.theme_status, body_w as usize)
    };
    let wrapped_total = count_wrapped_lines(&text);
    let max_scroll = wrapped_total.saturating_sub(inner.height as usize) as u16;
    if is_help {
        app.help_scroll_max = max_scroll;
        if app.help_scroll > max_scroll {
            app.help_scroll = max_scroll;
        }
    }
    let scroll = if is_help { app.help_scroll } else { 0 };
    frame.render_widget(
        Paragraph::new(text)
            .style(
                Style::default()
                    .fg(app.theme.modal_text)
                    .bg(app.theme.modal_background),
            )
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        body_area,
    );
}

fn render_vault_picker(frame: &mut Frame, app: &App) {
    let Some(Modal::VaultPicker { selected }) = &app.modal else {
        return;
    };
    let area = centered_rect(70, 50, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Open vault  (Enter open  d remove  Esc close) ")
        .borders(Borders::ALL)
        .border_style(app.theme.active_border)
        .title_style(
            Style::default()
                .fg(app.theme.modal_header)
                .add_modifier(Modifier::BOLD),
        )
        .style(
            Style::default()
                .bg(app.theme.modal_background)
                .fg(app.theme.modal_text),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = app
        .registry
        .vaults
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let marker = if i == *selected { ">" } else { " " };
            let style = if i == *selected {
                Style::default()
                    .fg(app.theme.text_active_focus)
                    .bg(app.theme.selected_background)
            } else {
                Style::default().fg(app.theme.modal_text)
            };
            Line::from(Span::styled(
                format!("{marker} {}  {}", entry.name, entry.path),
                style,
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_palette(frame: &mut Frame, app: &App) {
    let Some(Modal::Palette {
        query,
        selected,
        items,
    }) = &app.modal
    else {
        return;
    };
    let area = centered_rect(70, 70, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Command palette  (type, ↑↓, Enter, Esc) ")
        .borders(Borders::ALL)
        .border_style(app.theme.active_border)
        .title_style(
            Style::default()
                .fg(app.theme.modal_header)
                .add_modifier(Modifier::BOLD),
        )
        .style(
            Style::default()
                .bg(app.theme.modal_background)
                .fg(app.theme.modal_text),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(format!("> {query}█")).style(
            Style::default()
                .fg(app.theme.input_text_focus)
                .bg(app.theme.modal_background),
        ),
        chunks[0],
    );
    let lines: Vec<Line> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let marker = if i == *selected { ">" } else { " " };
            let style = if i == *selected {
                Style::default()
                    .fg(app.theme.text_active_focus)
                    .bg(app.theme.selected_background)
            } else {
                Style::default().fg(app.theme.modal_text)
            };
            Line::from(Span::styled(
                format!("{marker} {:<28}  {}", item.label, item.keys),
                style,
            ))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), chunks[1]);
}

fn render_finder(frame: &mut Frame, app: &App) {
    use crate::app::FinderKind;
    let Some(Modal::Finder {
        kind,
        query,
        selected,
        hits,
    }) = &app.modal
    else {
        return;
    };
    let title = match kind {
        FinderKind::Files => " Files  (type to filter, ↑↓, Enter, Esc) ",
        FinderKind::Content => " Search  (type to filter, ↑↓, Enter, Esc) ",
        FinderKind::Tags => " Tags  (type to filter, ↑↓, Enter, Esc) ",
        FinderKind::Wiki => " Wikilink  (type, #heading, Enter, Esc) ",
    };
    let area = centered_rect(80, 70, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(app.theme.active_border)
        .title_style(
            Style::default()
                .fg(app.theme.modal_header)
                .add_modifier(Modifier::BOLD),
        )
        .style(
            Style::default()
                .bg(app.theme.modal_background)
                .fg(app.theme.modal_text),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(format!("> {query}█")).style(
            Style::default()
                .fg(app.theme.input_text_focus)
                .bg(app.theme.modal_background),
        ),
        chunks[0],
    );
    let lines: Vec<Line> = hits
        .iter()
        .enumerate()
        .map(|(i, hit)| {
            let marker = if i == *selected { ">" } else { " " };
            let mut label = format!("{marker} {}", hit.title);
            if hit.title != hit.path {
                label.push_str(&format!("  {}", hit.path));
            }
            if let Some(sn) = &hit.snippet {
                label.push_str(&format!("  {sn}"));
            }
            let style = if i == *selected {
                Style::default()
                    .fg(app.theme.text_active_focus)
                    .bg(app.theme.selected_background)
            } else {
                Style::default().fg(app.theme.modal_text)
            };
            Line::from(Span::styled(label, style))
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), chunks[1]);
}

fn render_preview(frame: &mut Frame, app: &mut App, border: ratatui::style::Color) {
    use dd_render::{render_markdown_ex, PreviewPalette};

    let block = Block::default()
        .title(" preview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .style(
            Style::default()
                .fg(app.theme.text_primary)
                .bg(app.theme.body_background),
        );
    let inner = block.inner(app.preview_area);
    frame.render_widget(block, app.preview_area);
    app.preview_inner = inner;
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if app.editor.rel.is_none() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "open a note to preview",
                Style::default().fg(app.theme.text_secondary),
            ))),
            inner,
        );
        return;
    }

    let pal = PreviewPalette {
        text: app.theme.text_primary,
        muted: app.theme.text_secondary,
        heading: app.theme.text_labels,
        focus: app.theme.text_active_focus,
        link: app.theme.links,
        code: app.theme.files,
        quote: app.theme.text_secondary,
        success: app.theme.success,
        warning: app.theme.warning,
        error: app.theme.error,
        info: app.theme.info,
        background: app.theme.body_background,
    };
    let src = app.editor.text();
    let vault_root = app.vault.as_ref().map(|v| v.root.clone());
    let note_rel = app.editor.rel.as_ref().map(std::path::PathBuf::from);
    let tree = &app.tree;
    let resolve = |target: &str| -> Option<String> {
        let name = target.split('#').next()?.split('|').next()?.trim();
        if dd_render::looks_like_image(name) {
            return None;
        }
        let rel = tree.find_file(name)?;
        let root = vault_root.as_ref()?;
        let path = root.join(rel);
        let raw = std::fs::read_to_string(path).ok()?;
        let mut lines: Vec<&str> = raw.lines().take(80).collect();
        if raw.lines().count() > 80 {
            lines.push("…");
        }
        Some(lines.join("\n"))
    };
    let resolve_image = |url: &str| -> Option<Vec<u8>> {
        let root = vault_root.as_ref()?;
        if let Some(path) = crate::images::resolve_image_path(root, note_rel.as_deref(), url) {
            return std::fs::read(path).ok();
        }
        let base = std::path::Path::new(url).file_name()?.to_string_lossy();
        let rel = tree.find_file(base.as_ref())?;
        std::fs::read(root.join(rel)).ok()
    };
    let max_w = inner.width.saturating_sub(1).max(1);
    let max_rows = inner.height.clamp(1, 16);
    let text = render_markdown_ex(
        &src,
        pal,
        Some(&resolve),
        Some(&resolve_image),
        max_w,
        max_rows,
    );
    let total = text.lines.len();
    let vis = inner.height as usize;
    let max_scroll = total.saturating_sub(vis);
    if app.preview_scroll as usize > max_scroll {
        app.preview_scroll = max_scroll as u16;
    }
    frame.render_widget(
        Paragraph::new(text)
            .style(
                Style::default()
                    .fg(app.theme.text_primary)
                    .bg(app.theme.body_background),
            )
            .wrap(Wrap { trim: false })
            .scroll((app.preview_scroll, 0)),
        inner,
    );
    if total > vis && vis > 0 {
        paint_scrollbar(
            frame,
            inner,
            app.preview_scroll as usize,
            total,
            app.theme.scrollbar,
            app.theme.scrollbar_hover,
            app.theme.body_background,
        );
    }
}

fn render_editor(frame: &mut Frame, app: &mut App, editor_border: ratatui::style::Color) {
    use crate::highlight::highlight_line;
    use dd_edit::Mode;

    let block = Block::default()
        .title(app.editor_title())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(editor_border))
        .style(
            Style::default()
                .fg(app.theme.text_primary)
                .bg(app.theme.body_background),
        );
    let inner = block.inner(app.editor_area);
    frame.render_widget(block, app.editor_area);
    app.editor_inner = inner;
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let cmd = matches!(app.editor.mode, Mode::Command | Mode::Search);
    let text_h = if cmd {
        inner.height.saturating_sub(1)
    } else {
        inner.height
    };
    app.editor.ensure_scroll(text_h as usize);

    if app.editor.rel.is_none() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "open a note from the tree",
                Style::default().fg(app.theme.text_secondary),
            )))
            .style(
                Style::default()
                    .fg(app.theme.text_primary)
                    .bg(app.theme.body_background),
            ),
            inner,
        );
        return;
    }

    let (cur_line, cur_col) = app.editor.cursor_line_col();
    let visual = app.editor.visual_highlight();
    let lines_n = app.editor.line_count();
    let gutter_w = app.editor.gutter_cols().saturating_sub(1) as usize;
    let mut lines: Vec<Line> = Vec::new();
    for row in 0..text_h as usize {
        let idx = app.editor.scroll + row;
        if idx >= lines_n {
            lines.push(Line::from(""));
            continue;
        }
        let gutter = Span::styled(
            format!("{:>gutter_w$} ", idx + 1),
            Style::default().fg(app.theme.text_secondary),
        );
        let raw = app.editor.line(idx);
        let mut content = highlight_line(&raw, &app.theme);
        if let Some((a, b)) = visual {
            let start = line_char_start(&app.editor, idx);
            content = apply_selection(content, start, a, b, &app.theme);
        }
        let mut spans = vec![gutter];
        if idx == cur_line {
            spans.extend(with_cursor(
                content,
                cur_col,
                &app.theme,
                app.editor.mode == Mode::Insert,
            ));
        } else {
            spans.extend(content.spans);
        }
        lines.push(Line::from(spans));
    }
    let text_area = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: text_h,
    };
    frame.render_widget(
        Paragraph::new(lines).style(
            Style::default()
                .fg(app.theme.text_primary)
                .bg(app.theme.body_background),
        ),
        text_area,
    );

    if cmd {
        let prefix = if app.editor.mode == Mode::Search {
            "/"
        } else {
            ":"
        };
        let cmd_area = Rect {
            x: inner.x,
            y: inner.y + text_h,
            width: inner.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(format!("{prefix}{}█", app.editor.cmdline)).style(
                Style::default()
                    .fg(app.theme.input_text_focus)
                    .bg(app.theme.modal_background),
            ),
            cmd_area,
        );
    }
}

fn apply_selection(
    line: Line<'_>,
    line_start: usize,
    a: usize,
    b: usize,
    theme: &crate::theme::AppTheme,
) -> Line<'static> {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    if text.is_empty() {
        if a <= line_start && b > line_start {
            return Line::from(Span::styled(
                " ",
                Style::default()
                    .fg(theme.text_active_focus)
                    .bg(theme.selected_background),
            ));
        }
        return Line::from(text);
    }
    let sel = Style::default()
        .fg(theme.text_active_focus)
        .bg(theme.selected_background);
    let mut spans = Vec::new();
    let mut col = 0usize;
    for span in line.spans {
        let s = span.content.as_ref();
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            let abs = line_start + col + i;
            let selected = abs >= a && abs < b;
            let mut j = i + 1;
            while j < chars.len() {
                let absj = line_start + col + j;
                if (absj >= a && absj < b) != selected {
                    break;
                }
                j += 1;
            }
            let chunk: String = chars[i..j].iter().collect();
            if selected {
                spans.push(Span::styled(chunk, sel));
            } else {
                spans.push(Span::styled(chunk, span.style));
            }
            i = j;
        }
        col += chars.len();
    }
    Line::from(spans)
}

fn line_char_start(editor: &dd_edit::Editor, line: usize) -> usize {
    // Approximate: sum lengths of previous lines + newlines.
    let mut n = 0usize;
    for i in 0..line {
        n += editor.line(i).chars().count() + 1;
    }
    n
}

fn with_cursor(
    line: Line<'_>,
    col: usize,
    theme: &crate::theme::AppTheme,
    insert: bool,
) -> Vec<Span<'static>> {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    let chars: Vec<char> = text.chars().collect();
    let bar = Span::styled(
        "▏",
        Style::default()
            .fg(theme.cursor)
            .add_modifier(Modifier::SLOW_BLINK),
    );
    let mut out = Vec::new();
    if chars.is_empty() {
        if insert {
            out.push(bar);
        } else {
            out.push(Span::styled(
                " ",
                Style::default().fg(theme.base_background).bg(theme.cursor),
            ));
        }
        return out;
    }
    let mut i = 0usize;
    let mut placed = false;
    for span in line.spans {
        let s = span.content.as_ref();
        let n = s.chars().count();
        if col < i || col >= i + n {
            out.push(Span::styled(s.to_string(), span.style));
        } else {
            let rel = col - i;
            let pre: String = s.chars().take(rel).collect();
            let ch = s.chars().nth(rel).unwrap_or(' ');
            let post: String = s.chars().skip(rel + 1).collect();
            if !pre.is_empty() {
                out.push(Span::styled(pre, span.style));
            }
            if insert {
                out.push(bar.clone());
                out.push(Span::styled(ch.to_string(), span.style));
            } else {
                out.push(Span::styled(
                    ch.to_string(),
                    span.style.bg(theme.cursor).fg(theme.base_background),
                ));
            }
            placed = true;
            if !post.is_empty() {
                out.push(Span::styled(post, span.style));
            }
        }
        i += n;
    }
    if col >= chars.len() || (insert && !placed) {
        if insert {
            out.push(bar);
        } else {
            out.push(Span::styled(
                " ",
                Style::default().fg(theme.base_background).bg(theme.cursor),
            ));
        }
    }
    out
}

fn tree_title(app: &App, count: usize) -> String {
    let name = app
        .vault
        .as_ref()
        .map(|v| v.name.as_str())
        .unwrap_or("vault");
    if app.tree.filtering || !app.tree.filter.is_empty() {
        format!(" {name} /{} ({count}) ", app.tree.filter)
    } else if app.vault.is_some() {
        format!(" {name} ({count}) ")
    } else {
        format!(" {name} ")
    }
}

fn render_tree_rows(frame: &mut Frame, app: &App, inner: Rect, rows: &[VisibleRow]) {
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let selected = i == app.tree.selected && app.pane == Pane::Tree;
            if selected {
                Line::from(Span::styled(
                    format!("{}{}", row.prefix, row.name),
                    Style::default()
                        .fg(app.theme.text_active_focus)
                        .bg(app.theme.selected_background),
                ))
            } else {
                let color = match row.kind {
                    NodeKind::Dir => app.theme.folders,
                    NodeKind::File => app.theme.files,
                    NodeKind::Symlink => app.theme.links,
                };
                Line::from(vec![
                    Span::styled(
                        row.prefix.clone(),
                        Style::default().fg(app.theme.text_secondary),
                    ),
                    Span::styled(row.name.clone(), Style::default().fg(color)),
                ])
            }
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines)
            .style(
                Style::default()
                    .fg(app.theme.text_primary)
                    .bg(app.theme.body_background),
            )
            .scroll((app.tree.scroll as u16, 0)),
        inner,
    );
}

fn render_prompt(frame: &mut Frame, app: &App) {
    let Some(Modal::Prompt { kind, draft }) = &app.modal else {
        return;
    };
    let title = match kind {
        PromptKind::New { parent } => {
            let loc = if parent.as_os_str().is_empty() {
                "/".to_string()
            } else {
                parent.display().to_string()
            };
            format!(" New in {loc}  (name/ = folder) ")
        }
        PromptKind::Rename { rel } => format!(" Rename {} ", rel.display()),
        PromptKind::GitCommit { then_push: false } => " Git commit message ".to_string(),
        PromptKind::GitCommit { then_push: true } => {
            " Commit all changes, then push ".to_string()
        }
    };
    let area = centered_rect(70, 30, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.input_border_focus))
        .title_style(
            Style::default()
                .fg(app.theme.modal_header)
                .add_modifier(Modifier::BOLD),
        )
        .style(
            Style::default()
                .bg(app.theme.modal_background)
                .fg(app.theme.modal_text),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let input = format!("{draft}█");
    let hint = "Enter confirm   Esc cancel";
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(input).style(
            Style::default()
                .fg(app.theme.input_text_focus)
                .bg(app.theme.modal_background),
        ),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(hint).style(Style::default().fg(app.theme.text_secondary)),
        chunks[1],
    );
}

fn render_notice(frame: &mut Frame, app: &App) {
    let Some(Modal::Notice { title, message }) = &app.modal else {
        return;
    };
    let area = centered_rect(70, 40, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(title.clone())
        .borders(Borders::ALL)
        .border_style(app.theme.active_border)
        .title_style(
            Style::default()
                .fg(app.theme.modal_header)
                .add_modifier(Modifier::BOLD),
        )
        .style(
            Style::default()
                .bg(app.theme.modal_background)
                .fg(app.theme.modal_text),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines: Vec<Line> = message.lines().map(|l| Line::from(l.to_string())).collect();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter / Esc close",
        Style::default().fg(app.theme.text_secondary),
    )));
    let body = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(body, inner);
}

fn render_confirm(frame: &mut Frame, app: &App) {
    let Some(Modal::Confirm { message, .. }) = &app.modal else {
        return;
    };
    let tall = matches!(
        app.modal,
        Some(Modal::Confirm {
            kind: crate::app::ConfirmKind::AiSend,
            ..
        })
    );
    let area = if tall {
        centered_rect(70, 50, frame.area())
    } else {
        centered_rect(60, 25, frame.area())
    };
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Confirm ")
        .borders(Borders::ALL)
        .border_style(app.theme.active_border)
        .title_style(
            Style::default()
                .fg(app.theme.modal_header)
                .add_modifier(Modifier::BOLD),
        )
        .style(
            Style::default()
                .bg(app.theme.modal_background)
                .fg(app.theme.modal_text),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines: Vec<Line> = message.lines().map(|l| Line::from(l.to_string())).collect();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "y confirm   n / Esc cancel",
        Style::default().fg(app.theme.text_secondary),
    )));
    let body = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(body, inner);
}

fn paint_scrollbar(
    frame: &mut Frame,
    inner: Rect,
    scroll: usize,
    total: usize,
    scrollbar: ratatui::style::Color,
    hover: ratatui::style::Color,
    bg: ratatui::style::Color,
) {
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let track_x = inner.x + inner.width.saturating_sub(1);
    for row in 0..inner.height {
        frame.render_widget(
            Paragraph::new("│").style(Style::default().fg(scrollbar).bg(bg)),
            Rect {
                x: track_x,
                y: inner.y + row,
                width: 1,
                height: 1,
            },
        );
    }
    let total_h = inner.height as usize;
    let thumb_h = ((total_h * total_h) / total.max(1)).max(1).min(total_h);
    let scroll_range = total.saturating_sub(total_h).max(1);
    let thumb_top = (scroll * total_h.saturating_sub(thumb_h)) / scroll_range;
    for i in 0..thumb_h {
        let y = inner.y + (thumb_top + i) as u16;
        if y >= inner.y + inner.height {
            break;
        }
        frame.render_widget(
            Paragraph::new("█").style(Style::default().fg(hover).bg(bg)),
            Rect {
                x: track_x,
                y,
                width: 1,
                height: 1,
            },
        );
    }
}

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
