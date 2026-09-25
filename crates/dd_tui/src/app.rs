use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use dd_edit::{Action, Editor};
use dd_vault_core::{
    open, FileHit, GitOpResult, GitStatus, Index, Paths, Registry, ReindexReport, Vault,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

use crate::ai::AiState;
use crate::theme::{choose_header_copy, AppTheme, ThemeLoad};
use crate::toasts::{Toast, ToastLevel};
use crate::tree::TreeState;

pub const DOUBLE_CLICK_MS: u128 = 420;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Tree,
    Editor,
    Preview,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Leader {
    #[default]
    None,
    Space,
    SpaceV,
    SpaceF,
    SpaceS,
    SpaceT,
    SpaceN,
    SpaceA,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Modal {
    VaultPicker {
        selected: usize,
    },
    Prompt {
        kind: PromptKind,
        draft: String,
    },
    Confirm {
        kind: ConfirmKind,
        message: String,
    },
    Finder {
        kind: FinderKind,
        query: String,
        selected: usize,
        hits: Vec<FileHit>,
    },
    Notice {
        title: String,
        message: String,
    },
    Palette {
        query: String,
        selected: usize,
        items: Vec<PaletteItem>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteItem {
    pub id: &'static str,
    pub label: &'static str,
    pub keys: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinderKind {
    Files,
    Content,
    Tags,
    Wiki,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptKind {
    New { parent: PathBuf },
    Rename { rel: PathBuf },
    GitCommit { then_push: bool },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfirmKind {
    Delete { rel: PathBuf },
    Discard { next: DiscardNext },
    ReloadDisk { rel: PathBuf },
    AiSend,
    ForgetVault { name: String, path: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscardNext {
    Quit,
    Open(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MouseDrag {
    pub start_line: usize,
    pub start_col: usize,
    pub linewise: bool,
    pub moved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDrag {
    Tree,
    Preview,
}

pub struct App {
    pub theme: AppTheme,
    pub header_copy: String,
    pub vault: Option<Vault>,
    pub registry: Registry,
    pub paths: Option<Paths>,
    pub modal: Option<Modal>,
    pub leader: Leader,
    pub toasts: Vec<Toast>,
    pub should_quit: bool,
    pub show_help: bool,
    pub help_scroll: u16,
    pub help_scroll_max: u16,
    pub show_theme: bool,
    pub theme_editor: Option<ldnddev_theme::ThemeEditor>,
    pub theme_status: Option<String>,
    pub pane: Pane,
    pub tree: TreeState,
    pub tree_area: Rect,
    pub tree_inner: Rect,
    pub editor_area: Rect,
    pub last_click: Option<(u16, u16, Instant)>,
    pub mouse_drag: Option<MouseDrag>,
    pub editor: Editor,
    pub editor_inner: Rect,
    /// Soft-wrap the notes pane. Loaded from the vault `wrap` setting. Preview always wraps.
    pub wrap_notes: bool,
    pub preview_visible: bool,
    pub preview_scroll: u16,
    pub preview_area: Rect,
    pub preview_inner: Rect,
    pub index: Option<Index>,
    pub index_rx: Option<mpsc::Receiver<Result<ReindexReport, String>>>,
    pub watch: Option<crate::watch::VaultWatch>,
    pub reindex_queued: bool,
    pub quiet_reindex: bool,
    pub suppress_disk_prompt: Option<PathBuf>,
    pub pending_disk_prompt: Option<PathBuf>,
    pub git: GitStatus,
    pub git_rx: Option<mpsc::Receiver<Result<(String, GitOpResult), String>>>,
    pub busy_kind: Option<&'static str>,
    pub ai: AiState,
    pub zen: bool,
    pub focus: bool,
    pub explore: bool,
    pub tree_split: u16,
    pub preview_split: u16,
    pub split_drag: Option<SplitDrag>,
    pub body_width: u16,
}

impl App {
    pub fn from_load(load: ThemeLoad) -> Self {
        let mut app = Self::new(load.theme);
        app.theme_status = load.warning.clone();
        if let Some(msg) = load.warning {
            app.push_toast(ToastLevel::Warning, msg);
        }
        app
    }

    pub fn new(theme: AppTheme) -> Self {
        let header_copy = choose_header_copy(&theme.header_quotes);
        Self {
            theme,
            header_copy,
            vault: None,
            registry: Registry::default(),
            paths: None,
            modal: None,
            leader: Leader::None,
            toasts: Vec::new(),
            should_quit: false,
            show_help: false,
            help_scroll: 0,
            help_scroll_max: 0,
            show_theme: false,
            theme_editor: None,
            theme_status: None,
            pane: Pane::Tree,
            tree: TreeState::default(),
            tree_area: Rect::default(),
            tree_inner: Rect::default(),
            editor_area: Rect::default(),
            last_click: None,
            mouse_drag: None,
            editor: Editor::empty(),
            editor_inner: Rect::default(),
            wrap_notes: true,
            preview_visible: true,
            preview_scroll: 0,
            preview_area: Rect::default(),
            preview_inner: Rect::default(),
            index: None,
            index_rx: None,
            watch: None,
            reindex_queued: false,
            quiet_reindex: false,
            suppress_disk_prompt: None,
            pending_disk_prompt: None,
            git: GitStatus::default(),
            git_rx: None,
            busy_kind: None,
            ai: AiState::default(),
            zen: false,
            focus: false,
            explore: false,
            tree_split: 0,
            preview_split: 0,
            split_drag: None,
            body_width: 120,
        }
    }

    pub fn attach_registry(&mut self) {
        match Paths::from_env() {
            Ok(paths) => {
                match Registry::load(&paths) {
                    Ok(reg) => self.registry = reg,
                    Err(err) => self.push_toast(ToastLevel::Warning, err.to_string()),
                }
                self.paths = Some(paths);
                self.load_ai_settings();
            }
            Err(err) => self.push_toast(ToastLevel::Warning, err.to_string()),
        }
    }

    pub fn apply_initial_vault(&mut self, initial: Option<Vault>) {
        if let Some(vault) = initial {
            self.set_open_vault(vault);
            return;
        }
        if let Some(path) = self.registry.last_path() {
            match open(&path) {
                Ok(vault) => {
                    self.set_open_vault(vault);
                    return;
                }
                Err(err) => {
                    self.push_toast(
                        ToastLevel::Warning,
                        format!("Last vault unavailable: {err}"),
                    );
                }
            }
        }
        if !self.registry.vaults.is_empty() {
            self.open_picker();
        } else {
            self.push_toast(ToastLevel::Info, "No vault. Run: dd_vault init [path]");
        }
    }

    pub fn set_open_vault(&mut self, vault: Vault) {
        self.wrap_notes = dd_vault_core::VaultConfig::load(&vault.meta_dir).wrap;
        self.registry.register(&vault);
        self.persist_registry();
        self.tree = TreeState::load(&vault);
        self.editor = Editor::empty();
        self.preview_scroll = 0;
        match Index::open(&vault.meta_dir) {
            Ok(idx) => self.index = Some(idx),
            Err(err) => {
                self.index = None;
                self.push_toast(ToastLevel::Warning, format!("Index: {err}"));
            }
        }
        self.reindex_queued = false;
        self.quiet_reindex = false;
        self.suppress_disk_prompt = None;
        self.pending_disk_prompt = None;
        self.vault = Some(vault);
        self.refresh_git();
        self.spawn_reindex();
        if let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) {
            self.start_watch(&root);
        }
        self.modal = None;
        self.leader = Leader::None;
        if let Some(v) = &self.vault {
            self.push_toast(ToastLevel::Success, format!("Opened vault: {}", v.name));
        }
    }

    pub fn persist_registry(&mut self) {
        if let Some(paths) = &self.paths {
            if let Err(err) = self.registry.save(paths) {
                self.push_toast(
                    ToastLevel::Warning,
                    format!("Could not save vaults.toml: {err}"),
                );
            }
        }
    }

    pub fn open_picker(&mut self) {
        if self.registry.vaults.is_empty() {
            self.push_toast(
                ToastLevel::Info,
                "No registered vaults. Run: dd_vault init [path]",
            );
            return;
        }
        let selected = self
            .registry
            .last_path()
            .and_then(|p| {
                let s = p.to_string_lossy();
                self.registry.vaults.iter().position(|e| e.path == s)
            })
            .unwrap_or(0);
        self.modal = Some(Modal::VaultPicker { selected });
        self.leader = Leader::None;
    }

    pub fn begin_forget_vault(&mut self) {
        let Some(Modal::VaultPicker { selected }) = self.modal else {
            return;
        };
        let Some(entry) = self.registry.vaults.get(selected).cloned() else {
            return;
        };
        self.modal = Some(Modal::Confirm {
            kind: ConfirmKind::ForgetVault {
                name: entry.name.clone(),
                path: entry.path.clone(),
            },
            message: format!(
                "Remove vault '{}' from the list?\n{}\n\nThe folder on disk is not deleted.",
                entry.name, entry.path
            ),
        });
    }

    pub fn forget_vault(&mut self, path: &str, name: &str) {
        let was_open = self
            .vault
            .as_ref()
            .is_some_and(|v| v.root.to_string_lossy() == path);
        if !self.registry.unregister(path) {
            self.modal = None;
            return;
        }
        self.persist_registry();
        if was_open {
            self.close_vault();
        }
        self.push_toast(
            ToastLevel::Success,
            format!("Removed '{name}' from the list (files kept)"),
        );
        if self.registry.vaults.is_empty() {
            self.modal = None;
        } else {
            self.open_picker();
        }
    }

    pub fn close_vault(&mut self) {
        self.watch = None;
        self.index_rx = None;
        self.index = None;
        self.vault = None;
        self.tree = TreeState::default();
        self.editor = Editor::empty();
        self.git = GitStatus::default();
        self.git_rx = None;
        self.busy_kind = None;
        self.preview_scroll = 0;
    }

    pub fn activate_picker_selection(&mut self) {
        let Some(Modal::VaultPicker { selected }) = self.modal else {
            return;
        };
        let Some(entry) = self.registry.vaults.get(selected).cloned() else {
            return;
        };
        match open(std::path::Path::new(&entry.path)) {
            Ok(vault) => self.set_open_vault(vault),
            Err(err) => self.push_toast(ToastLevel::Error, err.to_string()),
        }
    }

    pub fn push_toast(&mut self, level: ToastLevel, message: impl Into<String>) {
        crate::toasts::push_toast(&mut self.toasts, level, message);
    }

    /// Rows of the notes pane that show buffer text (the command line takes the last row).
    pub fn editor_text_rows(&self) -> usize {
        let h = self.editor_inner.height as usize;
        if h == 0 {
            return 0;
        }
        if matches!(
            self.editor.mode,
            dd_edit::Mode::Command | dd_edit::Mode::Search
        ) {
            h.saturating_sub(1)
        } else {
            h
        }
    }

    /// Columns available for note text. `usize::MAX` means do not wrap.
    pub fn editor_wrap_width(&self) -> usize {
        if !self.wrap_notes {
            return usize::MAX;
        }
        let gutter = self.editor.gutter_cols();
        (self.editor_inner.width.saturating_sub(gutter) as usize).max(1)
    }

    pub fn editor_title(&self) -> String {
        let name = self.editor.rel.as_deref().unwrap_or("(no note)");
        let dirty = if self.editor.dirty { " ±" } else { "" };
        match self.git.state.title_badge_owned() {
            Some(badge) => format!(" {name} — {}{dirty}  {badge} ", self.editor.mode.label()),
            None => format!(" {name} — {}{dirty} ", self.editor.mode.label()),
        }
    }

    pub fn load_note(&mut self, rel: PathBuf) {
        let Some(vault) = &self.vault else {
            return;
        };
        let abs = vault.root.join(&rel);
        match Editor::open(abs, rel.to_string_lossy().into_owned()) {
            Ok(ed) => {
                self.editor = ed;
                self.pane = Pane::Editor;
            }
            Err(err) => self.push_toast(ToastLevel::Error, err.to_string()),
        }
    }

    pub fn save_note(&mut self) {
        match self.editor.save_to_disk() {
            Ok(()) => {
                self.editor.mark_saved();
                let name = self.editor.rel.as_deref().unwrap_or("file");
                self.push_toast(ToastLevel::Success, format!("Saved {name}"));
                self.refresh_git();
            }
            Err(err) => self.push_toast(ToastLevel::Error, err.to_string()),
        }
    }

    pub fn request_quit(&mut self, force: bool) {
        if force || !self.editor.dirty {
            self.should_quit = true;
            return;
        }
        self.modal = Some(Modal::Confirm {
            kind: ConfirmKind::Discard {
                next: DiscardNext::Quit,
            },
            message: "Discard unsaved changes and quit?".into(),
        });
    }

    pub fn open_file(&mut self, rel: PathBuf) {
        if self.editor.dirty {
            self.modal = Some(Modal::Confirm {
                kind: ConfirmKind::Discard {
                    next: DiscardNext::Open(rel.clone()),
                },
                message: format!("Discard unsaved changes and open {}?", rel.display()),
            });
            return;
        }
        self.load_note(rel);
    }

    pub fn apply_editor_action(&mut self, action: Action) {
        match action {
            Action::None => {}
            Action::Save => self.save_note(),
            Action::Quit => self.request_quit(false),
            Action::QuitForce => self.request_quit(true),
            Action::SaveQuit => {
                self.save_note();
                if !self.editor.dirty {
                    self.should_quit = true;
                }
            }
            Action::Open(name) => {
                let rel = std::path::PathBuf::from(name);
                self.open_file(rel);
            }
            Action::Help => {
                self.show_help = true;
                self.help_scroll = 0;
            }
            Action::Error(msg) => self.push_toast(ToastLevel::Error, msg),
            Action::Info(msg) => self.push_toast(ToastLevel::Info, msg),
            Action::WikiPicker => self.open_finder(FinderKind::Wiki),
            Action::Git(op) => self.handle_git(op),
            Action::Daily => self.open_daily(),
            Action::AiToggle => self.cycle_ai_card(),
            Action::AiOn => self.ai_enable(),
            Action::AiOff => self.ai_disable(),
            Action::AiPrompt(p) => self.request_ai(p, None),
            Action::CopyClipboard(text) => self.copy_text_to_clipboard(&text),
        }
    }

    pub fn poll_index(&mut self) {
        let recv = self.index_rx.as_ref().map(mpsc::Receiver::try_recv);
        match recv {
            Some(Ok(Ok(report))) => {
                if !self.quiet_reindex {
                    self.push_toast(
                        ToastLevel::Success,
                        format!("Indexed {} notes", report.notes_indexed),
                    );
                }
                if let Some(v) = &self.vault {
                    self.index = Index::open(&v.meta_dir).ok();
                }
                self.index_rx = None;
                if self.reindex_queued {
                    self.reindex_queued = false;
                    self.quiet_reindex = true;
                    self.spawn_reindex();
                }
            }
            Some(Ok(Err(err))) => {
                if !self.quiet_reindex {
                    self.push_toast(ToastLevel::Error, format!("Reindex failed: {err}"));
                }
                self.index_rx = None;
            }
            Some(Err(TryRecvError::Disconnected)) => self.index_rx = None,
            _ => {}
        }
    }

    pub fn open_finder(&mut self, kind: FinderKind) {
        self.leader = Leader::None;
        if self.index.is_none() {
            self.push_toast(ToastLevel::Warning, "Index not ready yet");
            return;
        }
        let hits = self.finder_hits(kind, "");
        self.modal = Some(Modal::Finder {
            kind,
            query: String::new(),
            selected: 0,
            hits,
        });
    }

    pub fn finder_hits(&self, kind: FinderKind, query: &str) -> Vec<FileHit> {
        let Some(idx) = &self.index else {
            return Vec::new();
        };
        let res = match kind {
            FinderKind::Files => idx.search_files(query),
            FinderKind::Content => idx.search_content(query),
            FinderKind::Tags => idx.search_tags(query),
            FinderKind::Wiki => idx.search_wiki(query),
        };
        res.unwrap_or_default()
    }

    pub fn refresh_finder(&mut self) {
        let Some(Modal::Finder { kind, query, .. }) = &self.modal else {
            return;
        };
        let kind = *kind;
        let query = query.clone();
        let hits = self.finder_hits(kind, &query);
        if let Some(Modal::Finder {
            selected,
            hits: slot,
            ..
        }) = &mut self.modal
        {
            *slot = hits;
            *selected = (*selected).min(slot.len().saturating_sub(1));
        }
    }

    pub fn activate_finder(&mut self) {
        let Some(Modal::Finder {
            kind,
            selected,
            hits,
            ..
        }) = &self.modal
        else {
            return;
        };
        let Some(hit) = hits.get(*selected).cloned() else {
            return;
        };
        let wiki = *kind == FinderKind::Wiki && self.editor.mode == dd_edit::Mode::Insert;
        self.modal = None;
        if wiki {
            let stem = std::path::Path::new(&hit.path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&hit.title);
            let insert = match &hit.heading {
                Some(h) => format!("{stem}#{h}]]"),
                None => format!("{stem}]]"),
            };
            self.editor.insert_text(&insert);
        } else {
            self.open_file(PathBuf::from(hit.path));
        }
    }

    pub fn toggle_preview(&mut self) {
        self.preview_visible = !self.preview_visible;
        if !self.preview_visible && self.pane == Pane::Preview {
            self.pane = Pane::Editor;
        }
        self.leader = Leader::None;
    }

    pub fn cycle_pane(&mut self) {
        self.pane = match self.pane {
            Pane::Tree => Pane::Editor,
            Pane::Editor if self.preview_layout_visible() => Pane::Preview,
            Pane::Editor => {
                if self.tree_layout_visible() {
                    Pane::Tree
                } else {
                    Pane::Editor
                }
            }
            Pane::Preview => {
                if self.tree_layout_visible() {
                    Pane::Tree
                } else {
                    Pane::Editor
                }
            }
        };
    }

    pub fn tree_layout_visible(&self) -> bool {
        if self.zen {
            return false;
        }
        if self.explore {
            return true;
        }
        if self.focus {
            return false;
        }
        self.body_width >= 80
    }

    pub fn preview_layout_visible(&self) -> bool {
        if self.zen {
            return false;
        }
        self.preview_visible || self.ai.size == crate::ai::AiSize::Full
    }

    pub fn ai_layout_visible(&self) -> bool {
        !self.zen
    }

    pub fn toggle_focus(&mut self) {
        self.leader = Leader::None;
        if self.zen {
            self.zen = false;
        }
        self.focus = !self.focus;
        self.explore = false;
        if self.focus && self.pane == Pane::Tree {
            self.pane = Pane::Editor;
        }
    }

    pub fn toggle_zen(&mut self) {
        self.leader = Leader::None;
        self.zen = !self.zen;
        if self.zen {
            self.collapse_ai();
            self.pane = Pane::Editor;
        }
    }

    pub fn toggle_explore(&mut self) {
        self.leader = Leader::None;
        if self.zen {
            self.zen = false;
        }
        self.explore = !self.explore;
        if self.explore {
            self.focus = false;
            self.pane = Pane::Tree;
        } else if self.pane == Pane::Tree {
            self.pane = Pane::Editor;
        }
    }

    pub fn vault_header_label(&self) -> String {
        self.vault
            .as_ref()
            .map(Vault::header_label)
            .unwrap_or_else(|| " no vault ".to_string())
    }

    pub fn footer_hint(&self, width: u16) -> String {
        let parts: &[&str] = if self.tree.filtering {
            if width < 80 {
                &["Esc:Clear", "Enter:Keep", "type"]
            } else {
                &["/: filter   Esc: clear   Enter: keep   type to filter"]
            }
        } else if self.show_help || self.show_theme || self.modal.is_some() {
            if width < 80 {
                &["F1:Help", "Esc:Close", "^Q:Quit"]
            } else {
                &["F1: Help", "Esc: Close", "Ctrl+Q: Quit"]
            }
        } else if width < 80 {
            &["F1:Help", "F2:Theme", "^Q:Quit", ":w", "<Space>ff"]
        } else if width < 120 {
            &[
                "F1: Help",
                "F2: Theme",
                "j/k: Nav",
                "i: Insert",
                ":w Save",
                "Ctrl+Q: Quit",
            ]
        } else {
            &[
                "F1: Help",
                "F2: Theme",
                "j/k: Nav",
                "i: Insert",
                ":w Save",
                "<Space>ff Files",
                "<Space>p Preview",
                "<Space>vv Vault",
                "<Space>ai AI",
                "Ctrl+Q: Quit",
                "(mouse: click/scroll)",
            ]
        };
        parts.join("  ")
    }

    pub fn is_busy(&self) -> bool {
        self.git_rx.is_some() || self.ai.rx.is_some()
    }

    pub fn busy_status(&self) -> Option<&'static str> {
        if self.git_rx.is_some() {
            return self.busy_kind;
        }
        if self.ai.rx.is_some() {
            return Some("thinking");
        }
        None
    }
}

pub fn run(initial: Option<Vault>) -> Result<()> {
    let load = AppTheme::load();
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::from_load(load);
    app.attach_registry();
    app.apply_initial_vault(initial);
    let result = run_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    result
}

fn run_loop<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()>
where
    B::Error: std::error::Error + Send + Sync + 'static,
{
    while !app.should_quit {
        app.poll_index();
        app.poll_watch();
        app.poll_ai();
        app.poll_git();
        crate::toasts::prune_toasts(&mut app.toasts);
        terminal.draw(|frame| crate::draw::draw(frame, app))?;
        let wait = if app.is_busy() {
            Duration::from_millis(80)
        } else {
            Duration::from_millis(100)
        };
        if event::poll(wait)? {
            app.handle_event(event::read()?)?;
        }
    }
    Ok(())
}

impl App {
    pub fn handle_event(&mut self, evt: Event) -> Result<()> {
        crate::events::handle_event(self, evt)
    }
}
