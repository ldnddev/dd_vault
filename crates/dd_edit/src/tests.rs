use crate::{Action, Editor, Key, Mode};

fn ed(text: &str) -> Editor {
    let mut e = Editor::empty();
    if !text.is_empty() {
        e.handle(Key::Char('i'));
        for c in text.chars() {
            if c == '\n' {
                e.handle(Key::Enter);
            } else {
                e.handle(Key::Char(c));
            }
        }
        e.handle(Key::Esc);
        e.handle(Key::Char('g'));
        e.handle(Key::Char('g'));
        e.dirty = false;
    }
    e
}

#[test]
fn insert_arrows_move_caret() {
    let mut e = Editor::empty();
    e.handle(Key::Char('i'));
    for c in ['a', 'b', 'c'] {
        e.handle(Key::Char(c));
    }
    assert_eq!(e.cursor_line_col(), (0, 3));
    e.handle(Key::Left);
    e.handle(Key::Left);
    assert_eq!(e.cursor_line_col(), (0, 1));
    e.handle(Key::Char('X'));
    assert_eq!(e.text(), "aXbc");
    e.handle(Key::End);
    assert_eq!(e.cursor_line_col(), (0, 4));
    e.handle(Key::Home);
    assert_eq!(e.cursor_line_col(), (0, 0));
}

#[test]
fn insert_and_escape_to_normal() {
    let mut e = Editor::empty();
    e.handle(Key::Char('i'));
    assert_eq!(e.mode, Mode::Insert);
    e.handle(Key::Char('h'));
    e.handle(Key::Char('i'));
    e.handle(Key::Esc);
    assert_eq!(e.mode, Mode::Normal);
    assert_eq!(e.text(), "hi");
}

#[test]
fn hjkl_and_line_ends() {
    let mut e = ed("ab\ncd");
    e.handle(Key::Char('l'));
    assert_eq!(e.cursor_line_col(), (0, 1));
    e.handle(Key::Char('j'));
    assert_eq!(e.cursor_line_col(), (1, 1));
    e.handle(Key::Char('0'));
    assert_eq!(e.cursor_line_col(), (1, 0));
    e.handle(Key::Char('$'));
    assert_eq!(e.cursor_line_col(), (1, 1));
}

#[test]
fn dd_yy_p_and_undo() {
    let mut e = ed("one\ntwo\nthree");
    e.handle(Key::Char('y'));
    e.handle(Key::Char('y'));
    e.handle(Key::Char('j'));
    e.handle(Key::Char('p'));
    let text = e.text();
    assert!(
        text.contains("one\ntwo\none\n") || text.starts_with("one\ntwo\none"),
        "{text}"
    );
    e.handle(Key::Char('g'));
    e.handle(Key::Char('g'));
    e.handle(Key::Char('d'));
    e.handle(Key::Char('d'));
    assert!(!e.text().starts_with("one"));
    e.handle(Key::Char('u'));
    assert!(
        e.text().starts_with("one") || e.text().contains("one"),
        "{}",
        e.text()
    );
}

#[test]
fn word_motions() {
    let mut e = ed("foo bar");
    e.handle(Key::Char('w'));
    assert_eq!(e.cursor_line_col(), (0, 4));
    e.handle(Key::Char('b'));
    assert_eq!(e.cursor_line_col(), (0, 0));
    e.handle(Key::Char('e'));
    assert_eq!(e.cursor_line_col(), (0, 2));
}

#[test]
fn search_finds_text() {
    let mut e = ed("alpha beta alpha");
    e.handle(Key::Char('/'));
    for c in ['b', 'e', 't', 'a'] {
        e.handle(Key::Char(c));
    }
    e.handle(Key::Enter);
    assert_eq!(e.cursor_line_col(), (0, 6));
    e.handle(Key::Char('n'));
}

#[test]
fn insert_snippet_is_one_undo() {
    let mut e = Editor::empty();
    e.insert_snippet("XY");
    assert_eq!(e.text(), "XY");
    e.handle(Key::Char('u'));
    assert_eq!(e.text(), "");
}

#[test]
fn command_w_requests_save() {
    let mut e = ed("x");
    e.handle(Key::Char(':'));
    e.handle(Key::Char('w'));
    assert_eq!(e.handle(Key::Enter), Action::Save);
}

#[test]
fn command_git_subcommands() {
    use crate::GitOp;
    let mut e = ed("x");
    e.handle(Key::Char(':'));
    for c in ['g', 'i', 't', ' ', 'p', 'u', 'l', 'l'] {
        e.handle(Key::Char(c));
    }
    assert_eq!(e.handle(Key::Enter), Action::Git(GitOp::Pull));

    e.handle(Key::Char(':'));
    for c in ['g', 'i', 't', ' ', 'p', 'u', 's', 'h'] {
        e.handle(Key::Char(c));
    }
    assert_eq!(e.handle(Key::Enter), Action::Git(GitOp::Push));

    e.handle(Key::Char(':'));
    for c in ['g', 'i', 't', ' ', 'c', 'o', 'm', 'm', 'i', 't'] {
        e.handle(Key::Char(c));
    }
    assert_eq!(e.handle(Key::Enter), Action::Git(GitOp::Commit));

    e.handle(Key::Char(':'));
    for c in ['g', 'i', 't'] {
        e.handle(Key::Char(c));
    }
    assert_eq!(e.handle(Key::Enter), Action::Git(GitOp::Status));

    e.handle(Key::Char(':'));
    for c in ['d', 'a', 'i', 'l', 'y'] {
        e.handle(Key::Char(c));
    }
    assert_eq!(e.handle(Key::Enter), Action::Daily);

    e.handle(Key::Char(':'));
    for c in ['a', 'i'] {
        e.handle(Key::Char(c));
    }
    assert_eq!(e.handle(Key::Enter), Action::AiToggle);
    e.handle(Key::Char(':'));
    for c in ['a', 'i', ' ', 'o', 'n'] {
        e.handle(Key::Char(c));
    }
    assert_eq!(e.handle(Key::Enter), Action::AiOn);
    e.handle(Key::Char(':'));
    for c in ['a', 'i', ' ', 'h', 'i'] {
        e.handle(Key::Char(c));
    }
    assert_eq!(e.handle(Key::Enter), Action::AiPrompt("hi".into()));
}

#[test]
fn click_to_places_caret() {
    let mut e = ed("abcd\nefgh");
    e.click_to(1, 2);
    assert_eq!(e.cursor_line_col(), (1, 2));
    assert_eq!(e.mode, Mode::Normal);
}

#[test]
fn mouse_drag_selects_visual() {
    let mut e = ed("abcd\nefgh");
    e.begin_mouse_select(0, 1, false);
    e.update_mouse_select(1, 2);
    assert_eq!(e.mode, Mode::Visual);
    let (a, b) = e.visual_highlight().expect("sel");
    assert!(b > a, "{a} {b}");
    e.finish_mouse_select();
    assert_eq!(e.mode, Mode::Visual);
}

#[test]
fn mouse_click_without_drag_is_not_visual() {
    let mut e = ed("abcd");
    e.begin_mouse_select(0, 2, false);
    e.finish_mouse_select();
    assert_eq!(e.mode, Mode::Normal);
    assert_eq!(e.cursor_line_col(), (0, 2));
}

#[test]
fn visual_delete() {
    let mut e = ed("abcd");
    e.handle(Key::Char('v'));
    e.handle(Key::Char('l'));
    e.handle(Key::Char('d'));
    assert_eq!(e.mode, Mode::Normal);
    assert_eq!(e.text(), "cd");
}

#[test]
fn save_roundtrip() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("n.md");
    std::fs::write(&path, "hello\n").expect("write");
    let mut e = Editor::open(path.clone(), "n.md".into()).expect("open");
    e.handle(Key::Char('$'));
    e.handle(Key::Char('a'));
    e.handle(Key::Char('!'));
    e.handle(Key::Esc);
    e.save_to_disk().expect("save");
    let body = std::fs::read_to_string(&path).expect("read");
    assert!(body.contains("hello"), "{body}");
    assert!(body.contains('!'), "{body}");
}

#[test]
fn reload_from_disk_picks_up_external_edit() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("n.md");
    std::fs::write(&path, "one\n").expect("write");
    let mut e = Editor::open(path.clone(), "n.md".into()).expect("open");
    e.handle(Key::Char('l'));
    std::fs::write(&path, "two\nthree\n").expect("rewrite");
    e.reload_from_disk().expect("reload");
    assert_eq!(e.text(), "two\nthree\n");
    assert!(!e.dirty);
}

#[test]
fn marks_jump() {
    let mut e = ed("aa\nbb\ncc");
    e.handle(Key::Char('j'));
    e.handle(Key::Char('m'));
    e.handle(Key::Char('a'));
    e.handle(Key::Char('g'));
    e.handle(Key::Char('g'));
    e.handle(Key::Char('`'));
    e.handle(Key::Char('a'));
    assert_eq!(e.cursor_line_col().0, 1);
}

#[test]
fn wrap_keeps_caret_row_on_screen() {
    let mut e = ed(&"a".repeat(25));
    e.handle(Key::Char('$'));
    assert_eq!(e.cursor_line_col(), (0, 24));
    e.ensure_scroll_wrapped(2, 10);
    assert_eq!(e.scroll, 0);
    assert_eq!(e.scroll_off, 1);
}

#[test]
fn wrap_insert_past_full_row_gets_its_own_row() {
    let mut e = Editor::empty();
    e.handle(Key::Char('i'));
    for _ in 0..10 {
        e.handle(Key::Char('a'));
    }
    assert_eq!(e.cursor_line_col(), (0, 10));
    e.ensure_scroll_wrapped(1, 10);
    assert_eq!(e.scroll, 0);
    assert_eq!(e.scroll_off, 1);
    e.ensure_scroll_wrapped(2, 10);
    assert_eq!(e.scroll_off, 0);
}

#[test]
fn wrap_hit_maps_screen_row_to_column() {
    let e = ed(&format!("{}\n{}", "a".repeat(25), "bbb"));
    assert_eq!(e.hit_wrapped(0, 3, 10), (0, 3));
    assert_eq!(e.hit_wrapped(1, 0, 10), (0, 10));
    assert_eq!(e.hit_wrapped(2, 4, 10), (0, 24));
    assert_eq!(e.hit_wrapped(3, 1, 10), (1, 1));
}

#[test]
fn wrap_scroll_moves_by_visual_rows() {
    let mut e = Editor::empty();
    e.handle(Key::Char('i'));
    for _ in 0..50 {
        e.handle(Key::Char('a'));
    }
    e.handle(Key::Esc);
    e.handle(Key::Char('0'));
    e.handle(Key::Char('2'));
    e.handle(Key::Char('0'));
    e.handle(Key::Char('l'));
    assert_eq!(e.cursor_line_col(), (0, 20));
    e.ensure_scroll_wrapped(3, 10);
    assert_eq!(e.scroll_off, 0);
    e.scroll_wrapped(1, 3, 10);
    assert_eq!(e.scroll, 0);
    assert_eq!(e.scroll_off, 1);
    e.scroll_wrapped(20, 3, 10);
    assert_eq!(e.scroll_off, 2);
}

#[test]
fn unwrapped_scroll_still_tracks_buffer_lines() {
    let mut e = ed("a\nb\nc\nd\ne");
    e.handle(Key::Char('G'));
    e.scroll_off = 4;
    e.ensure_scroll_wrapped(2, usize::MAX);
    assert_eq!(e.scroll, 3);
    assert_eq!(e.scroll_off, 0);
}
