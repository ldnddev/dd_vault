use std::path::{Path, PathBuf};

use dd_vault_core::{walk_tree, FsNode, NodeKind, Vault};

#[derive(Clone, Debug)]
pub struct VisibleRow {
    pub rel: PathBuf,
    pub name: String,
    pub kind: NodeKind,
    pub prefix: String,
}

#[derive(Clone, Debug, Default)]
pub struct TreeState {
    pub roots: Vec<FsNode>,
    /// Folder whose children fill the pane. Empty is the vault root.
    pub cwd: PathBuf,
    pub selected: usize,
    pub scroll: usize,
    pub filter: String,
    pub filtering: bool,
    /// Selection to restore when a vault-wide filter is cancelled.
    filter_anchor: Option<PathBuf>,
}

impl TreeState {
    pub fn load(vault: &Vault) -> Self {
        Self {
            roots: walk_tree(&vault.root).unwrap_or_default(),
            cwd: PathBuf::new(),
            selected: 0,
            scroll: 0,
            filter: String::new(),
            filtering: false,
            filter_anchor: None,
        }
    }

    pub fn reload(&mut self, vault: &Vault) {
        let keep = self.selected_rel();
        let cwd = self.cwd.clone();
        self.roots = walk_tree(&vault.root).unwrap_or_default();
        self.cwd = self.resolve_cwd(&cwd);
        if let Some(rel) = keep {
            self.select_rel(&rel);
        } else {
            self.selected = self.selected.min(self.visible().len().saturating_sub(1));
        }
    }

    pub fn visible(&self) -> Vec<VisibleRow> {
        if !self.filter.is_empty() {
            let mut out = Vec::new();
            collect_matches(&self.roots, &self.filter, &mut out);
            out.sort_by(|a, b| {
                let dir_a = a.kind == NodeKind::Dir;
                let dir_b = b.kind == NodeKind::Dir;
                dir_b.cmp(&dir_a).then_with(|| {
                    a.name
                        .to_lowercase()
                        .cmp(&b.name.to_lowercase())
                        .then_with(|| a.rel.cmp(&b.rel))
                })
            });
            return out;
        }
        let mut out = Vec::new();
        if !self.cwd.as_os_str().is_empty() {
            let parent = self
                .cwd
                .parent()
                .map(Path::to_path_buf)
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_default();
            out.push(VisibleRow {
                rel: parent,
                name: "..".to_string(),
                kind: NodeKind::Dir,
                prefix: "  ".to_string(),
            });
        }
        for node in children_of(&self.roots, &self.cwd) {
            out.push(row_for(node, false));
        }
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

    /// Open the selected folder as the pane's listing.
    pub fn enter(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        if row.name == ".." || row.kind != NodeKind::Dir {
            return;
        }
        let dest = row.rel.clone();
        self.dismiss_filter();
        self.cwd = dest;
        self.selected = 0;
        self.scroll = 0;
    }

    /// Return to the parent folder and select the one we left.
    /// While a filter is showing, this cancels the filter instead.
    pub fn leave(&mut self) {
        if self.filtering || !self.filter.is_empty() {
            self.cancel_filter();
            return;
        }
        if self.cwd.as_os_str().is_empty() {
            return;
        }
        let current = self.cwd.clone();
        self.cwd = current
            .parent()
            .map(Path::to_path_buf)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_default();
        self.scroll = 0;
        self.selected = 0;
        self.select_rel(&current);
    }

    /// Show `rel` in its parent folder. Used when a note is opened.
    pub fn reveal(&mut self, rel: &Path) {
        let parent = rel
            .parent()
            .map(Path::to_path_buf)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_default();
        self.cwd = self.resolve_cwd(&parent);
        self.scroll = 0;
        self.select_rel(rel);
    }

    pub fn begin_filter(&mut self) {
        if self.filter.is_empty() {
            self.filter_anchor = self.selected_rel();
        }
        self.filtering = true;
    }

    pub fn cancel_filter(&mut self) {
        let anchor = self.filter_anchor.take();
        self.filter.clear();
        self.filtering = false;
        self.scroll = 0;
        if let Some(rel) = anchor {
            self.select_rel(&rel);
        } else {
            self.selected = 0;
        }
    }

    pub fn dismiss_filter(&mut self) {
        self.filter.clear();
        self.filtering = false;
        self.filter_anchor = None;
    }

    pub fn parent_rel_for_new(&self) -> PathBuf {
        self.cwd.clone()
    }

    fn resolve_cwd(&self, cwd: &Path) -> PathBuf {
        if cwd.as_os_str().is_empty() {
            return PathBuf::new();
        }
        if find_node(&self.roots, cwd).is_some_and(|node| node.is_dir()) {
            return cwd.to_path_buf();
        }
        match cwd.parent() {
            Some(parent) => self.resolve_cwd(parent),
            None => PathBuf::new(),
        }
    }
}

fn children_of<'a>(nodes: &'a [FsNode], cwd: &Path) -> &'a [FsNode] {
    if cwd.as_os_str().is_empty() {
        return nodes;
    }
    find_node(nodes, cwd)
        .map(|node| node.children.as_slice())
        .unwrap_or(&[])
}

fn find_node<'a>(nodes: &'a [FsNode], rel: &Path) -> Option<&'a FsNode> {
    for node in nodes {
        if node.rel == rel {
            return Some(node);
        }
        if node.is_dir() && rel.starts_with(&node.rel) {
            if let Some(found) = find_node(&node.children, rel) {
                return Some(found);
            }
        }
    }
    None
}

fn collect_matches(nodes: &[FsNode], query: &str, out: &mut Vec<VisibleRow>) {
    let q = query.to_lowercase();
    for node in nodes {
        if node.name.to_lowercase().contains(&q) {
            out.push(row_for(node, true));
        }
        if node.is_dir() {
            collect_matches(&node.children, query, out);
        }
    }
}

fn row_for(node: &FsNode, full_path: bool) -> VisibleRow {
    let mut name = if full_path {
        node.rel.display().to_string()
    } else {
        node.name.clone()
    };
    if node.is_dir() && !name.ends_with('/') {
        name.push('/');
    }
    VisibleRow {
        rel: node.rel.clone(),
        name,
        kind: node.kind,
        prefix: if node.is_dir() {
            "▸ ".to_string()
        } else {
            "  ".to_string()
        },
    }
}

/// Columns left for a row label after the 2-column marker and an optional scrollbar.
pub(crate) fn name_column_budget(inner_width: u16, inner_height: u16, rows: usize) -> usize {
    let scroll = if inner_height > 0 && rows > inner_height as usize {
        1
    } else {
        0
    };
    inner_width.saturating_sub(2).saturating_sub(scroll) as usize
}

/// Keep the start of `name` and a short tail (the extension, when there is one).
pub(crate) fn middle_ellipsis(name: &str, width: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if width == 0 {
        return String::new();
    }
    if chars.len() <= width {
        return name.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let suffix = extension_len(&chars);
    let preferred = 8.max(suffix);
    let max_right = width - 2;
    let mut right = preferred.min(max_right);
    if suffix > 0 && right < suffix && suffix <= max_right {
        right = suffix;
    }
    if right + 1 >= width {
        let mut out: String = chars.iter().take(width - 1).collect();
        out.push('…');
        return out;
    }
    let left = width - 1 - right;
    let mut out: String = chars.iter().take(left).collect();
    out.push('…');
    out.extend(chars[chars.len() - right..].iter());
    out
}

fn extension_len(chars: &[char]) -> usize {
    if chars.last() == Some(&'/') {
        return 1;
    }
    match chars.iter().rposition(|c| *c == '.') {
        Some(dot) if dot > 0 => chars.len() - dot,
        _ => 0,
    }
}

/// Shorten `text` from the left. A tail that contains `/` starts at that segment.
pub(crate) fn left_ellipsis(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if width == 0 {
        return String::new();
    }
    if chars.len() <= width {
        return text.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let start = chars.len() - (width - 1);
    let snap = chars[start..]
        .iter()
        .position(|c| *c == '/')
        .filter(|i| *i > 0)
        .unwrap_or(0);
    let mut out = String::from("…");
    out.extend(chars[start + snap..].iter());
    out
}

/// Pane title. `count` is `None` when no vault is open.
/// `clipped_rel` is the selected path when its label does not fit in the row.
pub(crate) fn tree_title_text(
    label: &str,
    filter: &str,
    clipped_rel: Option<&str>,
    count: Option<usize>,
    width: usize,
) -> String {
    if width == 0 {
        return String::new();
    }
    let count_s = match count {
        Some(n) => format!(" ({n})"),
        None => String::new(),
    };
    let filter_s = if filter.is_empty() {
        String::new()
    } else {
        format!(" /{filter}")
    };
    let body = if let Some(rel) = clipped_rel {
        format!("{label}{filter_s} · {rel}")
    } else {
        format!("{label}{filter_s}")
    };
    let raw = format!(" {body}{count_s} ");
    if raw.chars().count() <= width {
        return raw;
    }
    let suffix = format!("{count_s} ");
    let suffix_len = suffix.chars().count();
    if suffix_len >= width {
        return left_ellipsis(raw.trim(), width);
    }
    let body_room = (width - suffix_len).saturating_sub(1);
    format!(" {}{suffix}", left_ellipsis(&body, body_room))
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
            self.preview_epoch = self.preview_epoch.wrapping_add(1);
        }
    }

    pub fn tree_activate(&mut self) {
        let Some(row) = self.tree.selected_row() else {
            return;
        };
        if row.name == ".." {
            self.tree.leave();
            return;
        }
        match row.kind {
            CoreKind::Dir => self.tree.enter(),
            CoreKind::File | CoreKind::Symlink => {
                let rel = row.rel.clone();
                self.tree.dismiss_filter();
                self.open_file(rel);
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
        if row.name == ".." {
            return;
        }
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
        if row.name == ".." {
            return;
        }
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
        match kind {
            PromptKind::AiProvider => {
                self.set_ai_provider(&draft);
                return;
            }
            PromptKind::AiKey { then_enable } => {
                self.submit_ai_key(draft, then_enable);
                return;
            }
            PromptKind::AiModel => {
                self.submit_ai_model(draft);
                return;
            }
            PromptKind::New { .. } | PromptKind::Rename { .. } | PromptKind::GitCommit { .. } => {}
        }
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
                            self.tree.dismiss_filter();
                            self.tree.reveal(&rel);
                            if !is_dir {
                                self.open_file(rel.clone());
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
            PromptKind::GitCommit { then_push } => {
                self.submit_git_commit(draft, then_push);
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
                            .unwrap_or_else(|_| PathBuf::from(name));
                        let old = rel.to_string_lossy();
                        if self.editor.rel.as_deref() == Some(old.as_ref()) {
                            if let Ok(abs) = dd_vault_core::confined_path(&vault.root, &new_rel) {
                                self.editor.path = Some(abs);
                            }
                            self.editor.rel = Some(new_rel.to_string_lossy().into_owned());
                        }
                        self.modal = None;
                        self.reload_tree();
                        self.tree.dismiss_filter();
                        self.tree.reveal(&new_rel);
                        self.refresh_git();
                        self.push_toast(
                            ToastLevel::Success,
                            format!("Renamed to {}", new_rel.display()),
                        );
                    }
                    Err(err) => self.push_toast(ToastLevel::Error, err.to_string()),
                }
            }
            PromptKind::AiProvider | PromptKind::AiKey { .. } | PromptKind::AiModel => {
                unreachable!("AI prompts return before vault match")
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
        if let ConfirmKind::ForgetVault { name, path } = &kind {
            let name = name.clone();
            let path = path.clone();
            self.forget_vault(&path, &name);
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
                    crate::app::DiscardNext::Navigate(to) => self.navigate_clean(to),
                }
            }
            ConfirmKind::CreateNote { rel, heading } => {
                self.create_and_open_note(rel, heading);
            }
            ConfirmKind::ReloadDisk { rel } => {
                self.modal = None;
                self.suppress_disk_prompt = None;
                let opened = dd_vault_core::confined_path(&vault.root, &rel)
                    .ok()
                    .filter(|path| path.is_file());
                let Some(abs) = opened else {
                    self.editor = dd_edit::Editor::empty();
                    self.push_toast(
                        ToastLevel::Warning,
                        format!("{} is gone from disk", rel.display()),
                    );
                    return;
                };
                if self.editor.rel.as_deref() == Some(rel.to_string_lossy().as_ref()) {
                    self.editor.path = Some(abs);
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
            ConfirmKind::ForgetVault { name, path } => {
                self.forget_vault(&path, &name);
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
