//! Debounced filesystem watcher. Open and read events are dropped so the app's
//! own reads do not ask to reload the note.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use dd_vault_core::{skip_dir_name, skip_file_name};
use notify::event::{AccessKind, AccessMode, EventKind};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

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
            if !is_watch_content_kind(ev.kind) {
                continue;
            }
            if ev.paths.is_empty() {
                continue;
            }
            self.pending.extend(ev.paths);
            self.deadline = Some(Instant::now() + WATCH_DEBOUNCE);
        }
    }

    pub fn take_ready(&mut self) -> Option<Vec<PathBuf>> {
        self.drain();
        self.deadline
            .filter(|deadline| Instant::now() >= *deadline)?;
        self.deadline = None;
        if self.pending.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.pending))
        }
    }
}

/// Keep create, modify, remove, and close-after-write. Drop open and read.
pub fn is_watch_content_kind(kind: EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        EventKind::Other => false,
        EventKind::Any | EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => true,
    }
}

pub fn watch_rel(root: &Path, abs: &Path) -> Option<PathBuf> {
    let rel = abs.strip_prefix(root).ok()?;
    for component in rel.components() {
        let name = component.as_os_str().to_string_lossy();
        if skip_dir_name(&name) || skip_file_name(&name) {
            return None;
        }
    }
    Some(rel.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, ModifyKind, RemoveKind};

    #[test]
    fn open_and_read_are_dropped() {
        assert!(!is_watch_content_kind(EventKind::Access(AccessKind::Open(
            AccessMode::Any
        ))));
        assert!(!is_watch_content_kind(EventKind::Access(AccessKind::Read)));
        assert!(!is_watch_content_kind(EventKind::Access(
            AccessKind::Close(AccessMode::Read)
        )));
        assert!(!is_watch_content_kind(EventKind::Other));
        assert!(is_watch_content_kind(EventKind::Access(AccessKind::Close(
            AccessMode::Write
        ))));
        assert!(is_watch_content_kind(EventKind::Create(CreateKind::File)));
        assert!(is_watch_content_kind(EventKind::Modify(ModifyKind::Any)));
        assert!(is_watch_content_kind(EventKind::Remove(RemoveKind::File)));
        assert!(is_watch_content_kind(EventKind::Any));
    }

    #[test]
    fn watch_paths_skip_git_meta_and_databases() {
        let root = Path::new("/vault");
        assert_eq!(
            watch_rel(root, Path::new("/vault/notes/a.md")),
            Some(PathBuf::from("notes/a.md"))
        );
        assert_eq!(watch_rel(root, Path::new("/vault/.git/config")), None);
        assert_eq!(
            watch_rel(root, Path::new("/vault/.dd_vault-name/index.db")),
            None
        );
        assert_eq!(watch_rel(root, Path::new("/vault/notes/.DS_Store")), None);
        assert_eq!(watch_rel(root, Path::new("/other/notes/a.md")), None);
    }
}
