use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};

use crate::theme::{color_to_hex, AppTheme};

fn wrap_to_lines(text: &str, width: usize) -> Vec<String> {
    let w = width.max(1);
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
            if with_space.chars().count() <= w {
                current = with_space;
                continue;
            }
            out.push(current);
            current = word.to_string();
        }
        if current.chars().count() > w {
            let chars: Vec<char> = current.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let end = (i + w).min(chars.len());
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

pub fn build_help_text(theme: &AppTheme, width: usize) -> Text<'static> {
    let h_style = Style::default()
        .fg(theme.modal_header)
        .add_modifier(Modifier::BOLD);
    let k_style = Style::default().fg(theme.text_active_focus);
    let div_style = Style::default().fg(theme.text_secondary);
    const KEY_COL: usize = 22;

    fn add_section(
        lines: &mut Vec<Line<'static>>,
        title: &'static str,
        items: &[(&'static str, &'static str)],
        h_style: Style,
        k_style: Style,
        div_style: Style,
        width: usize,
    ) {
        lines.push(Line::from(Span::styled(title.to_string(), h_style)));
        lines.push(Line::from(""));
        for (k, a) in items {
            let prefix = format!("  • {k:<18}");
            let chunks = wrap_to_lines(a, width.saturating_sub(KEY_COL));
            for (i, chunk) in chunks.iter().enumerate() {
                if i == 0 {
                    lines.push(Line::from(vec![
                        Span::styled(prefix.clone(), k_style),
                        Span::raw(chunk.clone()),
                    ]));
                } else {
                    lines.push(Line::from(Span::raw(format!(
                        "{}{chunk}",
                        " ".repeat(KEY_COL)
                    ))));
                }
            }
        }
        lines.push(Line::from(""));
        let rule = "─".repeat(width.saturating_sub(4).clamp(12, 50));
        lines.push(Line::from(Span::styled(format!("  {rule}"), div_style)));
        lines.push(Line::from(""));
    }

    let mut lines = Vec::new();
    add_section(
        &mut lines,
        "Global",
        &[
            ("F1", "Open/close this help"),
            (
                "F2",
                "Live theme editor (source, tokens, save local/global)",
            ),
            ("Ctrl+Q", "Quit"),
            ("Tab", "Focus tree → editor → preview"),
            ("<Space>p", "Toggle markdown preview"),
            ("<Space>ff", "File finder"),
            ("<Space>sg", "Content search (FTS)"),
            ("<Space>tg", "Tag search"),
            ("<Space>vv", "Vault picker (registered vaults)"),
            ("<Space>nd", "Open today's daily note"),
            ("<Space><Space>", "Command palette"),
            ("<Space>ai", "Cycle AI card (collapsed / chat / full)"),
            (":ai on|off|<prompt>", "Enable, disable, or prompt the AI"),
            ("<Space>z", "Focus mode (hide the tree)"),
            ("<Space>h", "Zen (editor only; keep header/footer)"),
            ("<Space>e", "Toggle the tree (explore)"),
            ("[[", "Wikilink picker (in INSERT)"),
        ],
        h_style,
        k_style,
        div_style,
        width,
    );
    add_section(
        &mut lines,
        "Help / Theme overlays",
        &[
            ("F1 / Esc", "Close help"),
            ("F2 / Esc", "Close theme (Esc reverts unsaved edits)"),
            ("j/k or arrows", "Scroll or move the selected token"),
            ("Y / S", "Save theme"),
            ("Tab", "Toggle save target local ↔ global"),
            ("R", "Reset to built-in palette"),
        ],
        h_style,
        k_style,
        div_style,
        width,
    );
    add_section(
        &mut lines,
        "Vault tree",
        &[
            ("j/k or arrows", "Move selection"),
            ("h / l", "Collapse / expand directory"),
            ("Enter", "Open file (or toggle folder)"),
            ("g / G", "First / last row"),
            ("/", "Filter by name (live)"),
            ("a", "New file (name/ creates a folder)"),
            ("r", "Rename"),
            ("d", "Delete (confirm)"),
        ],
        h_style,
        k_style,
        div_style,
        width,
    );
    add_section(
        &mut lines,
        "Editor (vim-flavored)",
        &[
            ("h j k l", "Move"),
            ("w b e  0 $  gg G", "Words / line / file"),
            ("i a o O", "Insert"),
            ("v V", "Visual / linewise"),
            ("dd yy p P", "Delete / yank / paste line"),
            ("u  Ctrl+R", "Undo / redo"),
            ("/ n N", "Search"),
            ("m{a-z}  `{a-z}", "Marks"),
            ("\"{a-z}", "Register prefix"),
            (":w :q :wq :q! :e", "Write / quit / open"),
            (
                ":git pull|push|commit",
                "Sync via git CLI (commit asks for a message)",
            ),
            (":daily", "Open today's daily note"),
            ("Ctrl+S", "Save"),
            ("Ctrl+V", "Paste clipboard image into assets/"),
            ("Esc", "Back to NORMAL"),
        ],
        h_style,
        k_style,
        div_style,
        width,
    );
    add_section(
        &mut lines,
        "Preview",
        &[
            ("<Space>p", "Show / hide the preview pane"),
            ("j/k", "Scroll when preview is focused"),
            ("g / G", "Top / bottom"),
        ],
        h_style,
        k_style,
        div_style,
        width,
    );
    add_section(
        &mut lines,
        "Mouse",
        &[
            ("Wheel", "Scroll the pane or overlay under the cursor"),
            ("Click pane", "Focus tree, editor, or preview"),
            ("Click tree row", "Select"),
            ("Click ▸/▾", "Expand or collapse"),
            ("Double-click", "Open file / toggle folder"),
            ("Click editor", "Place the caret"),
            ("Drag in editor", "Visual select (gutter = linewise)"),
            (
                "Drag pane border",
                "Resize tree | editor or editor / preview",
            ),
        ],
        h_style,
        k_style,
        div_style,
        width,
    );
    lines.push(Line::from(Span::styled("Notes", h_style)));
    lines.push(Line::from(""));
    let note = "Chrome follows LDNDDEV_TUI_VISUAL_STANDARD.md. Header is always 3 rows and footer 1, including zen. The tree is files on disk; .dd_vault-*, .git, .gitignore, and db files are hidden. External edits reload a clean buffer, or prompt Reload/Keep if dirty. Editor title shows git:clean / git:±N / git:conflict. :git commit scans for ghp_ / github_pat_ / AKIA. Overlapping pull conflicts write note.conflict-<ts>.md sidecars and keep the original. Ctrl+V pastes a clipboard image to assets/<note>-<ts>.png. Click/drag in the editor to place the caret or select; drag the pane borders to resize. :daily and <Space>nd open notes/daily/YYYY-MM-DD.md. <Space><Space> is the command palette. Search matches frontmatter titles and aliases. AI is off until :ai on; each request asks consent. SpaceXAI (grok-4.6) is the default network provider. Narrow terminals hide the tree until <Space>e.";
    for chunk in wrap_to_lines(note, width.saturating_sub(2)) {
        lines.push(Line::from(Span::raw(format!("  {chunk}"))));
    }
    Text::from(lines)
}

pub fn build_theme_text(theme: &AppTheme, status: &Option<String>, width: usize) -> Text<'static> {
    let h_style = Style::default()
        .fg(theme.modal_header)
        .add_modifier(Modifier::BOLD);
    let k_style = Style::default().fg(theme.text_active_focus);
    let div_style = Style::default().fg(theme.text_secondary);
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled("Theme", h_style)));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  App: ", k_style),
        Span::raw(format!("dd_vault v{}", env!("CARGO_PKG_VERSION"))),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Source: ", k_style),
        Span::raw(format!(
            "{}   (./dd_vault_theme.yml or ~/.config/ldnddev/)",
            theme.source.label()
        )),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Schema: ", k_style),
        Span::raw(format!("version {}", theme.version)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Status: ", k_style),
        Span::raw(
            status
                .clone()
                .unwrap_or_else(|| "OK (loaded cleanly)".to_string()),
        ),
    ]));
    lines.push(Line::from(""));
    let rule = "─".repeat(width.saturating_sub(4).clamp(12, 50));
    lines.push(Line::from(Span::styled(format!("  {rule}"), div_style)));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Loaded color tokens (sampled)",
        h_style,
    )));
    lines.push(Line::from(""));

    let tokens: [(&str, Color, &str); 28] = [
        ("base_background", theme.base_background, "app_shell base"),
        ("body_background", theme.body_background, "body panes"),
        (
            "modal_background",
            theme.modal_background,
            "modals & toasts",
        ),
        ("text_primary", theme.text_primary, "primary text"),
        ("text_secondary", theme.text_secondary, "muted text"),
        ("text_disabled", theme.text_disabled, "disabled text"),
        ("text_inverse", theme.text_inverse, "inverted text"),
        ("text_labels", theme.text_labels, "labels at rest"),
        ("text_active_focus", theme.text_active_focus, "focus + keys"),
        ("modal_labels", theme.modal_labels, "modal labels"),
        ("modal_header", theme.modal_header, "section titles bold"),
        (
            "selected_background",
            theme.selected_background,
            "selected row",
        ),
        ("border_default", theme.border_default, "idle pane border"),
        ("border_active", theme.border_active, "focused pane border"),
        (
            "input_border_default",
            theme.input_border_default,
            "idle inputs",
        ),
        (
            "input_border_focus",
            theme.input_border_focus,
            "focused inputs",
        ),
        (
            "input_text_default",
            theme.input_text_default,
            "idle input text",
        ),
        (
            "input_text_focus",
            theme.input_text_focus,
            "focused input text",
        ),
        ("cursor", theme.cursor, "input cursor overlay"),
        ("success", theme.success, "success toasts"),
        ("warning", theme.warning, "warning toasts"),
        ("error", theme.error, "error toasts"),
        ("info", theme.info, "info toasts"),
        ("folders", theme.folders, "tree folders"),
        ("files", theme.files, "tree files"),
        ("links", theme.links, "wikilinks / symlinks"),
        ("scrollbar", theme.scrollbar, "scrollbars"),
        ("scrollbar_hover", theme.scrollbar_hover, "scrollbar thumb"),
    ];
    for (name, color, role) in tokens {
        lines.push(Line::from(Span::raw(format!(
            "  {name:<22} {}   ({role})",
            color_to_hex(color)
        ))));
    }
    Text::from(lines)
}

pub fn count_wrapped_lines(text: &Text) -> usize {
    text.lines.len()
}
