use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use dd_edit::{Key as EditKey, Mode};

use crate::app::{
    App, ConfirmKind, FinderKind, Leader, Modal, MouseDrag, Pane, SplitDrag, DOUBLE_CLICK_MS,
};
use crate::theme::{apply_palette, extra_theme_fields, palette_from_theme, THEME_FILENAME};
use crate::toasts::ToastLevel;

pub fn handle_event(app: &mut App, evt: Event) -> Result<()> {
    if app.show_help {
        return handle_help(app, evt);
    }
    if app.show_theme {
        return handle_theme(app, evt);
    }
    if app.modal.is_some() {
        return handle_modal(app, evt);
    }

    match evt {
        Event::Key(k) => handle_key(app, k),
        Event::Mouse(m) => handle_mouse(app, m),
        _ => Ok(()),
    }
}

fn handle_help(app: &mut App, evt: Event) -> Result<()> {
    match evt {
        Event::Key(k) => match k.code {
            KeyCode::F(1) | KeyCode::Esc => {
                app.show_help = false;
                app.help_scroll = 0;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.help_scroll = app.help_scroll.saturating_add(1).min(app.help_scroll_max);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.help_scroll = app.help_scroll.saturating_sub(1);
            }
            KeyCode::PageDown => {
                app.help_scroll = app.help_scroll.saturating_add(10).min(app.help_scroll_max);
            }
            KeyCode::PageUp => {
                app.help_scroll = app.help_scroll.saturating_sub(10);
            }
            KeyCode::Home | KeyCode::Char('g') => app.help_scroll = 0,
            KeyCode::End | KeyCode::Char('G') => app.help_scroll = app.help_scroll_max,
            _ => {}
        },
        Event::Mouse(m) => match m.kind {
            MouseEventKind::ScrollUp => app.help_scroll = app.help_scroll.saturating_sub(3),
            MouseEventKind::ScrollDown => {
                app.help_scroll = app.help_scroll.saturating_add(3).min(app.help_scroll_max);
            }
            _ => {}
        },
        _ => {}
    }
    Ok(())
}

fn handle_theme(app: &mut App, evt: Event) -> Result<()> {
    match evt {
        Event::Key(k) => {
            if k.code == KeyCode::F(2) {
                close_theme(app, true);
            } else if let Some(ek) = map_editor_key(k) {
                dispatch_theme_editor(app, ek, k.modifiers.contains(KeyModifiers::SHIFT));
            }
        }
        Event::Mouse(m) => match m.kind {
            MouseEventKind::ScrollUp => {
                if let Some(editor) = app.theme_editor.as_mut() {
                    editor.select(editor.selected.saturating_sub(1));
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(editor) = app.theme_editor.as_mut() {
                    editor.select(editor.selected.saturating_add(1));
                }
            }
            _ => {}
        },
        _ => {}
    }
    Ok(())
}

fn handle_modal(app: &mut App, evt: Event) -> Result<()> {
    let Event::Key(k) = evt else {
        return Ok(());
    };
    if k.code == KeyCode::F(1) {
        app.show_help = true;
        app.help_scroll = 0;
        return Ok(());
    }
    if matches!(app.modal, Some(Modal::VaultPicker { .. })) {
        handle_picker_key(app, k);
    } else if matches!(app.modal, Some(Modal::Prompt { .. })) {
        handle_prompt_key(app, k);
    } else if matches!(app.modal, Some(Modal::Confirm { .. })) {
        handle_confirm_key(app, k);
    } else if matches!(app.modal, Some(Modal::Finder { .. })) {
        handle_finder_key(app, k);
    } else if matches!(app.modal, Some(Modal::Palette { .. })) {
        handle_palette_key(app, k);
    } else if matches!(app.modal, Some(Modal::Notice { .. }))
        && matches!(k.code, KeyCode::Esc | KeyCode::Enter)
    {
        app.modal = None;
    }
    Ok(())
}

fn handle_picker_key(app: &mut App, k: KeyEvent) {
    match k.code {
        KeyCode::Esc => app.modal = None,
        KeyCode::Enter => app.activate_picker_selection(),
        KeyCode::Char('d') => app.begin_forget_vault(),
        KeyCode::Down | KeyCode::Char('j') => move_picker(app, 1),
        KeyCode::Up | KeyCode::Char('k') => move_picker(app, -1),
        KeyCode::Home | KeyCode::Char('g') => {
            if let Some(Modal::VaultPicker { selected }) = &mut app.modal {
                *selected = 0;
            }
        }
        KeyCode::End | KeyCode::Char('G') => {
            if let Some(Modal::VaultPicker { selected }) = &mut app.modal {
                *selected = app.registry.vaults.len().saturating_sub(1);
            }
        }
        _ => {}
    }
}

fn handle_prompt_key(app: &mut App, k: KeyEvent) {
    match k.code {
        KeyCode::Esc => app.modal = None,
        KeyCode::Enter => app.submit_prompt(),
        KeyCode::Backspace => {
            if let Some(Modal::Prompt { draft, .. }) = &mut app.modal {
                draft.pop();
            }
        }
        KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) && !c.is_control() => {
            if let Some(Modal::Prompt { draft, .. }) = &mut app.modal {
                draft.push(c);
            }
        }
        _ => {}
    }
}

fn handle_finder_key(app: &mut App, k: KeyEvent) {
    match k.code {
        KeyCode::Esc => app.modal = None,
        KeyCode::Enter => app.activate_finder(),
        KeyCode::Down => finder_move(app, 1),
        KeyCode::Up => finder_move(app, -1),
        KeyCode::Char('n') if k.modifiers.contains(KeyModifiers::CONTROL) => finder_move(app, 1),
        KeyCode::Char('p') if k.modifiers.contains(KeyModifiers::CONTROL) => finder_move(app, -1),
        KeyCode::Backspace => {
            if let Some(Modal::Finder { query, .. }) = &mut app.modal {
                query.pop();
            }
            app.refresh_finder();
        }
        KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) && !c.is_control() => {
            if let Some(Modal::Finder { query, .. }) = &mut app.modal {
                query.push(c);
            }
            app.refresh_finder();
        }
        _ => {}
    }
}

fn handle_palette_key(app: &mut App, k: KeyEvent) {
    match k.code {
        KeyCode::Esc => app.modal = None,
        KeyCode::Enter => app.activate_palette(),
        KeyCode::Down => palette_move(app, 1),
        KeyCode::Up => palette_move(app, -1),
        KeyCode::Char('n') if k.modifiers.contains(KeyModifiers::CONTROL) => palette_move(app, 1),
        KeyCode::Char('p') if k.modifiers.contains(KeyModifiers::CONTROL) => palette_move(app, -1),
        KeyCode::Backspace => {
            if let Some(Modal::Palette { query, .. }) = &mut app.modal {
                query.pop();
            }
            app.refresh_palette();
        }
        KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) && !c.is_control() => {
            if let Some(Modal::Palette { query, .. }) = &mut app.modal {
                query.push(c);
            }
            app.refresh_palette();
        }
        _ => {}
    }
}

fn palette_move(app: &mut App, delta: i32) {
    if let Some(Modal::Palette {
        selected, items, ..
    }) = &mut app.modal
    {
        if items.is_empty() {
            *selected = 0;
            return;
        }
        let next = *selected as i32 + delta;
        *selected = next.clamp(0, items.len() as i32 - 1) as usize;
    }
}

fn finder_move(app: &mut App, delta: i32) {
    if let Some(Modal::Finder { selected, hits, .. }) = &mut app.modal {
        if hits.is_empty() {
            *selected = 0;
            return;
        }
        let next = *selected as i32 + delta;
        *selected = next.clamp(0, hits.len() as i32 - 1) as usize;
    }
}

fn handle_confirm_key(app: &mut App, k: KeyEvent) {
    match k.code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            if let Some(Modal::Confirm {
                kind: ConfirmKind::ReloadDisk { rel },
                ..
            }) = &app.modal
            {
                app.suppress_disk_prompt = Some(rel.clone());
            }
            app.modal = None;
        }
        KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => app.confirm_action(),
        _ => {}
    }
}

fn move_picker(app: &mut App, delta: i32) {
    let Some(Modal::VaultPicker { selected }) = &mut app.modal else {
        return;
    };
    let n = app.registry.vaults.len();
    if n == 0 {
        return;
    }
    let next = *selected as i32 + delta;
    *selected = next.clamp(0, n as i32 - 1) as usize;
}

fn handle_key(app: &mut App, k: KeyEvent) -> Result<()> {
    if k.code == KeyCode::Char('q') && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.leader = Leader::None;
        app.request_quit(false);
        return Ok(());
    }
    if k.code == KeyCode::Char('s') && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.save_note();
        return Ok(());
    }
    if k.code == KeyCode::Char('v')
        && k.modifiers.contains(KeyModifiers::CONTROL)
        && app.try_paste_clipboard_image()
    {
        return Ok(());
    }
    if k.code == KeyCode::F(1) {
        app.leader = Leader::None;
        app.show_help = true;
        app.help_scroll = 0;
        return Ok(());
    }
    if k.code == KeyCode::F(2) {
        app.leader = Leader::None;
        app.show_theme = true;
        app.theme_editor = Some(ldnddev_theme::ThemeEditor::new(
            palette_from_theme(&app.theme),
            extra_theme_fields(),
        ));
        return Ok(());
    }

    let typing = app.pane == Pane::Editor
        && matches!(app.editor.mode, Mode::Insert | Mode::Command | Mode::Search);

    if typing {
        if let Some(ek) = map_edit_key(k) {
            let action = app.editor.handle(ek);
            app.apply_editor_action(action);
        }
        return Ok(());
    }

    if app.tree.filtering {
        return handle_filter_key(app, k);
    }

    if app.leader != Leader::None {
        match (app.leader, k.code) {
            (Leader::Space, KeyCode::Char('p')) => app.toggle_preview(),
            (Leader::Space, KeyCode::Char('v')) => app.leader = Leader::SpaceV,
            (Leader::SpaceV, KeyCode::Char('v')) => app.open_picker(),
            (Leader::Space, KeyCode::Char('f')) => app.leader = Leader::SpaceF,
            (Leader::SpaceF, KeyCode::Char('f')) => app.open_finder(FinderKind::Files),
            (Leader::Space, KeyCode::Char('s')) => app.leader = Leader::SpaceS,
            (Leader::SpaceS, KeyCode::Char('g')) => app.open_finder(FinderKind::Content),
            (Leader::Space, KeyCode::Char('t')) => app.leader = Leader::SpaceT,
            (Leader::SpaceT, KeyCode::Char('g')) => app.open_finder(FinderKind::Tags),
            (Leader::Space, KeyCode::Char('n')) => app.leader = Leader::SpaceN,
            (Leader::SpaceN, KeyCode::Char('d')) => app.open_daily(),
            (Leader::Space, KeyCode::Char(' ')) => app.open_palette(),
            (Leader::Space, KeyCode::Char('a')) => app.leader = Leader::SpaceA,
            (Leader::SpaceA, KeyCode::Char('i')) => app.cycle_ai_card(),
            (Leader::Space, KeyCode::Char('z')) => app.toggle_focus(),
            (Leader::Space, KeyCode::Char('h')) => app.toggle_zen(),
            (Leader::Space, KeyCode::Char('e')) => app.toggle_explore(),
            (Leader::Space, KeyCode::Esc)
            | (Leader::SpaceV, KeyCode::Esc)
            | (Leader::SpaceN, KeyCode::Esc)
            | (Leader::SpaceA, KeyCode::Esc) => {
                app.leader = Leader::None;
            }
            _ => app.leader = Leader::None,
        }
        return Ok(());
    }

    if k.code == KeyCode::Char(' ')
        && !k.modifiers.contains(KeyModifiers::CONTROL)
        && !(app.ai.expanded() && !app.ai.draft.is_empty())
    {
        app.leader = Leader::Space;
        return Ok(());
    }

    if app.ai.expanded() {
        app.handle_ai_key(k);
        return Ok(());
    }

    match k.code {
        KeyCode::Tab => app.cycle_pane(),
        _ if app.pane == Pane::Tree => handle_tree_key(app, k),
        _ if app.pane == Pane::Editor => {
            if let Some(ek) = map_edit_key(k) {
                let action = app.editor.handle(ek);
                app.apply_editor_action(action);
            }
        }
        _ if app.pane == Pane::Preview => handle_preview_key(app, k),
        _ => {}
    }
    Ok(())
}

fn map_edit_key(k: KeyEvent) -> Option<EditKey> {
    if k.modifiers.contains(KeyModifiers::CONTROL) {
        return match k.code {
            KeyCode::Char(c) => Some(EditKey::Ctrl(c)),
            _ => None,
        };
    }
    Some(match k.code {
        KeyCode::Char(c) => EditKey::Char(c),
        KeyCode::Esc => EditKey::Esc,
        KeyCode::Enter => EditKey::Enter,
        KeyCode::Backspace => EditKey::Backspace,
        KeyCode::Tab => EditKey::Tab,
        KeyCode::Up => EditKey::Up,
        KeyCode::Down => EditKey::Down,
        KeyCode::Left => EditKey::Left,
        KeyCode::Right => EditKey::Right,
        KeyCode::Home => EditKey::Home,
        KeyCode::End => EditKey::End,
        KeyCode::PageUp => EditKey::PageUp,
        KeyCode::PageDown => EditKey::PageDown,
        _ => return None,
    })
}

fn handle_filter_key(app: &mut App, k: KeyEvent) -> Result<()> {
    match k.code {
        KeyCode::Esc => {
            app.tree.filter.clear();
            app.tree.filtering = false;
            app.tree.selected = 0;
        }
        KeyCode::Enter => {
            app.tree.filtering = false;
        }
        KeyCode::Backspace => {
            app.tree.filter.pop();
            app.tree.selected = 0;
        }
        KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) && !c.is_control() => {
            app.tree.filter.push(c);
            app.tree.selected = 0;
        }
        _ => {}
    }
    Ok(())
}

fn handle_preview_key(app: &mut App, k: KeyEvent) {
    match k.code {
        KeyCode::Down | KeyCode::Char('j') => {
            app.preview_scroll = app.preview_scroll.saturating_add(1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.preview_scroll = app.preview_scroll.saturating_sub(1);
        }
        KeyCode::PageDown => {
            app.preview_scroll = app.preview_scroll.saturating_add(10);
        }
        KeyCode::PageUp => {
            app.preview_scroll = app.preview_scroll.saturating_sub(10);
        }
        KeyCode::Home | KeyCode::Char('g') => app.preview_scroll = 0,
        KeyCode::End | KeyCode::Char('G') => app.preview_scroll = u16::MAX,
        _ => {}
    }
}

fn handle_tree_key(app: &mut App, k: KeyEvent) {
    match k.code {
        KeyCode::Down | KeyCode::Char('j') => app.tree.move_by(1),
        KeyCode::Up | KeyCode::Char('k') => app.tree.move_by(-1),
        KeyCode::PageDown => app.tree.move_by(10),
        KeyCode::PageUp => app.tree.move_by(-10),
        KeyCode::Home | KeyCode::Char('g') => app.tree.jump_home(),
        KeyCode::End | KeyCode::Char('G') => app.tree.jump_end(),
        KeyCode::Char('h') | KeyCode::Left => app.tree.collapse(),
        KeyCode::Char('l') | KeyCode::Right => app.tree.expand(),
        KeyCode::Enter => app.tree_activate(),
        KeyCode::Char('/') => {
            app.tree.filtering = true;
        }
        KeyCode::Char('a') => app.begin_new_entry(),
        KeyCode::Char('r') => app.begin_rename(),
        KeyCode::Char('d') => app.begin_delete(),
        _ => {}
    }
}

fn handle_mouse(app: &mut App, m: MouseEvent) -> Result<()> {
    match m.kind {
        MouseEventKind::Drag(MouseButton::Left) if app.split_drag.is_some() => {
            apply_split_drag(app, m.column, m.row);
        }
        MouseEventKind::Up(MouseButton::Left) if app.split_drag.is_some() => {
            app.split_drag = None;
        }
        MouseEventKind::Down(MouseButton::Left) if hit_tree_split(app, m.column, m.row) => {
            app.split_drag = Some(SplitDrag::Tree);
            apply_split_drag(app, m.column, m.row);
        }
        MouseEventKind::Down(MouseButton::Left) if hit_preview_split(app, m.column, m.row) => {
            app.split_drag = Some(SplitDrag::Preview);
            apply_split_drag(app, m.column, m.row);
        }
        MouseEventKind::Drag(MouseButton::Left) if app.mouse_drag.is_some() => {
            app.mouse_editor_drag(m.column, m.row);
        }
        MouseEventKind::Up(MouseButton::Left) if app.mouse_drag.is_some() => {
            app.mouse_editor_up();
        }
        MouseEventKind::Down(MouseButton::Left) if contains(app.ai.area, m.column, m.row) => {
            if !app.ai.expanded() {
                app.cycle_ai_card();
            }
        }
        MouseEventKind::ScrollUp if contains(app.tree_area, m.column, m.row) => {
            app.pane = Pane::Tree;
            app.tree.scroll = app.tree.scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollDown if contains(app.tree_area, m.column, m.row) => {
            app.pane = Pane::Tree;
            let visible = app.tree_inner.height as usize;
            let max = app.tree.visible().len().saturating_sub(visible.max(1));
            app.tree.scroll = (app.tree.scroll + 3).min(max);
        }
        MouseEventKind::Down(MouseButton::Left) if contains(app.tree_area, m.column, m.row) => {
            app.pane = Pane::Tree;
            if contains(app.tree_inner, m.column, m.row) {
                let idx = app.tree.scroll + (m.row.saturating_sub(app.tree_inner.y)) as usize;
                let rows = app.tree.visible();
                if idx < rows.len() {
                    let row = rows[idx].clone();
                    app.tree.selected = idx;
                    let glyph = m.column < app.tree_inner.x.saturating_add(row.glyph_cols.max(2));
                    if glyph && row.kind == dd_vault_core::NodeKind::Dir {
                        app.tree.toggle_dir();
                    }
                    let now = Instant::now();
                    if let Some((px, py, t0)) = app.last_click {
                        if px == m.column
                            && py == m.row
                            && now.duration_since(t0).as_millis() < DOUBLE_CLICK_MS
                        {
                            app.tree_activate();
                        }
                    }
                    app.last_click = Some((m.column, m.row, now));
                }
            }
        }
        MouseEventKind::ScrollUp if contains(app.preview_area, m.column, m.row) => {
            app.pane = Pane::Preview;
            app.preview_scroll = app.preview_scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollDown if contains(app.preview_area, m.column, m.row) => {
            app.pane = Pane::Preview;
            app.preview_scroll = app.preview_scroll.saturating_add(3);
        }
        MouseEventKind::Down(MouseButton::Left) if contains(app.preview_area, m.column, m.row) => {
            app.pane = Pane::Preview;
        }
        MouseEventKind::ScrollUp if contains(app.editor_area, m.column, m.row) => {
            app.pane = Pane::Editor;
            app.editor.scroll = app.editor.scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollDown if contains(app.editor_area, m.column, m.row) => {
            app.pane = Pane::Editor;
            app.editor.scroll = app.editor.scroll.saturating_add(3);
        }
        MouseEventKind::Down(MouseButton::Left) if contains(app.editor_inner, m.column, m.row) => {
            app.pane = Pane::Editor;
            app.mouse_editor_down(m.column, m.row);
        }
        MouseEventKind::Down(MouseButton::Left) if contains(app.editor_area, m.column, m.row) => {
            app.pane = Pane::Editor;
        }
        _ => {}
    }
    Ok(())
}

impl App {
    fn editor_hit(&self, x: u16, y: u16) -> Option<(usize, usize, bool)> {
        if self.editor.rel.is_none() || self.editor_inner.height == 0 {
            return None;
        }
        let inner = self.editor_inner;
        let gutter = self.editor.gutter_cols();
        let row = (y.saturating_sub(inner.y) as usize).min(
            (inner.height as usize)
                .saturating_sub(1)
                .min(self.editor.line_count().saturating_sub(1)),
        );
        let line = self.editor.scroll + row;
        let line = line.min(self.editor.line_count().saturating_sub(1));
        let dx = x.saturating_sub(inner.x);
        let in_gutter = dx < gutter;
        let col = if in_gutter { 0 } else { (dx - gutter) as usize };
        Some((line, col, in_gutter))
    }

    pub fn mouse_editor_down(&mut self, x: u16, y: u16) {
        let Some((line, col, in_gutter)) = self.editor_hit(x, y) else {
            return;
        };
        if in_gutter {
            self.editor.begin_mouse_select(line, 0, true);
            self.mouse_drag = Some(MouseDrag {
                start_line: line,
                start_col: 0,
                linewise: true,
                moved: true,
            });
        } else {
            self.editor.click_to(line, col);
            self.mouse_drag = Some(MouseDrag {
                start_line: line,
                start_col: col,
                linewise: false,
                moved: false,
            });
        }
    }

    pub fn mouse_editor_drag(&mut self, x: u16, y: u16) {
        let Some(mut drag) = self.mouse_drag else {
            return;
        };
        let Some((line, col, _)) = self.editor_hit_clamped(x, y) else {
            return;
        };
        if !drag.linewise && !drag.moved && (line != drag.start_line || col != drag.start_col) {
            self.editor
                .begin_mouse_select(drag.start_line, drag.start_col, false);
            drag.moved = true;
        }
        if drag.moved || drag.linewise {
            self.editor.update_mouse_select(line, col);
        }
        self.mouse_drag = Some(drag);
    }

    pub fn mouse_editor_up(&mut self) {
        if self.mouse_drag.take().is_some() {
            self.editor.finish_mouse_select();
        }
    }

    fn editor_hit_clamped(&self, x: u16, y: u16) -> Option<(usize, usize, bool)> {
        if self.editor.rel.is_none() || self.editor_inner.height == 0 {
            return None;
        }
        let inner = self.editor_inner;
        let y = y.clamp(
            inner.y,
            inner.y.saturating_add(inner.height.saturating_sub(1)),
        );
        let x = x.clamp(
            inner.x,
            inner.x.saturating_add(inner.width.saturating_sub(1)),
        );
        self.editor_hit(x, y)
    }
}

fn dispatch_theme_editor(app: &mut App, ek: ldnddev_theme::EditorKey, shift: bool) {
    let outcome = {
        let Some(editor) = app.theme_editor.as_mut() else {
            return;
        };
        editor.handle(ek, shift)
    };
    match outcome {
        ldnddev_theme::EditorOutcome::PaletteChanged => {
            if let Some(editor) = &app.theme_editor {
                apply_palette(&mut app.theme, &editor.palette);
            }
        }
        ldnddev_theme::EditorOutcome::RequestSave => {
            if let Some(editor) = &app.theme_editor {
                let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                match ldnddev_theme::save_theme(
                    &editor.palette,
                    &root,
                    THEME_FILENAME,
                    editor.save_target,
                    ldnddev_theme::default_config_home().as_deref(),
                    &editor.fields,
                ) {
                    Ok(path) => {
                        apply_palette(&mut app.theme, &editor.palette);
                        app.theme.source = match editor.save_target {
                            ldnddev_theme::ThemeSaveTarget::Local => {
                                ldnddev_theme::ThemeSource::Local
                            }
                            ldnddev_theme::ThemeSaveTarget::Global => {
                                ldnddev_theme::ThemeSource::Global
                            }
                        };
                        app.theme_editor = None;
                        app.show_theme = false;
                        app.push_toast(
                            ToastLevel::Success,
                            format!("Theme saved ({})", path.display()),
                        );
                    }
                    Err(err) => {
                        app.push_toast(ToastLevel::Error, format!("Theme save failed: {err}"));
                    }
                }
            }
        }
        ldnddev_theme::EditorOutcome::Closed { .. } => close_theme(app, true),
        ldnddev_theme::EditorOutcome::HexError(msg) => {
            app.push_toast(ToastLevel::Error, msg);
        }
        ldnddev_theme::EditorOutcome::None => {}
    }
}

fn close_theme(app: &mut App, revert: bool) {
    if let Some(mut editor) = app.theme_editor.take() {
        if revert {
            editor.revert();
            apply_palette(&mut app.theme, &editor.palette);
        }
    }
    app.show_theme = false;
}

fn map_editor_key(k: KeyEvent) -> Option<ldnddev_theme::EditorKey> {
    Some(match k.code {
        KeyCode::Up => ldnddev_theme::EditorKey::Up,
        KeyCode::Down => ldnddev_theme::EditorKey::Down,
        KeyCode::Left => ldnddev_theme::EditorKey::Left,
        KeyCode::Right => ldnddev_theme::EditorKey::Right,
        KeyCode::Tab => ldnddev_theme::EditorKey::Tab,
        KeyCode::Enter => ldnddev_theme::EditorKey::Enter,
        KeyCode::Esc => ldnddev_theme::EditorKey::Esc,
        KeyCode::Backspace => ldnddev_theme::EditorKey::Backspace,
        KeyCode::Char(c) => ldnddev_theme::EditorKey::Char(c),
        _ => return None,
    })
}

fn hit_tree_split(app: &App, x: u16, y: u16) -> bool {
    if app.tree_area.width == 0 {
        return false;
    }
    let edge = app.tree_area.x + app.tree_area.width.saturating_sub(1);
    y >= app.tree_area.y
        && y < app.tree_area.y + app.tree_area.height
        && (x == edge || x == edge.saturating_add(1))
}

fn hit_preview_split(app: &App, x: u16, y: u16) -> bool {
    if app.preview_area.height == 0 || app.editor_area.height == 0 {
        return false;
    }
    let edge = app.editor_area.y + app.editor_area.height.saturating_sub(1);
    let x0 = app.editor_area.x.min(app.preview_area.x);
    let x1 = (app.editor_area.x + app.editor_area.width)
        .max(app.preview_area.x + app.preview_area.width);
    x >= x0 && x < x1 && (y == edge || y == app.preview_area.y)
}

fn apply_split_drag(app: &mut App, x: u16, y: u16) {
    match app.split_drag {
        Some(SplitDrag::Tree) => {
            let origin = app.tree_area.x;
            let max = app.body_width.saturating_sub(20).max(12);
            app.tree_split = x.saturating_sub(origin).clamp(12, max);
        }
        Some(SplitDrag::Preview) => {
            let bottom = app.preview_area.y + app.preview_area.height;
            let max = bottom
                .saturating_sub(app.editor_area.y.saturating_add(3))
                .max(4);
            app.preview_split = bottom.saturating_sub(y).clamp(4, max);
        }
        None => {}
    }
}

fn contains(area: ratatui::layout::Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.x + area.width && y >= area.y && y < area.y + area.height
}
