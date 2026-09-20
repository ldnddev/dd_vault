//! Debounced filesystem watcher for the open vault.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::app::{App, ConfirmKind, Modal};
use crate::toasts::ToastLevel;
use dd_vault_core::{skip_dir_name, skip_file_name};

pub const WATCH_DEBOUNCE: Duration = Duration::from_millis(200);

pub struct VaultWatch {
    _watcher: RecommendedWatcher,
    rx: mpsc::Receiver<Event>,
    pending: Vec<PathBuf>,
    deadline: Option<Instant>,
}

impl VaultWatch {
    pub fn start(root: &Path) -> Result<Self, notify::Error> {
        let (tx, rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                if let Ok(ev) = res {
                    let _ = tx.send(ev);
                }
            },
            notify::Config::default(),
        )?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        Ok(Self {
            _watcher: watcher,
            rx,
            pending: Vec::new(),
            deadline: None,
        })
    }

    fn drain(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            self.pending.extend(ev.paths);
            self.deadline = Some(Instant::now() + WATCH_DEBOUNCE);
        }
    }

    /// Paths ready after debounce, if any.
    pub fn take_ready(&mut self) -> Option<Vec<PathBuf>> {
        self.drain();
        let due = self.deadline.filter(|d| Instant::now() >= *d)?;
        let _ = due;
        self.deadline = None;
        if self.pending.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pending))
        }
    }
}

pub fn watch_rel(root: &Path, abs: &Path) -> Option<PathBuf> {
    let rel = abs.strip_prefix(root).ok()?;
    for c in rel.components() {
        let name = c.as_os_str().to_string_lossy();
        if skip_dir_name(&name) || skip_file_name(&name) {
            return None;
        }
    }
    Some(rel.to_path_buf())
}

impl App {
    pub fn start_watch(&mut self, root: &Path) {
        match VaultWatch::start(root) {
            Ok(w) => self.watch = Some(w),
            Err(err) => {
                self.watch = None;
                self.push_toast(
                    ToastLevel::Warning,
                    format!("File watch unavailable: {err}"),
                );
            }
        }
    }

    pub fn poll_watch(&mut self) {
        let paths = self.watch.as_mut().and_then(VaultWatch::take_ready);
        if let Some(paths) = paths {
            self.handle_fs_changes(&paths);
        }
        if self.modal.is_none() {
            if let Some(rel) = self.pending_disk_prompt.take() {
                self.prompt_disk_change(rel);
            }
        }
    }

    pub fn handle_fs_changes(&mut self, abs_paths: &[PathBuf]) {
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let mut rels = Vec::new();
        for abs in abs_paths {
            if let Some(rel) = watch_rel(&vault.root, abs) {
                if !rels.contains(&rel) {
                    rels.push(rel);
                }
            }
        }
        if rels.is_empty() {
            return;
        }
        self.reload_tree();
        self.refresh_git();
        self.queue_reindex();
        if let Some(open) = self.editor.rel.clone() {
            let open_path = PathBuf::from(&open);
            if rels
                .iter()
                .any(|r| r == &open_path || open_path.starts_with(r))
            {
                self.on_open_file_changed(&vault.root, &open_path);
            }
        }
    }

    pub(crate) fn queue_reindex(&mut self) {
        if self.index_rx.is_some() {
            self.reindex_queued = true;
            return;
        }
        self.spawn_reindex();
    }

    pub(crate) fn spawn_reindex(&mut self) {
        let Some(vault) = self.vault.clone() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(dd_vault_core::reindex(&vault).map_err(|e| e.to_string()));
        });
        self.index_rx = Some(rx);
        self.quiet_reindex = true;
    }

    fn on_open_file_changed(&mut self, root: &Path, rel: &Path) {
        if self.suppress_disk_prompt.as_ref().is_some_and(|s| s == rel) {
            return;
        }
        let abs = root.join(rel);
        if !abs.is_file() {
            if self.editor.dirty {
                self.prompt_disk_change(rel.to_path_buf());
            } else {
                self.editor = dd_edit::Editor::empty();
                self.push_toast(
                    ToastLevel::Warning,
                    format!("{} disappeared from disk", rel.display()),
                );
            }
            return;
        }
        let disk = match std::fs::read_to_string(&abs) {
            Ok(s) => s,
            Err(err) => {
                self.push_toast(ToastLevel::Warning, format!("Watch read failed: {err}"));
                return;
            }
        };
        if disk == self.editor.text() {
            return;
        }
        if self.editor.dirty {
            self.prompt_disk_change(rel.to_path_buf());
        } else if let Err(err) = self.editor.reload_from_disk() {
            self.push_toast(ToastLevel::Error, err.to_string());
        } else {
            self.push_toast(ToastLevel::Info, format!("Reloaded {}", rel.display()));
        }
    }

    fn prompt_disk_change(&mut self, rel: PathBuf) {
        if self.modal.is_some() {
            self.pending_disk_prompt = Some(rel);
            return;
        }
        self.modal = Some(Modal::Confirm {
            kind: ConfirmKind::ReloadDisk { rel: rel.clone() },
            message: format!(
                "{} changed on disk. Reload and discard unsaved edits?",
                rel.display()
            ),
        });
    }
}
