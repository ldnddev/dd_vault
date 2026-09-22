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
        if self.git_rx.is_some() {
            self.push_toast(ToastLevel::Info, "git is already running");
            return false;
        }
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
        name: &'static str,
        op: fn(&GitCtx<'_>) -> Result<GitOpResult, dd_vault_core::Error>,
    ) {
        if !self.prepare_git_op() {
            return;
        }
        let Some((root, extra, cred)) = self.git_job_parts() else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let ctx = GitCtx {
                root: &root,
                credentials: cred.as_deref(),
                extra_secret_patterns: &extra,
            };
            let _ = tx.send(
                op(&ctx)
                    .map(|r| (name.to_string(), r))
                    .map_err(|e| e.to_string()),
            );
        });
        self.git_rx = Some(rx);
        self.busy_kind = Some(match name {
            "pull" => "pulling",
            "push" => "pushing",
            _ => "working",
        });
    }

    fn git_job_parts(&mut self) -> Option<(PathBuf, Vec<String>, Option<PathBuf>)> {
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
        Some((vault.root, extra, cred_ok))
    }

    pub fn submit_git_commit(&mut self, message: String, then_push: bool) {
        if !self.prepare_git_op() {
            return;
        }
        let Some((root, extra, cred)) = self.git_job_parts() else {
            return;
        };
        self.modal = None;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let ctx = GitCtx {
                root: &root,
                credentials: cred.as_deref(),
                extra_secret_patterns: &extra,
            };
            let result = (|| {
                let committed = commit(&ctx, &message).map_err(|e| e.to_string())?;
                if then_push {
                    match push(&ctx) {
                        Ok(pushed) => Ok((
                            "push".to_string(),
                            GitOpResult {
                                message: format!("{}; {}", committed.message, pushed.message),
                                sidecars: committed.sidecars,
                                auto_merged: committed.auto_merged,
                            },
                        )),
                        Err(err) => Err(format!("{}; push failed: {err}", committed.message)),
                    }
                } else {
                    Ok(("commit".to_string(), committed))
                }
            })();
            let _ = tx.send(result);
        });
        self.git_rx = Some(rx);
        self.busy_kind = Some(if then_push { "pushing" } else { "committing" });
    }

    pub fn poll_git(&mut self) {
        let recv = self.git_rx.as_ref().map(|r| r.try_recv());
        match recv {
            Some(Ok(Ok((op, res)))) => {
                self.git_rx = None;
                self.busy_kind = None;
                self.finish_git_result(&op, res);
            }
            Some(Ok(Err(err))) => {
                self.git_rx = None;
                self.busy_kind = None;
                self.refresh_git();
                self.push_toast(ToastLevel::Error, err);
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.git_rx = None;
                self.busy_kind = None;
            }
            _ => {}
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
