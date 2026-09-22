use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crate::app::{App, ConfirmKind, FinderKind, Leader, Modal, Pane};
use crate::theme::AppTheme;
use crate::toasts::ToastLevel;
use dd_vault_core::{init, Paths, Registry, Vault, VaultEntry};

fn send_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_event(Event::Key(KeyEvent::new(code, modifiers)))
        .expect("key");
}

fn chrome_app() -> App {
    let mut app = App::new(AppTheme::default());
    app.header_copy = "Files on disk. Opinions in git.".to_string();
    app
}

fn buffer_text(app: &mut App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| crate::draw::draw(frame, app))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..height {
        for x in 0..width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

#[test]
fn f1_opens_and_closes() {
    let mut app = chrome_app();
    send_key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    assert!(app.show_help);
    send_key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    assert!(!app.show_help);
}

#[test]
fn f2_opens_editor_and_esc_closes() {
    let mut app = chrome_app();
    send_key(&mut app, KeyCode::F(2), KeyModifiers::NONE);
    assert!(app.show_theme);
    assert!(app.theme_editor.is_some());
    send_key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(app.theme_editor.as_ref().unwrap().selected, 1);
    send_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.show_theme);
    assert!(app.theme_editor.is_none());
}

#[test]
fn ctrl_q_quits_bare_q_does_not() {
    let mut app = chrome_app();
    send_key(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(!app.should_quit);
    send_key(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
    assert!(app.should_quit);
}

#[test]
fn footer_starts_with_f1_help_then_f2_theme() {
    let app = chrome_app();
    let narrow = app.footer_hint(70);
    let wide = app.footer_hint(140);
    assert!(narrow.starts_with("F1:Help"), "{narrow}");
    assert!(narrow.contains("F2:Theme"), "{narrow}");
    assert!(wide.starts_with("F1: Help"), "{wide}");
    assert!(wide.contains("F2: Theme"), "{wide}");
}

#[test]
fn tab_and_click_move_focus() {
    let mut app = chrome_app();
    assert_eq!(app.pane, Pane::Tree);
    send_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(app.pane, Pane::Editor);
    let _ = buffer_text(&mut app, 100, 24);
    app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: app.tree_area.x + 1,
        row: app.tree_area.y + 1,
        modifiers: KeyModifiers::NONE,
    }))
    .expect("mouse");
    assert_eq!(app.pane, Pane::Tree);
}

#[test]
fn default_layout_snapshot_wide() {
    let mut app = chrome_app();
    let text = buffer_text(&mut app, 100, 24);
    insta::assert_snapshot!(text);
    assert!(text.contains("dd_vault"));
    assert!(text.contains("no vault"));
    assert!(text.contains("F1: Help"));
    assert!(text.contains("F2: Theme"));
    assert!(text.contains("NORMAL"));
}

#[test]
fn default_layout_snapshot_narrow() {
    let mut app = chrome_app();
    let text = buffer_text(&mut app, 80, 18);
    insta::assert_snapshot!(text);
    let first_lines: Vec<&str> = text.lines().take(3).collect();
    assert_eq!(first_lines.len(), 3);
    let footer = text.lines().last().unwrap();
    assert!(footer.contains("F1:"), "{footer}");
    assert!(footer.contains("F2:"), "{footer}");
}

#[test]
fn f1_help_snapshot() {
    let mut app = chrome_app();
    send_key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    let text = buffer_text(&mut app, 100, 24);
    insta::assert_snapshot!(text);
    assert!(text.contains("Key & Mouse bindings"));
}

#[test]
fn toast_snapshot() {
    let mut app = chrome_app();
    app.push_toast(ToastLevel::Info, "vault ready");
    app.push_toast(ToastLevel::Warning, "theme skipped");
    let text = buffer_text(&mut app, 100, 24);
    insta::assert_snapshot!(text);
    assert!(text.contains("theme skipped"));
}

#[test]
fn header_and_footer_heights_are_fixed() {
    let mut app = chrome_app();
    let text = buffer_text(&mut app, 90, 20);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 20);
    assert!(lines[0].contains("dd_vault"));
    assert!(lines[19].contains("F1:"));
}

#[test]
fn open_vault_updates_header() {
    let mut app = chrome_app();
    app.vault = Some(Vault {
        name: "notes".into(),
        root: "/tmp/notes".into(),
        meta_dir: "/tmp/notes/.dd_vault-notes".into(),
    });
    let text = buffer_text(&mut app, 100, 24);
    assert!(text.contains("vault:notes"), "{text}");
    assert!(!text.contains("no vault"), "{text}");
}

#[test]
fn space_vv_opens_picker() {
    let mut app = chrome_app();
    app.registry.vaults.push(VaultEntry {
        name: "alpha".into(),
        path: "/tmp/alpha".into(),
    });
    send_key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    assert_eq!(app.leader, Leader::Space);
    send_key(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    assert_eq!(app.leader, Leader::SpaceV);
    send_key(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    assert!(matches!(
        app.modal,
        Some(Modal::VaultPicker { selected: 0 })
    ));
    let text = buffer_text(&mut app, 100, 24);
    assert!(text.contains("Open vault"), "{text}");
    assert!(text.contains("alpha"), "{text}");
}

#[test]
fn space_vv_opens_picker_from_editor_and_preview() {
    let mut app = chrome_app();
    app.registry.vaults.push(VaultEntry {
        name: "alpha".into(),
        path: "/tmp/alpha".into(),
    });
    app.pane = Pane::Editor;
    send_key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    assert!(matches!(app.modal, Some(Modal::VaultPicker { .. })));
    app.modal = None;
    app.leader = Leader::None;
    app.pane = Pane::Preview;
    send_key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('v'), KeyModifiers::NONE);
    assert!(matches!(app.modal, Some(Modal::VaultPicker { .. })));
}

#[test]
fn picker_d_unregisters_vault_keeps_folder() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("keep-me");
    let vault = init(&root).expect("init");
    let mut app = chrome_app();
    app.paths = Some(Paths::new(dir.path().join("cfg")));
    app.set_open_vault(vault);
    app.toasts.clear();
    app.open_picker();
    send_key(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);
    assert!(matches!(
        app.modal,
        Some(Modal::Confirm {
            kind: ConfirmKind::ForgetVault { .. },
            ..
        })
    ));
    send_key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
    assert!(app.registry.vaults.is_empty());
    assert!(app.vault.is_none());
    assert!(root.is_dir(), "folder must remain");
}

#[test]
fn picker_enter_opens_real_vault() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("work");
    let vault = init(&root).expect("init");
    let mut app = chrome_app();
    app.paths = Some(Paths::new(dir.path().join("cfg")));
    app.registry.register(&vault);
    app.open_picker();
    send_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.vault.as_ref().map(|v| v.name.as_str()), Some("work"));
    assert!(app.modal.is_none());
    let saved = Registry::load(app.paths.as_ref().unwrap()).expect("registry");
    assert_eq!(saved.last_path(), Some(vault.root.clone()));
}

#[test]
fn apply_initial_vault_uses_last_path() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("daily");
    let vault = init(&root).expect("init");
    let mut app = chrome_app();
    app.paths = Some(Paths::new(dir.path().join("cfg")));
    app.registry.register(&vault);
    app.apply_initial_vault(None);
    assert_eq!(app.vault.as_ref().map(|v| v.name.as_str()), Some("daily"));
}

fn vault_app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("notes");
    let vault = init(&root).expect("init");
    std::fs::write(root.join("notes/hello.md"), "# hi\n").expect("note");
    let mut app = chrome_app();
    app.paths = Some(Paths::new(dir.path().join("cfg")));
    app.set_open_vault(vault);
    app.toasts.clear();
    (dir, app)
}

#[test]
fn tree_lists_vault_dirs_hides_gitignore() {
    let (_dir, mut app) = vault_app();
    let text = buffer_text(&mut app, 100, 24);
    assert!(text.contains("notes/"), "{text}");
    assert!(text.contains("assets/"), "{text}");
    assert!(
        !text.contains(".gitignore"),
        "tree should hide .gitignore: {text}"
    );
}

#[test]
fn tree_jk_and_enter_opens_file() {
    let (_dir, mut app) = vault_app();
    let names: Vec<String> = app.tree.visible().into_iter().map(|r| r.name).collect();
    assert!(names.iter().any(|n| n == "notes/"));
    send_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let selected = app.tree.selected_row().expect("row");
    if selected.kind == dd_vault_core::NodeKind::Dir {
        send_key(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);
        send_key(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
        send_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    }
    let visible = app.tree.visible();
    let hello = visible.iter().position(|r| r.name == "hello.md");
    if let Some(i) = hello {
        app.tree.selected = i;
        send_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.editor.rel.as_deref(), Some("notes/hello.md"));
        assert_eq!(app.pane, Pane::Editor);
    } else {
        panic!("hello.md not visible: {visible:?}");
    }
}

#[test]
fn tree_hl_collapses_and_expands() {
    let (_dir, mut app) = vault_app();
    let notes = app
        .tree
        .visible()
        .iter()
        .position(|r| r.name == "notes/")
        .expect("notes");
    app.tree.selected = notes;
    send_key(&mut app, KeyCode::Char('h'), KeyModifiers::NONE);
    let visible: Vec<_> = app.tree.visible().iter().map(|r| r.name.clone()).collect();
    assert!(
        !visible.iter().any(|n| n == "hello.md" || n == "daily/"),
        "{visible:?}"
    );
    send_key(&mut app, KeyCode::Char('l'), KeyModifiers::NONE);
    let visible: Vec<_> = app.tree.visible().iter().map(|r| r.name.clone()).collect();
    assert!(
        visible.iter().any(|n| n == "daily/" || n == "hello.md"),
        "{visible:?}"
    );
}

#[test]
fn tree_new_file_and_delete() {
    let (_dir, mut app) = vault_app();
    let notes = app
        .tree
        .visible()
        .iter()
        .position(|r| r.name == "notes/")
        .expect("notes");
    app.tree.selected = notes;
    send_key(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
    assert!(matches!(app.modal, Some(Modal::Prompt { .. })));
    for c in ['s', 'p', 'a', 'r', 'k'] {
        send_key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    send_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.modal.is_none());
    let root = app.vault.as_ref().unwrap().root.clone();
    assert!(root.join("notes/spark.md").is_file());
    assert_eq!(app.editor.rel.as_deref(), Some("notes/spark.md"));

    send_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    if app.pane == Pane::Preview {
        send_key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    }
    assert_eq!(app.pane, Pane::Tree);
    let idx = app
        .tree
        .visible()
        .iter()
        .position(|r| r.rel.ends_with("spark.md"))
        .expect("spark");
    app.tree.selected = idx;
    send_key(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
    assert!(!root.join("notes/spark.md").exists());
    assert!(app.editor.rel.is_none());
}

#[test]
fn tree_filter_narrows_rows() {
    let (_dir, mut app) = vault_app();
    send_key(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
    assert!(app.tree.filtering);
    for c in ['h', 'e', 'l', 'l', 'o'] {
        send_key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    let names: Vec<_> = app.tree.visible().iter().map(|r| r.name.clone()).collect();
    assert!(names.iter().any(|n| n == "hello.md"), "{names:?}");
    assert!(names.iter().any(|n| n == "notes/"), "{names:?}");
    assert!(!names.iter().any(|n| n == "assets/"), "{names:?}");
    send_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.tree.filtering);
    assert!(app.tree.filter.is_empty());
}

#[test]
fn editor_opens_and_edits_file() {
    let (dir, mut app) = vault_app();
    let _ = dir;
    app.load_note(std::path::PathBuf::from("notes/hello.md"));
    assert_eq!(app.editor.rel.as_deref(), Some("notes/hello.md"));
    assert!(!app.editor.dirty);
    let text = buffer_text(&mut app, 100, 24);
    assert!(text.contains("hi") || text.contains("# hi"), "{text}");
    assert!(text.contains("NORMAL"), "{text}");
    send_key(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('!'), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.editor.dirty);
    send_key(&mut app, KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(!app.editor.dirty);
    let body = std::fs::read_to_string(app.editor.path.as_ref().unwrap()).expect("read");
    assert!(body.contains('!'), "{body}");
}

#[test]
fn preview_renders_and_toggles() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("notes");
    let vault = init(&root).expect("init");
    std::fs::write(
        root.join("notes/hello.md"),
        "# UniqueHeading\n\nSee [[Inbox]] and a task:\n- [x] done\n",
    )
    .expect("note");
    let mut app = chrome_app();
    app.paths = Some(Paths::new(dir.path().join("cfg")));
    app.set_open_vault(vault);
    app.toasts.clear();
    app.load_note(std::path::PathBuf::from("notes/hello.md"));
    assert!(app.preview_visible);
    let text = buffer_text(&mut app, 120, 28);
    assert!(text.contains("preview"), "{text}");
    assert!(text.contains("UniqueHeading"), "{text}");
    assert!(text.contains("Inbox") || text.contains("done"), "{text}");

    send_key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('p'), KeyModifiers::NONE);
    assert!(!app.preview_visible);
    let _ = buffer_text(&mut app, 120, 28);
    assert_eq!(app.preview_area.height, 0);
}

#[test]
fn insert_arrows_move_in_tui() {
    let (_dir, mut app) = vault_app();
    app.load_note(std::path::PathBuf::from("notes/hello.md"));
    send_key(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);
    assert_eq!(app.editor.mode, dd_edit::Mode::Insert);
    let before = app.editor.cursor_line_col();
    send_key(&mut app, KeyCode::Right, KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Right, KeyModifiers::NONE);
    let after = app.editor.cursor_line_col();
    assert_ne!(before, after, "arrows should move in INSERT");
}

#[test]
fn file_finder_opens_note() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("notes");
    let vault = init(&root).expect("init");
    std::fs::write(root.join("notes/hello.md"), "# Hello\nUniqueFTSToken\n").expect("write");
    let mut app = chrome_app();
    app.paths = Some(Paths::new(dir.path().join("cfg")));
    app.set_open_vault(vault);
    app.toasts.clear();
    let start = std::time::Instant::now();
    while app.index_rx.is_some() && start.elapsed().as_secs() < 3 {
        app.poll_index();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    app.open_finder(FinderKind::Files);
    assert!(matches!(app.modal, Some(Modal::Finder { .. })));
    for c in ['h', 'e', 'l'] {
        send_key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    send_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(
        app.editor
            .rel
            .as_deref()
            .is_some_and(|r| r.contains("hello")),
        "{:?}",
        app.editor.rel
    );
}

#[test]
fn watch_reloads_clean_buffer() {
    let (dir, mut app) = vault_app();
    app.load_note(std::path::PathBuf::from("notes/hello.md"));
    let abs = dir.path().join("notes/notes/hello.md");
    std::fs::write(&abs, "# external\n").expect("write");
    app.handle_fs_changes(&[abs]);
    assert!(!app.editor.dirty);
    assert!(
        app.editor.text().contains("external"),
        "{}",
        app.editor.text()
    );
    assert!(app.modal.is_none());
}

#[test]
fn watch_prompts_when_dirty() {
    let (dir, mut app) = vault_app();
    app.load_note(std::path::PathBuf::from("notes/hello.md"));
    send_key(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('Z'), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.editor.dirty);
    let abs = dir.path().join("notes/notes/hello.md");
    std::fs::write(&abs, "# disk\n").expect("write");
    app.handle_fs_changes(&[abs]);
    assert!(matches!(
        app.modal,
        Some(Modal::Confirm {
            kind: ConfirmKind::ReloadDisk { .. },
            ..
        })
    ));
    send_key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
    assert!(!app.editor.dirty);
    assert!(app.editor.text().contains("disk"), "{}", app.editor.text());
}

fn git_init_at(root: &std::path::Path) {
    use std::process::Command;
    assert!(Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(root)
        .status()
        .unwrap()
        .success());
    for (k, v) in [
        ("user.email", "vault-test@example.com"),
        ("user.name", "dd_vault test"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", "/dev/null"),
    ] {
        assert!(Command::new("git")
            .args(["config", k, v])
            .current_dir(root)
            .status()
            .unwrap()
            .success());
    }
}

fn type_cmd(app: &mut App, cmd: &str) {
    send_key(app, KeyCode::Char(':'), KeyModifiers::NONE);
    for c in cmd.chars() {
        send_key(app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    send_key(app, KeyCode::Enter, KeyModifiers::NONE);
}

#[test]
fn editor_title_shows_git_badge() {
    let (_dir, mut app) = vault_app();
    assert!(
        !app.editor_title().contains("git:"),
        "{}",
        app.editor_title()
    );
    let root = app.vault.as_ref().unwrap().root.clone();
    git_init_at(&root);
    app.refresh_git();
    let title = app.editor_title();
    assert!(title.contains("git:±"), "{title}");

    let extra: Vec<String> = Vec::new();
    let ctx = dd_vault_core::GitCtx {
        root: &root,
        credentials: None,
        extra_secret_patterns: &extra,
    };
    dd_vault_core::commit(&ctx, "init").expect("commit");
    app.refresh_git();
    let title = app.editor_title();
    assert!(title.contains("git:clean"), "{title}");

    std::fs::write(root.join("notes/hello.md"), "# hi\nchanged\n").unwrap();
    app.refresh_git();
    let title = app.editor_title();
    assert!(title.contains("git:±"), "{title}");
}

#[test]
fn git_commit_modal_and_secret_scan() {
    let (_dir, mut app) = vault_app();
    let root = app.vault.as_ref().unwrap().root.clone();
    git_init_at(&root);
    app.refresh_git();
    app.pane = Pane::Editor;

    type_cmd(&mut app, "git commit");
    assert!(matches!(
        app.modal,
        Some(Modal::Prompt {
            kind: crate::app::PromptKind::GitCommit { .. },
            ..
        })
    ));
    for c in ['i', 'n', 'i', 't'] {
        send_key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    send_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.modal.is_none());
    let start = std::time::Instant::now();
    while app.git_rx.is_some() && start.elapsed().as_secs() < 3 {
        app.poll_git();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    app.poll_git();
    assert!(
        app.editor_title().contains("git:clean"),
        "{}",
        app.editor_title()
    );

    std::fs::write(root.join("notes/hello.md"), "leak ghp_ABCDEF\n").unwrap();
    app.refresh_git();
    type_cmd(&mut app, "git commit");
    for c in ['b', 'a', 'd'] {
        send_key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    send_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let start = std::time::Instant::now();
    while app.git_rx.is_some() && start.elapsed().as_secs() < 3 {
        app.poll_git();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    app.poll_git();
    assert!(
        app.toasts
            .iter()
            .any(|t| t.message.contains("secret") || t.message.contains("ghp_")),
        "{:?}",
        app.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
}

#[test]
fn busy_header_shows_dd_loader() {
    let mut app = chrome_app();
    let (_tx, rx) = std::sync::mpsc::channel();
    app.git_rx = Some(rx);
    app.busy_kind = Some("pushing");
    let text = buffer_text(&mut app, 100, 24);
    assert!(text.contains("d_d"), "family loader missing: {text}");
    assert!(text.contains("pushing") || text.contains("d_d"), "{text}");
}

#[test]
fn git_push_when_dirty_prompts_commit() {
    let (_dir, mut app) = vault_app();
    let root = app.vault.as_ref().unwrap().root.clone();
    git_init_at(&root);
    app.refresh_git();
    app.pane = Pane::Editor;
    type_cmd(&mut app, "git push");
    assert!(
        matches!(
            app.modal,
            Some(Modal::Prompt {
                kind: crate::app::PromptKind::GitCommit { then_push: true },
                ..
            })
        ),
        "{:?}",
        app.modal
    );
}

#[test]
fn git_status_command_toasts() {
    let (_dir, mut app) = vault_app();
    app.pane = Pane::Editor;
    type_cmd(&mut app, "git");
    assert!(
        app.toasts
            .iter()
            .any(|t| t.message.contains("Not a git repository")),
        "{:?}",
        app.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
}

#[test]
fn git_conflict_notice_dismisses() {
    let mut app = chrome_app();
    app.modal = Some(Modal::Notice {
        title: " Git conflicts ".into(),
        message: "notes/a.conflict-1.md".into(),
    });
    send_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.modal.is_none());
}

#[test]
fn editor_click_places_caret() {
    let (_dir, mut app) = vault_app();
    app.load_note(std::path::PathBuf::from("notes/hello.md"));
    let _ = buffer_text(&mut app, 100, 24);
    let inner = app.editor_inner;
    let gutter = app.editor.gutter_cols();
    app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: inner.x + gutter + 1,
        row: inner.y,
        modifiers: KeyModifiers::NONE,
    }))
    .expect("click");
    app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: inner.x + gutter + 1,
        row: inner.y,
        modifiers: KeyModifiers::NONE,
    }))
    .expect("up");
    assert_eq!(app.pane, Pane::Editor);
    assert_eq!(app.editor.mode, dd_edit::Mode::Normal);
    let (_line, col) = app.editor.cursor_line_col();
    assert_eq!(col, 1);
}

#[test]
fn editor_drag_selects_visual() {
    let (_dir, mut app) = vault_app();
    app.load_note(std::path::PathBuf::from("notes/hello.md"));
    let _ = buffer_text(&mut app, 100, 24);
    let inner = app.editor_inner;
    let gutter = app.editor.gutter_cols();
    let y = inner.y;
    app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: inner.x + gutter,
        row: y,
        modifiers: KeyModifiers::NONE,
    }))
    .expect("down");
    app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: inner.x + gutter + 3,
        row: y,
        modifiers: KeyModifiers::NONE,
    }))
    .expect("drag");
    app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: inner.x + gutter + 3,
        row: y,
        modifiers: KeyModifiers::NONE,
    }))
    .expect("up");
    assert_eq!(app.editor.mode, dd_edit::Mode::Visual);
    assert!(app.editor.visual_highlight().is_some());
}

#[test]
fn palette_opens_on_space_space() {
    let (_dir, mut app) = vault_app();
    send_key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    assert!(matches!(app.modal, Some(Modal::Palette { .. })));
    for c in ['d', 'a', 'i', 'l', 'y'] {
        send_key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    send_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(
        app.editor
            .rel
            .as_deref()
            .is_some_and(|r| r.contains("daily") && r.ends_with(".md")),
        "{:?}",
        app.editor.rel
    );
}

#[test]
fn space_nd_opens_daily() {
    let (_dir, mut app) = vault_app();
    send_key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);
    let rel = app.editor.rel.clone().expect("daily");
    assert!(rel.contains("daily"), "{rel}");
    assert!(rel.ends_with(".md"), "{rel}");
    let abs = app.vault.as_ref().unwrap().root.join(&rel);
    assert!(abs.is_file(), "{}", abs.display());
}

#[test]
fn ai_card_collapsed_in_layout() {
    let mut app = chrome_app();
    let text = buffer_text(&mut app, 100, 24);
    assert!(text.contains("AI"), "{text}");
    assert!(text.contains("off"), "{text}");
}

#[test]
fn space_ai_expands_card() {
    let (_dir, mut app) = vault_app();
    send_key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);
    assert_eq!(app.ai.size, crate::ai::AiSize::Chat);
    let text = buffer_text(&mut app, 100, 24);
    assert!(text.contains("AI"), "{text}");
}

#[test]
fn ai_off_refuses_request() {
    let (_dir, mut app) = vault_app();
    app.request_ai("hello".into(), None);
    assert!(
        app.toasts.iter().any(|t| t.message.contains("AI is off")),
        "{:?}",
        app.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
    assert!(app.modal.is_none());
}

#[test]
fn ai_scripted_consent_inserts() {
    let (_dir, mut app) = vault_app();
    app.load_note(std::path::PathBuf::from("notes/hello.md"));
    app.ai.scripted = Some(vec!["UNIQUEAIOUT".into()]);
    app.request_ai("say hi".into(), None);
    assert!(matches!(
        app.modal,
        Some(Modal::Confirm {
            kind: ConfirmKind::AiSend,
            ..
        })
    ));
    send_key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
    let start = std::time::Instant::now();
    while app.ai.rx.is_some() && start.elapsed().as_secs() < 2 {
        app.poll_ai();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    app.poll_ai();
    assert!(
        app.editor.text().contains("UNIQUEAIOUT"),
        "{}",
        app.editor.text()
    );
}

#[test]
fn space_z_hides_tree_keeps_chrome() {
    let (_dir, mut app) = vault_app();
    let _ = buffer_text(&mut app, 100, 24);
    assert!(app.tree_area.width > 0);
    send_key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('z'), KeyModifiers::NONE);
    assert!(app.focus);
    let text = buffer_text(&mut app, 100, 24);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 24);
    assert!(lines[0].contains("dd_vault"));
    assert!(lines[23].contains("F1:"));
    assert_eq!(app.tree_area.width, 0);
    assert!(app.preview_area.height > 0, "preview stays in focus mode");
    assert!(text.contains("AI"), "AI card stays in focus mode: {text}");
}

#[test]
fn space_h_zen_hides_tree_preview_ai_keeps_chrome() {
    let (_dir, mut app) = vault_app();
    send_key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('h'), KeyModifiers::NONE);
    assert!(app.zen);
    let text = buffer_text(&mut app, 100, 24);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 24);
    assert!(lines[0].contains("dd_vault"));
    assert!(lines[23].contains("F1:"));
    assert_eq!(app.tree_area.width, 0);
    assert_eq!(app.preview_area.height, 0);
    assert!(!text.contains("off  <Space>ai"), "{text}");
}

#[test]
fn drag_resizes_tree_split() {
    let (_dir, mut app) = vault_app();
    let _ = buffer_text(&mut app, 100, 24);
    let edge = app.tree_area.x + app.tree_area.width.saturating_sub(1);
    let y = app.tree_area.y + 2;
    let before = app.tree_area.width;
    app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: edge,
        row: y,
        modifiers: KeyModifiers::NONE,
    }))
    .expect("down");
    app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 40,
        row: y,
        modifiers: KeyModifiers::NONE,
    }))
    .expect("drag");
    app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 40,
        row: y,
        modifiers: KeyModifiers::NONE,
    }))
    .expect("up");
    let _ = buffer_text(&mut app, 100, 24);
    assert_ne!(app.tree_area.width, before, "tree split should move");
    assert_eq!(app.tree_split, 40);
}

#[test]
fn focus_layout_snapshot() {
    let (_dir, mut app) = vault_app();
    app.header_copy = "Files on disk. Opinions in git.".to_string();
    send_key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('z'), KeyModifiers::NONE);
    let text = buffer_text(&mut app, 100, 24);
    insta::assert_snapshot!(text);
    assert_eq!(app.tree_area.width, 0);
}

#[test]
fn zen_layout_snapshot() {
    let (_dir, mut app) = vault_app();
    app.header_copy = "Files on disk. Opinions in git.".to_string();
    send_key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    send_key(&mut app, KeyCode::Char('h'), KeyModifiers::NONE);
    let text = buffer_text(&mut app, 100, 24);
    insta::assert_snapshot!(text);
    assert_eq!(app.tree_area.width, 0);
    assert_eq!(app.preview_area.height, 0);
}

#[test]
fn paste_image_writes_asset_and_markdown() {
    let (_dir, mut app) = vault_app();
    app.load_note(std::path::PathBuf::from("notes/hello.md"));
    let mut rgba = vec![0u8; 4];
    rgba[0] = 200;
    rgba[3] = 255;
    app.paste_image_rgba(1, 1, &rgba);
    let root = app.vault.as_ref().unwrap().root.clone();
    let assets: Vec<_> = std::fs::read_dir(root.join("assets"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("hello-") && n.ends_with(".png"))
        .collect();
    assert_eq!(assets.len(), 1, "{assets:?}");
    assert!(
        app.editor
            .text()
            .contains(&format!("![](assets/{})", assets[0])),
        "{}",
        app.editor.text()
    );
    assert!(app.editor.dirty);
    let text = buffer_text(&mut app, 100, 28);
    assert!(
        text.contains('▀') || text.contains("[image:"),
        "preview should show the pasted image: {text}"
    );
}

#[test]
fn paste_image_without_note_toasts() {
    let (_dir, mut app) = vault_app();
    app.paste_image_rgba(1, 1, &[255, 0, 0, 255]);
    assert!(
        app.toasts.iter().any(|t| t.message.contains("Open a note")),
        "{:?}",
        app.toasts.iter().map(|t| &t.message).collect::<Vec<_>>()
    );
    let root = app.vault.as_ref().unwrap().root.clone();
    let extra: Vec<_> = std::fs::read_dir(root.join("assets"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
        .collect();
    assert!(extra.is_empty(), "{extra:?}");
}

fn editor_pane(full: &str, area: ratatui::layout::Rect) -> String {
    let lines: Vec<&str> = full.lines().collect();
    let mut out = String::new();
    for y in area.y..area.y.saturating_add(area.height) {
        let line = lines.get(y as usize).copied().unwrap_or("");
        let start = area.x as usize;
        let row: String = line.chars().skip(start).take(area.width as usize).collect();
        out.push_str(&row);
        out.push('\n');
    }
    out
}

#[test]
fn notes_panel_wraps_long_lines() {
    let (_dir, mut app) = vault_app();
    let root = app.vault.as_ref().unwrap().root.clone();
    let src = format!("{}ZZWRAP", "0123456789".repeat(40));
    std::fs::write(root.join("notes/hello.md"), &src).unwrap();
    app.load_note(std::path::PathBuf::from("notes/hello.md"));

    let text = buffer_text(&mut app, 100, 16);
    let pane = editor_pane(&text, app.editor_inner);
    assert!(
        !pane.contains("ZZWRAP"),
        "end of a long line should be below the fold until the caret moves:\n{pane}"
    );
    let width = (app.editor_inner.width - app.editor.gutter_cols()) as usize;
    let lines: Vec<&str> = pane.lines().collect();
    let gutter = app.editor.gutter_cols() as usize;
    let wrapped = lines[1].chars().nth(gutter).unwrap();
    assert_eq!(
        wrapped,
        src.chars().nth(width).unwrap(),
        "second row should continue the line:\n{pane}"
    );
    assert!(
        lines[1].chars().take(gutter).all(|c| c == ' '),
        "continuation row should not repeat the line number:\n{pane}"
    );

    let inner = app.editor_inner;
    app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: inner.x + app.editor.gutter_cols(),
        row: inner.y + 1,
        modifiers: KeyModifiers::NONE,
    }))
    .expect("click");
    app.handle_event(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: inner.x + app.editor.gutter_cols(),
        row: inner.y + 1,
        modifiers: KeyModifiers::NONE,
    }))
    .expect("up");
    assert_eq!(app.editor.cursor_line_col(), (0, width));

    send_key(&mut app, KeyCode::Char('$'), KeyModifiers::NONE);
    let text = buffer_text(&mut app, 100, 16);
    let pane = editor_pane(&text, app.editor_inner);
    assert!(
        pane.contains("ZZWRAP"),
        "caret at end of a wrapped line should scroll that row into view:\n{pane}"
    );
    assert!(
        app.editor.scroll_off > 0,
        "scroll_off {}",
        app.editor.scroll_off
    );
}

#[test]
fn notes_panel_wrap_can_be_disabled() {
    let dir = tempfile::tempdir().expect("tmp");
    let root = dir.path().join("plain");
    let vault = init(&root).expect("init");
    std::fs::write(vault.meta_dir.join("config.toml"), "wrap = false\n").unwrap();
    let mut app = chrome_app();
    app.paths = Some(Paths::new(dir.path().join("cfg")));
    app.set_open_vault(vault);
    app.toasts.clear();
    assert!(!app.wrap_notes);

    let root = app.vault.as_ref().unwrap().root.clone();
    let src = format!("{}ZZWRAP", "0123456789".repeat(40));
    std::fs::write(root.join("notes/hello.md"), &src).unwrap();
    app.load_note(std::path::PathBuf::from("notes/hello.md"));
    send_key(&mut app, KeyCode::Char('$'), KeyModifiers::NONE);
    let text = buffer_text(&mut app, 100, 16);
    let pane = editor_pane(&text, app.editor_inner);
    assert!(
        !pane.contains("ZZWRAP"),
        "wrap = false should clip the notes pane:\n{pane}"
    );
    assert_eq!(app.editor.scroll_off, 0);
}
