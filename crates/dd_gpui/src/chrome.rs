//! Help text, footer fit, toasts, and pane sizes. No terminal widgets.

use std::time::{Duration, Instant};

use ldnddev_theme::{
    ColorField, EXTRA_MODAL_HEADER, EXTRA_TEXT_DISABLED, EXTRA_TEXT_INVERSE, EditorKey,
};

pub const TOAST_TTL: Duration = Duration::from_secs(5);
const TOAST_CAP: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastLevel {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub level: ToastLevel,
    pub message: String,
    pub shown_at: Instant,
}

pub fn push_toast(toasts: &mut Vec<Toast>, level: ToastLevel, message: impl Into<String>) {
    toasts.push(Toast {
        level,
        message: message.into(),
        shown_at: Instant::now(),
    });
    if toasts.len() > TOAST_CAP {
        toasts.remove(0);
    }
}

pub fn prune_toasts(toasts: &mut Vec<Toast>) -> bool {
    let before = toasts.len();
    let now = Instant::now();
    toasts.retain(|toast| now.duration_since(toast.shown_at) < TOAST_TTL);
    toasts.len() != before
}

pub fn extra_theme_fields() -> &'static [ColorField] {
    &[EXTRA_MODAL_HEADER, EXTRA_TEXT_DISABLED, EXTRA_TEXT_INVERSE]
}

/// Reading and zen hide the tree. Explore forces it. Focus hides it. Otherwise 80 columns.
pub fn show_tree(zen: bool, focus: bool, explore: bool, reading: bool, cols: u16) -> bool {
    if reading || zen {
        return false;
    }
    if explore {
        return true;
    }
    if focus {
        return false;
    }
    cols >= 80
}

pub fn show_preview(zen: bool, reading: bool, preview_visible: bool) -> bool {
    if reading {
        return true;
    }
    if zen {
        return false;
    }
    preview_visible
}

pub fn show_ai(zen: bool, reading: bool) -> bool {
    !zen && !reading
}

pub fn tree_cols(split: u16, body_cols: u16) -> u16 {
    let max_w = body_cols.saturating_sub(20).max(12);
    let auto = if body_cols < 80 {
        (body_cols / 3).clamp(12, 24)
    } else {
        (body_cols / 4).clamp(18, 32)
    };
    if split == 0 {
        auto.min(max_w)
    } else {
        split.clamp(12, max_w)
    }
}

pub fn tree_width_px(split: u16, char_w: f32, body_px: f32) -> f32 {
    let char_w = char_w.max(1.0);
    let cols = (body_px / char_w).floor().max(1.0) as u16;
    f32::from(tree_cols(split, cols)) * char_w
}

/// Fixed preview height in pixels. `split` is character rows and must be non-zero.
pub fn preview_fixed_px(split: u16, line: f32, body_px: f32) -> f32 {
    let line = line.max(1.0);
    let rows = (body_px / line).floor().max(1.0) as u16;
    let max_h = rows.saturating_sub(4).max(4);
    f32::from(split.clamp(4, max_h)) * line
}

pub fn editor_key(key: &str, key_char: Option<&str>, control: bool) -> Option<EditorKey> {
    if control {
        return None;
    }
    let named = match key {
        "up" => Some(EditorKey::Up),
        "down" => Some(EditorKey::Down),
        "left" => Some(EditorKey::Left),
        "right" => Some(EditorKey::Right),
        "tab" => Some(EditorKey::Tab),
        "enter" | "return" => Some(EditorKey::Enter),
        "escape" => Some(EditorKey::Esc),
        "backspace" => Some(EditorKey::Backspace),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    let text = key_char.filter(|text| !text.is_empty()).unwrap_or(key);
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() || ch.is_control() {
        return None;
    }
    Some(EditorKey::Char(ch))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditMode {
    Normal,
    Insert,
    Visual,
    Command,
    Search,
}

#[derive(Clone, Copy, Debug)]
pub enum FooterKind<'a> {
    Help,
    Filter,
    Leader(&'a [&'a str], &'a str),
    Ai { streaming: bool },
    NoVault,
    Reading { visual: bool },
    Tree,
    Preview,
    Editor(EditMode),
}

pub fn footer_line(cols: u16, kind: FooterKind<'_>) -> String {
    if matches!(kind, FooterKind::Filter) {
        return if cols < 80 {
            "Esc:Clear  Enter:Keep  type".to_string()
        } else {
            "/: filter   Esc: clear   Enter: keep   type to filter".to_string()
        };
    }
    if matches!(kind, FooterKind::Help) {
        return if cols < 80 {
            "F1:Help  Esc:Close  ^Q:Quit".to_string()
        } else {
            "F1: Help  Esc: Close  Ctrl+Q: Quit".to_string()
        };
    }
    let (f1, f2, quit) = if cols < 80 {
        ("F1:Help", "F2:Theme", "^Q:Quit")
    } else {
        ("F1: Help", "F2: Theme", "Ctrl+Q: Quit")
    };
    let left = [f1, f2];
    match kind {
        FooterKind::Leader(middle, cancel) => fit_footer(cols, &left, middle, &[cancel]),
        FooterKind::Ai { streaming: true } => fit_footer(cols, &left, &["Esc: Cancel"], &[quit]),
        FooterKind::Ai { streaming: false } => fit_footer(
            cols,
            &left,
            &["Enter: Send", "Esc: Close", "Tab: Cycle"],
            &[quit],
        ),
        FooterKind::NoVault => fit_footer(cols, &left, &["<Space>vv: Open vault"], &[quit]),
        FooterKind::Reading { visual: true } => fit_footer(
            cols,
            &left,
            &["j/k: Extend", "y: Copy", "Esc: Cancel"],
            &[quit],
        ),
        FooterKind::Reading { visual: false } => fit_footer(
            cols,
            &left,
            &["j/k: Move", "v: Select", "y: Copy", "Esc: Back"],
            &[quit],
        ),
        FooterKind::Tree => fit_footer(
            cols,
            &left,
            &[
                "j/k: Move",
                "Enter: Open",
                "a: New",
                "h/l: Folder",
                "/: Filter",
            ],
            &[quit],
        ),
        FooterKind::Preview => fit_footer(
            cols,
            &left,
            &["j/k: Scroll", "g/G: Ends", "Ctrl+click: Open"],
            &[quit],
        ),
        FooterKind::Editor(EditMode::Insert) => fit_footer(
            cols,
            &left,
            &["Esc: Normal", "Ctrl+S: Save", "[[: Link"],
            &[quit],
        ),
        FooterKind::Editor(EditMode::Visual) => fit_footer(
            cols,
            &left,
            &["y: Yank", "d: Delete", "Esc: Normal"],
            &[quit],
        ),
        FooterKind::Editor(EditMode::Command) => {
            fit_footer(cols, &left, &["Enter: Run", "Esc: Cancel"], &[quit])
        }
        FooterKind::Editor(EditMode::Search) => {
            fit_footer(cols, &left, &["Enter: Find", "Esc: Cancel"], &[quit])
        }
        FooterKind::Editor(EditMode::Normal) => fit_footer(
            cols,
            &left,
            &["i: Insert", ":w: Save", "gf: Follow"],
            &[quit],
        ),
        FooterKind::Help | FooterKind::Filter => String::new(),
    }
}

fn fit_footer(width: u16, left: &[&str], middle: &[&str], right: &[&str]) -> String {
    let limit = usize::from(width);
    let mut mid: Vec<&str> = middle.to_vec();
    loop {
        let mut parts = Vec::with_capacity(left.len() + mid.len() + right.len());
        parts.extend_from_slice(left);
        parts.extend(mid.iter().copied());
        parts.extend_from_slice(right);
        let line = parts.join("  ");
        if line.chars().count() <= limit || mid.is_empty() {
            return line;
        }
        mid.pop();
    }
}

const NOTES: &str = "Chrome follows LDNDDEV_TUI_VISUAL_STANDARD.md. Header is always 3 rows and footer 1, including zen. The tree lists one folder at a time (l enters, h returns) and reads files on disk; .dd_vault-*, .git, .gitignore, and db files are hidden. Long names keep their start and extension. External edits reload a clean buffer, or prompt Reload/Keep if dirty. Editor title shows git:clean / git:±N / git:conflict. :git commit scans for ghp_ / github_pat_ / AKIA / sk- / xai- / private keys and does not run repo hooks, fsmonitor, or repo aliases. Git refuses a vault config that rewrites remote URLs or sets a custom upload-pack or receive-pack program. Symlinks that point outside the vault are not opened. Overlapping pull conflicts write note.conflict-<ts>.md sidecars and keep the original. Ctrl+V pastes a clipboard image to assets/<note>-<ts>.png. Click/drag in the editor to place the caret or select; y on that selection copies it to the system clipboard. Drag the pane borders to resize. :daily and <Space>nd open notes/daily/YYYY-MM-DD.md. <Space><Space> is the command palette. Search matches frontmatter titles and aliases. gf / Enter follows [[wikilinks]] and local markdown links (creates a missing note after confirm). Ctrl+O / Ctrl+I walk the jump list. Reopening a vault restores the last note, caret, wrap, preview, splits, and zen/focus. AI is off until :ai on; each request asks consent. Default network provider is SpaceXAI (grok-4.6). :ai provider openrouter stores a key in ~/.config/ldnddev/ai.keys; :ai provider ollama talks to a local Ollama daemon. ```mermaid flowcharts, sequence diagrams, and pie charts draw as Unicode boxes in the preview. Narrow terminals hide the tree until <Space>e. This is the desktop window (dd_vault-gui), not the terminal.";

pub fn help_lines() -> Vec<String> {
    let mut lines = Vec::new();
    add_section(
        &mut lines,
        "Global",
        &[
            ("F1", "Open/close this help"),
            ("F2", "Omarchy themes (Enter apply, e edit, s save)"),
            ("Ctrl+Q", "Quit"),
            ("Tab", "Focus tree → editor → preview"),
            (
                "gf / Enter",
                "Follow [[wikilink]] or local markdown link under the caret",
            ),
            ("Ctrl+O / Ctrl+I", "Jump list back / forward"),
            ("<Space>p", "Toggle markdown preview"),
            ("<Space>P", "Read the preview full-screen"),
            ("<Space>ff", "File finder"),
            ("<Space>sg", "Content search (FTS)"),
            ("<Space>tg", "Tag search"),
            (
                "<Space>vv",
                "Vault picker from any pane (d removes from the list)",
            ),
            ("<Space>nd", "Open today's daily note"),
            ("<Space><Space>", "Command palette"),
            ("<Space>ai", "Cycle AI card (collapsed / chat / full)"),
            (":ai on|off|<prompt>", "Enable, disable, or prompt the AI"),
            (
                ":ai provider",
                "spacexai, openrouter, ollama (local), openai, grok-cli, llm",
            ),
            (
                ":ai key / model",
                "Store an API key in ~/.config/ldnddev/ai.keys (0600) or set the model",
            ),
            ("<Space>z", "Focus mode (hide the tree)"),
            ("<Space>h", "Zen (editor only; keep header/footer)"),
            ("<Space>e", "Toggle the tree (explore)"),
            ("[[", "Wikilink picker (in INSERT)"),
        ],
    );
    add_section(
        &mut lines,
        "Help / Theme overlays",
        &[
            ("F1 / Esc", "Close help"),
            ("F2 / Esc", "Close theme (Esc reverts unsaved edits)"),
            ("Enter", "Apply the highlighted Omarchy theme"),
            ("e", "Edit colors for the applied theme"),
            (
                "j/k or arrows",
                "Move in the theme list or the color editor",
            ),
            ("Y / S", "Save the theme name (hex edits are overrides)"),
            ("Tab", "Toggle save target local ↔ global"),
            ("R", "Reset the color editor to the built-in palette"),
        ],
    );
    add_section(
        &mut lines,
        "Vault tree",
        &[
            ("j/k or arrows", "Move selection"),
            ("l / Right", "Enter the selected folder"),
            ("h / Left / ..", "Parent folder"),
            ("Enter", "Open a file, or enter a folder"),
            ("g / G", "First / last row"),
            ("/", "Filter names across the vault"),
            ("a", "New file (name/ creates a folder)"),
            ("r", "Rename"),
            ("d", "Delete (confirm)"),
        ],
    );
    add_section(
        &mut lines,
        "Editor (vim-flavored)",
        &[
            ("h j k l", "Move"),
            ("w b e  0 $  gg G", "Words / line / file"),
            ("i a o O", "Insert"),
            ("v V", "Visual / linewise"),
            (
                "dd yy p P",
                "Delete / yank / paste (y and yy copy to the system clipboard)",
            ),
            ("u  Ctrl+R", "Undo / redo"),
            ("/ n N", "Search"),
            ("m{a-z}  `{a-z}", "Marks"),
            ("\"{a-z}", "Register prefix"),
            (":w :q :wq :q! :e", "Write / quit / open"),
            (
                ":git pull|push|commit",
                "Pull; push stages all + commit message if dirty, then push; commit only",
            ),
            (":daily", "Open today's daily note"),
            ("gf  Enter", "Follow wikilink / markdown link"),
            (
                "gx  Ctrl+Enter  Ctrl+click",
                "Open the link in its default app",
            ),
            ("Ctrl+S", "Save"),
            ("Ctrl+V", "Paste clipboard image into assets/"),
            ("Esc", "Back to NORMAL"),
        ],
    );
    add_section(
        &mut lines,
        "Preview",
        &[
            ("<Space>p", "Show / hide the preview pane"),
            ("<Space>P", "Expand the preview over the body"),
            ("j/k", "Scroll, or move the reading cursor"),
            ("v  y", "In reading view, select rows and copy them"),
            ("Ctrl+Enter  Ctrl+click", "Open the link in its default app"),
            ("Esc", "Leave reading view"),
            ("g / G", "Top / bottom"),
            ("Click [[link]]", "Follow wikilink in the preview"),
            (
                "```mermaid",
                "Flowchart, sequence, and pie draw as boxes in the preview",
            ),
        ],
    );
    add_section(
        &mut lines,
        "Mouse",
        &[
            ("Wheel", "Scroll the pane or overlay under the cursor"),
            ("Click pane", "Focus tree, editor, or preview"),
            ("Click tree row", "Select"),
            ("Double-click", "Open a file or enter a folder"),
            ("Click editor", "Place the caret"),
            ("Drag in editor", "Visual select (gutter = linewise)"),
            (
                "Drag pane border",
                "Resize tree | editor or editor / preview",
            ),
        ],
    );
    lines.push("Notes".to_string());
    lines.push(String::new());
    for chunk in wrap_words(NOTES, 88) {
        lines.push(format!("  {chunk}"));
    }
    lines
}

fn add_section(lines: &mut Vec<String>, title: &str, items: &[(&str, &str)]) {
    lines.push(title.to_string());
    lines.push(String::new());
    for (key, action) in items {
        let prefix = format!("  • {key:<18}");
        let chunks = wrap_words(action, 56);
        for (i, chunk) in chunks.iter().enumerate() {
            if i == 0 {
                lines.push(format!("{prefix}{chunk}"));
            } else {
                lines.push(format!("{:22}{chunk}", ""));
            }
        }
    }
    lines.push(String::new());
    lines.push(format!("  {}", "─".repeat(40)));
    lines.push(String::new());
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else {
            let with_space = format!("{current} {word}");
            if with_space.chars().count() <= width {
                current = with_space;
                continue;
            }
            out.push(current);
            current = word.to_string();
        }
        if current.chars().count() > width {
            let chars: Vec<char> = current.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let end = (i + width).min(chars.len());
                out.push(chars[i..end].iter().collect());
                i = end;
            }
            current.clear();
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_keeps_the_terminal_notes_and_names_this_window() {
        // Wrapped lines are indented, so join with a space and collapse.
        let text = help_lines()
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("Narrow terminals hide the tree"));
        assert!(text.contains("desktop window (dd_vault-gui), not the terminal"));
        assert!(text.contains("F1"));
        assert!(text.contains("F2"));
        assert!(text.contains("<Space>h"));
        assert!(text.contains("<Space>z"));
        assert!(text.contains("<Space>e"));
    }

    #[test]
    fn layout_flags_match_the_terminal_rules() {
        assert!(!show_tree(true, false, true, false, 120));
        assert!(!show_tree(false, false, true, true, 120));
        assert!(show_tree(false, true, true, false, 40));
        assert!(!show_tree(false, true, false, false, 120));
        assert!(show_tree(false, false, false, false, 80));
        assert!(!show_tree(false, false, false, false, 79));
        assert!(show_preview(true, true, false));
        assert!(!show_preview(true, false, true));
        assert!(show_preview(false, false, true));
        assert!(!show_ai(true, false));
        assert!(!show_ai(false, true));
        assert!(show_ai(false, false));
    }

    #[test]
    fn splits_clamp_like_the_terminal() {
        assert_eq!(tree_cols(0, 100), 25);
        assert_eq!(tree_cols(0, 50), 16);
        assert_eq!(tree_cols(10, 100), 12);
        assert_eq!(tree_cols(50, 100), 50);
        assert_eq!(tree_cols(0, 30), 12);
        assert!((tree_width_px(0, 8.0, 800.0) - 200.0).abs() < 0.1);
        assert!((preview_fixed_px(10, 20.0, 400.0) - 200.0).abs() < 0.1);
        assert!((preview_fixed_px(100, 20.0, 400.0) - 320.0).abs() < 0.1);
    }

    #[test]
    fn toasts_cap_at_four_and_expire() {
        let mut toasts = Vec::new();
        for i in 0..6 {
            push_toast(&mut toasts, ToastLevel::Info, format!("{i}"));
        }
        assert_eq!(toasts.len(), 4);
        assert_eq!(toasts[0].message, "2");
        assert_eq!(toasts[3].message, "5");
        toasts[0].shown_at = Instant::now() - Duration::from_secs(6);
        assert!(prune_toasts(&mut toasts));
        assert_eq!(toasts.len(), 3);
        assert!(!prune_toasts(&mut toasts));
    }

    #[test]
    fn footer_pins_f1_and_drops_context_from_the_right() {
        let wide = footer_line(120, FooterKind::Editor(EditMode::Normal));
        assert!(wide.contains("F1: Help"));
        assert!(wide.contains("F2: Theme"));
        assert!(wide.contains("gf: Follow"));
        assert!(wide.contains("Ctrl+Q: Quit"));
        let narrow = footer_line(40, FooterKind::Editor(EditMode::Normal));
        assert!(narrow.starts_with("F1:Help"));
        assert!(narrow.contains("F2:Theme"));
        assert!(narrow.contains("^Q:Quit"));
        assert!(!narrow.contains("gf: Follow"));
        assert_eq!(
            footer_line(100, FooterKind::Help),
            "F1: Help  Esc: Close  Ctrl+Q: Quit"
        );
        assert_eq!(
            footer_line(40, FooterKind::Help),
            "F1:Help  Esc:Close  ^Q:Quit"
        );
        let leader = footer_line(
            36,
            FooterKind::Leader(&["one", "two", "three", "four", "five"], "Esc cancel"),
        );
        assert!(leader.contains("F1:Help"));
        assert!(leader.contains("Esc cancel"));
        assert!(!leader.contains("five"));
    }

    #[test]
    fn theme_keys_include_shift_characters() {
        assert_eq!(editor_key("escape", None, false), Some(EditorKey::Esc));
        assert_eq!(
            editor_key("j", Some("j"), false),
            Some(EditorKey::Char('j'))
        );
        assert_eq!(
            editor_key("r", Some("R"), false),
            Some(EditorKey::Char('R'))
        );
        assert_eq!(editor_key("f2", None, false), None);
        assert_eq!(editor_key("q", Some("q"), true), None);
    }
}
