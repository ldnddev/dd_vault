//! ropey buffer + vim-flavored mode layer.

use std::collections::HashMap;
use std::path::PathBuf;

use ropey::Rope;

mod error;
mod keys;

pub use error::Error;
pub use keys::Key;

const MAX_OPEN_BYTES: u64 = 2_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Normal,
    Insert,
    Visual,
    Command,
    Search,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
            Self::Command => "COMMAND",
            Self::Search => "SEARCH",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    None,
    G,
    D,
    Y,
    Mark,
    Jump,
    Register,
}

#[derive(Clone, Debug, Default)]
struct Yank {
    text: String,
    linewise: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitOp {
    Pull,
    Push,
    Commit,
    Status,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Save,
    Quit,
    QuitForce,
    SaveQuit,
    Open(String),
    Help,
    WikiPicker,
    Git(GitOp),
    Daily,
    AiToggle,
    AiOn,
    AiOff,
    AiPrompt(String),
    Error(String),
    Info(String),
    /// Yanked text (`y`, `yy`, visual `y`) for the OS clipboard.
    CopyClipboard(String),
}

#[derive(Clone, Debug)]
pub struct Editor {
    rope: Rope,
    cursor: usize,
    preferred_col: usize,
    pub mode: Mode,
    pub path: Option<PathBuf>,
    pub rel: Option<String>,
    pub dirty: bool,
    pub scroll: usize,
    /// Visual rows of `scroll`'s buffer line that sit above the viewport.
    pub scroll_off: usize,
    undo: Vec<(Rope, usize)>,
    undo_at: usize,
    visual_anchor: Option<usize>,
    visual_line: bool,
    unnamed: Yank,
    registers: HashMap<char, Yank>,
    pending_register: Option<char>,
    marks: HashMap<char, usize>,
    search: String,
    count: usize,
    pending: Pending,
    pub cmdline: String,
}

impl Default for Editor {
    fn default() -> Self {
        Self::empty()
    }
}

impl Editor {
    pub fn empty() -> Self {
        let rope = Rope::new();
        Self {
            undo: vec![(rope.clone(), 0)],
            undo_at: 0,
            rope,
            cursor: 0,
            preferred_col: 0,
            mode: Mode::Normal,
            path: None,
            rel: None,
            dirty: false,
            scroll: 0,
            scroll_off: 0,
            visual_anchor: None,
            visual_line: false,
            unnamed: Yank::default(),
            registers: HashMap::new(),
            pending_register: None,
            marks: HashMap::new(),
            search: String::new(),
            count: 0,
            pending: Pending::None,
            cmdline: String::new(),
        }
    }

    pub fn open(path: PathBuf, rel: String) -> Result<Self, Error> {
        let meta = std::fs::metadata(&path)?;
        if meta.len() > MAX_OPEN_BYTES {
            return Err(Error::TooLarge(path));
        }
        let text = std::fs::read_to_string(&path)?;
        let mut ed = Self::empty();
        ed.rope = Rope::from_str(&text);
        ed.path = Some(path);
        ed.rel = Some(rel);
        ed.undo = vec![(ed.rope.clone(), 0)];
        ed.undo_at = 0;
        Ok(ed)
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines().max(1)
    }

    pub fn line(&self, idx: usize) -> String {
        if idx >= self.rope.len_lines() {
            return String::new();
        }
        let mut s = self.rope.line(idx).to_string();
        if s.ends_with('\n') {
            s.pop();
            if s.ends_with('\r') {
                s.pop();
            }
        }
        s
    }

    pub fn cursor_line_col(&self) -> (usize, usize) {
        let line = self
            .rope
            .char_to_line(self.cursor.min(self.rope.len_chars()));
        let start = self.rope.line_to_char(line);
        (line, self.cursor.saturating_sub(start))
    }

    /// Gutter width in columns, including the trailing space after the number.
    pub fn gutter_cols(&self) -> u16 {
        (self.line_count().to_string().len().max(3) + 1) as u16
    }

    pub fn pos_at(&self, line: usize, col: usize, insert: bool) -> usize {
        if self.rope.len_chars() == 0 {
            return 0;
        }
        let line = line.min(self.line_count().saturating_sub(1));
        let start = self.line_start(line);
        let end = if insert {
            self.line_insert_end(line)
        } else {
            self.line_last_char(line)
        };
        let max_col = end.saturating_sub(start);
        start + col.min(max_col)
    }

    /// Click: place caret. Keeps INSERT; otherwise NORMAL. Clears visual.
    pub fn click_to(&mut self, line: usize, col: usize) {
        if matches!(self.mode, Mode::Command | Mode::Search | Mode::Visual) {
            self.cmdline.clear();
            self.visual_anchor = None;
            self.mode = Mode::Normal;
        }
        let insert = self.mode == Mode::Insert;
        self.cursor = self.pos_at(line, col, insert);
        self.sync_col();
    }

    /// Start a mouse selection (charwise or linewise visual).
    pub fn begin_mouse_select(&mut self, line: usize, col: usize, linewise: bool) {
        self.cmdline.clear();
        self.cursor = self.pos_at(line, col, true);
        self.visual_anchor = Some(self.cursor);
        self.visual_line = linewise;
        self.mode = Mode::Visual;
        self.sync_col();
    }

    pub fn update_mouse_select(&mut self, line: usize, col: usize) {
        if self.mode != Mode::Visual {
            return;
        }
        self.cursor = self.pos_at(line, col, true);
        self.sync_col();
    }

    /// Drop visual if the selection never moved off the anchor.
    pub fn finish_mouse_select(&mut self) {
        if self.mode != Mode::Visual {
            return;
        }
        if self.visual_line {
            return;
        }
        if self.visual_anchor == Some(self.cursor) {
            self.mode = Mode::Normal;
            self.visual_anchor = None;
        }
    }

    pub fn ensure_scroll(&mut self, height: usize) {
        self.ensure_scroll_wrapped(height, usize::MAX);
    }

    /// Keep the caret's wrapped row on screen. `width` is the text columns
    /// available beside the gutter; `usize::MAX` disables wrapping.
    pub fn ensure_scroll_wrapped(&mut self, height: usize, width: usize) {
        if height == 0 {
            return;
        }
        let width = Self::view_width(width);
        let caret = self.caret_pos();
        let cursor_at = self.flat_of(caret.0, Self::row_of(caret.1, width), width, caret);
        let mut origin = self.origin_flat(width, caret);
        if cursor_at < origin {
            origin = cursor_at;
        } else if cursor_at >= origin.saturating_add(height) {
            origin = cursor_at + 1 - height;
        }
        let max_origin = self.total_rows(width, caret).saturating_sub(height);
        if origin > max_origin {
            origin = max_origin;
        }
        self.set_origin(origin, width, caret);
    }

    /// Move the viewport by `delta` visual rows, then pull it back so the caret stays visible.
    pub fn scroll_wrapped(&mut self, delta: isize, height: usize, width: usize) {
        if height == 0 {
            return;
        }
        let width = Self::view_width(width);
        let caret = self.caret_pos();
        let origin = (self.origin_flat(width, caret) as isize + delta).max(0) as usize;
        self.set_origin(origin, width, caret);
        self.ensure_scroll_wrapped(height, width);
    }

    /// Map a viewport row and a column inside the text (not the gutter) to a buffer position.
    pub fn hit_wrapped(&self, view_row: usize, content_col: usize, width: usize) -> (usize, usize) {
        let width = Self::view_width(width);
        let caret = self.caret_pos();
        let last = self.line_count().saturating_sub(1);
        let mut idx = self.scroll.min(last);
        let mut off = self.scroll_off;
        let mut row_left = view_row;
        loop {
            let rows = self.line_rows_at(idx, width, caret);
            let off_clamped = off.min(rows.saturating_sub(1));
            let visible = rows - off_clamped;
            if row_left < visible || idx == last {
                let wrap_row = off_clamped + row_left.min(visible.saturating_sub(1));
                let col = if width == usize::MAX {
                    content_col
                } else {
                    wrap_row.saturating_mul(width).saturating_add(content_col)
                };
                return (idx, col);
            }
            row_left -= visible;
            off = 0;
            idx += 1;
        }
    }

    fn view_width(width: usize) -> usize {
        if width == 0 {
            1
        } else {
            width
        }
    }

    fn caret_pos(&self) -> (usize, usize) {
        self.cursor_line_col()
    }

    fn row_of(col: usize, width: usize) -> usize {
        if width == usize::MAX {
            0
        } else {
            col / width
        }
    }

    fn line_char_len(&self, idx: usize) -> usize {
        let lines = self.rope.len_lines();
        if idx >= lines {
            return 0;
        }
        let start = self.rope.line_to_char(idx);
        let end = if idx + 1 >= lines {
            self.rope.len_chars()
        } else {
            self.rope.line_to_char(idx + 1)
        };
        let mut len = end.saturating_sub(start);
        if len > 0 && self.rope.char(end - 1) == '\n' {
            len -= 1;
            if len > 0 && self.rope.char(start + len - 1) == '\r' {
                len -= 1;
            }
        }
        len
    }

    /// Visual rows this buffer line occupies, including the insert caret past a full row.
    fn line_rows_at(&self, idx: usize, width: usize, caret: (usize, usize)) -> usize {
        let len = self.line_char_len(idx);
        let base = if width == usize::MAX || len == 0 {
            1
        } else {
            len.div_ceil(width)
        };
        if idx == caret.0 {
            base.max(Self::row_of(caret.1, width) + 1)
        } else {
            base
        }
    }

    fn total_rows(&self, width: usize, caret: (usize, usize)) -> usize {
        (0..self.line_count())
            .map(|i| self.line_rows_at(i, width, caret))
            .sum()
    }

    fn flat_of(&self, line: usize, row: usize, width: usize, caret: (usize, usize)) -> usize {
        let line = line.min(self.line_count().saturating_sub(1));
        let mut n = 0usize;
        for i in 0..line {
            n += self.line_rows_at(i, width, caret);
        }
        let rows = self.line_rows_at(line, width, caret);
        n + row.min(rows.saturating_sub(1))
    }

    fn origin_flat(&self, width: usize, caret: (usize, usize)) -> usize {
        let last = self.line_count().saturating_sub(1);
        let scroll = self.scroll.min(last);
        let mut n = 0usize;
        for i in 0..scroll {
            n += self.line_rows_at(i, width, caret);
        }
        let rows = self.line_rows_at(scroll, width, caret);
        n + self.scroll_off.min(rows.saturating_sub(1))
    }

    fn set_origin(&mut self, mut flat: usize, width: usize, caret: (usize, usize)) {
        let nlines = self.line_count();
        for i in 0..nlines {
            let rows = self.line_rows_at(i, width, caret);
            if flat < rows || i + 1 == nlines {
                self.scroll = i;
                self.scroll_off = flat.min(rows.saturating_sub(1));
                return;
            }
            flat -= rows;
        }
        self.scroll = 0;
        self.scroll_off = 0;
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// Reload file contents from disk, keeping caret/scroll when possible.
    pub fn reload_from_disk(&mut self) -> Result<(), Error> {
        let path = self.path.clone().ok_or(Error::NoPath)?;
        let rel = self.rel.clone().ok_or(Error::NoPath)?;
        let cursor = self.cursor;
        let scroll = self.scroll;
        let scroll_off = self.scroll_off;
        let preferred = self.preferred_col;
        let mode = self.mode;
        let next = Self::open(path, rel)?;
        let len = next.rope.len_chars();
        self.rope = next.rope;
        self.undo = next.undo;
        self.undo_at = next.undo_at;
        self.dirty = false;
        self.cursor = cursor.min(len);
        self.scroll = scroll;
        self.scroll_off = scroll_off;
        self.preferred_col = preferred;
        self.mode = match mode {
            Mode::Insert | Mode::Normal => mode,
            _ => Mode::Normal,
        };
        self.visual_anchor = None;
        self.cmdline.clear();
        Ok(())
    }

    pub fn save_to_disk(&self) -> Result<(), Error> {
        let Some(path) = &self.path else {
            return Err(Error::NoPath);
        };
        std::fs::write(path, self.text())?;
        Ok(())
    }

    pub fn handle(&mut self, key: Key) -> Action {
        match self.mode {
            Mode::Insert => self.handle_insert(key),
            Mode::Normal => self.handle_normal(key),
            Mode::Visual => self.handle_visual(key),
            Mode::Command => self.handle_command(key),
            Mode::Search => self.handle_search(key),
        }
    }

    fn take_count(&mut self) -> usize {
        let n = if self.count == 0 { 1 } else { self.count };
        self.count = 0;
        n
    }

    fn snapshot(&mut self) {
        self.undo.truncate(self.undo_at + 1);
        self.undo.push((self.rope.clone(), self.cursor));
        self.undo_at = self.undo.len() - 1;
    }

    fn handle_insert(&mut self, key: Key) -> Action {
        match key {
            Key::Esc => {
                self.mode = Mode::Normal;
                if self.cursor > 0 && !self.at_line_start() {
                    let ch = self.char_at(self.cursor.saturating_sub(1));
                    if ch != Some('\n') {
                        self.cursor -= 1;
                    }
                }
                self.sync_col();
                Action::None
            }
            Key::Enter => {
                self.insert_str("\n");
                Action::None
            }
            Key::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.rope.remove(self.cursor..self.cursor + 1);
                    self.dirty = true;
                    self.sync_col();
                }
                Action::None
            }
            Key::Char(c) => {
                let wiki = c == '[' && self.char_at(self.cursor.saturating_sub(1)) == Some('[');
                self.insert_str(&c.to_string());
                if wiki {
                    Action::WikiPicker
                } else {
                    Action::None
                }
            }
            Key::Tab => {
                self.insert_str("  ");
                Action::None
            }
            Key::Left => {
                self.move_left_insert();
                Action::None
            }
            Key::Right => {
                self.move_right_insert();
                Action::None
            }
            Key::Up => {
                self.move_vert_insert(-1);
                Action::None
            }
            Key::Down => {
                self.move_vert_insert(1);
                Action::None
            }
            Key::Home => {
                self.cursor = self.line_start(self.cursor_line_col().0);
                self.sync_col();
                Action::None
            }
            Key::End => {
                self.cursor = self.line_insert_end(self.cursor_line_col().0);
                self.sync_col();
                Action::None
            }
            _ => Action::None,
        }
    }

    fn handle_normal(&mut self, key: Key) -> Action {
        match (self.pending, key) {
            (Pending::Register, Key::Char(c)) if c.is_ascii_alphabetic() => {
                self.pending_register = Some(c);
                self.pending = Pending::None;
                return Action::None;
            }
            (Pending::Mark, Key::Char(c)) if c.is_ascii_lowercase() => {
                self.marks.insert(c, self.cursor);
                self.pending = Pending::None;
                return Action::None;
            }
            (Pending::Jump, Key::Char(c)) if c.is_ascii_lowercase() => {
                if let Some(&pos) = self.marks.get(&c) {
                    self.cursor = pos.min(self.rope.len_chars());
                    self.sync_col();
                }
                self.pending = Pending::None;
                return Action::None;
            }
            (Pending::G, Key::Char('g')) => {
                self.pending = Pending::None;
                self.go_line(1);
                return Action::None;
            }
            (Pending::D, Key::Char('d')) => {
                self.pending = Pending::None;
                let n = self.take_count();
                self.delete_lines(n);
                return Action::None;
            }
            (Pending::Y, Key::Char('y')) => {
                self.pending = Pending::None;
                let n = self.take_count();
                return self.yank_lines(n);
            }
            (Pending::D, key) => {
                self.pending = Pending::None;
                if let Some(end) = self.motion_end(key) {
                    self.delete_range(self.cursor.min(end), self.cursor.max(end), false);
                }
                return Action::None;
            }
            (Pending::Y, key) => {
                self.pending = Pending::None;
                if let Some(end) = self.motion_end(key) {
                    return self.yank_range(self.cursor.min(end), self.cursor.max(end), false);
                }
                return Action::None;
            }
            (Pending::G, _) => {
                self.pending = Pending::None;
            }
            _ => {}
        }

        match key {
            Key::Char(c) if c.is_ascii_digit() && (c != '0' || self.count > 0) => {
                self.count = self
                    .count
                    .saturating_mul(10)
                    .saturating_add((c as u8 - b'0') as usize);
                Action::None
            }
            Key::Char('"') => {
                self.pending = Pending::Register;
                Action::None
            }
            Key::Char('m') => {
                self.pending = Pending::Mark;
                Action::None
            }
            Key::Char('`') => {
                self.pending = Pending::Jump;
                Action::None
            }
            Key::Char('g') => {
                self.pending = Pending::G;
                Action::None
            }
            Key::Char('d') => {
                self.pending = Pending::D;
                Action::None
            }
            Key::Char('y') => {
                self.pending = Pending::Y;
                Action::None
            }
            Key::Char('i') => {
                self.take_count();
                self.snapshot();
                self.mode = Mode::Insert;
                Action::None
            }
            Key::Char('a') => {
                self.take_count();
                self.snapshot();
                if self.cursor < self.rope.len_chars() && self.char_at(self.cursor) != Some('\n') {
                    self.cursor += 1;
                }
                self.mode = Mode::Insert;
                Action::None
            }
            Key::Char('o') => {
                self.take_count();
                self.snapshot();
                let end = self.line_break_idx(self.cursor_line_col().0);
                self.cursor = end;
                self.insert_str("\n");
                self.mode = Mode::Insert;
                Action::None
            }
            Key::Char('O') => {
                self.take_count();
                self.snapshot();
                let start = self.line_start(self.cursor_line_col().0);
                self.cursor = start;
                self.insert_str("\n");
                self.cursor = start;
                self.mode = Mode::Insert;
                Action::None
            }
            Key::Char('v') => {
                self.take_count();
                self.visual_anchor = Some(self.cursor);
                self.visual_line = false;
                self.mode = Mode::Visual;
                Action::None
            }
            Key::Char('V') => {
                self.take_count();
                self.visual_anchor = Some(self.cursor);
                self.visual_line = true;
                self.mode = Mode::Visual;
                Action::None
            }
            Key::Char('p') => {
                self.paste(false);
                Action::None
            }
            Key::Char('P') => {
                self.paste(true);
                Action::None
            }
            Key::Char('u') => {
                self.undo();
                Action::None
            }
            Key::Ctrl('r') => {
                self.redo();
                Action::None
            }
            Key::Char('G') => {
                let n = self.count;
                self.count = 0;
                if n == 0 {
                    self.go_line(self.line_count());
                } else {
                    self.go_line(n);
                }
                Action::None
            }
            Key::Char('/') => {
                self.mode = Mode::Search;
                self.cmdline.clear();
                Action::None
            }
            Key::Char(':') => {
                self.mode = Mode::Command;
                self.cmdline.clear();
                Action::None
            }
            Key::Char('n') => {
                self.find_next(true);
                Action::None
            }
            Key::Char('N') => {
                self.find_next(false);
                Action::None
            }
            other => {
                let times = self.take_count();
                for _ in 0..times {
                    self.apply_motion_key(other);
                }
                Action::None
            }
        }
    }

    fn handle_visual(&mut self, key: Key) -> Action {
        match key {
            Key::Esc => {
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                Action::None
            }
            Key::Char('d') => {
                let (a, b, linewise) = self.visual_range();
                self.delete_range(a, b, linewise);
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                Action::None
            }
            Key::Char('y') => {
                let (a, b, linewise) = self.visual_range();
                let action = self.yank_range(a, b, linewise);
                self.mode = Mode::Normal;
                self.visual_anchor = None;
                action
            }
            other => {
                self.apply_motion_key(other);
                Action::None
            }
        }
    }

    fn handle_command(&mut self, key: Key) -> Action {
        match key {
            Key::Esc => {
                self.mode = Mode::Normal;
                self.cmdline.clear();
                Action::None
            }
            Key::Backspace => {
                if self.cmdline.is_empty() {
                    self.mode = Mode::Normal;
                } else {
                    self.cmdline.pop();
                }
                Action::None
            }
            Key::Enter => {
                let cmd = self.cmdline.trim().to_string();
                self.cmdline.clear();
                self.mode = Mode::Normal;
                self.run_command(&cmd)
            }
            Key::Char(c) => {
                self.cmdline.push(c);
                Action::None
            }
            _ => Action::None,
        }
    }

    fn handle_search(&mut self, key: Key) -> Action {
        match key {
            Key::Esc => {
                self.mode = Mode::Normal;
                self.cmdline.clear();
                Action::None
            }
            Key::Backspace => {
                if self.cmdline.is_empty() {
                    self.mode = Mode::Normal;
                } else {
                    self.cmdline.pop();
                }
                Action::None
            }
            Key::Enter => {
                self.search = self.cmdline.clone();
                self.cmdline.clear();
                self.mode = Mode::Normal;
                self.find_next(true);
                Action::None
            }
            Key::Char(c) => {
                self.cmdline.push(c);
                Action::None
            }
            _ => Action::None,
        }
    }

    fn run_command(&mut self, cmd: &str) -> Action {
        let mut parts = cmd.split_whitespace();
        match parts.next().unwrap_or("") {
            "w" | "write" => Action::Save,
            "q" | "quit" => Action::Quit,
            "q!" => Action::QuitForce,
            "wq" | "x" => Action::SaveQuit,
            "e" | "edit" => match parts.next() {
                Some(p) => Action::Open(p.to_string()),
                None => Action::Error("E32: No file name".into()),
            },
            "help" => Action::Help,
            "daily" => Action::Daily,
            "ai" => {
                let rest: Vec<&str> = parts.collect();
                match rest.first().copied() {
                    None => Action::AiToggle,
                    Some("on") if rest.len() == 1 => Action::AiOn,
                    Some("off") if rest.len() == 1 => Action::AiOff,
                    _ => Action::AiPrompt(rest.join(" ")),
                }
            }
            "git" => match parts.next() {
                None | Some("status") => Action::Git(GitOp::Status),
                Some("pull") => Action::Git(GitOp::Pull),
                Some("push") => Action::Git(GitOp::Push),
                Some("commit") => Action::Git(GitOp::Commit),
                Some(other) => Action::Error(format!(
                    "unknown git subcommand: {other} (pull|push|commit)"
                )),
            },
            "" => Action::None,
            other => Action::Error(format!("E492: Not an editor command: {other}")),
        }
    }

    fn apply_motion_key(&mut self, key: Key) {
        match key {
            Key::Char('h') | Key::Left => self.move_left(),
            Key::Char('l') | Key::Right => self.move_right(),
            Key::Char('j') | Key::Down => self.move_vert(1),
            Key::Char('k') | Key::Up => self.move_vert(-1),
            Key::Char('0') | Key::Home => {
                self.cursor = self.line_start(self.cursor_line_col().0);
                self.sync_col();
            }
            Key::Char('$') | Key::End => {
                self.cursor = self.line_last_char(self.cursor_line_col().0);
                self.sync_col();
            }
            Key::Char('w') => self.word_forward(),
            Key::Char('b') => self.word_back(),
            Key::Char('e') => self.word_end(),
            Key::PageDown => self.move_vert(10),
            Key::PageUp => self.move_vert(-10),
            _ => {}
        }
    }

    fn motion_end(&mut self, key: Key) -> Option<usize> {
        let start = self.cursor;
        self.apply_motion_key(key);
        let end = self.cursor;
        self.cursor = start;
        if start == end {
            None
        } else {
            Some(end)
        }
    }

    fn insert_str(&mut self, s: &str) {
        self.rope.insert(self.cursor, s);
        self.cursor += s.chars().count();
        self.dirty = true;
        self.sync_col();
    }

    fn char_at(&self, i: usize) -> Option<char> {
        if i >= self.rope.len_chars() {
            None
        } else {
            Some(self.rope.char(i))
        }
    }

    fn at_line_start(&self) -> bool {
        let (line, col) = self.cursor_line_col();
        let _ = line;
        col == 0
    }

    fn line_start(&self, line: usize) -> usize {
        let line = line.min(self.line_count().saturating_sub(1));
        self.rope.line_to_char(line)
    }

    fn line_break_idx(&self, line: usize) -> usize {
        if line + 1 >= self.rope.len_lines() {
            self.rope.len_chars()
        } else {
            self.rope.line_to_char(line + 1)
        }
    }

    fn line_last_char(&self, line: usize) -> usize {
        let start = self.line_start(line);
        let brk = self.line_break_idx(line);
        if brk > start && self.char_at(brk - 1) == Some('\n') {
            if brk - 1 > start {
                brk - 2
            } else {
                start
            }
        } else if brk > start {
            brk - 1
        } else {
            start
        }
    }

    fn sync_col(&mut self) {
        let (_, col) = self.cursor_line_col();
        self.preferred_col = col;
    }

    fn move_left(&mut self) {
        if self.cursor > 0 && self.char_at(self.cursor - 1) != Some('\n') {
            self.cursor -= 1;
            self.sync_col();
        }
    }

    fn move_right(&mut self) {
        let (line, _) = self.cursor_line_col();
        let last = self.line_last_char(line);
        if self.cursor < last {
            self.cursor += 1;
            self.sync_col();
        }
    }

    fn move_vert(&mut self, delta: i32) {
        let (line, _) = self.cursor_line_col();
        let dest = (line as i32 + delta).clamp(0, self.line_count() as i32 - 1) as usize;
        let start = self.line_start(dest);
        let last = self.line_last_char(dest);
        let max_col = last.saturating_sub(start);
        let col = self.preferred_col.min(max_col);
        self.cursor = start + col;
    }

    /// Caret may sit after the last character (the insert point at EOL/EOF).
    fn line_insert_end(&self, line: usize) -> usize {
        let start = self.line_start(line);
        let brk = self.line_break_idx(line);
        if brk > start && self.char_at(brk - 1) == Some('\n') {
            brk - 1
        } else {
            brk
        }
    }

    fn move_left_insert(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.sync_col();
        }
    }

    fn move_right_insert(&mut self) {
        if self.cursor < self.rope.len_chars() {
            self.cursor += 1;
            self.sync_col();
        }
    }

    fn move_vert_insert(&mut self, delta: i32) {
        let (line, _) = self.cursor_line_col();
        let dest = (line as i32 + delta).clamp(0, self.line_count() as i32 - 1) as usize;
        let start = self.line_start(dest);
        let end = self.line_insert_end(dest);
        let max_col = end.saturating_sub(start);
        let col = self.preferred_col.min(max_col);
        self.cursor = start + col;
    }

    /// Insert `s` at the caret (used by the `[[` wiki picker).
    pub fn insert_text(&mut self, s: &str) {
        self.insert_str(s);
    }

    /// Insert `s` at the caret as its own undo step (clipboard image, snippets).
    pub fn insert_snippet(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.snapshot();
        self.insert_str(s);
    }

    fn go_line(&mut self, n: usize) {
        let dest = n.saturating_sub(1).min(self.line_count().saturating_sub(1));
        self.cursor = self.line_start(dest);
        self.sync_col();
    }

    fn is_word(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn word_forward(&mut self) {
        let len = self.rope.len_chars();
        if self.cursor >= len {
            return;
        }
        let c = self.rope.char(self.cursor);
        if Self::is_word(c) {
            while self.cursor < len && Self::is_word(self.rope.char(self.cursor)) {
                self.cursor += 1;
            }
        } else if !c.is_whitespace() {
            while self.cursor < len {
                let ch = self.rope.char(self.cursor);
                if ch.is_whitespace() || Self::is_word(ch) {
                    break;
                }
                self.cursor += 1;
            }
        }
        while self.cursor < len && self.rope.char(self.cursor).is_whitespace() {
            self.cursor += 1;
        }
        self.sync_col();
    }

    fn word_back(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        while self.cursor > 0 && self.rope.char(self.cursor).is_whitespace() {
            self.cursor -= 1;
        }
        if self.cursor == 0 {
            self.sync_col();
            return;
        }
        let word = Self::is_word(self.rope.char(self.cursor));
        while self.cursor > 0 {
            let prev = self.rope.char(self.cursor - 1);
            if prev.is_whitespace() {
                break;
            }
            if Self::is_word(prev) != word {
                break;
            }
            self.cursor -= 1;
        }
        self.sync_col();
    }

    fn word_end(&mut self) {
        let len = self.rope.len_chars();
        if self.cursor + 1 < len {
            self.cursor += 1;
        }
        while self.cursor < len && self.rope.char(self.cursor).is_whitespace() {
            self.cursor += 1;
        }
        if self.cursor >= len {
            self.cursor = len.saturating_sub(1);
            self.sync_col();
            return;
        }
        let word = Self::is_word(self.rope.char(self.cursor));
        while self.cursor + 1 < len {
            let next = self.rope.char(self.cursor + 1);
            if next.is_whitespace() {
                break;
            }
            if Self::is_word(next) != word {
                break;
            }
            self.cursor += 1;
        }
        self.sync_col();
    }

    fn store_yank(&mut self, yank: Yank) {
        if let Some(name) = self.pending_register.take() {
            self.registers.insert(name, yank.clone());
        }
        self.unnamed = yank;
    }

    fn yank_range(&mut self, a: usize, b: usize, linewise: bool) -> Action {
        let a = a.min(self.rope.len_chars());
        let b = b.min(self.rope.len_chars()).max(a);
        let text = self.rope.slice(a..b).to_string();
        let copy = if text.is_empty() {
            Action::None
        } else {
            Action::CopyClipboard(text.clone())
        };
        self.store_yank(Yank { text, linewise });
        copy
    }

    fn delete_range(&mut self, a: usize, b: usize, linewise: bool) {
        let a = a.min(self.rope.len_chars());
        let b = b.min(self.rope.len_chars()).max(a);
        if a == b {
            return;
        }
        self.snapshot();
        let text = self.rope.slice(a..b).to_string();
        self.store_yank(Yank { text, linewise });
        self.rope.remove(a..b);
        self.cursor = a.min(self.rope.len_chars());
        self.dirty = true;
        self.sync_col();
    }

    fn delete_lines(&mut self, n: usize) {
        let line = self.cursor_line_col().0;
        let start = self.line_start(line);
        let end_line = (line + n).min(self.line_count());
        let end = if end_line >= self.line_count() {
            self.rope.len_chars()
        } else {
            self.line_start(end_line)
        };
        self.delete_range(start, end, true);
    }

    fn yank_lines(&mut self, n: usize) -> Action {
        let line = self.cursor_line_col().0;
        let start = self.line_start(line);
        let end_line = (line + n).min(self.line_count());
        let end = if end_line >= self.line_count() {
            self.rope.len_chars()
        } else {
            self.line_start(end_line)
        };
        self.yank_range(start, end, true)
    }

    fn paste(&mut self, before: bool) {
        let yank = self
            .pending_register
            .and_then(|n| self.registers.get(&n).cloned())
            .unwrap_or_else(|| self.unnamed.clone());
        self.pending_register = None;
        if yank.text.is_empty() {
            return;
        }
        self.snapshot();
        if yank.linewise {
            let line = self.cursor_line_col().0;
            let idx = if before {
                self.line_start(line)
            } else {
                self.line_break_idx(line)
            };
            self.cursor = idx;
            let mut text = yank.text;
            if !text.ends_with('\n') {
                text.push('\n');
            }
            self.rope.insert(self.cursor, &text);
            self.dirty = true;
        } else {
            if !before && self.cursor < self.rope.len_chars() {
                self.cursor += 1;
            }
            self.rope.insert(self.cursor, &yank.text);
            self.cursor += yank.text.chars().count();
            self.dirty = true;
        }
        self.sync_col();
    }

    fn visual_range(&self) -> (usize, usize, bool) {
        let anchor = self.visual_anchor.unwrap_or(self.cursor);
        let (mut a, mut b) = if anchor <= self.cursor {
            (anchor, self.cursor)
        } else {
            (self.cursor, anchor)
        };
        if self.visual_line {
            a = self.line_start(self.rope.char_to_line(a.min(self.rope.len_chars())));
            let bline = self.rope.char_to_line(b.min(self.rope.len_chars()));
            b = self.line_break_idx(bline);
            (a, b, true)
        } else {
            if b < self.rope.len_chars() {
                b += 1;
            }
            (a, b, false)
        }
    }

    fn undo(&mut self) {
        if self.undo_at == 0 {
            return;
        }
        self.undo_at -= 1;
        let (rope, cursor) = self.undo[self.undo_at].clone();
        self.rope = rope;
        self.cursor = cursor.min(self.rope.len_chars());
        self.dirty = true;
        self.sync_col();
    }

    fn redo(&mut self) {
        if self.undo_at + 1 >= self.undo.len() {
            return;
        }
        self.undo_at += 1;
        let (rope, cursor) = self.undo[self.undo_at].clone();
        self.rope = rope;
        self.cursor = cursor.min(self.rope.len_chars());
        self.dirty = true;
        self.sync_col();
    }

    fn find_next(&mut self, forward: bool) {
        if self.search.is_empty() {
            return;
        }
        let hay = self.text();
        let needle = &self.search;
        if forward {
            let from = self.cursor.saturating_add(1).min(hay.chars().count());
            let byte_from = hay
                .char_indices()
                .nth(from)
                .map(|(b, _)| b)
                .unwrap_or(hay.len());
            if let Some(found) = hay[byte_from..].find(needle) {
                self.cursor = hay[..byte_from + found].chars().count();
            } else if let Some(found) = hay.find(needle) {
                self.cursor = hay[..found].chars().count();
            }
        } else {
            let prefix: String = hay.chars().take(self.cursor).collect();
            if let Some(found) = prefix.rfind(needle) {
                self.cursor = prefix[..found].chars().count();
            } else if let Some(found) = hay.rfind(needle) {
                self.cursor = hay[..found].chars().count();
            }
        }
        self.sync_col();
    }
}

impl Editor {
    pub fn visual_highlight(&self) -> Option<(usize, usize)> {
        if self.mode != Mode::Visual {
            return None;
        }
        let (a, b, _) = self.visual_range();
        Some((a, b))
    }

    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.visual_highlight()?;
        if a >= b {
            return None;
        }
        Some(self.rope.slice(a..b).to_string())
    }

    pub fn replace_range(&mut self, a: usize, b: usize, text: &str) {
        let a = a.min(self.rope.len_chars());
        let b = b.min(self.rope.len_chars()).max(a);
        self.snapshot();
        if a < b {
            self.rope.remove(a..b);
        }
        self.cursor = a;
        if !text.is_empty() {
            self.insert_str(text);
        } else {
            self.dirty = true;
            self.sync_col();
        }
        self.mode = Mode::Normal;
        self.visual_anchor = None;
    }
}

#[cfg(test)]
mod tests;
