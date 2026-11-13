#[allow(unused_imports)] // `cargo test` reaches this through `crate::tree::left_ellipsis`.
pub use dd_vault_core::left_ellipsis;
pub use dd_vault_core::{
    middle_ellipsis, name_column_budget, tree_title_text, TreeState, VisibleRow,
};

use std::path::{Path, PathBuf};

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
            cursor: 0,
        });
    }

    pub fn begin_rename(&mut self) {
        let Some(row) = self.tree.selected_row() else {
            return;
        };
        if row.name == ".." {
            return;
        }
        let draft = row
            .rel
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let cursor = draft.chars().count();
        self.modal = Some(Modal::Prompt {
            kind: PromptKind::Rename {
                rel: row.rel.clone(),
            },
            draft,
            cursor,
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
        let Some(Modal::Prompt { kind, draft, .. }) = self.modal.clone() else {
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
