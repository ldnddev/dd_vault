//! GPUI keystrokes mapped onto `dd_edit::Key`.

use dd_edit::Key;

/// The parts of a GPUI keystroke the editor map needs.
#[derive(Clone, Copy, Debug)]
pub struct KeyPress<'a> {
    pub key: &'a str,
    pub key_char: Option<&'a str>,
    pub control: bool,
    pub shift: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalKey {
    Quit,
    Save,
    Edit(Key),
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKey {
    Move(i32),
    Home,
    End,
    Leave,
    EnterDir,
    Activate,
    Filter,
    New,
    Rename,
    Delete,
    Ignore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKey {
    Cancel,
    Stop,
    Backspace,
    Type(char),
    Ignore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerKey {
    Move(i32),
    Home,
    End,
    Open,
    Forget,
    Quit,
    Ignore,
}

pub fn global_key(press: KeyPress<'_>) -> GlobalKey {
    if press.control && eq_ascii(press.key, "q") {
        return GlobalKey::Quit;
    }
    if press.control && eq_ascii(press.key, "s") {
        return GlobalKey::Save;
    }
    match map_edit_key(press) {
        Some(key) => GlobalKey::Edit(key),
        None => GlobalKey::None,
    }
}

/// Same letters as the TUI `map_edit_key`: Ctrl makes `Ctrl(char)`, otherwise
/// the named key or the typed character.
pub fn map_edit_key(press: KeyPress<'_>) -> Option<Key> {
    if press.control {
        let mut chars = press.key.chars();
        let ch = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        return Some(Key::Ctrl(if ch.is_ascii_alphabetic() {
            ch.to_ascii_lowercase()
        } else {
            ch
        }));
    }
    if let Some(key) = named(press.key) {
        return Some(key);
    }
    if let Some(ch) = one_char(press.key_char.unwrap_or("")) {
        return Some(Key::Char(ch));
    }
    one_char(press.key).map(Key::Char)
}

pub fn tree_key(press: KeyPress<'_>) -> TreeKey {
    if press.control {
        return TreeKey::Ignore;
    }
    match press.key {
        "down" | "j" => TreeKey::Move(1),
        "up" | "k" => TreeKey::Move(-1),
        "pagedown" | "page_down" => TreeKey::Move(10),
        "pageup" | "page_up" => TreeKey::Move(-10),
        "home" | "g" => TreeKey::Home,
        "end" | "G" => TreeKey::End,
        "left" | "h" => TreeKey::Leave,
        "right" | "l" => TreeKey::EnterDir,
        "enter" | "return" => TreeKey::Activate,
        "/" => TreeKey::Filter,
        "a" => TreeKey::New,
        "r" => TreeKey::Rename,
        "d" => TreeKey::Delete,
        _ => match press.key_char {
            Some("j") => TreeKey::Move(1),
            Some("k") => TreeKey::Move(-1),
            Some("g") => TreeKey::Home,
            Some("G") => TreeKey::End,
            Some("h") => TreeKey::Leave,
            Some("l") => TreeKey::EnterDir,
            Some("/") => TreeKey::Filter,
            Some("a") => TreeKey::New,
            Some("r") => TreeKey::Rename,
            Some("d") => TreeKey::Delete,
            _ => TreeKey::Ignore,
        },
    }
}

pub fn filter_key(press: KeyPress<'_>) -> FilterKey {
    if press.control {
        return FilterKey::Ignore;
    }
    match press.key {
        "escape" => FilterKey::Cancel,
        "enter" | "return" => FilterKey::Stop,
        "backspace" => FilterKey::Backspace,
        "space" => FilterKey::Type(' '),
        _ => match one_char(press.key_char.unwrap_or(press.key)) {
            Some(ch) if !ch.is_control() => FilterKey::Type(ch),
            _ => FilterKey::Ignore,
        },
    }
}

pub fn picker_key(press: KeyPress<'_>) -> PickerKey {
    if press.control && eq_ascii(press.key, "q") {
        return PickerKey::Quit;
    }
    if press.control {
        return PickerKey::Ignore;
    }
    match press.key {
        "down" | "j" => PickerKey::Move(1),
        "up" | "k" => PickerKey::Move(-1),
        "home" | "g" => PickerKey::Home,
        "end" | "G" => PickerKey::End,
        "enter" | "return" => PickerKey::Open,
        "d" => PickerKey::Forget,
        "escape" => PickerKey::Quit,
        _ => match press.key_char {
            Some("j") => PickerKey::Move(1),
            Some("k") => PickerKey::Move(-1),
            Some("g") => PickerKey::Home,
            Some("G") => PickerKey::End,
            Some("d") => PickerKey::Forget,
            _ => PickerKey::Ignore,
        },
    }
}

fn named(key: &str) -> Option<Key> {
    Some(match key {
        "escape" => Key::Esc,
        "enter" | "return" => Key::Enter,
        "backspace" => Key::Backspace,
        "tab" => Key::Tab,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "page_up" => Key::PageUp,
        "pagedown" | "page_down" => Key::PageDown,
        "space" => Key::Char(' '),
        other => {
            let rest = other
                .strip_prefix('f')
                .or_else(|| other.strip_prefix('F'))?;
            let n: u8 = rest.parse().ok()?;
            if (1..=12).contains(&n) {
                return Some(Key::F(n));
            }
            return None;
        }
    })
}

fn one_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() || ch.is_control() {
        return None;
    }
    Some(ch)
}

fn eq_ascii(key: &str, letter: &str) -> bool {
    key.eq_ignore_ascii_case(letter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press<'a>(key: &'a str, key_char: Option<&'a str>, control: bool) -> KeyPress<'a> {
        KeyPress {
            key,
            key_char,
            control,
            shift: false,
        }
    }

    #[test]
    fn letters_and_ctrl_match_the_tui_map() {
        assert_eq!(
            map_edit_key(press("s", Some("s"), false)),
            Some(Key::Char('s'))
        );
        assert_eq!(
            map_edit_key(press("a", Some("A"), false)),
            Some(Key::Char('A'))
        );
        assert_eq!(map_edit_key(press("s", None, true)), Some(Key::Ctrl('s')));
        assert_eq!(map_edit_key(press("S", None, true)), Some(Key::Ctrl('s')));
        assert_eq!(map_edit_key(press("up", None, true)), None);
    }

    #[test]
    fn named_keys_and_function_keys() {
        assert_eq!(map_edit_key(press("escape", None, false)), Some(Key::Esc));
        assert_eq!(map_edit_key(press("enter", None, false)), Some(Key::Enter));
        assert_eq!(
            map_edit_key(press("backspace", None, false)),
            Some(Key::Backspace)
        );
        assert_eq!(map_edit_key(press("tab", None, false)), Some(Key::Tab));
        assert_eq!(map_edit_key(press("up", None, false)), Some(Key::Up));
        assert_eq!(
            map_edit_key(press("pageup", None, false)),
            Some(Key::PageUp)
        );
        assert_eq!(
            map_edit_key(press("pagedown", None, false)),
            Some(Key::PageDown)
        );
        assert_eq!(map_edit_key(press("f1", None, false)), Some(Key::F(1)));
        assert_eq!(map_edit_key(press("f12", None, false)), Some(Key::F(12)));
        assert_eq!(map_edit_key(press("f13", None, false)), None);
    }

    #[test]
    fn ctrl_s_and_ctrl_q_are_global() {
        assert_eq!(global_key(press("s", None, true)), GlobalKey::Save);
        assert_eq!(global_key(press("q", None, true)), GlobalKey::Quit);
        assert_eq!(
            global_key(press("tab", None, false)),
            GlobalKey::Edit(Key::Tab)
        );
    }

    #[test]
    fn tree_and_filter_keys() {
        assert_eq!(tree_key(press("j", Some("j"), false)), TreeKey::Move(1));
        assert_eq!(tree_key(press("k", Some("k"), false)), TreeKey::Move(-1));
        assert_eq!(tree_key(press("h", Some("h"), false)), TreeKey::Leave);
        assert_eq!(tree_key(press("l", Some("l"), false)), TreeKey::EnterDir);
        assert_eq!(tree_key(press("enter", None, false)), TreeKey::Activate);
        assert_eq!(tree_key(press("/", Some("/"), false)), TreeKey::Filter);
        assert_eq!(tree_key(press("a", Some("a"), false)), TreeKey::New);
        assert_eq!(tree_key(press("r", Some("r"), false)), TreeKey::Rename);
        assert_eq!(tree_key(press("d", Some("d"), false)), TreeKey::Delete);
        assert_eq!(picker_key(press("d", Some("d"), false)), PickerKey::Forget);
        assert_eq!(filter_key(press("escape", None, false)), FilterKey::Cancel);
        assert_eq!(filter_key(press("enter", None, false)), FilterKey::Stop);
        assert_eq!(
            filter_key(press("n", Some("n"), false)),
            FilterKey::Type('n')
        );
        assert_eq!(
            filter_key(press("backspace", None, false)),
            FilterKey::Backspace
        );
    }
}
