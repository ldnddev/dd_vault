//! Desktop window: vault picker, folder tree, and the vim notes pane.

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use dd_edit::{Action, Editor, GitOp, Mode};
use dd_render::encode_png_rgba;
use dd_vault_core::{
    FileHit, GitOpResult, GitStatus, Index, Jump, JumpList, NodeKind, Paths, Registry,
    ReindexReport, Session, TreeState, Vault, VaultConfig, confined_path, create_dir, create_file,
    delete_entry, ensure_daily, external_open_target, is_external_url, launch_default_app, link_at,
    load_secret_patterns, middle_ellipsis, name_column_budget, parse_note, read_vault_text,
    rename_entry, status, today_ymd, tree_title_text, usable_credentials, validate_entry_name,
    wikilink_new_rel, write_vault_file,
};
use gpui::prelude::*;
use gpui::{
    App, Bounds, ClipboardItem, Context, Entity, FocusHandle, FontWeight, HighlightStyle, Image,
    ImageFormat, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ObjectFit, Pixels, Point, Render, ScrollEvent, ScrollHandle, SharedString, StyledText, TextRun,
    UnderlineStyle, Window, WindowBounds, WindowOptions, canvas, div, font, img, point, px,
    relative, rgb, size,
};
use gpui_platform::application;
use ldnddev_theme::{
    COLOR_FIELDS, ColorField, EditorKey, EditorOutcome, OmarchyPaths, Palette, PickerOutcome,
    PickerRow, ThemeEditor, ThemeEditorRow, ThemePicker, ThemeSaveTarget, ThemeSource,
    apply_omarchy, default_config_home, theme_editor_rows,
};

use crate::ai_card::{AiCard, AiKey, AiPoll, AiPrepare, AiSize, NoteSnap, masked_key};
use crate::chrome::{
    self, EditMode, FooterKind, ToastLevel, editor_key, extra_theme_fields, footer_line,
    help_lines, preview_fixed_px, prune_toasts, push_toast, show_ai, show_preview, show_tree,
    tree_width_px,
};
use crate::colors::{self, GuiTheme, from_palette};
use crate::commands::{
    Chord, JumpAction, Leader, NavigateTo, PaletteAction, PaletteItem, filter_palette,
    jump_to_heading, leader_step, looks_like_image, palette_action, parse_new_name,
};
use crate::editor_view::{
    Caret, EditorRow, OpenChoice, SessionWrite, choose_open, pane_name, prompt_line,
    remember_session, visible_editor_rows, wheel_rows,
};
use crate::fonts::{GuiFonts, installed_families};
use crate::git_ui::{
    busy_word, commit_then_maybe_push, conflict_notice, notes_title, run_git, sidecar_labels,
};
use crate::highlight::{highlight_line, with_background};
use crate::keys::{
    FilterKey, GlobalKey, KeyPress, PickerKey, TreeKey, filter_key, global_key, picker_key,
    tree_key,
};
use crate::paste::{image_markdown, note_stem, unique_asset_rel};
use crate::preview::{
    PreviewCache, PreviewColors, PreviewKey, PreviewRequest, RichBlock, RichRun, block_plain,
    block_tops, build_preview, embed_body, layout_blocks, load_image_bytes, move_clamped,
    yank_text,
};
use crate::scroll::{ScrollPort, index_at, offset_for_thumb, rows_viewport_px, thumb_geom};
use crate::watch::{VaultWatch, watch_rel};

const MAX_OPEN_BYTES: u64 = 2_000_000;

const FONT: f32 = 14.0;
const LINE: f32 = 20.0;
const CHROME: f32 = 28.0;
const SHEET_W: f32 = 380.0;
const BAR_W: f32 = 10.0;

pub struct Boot {
    pub theme: GuiTheme,
    pub palette: Palette,
    pub warning: Option<String>,
    pub paths: Paths,
    pub registry: Registry,
    pub initial: Option<Vault>,
}

pub fn run(boot: Boot) {
    application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("dd_vault".into()),
                    ..Default::default()
                }),
                focus: true,
                show: true,
                app_id: Some("dd_vault-gui".into()),
                ..Default::default()
            },
            move |window, cx| {
                let shell = cx.new(|cx| Shell::new(boot, cx));
                shell.update(cx, |this, cx| this.focus.focus(window, cx));
                shell
            },
        )
        .expect("open window");
        cx.activate(true);
    });
}

struct OpenVault {
    vault: Vault,
    tree: TreeState,
    editor: Editor,
    wrap: bool,
    pane_tree: bool,
    jumps: JumpList,
    index: Option<Index>,
    index_rx: Option<mpsc::Receiver<Result<ReindexReport, String>>>,
    reindex_queued: bool,
    watch: Option<VaultWatch>,
    suppress_disk: Option<PathBuf>,
    pending_disk: Option<PathBuf>,
    preview_visible: bool,
    preview_read: bool,
    preview_scroll: usize,
    preview_row: usize,
    preview_visual: Option<usize>,
    preview_epoch: u64,
    focus_preview: bool,
    git: GitStatus,
    zen: bool,
    focus_mode: bool,
    explore: bool,
    tree_split: u16,
    preview_split: u16,
}

enum Confirm {
    Quit,
    Navigate(NavigateTo),
    CreateNote {
        rel: PathBuf,
        heading: Option<String>,
    },
    ReloadDisk {
        rel: PathBuf,
    },
    ForgetVault {
        name: String,
        path: String,
    },
    Delete {
        rel: PathBuf,
    },
    AiSend {
        message: String,
    },
}

#[derive(Clone)]
enum PromptKind {
    New { parent: PathBuf },
    Rename { rel: PathBuf },
    GitCommit { then_push: bool },
    AiProvider,
    AiKey { then_enable: bool },
    AiModel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FinderKind {
    Files,
    Content,
    Tags,
    Wiki,
}

enum Overlay {
    Prompt {
        kind: PromptKind,
        draft: String,
    },
    Finder {
        kind: FinderKind,
        query: String,
        selected: usize,
        hits: Vec<FileHit>,
    },
    Palette {
        query: String,
        selected: usize,
        items: Vec<PaletteItem>,
    },
    Picker,
    Notice {
        title: String,
        message: String,
    },
}

struct Drag {
    linewise: bool,
    start_line: usize,
    start_col: usize,
    moved: bool,
}

struct Hit {
    notes: Option<Bounds<Pixels>>,
    notes_pane: Option<Bounds<Pixels>>,
    tree: Option<Bounds<Pixels>>,
    tree_pane: Option<Bounds<Pixels>>,
    preview: Option<Bounds<Pixels>>,
    preview_pane: Option<Bounds<Pixels>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SplitDrag {
    Tree,
    Preview,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BarKind {
    Tree,
    Notes,
    Preview,
}

#[derive(Clone, Copy)]
struct ScrollDrag {
    kind: BarKind,
    grab: f32,
}

struct BarPaint {
    offset: f32,
    visible: f32,
    total: f32,
    track: f32,
    hot: bool,
    kind: BarKind,
}

pub struct Shell {
    focus: FocusHandle,
    theme: GuiTheme,
    palette: Palette,
    omarchy: OmarchyPaths,
    paths: Paths,
    registry: Registry,
    open: Option<OpenVault>,
    picker: Vec<dd_vault_core::VaultEntry>,
    picker_selected: usize,
    notice: Option<String>,
    status: Option<String>,
    confirm: Option<Confirm>,
    overlay: Option<Overlay>,
    leader: Leader,
    hit: Hit,
    char_w: Pixels,
    line_h: Pixels,
    gutter_cols: usize,
    text_cols: usize,
    text_rows: usize,
    tree_rows: usize,
    wrap_width: usize,
    drag: Option<Drag>,
    split_drag: Option<SplitDrag>,
    preview_cache: Option<PreviewCache>,
    preview_px: f32,
    preview_content: f32,
    preview_view_h: f32,
    preview_tops: Vec<f32>,
    preview_heights: Vec<f32>,
    preview_vis: usize,
    scroll_drag: Option<ScrollDrag>,
    tree_port: ScrollPort,
    notes_port: ScrollPort,
    preview_port: ScrollPort,
    reading_follow: bool,
    git_rx: Option<mpsc::Receiver<Result<(String, GitOpResult), String>>>,
    busy_kind: Option<&'static str>,
    ai: AiCard,
    toasts: Vec<chrome::Toast>,
    toast_seen: Option<String>,
    help: bool,
    help_scroll: usize,
    help_view: usize,
    theme_picker: Option<ThemePicker>,
    theme_editor: Option<ThemeEditor>,
    mono: String,
    sans: String,
    font_saved: (String, String),
    font_snapshot: (String, String),
    families: Vec<String>,
    family_selected: usize,
    family_scroll: usize,
    font_filter: Option<String>,
    appear: AppearSection,
    measured_mono: String,
    body_cols: u16,
    tree_px: f32,
}

#[derive(Clone)]
struct TreeRow {
    label: String,
    selected: bool,
    color: u32,
}

struct NotesView {
    title: String,
    rows: Vec<EditorRow>,
    prompt: Option<String>,
    caret_bar: Option<Point<Pixels>>,
    focused: bool,
    bar: Option<BarPaint>,
    port: ScrollHandle,
    content_h: f32,
    origin: usize,
}

struct TreeView {
    title: String,
    rows: Vec<TreeRow>,
    filter: Option<String>,
    focused: bool,
    width: f32,
    bar: Option<BarPaint>,
    port: ScrollHandle,
    content_h: f32,
    origin: usize,
}

struct VaultPanes {
    tree: Option<TreeView>,
    notes: Option<NotesView>,
    preview: Option<PreviewView>,
    ai: Option<AiPaint>,
}

enum Body {
    Picker,
    Vault(Box<VaultPanes>),
}

struct PreviewView {
    title: String,
    focused: bool,
    blocks: Vec<RichBlock>,
    scroll_px: f32,
    view_h: f32,
    fixed_h: Option<f32>,
    cursor: Option<usize>,
    selected: Option<(usize, usize)>,
    message: Option<String>,
    bar: Option<BarPaint>,
    port: ScrollHandle,
    content_h: f32,
    mono: String,
    sans: String,
}

#[derive(Clone)]
struct AiPaint {
    title: String,
    lines: Vec<String>,
    draft: Option<String>,
    wide: bool,
}

struct ToastPaint {
    message: String,
    color: u32,
}

struct Frame {
    header_right: String,
    busy: Option<&'static str>,
    body: Body,
    footer: String,
    confirm: Option<String>,
    overlay: Option<OverlayView>,
    ai_float: Option<AiPaint>,
    help: Option<Vec<String>>,
    theme_panel: Option<AppearanceView>,
    toasts: Vec<ToastPaint>,
    live_pointer: bool,
    split_cursor: Option<SplitDrag>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppearSection {
    Theme,
    Mono,
    Sans,
}

#[derive(Clone, Copy)]
enum AppearAction {
    None,
    Theme(usize),
    Color(usize),
    Family(usize),
    Focus(AppearSection),
}

#[derive(Clone)]
struct AppearLine {
    text: String,
    selected: bool,
    action: AppearAction,
}

#[derive(Clone)]
struct AppearanceView {
    title: String,
    hint: String,
    lines: Vec<AppearLine>,
    editor_font: String,
    preview_font: String,
    editor_on: bool,
    preview_on: bool,
}

#[derive(Clone)]
struct OverlayLine {
    text: String,
    selected: bool,
}

#[derive(Clone)]
struct OverlayView {
    title: String,
    query: String,
    lines: Vec<OverlayLine>,
    warn: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Effect {
    None,
    Quit,
}

impl Shell {
    fn new(boot: Boot, cx: &mut Context<Self>) -> Self {
        let (open, notice, status) = attach_boot(&boot);
        let picker = boot.registry.vaults.clone();
        let ai = AiCard::load(&boot.paths);
        let fonts = GuiFonts::load(&boot.paths.ldnddev_dir().join("config.toml"));
        let font_pair = (fonts.mono.clone(), fonts.sans.clone());
        let mut toasts = Vec::new();
        if let Some(warning) = boot.warning.clone() {
            push_toast(&mut toasts, ToastLevel::Warning, warning);
        }
        let mut shell = Self {
            focus: cx.focus_handle(),
            theme: boot.theme,
            palette: boot.palette,
            omarchy: OmarchyPaths::system(),
            paths: boot.paths,
            registry: boot.registry,
            open,
            picker,
            picker_selected: 0,
            notice,
            status,
            confirm: None,
            overlay: None,
            leader: Leader::None,
            hit: Hit {
                notes: None,
                notes_pane: None,
                tree: None,
                tree_pane: None,
                preview: None,
                preview_pane: None,
            },
            char_w: px(0.),
            line_h: px(LINE),
            gutter_cols: 4,
            text_cols: 40,
            text_rows: 20,
            tree_rows: 20,
            wrap_width: 40,
            drag: None,
            split_drag: None,
            preview_cache: None,
            preview_px: 0.0,
            preview_content: 0.0,
            preview_view_h: LINE,
            preview_tops: Vec::new(),
            preview_heights: Vec::new(),
            preview_vis: 1,
            scroll_drag: None,
            tree_port: ScrollPort::new(),
            notes_port: ScrollPort::new(),
            preview_port: ScrollPort::new(),
            reading_follow: false,
            git_rx: None,
            busy_kind: None,
            ai,
            toasts,
            toast_seen: None,
            help: false,
            help_scroll: 0,
            help_view: 24,
            theme_picker: None,
            theme_editor: None,
            mono: fonts.mono,
            sans: fonts.sans,
            font_saved: font_pair.clone(),
            font_snapshot: font_pair,
            families: Vec::new(),
            family_selected: 0,
            family_scroll: 0,
            font_filter: None,
            appear: AppearSection::Theme,
            measured_mono: String::new(),
            body_cols: 120,
            tree_px: 0.,
        };
        if shell.open.as_ref().is_some_and(|open| open.zen) {
            shell.ai.collapse();
        }
        shell.queue_reindex();
        shell.start_poll(cx);
        shell
    }

    fn start_poll(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let ok = this.update(cx, |shell, cx| {
                    if shell.poll_services() {
                        cx.notify();
                    }
                });
                if ok.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn frame(&mut self, window: &mut Window) -> Frame {
        self.ensure_char_width(window);
        self.sync_status_toast();
        self.update_body_cols(window);
        let header_right = self
            .open
            .as_ref()
            .map(|open| format!("vault:{}", open.vault.name))
            .unwrap_or_default();
        let busy = self.busy_label();
        let reading = self.open.as_ref().is_some_and(|open| open.preview_read);
        let zen = self.open.as_ref().is_some_and(|open| open.zen);
        let focus_mode = self.open.as_ref().is_some_and(|open| open.focus_mode);
        let explore = self.open.as_ref().is_some_and(|open| open.explore);
        let show_tree_pane =
            self.open.is_some() && show_tree(zen, focus_mode, explore, reading, self.body_cols);
        let preview_on = self.open.as_ref().is_some_and(|open| open.preview_visible);
        let ai_here = self.open.is_some() && show_ai(zen, reading);
        let ai_full = ai_here && self.ai.size == AiSize::Full;
        let show_preview_pane =
            self.open.is_some() && !ai_full && show_preview(zen, reading, preview_on);
        let ai_paint = if ai_here { Some(self.ai_paint()) } else { None };
        let ai_float = if ai_full { None } else { ai_paint.clone() };
        let ai_pane = if ai_full { ai_paint } else { None };
        let split = self.open.as_ref().map(|open| open.tree_split).unwrap_or(0);
        let char_w = (self.char_w / px(1.)).max(1.0);
        let body_px = window.viewport_size().width / px(1.);
        self.tree_px = if show_tree_pane {
            tree_width_px(split, char_w, body_px)
        } else {
            0.0
        };
        let confirm = self.confirm_message();
        let overlay = self.overlay_view();
        let body = if self.open.is_some() {
            if !show_tree_pane {
                self.hit.tree = None;
                self.hit.tree_pane = None;
            }
            if reading {
                self.hit.tree = None;
                self.hit.tree_pane = None;
                self.hit.notes = None;
                self.hit.notes_pane = None;
            }
            if !show_preview_pane {
                self.hit.preview = None;
                self.hit.preview_pane = None;
                self.preview_tops.clear();
                self.preview_heights.clear();
            }
            let tree = if show_tree_pane {
                Some(self.tree_view(window))
            } else {
                None
            };
            let notes = if reading {
                None
            } else {
                Some(self.notes_view(window))
            };
            let preview = if show_preview_pane {
                Some(self.preview_view(window))
            } else {
                None
            };
            Body::Vault(Box::new(VaultPanes {
                tree,
                notes,
                preview,
                ai: ai_pane,
            }))
        } else {
            self.hit.notes = None;
            self.hit.notes_pane = None;
            self.hit.tree = None;
            self.hit.tree_pane = None;
            self.hit.preview = None;
            self.hit.preview_pane = None;
            Body::Picker
        };
        let footer = self.footer_text();
        let help = if self.help {
            Some(self.help_page())
        } else {
            None
        };
        if self.theme_open() && self.appear != AppearSection::Theme {
            self.reveal_family();
        }
        let theme_panel = if self.help {
            None
        } else if self.theme_open() {
            Some(self.appearance_view())
        } else {
            None
        };
        let toasts = self
            .toasts
            .iter()
            .map(|toast| ToastPaint {
                message: toast.message.clone(),
                color: toast_color(&self.theme, toast.level),
            })
            .collect();
        Frame {
            header_right,
            busy,
            body,
            footer,
            confirm,
            overlay,
            ai_float,
            help,
            theme_panel,
            toasts,
            live_pointer: self.open.is_some() && !self.modal_up(),
            split_cursor: self.split_drag,
        }
    }

    fn sync_status_toast(&mut self) {
        match &self.status {
            Some(message) if self.toast_seen.as_deref() != Some(message.as_str()) => {
                let message = message.clone();
                self.toast_seen = Some(message.clone());
                push_toast(&mut self.toasts, ToastLevel::Info, message);
            }
            None => self.toast_seen = None,
            _ => {}
        }
    }

    fn update_body_cols(&mut self, window: &Window) {
        let width = window.viewport_size().width / px(1.);
        let char_w = (self.char_w / px(1.)).max(1.0);
        self.body_cols = (width / char_w).floor().clamp(1.0, 500.0) as u16;
        let height = window.viewport_size().height / px(1.);
        let body = (height - CHROME * 2.0).max(LINE);
        self.help_view = (body / LINE).floor().max(1.0) as usize;
    }

    fn ensure_char_width(&mut self, window: &mut Window) {
        if self.char_w > px(0.) && self.measured_mono == self.mono {
            return;
        }
        let text: SharedString = "M".into();
        let run = TextRun {
            len: text.len(),
            font: font(self.mono.clone()),
            color: rgb(self.theme.text_primary).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_line(text, px(FONT), &[run], None);
        let width = shaped.width();
        self.char_w = if width > px(0.) { width } else { px(8.) };
        self.measured_mono = self.mono.clone();
    }

    fn tree_view(&mut self, window: &Window) -> TreeView {
        let theme = self.theme.clone();
        let filtering = self.open.as_ref().is_some_and(|open| open.tree.filtering);
        let view_px = self.pane_view_px(BarKind::Tree, window);
        let rows_fit = self.rows_from_px(view_px);
        self.tree_rows = rows_fit;
        self.adopt_tree_scroll();
        let char_w = self.char_w;
        let tree_px = self.tree_px;
        let open = self.open.as_mut().expect("vault");
        let visible = open.tree.visible();
        let max_scroll = visible.len().saturating_sub(rows_fit.max(1));
        if open.tree.scroll > max_scroll {
            open.tree.scroll = max_scroll;
        }
        let cols = ((tree_px / (char_w / px(1.)).max(1.0)).floor() as usize).clamp(4, 200);
        let budget = name_column_budget(cols as u16, rows_fit as u16, visible.len()).max(1);
        let label = if open.tree.cwd.as_os_str().is_empty() {
            open.vault.name.clone()
        } else {
            format!("/{}", open.tree.cwd.display())
        };
        let clipped = visible.get(open.tree.selected).and_then(|row| {
            if row.name == ".." || row.name.chars().count() <= budget {
                None
            } else {
                Some(row.rel.display().to_string())
            }
        });
        let title = tree_title_text(
            &label,
            &open.tree.filter,
            clipped.as_deref(),
            Some(visible.len()),
            cols,
        );
        let selected = open.tree.selected;
        let start = open.tree.scroll;
        let rows = visible
            .iter()
            .enumerate()
            .skip(start)
            .take(rows_fit.max(1))
            .map(|(i, row)| {
                let name = middle_ellipsis(&row.name, budget);
                let color = if i == selected {
                    theme.text_active_focus
                } else {
                    match row.kind {
                        NodeKind::Dir => theme.folders,
                        NodeKind::File => theme.files,
                        NodeKind::Symlink => theme.links,
                    }
                };
                TreeRow {
                    label: format!("{}{name}", row.prefix),
                    selected: i == selected,
                    color,
                }
            })
            .collect();
        let filter = if filtering {
            Some(format!("/{}", open.tree.filter))
        } else {
            None
        };
        let total = visible.len() as f32;
        let offset = open.tree.scroll as f32;
        let focused = open.pane_tree;
        let origin = open.tree.scroll;
        let port = self.tree_port.handle.clone();
        self.tree_port.push_px(offset * LINE);
        TreeView {
            title,
            rows,
            filter,
            focused,
            width: tree_px,
            bar: self.bar_paint(
                BarKind::Tree,
                offset,
                rows_fit.max(1) as f32,
                total,
                view_px,
            ),
            port,
            content_h: (total * LINE).max(1.0),
            origin,
        }
    }

    fn notes_view(&mut self, window: &Window) -> NotesView {
        let prompt = self
            .open
            .as_ref()
            .and_then(|open| prompt_line(&open.editor));
        self.gutter_cols = self
            .open
            .as_ref()
            .map(|open| open.editor.gutter_cols() as usize)
            .unwrap_or(4);
        let view_px = self.pane_view_px(BarKind::Notes, window);
        let rows_fit = self.rows_from_px(view_px);
        self.text_rows = rows_fit;
        let char_w = self.char_w;
        let notes_w = self
            .hit
            .notes
            .map(|bounds| bounds.size.width)
            .unwrap_or_else(|| (window.viewport_size().width - px(self.tree_px)).max(char_w * 8.));
        let gutter_px = char_w * self.gutter_cols as f32;
        let text_px = (notes_w - gutter_px - char_w - px(BAR_W)).max(char_w);
        self.text_cols = (text_px / char_w).floor().max(1.0) as usize;
        let wrap = self.open.as_ref().is_some_and(|open| open.wrap);
        self.wrap_width = if wrap { self.text_cols } else { usize::MAX };
        self.adopt_notes_scroll();
        let text_rows = self.text_rows;
        let wrap_width = self.wrap_width;
        let text_cols = self.text_cols;
        let open = self.open.as_mut().expect("vault");
        if open.editor.follow_caret {
            open.editor.ensure_scroll_wrapped(text_rows, wrap_width);
        } else {
            open.editor.clamp_scroll_wrapped(text_rows, wrap_width);
        }
        let focused = !open.pane_tree && !open.focus_preview;
        let rows = visible_editor_rows(&open.editor, wrap_width, text_cols, text_rows);
        let mut caret_bar = None;
        for (i, row) in rows.iter().enumerate() {
            if let Some(Caret::Bar(col)) = row.caret {
                caret_bar = Some(point(gutter_px + char_w * col as f32, px(LINE) * i as f32));
            }
        }
        let name = open.editor.rel.as_deref().unwrap_or("(no note)");
        let title = notes_title(
            name,
            open.editor.mode.label(),
            open.editor.dirty,
            open.git.state,
        );
        let total = open.editor.visual_len(wrap_width) as f32;
        let offset = open.editor.visual_origin(wrap_width) as f32;
        let origin = offset as usize;
        let port = self.notes_port.handle.clone();
        self.notes_port.push_px(offset * LINE);
        NotesView {
            title,
            rows,
            prompt,
            caret_bar,
            focused,
            bar: self.bar_paint(
                BarKind::Notes,
                offset,
                text_rows.max(1) as f32,
                total,
                view_px,
            ),
            port,
            content_h: (total * LINE).max(1.0),
            origin,
        }
    }

    fn preview_view(&mut self, window: &Window) -> PreviewView {
        let reading = self.open.as_ref().is_some_and(|open| open.preview_read);
        let focused = self.open.as_ref().is_some_and(|open| open.focus_preview);
        let title = if reading {
            " reading ".to_string()
        } else {
            " preview ".to_string()
        };
        let fixed_h = self.preview_fixed_h(window);
        let height = self.pane_view_px(BarKind::Preview, window);
        let has_note = self
            .open
            .as_ref()
            .is_some_and(|open| open.editor.rel.is_some());
        if !has_note {
            self.preview_tops.clear();
            self.preview_heights.clear();
            self.preview_content = 0.0;
            self.preview_view_h = height;
            self.preview_px = 0.0;
            self.preview_vis = 1;
            self.preview_port.push_px(0.0);
            return PreviewView {
                title,
                focused,
                blocks: Vec::new(),
                scroll_px: 0.0,
                view_h: height,
                fixed_h,
                cursor: None,
                selected: None,
                message: Some("open a note to preview".into()),
                bar: None,
                port: self.preview_port.handle.clone(),
                content_h: 1.0,
                mono: self.mono.clone(),
                sans: self.sans.clone(),
            };
        }
        let cols = self.preview_cols(window);
        let fit = (height / LINE).floor().max(1.0) as u16;
        let (image_rows, diagram_rows) = if reading {
            (fit.max(1), fit.max(1))
        } else {
            (fit.clamp(1, 16), fit.saturating_mul(2).clamp(16, 48))
        };
        self.refresh_preview(cols, image_rows, diagram_rows);
        let width = self.preview_width_px(window);
        let max_image = if reading {
            height.max(LINE)
        } else {
            height.clamp(LINE, LINE * 16.0)
        };
        let blocks = self
            .preview_cache
            .as_ref()
            .map(|cache| layout_blocks(&cache.doc, width, max_image))
            .unwrap_or_default();
        let (tops, heights, content) = block_tops(&blocks);
        self.preview_tops = tops;
        self.preview_heights = heights;
        self.preview_content = content;
        self.preview_view_h = height;
        let count = blocks.len().max(1);
        if let Some(open) = &mut self.open
            && open.preview_row >= count
        {
            open.preview_row = count - 1;
        }
        self.adopt_preview_scroll();
        if reading && self.reading_follow {
            self.reveal_reading_px();
        }
        let max_px = (content - height).max(0.0);
        if self.preview_px > max_px {
            self.preview_px = max_px;
        }
        if self.preview_px < 0.0 {
            self.preview_px = 0.0;
        }
        let top_index = index_at(&self.preview_tops, &self.preview_heights, self.preview_px);
        if let Some(open) = &mut self.open
            && !reading
        {
            open.preview_scroll = top_index;
        }
        let focused = self.open.as_ref().is_some_and(|open| open.focus_preview);
        let cursor = self.open.as_ref().map(|open| open.preview_row).unwrap_or(0);
        let visual = self.open.as_ref().and_then(|open| open.preview_visual);
        self.preview_vis = blocks_fitting(&self.preview_heights, cursor, height).max(1);
        let selected = if reading {
            let row = cursor.min(count - 1);
            match visual {
                Some(anchor) => {
                    let anchor = anchor.min(count - 1);
                    Some((anchor.min(row), anchor.max(row)))
                }
                None => None,
            }
        } else {
            None
        };
        let cursor = if reading {
            Some(cursor.min(count - 1))
        } else {
            None
        };
        let bar = self.bar_paint(BarKind::Preview, self.preview_px, height, content, height);
        self.preview_port.push_px(self.preview_px);
        PreviewView {
            title,
            focused,
            blocks,
            scroll_px: self.preview_px,
            view_h: height,
            fixed_h,
            cursor,
            selected,
            message: None,
            bar,
            port: self.preview_port.handle.clone(),
            content_h: content.max(1.0),
            mono: self.mono.clone(),
            sans: self.sans.clone(),
        }
    }

    fn reveal_reading_px(&mut self) {
        let row = self.open.as_ref().map(|open| open.preview_row).unwrap_or(0);
        let top = self.preview_tops.get(row).copied().unwrap_or(0.0);
        let h = self.preview_heights.get(row).copied().unwrap_or(LINE);
        let view = self.preview_view_h.max(1.0);
        if top < self.preview_px {
            self.preview_px = top;
        } else if top + h > self.preview_px + view {
            self.preview_px = (top + h - view).max(0.0);
        }
    }

    fn bar_paint(
        &self,
        kind: BarKind,
        offset: f32,
        visible: f32,
        total: f32,
        track: f32,
    ) -> Option<BarPaint> {
        let track = track.max(1.0);
        thumb_geom(offset, visible, total, track)?;
        let hot = self.scroll_drag.is_some_and(|drag| drag.kind == kind);
        Some(BarPaint {
            offset,
            visible,
            total,
            track,
            hot,
            kind,
        })
    }

    fn pane_view_px(&self, kind: BarKind, window: &Window) -> f32 {
        let filtering = self.open.as_ref().is_some_and(|open| open.tree.filtering);
        let prompt = self
            .open
            .as_ref()
            .and_then(|open| prompt_line(&open.editor));
        let (port, rows, pane, chrome, extra) = match kind {
            BarKind::Tree => (
                self.tree_port.view_px(),
                self.hit.tree,
                self.hit.tree_pane,
                CHROME + if filtering { LINE } else { 0.0 },
                filtering,
            ),
            BarKind::Notes => (
                self.notes_port.view_px(),
                self.hit.notes,
                self.hit.notes_pane,
                CHROME + if prompt.is_some() { LINE } else { 0.0 },
                prompt.is_some(),
            ),
            BarKind::Preview => (
                self.preview_port.view_px(),
                self.hit.preview,
                self.hit.preview_pane,
                CHROME,
                false,
            ),
        };
        let fallback = if kind == BarKind::Preview {
            self.preview_fallback_px(window)
        } else {
            self.fallback_body_px(window, extra)
        };
        rows_viewport_px(
            port,
            rows.map(|bounds| bounds.size.height / px(1.)),
            pane.map(|bounds| bounds.size.height / px(1.)),
            chrome,
            fallback,
        )
    }

    fn fallback_body_px(&self, window: &Window, extra: bool) -> f32 {
        let extra_h = if extra { LINE } else { 0.0 };
        (window.viewport_size().height / px(1.) - CHROME * 3.0 - extra_h).max(LINE)
    }

    fn preview_fallback_px(&self, window: &Window) -> f32 {
        let view = window.viewport_size().height / px(1.);
        let body = (view - CHROME * 2.0).max(LINE * 8.0);
        if self.open.as_ref().is_some_and(|open| open.preview_read) {
            (body - CHROME).max(LINE)
        } else {
            (body * 0.4).max(LINE * 6.0)
        }
    }

    fn preview_fixed_h(&self, window: &Window) -> Option<f32> {
        if self.open.as_ref().is_some_and(|open| open.preview_read) {
            return None;
        }
        let split = self
            .open
            .as_ref()
            .map(|open| open.preview_split)
            .unwrap_or(0);
        if split == 0 {
            return None;
        }
        let view = window.viewport_size().height / px(1.);
        let body = (view - CHROME * 2.0).max(LINE * 8.0);
        Some(preview_fixed_px(split, LINE, body))
    }

    fn rows_from_px(&self, height: f32) -> usize {
        let line = (self.line_h / px(1.)).max(1.0);
        (height / line).floor().max(1.0) as usize
    }

    fn preview_width_px(&self, window: &Window) -> f32 {
        let raw = if let Some(bounds) = self.hit.preview {
            bounds.size.width / px(1.)
        } else {
            let full = window.viewport_size().width / px(1.);
            if self.open.as_ref().is_some_and(|open| open.preview_read) {
                full
            } else {
                (full - self.tree_px).max(LINE * 8.0)
            }
        };
        (raw - BAR_W).max(LINE)
    }

    fn preview_cols(&self, window: &Window) -> usize {
        let char_w = (self.char_w / px(1.)).max(1.0);
        let width = (self.preview_width_px(window) - 16.0).max(char_w);
        (width / char_w).floor().max(8.0) as usize
    }

    fn refresh_preview(&mut self, cols: usize, image_rows: u16, diagram_rows: u16) {
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let Some(rel) = open.editor.rel.clone() else {
            return;
        };
        let src = open.editor.text();
        let epoch = open.preview_epoch;
        let colors = PreviewColors::from_theme(&self.theme);
        let palette = colors.key();
        let rel_key = Some(rel.clone());
        let same_note = self
            .preview_cache
            .as_ref()
            .is_some_and(|cache| cache.rel.as_deref() == Some(rel.as_str()));
        if self.preview_cache.as_ref().is_some_and(|cache| {
            cache.matches(&PreviewKey {
                rel: &rel_key,
                src: &src,
                cols,
                image_rows,
                diagram_rows,
                palette,
                epoch,
            })
        }) {
            return;
        }
        let root = open.vault.root.clone();
        let note = PathBuf::from(&rel);
        let doc = build_preview(&PreviewRequest {
            src: &src,
            cols,
            colors: &colors,
            image_rows,
            diagram_rows,
            embed: Some(&|target| embed_body(&root, target, &|name| open.tree.find_file(name))),
            read_image: Some(&|url| {
                load_image_bytes(&root, Some(note.as_path()), url, &|name| {
                    open.tree.find_file(name)
                })
            }),
        });
        self.preview_cache = Some(PreviewCache {
            rel: rel_key,
            src,
            cols,
            image_rows,
            diagram_rows,
            palette,
            epoch,
            doc,
        });
        if !same_note {
            self.preview_px = 0.0;
        }
    }

    fn adopt_tree_scroll(&mut self) {
        if self.modal_up() {
            return;
        }
        let Some(y) = self.tree_port.pull_px() else {
            return;
        };
        let next = (y / LINE).round().max(0.0);
        let current = self.open.as_ref().map(|open| open.tree.scroll).unwrap_or(0) as f32;
        if (next - current).abs() < 0.5 {
            return;
        }
        self.set_tree_offset(next);
    }

    fn adopt_notes_scroll(&mut self) {
        if self.modal_up() {
            return;
        }
        let Some(y) = self.notes_port.pull_px() else {
            return;
        };
        let next = (y / LINE).round().max(0.0);
        let current = self
            .open
            .as_ref()
            .map(|open| open.editor.visual_origin(self.wrap_width))
            .unwrap_or(0) as f32;
        if (next - current).abs() < 0.5 {
            return;
        }
        self.set_notes_offset(next);
    }

    fn adopt_preview_scroll(&mut self) {
        if self.modal_up() {
            return;
        }
        let Some(y) = self.preview_port.pull_px() else {
            return;
        };
        self.set_preview_px(y);
    }

    fn footer_text(&self) -> String {
        if self.confirm.is_some() {
            return match self.confirm {
                Some(Confirm::Delete { .. }) => "y delete    n keep    Esc cancel".into(),
                Some(Confirm::ReloadDisk { .. }) => "y reload    n keep    Esc cancel".into(),
                Some(Confirm::ForgetVault { .. }) => "y forget    n keep    Esc cancel".into(),
                Some(Confirm::CreateNote { .. }) => "y create    n cancel    Esc cancel".into(),
                Some(Confirm::AiSend { .. }) => "y send    n cancel    Esc cancel".into(),
                _ => "y discard    n keep    Esc cancel".into(),
            };
        }
        if let Some(hint) = self.overlay_hint() {
            return hint.into();
        }
        footer_line(self.body_cols, self.footer_kind())
    }

    fn footer_kind(&self) -> FooterKind<'_> {
        if self.help || self.theme_open() {
            return FooterKind::Help;
        }
        if self.open.as_ref().is_some_and(|open| open.tree.filtering) {
            return FooterKind::Filter;
        }
        if let Some((middle, cancel)) = leader_footer(self.leader) {
            return FooterKind::Leader(middle, cancel);
        }
        if self.ai_keys_here() {
            return FooterKind::Ai {
                streaming: self.ai.streaming(),
            };
        }
        let Some(open) = &self.open else {
            return FooterKind::NoVault;
        };
        if open.preview_read && open.focus_preview {
            return FooterKind::Reading {
                visual: open.preview_visual.is_some(),
            };
        }
        if open.focus_preview {
            return FooterKind::Preview;
        }
        if open.pane_tree {
            return FooterKind::Tree;
        }
        let mode = match open.editor.mode {
            Mode::Insert => EditMode::Insert,
            Mode::Visual => EditMode::Visual,
            Mode::Command => EditMode::Command,
            Mode::Search => EditMode::Search,
            Mode::Normal => EditMode::Normal,
        };
        FooterKind::Editor(mode)
    }

    fn overlay_hint(&self) -> Option<&'static str> {
        match &self.overlay {
            Some(Overlay::Prompt { kind, .. }) => Some(prompt_hint(kind)),
            Some(Overlay::Finder { .. }) => Some("type to filter    Enter open    Esc cancel"),
            Some(Overlay::Palette { .. }) => Some("type to filter    Enter run    Esc cancel"),
            Some(Overlay::Picker) => Some("j/k move    Enter open    d forget    Esc close"),
            Some(Overlay::Notice { .. }) => Some("Esc close"),
            None => None,
        }
    }

    fn confirm_message(&self) -> Option<String> {
        match self.confirm.as_ref() {
            Some(Confirm::Quit) => Some("Discard unsaved changes and quit?".into()),
            Some(Confirm::Navigate(to)) => Some(format!(
                "Discard unsaved changes and open {}?",
                to.rel.display()
            )),
            Some(Confirm::CreateNote { rel, .. }) => {
                Some(format!("Create {} and open it?", rel.display()))
            }
            Some(Confirm::ReloadDisk { rel }) => Some(format!(
                "{} changed on disk. Reload and discard unsaved edits?",
                rel.display()
            )),
            Some(Confirm::ForgetVault { name, path }) => Some(format!(
                "Remove vault '{name}' from the list? The folder on disk is not deleted. {path}"
            )),
            Some(Confirm::Delete { rel }) => {
                let is_dir = self.open.as_ref().is_some_and(|open| {
                    confined_path(&open.vault.root, rel).is_ok_and(|path| path.is_dir())
                });
                let kind = if is_dir { "folder" } else { "file" };
                Some(format!("Delete {kind} {}?", rel.display()))
            }
            Some(Confirm::AiSend { message }) => Some(message.clone()),
            None => None,
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let press = KeyPress {
            key: &event.keystroke.key,
            key_char: event.keystroke.key_char.as_deref(),
            control: event.keystroke.modifiers.control,
            shift: event.keystroke.modifiers.shift,
        };
        if self.handle_press(press, cx) == Effect::Quit {
            cx.quit();
            return;
        }
        cx.notify();
    }

    fn handle_press(&mut self, press: KeyPress<'_>, cx: &mut Context<Self>) -> Effect {
        if self.help {
            return self.handle_help_key(press);
        }
        if self.theme_open() {
            return self.handle_theme_key(press);
        }
        if is_fn(press, 1) {
            self.open_help();
            return Effect::None;
        }
        if is_fn(press, 2) && self.confirm.is_none() && self.overlay.is_none() {
            self.open_theme();
            return Effect::None;
        }
        if self.confirm.is_some() {
            return self.handle_confirm(press);
        }
        if self.overlay.is_some() {
            self.status = None;
            return self.handle_overlay(press);
        }
        self.status = None;
        match global_key(press) {
            GlobalKey::Quit => {
                self.leader = Leader::None;
                return self.request_quit(false);
            }
            GlobalKey::Save => {
                self.save();
                return Effect::None;
            }
            GlobalKey::Edit(_) | GlobalKey::None => {}
        }
        if self.open.is_none() {
            return self.handle_picker(press);
        }
        if press.control && eq_key(press.key, "v") && self.try_paste_image() {
            return Effect::None;
        }
        if press.control && matches!(press.key, "enter" | "return") {
            self.leader = Leader::None;
            self.open_external_current();
            return Effect::None;
        }
        let typing = self.open.as_ref().is_some_and(|open| {
            !open.pane_tree
                && !open.focus_preview
                && matches!(
                    open.editor.mode,
                    Mode::Insert | Mode::Command | Mode::Search
                )
        });
        if typing {
            self.leader = Leader::None;
            if let GlobalKey::Edit(key) = global_key(press) {
                return self.edit(key, cx);
            }
            return Effect::None;
        }
        if press.control && eq_key(press.key, "o") {
            self.leader = Leader::None;
            self.jump_back();
            return Effect::None;
        }
        if press.control && eq_key(press.key, "i") {
            self.leader = Leader::None;
            self.jump_forward();
            return Effect::None;
        }
        if self.open.as_ref().is_some_and(|open| open.tree.filtering) {
            return self.handle_filter(press);
        }
        if self.leader != Leader::None {
            let (leader, chord) = leader_step(self.leader, press.key, press.key_char);
            self.leader = leader;
            if let Some(chord) = chord {
                self.run_chord(chord);
            }
            return Effect::None;
        }
        let ai_keys = self.ai_keys_here();
        let space = !press.control && (press.key == "space" || press.key_char == Some(" "));
        if space && !(ai_keys && !self.ai.draft.is_empty()) {
            self.leader = Leader::Space;
            return Effect::None;
        }
        if ai_keys {
            self.handle_ai_key(press);
            return Effect::None;
        }
        if matches!(global_key(press), GlobalKey::Edit(dd_edit::Key::Tab)) {
            self.cycle_pane();
            self.persist();
            return Effect::None;
        }
        if self.open.as_ref().is_some_and(|open| open.focus_preview) {
            self.handle_preview_key(press, cx);
            return Effect::None;
        }
        let on_tree = self.open.as_ref().is_some_and(|open| open.pane_tree);
        if on_tree {
            self.handle_tree(press)
        } else if let GlobalKey::Edit(key) = global_key(press) {
            self.edit(key, cx)
        } else {
            Effect::None
        }
    }

    fn handle_confirm(&mut self, press: KeyPress<'_>) -> Effect {
        let accept = matches!(press.key, "y" | "Y" | "enter" | "return")
            || matches!(press.key_char, Some("y" | "Y"));
        let cancel =
            matches!(press.key, "n" | "N" | "escape") || matches!(press.key_char, Some("n" | "N"));
        if !accept && !cancel {
            return Effect::None;
        }
        let confirm = self.confirm.take();
        if cancel {
            match confirm {
                Some(Confirm::ReloadDisk { rel }) => {
                    if let Some(open) = &mut self.open {
                        open.suppress_disk = Some(rel);
                    }
                }
                Some(Confirm::AiSend { .. }) => self.ai.cancel_consent(),
                _ => {}
            }
            self.flush_disk_prompt();
            return Effect::None;
        }
        let effect = match confirm {
            Some(Confirm::Quit) => Effect::Quit,
            Some(Confirm::Navigate(to)) => {
                if let Some(open) = &mut self.open {
                    open.editor.dirty = false;
                }
                self.navigate_clean(to);
                Effect::None
            }
            Some(Confirm::CreateNote { rel, heading }) => {
                self.create_and_open_note(rel, heading);
                Effect::None
            }
            Some(Confirm::ReloadDisk { rel }) => {
                self.accept_reload(rel);
                Effect::None
            }
            Some(Confirm::ForgetVault { name, path }) => {
                self.forget_vault(&path, &name);
                Effect::None
            }
            Some(Confirm::Delete { rel }) => {
                self.delete_rel(rel);
                Effect::None
            }
            Some(Confirm::AiSend { .. }) => {
                self.start_pending_ai();
                Effect::None
            }
            None => Effect::None,
        };
        self.flush_disk_prompt();
        effect
    }

    fn handle_picker(&mut self, press: KeyPress<'_>) -> Effect {
        let n = self.picker.len() as i32;
        match picker_key(press) {
            PickerKey::Quit => Effect::Quit,
            PickerKey::Ignore => Effect::None,
            PickerKey::Home => {
                self.picker_selected = 0;
                Effect::None
            }
            PickerKey::End => {
                self.picker_selected = self.picker.len().saturating_sub(1);
                Effect::None
            }
            PickerKey::Move(delta) => {
                if n > 0 {
                    let next = self.picker_selected as i32 + delta;
                    self.picker_selected = next.clamp(0, n - 1) as usize;
                }
                Effect::None
            }
            PickerKey::Open => {
                self.open_picker_selection();
                Effect::None
            }
            PickerKey::Forget => {
                self.begin_forget();
                Effect::None
            }
        }
    }

    fn handle_filter(&mut self, press: KeyPress<'_>) -> Effect {
        let Some(open) = &mut self.open else {
            return Effect::None;
        };
        match filter_key(press) {
            FilterKey::Cancel => open.tree.cancel_filter(),
            FilterKey::Stop => open.tree.filtering = false,
            FilterKey::Backspace => {
                open.tree.filter.pop();
                open.tree.selected = 0;
            }
            FilterKey::Type(ch) => {
                open.tree.filter.push(ch);
                open.tree.selected = 0;
            }
            FilterKey::Ignore => {}
        }
        Effect::None
    }

    fn handle_tree(&mut self, press: KeyPress<'_>) -> Effect {
        let rows = self.tree_rows.max(1);
        let Some(open) = &mut self.open else {
            return Effect::None;
        };
        match tree_key(press) {
            TreeKey::Move(delta) => {
                open.tree.move_by(delta);
                open.tree.ensure_visible(rows);
            }
            TreeKey::Home => {
                open.tree.jump_home();
                open.tree.ensure_visible(rows);
            }
            TreeKey::End => {
                open.tree.jump_end();
                open.tree.ensure_visible(rows);
            }
            TreeKey::Leave => open.tree.leave(),
            TreeKey::EnterDir => open.tree.enter(),
            TreeKey::Activate => return self.activate_tree(),
            TreeKey::Filter => open.tree.begin_filter(),
            TreeKey::New => return self.begin_new(),
            TreeKey::Rename => return self.begin_rename(),
            TreeKey::Delete => return self.begin_delete(),
            TreeKey::Ignore => {}
        }
        Effect::None
    }

    fn activate_tree(&mut self) -> Effect {
        let Some(row) = self.open.as_ref().and_then(|open| open.tree.selected_row()) else {
            return Effect::None;
        };
        if row.name == ".." {
            if let Some(open) = &mut self.open {
                open.tree.leave();
            }
            return Effect::None;
        }
        match row.kind {
            NodeKind::Dir => {
                if let Some(open) = &mut self.open {
                    open.tree.enter();
                }
            }
            NodeKind::File | NodeKind::Symlink => {
                let rel = row.rel.clone();
                if let Some(open) = &mut self.open {
                    open.tree.dismiss_filter();
                }
                self.open_path(rel);
            }
        }
        Effect::None
    }

    fn edit(&mut self, key: dd_edit::Key, cx: &mut Context<Self>) -> Effect {
        let Some(open) = &mut self.open else {
            return Effect::None;
        };
        let action = open.editor.handle(key);
        self.apply_action(action, cx)
    }

    fn apply_action(&mut self, action: Action, cx: &mut Context<Self>) -> Effect {
        match action {
            Action::None => Effect::None,
            Action::CopyClipboard(text) => {
                if !text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                Effect::None
            }
            Action::Save => {
                self.save();
                Effect::None
            }
            Action::Quit => self.request_quit(false),
            Action::QuitForce => self.request_quit(true),
            Action::SaveQuit => {
                self.save();
                if self.open.as_ref().is_some_and(|open| !open.editor.dirty) {
                    self.request_quit(true)
                } else {
                    Effect::None
                }
            }
            Action::Open(name) => {
                self.open_path(PathBuf::from(name));
                Effect::None
            }
            Action::Error(msg) | Action::Info(msg) => {
                self.status = Some(msg);
                Effect::None
            }
            Action::Daily => {
                self.open_daily();
                Effect::None
            }
            Action::WikiPicker => {
                self.open_finder(FinderKind::Wiki);
                Effect::None
            }
            Action::FollowLink { from_gf } => {
                self.follow_link(from_gf);
                Effect::None
            }
            Action::Help => {
                self.open_help();
                Effect::None
            }
            Action::Git(op) => {
                self.handle_git(op);
                Effect::None
            }
            Action::AiToggle => {
                self.ai.cycle();
                Effect::None
            }
            Action::AiOn => {
                self.ai_enable();
                Effect::None
            }
            Action::AiOff => {
                self.ai_disable();
                Effect::None
            }
            Action::AiPrompt(prompt) => {
                self.request_ai(prompt, None);
                Effect::None
            }
            Action::AiProvider(name) => {
                if name.trim().is_empty() {
                    self.prompt_ai_provider();
                } else {
                    self.set_ai_provider(&name);
                }
                Effect::None
            }
            Action::AiKey(key) => {
                match key {
                    Some(key) => self.submit_ai_key(key, false),
                    None => self.prompt_ai_key(false),
                }
                Effect::None
            }
            Action::AiModel(model) => {
                match model {
                    Some(model) => self.submit_ai_model(model),
                    None => self.prompt_ai_model(),
                }
                Effect::None
            }
            Action::OpenExternal => {
                self.open_external_at_caret();
                Effect::None
            }
        }
    }

    fn request_quit(&mut self, force: bool) -> Effect {
        self.persist();
        let dirty = self.open.as_ref().is_some_and(|open| open.editor.dirty);
        if force || !dirty {
            return Effect::Quit;
        }
        self.confirm = Some(Confirm::Quit);
        Effect::None
    }

    fn save(&mut self) {
        let Some(open) = &mut self.open else {
            return;
        };
        let Some(rel) = open.editor.rel.clone() else {
            self.status = Some("No file name".into());
            return;
        };
        let text = open.editor.text();
        match write_vault_file(&open.vault.root, Path::new(&rel), text.as_bytes()) {
            Ok(path) => {
                open.editor.path = Some(path);
                open.editor.mark_saved();
                self.status = Some(format!("Saved {rel}"));
                self.persist();
                self.refresh_git();
            }
            Err(err) => self.status = Some(err.to_string()),
        }
    }

    fn persist(&mut self) {
        let Some(open) = &self.open else {
            return;
        };
        let mut session = Session::load(&open.vault.meta_dir).unwrap_or_default();
        let pane = pane_name(open.pane_tree, open.focus_preview);
        remember_session(
            &mut session,
            SessionWrite {
                note: open.editor.rel.clone(),
                cursor: open.editor.cursor(),
                scroll: open.editor.scroll,
                scroll_off: open.editor.scroll_off,
                wrap: open.wrap,
                preview: open.preview_visible,
                pane,
                zen: open.zen,
                focus: open.focus_mode,
                explore: open.explore,
                tree_split: open.tree_split,
                preview_split: open.preview_split,
            },
        );
        let _ = session.save(&open.vault.meta_dir);
    }

    fn open_path(&mut self, rel: PathBuf) {
        self.navigate(NavigateTo::file(rel));
    }

    fn load_note(&mut self, rel: PathBuf) {
        let Some(open) = &mut self.open else {
            return;
        };
        let abs = match confined_path(&open.vault.root, &rel) {
            Ok(path) => path,
            Err(err) => {
                self.status = Some(err.to_string());
                return;
            }
        };
        if !abs.is_file() {
            self.status = Some(format!("not a file: {}", rel.display()));
            return;
        }
        match Editor::open(abs, rel.to_string_lossy().into_owned()) {
            Ok(mut editor) => {
                editor.follow_caret = true;
                open.editor = editor;
                open.tree.reveal(&rel);
                self.focus_after_open();
                self.confirm = None;
                self.persist();
            }
            Err(err) => self.status = Some(err.to_string()),
        }
    }

    fn open_picker_selection(&mut self) {
        let Some(entry) = self.picker.get(self.picker_selected) else {
            self.notice = Some("No vaults yet. Run dd_vault-gui init <path>.".into());
            return;
        };
        let path = PathBuf::from(&entry.path);
        match dd_vault_core::open(&path) {
            Ok(vault) => {
                if let Err(err) = register(&self.paths, &vault) {
                    self.status = Some(err.to_string());
                }
                if let Ok(registry) = Registry::load(&self.paths) {
                    self.picker = registry.vaults.clone();
                    self.registry = registry;
                }
                self.notice = None;
                self.overlay = None;
                self.leader = Leader::None;
                let (open, watch_status) = attach_vault(vault);
                self.status = watch_status;
                self.preview_cache = None;
                self.open = Some(open);
                self.queue_reindex();
            }
            Err(err) => self.status = Some(err.to_string()),
        }
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left || self.modal_up() {
            return;
        }
        if let Some(kind) = self.split_at(event.position) {
            self.split_drag = Some(kind);
            self.drag = None;
            cx.notify();
            return;
        }
        if self.hit_tree(event.position) {
            self.click_tree(event);
        } else if self.hit_preview(event.position) {
            self.click_preview(event);
        } else if let Some((line, col, in_gutter)) = self.hit_notes(event.position, false) {
            if event.modifiers.control && !in_gutter {
                self.click_notes(line, col, false);
                self.drag = None;
                self.open_external_at_caret();
            } else {
                self.click_notes(line, col, in_gutter);
            }
        }
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(drag) = self.scroll_drag {
            if event.dragging() {
                self.drag_bar(drag, event.position);
                cx.notify();
            }
            return;
        }
        if let Some(kind) = self.split_drag {
            if event.dragging() {
                self.drag_split(kind, event.position);
                cx.notify();
            }
            return;
        }
        if self.modal_up() || !event.dragging() || self.drag.is_none() {
            return;
        }
        let Some((line, col, _)) = self.hit_notes(event.position, true) else {
            return;
        };
        let Some(mut drag) = self.drag.take() else {
            return;
        };
        let Some(open) = &mut self.open else {
            return;
        };
        if !drag.linewise && !drag.moved && (line != drag.start_line || col != drag.start_col) {
            open.editor
                .begin_mouse_select(drag.start_line, drag.start_col, false);
            drag.moved = true;
        }
        if drag.moved || drag.linewise {
            open.editor.update_mouse_select(line, col);
        }
        self.drag = Some(drag);
        cx.notify();
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        if self.scroll_drag.take().is_some() {
            cx.notify();
            return;
        }
        if self.split_drag.take().is_some() {
            self.persist();
            cx.notify();
            return;
        }
        if self.drag.is_none() {
            return;
        }
        self.drag = None;
        if let Some(open) = &mut self.open {
            open.editor.finish_mouse_select();
        }
        cx.notify();
    }

    fn wheel(&mut self, event: &ScrollEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let dy = event.delta.pixel_delta(self.line_h).y / px(1.);
        let rows = wheel_rows(dy, self.line_h / px(1.));
        if rows == 0 {
            return;
        }
        if self.help {
            self.scroll_help(rows);
            cx.notify();
            return;
        }
        if self.theme_open() {
            self.scroll_theme(rows);
            cx.notify();
        }
    }

    fn hit_tree(&self, pos: Point<Pixels>) -> bool {
        self.hit.tree.is_some_and(|bounds| bounds.contains(&pos))
    }

    fn click_tree(&mut self, event: &MouseDownEvent) {
        let Some(bounds) = self.hit.tree else {
            return;
        };
        let y = (event.position.y - bounds.origin.y) / self.line_h;
        let index = self.open.as_ref().map(|open| open.tree.scroll).unwrap_or(0)
            + y.floor().max(0.0) as usize;
        let activate = {
            let Some(open) = &mut self.open else {
                return;
            };
            open.pane_tree = true;
            open.focus_preview = false;
            if index < open.tree.visible().len() {
                open.tree.selected = index;
                event.click_count >= 2
            } else {
                false
            }
        };
        if activate {
            self.activate_tree();
        }
    }

    fn click_notes(&mut self, line: usize, col: usize, in_gutter: bool) {
        let Some(open) = &mut self.open else {
            return;
        };
        open.pane_tree = false;
        open.focus_preview = false;
        if in_gutter {
            open.editor.begin_mouse_select(line, 0, true);
            self.drag = Some(Drag {
                linewise: true,
                start_line: line,
                start_col: 0,
                moved: true,
            });
        } else {
            open.editor.click_to(line, col);
            self.drag = Some(Drag {
                linewise: false,
                start_line: line,
                start_col: col,
                moved: false,
            });
        }
    }

    fn hit_notes(&self, pos: Point<Pixels>, clamp: bool) -> Option<(usize, usize, bool)> {
        let bounds = self.hit.notes?;
        if !clamp && !bounds.contains(&pos) {
            return None;
        }
        let mut x = (pos.x - bounds.origin.x) / px(1.);
        let mut y = (pos.y - bounds.origin.y) / px(1.);
        if clamp {
            let max_x = (bounds.size.width / px(1.) - 1.0).max(0.0);
            let max_y = (bounds.size.height / px(1.) - 1.0).max(0.0);
            x = x.clamp(0.0, max_x);
            y = y.clamp(0.0, max_y);
        }
        let line_h = (self.line_h / px(1.)).max(1.0);
        let char_w = (self.char_w / px(1.)).max(1.0);
        let row = (y / line_h).floor().max(0.0) as usize;
        if !clamp && row >= self.text_rows {
            return None;
        }
        let row = row.min(self.text_rows.saturating_sub(1));
        let gutter_px = self.gutter_cols as f32 * char_w;
        let in_gutter = x < gutter_px;
        let col = if in_gutter {
            0
        } else {
            ((x - gutter_px) / char_w).floor().max(0.0) as usize
        };
        let open = self.open.as_ref()?;
        let (line, col) = open.editor.hit_wrapped(row, col, self.wrap_width);
        Some((line, if in_gutter { 0 } else { col }, in_gutter))
    }

    fn modal_up(&self) -> bool {
        self.confirm.is_some() || self.overlay.is_some() || self.help || self.theme_open()
    }

    fn cycle_pane(&mut self) {
        let Some(open) = &mut self.open else {
            return;
        };
        if open.preview_read {
            open.pane_tree = false;
            open.focus_preview = !open.focus_preview;
            return;
        }
        let tree = show_tree(
            open.zen,
            open.focus_mode,
            open.explore,
            false,
            self.body_cols,
        );
        let preview = show_preview(open.zen, false, open.preview_visible);
        if open.pane_tree {
            open.pane_tree = false;
            open.focus_preview = false;
        } else if !open.focus_preview && preview {
            open.focus_preview = true;
        } else if tree {
            open.focus_preview = false;
            open.pane_tree = true;
        } else {
            open.focus_preview = false;
            open.pane_tree = false;
        }
    }

    fn toggle_zen(&mut self) {
        self.leader = Leader::None;
        let Some(open) = &mut self.open else {
            return;
        };
        open.zen = !open.zen;
        if open.zen {
            open.pane_tree = false;
            open.focus_preview = false;
        }
        let zen = open.zen;
        if zen {
            self.ai.collapse();
        }
        self.persist();
    }

    fn toggle_focus(&mut self) {
        self.leader = Leader::None;
        let Some(open) = &mut self.open else {
            return;
        };
        if open.zen {
            open.zen = false;
        }
        open.focus_mode = !open.focus_mode;
        open.explore = false;
        if open.focus_mode && open.pane_tree {
            open.pane_tree = false;
            open.focus_preview = false;
        }
        self.persist();
    }

    fn toggle_explore(&mut self) {
        self.leader = Leader::None;
        let Some(open) = &mut self.open else {
            return;
        };
        if open.zen {
            open.zen = false;
        }
        open.explore = !open.explore;
        if open.explore {
            open.focus_mode = false;
            open.pane_tree = true;
            open.focus_preview = false;
        } else if open.pane_tree {
            open.pane_tree = false;
        }
        self.persist();
    }

    fn toggle_preview(&mut self) {
        let Some(open) = &mut self.open else {
            return;
        };
        open.preview_read = false;
        open.preview_visual = None;
        open.preview_visible = !open.preview_visible;
        if !open.preview_visible {
            open.focus_preview = false;
        }
        self.persist();
    }

    fn toggle_preview_read(&mut self) {
        let Some(open) = &mut self.open else {
            return;
        };
        if open.editor.rel.is_none() {
            self.status = Some("Open a note to read the preview".into());
            return;
        }
        open.preview_read = !open.preview_read;
        open.preview_visual = None;
        if open.preview_read {
            open.preview_visible = true;
            open.focus_preview = true;
            open.pane_tree = false;
            open.preview_row = open.preview_scroll;
            self.reading_follow = true;
        } else {
            open.focus_preview = false;
        }
    }

    fn focus_after_open(&mut self) {
        let Some(open) = &mut self.open else {
            return;
        };
        open.pane_tree = false;
        if open.preview_read {
            open.focus_preview = true;
            open.preview_scroll = 0;
            open.preview_row = 0;
            open.preview_visual = None;
            self.preview_px = 0.0;
            self.reading_follow = true;
        } else {
            open.focus_preview = false;
        }
    }

    fn handle_preview_key(&mut self, press: KeyPress<'_>, cx: &mut Context<Self>) {
        if self.open.as_ref().is_some_and(|open| open.preview_read) {
            self.handle_reading_key(press, cx);
            return;
        }
        let page = self.preview_view_h.max(LINE);
        match preview_motion(press) {
            Some(delta) if delta.abs() >= 10 => self.nudge_preview(delta.signum() as f32 * page),
            Some(delta) => self.nudge_preview(delta as f32 * LINE),
            None if is_home(press) => self.set_preview_px(0.0),
            None if is_end(press) => self.set_preview_px(self.preview_content),
            None => {}
        }
    }

    fn handle_reading_key(&mut self, press: KeyPress<'_>, cx: &mut Context<Self>) {
        self.reading_follow = true;
        if press.key == "escape" {
            let visual = self.open.as_ref().and_then(|open| open.preview_visual);
            if visual.is_some() {
                if let Some(open) = &mut self.open {
                    open.preview_visual = None;
                }
            } else {
                self.toggle_preview_read();
            }
            return;
        }
        let ch = typed_char(press);
        if ch == Some('v') {
            if let Some(open) = &mut self.open {
                open.preview_visual = if open.preview_visual.is_some() {
                    None
                } else {
                    Some(open.preview_row)
                };
            }
            return;
        }
        if ch == Some('y') {
            self.yank_preview(cx);
            return;
        }
        let len = self.preview_len();
        let page = self.preview_vis.max(1) as isize;
        let delta = if let Some(delta) = preview_motion(press) {
            if delta == 10 || delta == -10 {
                delta.signum() * page
            } else {
                delta
            }
        } else if is_home(press) {
            let Some(open) = &mut self.open else {
                return;
            };
            open.preview_row = 0;
            open.preview_scroll = 0;
            return;
        } else if is_end(press) {
            let Some(open) = &mut self.open else {
                return;
            };
            open.preview_row = len.saturating_sub(1);
            open.preview_scroll = open.preview_row;
            return;
        } else {
            return;
        };
        let Some(open) = &mut self.open else {
            return;
        };
        open.preview_row = move_clamped(open.preview_row, len, delta);
        open.preview_scroll = open.preview_row;
    }

    fn yank_preview(&mut self, cx: &mut Context<Self>) {
        let lines = self.block_plains();
        let (row, visual) = self
            .open
            .as_ref()
            .map(|open| (open.preview_row, open.preview_visual))
            .unwrap_or((0, None));
        let text = yank_text(&lines, row, visual);
        if text.is_empty() {
            return;
        }
        let count = text.lines().count();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        if let Some(open) = &mut self.open {
            open.preview_visual = None;
        }
        self.status = Some(format!(
            "Copied {count} preview line{}",
            if count == 1 { "" } else { "s" }
        ));
    }

    fn preview_len(&self) -> usize {
        self.block_plains().len().max(1)
    }

    fn block_plains(&self) -> Vec<String> {
        self.preview_cache
            .as_ref()
            .map(|cache| cache.doc.blocks.iter().map(block_plain).collect())
            .unwrap_or_default()
    }

    fn preview_link(
        &self,
        index: usize,
        _col: usize,
        _span: usize,
    ) -> Option<(String, Option<String>)> {
        let block = self.preview_cache.as_ref()?.doc.blocks.get(index)?;
        let run = block.runs.iter().find(|run| !run.link.is_empty())?;
        Some(split_link(&run.link))
    }

    fn open_external_current(&mut self) {
        let on_preview = self
            .open
            .as_ref()
            .is_some_and(|open| open.focus_preview || open.preview_read);
        if on_preview {
            let index = self
                .open
                .as_ref()
                .map(|open| {
                    if open.preview_read {
                        open.preview_row
                    } else {
                        open.preview_scroll
                    }
                })
                .unwrap_or(0);
            self.open_external_on_row(index);
        } else {
            self.open_external_at_caret();
        }
    }

    fn open_external_at_caret(&mut self) {
        let Some(text) = self.open.as_ref().and_then(|open| {
            open.editor.rel.as_ref()?;
            Some(open.editor.text())
        }) else {
            self.status = Some("Open a note first".into());
            return;
        };
        let cursor = self
            .open
            .as_ref()
            .map(|open| open.editor.cursor())
            .unwrap_or(0);
        let Some(link) = link_at(&text, cursor) else {
            self.status = Some("No link under the caret".into());
            return;
        };
        self.open_external_raw(&link.dst_raw);
    }

    fn open_external_on_row(&mut self, index: usize) {
        match self.preview_link(index, 0, 1) {
            Some((target, _)) => self.open_external_raw(&target),
            None => self.status = Some("No link on this preview line".into()),
        }
    }

    fn open_external_raw(&mut self, raw: &str) {
        match self.external_for(raw) {
            Ok(target) => match launch_default_app(&target) {
                Ok(()) => self.status = Some("Opened in the default app".into()),
                Err(err) => self.status = Some(err),
            },
            Err(err) => self.status = Some(err),
        }
    }

    fn external_for(&self, raw: &str) -> Result<String, String> {
        let dst = raw
            .split('#')
            .next()
            .unwrap_or(raw)
            .split('|')
            .next()
            .unwrap_or(raw)
            .trim();
        let resolved = if dst.is_empty() || is_external_url(dst) {
            None
        } else {
            self.resolve_link_path(dst)
        };
        external_open_target(
            self.open.as_ref().map(|open| open.vault.root.as_path()),
            resolved.as_deref(),
            raw,
        )
    }

    fn hit_preview(&self, pos: Point<Pixels>) -> bool {
        self.hit.preview.is_some_and(|bounds| bounds.contains(&pos))
    }

    fn click_preview(&mut self, event: &MouseDownEvent) {
        let Some((index, col)) = self.preview_cell(event.position) else {
            return;
        };
        let reading = self.open.as_ref().is_some_and(|open| open.preview_read);
        if reading {
            self.reading_follow = true;
        }
        if let Some(open) = &mut self.open {
            open.pane_tree = false;
            open.focus_preview = true;
            if reading {
                open.preview_row = index;
            }
        }
        if event.modifiers.control {
            match self.preview_link(index, col, 1) {
                Some((target, _)) => self.open_external_raw(&target),
                None => self.status = Some("No link under the pointer".into()),
            }
        }
    }

    fn preview_cell(&self, pos: Point<Pixels>) -> Option<(usize, usize)> {
        let bounds = self.hit.preview?;
        if !bounds.contains(&pos) {
            return None;
        }
        let local_x = (pos.x - bounds.origin.x) / px(1.);
        let width = bounds.size.width / px(1.);
        if local_x >= width - BAR_W {
            return None;
        }
        let y = (pos.y - bounds.origin.y) / px(1.) + self.preview_px;
        if self.preview_tops.is_empty() {
            return None;
        }
        let index = index_at(&self.preview_tops, &self.preview_heights, y);
        let x = (local_x - 8.0).max(0.0);
        let char_w = (self.char_w / px(1.)).max(1.0);
        let col = (x / char_w).floor().max(0.0) as usize;
        Some((index, col))
    }

    fn focus_preview_pane(&mut self) {
        if let Some(open) = &mut self.open {
            open.pane_tree = false;
            open.focus_preview = true;
        }
    }

    fn nudge_preview(&mut self, dy: f32) {
        self.set_preview_px(self.preview_px + dy);
    }

    fn set_preview_px(&mut self, px: f32) {
        // Pixel scrolling must not be snapped back by the reading-view reveal.
        self.reading_follow = false;
        let max = (self.preview_content - self.preview_view_h).max(0.0);
        self.preview_px = px.clamp(0.0, max);
        let index = index_at(&self.preview_tops, &self.preview_heights, self.preview_px);
        if let Some(open) = &mut self.open {
            open.pane_tree = false;
            open.focus_preview = true;
            open.preview_scroll = index;
            if open.preview_read {
                open.preview_row = index;
            }
        }
    }

    fn bar_down(&mut self, kind: BarKind, event: &MouseDownEvent) {
        if self.modal_up() || event.button != MouseButton::Left {
            return;
        }
        let Some(bounds) = self.bar_bounds(kind) else {
            return;
        };
        let y = (event.position.y - bounds.origin.y) / px(1.);
        let track = self.pointer_track(kind, bounds);
        if y < 0.0 || y > track {
            return;
        }
        let (offset, visible, total) = self.bar_units(kind);
        let Some(thumb) = thumb_geom(offset, visible, total, track) else {
            return;
        };
        if y >= thumb.y && y <= thumb.y + thumb.h {
            self.scroll_drag = Some(ScrollDrag {
                kind,
                grab: y - thumb.y,
            });
        } else if y < thumb.y {
            self.page_bar(kind, -1.0);
        } else {
            self.page_bar(kind, 1.0);
        }
    }

    fn drag_bar(&mut self, drag: ScrollDrag, pos: Point<Pixels>) {
        let Some(bounds) = self.bar_bounds(drag.kind) else {
            return;
        };
        let y = (pos.y - bounds.origin.y) / px(1.);
        let track = self.pointer_track(drag.kind, bounds);
        let (_offset, visible, total) = self.bar_units(drag.kind);
        let next = offset_for_thumb(y - drag.grab, visible, total, track);
        match drag.kind {
            BarKind::Tree => self.set_tree_offset(next),
            BarKind::Notes => self.set_notes_offset(next),
            BarKind::Preview => self.set_preview_px(next),
        }
    }

    fn page_bar(&mut self, kind: BarKind, sign: f32) {
        match kind {
            BarKind::Tree => {
                let current = self.open.as_ref().map(|open| open.tree.scroll).unwrap_or(0) as f32;
                self.set_tree_offset(current + self.tree_rows.max(1) as f32 * sign);
            }
            BarKind::Notes => {
                let current = self
                    .open
                    .as_ref()
                    .map(|open| open.editor.visual_origin(self.wrap_width))
                    .unwrap_or(0) as f32;
                self.set_notes_offset(current + self.text_rows.max(1) as f32 * sign);
            }
            BarKind::Preview => self.set_preview_px(self.preview_px + self.preview_view_h * sign),
        }
    }

    fn pointer_track(&self, kind: BarKind, bounds: Bounds<Pixels>) -> f32 {
        let laid_out = match kind {
            BarKind::Tree => self.tree_port.view_px(),
            BarKind::Notes => self.notes_port.view_px(),
            BarKind::Preview => self.preview_port.view_px(),
        };
        laid_out.unwrap_or(bounds.size.height / px(1.)).max(1.0)
    }

    fn bar_bounds(&self, kind: BarKind) -> Option<Bounds<Pixels>> {
        match kind {
            BarKind::Tree => self.hit.tree,
            BarKind::Notes => self.hit.notes,
            BarKind::Preview => self.hit.preview,
        }
    }

    fn bar_units(&self, kind: BarKind) -> (f32, f32, f32) {
        match kind {
            BarKind::Tree => {
                let total = self
                    .open
                    .as_ref()
                    .map(|open| open.tree.visible().len())
                    .unwrap_or(0) as f32;
                let offset = self.open.as_ref().map(|open| open.tree.scroll).unwrap_or(0) as f32;
                (offset, self.tree_rows.max(1) as f32, total)
            }
            BarKind::Notes => {
                let (offset, total) = self
                    .open
                    .as_ref()
                    .map(|open| {
                        (
                            open.editor.visual_origin(self.wrap_width) as f32,
                            open.editor.visual_len(self.wrap_width) as f32,
                        )
                    })
                    .unwrap_or((0.0, 0.0));
                (offset, self.text_rows.max(1) as f32, total)
            }
            BarKind::Preview => (
                self.preview_px,
                self.preview_view_h.max(1.0),
                self.preview_content,
            ),
        }
    }

    fn set_tree_offset(&mut self, offset: f32) {
        let rows = self.tree_rows.max(1);
        let Some(open) = &mut self.open else {
            return;
        };
        let max = open.tree.visible().len().saturating_sub(rows);
        let next = offset.round().max(0.0) as usize;
        open.tree.scroll = next.min(max);
        open.pane_tree = true;
        open.focus_preview = false;
    }

    fn set_notes_offset(&mut self, offset: f32) {
        let rows = self.text_rows.max(1);
        let width = self.wrap_width;
        let Some(open) = &mut self.open else {
            return;
        };
        let current = open.editor.visual_origin(width) as isize;
        let delta = offset.round() as isize - current;
        open.editor.scroll_wrapped(delta, rows, width);
        open.pane_tree = false;
        open.focus_preview = false;
    }

    fn later(&mut self) {
        self.leader = Leader::None;
        self.status = Some("Not in this window yet".into());
    }

    fn run_chord(&mut self, chord: Chord) {
        self.leader = Leader::None;
        match chord {
            Chord::Picker => self.open_vault_picker(),
            Chord::Files => self.open_finder(FinderKind::Files),
            Chord::Content => self.open_finder(FinderKind::Content),
            Chord::Tags => self.open_finder(FinderKind::Tags),
            Chord::Daily => self.open_daily(),
            Chord::Palette => self.open_palette(),
            Chord::Preview => self.toggle_preview(),
            Chord::PreviewRead => self.toggle_preview_read(),
            Chord::Ai => self.ai.cycle(),
            Chord::Focus => self.toggle_focus(),
            Chord::Zen => self.toggle_zen(),
            Chord::Explore => self.toggle_explore(),
        }
    }

    fn open_vault_picker(&mut self) {
        self.leader = Leader::None;
        if self.registry.vaults.is_empty() {
            self.status = Some("No registered vaults. Run: dd_vault-gui init [path]".into());
            return;
        }
        let last = self
            .registry
            .last_path()
            .map(|path| path.to_string_lossy().into_owned());
        self.picker = self.registry.vaults.clone();
        self.picker_selected = last
            .as_ref()
            .and_then(|path| self.picker.iter().position(|entry| entry.path == *path))
            .unwrap_or(0);
        if self.open.is_some() {
            self.overlay = Some(Overlay::Picker);
        }
    }

    fn begin_forget(&mut self) {
        let Some(entry) = self.picker.get(self.picker_selected).cloned() else {
            return;
        };
        self.overlay = None;
        self.leader = Leader::None;
        self.confirm = Some(Confirm::ForgetVault {
            name: entry.name,
            path: entry.path,
        });
    }

    fn forget_vault(&mut self, path: &str, name: &str) {
        let was_open = self
            .open
            .as_ref()
            .is_some_and(|open| open.vault.root.to_string_lossy() == path);
        if !self.registry.unregister(path) {
            self.confirm = None;
            return;
        }
        if let Err(err) = self.registry.save(&self.paths) {
            self.status = Some(format!("Could not save vaults.toml: {err}"));
        }
        self.picker = self.registry.vaults.clone();
        self.picker_selected = self
            .picker_selected
            .min(self.picker.len().saturating_sub(1));
        self.confirm = None;
        if was_open {
            self.persist();
            self.open = None;
            self.leader = Leader::None;
        }
        self.overlay = None;
        if self.status.is_none() {
            self.status = Some(format!("Removed '{name}' from the list (files kept)"));
        }
        if self.registry.vaults.is_empty() {
            self.notice = Some("No vaults yet. Run dd_vault-gui init <path>.".into());
        } else if self.open.is_some() {
            self.overlay = Some(Overlay::Picker);
        }
    }

    fn begin_new(&mut self) -> Effect {
        let Some(parent) = self
            .open
            .as_ref()
            .map(|open| open.tree.parent_rel_for_new())
        else {
            self.status = Some("Open a vault first".into());
            return Effect::None;
        };
        self.leader = Leader::None;
        self.overlay = Some(Overlay::Prompt {
            kind: PromptKind::New { parent },
            draft: String::new(),
        });
        Effect::None
    }

    fn begin_rename(&mut self) -> Effect {
        let draft = {
            let Some(row) = self.open.as_ref().and_then(|open| open.tree.selected_row()) else {
                return Effect::None;
            };
            if row.name == ".." {
                return Effect::None;
            }
            (
                row.rel.clone(),
                row.rel
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
        };
        self.leader = Leader::None;
        self.overlay = Some(Overlay::Prompt {
            kind: PromptKind::Rename { rel: draft.0 },
            draft: draft.1,
        });
        Effect::None
    }

    fn begin_delete(&mut self) -> Effect {
        let Some(rel) = self.open.as_ref().and_then(|open| {
            let row = open.tree.selected_row()?;
            if row.name == ".." {
                None
            } else {
                Some(row.rel.clone())
            }
        }) else {
            return Effect::None;
        };
        self.leader = Leader::None;
        self.confirm = Some(Confirm::Delete { rel });
        Effect::None
    }

    fn delete_rel(&mut self, rel: PathBuf) {
        let Some(root) = self.open.as_ref().map(|open| open.vault.root.clone()) else {
            self.confirm = None;
            return;
        };
        match delete_entry(&root, &rel) {
            Ok(()) => {
                if let Some(open) = &mut self.open {
                    if let Some(current) = open.editor.rel.clone().map(PathBuf::from)
                        && (current == rel || current.starts_with(&rel))
                    {
                        open.editor = Editor::empty();
                    }
                    reload_tree(open);
                }
                self.confirm = None;
                self.queue_reindex();
                self.persist();
                self.refresh_git();
                self.status = Some(format!("Deleted {}", rel.display()));
            }
            Err(err) => self.status = Some(err.to_string()),
        }
    }

    fn handle_overlay(&mut self, press: KeyPress<'_>) -> Effect {
        let effect = if matches!(self.overlay, Some(Overlay::Picker)) {
            self.handle_overlay_picker(press)
        } else if matches!(self.overlay, Some(Overlay::Prompt { .. })) {
            self.handle_prompt_key(press);
            Effect::None
        } else if matches!(self.overlay, Some(Overlay::Finder { .. })) {
            self.handle_finder_key(press);
            Effect::None
        } else if matches!(self.overlay, Some(Overlay::Palette { .. })) {
            self.handle_palette_key(press)
        } else if matches!(self.overlay, Some(Overlay::Notice { .. })) {
            self.handle_notice_key(press);
            Effect::None
        } else {
            Effect::None
        };
        if self.overlay.is_none() && self.confirm.is_none() {
            self.flush_disk_prompt();
        }
        effect
    }

    fn handle_overlay_picker(&mut self, press: KeyPress<'_>) -> Effect {
        if press.control {
            return Effect::None;
        }
        match picker_key(press) {
            PickerKey::Quit => self.overlay = None,
            PickerKey::Forget => self.begin_forget(),
            PickerKey::Open => self.open_picker_selection(),
            PickerKey::Home => self.picker_selected = 0,
            PickerKey::End => {
                self.picker_selected = self.picker.len().saturating_sub(1);
            }
            PickerKey::Move(delta) => {
                let n = self.picker.len() as i32;
                if n > 0 {
                    let next = self.picker_selected as i32 + delta;
                    self.picker_selected = next.clamp(0, n - 1) as usize;
                }
            }
            PickerKey::Ignore => {}
        }
        Effect::None
    }

    fn handle_prompt_key(&mut self, press: KeyPress<'_>) {
        if press.key == "escape" {
            self.overlay = None;
            return;
        }
        if press.key == "enter" || press.key == "return" {
            self.submit_prompt();
            return;
        }
        if press.key == "backspace" {
            if let Some(Overlay::Prompt { draft, .. }) = &mut self.overlay {
                draft.pop();
            }
            return;
        }
        if let Some(ch) = typed_char(press)
            && let Some(Overlay::Prompt { draft, .. }) = &mut self.overlay
        {
            draft.push(ch);
        }
    }

    fn submit_prompt(&mut self) {
        let Some((kind, draft)) = self.overlay.as_ref().and_then(Overlay::clone_kind) else {
            return;
        };
        match kind {
            PromptKind::New { parent } => self.submit_new(parent, draft),
            PromptKind::Rename { rel } => self.submit_rename(rel, draft),
            PromptKind::GitCommit { then_push } => self.submit_git_commit(draft, then_push),
            PromptKind::AiProvider => self.set_ai_provider(&draft),
            PromptKind::AiKey { then_enable } => self.submit_ai_key(draft, then_enable),
            PromptKind::AiModel => self.submit_ai_model(draft),
        }
    }

    fn submit_new(&mut self, parent: PathBuf, draft: String) {
        let Some(root) = self.open.as_ref().map(|open| open.vault.root.clone()) else {
            self.overlay = None;
            return;
        };
        let (name, is_dir) = match parse_new_name(&draft) {
            Ok(parsed) => parsed,
            Err(err) => {
                self.status = Some(err);
                return;
            }
        };
        let rel = if parent.as_os_str().is_empty() {
            PathBuf::from(&name)
        } else {
            parent.join(&name)
        };
        let result = if is_dir {
            create_dir(&root, &rel)
        } else {
            create_file(&root, &rel)
        };
        if let Err(err) = result {
            self.status = Some(err.to_string());
            return;
        }
        self.overlay = None;
        if let Some(open) = &mut self.open {
            reload_tree(open);
            open.tree.dismiss_filter();
            open.tree.reveal(&rel);
        }
        self.queue_reindex();
        self.refresh_git();
        self.status = Some(format!("Created {}", rel.display()));
        if !is_dir {
            self.navigate(NavigateTo::file(rel));
        }
    }

    fn submit_rename(&mut self, rel: PathBuf, draft: String) {
        let name = draft.trim();
        if let Err(err) = validate_entry_name(name) {
            self.status = Some(err.to_string());
            return;
        }
        let name = name.to_string();
        let Some(root) = self.open.as_ref().map(|open| open.vault.root.clone()) else {
            self.overlay = None;
            return;
        };
        let dest = match rename_entry(&root, &rel, &name) {
            Ok(dest) => dest,
            Err(err) => {
                self.status = Some(err.to_string());
                return;
            }
        };
        let new_rel = dest
            .strip_prefix(&root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| PathBuf::from(&name));
        if let Some(open) = &mut self.open {
            let old = rel.to_string_lossy();
            if open.editor.rel.as_deref() == Some(old.as_ref()) {
                if let Ok(abs) = confined_path(&root, &new_rel) {
                    open.editor.path = Some(abs);
                }
                open.editor.rel = Some(new_rel.to_string_lossy().into_owned());
            }
            reload_tree(open);
            open.tree.dismiss_filter();
            open.tree.reveal(&new_rel);
        }
        self.overlay = None;
        self.queue_reindex();
        self.persist();
        self.refresh_git();
        self.status = Some(format!("Renamed to {}", new_rel.display()));
    }

    fn handle_finder_key(&mut self, press: KeyPress<'_>) {
        if press.key == "escape" {
            self.overlay = None;
            return;
        }
        if press.key == "enter" || press.key == "return" {
            self.activate_finder();
            return;
        }
        if let Some(delta) = list_delta(press) {
            if let Some(Overlay::Finder { selected, hits, .. }) = &mut self.overlay {
                move_selected(selected, hits.len(), delta);
            }
            return;
        }
        if press.key == "backspace" {
            if let Some(Overlay::Finder { query, .. }) = &mut self.overlay {
                query.pop();
            }
            self.refresh_finder();
            return;
        }
        if let Some(ch) = typed_char(press) {
            if let Some(Overlay::Finder { query, .. }) = &mut self.overlay {
                query.push(ch);
            }
            self.refresh_finder();
        }
    }

    fn handle_palette_key(&mut self, press: KeyPress<'_>) -> Effect {
        if press.key == "escape" {
            self.overlay = None;
            return Effect::None;
        }
        if press.key == "enter" || press.key == "return" {
            return self.activate_palette();
        }
        if let Some(delta) = list_delta(press) {
            if let Some(Overlay::Palette {
                selected, items, ..
            }) = &mut self.overlay
            {
                move_selected(selected, items.len(), delta);
            }
            return Effect::None;
        }
        if press.key == "backspace" {
            if let Some(Overlay::Palette { query, .. }) = &mut self.overlay {
                query.pop();
            }
            self.refresh_palette();
            return Effect::None;
        }
        if let Some(ch) = typed_char(press) {
            if let Some(Overlay::Palette { query, .. }) = &mut self.overlay {
                query.push(ch);
            }
            self.refresh_palette();
        }
        Effect::None
    }

    fn open_palette(&mut self) {
        self.leader = Leader::None;
        self.overlay = Some(Overlay::Palette {
            query: String::new(),
            selected: 0,
            items: filter_palette(""),
        });
    }

    fn refresh_palette(&mut self) {
        let Some(query) = self.overlay.as_ref().and_then(|overlay| match overlay {
            Overlay::Palette { query, .. } => Some(query.clone()),
            _ => None,
        }) else {
            return;
        };
        let items = filter_palette(&query);
        if let Some(Overlay::Palette {
            selected,
            items: slot,
            ..
        }) = &mut self.overlay
        {
            *slot = items;
            *selected = (*selected).min(slot.len().saturating_sub(1));
        }
    }

    fn activate_palette(&mut self) -> Effect {
        let item = match &self.overlay {
            Some(Overlay::Palette {
                selected, items, ..
            }) => items.get(*selected).copied(),
            _ => None,
        };
        self.overlay = None;
        let Some(item) = item else {
            return Effect::None;
        };
        match palette_action(item.id) {
            PaletteAction::Save => self.save(),
            PaletteAction::Quit => return self.request_quit(false),
            PaletteAction::Files => self.open_finder(FinderKind::Files),
            PaletteAction::Content => self.open_finder(FinderKind::Content),
            PaletteAction::Tags => self.open_finder(FinderKind::Tags),
            PaletteAction::Vault => self.open_vault_picker(),
            PaletteAction::Daily => self.open_daily(),
            PaletteAction::JumpBack => self.jump_back(),
            PaletteAction::JumpForward => self.jump_forward(),
            PaletteAction::Follow => self.follow_link(true),
            PaletteAction::New => {
                self.begin_new();
            }
            PaletteAction::Preview => self.toggle_preview(),
            PaletteAction::Ai => self.ai.cycle(),
            PaletteAction::AiOn => self.ai_enable(),
            PaletteAction::AiOff => self.ai_disable(),
            PaletteAction::AiOpenRouter => self.set_ai_provider("openrouter"),
            PaletteAction::AiOllama => self.set_ai_provider("ollama"),
            PaletteAction::AiSpaceXai => self.set_ai_provider("spacexai"),
            PaletteAction::AiKey => self.prompt_ai_key(false),
            PaletteAction::AiModel => self.prompt_ai_model(),
            PaletteAction::GitStatus => self.handle_git(GitOp::Status),
            PaletteAction::GitCommit => self.handle_git(GitOp::Commit),
            PaletteAction::GitPull => self.handle_git(GitOp::Pull),
            PaletteAction::GitPush => self.handle_git(GitOp::Push),
            PaletteAction::Help => self.open_help(),
            PaletteAction::Theme => self.open_theme(),
            PaletteAction::Focus => self.toggle_focus(),
            PaletteAction::Zen => self.toggle_zen(),
            PaletteAction::Explore => self.toggle_explore(),
            PaletteAction::Later => self.later(),
        }
        Effect::None
    }

    fn open_finder(&mut self, kind: FinderKind) {
        self.leader = Leader::None;
        if self.open.is_none() {
            self.status = Some("No vault open".into());
            return;
        }
        if self.open.as_ref().is_some_and(|open| open.index.is_none()) {
            self.status = Some("Index not ready yet".into());
            return;
        }
        let hits = self.finder_hits(kind, "");
        self.overlay = Some(Overlay::Finder {
            kind,
            query: String::new(),
            selected: 0,
            hits,
        });
    }

    fn finder_hits(&self, kind: FinderKind, query: &str) -> Vec<FileHit> {
        let Some(index) = self.open.as_ref().and_then(|open| open.index.as_ref()) else {
            return Vec::new();
        };
        let found = match kind {
            FinderKind::Files => index.search_files(query),
            FinderKind::Content => index.search_content(query),
            FinderKind::Tags => index.search_tags(query),
            FinderKind::Wiki => index.search_wiki(query),
        };
        found.unwrap_or_default()
    }

    fn refresh_finder(&mut self) {
        let Some((kind, query)) = self.overlay.as_ref().and_then(|overlay| match overlay {
            Overlay::Finder { kind, query, .. } => Some((*kind, query.clone())),
            _ => None,
        }) else {
            return;
        };
        let hits = self.finder_hits(kind, &query);
        if let Some(Overlay::Finder {
            selected,
            hits: slot,
            ..
        }) = &mut self.overlay
        {
            *slot = hits;
            *selected = (*selected).min(slot.len().saturating_sub(1));
        }
    }

    fn activate_finder(&mut self) {
        let Some((kind, hit)) = self.overlay.as_ref().and_then(|overlay| match overlay {
            Overlay::Finder {
                kind,
                selected,
                hits,
                ..
            } => hits.get(*selected).cloned().map(|hit| (*kind, hit)),
            _ => None,
        }) else {
            return;
        };
        let wiki = kind == FinderKind::Wiki
            && self
                .open
                .as_ref()
                .is_some_and(|open| open.editor.mode == Mode::Insert);
        self.overlay = None;
        if wiki {
            let stem = Path::new(&hit.path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(hit.title.as_str());
            let insert = match &hit.heading {
                Some(heading) => format!("{stem}#{heading}]]"),
                None => format!("{stem}]]"),
            };
            if let Some(open) = &mut self.open {
                open.editor.insert_text(&insert);
            }
            return;
        }
        self.navigate(NavigateTo {
            rel: PathBuf::from(hit.path),
            heading: hit.heading,
            cursor: None,
            scroll: None,
            scroll_off: None,
            jump: JumpAction::Record,
        });
    }

    fn open_daily(&mut self) {
        self.leader = Leader::None;
        let Some(vault) = self.open.as_ref().map(|open| open.vault.clone()) else {
            self.status = Some("No vault open".into());
            return;
        };
        let cfg = VaultConfig::load_for(&vault, Some(&self.paths));
        match ensure_daily(&vault.root, &cfg.daily_note_path, &today_ymd()) {
            Ok(rel) => {
                if let Some(open) = &mut self.open {
                    reload_tree(open);
                }
                self.queue_reindex();
                self.refresh_git();
                self.navigate(NavigateTo::file(rel));
            }
            Err(err) => self.status = Some(err.to_string()),
        }
    }

    fn current_jump(&self) -> Option<Jump> {
        let open = self.open.as_ref()?;
        Some(Jump {
            rel: PathBuf::from(open.editor.rel.as_ref()?),
            cursor: open.editor.cursor(),
            scroll: open.editor.scroll,
            scroll_off: open.editor.scroll_off,
        })
    }

    fn navigate(&mut self, to: NavigateTo) {
        let Some(choice) = self.open.as_ref().map(|open| {
            let next = to.rel.to_string_lossy().into_owned();
            choose_open(open.editor.dirty, open.editor.rel.as_deref(), &next)
        }) else {
            return;
        };
        match choice {
            OpenChoice::Same => {
                let here = self.current_jump();
                self.commit_jump(&to, here);
                self.apply_view(&to);
                if let Some(open) = &mut self.open {
                    open.tree.reveal(&to.rel);
                }
                self.focus_after_open();
                self.persist();
            }
            OpenChoice::Confirm => self.confirm = Some(Confirm::Navigate(to)),
            OpenChoice::Load => self.navigate_clean(to),
        }
    }

    fn navigate_clean(&mut self, to: NavigateTo) {
        let here = self.current_jump();
        self.load_note(to.rel.clone());
        let opened = self.open.as_ref().is_some_and(|open| {
            open.editor.rel.as_deref() == Some(to.rel.to_string_lossy().as_ref())
        });
        if !opened {
            return;
        }
        self.commit_jump(&to, here);
        self.apply_view(&to);
        if let Some(open) = &mut self.open {
            open.tree.reveal(&to.rel);
        }
        self.focus_after_open();
        self.persist();
    }

    fn commit_jump(&mut self, to: &NavigateTo, here: Option<Jump>) {
        let Some(open) = &mut self.open else {
            return;
        };
        match to.jump {
            JumpAction::Record => {
                if let Some(here) = here {
                    open.jumps.push(here);
                }
            }
            JumpAction::Back => {
                let here = here.unwrap_or_else(|| fallback_jump(to));
                let _ = open.jumps.back(here);
            }
            JumpAction::Forward => {
                let here = here.unwrap_or_else(|| fallback_jump(to));
                let _ = open.jumps.forward(here);
            }
        }
    }

    fn apply_view(&mut self, to: &NavigateTo) {
        let Some(open) = &mut self.open else {
            return;
        };
        if let (Some(cursor), Some(scroll), Some(off)) = (to.cursor, to.scroll, to.scroll_off) {
            open.editor.restore_view(cursor, scroll, off);
        } else if let Some(heading) = &to.heading {
            jump_to_heading(&mut open.editor, heading);
        }
        open.pane_tree = false;
        if !open.preview_read {
            open.focus_preview = false;
        }
    }

    fn jump_back(&mut self) {
        let Some(dest) = self
            .open
            .as_ref()
            .and_then(|open| open.jumps.peek_back().cloned())
        else {
            self.status = Some("Jump list is empty".into());
            return;
        };
        self.navigate(NavigateTo::from_jump(dest, JumpAction::Back));
    }

    fn jump_forward(&mut self) {
        let Some(dest) = self
            .open
            .as_ref()
            .and_then(|open| open.jumps.peek_forward().cloned())
        else {
            self.status = Some("Jump list is empty".into());
            return;
        };
        self.navigate(NavigateTo::from_jump(dest, JumpAction::Forward));
    }

    fn follow_link(&mut self, from_gf: bool) {
        let Some(text) = self.open.as_ref().and_then(|open| {
            open.editor.rel.as_ref()?;
            Some(open.editor.text())
        }) else {
            if from_gf {
                self.status = Some("Open a note first".into());
            }
            return;
        };
        let cursor = self
            .open
            .as_ref()
            .map(|open| open.editor.cursor())
            .unwrap_or(0);
        let Some(link) = link_at(&text, cursor) else {
            if from_gf {
                self.status = Some("No link under the caret".into());
            }
            return;
        };
        self.follow_target(&link.dst_raw, link.dst_heading.as_deref());
    }

    fn follow_target(&mut self, raw: &str, heading: Option<&str>) {
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
        if is_external_url(dst) {
            self.status = Some("External link. gx, Ctrl+Enter, or Ctrl+click opens it".into());
            return;
        }
        if looks_like_image(dst) {
            self.status = Some("Image links stay in the preview".into());
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
        let current = self
            .open
            .as_ref()
            .and_then(|open| open.editor.rel.clone())
            .map(PathBuf::from);
        match wikilink_new_rel(current.as_deref(), dst) {
            Ok(rel) => {
                self.confirm = Some(Confirm::CreateNote {
                    rel,
                    heading: heading.map(str::to_string),
                });
            }
            Err(err) => self.status = Some(err.to_string()),
        }
    }

    fn resolve_link_path(&self, dst: &str) -> Option<PathBuf> {
        let open = self.open.as_ref()?;
        if let Some(index) = &open.index
            && let Ok(Some(path)) = index.resolve_wikilink(dst)
        {
            return Some(PathBuf::from(path));
        }
        open.tree.find_file(dst)
    }

    fn create_and_open_note(&mut self, rel: PathBuf, heading: Option<String>) {
        let Some(root) = self.open.as_ref().map(|open| open.vault.root.clone()) else {
            return;
        };
        let existing = match confined_path(&root, &rel) {
            Ok(path) => path.is_file(),
            Err(err) => {
                self.status = Some(err.to_string());
                return;
            }
        };
        if !existing {
            let title = rel
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Untitled");
            let body = format!("# {title}\n");
            if let Err(err) = write_vault_file(&root, &rel, body.as_bytes()) {
                self.status = Some(err.to_string());
                return;
            }
            if let Some(open) = &mut self.open {
                reload_tree(open);
            }
            self.queue_reindex();
            self.refresh_git();
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

    fn accept_reload(&mut self, rel: PathBuf) {
        let Some(root) = self.open.as_ref().map(|open| open.vault.root.clone()) else {
            return;
        };
        if let Some(open) = &mut self.open {
            open.suppress_disk = None;
        }
        let inside = confined_path(&root, &rel)
            .ok()
            .filter(|path| path.is_file());
        let Some(abs) = inside else {
            if let Some(open) = &mut self.open {
                open.editor = Editor::empty();
            }
            self.status = Some(format!("{} is gone from disk", rel.display()));
            return;
        };
        let same = self
            .open
            .as_ref()
            .is_some_and(|open| open.editor.rel.as_deref() == Some(rel.to_string_lossy().as_ref()));
        if same {
            let reloaded = self.open.as_mut().map(|open| {
                open.editor.path = Some(abs);
                open.editor.reload_from_disk()
            });
            self.status = match reloaded {
                Some(Ok(())) => Some(format!("Reloaded {}", rel.display())),
                Some(Err(err)) => Some(err.to_string()),
                None => None,
            };
        } else {
            self.load_note(rel);
        }
    }

    fn queue_reindex(&mut self) {
        if self
            .open
            .as_ref()
            .is_some_and(|open| open.index_rx.is_some())
        {
            if let Some(open) = &mut self.open {
                open.reindex_queued = true;
            }
            return;
        }
        self.spawn_reindex();
    }

    fn spawn_reindex(&mut self) {
        let Some(vault) = self.open.as_ref().map(|open| open.vault.clone()) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(dd_vault_core::reindex(&vault).map_err(|err| err.to_string()));
        });
        if let Some(open) = &mut self.open {
            open.index_rx = Some(rx);
        }
    }

    fn poll_services(&mut self) -> bool {
        let watched = self.poll_watch();
        let indexed = self.poll_index();
        let prompted = self.flush_disk_prompt();
        let git = self.poll_git();
        let ai = self.poll_ai();
        let toasted = prune_toasts(&mut self.toasts);
        watched || indexed || prompted || git || ai || toasted
    }

    fn poll_watch(&mut self) -> bool {
        let paths = self
            .open
            .as_mut()
            .and_then(|open| open.watch.as_mut().and_then(VaultWatch::take_ready));
        if let Some(paths) = paths {
            self.handle_fs_changes(&paths)
        } else {
            false
        }
    }

    fn handle_fs_changes(&mut self, abs_paths: &[PathBuf]) -> bool {
        let Some(root) = self.open.as_ref().map(|open| open.vault.root.clone()) else {
            return false;
        };
        let mut rels = Vec::new();
        for abs in abs_paths {
            if let Some(rel) = watch_rel(&root, abs)
                && !rels.contains(&rel)
            {
                rels.push(rel);
            }
        }
        if rels.is_empty() {
            return false;
        }
        if let Some(open) = &mut self.open {
            reload_tree(open);
        }
        self.refresh_git();
        self.queue_reindex();
        if let Some(open_rel) = self.open.as_ref().and_then(|open| open.editor.rel.clone()) {
            let open_path = PathBuf::from(&open_rel);
            if rels
                .iter()
                .any(|rel| rel == &open_path || open_path.starts_with(rel))
            {
                self.on_open_file_changed(&open_path);
            }
        }
        true
    }

    fn on_open_file_changed(&mut self, rel: &Path) {
        if self
            .open
            .as_ref()
            .and_then(|open| open.suppress_disk.as_ref())
            .is_some_and(|saved| saved == rel)
        {
            return;
        }
        let Some(root) = self.open.as_ref().map(|open| open.vault.root.clone()) else {
            return;
        };
        let inside = match confined_path(&root, rel) {
            Ok(path) => path,
            Err(err) => {
                self.status = Some(err.to_string());
                return;
            }
        };
        if !inside.is_file() {
            if self.open.as_ref().is_some_and(|open| open.editor.dirty) {
                self.prompt_disk(rel.to_path_buf());
            } else if let Some(open) = &mut self.open {
                open.editor = Editor::empty();
                self.status = Some(format!("{} disappeared from disk", rel.display()));
            }
            return;
        }
        let disk = match read_vault_text(&root, rel, MAX_OPEN_BYTES) {
            Ok(text) => text,
            Err(err) => {
                self.status = Some(format!("Watch read failed: {err}"));
                return;
            }
        };
        if self
            .open
            .as_ref()
            .is_some_and(|open| disk == open.editor.text())
        {
            return;
        }
        if self.open.as_ref().is_some_and(|open| open.editor.dirty) {
            self.prompt_disk(rel.to_path_buf());
            return;
        }
        let reloaded = self.open.as_mut().map(|open| {
            open.editor.path = Some(inside);
            open.editor.reload_from_disk()
        });
        self.status = match reloaded {
            Some(Ok(())) => Some(format!("Reloaded {}", rel.display())),
            Some(Err(err)) => Some(err.to_string()),
            None => None,
        };
    }

    fn prompt_disk(&mut self, rel: PathBuf) {
        if self.modal_up() {
            if let Some(open) = &mut self.open {
                open.pending_disk = Some(rel);
            }
            return;
        }
        self.confirm = Some(Confirm::ReloadDisk { rel });
    }

    fn flush_disk_prompt(&mut self) -> bool {
        if self.modal_up() {
            return false;
        }
        let Some(rel) = self.open.as_mut().and_then(|open| open.pending_disk.take()) else {
            return false;
        };
        let suppressed = self
            .open
            .as_ref()
            .and_then(|open| open.suppress_disk.clone());
        if suppressed.as_ref() == Some(&rel) {
            return false;
        }
        self.confirm = Some(Confirm::ReloadDisk { rel });
        true
    }

    fn poll_index(&mut self) -> bool {
        let recv = self
            .open
            .as_ref()
            .and_then(|open| open.index_rx.as_ref().map(|rx| rx.try_recv()));
        match recv {
            Some(Ok(Ok(_report))) => self.finish_reindex(),
            Some(Ok(Err(err))) => {
                if let Some(open) = &mut self.open {
                    open.index_rx = None;
                }
                self.status = Some(format!("Reindex failed: {err}"));
                self.kick_queued_reindex();
                true
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                if let Some(open) = &mut self.open {
                    open.index_rx = None;
                }
                self.kick_queued_reindex();
                false
            }
            _ => false,
        }
    }

    fn finish_reindex(&mut self) -> bool {
        let meta = self.open.as_ref().map(|open| open.vault.meta_dir.clone());
        if let Some(open) = &mut self.open {
            open.index_rx = None;
            if let Some(meta) = meta.as_ref() {
                open.index = Index::open(meta).ok();
            }
        }
        let finder = matches!(self.overlay, Some(Overlay::Finder { .. }));
        if finder {
            self.refresh_finder();
        }
        self.kick_queued_reindex();
        finder
    }

    fn kick_queued_reindex(&mut self) {
        if self.open.as_ref().is_some_and(|open| open.reindex_queued) {
            if let Some(open) = &mut self.open {
                open.reindex_queued = false;
            }
            self.spawn_reindex();
        }
    }

    fn overlay_view(&self) -> Option<OverlayView> {
        match &self.overlay {
            None => None,
            Some(Overlay::Prompt { kind, draft }) => {
                let title = match kind {
                    PromptKind::New { parent } if parent.as_os_str().is_empty() => {
                        "New file or folder".to_string()
                    }
                    PromptKind::New { parent } => format!("New in {}", parent.display()),
                    PromptKind::Rename { rel } => format!("Rename {}", rel.display()),
                    PromptKind::GitCommit { then_push: false } => "Git commit message".to_string(),
                    PromptKind::GitCommit { then_push: true } => {
                        "Commit all changes, then push".to_string()
                    }
                    PromptKind::AiProvider => {
                        format!("AI provider ({})", dd_ai::provider_ids_hint())
                    }
                    PromptKind::AiKey { .. } => {
                        format!("API key for {}", self.ai.settings.provider)
                    }
                    PromptKind::AiModel => "AI model".to_string(),
                };
                let query = match kind {
                    PromptKind::AiKey { .. } => masked_key(draft),
                    _ => draft.clone(),
                };
                Some(OverlayView {
                    title,
                    query,
                    lines: Vec::new(),
                    warn: false,
                })
            }
            Some(Overlay::Finder {
                kind,
                query,
                selected,
                hits,
            }) => {
                let title = match kind {
                    FinderKind::Files => "Files",
                    FinderKind::Content => "Search",
                    FinderKind::Tags => "Tags",
                    FinderKind::Wiki => "Wiki",
                };
                let labels = if hits.is_empty() {
                    vec!["(no matches)".to_string()]
                } else {
                    hits.iter().map(hit_label).collect()
                };
                Some(OverlayView {
                    title: title.to_string(),
                    query: query.clone(),
                    lines: windowed(&labels, *selected),
                    warn: false,
                })
            }
            Some(Overlay::Palette {
                query,
                selected,
                items,
            }) => {
                let labels = if items.is_empty() {
                    vec!["(no matches)".to_string()]
                } else {
                    items
                        .iter()
                        .map(|item| format!("{}    {}", item.label, item.keys))
                        .collect()
                };
                Some(OverlayView {
                    title: "Commands".to_string(),
                    query: query.clone(),
                    lines: windowed(&labels, *selected),
                    warn: false,
                })
            }
            Some(Overlay::Picker) => {
                let labels = if self.picker.is_empty() {
                    vec!["No registered vaults.".to_string()]
                } else {
                    self.picker
                        .iter()
                        .map(|entry| format!("{}  {}", entry.name, entry.path))
                        .collect()
                };
                Some(OverlayView {
                    title: "Vaults".to_string(),
                    query: String::new(),
                    lines: windowed(&labels, self.picker_selected),
                    warn: false,
                })
            }
            Some(Overlay::Notice { title, message }) => Some(OverlayView {
                title: title.clone(),
                query: String::new(),
                lines: message
                    .split('\n')
                    .map(|line| OverlayLine {
                        text: line.to_string(),
                        selected: false,
                    })
                    .collect(),
                warn: true,
            }),
        }
    }

    fn ai_keys_here(&self) -> bool {
        self.ai.expanded() && self.open.as_ref().is_some_and(|open| !open.preview_read)
    }

    fn busy_label(&self) -> Option<&'static str> {
        if self.git_rx.is_some() {
            return self.busy_kind.or(Some("working"));
        }
        if self.ai.streaming() {
            return Some("thinking");
        }
        None
    }

    fn ai_paint(&self) -> AiPaint {
        let limit = match self.ai.size {
            AiSize::Collapsed => 1,
            AiSize::Chat => 8,
            AiSize::Full => 16,
        };
        AiPaint {
            title: self.ai.title(),
            lines: self.ai.body_lines(limit),
            draft: self.ai.draft_line(),
            wide: self.ai.size != AiSize::Collapsed,
        }
    }

    fn handle_ai_key(&mut self, press: KeyPress<'_>) {
        let ch = typed_char(press);
        match self.ai.on_key(press.key, ch) {
            AiKey::Send(draft) => self.request_ai(draft, None),
            AiKey::Cancelled => self.status = Some("AI cancelled".into()),
            AiKey::Ignore | AiKey::Collapse | AiKey::Cycle => {}
        }
    }

    fn handle_notice_key(&mut self, press: KeyPress<'_>) {
        let close = matches!(press.key, "escape" | "enter" | "return")
            || matches!(typed_char(press), Some('n' | 'y' | 'N' | 'Y'));
        if close {
            self.overlay = None;
        }
    }

    fn refresh_git(&mut self) -> bool {
        let Some(root) = self.open.as_ref().map(|open| open.vault.root.clone()) else {
            return false;
        };
        match status(&root) {
            Ok(git) => {
                if let Some(open) = &mut self.open {
                    open.git = git;
                }
                true
            }
            Err(err) => {
                if let Some(open) = &mut self.open {
                    open.git = GitStatus::default();
                }
                self.status = Some(err.to_string());
                false
            }
        }
    }

    fn handle_git(&mut self, op: GitOp) {
        match op {
            GitOp::Status => {
                if self.refresh_git()
                    && let Some(open) = &self.open
                {
                    self.status = Some(open.git.detail());
                }
            }
            GitOp::Commit => self.begin_git_commit(false),
            GitOp::Push => {
                if !self.prepare_git_op() {
                    return;
                }
                let state = self.open.as_ref().map(|open| open.git.state);
                match state {
                    Some(dd_vault_core::GitState::Conflict) => {
                        self.status =
                            Some("unmerged files; :git pull to resolve before push".into());
                    }
                    Some(dd_vault_core::GitState::Dirty(_)) => self.begin_git_commit(true),
                    Some(_) => self.spawn_git_op("push", dd_vault_core::push),
                    None => self.status = Some("No vault open".into()),
                }
            }
            GitOp::Pull => self.spawn_git_op("pull", dd_vault_core::pull),
        }
    }

    fn prepare_git_op(&mut self) -> bool {
        if self.git_rx.is_some() {
            self.status = Some("git is already running".into());
            return false;
        }
        if self.open.is_none() {
            self.status = Some("No vault open".into());
            return false;
        }
        if self.open.as_ref().is_some_and(|open| open.editor.dirty) {
            self.save();
            if self.open.as_ref().is_some_and(|open| open.editor.dirty) {
                return false;
            }
        }
        if !self.refresh_git() {
            return false;
        }
        if self
            .open
            .as_ref()
            .is_some_and(|open| matches!(open.git.state, dd_vault_core::GitState::NotRepo))
        {
            self.status =
                Some("not a git repository (run git init in the vault to enable sync)".into());
            return false;
        }
        true
    }

    fn begin_git_commit(&mut self, then_push: bool) {
        if !self.prepare_git_op() {
            return;
        }
        if self
            .open
            .as_ref()
            .is_some_and(|open| matches!(open.git.state, dd_vault_core::GitState::Conflict))
        {
            self.status = Some("unmerged files; :git pull to resolve before commit".into());
            return;
        }
        self.overlay = Some(Overlay::Prompt {
            kind: PromptKind::GitCommit { then_push },
            draft: String::new(),
        });
    }

    fn git_parts(&mut self) -> Option<(PathBuf, Vec<String>, Option<PathBuf>)> {
        let vault = match self.open.as_ref().map(|open| open.vault.clone()) {
            Some(vault) => vault,
            None => {
                self.status = Some("No vault open".into());
                return None;
            }
        };
        let extra = load_secret_patterns(&vault, Some(&self.paths));
        let cred_path = self.paths.credentials_file();
        let cred = match usable_credentials(&cred_path) {
            Ok(path) => path,
            Err(err) => {
                self.status = Some(err.to_string());
                None
            }
        };
        Some((vault.root, extra, cred))
    }

    fn spawn_git_op(
        &mut self,
        name: &'static str,
        op: fn(&dd_vault_core::GitCtx<'_>) -> Result<GitOpResult, dd_vault_core::Error>,
    ) {
        if !self.prepare_git_op() {
            return;
        }
        let Some((root, extra, cred)) = self.git_parts() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(run_git(&root, cred.as_deref(), &extra, name, op));
        });
        self.git_rx = Some(rx);
        self.busy_kind = Some(busy_word(name));
    }

    fn submit_git_commit(&mut self, message: String, then_push: bool) {
        if !self.prepare_git_op() {
            return;
        }
        let Some((root, extra, cred)) = self.git_parts() else {
            return;
        };
        self.overlay = None;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(commit_then_maybe_push(
                &root,
                cred.as_deref(),
                &extra,
                &message,
                then_push,
            ));
        });
        self.git_rx = Some(rx);
        self.busy_kind = Some(if then_push { "pushing" } else { "committing" });
    }

    fn poll_git(&mut self) -> bool {
        let recv = self.git_rx.as_ref().map(|rx| rx.try_recv());
        match recv {
            Some(Ok(Ok((_op, result)))) => {
                self.git_rx = None;
                self.busy_kind = None;
                self.finish_git(result);
                true
            }
            Some(Ok(Err(err))) => {
                self.git_rx = None;
                self.busy_kind = None;
                self.refresh_git();
                self.status = Some(err);
                true
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.git_rx = None;
                self.busy_kind = None;
                true
            }
            _ => false,
        }
    }

    fn finish_git(&mut self, result: GitOpResult) {
        self.refresh_git();
        if let Some(open) = &mut self.open {
            reload_tree(open);
        }
        let reload_err = self.reload_open_if_clean();
        self.queue_reindex();
        if result.sidecars.is_empty() {
            self.status = Some(reload_err.unwrap_or(result.message));
            return;
        }
        let root = self
            .open
            .as_ref()
            .map(|open| open.vault.root.clone())
            .unwrap_or_default();
        let labels = sidecar_labels(&root, &result.sidecars);
        self.status = Some(reload_err.unwrap_or_else(|| result.message.clone()));
        self.overlay = Some(Overlay::Notice {
            title: "Git conflicts".into(),
            message: conflict_notice(&result.message, &labels),
        });
    }

    fn reload_open_if_clean(&mut self) -> Option<String> {
        if self
            .open
            .as_ref()
            .is_some_and(|open| open.editor.dirty || open.editor.rel.is_none())
        {
            return None;
        }
        match self
            .open
            .as_mut()
            .map(|open| open.editor.reload_from_disk())
        {
            Some(Err(err)) => Some(err.to_string()),
            _ => None,
        }
    }

    fn try_paste_image(&mut self) -> bool {
        let editor = self.open.as_ref().is_some_and(|open| {
            !open.pane_tree
                && !open.focus_preview
                && matches!(open.editor.mode, Mode::Normal | Mode::Insert)
        });
        if !editor {
            return false;
        }
        let Some((width, height, rgba)) = clipboard_image() else {
            return false;
        };
        self.paste_image(width, height, &rgba);
        true
    }

    fn paste_image(&mut self, width: u32, height: u32, rgba: &[u8]) {
        let Some(root) = self.open.as_ref().map(|open| open.vault.root.clone()) else {
            self.status = Some("No vault open".into());
            return;
        };
        if self
            .open
            .as_ref()
            .is_some_and(|open| open.editor.rel.is_none())
        {
            self.status = Some("Open a note before pasting an image".into());
            return;
        }
        let png = match encode_png_rgba(width, height, rgba) {
            Ok(png) => png,
            Err(err) => {
                self.status = Some(format!("Image encode failed: {err}"));
                return;
            }
        };
        let stem = note_stem(
            self.open
                .as_ref()
                .and_then(|open| open.editor.rel.as_deref()),
        );
        let rel = unique_asset_rel(&root, &stem);
        if let Err(err) = write_vault_file(&root, &rel, &png) {
            self.status = Some(err.to_string());
            return;
        }
        let link = image_markdown(&rel);
        if let Some(open) = &mut self.open {
            open.editor.insert_snippet(&link);
            reload_tree(open);
        }
        self.refresh_git();
        self.status = Some(format!("Pasted {}", rel.display()));
    }

    fn persist_ai(&mut self) {
        if let Err(err) = self.ai.persist(&self.paths) {
            self.status = Some(err);
        }
    }

    fn ai_enable(&mut self) {
        match self.ai.enable() {
            Ok(()) => {
                let provider = self.ai.settings.provider.clone();
                self.persist_ai();
                self.status = Some(format!("AI on ({provider})"));
            }
            Err(_) => self.prompt_ai_key(true),
        }
    }

    fn ai_disable(&mut self) {
        self.ai.disable();
        self.persist_ai();
        self.status = Some("AI off".into());
    }

    fn prompt_ai_provider(&mut self) {
        self.overlay = Some(Overlay::Prompt {
            kind: PromptKind::AiProvider,
            draft: self.ai.settings.provider.clone(),
        });
    }

    fn set_ai_provider(&mut self, name: &str) {
        match self.ai.set_provider(name) {
            Ok(outcome) => {
                let prompt_key = outcome.prompt_key;
                let enabled = self.ai.settings.enabled;
                self.persist_ai();
                self.overlay = None;
                self.status = Some(outcome.status);
                if prompt_key {
                    self.prompt_ai_key(enabled);
                }
            }
            Err(err) => self.status = Some(err),
        }
    }

    fn prompt_ai_key(&mut self, then_enable: bool) {
        self.overlay = Some(Overlay::Prompt {
            kind: PromptKind::AiKey { then_enable },
            draft: String::new(),
        });
    }

    fn submit_ai_key(&mut self, key: String, then_enable: bool) {
        match self.ai.submit_key(&key, then_enable) {
            Ok(status) => {
                self.overlay = None;
                if then_enable {
                    self.persist_ai();
                }
                self.status = Some(status);
            }
            Err(err) => self.status = Some(err),
        }
    }

    fn prompt_ai_model(&mut self) {
        self.overlay = Some(Overlay::Prompt {
            kind: PromptKind::AiModel,
            draft: self.ai.settings.resolved_model(),
        });
    }

    fn submit_ai_model(&mut self, model: String) {
        match self.ai.submit_model(&model) {
            Ok(status) => {
                self.persist_ai();
                self.overlay = None;
                self.status = Some(status);
            }
            Err(err) => self.status = Some(err),
        }
    }

    fn request_ai(&mut self, prompt: String, task: Option<dd_ai::Task>) {
        let note = self.note_snap();
        match self.ai.prepare(prompt, task, &note) {
            AiPrepare::Status(message) => self.status = Some(message),
            AiPrepare::NeedKey(message) => {
                self.status = Some(message);
                self.prompt_ai_key(false);
            }
            AiPrepare::Confirm { message } => {
                self.confirm = Some(Confirm::AiSend { message });
            }
        }
    }

    fn note_snap(&self) -> NoteSnap {
        let hops = self.hop_snippets();
        let Some(open) = &self.open else {
            return NoteSnap {
                rel: "(no note)".into(),
                text: String::new(),
                selection: None,
                replace: None,
                hops,
            };
        };
        NoteSnap {
            rel: open
                .editor
                .rel
                .clone()
                .unwrap_or_else(|| "(no note)".into()),
            text: open.editor.text(),
            selection: open.editor.selected_text(),
            replace: open.editor.visual_highlight(),
            hops,
        }
    }

    fn hop_snippets(&self) -> Vec<(String, String)> {
        let Some(open) = &self.open else {
            return Vec::new();
        };
        let rel = open
            .editor
            .rel
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_default();
        let parsed = parse_note(&rel, &open.editor.text());
        let mut hops = Vec::new();
        for link in parsed.links.iter().take(4) {
            let name = link.dst_raw.split('|').next().unwrap_or(&link.dst_raw);
            let name = name.split('#').next().unwrap_or(name).trim();
            if name.is_empty() {
                continue;
            }
            let Some(found) = open.tree.find_file(name) else {
                continue;
            };
            let Ok(body) = read_vault_text(&open.vault.root, &found, 2_000_000) else {
                continue;
            };
            let snippet: String = body.chars().take(1200).collect();
            hops.push((name.to_string(), snippet));
        }
        hops
    }

    fn start_pending_ai(&mut self) {
        let Some(log) = self.ai.start() else {
            return;
        };
        if let Some(meta) = self.open.as_ref().map(|open| open.vault.meta_dir.clone()) {
            dd_ai::log_request(
                &meta,
                &log.provider,
                log.badge,
                &log.model,
                log.task,
                &log.prompt,
            );
        }
    }

    fn poll_ai(&mut self) -> bool {
        match self.ai.poll() {
            AiPoll::Quiet => false,
            AiPoll::Changed => true,
            AiPoll::Failed(err) => {
                self.status = Some(err);
                true
            }
            AiPoll::Insert(insert) => {
                self.apply_ai_insert(insert);
                true
            }
        }
    }

    fn apply_ai_insert(&mut self, insert: crate::ai_card::AiInsert) {
        let Some(open) = &mut self.open else {
            return;
        };
        let note_open = open.editor.rel.is_some();
        match insert.replace {
            Some((start, end)) if insert.rewrite => {
                open.editor.replace_range(start, end, &insert.text);
            }
            _ => {
                if !insert.text.starts_with('\n') && note_open {
                    open.editor.insert_snippet(&format!("\n{}", insert.text));
                } else {
                    open.editor.insert_snippet(&insert.text);
                }
            }
        }
        self.status = Some("AI inserted".into());
    }

    fn theme_open(&self) -> bool {
        self.theme_picker.is_some() || self.theme_editor.is_some()
    }

    fn open_help(&mut self) {
        if self.theme_open() {
            self.close_theme(true);
        }
        self.help = true;
        self.help_scroll = 0;
        self.leader = Leader::None;
    }

    fn handle_help_key(&mut self, press: KeyPress<'_>) -> Effect {
        if press.control {
            return Effect::None;
        }
        let max = self.help_scroll_max();
        if is_fn(press, 1) || press.key == "escape" {
            self.help = false;
            self.help_scroll = 0;
            return Effect::None;
        }
        if press.key == "end" || press.key_char == Some("G") {
            self.help_scroll = max;
        } else if matches!(press.key, "down" | "j") || press.key_char == Some("j") {
            self.help_scroll = self.help_scroll.saturating_add(1).min(max);
        } else if matches!(press.key, "up" | "k") || press.key_char == Some("k") {
            self.help_scroll = self.help_scroll.saturating_sub(1);
        } else if matches!(press.key, "pagedown" | "page_down") {
            self.help_scroll = self.help_scroll.saturating_add(10).min(max);
        } else if matches!(press.key, "pageup" | "page_up") {
            self.help_scroll = self.help_scroll.saturating_sub(10);
        } else if press.key == "home" || press.key == "g" || press.key_char == Some("g") {
            self.help_scroll = 0;
        }
        Effect::None
    }

    fn help_scroll_max(&self) -> usize {
        help_lines().len().saturating_sub(self.help_view.max(1))
    }

    fn help_page(&self) -> Vec<String> {
        let lines = help_lines();
        let start = self.help_scroll.min(lines.len().saturating_sub(1));
        lines
            .into_iter()
            .skip(start)
            .take(self.help_view.max(1))
            .collect()
    }

    fn scroll_help(&mut self, rows: isize) {
        let max = self.help_scroll_max() as isize;
        let next = self.help_scroll as isize + rows;
        self.help_scroll = next.clamp(0, max) as usize;
    }

    fn open_theme(&mut self) {
        self.help = false;
        self.leader = Leader::None;
        self.font_snapshot = (self.mono.clone(), self.sans.clone());
        self.families = installed_families();
        self.ensure_faces_listed();
        self.appear = AppearSection::Theme;
        self.font_filter = None;
        self.family_selected = 0;
        self.family_scroll = 0;
        let picker = ThemePicker::new(
            self.palette.clone(),
            extra_theme_fields(),
            self.omarchy.clone(),
        );
        if picker.lists_omarchy() {
            self.theme_picker = Some(picker);
            self.theme_editor = None;
        } else {
            self.theme_picker = None;
            self.theme_editor = Some(ThemeEditor::new(self.palette.clone(), extra_theme_fields()));
        }
    }

    fn handle_theme_key(&mut self, press: KeyPress<'_>) -> Effect {
        if !press.control && is_fn(press, 2) {
            self.close_theme(true);
            return Effect::None;
        }
        if self.font_filter.is_some() && self.appear != AppearSection::Theme {
            return self.handle_font_filter(press);
        }
        if self.appear != AppearSection::Theme {
            return self.handle_font_section(press);
        }
        if !press.control && press.key_char == Some("f") && !self.editing_hex() {
            self.cycle_appear();
            return Effect::None;
        }
        let Some(key) = editor_key(press.key, press.key_char, press.control) else {
            return Effect::None;
        };
        if self.theme_editor.is_some() {
            self.dispatch_theme_editor(key, press.shift);
        } else {
            self.dispatch_theme_picker(key);
        }
        Effect::None
    }

    fn dispatch_theme_picker(&mut self, key: EditorKey) {
        let outcome = {
            let Some(picker) = self.theme_picker.as_mut() else {
                return;
            };
            picker.handle(key)
        };
        match outcome {
            PickerOutcome::Applied => {
                let palette = self
                    .theme_picker
                    .as_ref()
                    .map(|picker| picker.palette.clone());
                if let Some(palette) = palette {
                    self.apply_live_palette(palette);
                }
            }
            PickerOutcome::EditColors => {
                let Some(picker) = self.theme_picker.as_ref() else {
                    return;
                };
                let mut editor = ThemeEditor::new(picker.palette.clone(), extra_theme_fields());
                editor.save_target = picker.save_target;
                self.theme_editor = Some(editor);
            }
            PickerOutcome::RequestSave => self.request_save_theme(),
            PickerOutcome::Error(msg) => push_toast(&mut self.toasts, ToastLevel::Error, msg),
            PickerOutcome::Closed { .. } => self.close_theme(true),
            PickerOutcome::None => {
                if theme_motion(key) {
                    self.preview_theme_row();
                }
            }
        }
    }

    fn dispatch_theme_editor(&mut self, key: EditorKey, shift: bool) {
        let outcome = {
            let Some(editor) = self.theme_editor.as_mut() else {
                return;
            };
            editor.handle(key, shift)
        };
        match outcome {
            EditorOutcome::PaletteChanged => {
                let changed = self
                    .theme_editor
                    .as_ref()
                    .map(|editor| (editor.palette.clone(), editor.save_target));
                if let Some((palette, target)) = changed {
                    if let Some(picker) = self.theme_picker.as_mut() {
                        picker.palette = palette.clone();
                        picker.save_target = target;
                    }
                    self.apply_live_palette(palette);
                }
            }
            EditorOutcome::RequestSave => self.request_save_theme(),
            EditorOutcome::Closed { .. } => {
                if self.theme_picker.is_some() {
                    if let Some(editor) = self.theme_editor.take() {
                        if let Some(picker) = self.theme_picker.as_mut() {
                            picker.palette = editor.palette.clone();
                            picker.save_target = editor.save_target;
                        }
                        self.apply_live_palette(editor.palette);
                    }
                } else {
                    self.close_theme(true);
                }
            }
            EditorOutcome::HexError(msg) => push_toast(&mut self.toasts, ToastLevel::Error, msg),
            EditorOutcome::None => {}
        }
    }

    fn close_theme(&mut self, revert: bool) {
        if revert {
            let reverted = self
                .theme_picker
                .as_ref()
                .map(|picker| picker.revert_palette());
            if let Some(palette) = reverted {
                self.apply_live_palette(palette);
            } else if let Some(mut editor) = self.theme_editor.clone() {
                editor.revert();
                self.apply_live_palette(editor.palette);
            }
            let mono = self.font_snapshot.0.clone();
            let sans = self.font_snapshot.1.clone();
            if mono != self.mono || sans != self.sans {
                self.mono = mono;
                self.sans = sans;
                self.note_font_change();
            }
        }
        self.theme_picker = None;
        self.theme_editor = None;
        self.appear = AppearSection::Theme;
        self.font_filter = None;
    }

    fn apply_live_palette(&mut self, palette: Palette) {
        let resolved = apply_omarchy(&palette, &self.omarchy).unwrap_or(palette);
        if self.palette == resolved {
            return;
        }
        self.palette = resolved.clone();
        self.theme = from_palette(resolved);
        self.preview_cache = None;
    }

    fn save_theme_palette(&mut self, mut palette: Palette, target: ThemeSaveTarget) {
        palette.source = match target {
            ThemeSaveTarget::Local => ThemeSource::Local,
            ThemeSaveTarget::Global => ThemeSource::Global,
        };
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let fields = self.theme_fields();
        match ldnddev_theme::save_theme(
            &palette,
            &root,
            colors::THEME_FILENAME,
            target,
            default_config_home().as_deref(),
            &fields,
        ) {
            Ok(path) => {
                self.commit_fonts();
                self.apply_live_palette(palette);
                self.theme_editor = None;
                self.theme_picker = None;
                self.appear = AppearSection::Theme;
                self.font_filter = None;
                push_toast(
                    &mut self.toasts,
                    ToastLevel::Success,
                    format!("Theme saved ({})", path.display()),
                );
            }
            Err(err) => push_toast(
                &mut self.toasts,
                ToastLevel::Error,
                format!("Theme save failed: {err}"),
            ),
        }
    }

    fn theme_fields(&self) -> Vec<ColorField> {
        if let Some(editor) = &self.theme_editor {
            return editor.fields.clone();
        }
        let mut fields = COLOR_FIELDS.to_vec();
        for extra in extra_theme_fields() {
            if !fields.iter().any(|field| field.key == extra.key) {
                fields.push(*extra);
            }
        }
        fields
    }

    fn appearance_view(&self) -> AppearanceView {
        let rows = self.sheet_list_rows();
        let (title, hint, lines) = if self.appear != AppearSection::Theme {
            self.font_section_view(rows)
        } else if let Some(editor) = &self.theme_editor {
            (
                "Appearance · colors".into(),
                format!(
                    "save {}    Tab target    y save    R reset    f fonts    Esc revert",
                    editor.save_target.label()
                ),
                color_lines(editor, rows),
            )
        } else if let Some(picker) = &self.theme_picker {
            (
                "Appearance".into(),
                format!(
                    "j/k live    e colors    y save {}    f fonts    Esc revert",
                    picker.save_target.label()
                ),
                theme_lines(picker, rows),
            )
        } else {
            ("Appearance".into(), String::new(), Vec::new())
        };
        AppearanceView {
            title,
            hint,
            lines,
            editor_font: self.mono.clone(),
            preview_font: self.sans.clone(),
            editor_on: self.appear == AppearSection::Mono,
            preview_on: self.appear == AppearSection::Sans,
        }
    }

    fn font_section_view(&self, rows: usize) -> (String, String, Vec<AppearLine>) {
        let choices = self.font_choices();
        let live = self.live_family();
        let title = match self.appear {
            AppearSection::Sans => "Appearance · preview font",
            _ => "Appearance · editor font",
        };
        let hint = if let Some(query) = &self.font_filter {
            format!("/{query}    arrows move    Enter keep    Esc clear")
        } else {
            "j/k live    Enter keep    / filter    f cycle    h back    s save".into()
        };
        let start = self.family_scroll.min(choices.len());
        let mut lines: Vec<AppearLine> = choices
            .iter()
            .enumerate()
            .skip(start)
            .take(rows)
            .map(|(index, name)| AppearLine {
                text: name.clone(),
                selected: name == &live,
                action: AppearAction::Family(index),
            })
            .collect();
        if lines.is_empty() {
            lines.push(AppearLine {
                text: "(no matching fonts)".into(),
                selected: false,
                action: AppearAction::None,
            });
        }
        (title.into(), hint, lines)
    }

    fn sheet_list_rows(&self) -> usize {
        self.help_view.max(10).saturating_sub(5).max(3)
    }

    fn scroll_theme(&mut self, rows: isize) {
        if rows == 0 {
            return;
        }
        if self.appear != AppearSection::Theme {
            self.nudge_font(rows);
            return;
        }
        let delta = rows as i32;
        if let Some(editor) = self.theme_editor.as_mut() {
            let next = editor.selected as i32 + delta;
            editor.select(next.max(0) as usize);
            return;
        }
        let changed = if let Some(picker) = self.theme_picker.as_mut() {
            let before = picker.selected;
            let next = picker.selected as i32 + delta;
            picker.select(next.max(0) as usize);
            picker.selected != before
        } else {
            false
        };
        if changed {
            self.preview_theme_row();
        }
    }

    fn preview_theme_row(&mut self) {
        let outcome = {
            let Some(picker) = self.theme_picker.as_mut() else {
                return;
            };
            picker.preview_selected()
        };
        match outcome {
            PickerOutcome::Applied => {
                let palette = self
                    .theme_picker
                    .as_ref()
                    .map(|picker| picker.palette.clone());
                if let Some(palette) = palette {
                    self.apply_live_palette(palette);
                }
            }
            PickerOutcome::Error(msg) => push_toast(&mut self.toasts, ToastLevel::Error, msg),
            _ => {}
        }
    }

    fn pick_theme_row(&mut self, index: usize) {
        if let Some(picker) = self.theme_picker.as_mut() {
            picker.select(index);
        } else {
            return;
        }
        self.preview_theme_row();
    }

    fn request_save_theme(&mut self) {
        let saved = if let Some(editor) = &self.theme_editor {
            Some((editor.palette.clone(), editor.save_target))
        } else {
            self.theme_picker
                .as_ref()
                .map(|picker| (picker.palette.clone(), picker.save_target))
        };
        if let Some((palette, target)) = saved {
            self.save_theme_palette(palette, target);
        }
    }

    fn editing_hex(&self) -> bool {
        self.theme_editor
            .as_ref()
            .is_some_and(|editor| editor.editing_hex)
    }

    fn gui_config_path(&self) -> PathBuf {
        self.paths.ldnddev_dir().join("config.toml")
    }

    fn fonts_dirty(&self) -> bool {
        self.mono != self.font_saved.0 || self.sans != self.font_saved.1
    }

    fn commit_fonts(&mut self) -> bool {
        if !self.fonts_dirty() {
            self.font_snapshot = (self.mono.clone(), self.sans.clone());
            return true;
        }
        let fonts = GuiFonts {
            mono: self.mono.clone(),
            sans: self.sans.clone(),
        };
        match fonts.save(&self.gui_config_path()) {
            Ok(()) => {
                self.font_saved = (self.mono.clone(), self.sans.clone());
                self.font_snapshot = self.font_saved.clone();
                true
            }
            Err(err) => {
                push_toast(
                    &mut self.toasts,
                    ToastLevel::Error,
                    format!("Font save failed: {err}"),
                );
                false
            }
        }
    }

    fn keep_fonts(&mut self) {
        let dirty = self.fonts_dirty();
        if !self.commit_fonts() {
            return;
        }
        if dirty {
            push_toast(&mut self.toasts, ToastLevel::Success, "Fonts saved");
        }
        self.font_filter = None;
        self.appear = AppearSection::Theme;
    }

    fn note_font_change(&mut self) {
        self.preview_cache = None;
        self.char_w = px(0.);
        self.measured_mono.clear();
    }

    fn ensure_faces_listed(&mut self) {
        for name in [self.mono.clone(), self.sans.clone()] {
            if !name.is_empty() && !self.families.iter().any(|have| have == &name) {
                self.families.push(name);
            }
        }
        self.families.sort_by_key(|name| name.to_lowercase());
    }

    fn live_family(&self) -> String {
        match self.appear {
            AppearSection::Sans => self.sans.clone(),
            _ => self.mono.clone(),
        }
    }

    fn font_choices(&self) -> Vec<String> {
        let Some(query) = self.font_filter.as_deref() else {
            return self.families.clone();
        };
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return self.families.clone();
        }
        self.families
            .iter()
            .filter(|name| name.to_lowercase().contains(&query))
            .cloned()
            .collect()
    }

    fn retarget_font_highlight(&mut self) {
        let choices = self.font_choices();
        let live = self.live_family();
        self.family_selected = choices.iter().position(|name| name == &live).unwrap_or(0);
        self.family_scroll = 0;
        self.reveal_family();
    }

    fn reveal_family(&mut self) {
        let span = self.sheet_list_rows().saturating_sub(1);
        if self.family_selected < self.family_scroll {
            self.family_scroll = self.family_selected;
        } else if self.family_selected > self.family_scroll + span {
            self.family_scroll = self.family_selected - span;
        }
    }

    fn apply_font_choice(&mut self, name: &str) {
        let slot = match self.appear {
            AppearSection::Sans => &mut self.sans,
            AppearSection::Mono => &mut self.mono,
            AppearSection::Theme => return,
        };
        if slot == name {
            return;
        }
        *slot = name.to_string();
        self.note_font_change();
    }

    fn nudge_font(&mut self, delta: isize) {
        let choices = self.font_choices();
        if choices.is_empty() {
            return;
        }
        let last = choices.len() - 1;
        let live = self.live_family();
        let base = choices.iter().position(|name| name == &live);
        let next = match (delta, base) {
            (isize::MAX, _) => last,
            (isize::MIN, _) => 0,
            (_, Some(index)) => {
                let cur = index as isize + delta;
                cur.clamp(0, last as isize) as usize
            }
            (_, None) if delta < 0 => last,
            (_, None) => 0,
        };
        self.family_selected = next;
        self.reveal_family();
        let name = choices[next].clone();
        self.apply_font_choice(&name);
    }

    fn choose_family(&mut self, index: usize) {
        let choices = self.font_choices();
        let Some(name) = choices.get(index).cloned() else {
            return;
        };
        self.family_selected = index;
        self.reveal_family();
        self.apply_font_choice(&name);
    }

    fn cycle_appear(&mut self) {
        let next = match self.appear {
            AppearSection::Theme => AppearSection::Mono,
            AppearSection::Mono => AppearSection::Sans,
            AppearSection::Sans => AppearSection::Theme,
        };
        self.font_filter = None;
        if next == AppearSection::Theme {
            self.appear = AppearSection::Theme;
        } else {
            self.focus_fonts(next);
        }
    }

    fn focus_fonts(&mut self, section: AppearSection) {
        self.appear = section;
        self.font_filter = None;
        self.retarget_font_highlight();
    }

    fn handle_font_section(&mut self, press: KeyPress<'_>) -> Effect {
        if press.key == "escape" {
            self.close_theme(true);
            return Effect::None;
        }
        if press.control {
            return Effect::None;
        }
        if press.key_char == Some("f") {
            self.cycle_appear();
            return Effect::None;
        }
        if press.key == "/" || press.key_char == Some("/") {
            self.font_filter = Some(String::new());
            self.retarget_font_highlight();
            return Effect::None;
        }
        if matches!(press.key, "left" | "h") || press.key_char == Some("h") {
            self.appear = AppearSection::Theme;
            self.font_filter = None;
            return Effect::None;
        }
        if press.key == "enter" || press.key == "return" {
            self.keep_fonts();
            return Effect::None;
        }
        if matches!(
            press.key_char,
            Some("s") | Some("S") | Some("y") | Some("Y")
        ) {
            self.request_save_theme();
            return Effect::None;
        }
        if let Some(delta) = font_delta(press) {
            self.nudge_font(delta);
        }
        Effect::None
    }

    fn handle_font_filter(&mut self, press: KeyPress<'_>) -> Effect {
        if press.control {
            return Effect::None;
        }
        if press.key == "escape" {
            self.font_filter = None;
            self.retarget_font_highlight();
            return Effect::None;
        }
        if press.key == "enter" || press.key == "return" {
            self.keep_fonts();
            return Effect::None;
        }
        if press.key == "backspace" {
            if let Some(query) = self.font_filter.as_mut() {
                query.pop();
            }
            self.retarget_font_highlight();
            return Effect::None;
        }
        if let Some(delta) = font_arrow_delta(press) {
            self.nudge_font(delta);
            return Effect::None;
        }
        if let Some(text) = press.key_char.filter(|text| text.chars().count() == 1)
            && let Some(ch) = text.chars().next()
            && !ch.is_control()
        {
            if let Some(query) = self.font_filter.as_mut() {
                query.push(ch);
            }
            self.retarget_font_highlight();
        }
        Effect::None
    }

    fn appear_click(&mut self, action: AppearAction) {
        match action {
            AppearAction::None => {}
            AppearAction::Theme(index) => self.pick_theme_row(index),
            AppearAction::Color(index) => {
                if let Some(editor) = self.theme_editor.as_mut() {
                    editor.select(index);
                }
            }
            AppearAction::Family(index) => self.choose_family(index),
            AppearAction::Focus(section) => {
                self.font_filter = None;
                if section == AppearSection::Theme {
                    self.appear = AppearSection::Theme;
                } else {
                    self.focus_fonts(section);
                }
            }
        }
    }

    fn split_at(&self, pos: Point<Pixels>) -> Option<SplitDrag> {
        if let Some(bounds) = self.hit.tree_pane {
            let edge = bounds.origin.x + bounds.size.width;
            let dx = ((pos.x - edge) / px(1.)).abs();
            let y = pos.y;
            if y >= bounds.origin.y && y <= bounds.origin.y + bounds.size.height && dx <= 6.0 {
                return Some(SplitDrag::Tree);
            }
        }
        if let (Some(preview), Some(notes)) = (self.hit.preview_pane, self.hit.notes_pane) {
            let edge = preview.origin.y;
            let dy = ((pos.y - edge) / px(1.)).abs();
            let x0 = notes.origin.x.min(preview.origin.x);
            let right = notes.origin.x + notes.size.width;
            let preview_right = preview.origin.x + preview.size.width;
            let x1 = if right > preview_right {
                right
            } else {
                preview_right
            };
            if pos.x >= x0 && pos.x <= x1 && dy <= 6.0 {
                return Some(SplitDrag::Preview);
            }
        }
        None
    }

    fn drag_split(&mut self, kind: SplitDrag, pos: Point<Pixels>) {
        let char_w = (self.char_w / px(1.)).max(1.0);
        let line = (self.line_h / px(1.)).max(1.0);
        match kind {
            SplitDrag::Tree => {
                let Some(bounds) = self.hit.tree_pane else {
                    return;
                };
                let x = (pos.x - bounds.origin.x) / px(1.);
                let cols = (x / char_w).round().max(0.0) as u16;
                let max = self.body_cols.saturating_sub(20).max(12);
                if let Some(open) = &mut self.open {
                    open.tree_split = cols.clamp(12, max);
                }
            }
            SplitDrag::Preview => {
                let Some(bounds) = self.hit.preview_pane else {
                    return;
                };
                let bottom = (bounds.origin.y + bounds.size.height) / px(1.);
                let y = pos.y / px(1.);
                let rows = ((bottom - y) / line).round().max(0.0) as u16;
                let notes_h = self
                    .hit
                    .notes_pane
                    .map(|pane| pane.size.height / px(1.))
                    .unwrap_or(0.0);
                let column = ((notes_h + bounds.size.height / px(1.)) / line)
                    .floor()
                    .max(4.0) as u16;
                let max = column.saturating_sub(4).max(4);
                if let Some(open) = &mut self.open {
                    open.preview_split = rows.clamp(4, max);
                }
            }
        }
    }
}

impl Overlay {
    fn clone_kind(&self) -> Option<(PromptKind, String)> {
        match self {
            Overlay::Prompt { kind, draft } => Some((kind.clone(), draft.clone())),
            _ => None,
        }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl gpui::IntoElement {
        if !self.focus.is_focused(window) {
            self.focus.focus(window, cx);
        }
        let frame = self.frame(window);
        let theme = self.theme.clone();
        let focus = self.focus.clone();
        let entity = cx.entity();
        let notice = self.notice.clone();
        let picker = self.picker.clone();
        let picker_selected = self.picker_selected;
        let tree_edge = self.hit.tree_pane;
        let preview_edge = self.hit.preview_pane;
        let notes_edge = self.hit.notes_pane;
        div()
            .id("shell")
            .track_focus(&focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_scroll(cx.listener(Self::wheel))
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(theme.base_background))
            .text_color(rgb(theme.text_primary))
            .font_family(self.mono.clone())
            .text_size(px(FONT))
            .line_height(px(LINE))
            .child(header(&theme, &frame))
            .child(match &frame.body {
                Body::Picker => picker_body(&theme, &picker, picker_selected, notice),
                Body::Vault(panes) => vault_body(&theme, panes, entity.clone()),
            })
            .when(frame.ai_float.is_some(), |this| {
                this.child(ai_float_card(
                    &theme,
                    frame.ai_float.clone().unwrap_or(AiPaint {
                        title: String::new(),
                        lines: Vec::new(),
                        draft: None,
                        wide: false,
                    }),
                ))
            })
            .when(frame.overlay.is_some(), |this| {
                this.child(overlay_panel(
                    &theme,
                    frame.overlay.clone().unwrap_or_else(|| OverlayView {
                        title: String::new(),
                        query: String::new(),
                        lines: Vec::new(),
                        warn: false,
                    }),
                ))
            })
            .when(frame.confirm.is_some(), |this| {
                this.child(confirm_bar(
                    &theme,
                    frame.confirm.as_deref().unwrap_or_default(),
                ))
            })
            .when(frame.help.is_some(), |this| {
                this.child(help_panel(
                    &theme,
                    frame.help.as_deref().unwrap_or_default(),
                ))
            })
            .when(frame.theme_panel.is_some(), |this| {
                this.child(appearance_sheet(
                    &theme,
                    frame.theme_panel.as_ref().unwrap_or(&AppearanceView {
                        title: String::new(),
                        hint: String::new(),
                        lines: Vec::new(),
                        editor_font: String::new(),
                        preview_font: String::new(),
                        editor_on: false,
                        preview_on: false,
                    }),
                    entity.clone(),
                ))
            })
            .when(!frame.toasts.is_empty(), |this| {
                let lift = if frame.confirm.is_some() {
                    LINE * 8. + 12.
                } else {
                    0.
                };
                this.child(toast_stack(&theme, &frame.toasts, lift))
            })
            .child(footer(&theme, &frame.footer))
            .when(frame.live_pointer, |shell| {
                shell.child(resize_strips(tree_edge, preview_edge, notes_edge))
            })
            .when(frame.split_cursor == Some(SplitDrag::Tree), |shell| {
                shell.child(div().absolute().size_full().cursor_col_resize())
            })
            .when(frame.split_cursor == Some(SplitDrag::Preview), |shell| {
                shell.child(div().absolute().size_full().cursor_row_resize())
            })
    }
}

fn header(theme: &GuiTheme, frame: &Frame) -> gpui::Div {
    div()
        .h(px(CHROME))
        .flex()
        .flex_row()
        .items_center()
        .px(px(8.))
        .bg(rgb(theme.body_background))
        .border_b_1()
        .border_color(rgb(theme.border_default))
        .child(div().child("dd_vault"))
        .child(
            div()
                .flex_1()
                .px(px(12.))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(rgb(theme.text_secondary))
                .child(theme.quote.clone()),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .text_color(rgb(theme.text_labels))
                .when(frame.busy.is_some(), |row| {
                    row.child(
                        div()
                            .text_color(rgb(theme.warning))
                            .pr(px(12.))
                            .child(frame.busy.unwrap_or("").to_string()),
                    )
                })
                .child(frame.header_right.clone()),
        )
}

fn footer(theme: &GuiTheme, text: &str) -> gpui::Div {
    div()
        .h(px(CHROME))
        .flex()
        .items_center()
        .px(px(8.))
        .bg(rgb(theme.body_background))
        .border_t_1()
        .border_color(rgb(theme.border_default))
        .text_color(rgb(theme.text_secondary))
        .overflow_hidden()
        .whitespace_nowrap()
        .child(text.to_string())
}

fn confirm_bar(theme: &GuiTheme, message: &str) -> gpui::Div {
    let mut bar = div()
        .px(px(8.))
        .py(px(6.))
        .max_h(px(LINE * 8.))
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(rgb(theme.modal_background))
        .text_color(rgb(theme.modal_header))
        .border_t_1()
        .border_color(rgb(theme.border_active));
    for line in message.split('\n') {
        let shown = if line.is_empty() {
            " ".to_string()
        } else {
            line.to_string()
        };
        bar = bar.child(
            div()
                .h(px(LINE))
                .overflow_hidden()
                .whitespace_nowrap()
                .child(shown),
        );
    }
    bar
}

fn picker_body(
    theme: &GuiTheme,
    entries: &[dd_vault_core::VaultEntry],
    selected: usize,
    notice: Option<String>,
) -> gpui::Div {
    let mut list = div().flex_1().flex().flex_col().overflow_hidden().p(px(8.));
    if entries.is_empty() {
        return list.child(div().text_color(rgb(theme.text_secondary)).child(
            notice.unwrap_or_else(|| "No vaults yet. Run dd_vault-gui init <path>.".into()),
        ));
    }
    if let Some(notice) = notice {
        list = list.child(div().text_color(rgb(theme.warning)).child(notice));
    }
    list.child("Open a vault")
        .children(entries.iter().enumerate().map(|(i, entry)| {
            let marked = i == selected;
            div()
                .h(px(LINE))
                .overflow_hidden()
                .whitespace_nowrap()
                .when(marked, |row| {
                    row.bg(rgb(theme.selected_background))
                        .text_color(rgb(theme.text_active_focus))
                })
                .child(format!("{}  {}", entry.name, entry.path))
        }))
}

fn vault_body(theme: &GuiTheme, panes: &VaultPanes, entity: Entity<Shell>) -> gpui::Div {
    let mut row = div().flex_1().flex().flex_row().overflow_hidden();
    if let Some(tree) = &panes.tree {
        row = row.child(tree_pane(theme, tree, entity.clone()));
    }
    row.child(editor_column(
        theme,
        &panes.notes,
        &panes.preview,
        &panes.ai,
        entity,
    ))
}

fn editor_column(
    theme: &GuiTheme,
    notes: &Option<NotesView>,
    preview: &Option<PreviewView>,
    ai: &Option<AiPaint>,
    entity: Entity<Shell>,
) -> gpui::Div {
    let mut column = div()
        .flex_1()
        .h_full()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .overflow_hidden();
    let split = preview.is_some() || ai.is_some();
    if let Some(notes) = notes {
        let grow = if split { 3.0 } else { 1.0 };
        column = column.child(
            notes_pane(theme, notes, entity.clone())
                .flex_grow(grow)
                .flex_shrink(1.)
                // An auto basis grows with the document, the viewport becomes
                // the content, and the wheel clamps back to the top.
                .flex_basis(px(0.))
                .min_h(px(0.)),
        );
    }
    if let Some(preview) = preview {
        let pane = preview_pane(theme, preview, entity);
        column = if let Some(height) = preview.fixed_h {
            column.child(pane.h(px(height)).flex_shrink(0.).min_h(px(0.)))
        } else {
            let grow = if notes.is_some() { 2.0 } else { 1.0 };
            column.child(
                pane.flex_grow(grow)
                    .flex_shrink(1.)
                    .flex_basis(px(0.))
                    .min_h(px(0.)),
            )
        };
    }
    if let Some(ai) = ai {
        let grow = if notes.is_some() { 2.0 } else { 1.0 };
        column = column.child(
            ai_pane(theme, ai)
                .flex_grow(grow)
                .flex_shrink(1.)
                .flex_basis(px(0.))
                .min_h(px(0.)),
        );
    }
    column
}

fn ai_pane(theme: &GuiTheme, card: &AiPaint) -> gpui::Div {
    ai_card_body(theme, card)
        .flex_1()
        .min_w(px(0.))
        .border_t_1()
        .border_color(rgb(theme.border_default))
        .bg(rgb(theme.body_background))
}

fn ai_float_card(theme: &GuiTheme, card: AiPaint) -> gpui::Div {
    let cols = if card.wide { 40.0 } else { 22.0 };
    let rows = if card.wide { 12.0 } else { 3.0 };
    ai_card_body(theme, &card)
        .absolute()
        .bottom(px(CHROME + 8.))
        .right(px(12.))
        .w(px(8. * cols))
        .max_h(px(LINE * rows))
        .border_1()
        .border_color(rgb(theme.border_active))
        .bg(rgb(theme.modal_background))
}

fn ai_card_body(theme: &GuiTheme, card: &AiPaint) -> gpui::Div {
    let mut panel = div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .px(px(8.))
        .py(px(4.))
        .child(
            div()
                .h(px(LINE))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(rgb(theme.text_labels))
                .child(card.title.clone()),
        );
    for line in &card.lines {
        let shown = if line.is_empty() {
            " ".to_string()
        } else {
            line.clone()
        };
        panel = panel.child(
            div()
                .h(px(LINE))
                .overflow_hidden()
                .whitespace_nowrap()
                .child(shown),
        );
    }
    if let Some(draft) = &card.draft {
        panel = panel.child(
            div()
                .h(px(LINE))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(rgb(theme.text_labels))
                .child(draft.clone()),
        );
    }
    panel
}

fn tree_pane(theme: &GuiTheme, tree: &TreeView, entity: Entity<Shell>) -> gpui::Div {
    let title_color = if tree.focused {
        theme.text_active_focus
    } else {
        theme.text_secondary
    };
    div()
        .w(px(tree.width.max(1.0)))
        .h_full()
        .relative()
        .flex()
        .flex_col()
        .overflow_hidden()
        .border_r_1()
        .border_color(rgb(theme.border_default))
        .bg(rgb(theme.body_background))
        .child(pane_title(&tree.title, title_color, theme))
        .child(tree_rows(theme, tree, entity.clone()))
        .child(pane_bounds(entity, |shell, bounds| {
            shell.hit.tree_pane = Some(bounds)
        }))
        .when(tree.filter.is_some(), |pane| {
            pane.child(
                div()
                    .h(px(LINE))
                    .px(px(6.))
                    .text_color(rgb(theme.text_labels))
                    .child(tree.filter.clone().unwrap_or_default()),
            )
        })
}

fn scroll_document(height: f32) -> gpui::Div {
    let height = height.max(1.0);
    div().relative().w_full().h(px(height)).min_h(px(height))
}

/// Absolute, so a tall document cannot grow the pane. The wheel updates
/// `handle`; the next frame copies that offset into the model.
fn y_scroll(
    id: &'static str,
    handle: &ScrollHandle,
    document: gpui::Div,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .absolute()
        .size_full()
        .overflow_y_scroll()
        .line_height(px(LINE))
        .track_scroll(handle)
        .child(document)
}

fn tree_rows(theme: &GuiTheme, tree: &TreeView, entity: Entity<Shell>) -> gpui::Div {
    let mut document = scroll_document(tree.content_h);
    for (i, row) in tree.rows.iter().enumerate() {
        let top = (tree.origin + i) as f32 * LINE;
        document = document.child(
            div()
                .absolute()
                .top(px(top))
                .left(px(0.))
                .right(px(0.))
                .h(px(LINE))
                .flex_shrink_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .pl(px(4.))
                .pr(px(4. + BAR_W))
                .when(row.selected && tree.focused, |line| {
                    line.bg(rgb(theme.selected_background))
                })
                .text_color(rgb(row.color))
                .child(row.label.clone()),
        );
    }
    div()
        .flex_1()
        .min_h(px(0.))
        .relative()
        .overflow_hidden()
        .child(y_scroll("tree-scroll", &tree.port, document))
        .when(tree.bar.is_some(), |area| {
            area.child(scroll_bar(
                theme,
                tree.bar.as_ref().unwrap(),
                entity.clone(),
            ))
        })
        .child(
            canvas(
                move |bounds, _, cx| {
                    entity.update(cx, |shell, _| shell.hit.tree = Some(bounds));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
}

fn notes_pane(theme: &GuiTheme, notes: &NotesView, entity: Entity<Shell>) -> gpui::Div {
    let title_color = if notes.focused {
        theme.text_active_focus
    } else {
        theme.text_secondary
    };
    div()
        .min_w(px(0.))
        .relative()
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(pane_title(&notes.title, title_color, theme))
        .child(notes_rows(theme, notes, entity.clone()))
        .child(pane_bounds(entity, |shell, bounds| {
            shell.hit.notes_pane = Some(bounds);
        }))
        .when(notes.prompt.is_some(), |pane| {
            pane.child(
                div()
                    .h(px(LINE))
                    .px(px(6.))
                    .bg(rgb(theme.modal_background))
                    .text_color(rgb(theme.modal_text))
                    .child(notes.prompt.clone().unwrap_or_default()),
            )
        })
}

fn notes_rows(theme: &GuiTheme, notes: &NotesView, entity: Entity<Shell>) -> gpui::Div {
    let mut document = scroll_document(notes.content_h);
    for (i, row) in notes.rows.iter().enumerate() {
        let top = (notes.origin + i) as f32 * LINE;
        document = document.child(
            note_line(theme, row)
                .absolute()
                .top(px(top))
                .left(px(0.))
                .right(px(0.)),
        );
    }
    let mut area = div()
        .flex_1()
        .min_h(px(0.))
        .relative()
        .overflow_hidden()
        .child(y_scroll("notes-scroll", &notes.port, document));
    if let Some(origin) = notes.caret_bar {
        area = area.child(
            div()
                .absolute()
                .left(origin.x)
                .top(origin.y)
                .w(px(2.))
                .h(px(LINE))
                .bg(rgb(theme.cursor)),
        );
    }
    if let Some(bar) = &notes.bar {
        area = area.child(scroll_bar(theme, bar, entity.clone()));
    }
    area.child(
        canvas(
            move |bounds, _, cx| {
                entity.update(cx, |shell, _| shell.hit.notes = Some(bounds));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full(),
    )
}

fn note_line(theme: &GuiTheme, row: &EditorRow) -> gpui::Div {
    div()
        .h(px(LINE))
        .flex_shrink_0()
        .flex()
        .flex_row()
        .overflow_hidden()
        .child(
            div()
                .text_color(rgb(theme.text_disabled))
                .child(row.gutter.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .pr(px(BAR_W))
                .cursor_text()
                .child(styled_line(theme, row)),
        )
}

fn styled_line(theme: &GuiTheme, row: &EditorRow) -> StyledText {
    let mut spans = highlight_line(&row.text);
    for range in &row.selection {
        spans = with_background(
            &row.text,
            &spans,
            range.clone(),
            parts(theme.selected_background),
            None,
        );
    }
    if let Some(Caret::Block(col)) = row.caret {
        spans = with_background(
            &row.text,
            &spans,
            col..col + 1,
            parts(theme.cursor),
            Some(parts(theme.text_inverse)),
        );
    }
    let highlights = spans.into_iter().map(|span| {
        let mut style = HighlightStyle {
            color: Some(rgb(pack(span.color)).into()),
            ..Default::default()
        };
        if span.bold {
            style.font_weight = Some(FontWeight::BOLD);
        }
        if span.italic {
            style.font_style = Some(gpui::FontStyle::Italic);
        }
        if span.underline {
            style.underline = Some(UnderlineStyle {
                thickness: px(1.),
                color: style.color,
                wavy: false,
            });
        }
        if let Some(background) = span.background {
            style.background_color = Some(rgb(pack(background)).into());
        }
        (span.bytes, style)
    });
    StyledText::new(row.text.clone()).with_highlights(highlights)
}

fn preview_pane(theme: &GuiTheme, preview: &PreviewView, entity: Entity<Shell>) -> gpui::Div {
    let title_color = if preview.focused {
        theme.text_active_focus
    } else {
        theme.text_secondary
    };
    let mut area = div().flex_1().min_h(px(0.)).relative().overflow_hidden();
    if let Some(message) = &preview.message {
        area = area.child(
            div()
                .px(px(8.))
                .text_color(rgb(theme.text_secondary))
                .child(message.clone()),
        );
    } else {
        area = area.child(y_scroll(
            "preview-scroll",
            &preview.port,
            preview_column(theme, preview, entity.clone()),
        ));
    }
    if let Some(bar) = &preview.bar {
        area = area.child(scroll_bar(theme, bar, entity.clone()));
    }
    div()
        .min_w(px(0.))
        .relative()
        .flex()
        .flex_col()
        .overflow_hidden()
        .border_t_1()
        .border_color(rgb(theme.border_default))
        .bg(rgb(theme.body_background))
        .child(pane_title(&preview.title, title_color, theme))
        .child(pane_bounds(entity.clone(), |shell, bounds| {
            shell.hit.preview_pane = Some(bounds);
        }))
        .child(
            area.child(
                canvas(
                    move |bounds, _, cx| {
                        entity.update(cx, |shell, _| shell.hit.preview = Some(bounds));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            ),
        )
}

struct Faces<'a> {
    mono: &'a str,
    sans: &'a str,
}

#[derive(Clone, Copy)]
struct PreviewSlice {
    scroll_px: f32,
    view_h: f32,
    cursor: Option<usize>,
    selected: Option<(usize, usize)>,
}

fn preview_column(theme: &GuiTheme, preview: &PreviewView, entity: Entity<Shell>) -> gpui::Div {
    let paint = PreviewSlice {
        scroll_px: preview.scroll_px,
        view_h: preview.view_h,
        cursor: preview.cursor,
        selected: preview.selected,
    };
    let faces = Faces {
        mono: preview.mono.as_str(),
        sans: preview.sans.as_str(),
    };
    let view_bottom = paint.scroll_px + paint.view_h;
    let mut y = 0.0;
    let mut shown = Vec::new();
    for (index, block) in preview.blocks.iter().enumerate() {
        let next = y + block.height;
        if next > paint.scroll_px && y < view_bottom {
            shown.push((index, y));
        }
        y = next;
    }
    let mut column = scroll_document(preview.content_h).mr(px(BAR_W));
    for (index, top) in shown {
        column = column.child(
            preview_block(
                theme,
                &preview.blocks[index],
                index,
                top,
                paint,
                &faces,
                entity.clone(),
            )
            .absolute()
            .top(px(top))
            .left(px(0.)),
        );
    }
    column
}

fn preview_block(
    theme: &GuiTheme,
    block: &RichBlock,
    index: usize,
    top: f32,
    paint: PreviewSlice,
    faces: &Faces<'_>,
    entity: Entity<Shell>,
) -> gpui::Div {
    let cursor = paint.cursor == Some(index);
    let selected = paint
        .selected
        .is_some_and(|(start, end)| index >= start && index <= end);
    let color = block_color(theme, block);
    let family = if block.mono { faces.mono } else { faces.sans };
    let mut body = div()
        .w_full()
        .h(px(block.height))
        .overflow_hidden()
        .relative()
        .font_family(family)
        .text_size(px(block.size))
        .line_height(px(block.line_h))
        .text_color(rgb(color));
    if block.kind == dd_render::BlockKind::Quote {
        body = body
            .border_l(px(2.))
            .border_color(rgb(theme.text_secondary));
    }
    if cursor {
        body = body
            .bg(rgb(theme.selected_background))
            .border_l(px(2.))
            .border_color(rgb(theme.cursor));
    } else if selected {
        body = body.bg(rgb(theme.selected_background));
    }
    if block.kind == dd_render::BlockKind::Heading {
        body = body.font_weight(FontWeight::BOLD);
    }
    if block.kind == dd_render::BlockKind::Rule {
        return body.child(
            div()
                .mt(px(5.))
                .mx(px(8.))
                .h(px(1.))
                .bg(rgb(theme.border_default)),
        );
    }
    if let Some(image) = &block.image {
        let picture = Arc::new(Image::from_bytes(ImageFormat::Png, image.png.clone()));
        let h = (block.height - 8.0).max(LINE);
        return body.px(px(8.)).child(
            img(picture)
                .h(px(h))
                .w_full()
                .object_fit(ObjectFit::Contain),
        );
    }
    body = body.pl(px(8. + f32::from(block.depth) * 16.)).pr(px(12.));
    let skip = line_skip(top, block.line_h, paint.scroll_px, block.lines.len());
    let take = ((paint.view_h / block.line_h.max(1.0)).ceil() as usize).saturating_add(2);
    let mut lines = div().w_full();
    if skip > 0 {
        lines = lines.child(div().h(px(skip as f32 * block.line_h)));
    }
    if !block.marker.is_empty() {
        lines = lines.child(preview_item(theme, block, skip, take, faces, entity));
    } else {
        for line in block.lines.iter().skip(skip).take(take) {
            lines = lines.child(preview_line(theme, block, line, faces, entity.clone()));
        }
    }
    body.child(lines)
}

fn preview_item(
    theme: &GuiTheme,
    block: &RichBlock,
    skip: usize,
    take: usize,
    faces: &Faces<'_>,
    entity: Entity<Shell>,
) -> gpui::Div {
    let marker_w = (block.marker.chars().count() as f32 * block.size * 0.55).max(block.size);
    let mut text = div().flex_1().min_w(px(0.));
    for (i, line) in block.lines.iter().enumerate().skip(skip).take(take) {
        let mut row = div()
            .h(px(block.line_h))
            .flex()
            .flex_row()
            .overflow_hidden();
        if i == 0 {
            row = row.child(
                div()
                    .w(px(marker_w))
                    .flex_shrink_0()
                    .child(block.marker.clone()),
            );
        } else {
            row = row.child(div().w(px(marker_w)).flex_shrink_0());
        }
        row = row.child(preview_runs(theme, block, line, faces, entity.clone()));
        text = text.child(row);
    }
    text
}

fn preview_line(
    theme: &GuiTheme,
    block: &RichBlock,
    line: &[RichRun],
    faces: &Faces<'_>,
    entity: Entity<Shell>,
) -> gpui::Div {
    div()
        .h(px(block.line_h))
        .overflow_hidden()
        .child(preview_runs(theme, block, line, faces, entity))
}

fn preview_runs(
    theme: &GuiTheme,
    block: &RichBlock,
    line: &[RichRun],
    faces: &Faces<'_>,
    entity: Entity<Shell>,
) -> gpui::Div {
    let mut row = div().flex().flex_row().overflow_hidden();
    if line.is_empty() {
        return row.child(" ");
    }
    for run in line {
        row = row.child(preview_run(theme, block, run, faces, entity.clone()));
    }
    row
}

fn preview_run(
    theme: &GuiTheme,
    block: &RichBlock,
    run: &RichRun,
    faces: &Faces<'_>,
    entity: Entity<Shell>,
) -> gpui::Div {
    let mut el = div().flex_shrink_0();
    if run.bold || block.kind == dd_render::BlockKind::Heading {
        el = el.font_weight(FontWeight::BOLD);
    }
    if run.italic {
        el = el.italic();
    }
    if run.strike {
        el = el.line_through();
    }
    if run.code {
        el = el
            .font_family(faces.mono)
            .text_size(px(13.))
            .px(px(2.))
            .text_color(rgb(theme.files));
    }
    if run.link.is_empty() {
        return el.child(run.text.clone());
    }
    let target = run.link.clone();
    el.cursor_pointer()
        .underline()
        .text_color(rgb(theme.links))
        .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
            entity.update(cx, |shell, cx| {
                cx.stop_propagation();
                if shell.modal_up() {
                    return;
                }
                shell.focus_preview_pane();
                if event.modifiers.control {
                    shell.open_external_raw(&target);
                } else {
                    let (dst, heading) = split_link(&target);
                    shell.follow_target(&dst, heading.as_deref());
                }
            });
        })
        .child(run.text.clone())
}

fn scroll_bar(theme: &GuiTheme, bar: &BarPaint, entity: Entity<Shell>) -> gpui::Div {
    let Some(thumb) = thumb_geom(bar.offset, bar.visible, bar.total, bar.track) else {
        return div();
    };
    let kind = bar.kind;
    let color = if bar.hot {
        theme.scrollbar_hover
    } else {
        theme.scrollbar
    };
    div()
        .absolute()
        .right(px(0.))
        .top(px(0.))
        .w(px(BAR_W))
        .h(px(bar.track.max(1.0)))
        .cursor(gpui::CursorStyle::Arrow)
        .on_mouse_down(MouseButton::Left, move |event, _window, cx| {
            entity.update(cx, |shell, cx| {
                cx.stop_propagation();
                shell.bar_down(kind, event);
            });
        })
        .child(
            div()
                .absolute()
                .top(px(thumb.y))
                .left(px(2.))
                .w(px(6.))
                .h(px(thumb.h))
                .bg(rgb(color)),
        )
}

fn resize_strips(
    tree: Option<Bounds<Pixels>>,
    preview: Option<Bounds<Pixels>>,
    notes: Option<Bounds<Pixels>>,
) -> gpui::Div {
    let mut layer = div().absolute().size_full();
    if let Some(bounds) = tree {
        let edge = bounds.origin.x + bounds.size.width;
        layer = layer.child(
            div()
                .absolute()
                .left(edge - px(6.))
                .top(bounds.origin.y)
                .w(px(12.))
                .h(bounds.size.height)
                .cursor_col_resize(),
        );
    }
    if let (Some(preview), Some(notes)) = (preview, notes) {
        let x0 = if notes.origin.x < preview.origin.x {
            notes.origin.x
        } else {
            preview.origin.x
        };
        let notes_right = notes.origin.x + notes.size.width;
        let preview_right = preview.origin.x + preview.size.width;
        let right = if notes_right > preview_right {
            notes_right
        } else {
            preview_right
        };
        layer = layer.child(
            div()
                .absolute()
                .left(x0)
                .top(preview.origin.y - px(6.))
                .w((right - x0).max(px(0.)))
                .h(px(12.))
                .cursor_row_resize(),
        );
    }
    layer
}

fn block_color(theme: &GuiTheme, block: &RichBlock) -> u32 {
    match block.kind {
        dd_render::BlockKind::Heading | dd_render::BlockKind::Banner => match block.level {
            1 => theme.success,
            2 => theme.text_active_focus,
            3 => theme.warning,
            4 => theme.error,
            _ if block.kind == dd_render::BlockKind::Banner => theme.info,
            _ => theme.text_labels,
        },
        dd_render::BlockKind::Quote => theme.text_secondary,
        dd_render::BlockKind::Code | dd_render::BlockKind::Pre => theme.files,
        _ => theme.text_primary,
    }
}

fn line_skip(top: f32, line_h: f32, scroll: f32, count: usize) -> usize {
    if line_h <= 0.0 || top >= scroll {
        return 0;
    }
    let skip = ((scroll - top) / line_h).floor() as usize;
    skip.min(count)
}

fn blocks_fitting(heights: &[f32], start: usize, view: f32) -> usize {
    let mut used = 0.0;
    let mut count = 0usize;
    for height in heights.iter().skip(start) {
        if count > 0 && used + *height > view {
            break;
        }
        used += *height;
        count += 1;
    }
    count
}

fn split_link(raw: &str) -> (String, Option<String>) {
    match raw.split_once('#') {
        Some((dst, heading)) if !dst.is_empty() && !heading.is_empty() => {
            (dst.to_string(), Some(heading.to_string()))
        }
        _ => (raw.to_string(), None),
    }
}

fn pane_title(text: &str, color: u32, theme: &GuiTheme) -> gpui::Div {
    div()
        .h(px(CHROME))
        .flex()
        .items_center()
        .px(px(6.))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_color(rgb(color))
        .border_b_1()
        .border_color(rgb(theme.border_default))
        .child(text.to_string())
}

fn parts(packed: u32) -> (u8, u8, u8) {
    ((packed >> 16) as u8, (packed >> 8) as u8, packed as u8)
}

fn pack(color: (u8, u8, u8)) -> u32 {
    (u32::from(color.0) << 16) | (u32::from(color.1) << 8) | u32::from(color.2)
}

fn overlay_panel(theme: &GuiTheme, view: OverlayView) -> gpui::Div {
    let mut panel = div()
        .w_full()
        .max_h(px(LINE * 10.))
        .flex()
        .flex_col()
        .overflow_hidden()
        .px(px(8.))
        .py(px(4.))
        .bg(rgb(theme.modal_background))
        .text_color(rgb(theme.modal_text))
        .border_t_1()
        .border_color(rgb(theme.border_active))
        .child(
            div()
                .text_color(rgb(if view.warn {
                    theme.warning
                } else {
                    theme.modal_header
                }))
                .child(view.title),
        );
    if !view.query.is_empty() {
        panel = panel.child(
            div()
                .h(px(LINE))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(rgb(theme.text_labels))
                .child(view.query),
        );
    }
    for line in view.lines {
        panel = panel.child(
            div()
                .h(px(LINE))
                .overflow_hidden()
                .whitespace_nowrap()
                .when(line.selected, |row| {
                    row.bg(rgb(theme.selected_background))
                        .text_color(rgb(theme.text_active_focus))
                })
                .child(line.text),
        );
    }
    panel
}

fn windowed(lines: &[String], selected: usize) -> Vec<OverlayLine> {
    const VISIBLE: usize = 8;
    if lines.is_empty() {
        return Vec::new();
    }
    let start = selected
        .saturating_sub(VISIBLE / 2)
        .min(lines.len().saturating_sub(VISIBLE));
    lines
        .iter()
        .enumerate()
        .skip(start)
        .take(VISIBLE)
        .map(|(index, text)| OverlayLine {
            text: text.clone(),
            selected: index == selected,
        })
        .collect()
}

fn hit_label(hit: &FileHit) -> String {
    let head = if hit.title.is_empty() {
        hit.path.clone()
    } else {
        format!("{}  {}", hit.title, hit.path)
    };
    if let Some(snippet) = &hit.snippet {
        format!("{head}  {snippet}")
    } else if let Some(heading) = &hit.heading {
        format!("{head}  #{heading}")
    } else {
        head
    }
}

fn fallback_jump(to: &NavigateTo) -> Jump {
    Jump {
        rel: to.rel.clone(),
        cursor: to.cursor.unwrap_or(0),
        scroll: to.scroll.unwrap_or(0),
        scroll_off: to.scroll_off.unwrap_or(0),
    }
}

fn move_selected(selected: &mut usize, len: usize, delta: i32) {
    if len == 0 {
        *selected = 0;
        return;
    }
    let next = *selected as i32 + delta;
    *selected = next.clamp(0, len as i32 - 1) as usize;
}

fn list_delta(press: KeyPress<'_>) -> Option<i32> {
    if press.control && eq_key(press.key, "n") {
        return Some(1);
    }
    if press.control && eq_key(press.key, "p") {
        return Some(-1);
    }
    if press.control {
        return None;
    }
    match press.key {
        "down" => Some(1),
        "up" => Some(-1),
        _ => None,
    }
}

fn typed_char(press: KeyPress<'_>) -> Option<char> {
    if press.control {
        return None;
    }
    if press.key == "space" {
        return Some(' ');
    }
    let text = press.key_char.unwrap_or(press.key);
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() || ch.is_control() {
        return None;
    }
    Some(ch)
}

fn eq_key(key: &str, letter: &str) -> bool {
    key.eq_ignore_ascii_case(letter)
}

fn prompt_hint(kind: &PromptKind) -> &'static str {
    match kind {
        PromptKind::GitCommit { then_push: true } => {
            "commit message    Enter commit and push    Esc cancel"
        }
        PromptKind::GitCommit { then_push: false } => {
            "commit message    Enter commit    Esc cancel"
        }
        PromptKind::AiProvider => "provider id    Enter accept    Esc cancel",
        PromptKind::AiKey { .. } => "paste a key    Enter save    Esc cancel",
        PromptKind::AiModel => "model id    Enter accept    Esc cancel",
        PromptKind::New { .. } | PromptKind::Rename { .. } => {
            "type a name    Enter accept    Esc cancel"
        }
    }
}

fn clipboard_image() -> Option<(u32, u32, Vec<u8>)> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let image = clipboard.get_image().ok()?;
    let width = u32::try_from(image.width).ok()?;
    let height = u32::try_from(image.height).ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height, image.bytes.into_owned()))
}

fn attach_boot(boot: &Boot) -> (Option<OpenVault>, Option<String>, Option<String>) {
    if let Some(vault) = boot.initial.clone() {
        let (open, status) = attach_vault(vault);
        return (Some(open), None, status);
    }
    if let Some(path) = boot.registry.last_path() {
        match dd_vault_core::open(&path) {
            Ok(vault) => {
                let (open, status) = attach_vault(vault);
                return (Some(open), None, status);
            }
            Err(err) => return (None, Some(err.to_string()), None),
        }
    }
    if boot.registry.vaults.is_empty() {
        return (
            None,
            Some("No vaults yet. Run dd_vault-gui init <path>.".into()),
            None,
        );
    }
    (None, None, None)
}

fn reload_tree(open: &mut OpenVault) {
    open.tree.reload(&open.vault);
    open.preview_epoch = open.preview_epoch.wrapping_add(1);
}

fn preview_motion(press: KeyPress<'_>) -> Option<isize> {
    if press.control {
        return None;
    }
    match press.key {
        "down" => Some(1),
        "up" => Some(-1),
        "pagedown" | "page_down" => Some(10),
        "pageup" | "page_up" => Some(-10),
        _ => match typed_char(press) {
            Some('j') => Some(1),
            Some('k') => Some(-1),
            _ => None,
        },
    }
}

fn is_home(press: KeyPress<'_>) -> bool {
    !press.control && (press.key == "home" || typed_char(press) == Some('g'))
}

fn is_end(press: KeyPress<'_>) -> bool {
    !press.control && (press.key == "end" || typed_char(press) == Some('G'))
}

fn attach_vault(vault: Vault) -> (OpenVault, Option<String>) {
    let config = VaultConfig::load_for(&vault, None);
    let mut wrap = config.wrap;
    let mut preview_visible = config.preview;
    let mut preview_scroll = 0usize;
    let mut focus_preview = false;
    let mut editor = Editor::empty();
    let mut tree = TreeState::load(&vault);
    let mut pane_tree = true;
    let mut zen = false;
    let mut focus_mode = false;
    let mut explore = false;
    let mut tree_split = 0u16;
    let mut preview_split = 0u16;
    if let Some(session) = Session::load(&vault.meta_dir) {
        zen = session.zen;
        focus_mode = session.focus;
        explore = session.explore;
        tree_split = session.tree_split;
        preview_split = session.preview_split;
        if let Some(saved) = session.wrap {
            wrap = saved;
        }
        if let Some(flag) = session.preview {
            preview_visible = flag;
        }
        preview_scroll = usize::from(session.preview_scroll);
        if let Some(rel) = session.note.clone() {
            let rel_path = PathBuf::from(&rel);
            if let Ok(path) = confined_path(&vault.root, &rel_path)
                && path.is_file()
                && let Ok(mut opened) = Editor::open(path, rel)
            {
                opened.restore_view(session.cursor, session.scroll, session.scroll_off);
                editor = opened;
                tree.reveal(&rel_path);
            }
        }
        pane_tree = match session.pane.as_deref() {
            Some("tree") => true,
            Some("preview") if editor.rel.is_some() && preview_visible => {
                focus_preview = true;
                false
            }
            Some("editor") if editor.rel.is_some() => false,
            _ => editor.rel.is_none(),
        };
        if explore {
            pane_tree = true;
            focus_preview = false;
        } else if zen || focus_mode {
            pane_tree = false;
            focus_preview = false;
        }
    }
    let (watch, mut status) = match VaultWatch::start(&vault.root) {
        Ok(watch) => (Some(watch), None),
        Err(err) => (None, Some(format!("File watch unavailable: {err}"))),
    };
    let index = match Index::open(&vault.meta_dir) {
        Ok(index) => Some(index),
        Err(err) => {
            if status.is_none() {
                status = Some(format!("Index: {err}"));
            }
            None
        }
    };
    let git = match dd_vault_core::status(&vault.root) {
        Ok(git) => git,
        Err(err) => {
            if status.is_none() {
                status = Some(err.to_string());
            }
            GitStatus::default()
        }
    };
    (
        OpenVault {
            vault,
            tree,
            editor,
            wrap,
            pane_tree,
            jumps: JumpList::default(),
            index,
            index_rx: None,
            reindex_queued: false,
            watch,
            suppress_disk: None,
            pending_disk: None,
            preview_visible,
            preview_read: false,
            preview_scroll,
            preview_row: 0,
            preview_visual: None,
            preview_epoch: 0,
            focus_preview,
            git,
            zen,
            focus_mode,
            explore,
            tree_split,
            preview_split,
        },
        status,
    )
}

fn leader_footer(leader: Leader) -> Option<(&'static [&'static str], &'static str)> {
    Some(match leader {
        Leader::None => return None,
        Leader::Space => (
            &[
                "Space…",
                "p preview",
                "P read",
                "ff files",
                "vv vault",
                "sg search",
                "nd daily",
                "z focus",
                "h zen",
                "e explore",
            ],
            "Esc cancel",
        ),
        Leader::Vault => (&["Space v…", "v: vault picker"], "Esc: cancel"),
        Leader::Files => (&["Space f…", "f: file finder"], "Esc: cancel"),
        Leader::Search => (&["Space s…", "g: search notes"], "Esc: cancel"),
        Leader::Tags => (&["Space t…", "g: tag search"], "Esc: cancel"),
        Leader::Daily => (&["Space n…", "d: today's note"], "Esc: cancel"),
        Leader::Ai => (&["Space a…", "i: AI card"], "Esc: cancel"),
    })
}

fn theme_motion(key: EditorKey) -> bool {
    matches!(
        key,
        EditorKey::Down
            | EditorKey::Up
            | EditorKey::Char('j')
            | EditorKey::Char('k')
            | EditorKey::Char('g')
            | EditorKey::Char('G')
    )
}

fn font_delta(press: KeyPress<'_>) -> Option<isize> {
    if press.key_char == Some("G") || press.key == "end" {
        return Some(isize::MAX);
    }
    if press.key == "home" || press.key == "g" || press.key_char == Some("g") {
        return Some(isize::MIN);
    }
    font_arrow_delta(press).or(match press.key {
        "j" => Some(1),
        "k" => Some(-1),
        _ => None,
    })
}

fn font_arrow_delta(press: KeyPress<'_>) -> Option<isize> {
    match press.key {
        "down" => Some(1),
        "up" => Some(-1),
        "end" => Some(isize::MAX),
        "home" => Some(isize::MIN),
        "pagedown" | "page_down" => Some(8),
        "pageup" | "page_up" => Some(-8),
        _ => None,
    }
}

fn is_fn(press: KeyPress<'_>, n: u8) -> bool {
    if press.control {
        return false;
    }
    let rest = press
        .key
        .strip_prefix('f')
        .or_else(|| press.key.strip_prefix('F'));
    rest.and_then(|rest| rest.parse::<u8>().ok()) == Some(n)
}

fn toast_color(theme: &GuiTheme, level: ToastLevel) -> u32 {
    match level {
        ToastLevel::Success => theme.success,
        ToastLevel::Info => theme.info,
        ToastLevel::Warning => theme.warning,
        ToastLevel::Error => theme.error,
    }
}

fn picker_label(row: &PickerRow) -> String {
    match row {
        PickerRow::Follow { name } => format!("Follow Omarchy ({name})"),
        PickerRow::Theme(theme) => theme.name.clone(),
        PickerRow::Custom => "Custom (built-in colors)".into(),
    }
}

fn theme_lines(picker: &ThemePicker, rows: usize) -> Vec<AppearLine> {
    let start = picker.scroll.min(picker.rows.len());
    picker
        .rows
        .iter()
        .enumerate()
        .skip(start)
        .take(rows)
        .map(|(index, row)| AppearLine {
            text: picker_label(row),
            selected: index == picker.selected,
            action: AppearAction::Theme(index),
        })
        .collect()
}

fn color_lines(editor: &ThemeEditor, rows: usize) -> Vec<AppearLine> {
    let view = theme_editor_rows(&editor.fields);
    let mut lines = Vec::new();
    for row in view.iter().skip(editor.scroll.min(view.len())).take(rows) {
        match row {
            ThemeEditorRow::Header(group) => lines.push(AppearLine {
                text: format!("— {group}"),
                selected: false,
                action: AppearAction::None,
            }),
            ThemeEditorRow::Color(idx) => {
                let key = editor.fields.get(*idx).map(|field| field.key).unwrap_or("");
                let hex = editor
                    .palette
                    .get(key)
                    .map(|color| color.to_hex())
                    .unwrap_or_default();
                let selected = *idx == editor.selected;
                let shown_hex = if editor.editing_hex && selected {
                    editor.hex_draft.clone()
                } else {
                    hex
                };
                let channel = ["R", "G", "B"][editor.channel.min(2)];
                lines.push(AppearLine {
                    text: format!("{key:<22} {shown_hex}  {channel}"),
                    selected,
                    action: AppearAction::Color(*idx),
                });
            }
        }
    }
    lines
}

fn pane_bounds(
    entity: Entity<Shell>,
    store: impl Fn(&mut Shell, Bounds<Pixels>) + 'static,
) -> gpui::Div {
    div().absolute().size_full().child(
        canvas(
            move |bounds, _, cx| {
                entity.update(cx, |shell, _| store(shell, bounds));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full(),
    )
}

fn help_panel(theme: &GuiTheme, lines: &[String]) -> gpui::Div {
    let mut panel = sheet_frame(theme, "Help  ·  desktop window (dd_vault-gui)");
    for line in lines {
        panel = panel.child(sheet_line(theme, line, false));
    }
    panel
}

fn appearance_sheet(theme: &GuiTheme, view: &AppearanceView, entity: Entity<Shell>) -> gpui::Div {
    let mut list = div().flex_1().min_h(px(0.)).overflow_hidden();
    for line in &view.lines {
        list = list.child(appear_line(
            theme,
            &line.text,
            line.selected,
            line.action,
            entity.clone(),
        ));
    }
    div()
        .absolute()
        .top(px(CHROME))
        .right(px(0.))
        .bottom(px(CHROME))
        .w(px(SHEET_W))
        .max_w(relative(0.46))
        .overflow_hidden()
        .flex()
        .flex_col()
        .px(px(8.))
        .py(px(4.))
        .bg(rgb(theme.modal_background))
        .text_color(rgb(theme.modal_text))
        .border_l_1()
        .border_color(rgb(theme.border_default))
        .child(sheet_heading(theme, &view.title))
        .child(sheet_heading(theme, &view.hint))
        .child(list)
        .child(sheet_heading(theme, "Fonts"))
        .child(appear_line(
            theme,
            &format!("Editor   {}", view.editor_font),
            view.editor_on,
            AppearAction::Focus(AppearSection::Mono),
            entity.clone(),
        ))
        .child(appear_line(
            theme,
            &format!("Preview  {}", view.preview_font),
            view.preview_on,
            AppearAction::Focus(AppearSection::Sans),
            entity,
        ))
}

fn sheet_heading(theme: &GuiTheme, text: &str) -> gpui::Div {
    div()
        .h(px(LINE))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_color(rgb(theme.modal_header))
        .child(if text.is_empty() {
            " ".to_string()
        } else {
            text.to_string()
        })
}

fn appear_line(
    theme: &GuiTheme,
    text: &str,
    selected: bool,
    action: AppearAction,
    entity: Entity<Shell>,
) -> gpui::Div {
    let shown = if text.is_empty() {
        " ".to_string()
    } else {
        text.to_string()
    };
    div()
        .h(px(LINE))
        .overflow_hidden()
        .whitespace_nowrap()
        .cursor_pointer()
        .when(selected, |row| {
            row.bg(rgb(theme.selected_background))
                .text_color(rgb(theme.text_active_focus))
        })
        .on_mouse_down(MouseButton::Left, move |_event, _window, cx| {
            entity.update(cx, |shell, cx| {
                cx.stop_propagation();
                shell.appear_click(action);
            });
        })
        .child(shown)
}

fn sheet_frame(theme: &GuiTheme, title: &str) -> gpui::Div {
    div()
        .absolute()
        .top(px(CHROME))
        .left(px(0.))
        .right(px(0.))
        .bottom(px(CHROME))
        .overflow_hidden()
        .flex()
        .flex_col()
        .px(px(8.))
        .py(px(4.))
        .bg(rgb(theme.modal_background))
        .text_color(rgb(theme.modal_text))
        .child(
            div()
                .h(px(LINE))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(rgb(theme.modal_header))
                .child(title.to_string()),
        )
}

fn sheet_line(theme: &GuiTheme, text: &str, selected: bool) -> gpui::Div {
    let shown = if text.is_empty() {
        " ".to_string()
    } else {
        text.to_string()
    };
    div()
        .h(px(LINE))
        .overflow_hidden()
        .whitespace_nowrap()
        .when(selected, |row| {
            row.bg(rgb(theme.selected_background))
                .text_color(rgb(theme.text_active_focus))
        })
        .child(shown)
}

fn toast_stack(theme: &GuiTheme, toasts: &[ToastPaint], lift: f32) -> gpui::Div {
    let mut stack = div()
        .absolute()
        .right(px(12.))
        .bottom(px(CHROME + 8. + lift))
        .flex()
        .flex_col();
    for toast in toasts {
        stack = stack.child(
            div()
                .mt(px(4.))
                .px(px(8.))
                .max_w(px(560.))
                .overflow_hidden()
                .whitespace_nowrap()
                .bg(rgb(theme.modal_background))
                .text_color(rgb(toast.color))
                .border_1()
                .border_color(rgb(toast.color))
                .child(toast.message.clone()),
        );
    }
    stack
}

fn register(paths: &Paths, vault: &Vault) -> Result<(), dd_vault_core::Error> {
    let mut registry = Registry::load(paths)?;
    registry.register(vault);
    registry.save(paths)?;
    Ok(())
}

#[cfg(test)]
mod chrome_tests {
    use super::leader_footer;
    use crate::chrome::{FooterKind, footer_line};
    use crate::commands::Leader;

    #[test]
    fn space_leader_footer_names_focus_zen_and_explore() {
        let (middle, cancel) = leader_footer(Leader::Space).unwrap();
        let line = footer_line(200, FooterKind::Leader(middle, cancel));
        assert!(line.contains("z focus"));
        assert!(line.contains("h zen"));
        assert!(line.contains("e explore"));
        assert!(line.contains("F1: Help"));
        assert!(line.contains(cancel));
        assert!(leader_footer(Leader::None).is_none());
    }
}
