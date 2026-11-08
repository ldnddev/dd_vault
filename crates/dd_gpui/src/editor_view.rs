//! Visible notes rows. Wrapping uses `dd_edit`'s break points.

use dd_edit::{Editor, Mode, visual_cursor, wrap_row_starts};
use dd_vault_core::Session;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caret {
    /// Block on this character. `text.chars().count()` is a phantom cell past the end.
    Block(usize),
    /// Bar before this character. `text.chars().count()` is a bar after the last one.
    Bar(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorRow {
    pub gutter: String,
    pub text: String,
    /// Character ranges inside `text` that are selected.
    pub selection: Vec<std::ops::Range<usize>>,
    pub caret: Option<Caret>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenChoice {
    Same,
    Load,
    Confirm,
}

/// Wheel delta in pixels. GPUI reports wheel-down as a negative y.
pub fn wheel_rows(delta_y: f32, line_h: f32) -> isize {
    if delta_y == 0.0 || line_h <= 0.0 {
        return 0;
    }
    let rows = (-delta_y / line_h).round() as isize;
    if rows == 0 {
        if delta_y < 0.0 { 1 } else { -1 }
    } else {
        rows
    }
}

pub fn choose_open(dirty: bool, current: Option<&str>, next: &str) -> OpenChoice {
    if current == Some(next) {
        OpenChoice::Same
    } else if dirty {
        OpenChoice::Confirm
    } else {
        OpenChoice::Load
    }
}

/// Fields this window writes. Reading view stays off disk.
pub struct SessionWrite<'a> {
    pub note: Option<String>,
    pub cursor: usize,
    pub scroll: usize,
    pub scroll_off: usize,
    pub wrap: bool,
    pub preview: bool,
    pub pane: &'a str,
    pub zen: bool,
    pub focus: bool,
    pub explore: bool,
    pub tree_split: u16,
    pub preview_split: u16,
}

/// Update the fields this window owns. `preview` is the split under the editor.
pub fn remember_session(session: &mut Session, write: SessionWrite<'_>) {
    session.note = write.note;
    session.cursor = write.cursor;
    session.scroll = write.scroll;
    session.scroll_off = write.scroll_off;
    session.wrap = Some(write.wrap);
    session.preview = Some(write.preview);
    session.pane = Some(write.pane.to_string());
    session.zen = write.zen;
    session.focus = write.focus;
    session.explore = write.explore;
    session.tree_split = write.tree_split;
    session.preview_split = write.preview_split;
}

pub fn prompt_line(editor: &Editor) -> Option<String> {
    match editor.mode {
        Mode::Command => Some(format!(":{}", editor.cmdline)),
        Mode::Search => Some(format!("/{}", editor.cmdline)),
        _ => None,
    }
}

/// `wrap_width` is `usize::MAX` when wrap is off. `clip_cols` cuts a non-wrapping row.
pub fn visible_editor_rows(
    editor: &Editor,
    wrap_width: usize,
    clip_cols: usize,
    rows: usize,
) -> Vec<EditorRow> {
    let rows = rows.max(1);
    let clip_cols = clip_cols.max(1);
    let (caret_line, caret_col) = editor.cursor_line_col();
    let show_caret = !matches!(editor.mode, Mode::Command | Mode::Search);
    let insert = editor.mode == Mode::Insert;
    let selection = editor.visual_highlight();
    let line_starts = line_char_starts(&editor.text());
    let gutter_cols = editor.gutter_cols() as usize;
    let num_width = gutter_cols.saturating_sub(1);

    let mut out = Vec::new();
    let last = editor.line_count().saturating_sub(1);
    let mut line_idx = editor.scroll.min(last);
    let mut skip = editor.scroll_off;
    while out.len() < rows && line_idx <= last {
        let line = editor.line(line_idx);
        let width = if wrap_width == usize::MAX {
            usize::MAX
        } else {
            wrap_width.max(1)
        };
        let starts = wrap_row_starts(&line, width);
        let len = line.chars().count();
        let caret_here = show_caret && caret_line == line_idx;
        let (caret_row, caret_vcol) = if caret_here {
            visual_cursor(&line, caret_col, width)
        } else {
            (0, 0)
        };
        let visual_rows = if caret_here {
            starts.len().max(caret_row + 1)
        } else {
            starts.len().max(1)
        };
        for vr in 0..visual_rows {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if out.len() >= rows {
                break;
            }
            let start = starts.get(vr).copied().unwrap_or(len);
            let end = starts.get(vr + 1).copied().unwrap_or(len);
            let mut text: String = line
                .chars()
                .skip(start)
                .take(end.saturating_sub(start))
                .collect();
            if wrap_width == usize::MAX {
                text = text.chars().take(clip_cols).collect();
            }
            let base = line_starts.get(line_idx).copied().unwrap_or(0);
            let mut selection_ranges = Vec::new();
            if let Some((a, b)) = selection {
                let row_a = base + start;
                let row_b = base + end;
                let from = a.max(row_a);
                let to = b.min(row_b);
                if from < to {
                    let mut local_a = from - base - start;
                    let mut local_b = to - base - start;
                    if wrap_width == usize::MAX {
                        local_a = local_a.min(clip_cols);
                        local_b = local_b.min(clip_cols);
                    }
                    if local_a < local_b {
                        selection_ranges.push(local_a..local_b);
                    }
                }
            }
            let mut caret = None;
            if caret_here && vr == caret_row {
                let shown = text.chars().count();
                if wrap_width == usize::MAX && caret_vcol > clip_cols {
                    caret = None;
                } else if insert {
                    caret = Some(Caret::Bar(caret_vcol.min(shown)));
                } else if caret_vcol < shown {
                    caret = Some(Caret::Block(caret_vcol));
                } else if wrap_width != usize::MAX || caret_vcol <= clip_cols {
                    text.push(' ');
                    caret = Some(Caret::Block(shown));
                }
            }
            let gutter = if vr == 0 {
                format!("{:>num_width$} ", line_idx + 1)
            } else {
                " ".repeat(gutter_cols)
            };
            out.push(EditorRow {
                gutter,
                text,
                selection: selection_ranges,
                caret,
            });
        }
        if line_idx == last {
            break;
        }
        line_idx += 1;
        skip = 0;
    }
    if out.is_empty() {
        out.push(EditorRow {
            gutter: format!("{:>num_width$} ", 1),
            text: String::new(),
            selection: Vec::new(),
            caret: if show_caret {
                Some(if insert {
                    Caret::Bar(0)
                } else {
                    Caret::Block(0)
                })
            } else {
                None
            },
        });
    }
    out
}

fn line_char_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut n = 0usize;
    for ch in text.chars() {
        n += 1;
        if ch == '\n' {
            starts.push(n);
        }
    }
    starts
}

pub fn pane_name(tree: bool, preview: bool) -> &'static str {
    if tree {
        "tree"
    } else if preview {
        "preview"
    } else {
        "editor"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_caret_on_the_first_character() {
        let mut editor = Editor::empty();
        editor.insert_text("hello");
        editor.restore_view(0, 0, 0);
        let rows = visible_editor_rows(&editor, usize::MAX, 20, 4);
        assert_eq!(rows[0].text, "hello");
        assert_eq!(rows[0].caret, Some(Caret::Block(0)));
        assert!(rows[0].gutter.trim().ends_with('1'));
    }

    #[test]
    fn wraps_on_the_shared_break_points() {
        let mut editor = Editor::empty();
        editor.insert_text("hello world");
        editor.restore_view(0, 0, 0);
        let rows = visible_editor_rows(&editor, 6, 6, 8);
        assert_eq!(rows[0].text, "hello ");
        assert_eq!(rows[1].text, "world");
        assert!(rows[1].gutter.chars().all(|ch| ch == ' '));
    }

    #[test]
    fn wrap_off_clips_and_hides_a_caret_past_the_edge() {
        let mut editor = Editor::empty();
        editor.insert_text("hello");
        let rows = visible_editor_rows(&editor, usize::MAX, 4, 3);
        assert_eq!(rows[0].text, "hell");
        assert_eq!(rows[0].caret, None);
    }

    #[test]
    fn insert_bar_sits_after_the_last_character() {
        let mut editor = Editor::empty();
        editor.insert_text("hello");
        editor.mode = Mode::Insert;
        let rows = visible_editor_rows(&editor, usize::MAX, 20, 3);
        assert_eq!(rows[0].text, "hello");
        assert_eq!(rows[0].caret, Some(Caret::Bar(5)));
    }

    #[test]
    fn wheel_down_is_a_positive_row_delta() {
        assert_eq!(wheel_rows(-20.0, 20.0), 1);
        assert_eq!(wheel_rows(40.0, 20.0), -2);
        assert_eq!(wheel_rows(-1.0, 20.0), 1);
        assert_eq!(wheel_rows(0.0, 20.0), 0);
    }

    #[test]
    fn dirty_open_confirms_and_session_keeps_preview() {
        assert_eq!(choose_open(true, Some("a.md"), "b.md"), OpenChoice::Confirm);
        assert_eq!(choose_open(false, Some("a.md"), "b.md"), OpenChoice::Load);
        assert_eq!(choose_open(true, Some("a.md"), "a.md"), OpenChoice::Same);
        let mut session = Session {
            preview: Some(false),
            zen: true,
            ..Session::default()
        };
        remember_session(
            &mut session,
            SessionWrite {
                note: Some("n.md".into()),
                cursor: 4,
                scroll: 1,
                scroll_off: 0,
                wrap: true,
                preview: false,
                pane: "editor",
                zen: true,
                focus: false,
                explore: false,
                tree_split: 0,
                preview_split: 0,
            },
        );
        assert_eq!(session.note.as_deref(), Some("n.md"));
        assert_eq!(session.cursor, 4);
        assert_eq!(session.preview, Some(false));
        assert!(session.zen);
        assert_eq!(session.pane.as_deref(), Some("editor"));
        remember_session(
            &mut session,
            SessionWrite {
                note: Some("n.md".into()),
                cursor: 4,
                scroll: 1,
                scroll_off: 0,
                wrap: true,
                preview: true,
                pane: "preview",
                zen: true,
                focus: false,
                explore: false,
                tree_split: 0,
                preview_split: 0,
            },
        );
        assert_eq!(session.preview, Some(true));
        assert!(session.zen);
        assert_eq!(session.pane.as_deref(), Some("preview"));
        remember_session(
            &mut session,
            SessionWrite {
                note: Some("n.md".into()),
                cursor: 4,
                scroll: 1,
                scroll_off: 0,
                wrap: true,
                preview: true,
                pane: "preview",
                zen: false,
                focus: true,
                explore: true,
                tree_split: 14,
                preview_split: 8,
            },
        );
        assert!(!session.zen);
        assert!(session.focus);
        assert!(session.explore);
        assert_eq!(session.tree_split, 14);
        assert_eq!(session.preview_split, 8);
    }
}
