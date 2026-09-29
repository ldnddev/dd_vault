//! Wikilink follow, jump list, and session restore.

use std::path::{Path, PathBuf};

use dd_vault_core::{link_at, wikilink_new_rel, Session};

use crate::app::{App, ConfirmKind, DiscardNext, JumpAction, Modal, NavigateTo, Pane};
use crate::jump::Jump;
use crate::toasts::ToastLevel;
use crate::tree::TreeState;

impl App {
    pub fn current_jump(&self) -> Option<Jump> {
        Some(Jump {
            rel: PathBuf::from(self.editor.rel.as_ref()?),
            cursor: self.editor.cursor(),
            scroll: self.editor.scroll,
            scroll_off: self.editor.scroll_off,
        })
    }

    pub fn navigate(&mut self, to: NavigateTo) {
        if self.editor.rel.as_deref() == Some(to.rel.to_string_lossy().as_ref()) {
            self.commit_jump(&to, self.current_jump());
            self.apply_navigate_view(&to);
            self.tree.reveal(&to.rel);
            self.save_session();
            return;
        }
        if self.editor.dirty {
            self.modal = Some(Modal::Confirm {
                kind: ConfirmKind::Discard {
                    next: DiscardNext::Navigate(to.clone()),
                },
                message: format!("Discard unsaved changes and open {}?", to.rel.display()),
            });
            return;
        }
        self.navigate_clean(to);
    }

    pub(crate) fn navigate_clean(&mut self, to: NavigateTo) {
        let here = self.current_jump();
        self.load_note(to.rel.clone());
        if self.editor.rel.as_deref() != Some(to.rel.to_string_lossy().as_ref()) {
            return;
        }
        self.commit_jump(&to, here);
        self.apply_navigate_view(&to);
        self.tree.reveal(&to.rel);
        self.save_session();
    }

    fn commit_jump(&mut self, to: &NavigateTo, here: Option<Jump>) {
        match to.jump {
            JumpAction::Record => {
                if let Some(here) = here {
                    self.jumps.push(here);
                }
            }
            JumpAction::Back => {
                let here = here.unwrap_or_else(|| Jump {
                    rel: to.rel.clone(),
                    cursor: to.cursor.unwrap_or(0),
                    scroll: to.scroll.unwrap_or(0),
                    scroll_off: to.scroll_off.unwrap_or(0),
                });
                let _ = self.jumps.back(here);
            }
            JumpAction::Forward => {
                let here = here.unwrap_or_else(|| Jump {
                    rel: to.rel.clone(),
                    cursor: to.cursor.unwrap_or(0),
                    scroll: to.scroll.unwrap_or(0),
                    scroll_off: to.scroll_off.unwrap_or(0),
                });
                let _ = self.jumps.forward(here);
            }
        }
    }

    fn apply_navigate_view(&mut self, to: &NavigateTo) {
        if let (Some(cursor), Some(scroll), Some(off)) = (to.cursor, to.scroll, to.scroll_off) {
            self.editor.restore_view(cursor, scroll, off);
        } else if let Some(heading) = &to.heading {
            jump_to_heading(&mut self.editor, heading);
        }
        self.pane = Pane::Editor;
    }

    pub fn follow_link(&mut self, from_gf: bool) {
        let Some(src) = self.editor.rel.as_ref().map(|_| self.editor.text()) else {
            if from_gf {
                self.push_toast(ToastLevel::Info, "Open a note first");
            }
            return;
        };
        let Some(link) = link_at(&src, self.editor.cursor()) else {
            if from_gf {
                self.push_toast(ToastLevel::Info, "No link under the caret");
            }
            return;
        };
        self.follow_target(&link.dst_raw, link.dst_heading.as_deref());
    }

    pub fn follow_target(&mut self, raw: &str, heading: Option<&str>) {
        let dst = raw
            .split('#')
            .next()
            .unwrap_or(raw)
            .split('|')
            .next()
            .unwrap_or(raw)
            .trim();
        if dst.is_empty() {
            return;
        }
        if dd_render::looks_like_image(dst) {
            self.push_toast(ToastLevel::Info, "Image links stay in the preview");
            return;
        }
        if let Some(rel) = self.resolve_link_path(dst) {
            self.navigate(NavigateTo {
                rel,
                heading: heading.map(str::to_string),
                cursor: None,
                scroll: None,
                scroll_off: None,
                jump: JumpAction::Record,
            });
            return;
        }
        let current = self.editor.rel.as_ref().map(PathBuf::from);
        match wikilink_new_rel(current.as_deref(), dst) {
            Ok(rel) => {
                self.modal = Some(Modal::Confirm {
                    kind: ConfirmKind::CreateNote {
                        rel: rel.clone(),
                        heading: heading.map(str::to_string),
                    },
                    message: format!("Create {} and open it?", rel.display()),
                });
            }
            Err(err) => self.push_toast(ToastLevel::Error, err.to_string()),
        }
    }

    fn resolve_link_path(&self, dst: &str) -> Option<PathBuf> {
        if let Some(idx) = &self.index {
            if let Ok(Some(path)) = idx.resolve_wikilink(dst) {
                return Some(PathBuf::from(path));
            }
        }
        self.tree.find_file(dst)
    }

    pub fn create_and_open_note(&mut self, rel: PathBuf, heading: Option<String>) {
        let Some(vault) = self.vault.clone() else {
            return;
        };
        self.modal = None;
        if !vault.root.join(&rel).exists() {
            match dd_vault_core::create_file(&vault.root, &rel) {
                Ok(abs) => {
                    let title = rel
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("Untitled");
                    if let Err(err) = std::fs::write(&abs, format!("# {title}\n")) {
                        self.push_toast(ToastLevel::Error, err.to_string());
                        return;
                    }
                    self.reload_tree();
                    self.refresh_git();
                    self.queue_reindex();
                }
                Err(err) => {
                    self.push_toast(ToastLevel::Error, err.to_string());
                    return;
                }
            }
        }
        self.navigate(NavigateTo {
            rel,
            heading,
            cursor: None,
            scroll: None,
            scroll_off: None,
            jump: JumpAction::Record,
        });
    }

    pub fn jump_back(&mut self) {
        let Some(dest) = self.jumps.peek_back().cloned() else {
            self.push_toast(ToastLevel::Info, "Jump list is empty");
            return;
        };
        self.navigate(NavigateTo::from_jump(dest, JumpAction::Back));
    }

    pub fn jump_forward(&mut self) {
        let Some(dest) = self.jumps.peek_forward().cloned() else {
            self.push_toast(ToastLevel::Info, "Jump list is empty");
            return;
        };
        self.navigate(NavigateTo::from_jump(dest, JumpAction::Forward));
    }

    pub fn preview_follow_at(&mut self, x: u16, y: u16) -> bool {
        let inner = self.preview_inner;
        if x < inner.x || y < inner.y || x >= inner.x + inner.width || y >= inner.y + inner.height {
            return false;
        }
        let Some(cache) = &self.preview_cache else {
            return false;
        };
        let width = inner.width.max(1) as usize;
        let visual = (y - inner.y) as usize + self.preview_scroll as usize;
        let col = (x - inner.x) as usize;
        let Some((line, col0)) = visual_preview_line(&cache.text, width, visual) else {
            return false;
        };
        let Some(link) = link_at(&line, col0.saturating_add(col)) else {
            return false;
        };
        self.follow_target(&link.dst_raw, link.dst_heading.as_deref());
        true
    }

    pub fn save_session(&mut self) {
        let Some(vault) = &self.vault else {
            return;
        };
        let pane = match self.pane {
            Pane::Tree => "tree",
            Pane::Editor => "editor",
            Pane::Preview => "preview",
        };
        let session = Session {
            note: self.editor.rel.clone(),
            cursor: self.editor.cursor(),
            scroll: self.editor.scroll,
            scroll_off: self.editor.scroll_off,
            wrap: Some(self.wrap_notes),
            preview: Some(self.preview_visible),
            preview_scroll: self.preview_scroll,
            preview_split: self.preview_split,
            tree_split: self.tree_split,
            zen: self.zen,
            focus: self.focus,
            explore: self.explore,
            pane: Some(pane.into()),
        };
        let _ = session.save(&vault.meta_dir);
    }

    pub fn restore_session(&mut self) {
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let Some(session) = Session::load(&vault.meta_dir) else {
            return;
        };
        if let Some(w) = session.wrap {
            self.wrap_notes = w;
        }
        if let Some(p) = session.preview {
            self.preview_visible = p;
        }
        self.preview_scroll = session.preview_scroll;
        self.preview_split = session.preview_split;
        self.tree_split = session.tree_split;
        self.zen = session.zen;
        self.focus = session.focus;
        self.explore = session.explore;
        if let Some(rel) = session.note {
            let path = vault.root.join(&rel);
            if path.is_file() {
                self.load_note(PathBuf::from(&rel));
                self.editor
                    .restore_view(session.cursor, session.scroll, session.scroll_off);
                self.tree.reveal(Path::new(&rel));
            }
        }
        self.pane = match session.pane.as_deref() {
            Some("tree") if self.tree_layout_visible() => Pane::Tree,
            Some("preview") if self.preview_layout_visible() => Pane::Preview,
            _ => {
                if self.editor.rel.is_some() {
                    Pane::Editor
                } else {
                    Pane::Tree
                }
            }
        };
        if self.zen {
            self.pane = Pane::Editor;
        }
    }
}

fn jump_to_heading(editor: &mut dd_edit::Editor, heading: &str) {
    let want = heading.trim();
    for (i, line) in editor.text().lines().enumerate() {
        let t = line.trim();
        let hashes = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) {
            let text = t[hashes..].trim();
            if text.eq_ignore_ascii_case(want) {
                editor.go_line(i + 1);
                return;
            }
        }
    }
}

fn visual_preview_line(
    text: &ratatui::text::Text<'_>,
    width: usize,
    visual_row: usize,
) -> Option<(String, usize)> {
    let w = width.max(1);
    let mut vis = 0usize;
    for line in &text.lines {
        let s: String = line.spans.iter().map(|sp| sp.content.as_ref()).collect();
        let n = s.chars().count();
        let rows = n.div_ceil(w).max(1);
        if visual_row < vis + rows {
            let row_in = visual_row - vis;
            return Some((s, row_in * w));
        }
        vis += rows;
    }
    None
}

impl TreeState {
    pub fn reveal(&mut self, rel: &Path) {
        let mut p = rel.parent();
        while let Some(dir) = p {
            if dir.as_os_str().is_empty() {
                break;
            }
            self.collapsed.remove(dir);
            p = dir.parent();
        }
        self.select_rel(rel);
    }
}
