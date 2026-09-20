use std::collections::HashSet;
use std::path::{Path, PathBuf};

use dd_vault_core::{walk_tree, FsNode, NodeKind, Vault};

#[derive(Clone, Debug)]
pub struct VisibleRow {
    pub rel: PathBuf,
    pub name: String,
    pub kind: NodeKind,
    pub prefix: String,
    pub glyph_cols: u16,
}

#[derive(Clone, Debug, Default)]
pub struct TreeState {
    pub roots: Vec<FsNode>,
    pub collapsed: HashSet<PathBuf>,
    pub selected: usize,
    pub scroll: usize,
    pub filter: String,
    pub filtering: bool,
}

impl TreeState {
    pub fn load(vault: &Vault) -> Self {
        let roots = walk_tree(&vault.root).unwrap_or_default();
        Self {
            roots,
            collapsed: HashSet::new(),
            selected: 0,
            scroll: 0,
            filter: String::new(),
            filtering: false,
        }
    }

    pub fn reload(&mut self, vault: &Vault) {
        let keep = self.selected_rel();
        self.roots = walk_tree(&vault.root).unwrap_or_default();
        self.collapsed.retain(|p| vault.root.join(p).exists());
        if let Some(rel) = keep {
            self.select_rel(&rel);
        } else {
            self.selected = self.selected.min(self.visible().len().saturating_sub(1));
        }
    }

    pub fn visible(&self) -> Vec<VisibleRow> {
        let mut out = Vec::new();
        let filter = if self.filter.is_empty() {
            None
        } else {
            Some(self.filter.as_str())
        };
        let filtering = filter.is_some();
        flatten(
            &self.roots,
            &self.collapsed,
            filter,
            filtering,
            &[],
            0,
            &mut out,
        );
        out
    }

    pub fn find_file(&self, name: &str) -> Option<PathBuf> {
        fn walk(nodes: &[FsNode], want: &str) -> Option<PathBuf> {
            let want_stem = want.trim_end_matches(".md");
            for n in nodes {
                if n.kind != NodeKind::Dir {
                    let stem = n.name.trim_end_matches(".md");
                    if n.name == want || stem.eq_ignore_ascii_case(want_stem) {
                        return Some(n.rel.clone());
                    }
                }
                if let Some(found) = walk(&n.children, want) {
                    return Some(found);
                }
            }
            None
        }
        walk(&self.roots, name)
    }

    pub fn selected_rel(&self) -> Option<PathBuf> {
        self.visible().get(self.selected).map(|r| r.rel.clone())
    }

    pub fn selected_row(&self) -> Option<VisibleRow> {
        self.visible().get(self.selected).cloned()
    }

    pub fn select_rel(&mut self, rel: &Path) {
        let rows = self.visible();
        if let Some(i) = rows.iter().position(|r| r.rel == rel) {
            self.selected = i;
        } else {
            self.selected = self.selected.min(rows.len().saturating_sub(1));
        }
    }

    pub fn move_by(&mut self, delta: i32) {
        let n = self.visible().len();
        if n == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as i32 + delta;
        self.selected = next.clamp(0, n as i32 - 1) as usize;
    }

    pub fn jump_home(&mut self) {
        self.selected = 0;
    }

    pub fn jump_end(&mut self) {
        let n = self.visible().len();
        self.selected = n.saturating_sub(1);
    }

    pub fn ensure_visible(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        if self.selected >= self.scroll + height {
            self.scroll = self.selected + 1 - height;
        }
        let max_scroll = self.visible().len().saturating_sub(height);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    pub fn toggle_dir(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if row.kind != NodeKind::Dir {
            return;
        }
        if !self.collapsed.remove(&row.rel) {
            self.collapsed.insert(row.rel);
        }
    }

    pub fn collapse(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if row.kind == NodeKind::Dir && !self.collapsed.contains(&row.rel) {
            self.collapsed.insert(row.rel);
            return;
        }
        if let Some(parent) = row.rel.parent() {
            if !parent.as_os_str().is_empty() {
                self.collapsed.insert(parent.to_path_buf());
                self.select_rel(parent);
            }
        }
    }

    pub fn expand(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if row.kind == NodeKind::Dir {
            self.collapsed.remove(&row.rel);
        }
    }

    pub fn parent_rel_for_new(&self) -> PathBuf {
        match self.selected_row() {
            Some(row) if row.kind == NodeKind::Dir => row.rel,
            Some(row) => row
                .rel
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map(Path::to_path_buf)
                .unwrap_or_default(),
            None => PathBuf::new(),
        }
    }
}

fn flatten(
    nodes: &[FsNode],
    collapsed: &HashSet<PathBuf>,
    filter: Option<&str>,
    filtering: bool,
    ancestors_last: &[bool],
    depth: usize,
    out: &mut Vec<VisibleRow>,
) {
    let visible: Vec<&FsNode> = nodes
        .iter()
        .filter(|n| filter.map(|q| n.matches_filter(q)).unwrap_or(true))
        .collect();
    let n = visible.len();
    for (i, node) in visible.iter().enumerate() {
        let is_last = i + 1 == n;
        let expanded = node.is_dir() && (filtering || !collapsed.contains(&node.rel));
        let prefix = build_prefix(ancestors_last, is_last, depth, node.is_dir(), expanded);
        let glyph_cols = prefix.chars().count() as u16;
        let mut name = node.name.clone();
        if node.is_dir() && !name.ends_with('/') {
            name.push('/');
        }
        out.push(VisibleRow {
            rel: node.rel.clone(),
            name,
            kind: node.kind,
            prefix,
            glyph_cols,
        });
        if node.is_dir() && expanded {
            let mut next = ancestors_last.to_vec();
            next.push(is_last);
            flatten(
                &node.children,
                collapsed,
                filter,
                filtering,
                &next,
                depth + 1,
                out,
            );
        }
    }
}

fn build_prefix(
    ancestors_last: &[bool],
    is_last: bool,
    depth: usize,
    is_dir: bool,
    expanded: bool,
) -> String {
    let mut s = String::new();
    if depth == 0 {
        if is_dir {
            s.push_str(if expanded { "▾ " } else { "▸ " });
        } else {
            s.push_str("  ");
        }
        return s;
    }
    for (i, last) in ancestors_last.iter().enumerate() {
        if i == 0 {
            s.push_str("  ");
        } else {
            s.push_str(if *last { "   " } else { "│  " });
        }
    }
    s.push_str(if is_last { "└─ " } else { "├─ " });
    if is_dir {
        s.push_str(if expanded { "▾ " } else { "▸ " });
    }
    s
}

use crate::app::{App, ConfirmKind, Modal, PromptKind};
use crate::toasts::ToastLevel;
use dd_vault_core::{
    create_dir, create_file, delete_entry, rename_entry, validate_entry_name, NodeKind as CoreKind,
};

impl App {
    pub fn reload_tree(&mut self) {
        if let Some(vault) = &self.vault {
            self.tree.reload(vault);
        }
    }

    pub fn tree_activate(&mut self) {
        let Some(row) = self.tree.selected_row() else {
            return;
        };
        match row.kind {
            CoreKind::Dir => self.tree.toggle_dir(),
            CoreKind::File | CoreKind::Symlink => {
                self.open_file(row.rel);
            }
        }
    }

    pub fn begin_new_entry(&mut self) {
        if self.vault.is_none() {
            self.push_toast(ToastLevel::Info, "Open a vault first");
            return;
        }
        let parent = self.tree.parent_rel_for_new();
        self.modal = Some(Modal::Prompt {
            kind: PromptKind::New { parent },
            draft: String::new(),
        });
    }

    pub fn begin_rename(&mut self) {
        let Some(row) = self.tree.selected_row() else {
            return;
        };
        self.modal = Some(Modal::Prompt {
            kind: PromptKind::Rename {
                rel: row.rel.clone(),
            },
            draft: row
                .rel
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
        });
    }

    pub fn begin_delete(&mut self) {
        let Some(row) = self.tree.selected_row() else {
            return;
        };
        let kind = if row.kind == CoreKind::Dir {
            "folder"
        } else {
            "file"
        };
        let message = format!("Delete {kind} {}?", row.rel.display());
        self.modal = Some(Modal::Confirm {
            kind: ConfirmKind::Delete { rel: row.rel },
            message,
        });
    }

    pub fn submit_prompt(&mut self) {
        let Some(Modal::Prompt { kind, draft }) = self.modal.clone() else {
            return;
        };
        let Some(vault) = self.vault.clone() else {
            self.modal = None;
            return;
        };
        match kind {
            PromptKind::New { parent } => match parse_new_name(&draft) {
                Ok((name, is_dir)) => {
                    let rel = if parent.as_os_str().is_empty() {
                        PathBuf::from(&name)
                    } else {
                        parent.join(&name)
                    };
                    let result = if is_dir {
                        create_dir(&vault.root, &rel)
                    } else {
                        create_file(&vault.root, &rel)
                    };
                    match result {
                        Ok(_) => {
                            self.modal = None;
                            self.reload_tree();
                            self.tree.collapsed.remove(&parent);
                            self.tree.select_rel(&rel);
                            if !is_dir {
                                self.load_note(rel.clone());
                            }
                            self.refresh_git();
                            self.push_toast(
                                ToastLevel::Success,
                                format!("Created {}", rel.display()),
                            );
                        }
                        Err(err) => self.push_toast(ToastLevel::Error, err.to_string()),
                    }
                }
                Err(err) => self.push_toast(ToastLevel::Error, err),
            },
            PromptKind::GitCommit => {
                self.submit_git_commit(draft);
            }
            PromptKind::Rename { rel } => {
                let name = draft.trim();
                if let Err(err) = validate_entry_name(name) {
                    self.push_toast(ToastLevel::Error, err.to_string());
                    return;
                }
                match rename_entry(&vault.root, &rel, name) {
                    Ok(dest) => {
                        let new_rel = dest
                            .strip_prefix(&vault.root)
                            .map(Path::to_path_buf)
                            .unwrap_or(dest);
                        let old = rel.to_string_lossy();
                        if self.editor.rel.as_deref() == Some(old.as_ref()) {
                            let abs = vault.root.join(&new_rel);
                            self.editor.path = Some(abs);
                            self.editor.rel = Some(new_rel.to_string_lossy().into_owned());
                        }
                        self.modal = None;
                        self.reload_tree();
                        self.tree.select_rel(&new_rel);
                        self.refresh_git();
                        self.push_toast(
                            ToastLevel::Success,
                            format!("Renamed to {}", new_rel.display()),
                        );
                    }
                    Err(err) => self.push_toast(ToastLevel::Error, err.to_string()),
                }
            }
        }
    }

    pub fn confirm_action(&mut self) {
        let Some(Modal::Confirm { kind, .. }) = self.modal.clone() else {
            return;
        };
        if matches!(kind, ConfirmKind::AiSend) {
            self.modal = None;
            self.start_pending_ai();
            return;
        }
        let Some(vault) = self.vault.clone() else {
            self.modal = None;
            return;
        };
        match kind {
            ConfirmKind::Delete { rel } => match delete_entry(&vault.root, &rel) {
                Ok(()) => {
                    if let Some(open) = &self.editor.rel {
                        let open_path = PathBuf::from(open);
                        if open_path == rel || open_path.starts_with(&rel) {
                            self.editor = dd_edit::Editor::empty();
                        }
                    }
                    self.modal = None;
                    self.reload_tree();
                    self.refresh_git();
                    self.push_toast(ToastLevel::Success, format!("Deleted {}", rel.display()));
                }
                Err(err) => self.push_toast(ToastLevel::Error, err.to_string()),
            },
            ConfirmKind::Discard { next } => {
                self.modal = None;
                self.editor.dirty = false;
                match next {
                    crate::app::DiscardNext::Quit => self.should_quit = true,
                    crate::app::DiscardNext::Open(rel) => self.load_note(rel),
                }
            }
            ConfirmKind::ReloadDisk { rel } => {
                self.modal = None;
                self.suppress_disk_prompt = None;
                let abs = vault.root.join(&rel);
                if !abs.is_file() {
                    self.editor = dd_edit::Editor::empty();
                    self.push_toast(
                        ToastLevel::Warning,
                        format!("{} is gone from disk", rel.display()),
                    );
                } else if self.editor.rel.as_deref() == Some(rel.to_string_lossy().as_ref()) {
                    match self.editor.reload_from_disk() {
                        Ok(()) => {
                            self.push_toast(ToastLevel::Info, format!("Reloaded {}", rel.display()))
                        }
                        Err(err) => self.push_toast(ToastLevel::Error, err.to_string()),
                    }
                } else {
                    self.load_note(rel);
                }
            }
            ConfirmKind::AiSend => {
                self.modal = None;
                self.start_pending_ai();
            }
        }
    }
}

fn parse_new_name(raw: &str) -> Result<(String, bool), String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Name is empty".into());
    }
    let is_dir = trimmed.ends_with('/');
    let name = trimmed.trim_end_matches('/').trim();
    validate_entry_name(name).map_err(|e| e.to_string())?;
    if is_dir {
        Ok((name.to_string(), true))
    } else if name.contains('.') {
        Ok((name.to_string(), false))
    } else {
        Ok((format!("{name}.md"), false))
    }
}
