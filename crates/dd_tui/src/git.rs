//! `:git pull|push|commit` and editor-title badge.

use std::path::PathBuf;

use dd_edit::GitOp;
use dd_vault_core::{
    commit, load_secret_patterns, pull, push, status, usable_credentials, GitCtx, GitOpResult,
};

use crate::app::{App, Modal, PromptKind};
use crate::toasts::ToastLevel;

impl App {
    pub fn refresh_git(&mut self) {
        let Some(vault) = &self.vault else {
            self.git = dd_vault_core::GitStatus::default();
            return;
        };
        match status(&vault.root) {
            Ok(st) => self.git = st,
            Err(err) => {
                self.git = dd_vault_core::GitStatus::default();
                self.push_toast(ToastLevel::Warning, err.to_string());
            }
        }
    }

    pub fn handle_git(&mut self, op: GitOp) {
        match op {
            GitOp::Status => {
                self.refresh_git();
                self.push_toast(ToastLevel::Info, self.git.detail());
            }
            GitOp::Commit => self.begin_git_commit(false),
            GitOp::Push => {
                if !self.prepare_git_op() {
                    return;
                }
                match self.git.state {
                    dd_vault_core::GitState::Conflict => {
                        self.push_toast(
                            ToastLevel::Error,
                            "unmerged files; :git pull to resolve before push",
                        );
                    }
                    dd_vault_core::GitState::Dirty(_) => self.begin_git_commit(true),
                    _ => self.run_git_op("push", push),
                }
            }
            GitOp::Pull => self.run_git_op("pull", pull),
        }
    }

    fn prepare_git_op(&mut self) -> bool {
        if self.vault.is_none() {
            self.push_toast(ToastLevel::Error, "No vault open");
            return false;
        }
        if self.editor.dirty {
            self.save_note();
            if self.editor.dirty {
                return false;
            }
        }
        self.refresh_git();
        if matches!(self.git.state, dd_vault_core::GitState::NotRepo) {
            self.push_toast(
                ToastLevel::Error,
                "not a git repository (run git init in the vault to enable sync)",
            );
            return false;
        }
        true
    }

    fn begin_git_commit(&mut self, then_push: bool) {
        if !self.prepare_git_op() {
            return;
        }
        if matches!(self.git.state, dd_vault_core::GitState::Conflict) {
            self.push_toast(
                ToastLevel::Error,
                "unmerged files; :git pull to resolve before commit",
            );
            return;
        }
        self.modal = Some(Modal::Prompt {
            kind: PromptKind::GitCommit { then_push },
            draft: String::new(),
        });
    }

    fn run_git_op(
        &mut self,
        name: &str,
        op: fn(&GitCtx<'_>) -> Result<GitOpResult, dd_vault_core::Error>,
    ) {
        let Some(result) = self.with_git_ctx(|ctx| op(&ctx)) else {
            return;
        };
        match result {
            Ok(res) => self.finish_git_result(name, res),
            Err(err) => {
                self.refresh_git();
                self.push_toast(ToastLevel::Error, err.to_string());
            }
        }
    }

    fn with_git_ctx<T>(&mut self, f: impl FnOnce(GitCtx<'_>) -> T) -> Option<T> {
        let vault = match &self.vault {
            Some(v) => v.clone(),
            None => {
                self.push_toast(ToastLevel::Error, "No vault open");
                return None;
            }
        };
        let extra = load_secret_patterns(&vault, self.paths.as_ref());
        let cred_path = self.paths.as_ref().map(|p| p.credentials_file());
        let mut cred_ok = None;
        if let Some(path) = cred_path.as_ref() {
            match usable_credentials(path) {
                Ok(c) => cred_ok = c,
                Err(err) => self.push_toast(ToastLevel::Warning, err.to_string()),
            }
        }
        Some(f(GitCtx {
            root: &vault.root,
            credentials: cred_ok.as_deref(),
            extra_secret_patterns: &extra,
        }))
    }

    pub fn submit_git_commit(&mut self, message: String, then_push: bool) {
        if self.editor.dirty {
            self.save_note();
            if self.editor.dirty {
                return;
            }
        }
        let msg = message;
        let result = self.with_git_ctx(|ctx| commit(&ctx, &msg));
        let Some(result) = result else {
            return;
        };
        match result {
            Ok(res) => {
                self.modal = None;
                self.finish_git_result("commit", res);
                if then_push {
                    self.run_git_op("push", push);
                }
            }
            Err(err) => self.push_toast(ToastLevel::Error, err.to_string()),
        }
    }

    fn finish_git_result(&mut self, op: &str, res: GitOpResult) {
        self.refresh_git();
        self.reload_tree();
        self.reload_open_if_clean();
        self.queue_reindex();
        if res.sidecars.is_empty() {
            let level = if op == "commit" && res.message.starts_with("Nothing") {
                ToastLevel::Info
            } else {
                ToastLevel::Success
            };
            self.push_toast(level, res.message);
            return;
        }
        let listed: Vec<String> = res
            .sidecars
            .iter()
            .map(|p| {
                self.vault
                    .as_ref()
                    .and_then(|v| p.strip_prefix(&v.root).ok())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| p.clone())
                    .display()
                    .to_string()
            })
            .collect();
        self.push_toast(ToastLevel::Warning, res.message.clone());
        self.modal = Some(Modal::Notice {
            title: " Git conflicts ".into(),
            message: format!(
                "{}\n\nOriginal kept. Incoming copies:\n{}",
                res.message,
                listed
                    .iter()
                    .map(|s| format!("  • {s}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        });
    }

    fn reload_open_if_clean(&mut self) {
        if self.editor.dirty || self.editor.rel.is_none() {
            return;
        }
        if let Err(err) = self.editor.reload_from_disk() {
            self.push_toast(ToastLevel::Warning, err.to_string());
        }
    }
}
