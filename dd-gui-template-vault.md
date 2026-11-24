# dd_* desktop window

How to add a GPUI window next to a `dd_*` terminal app. The first window built this way is `dd_vault-gui`. Copy this shape into the next app. The window crate stays inside that app's repository. It does not depend on another app's GUI crate, and it does not depend on a checkout of `ldnddev_theme` outside the repo.

The terminal program remains the source of behavior. The window calls the same core functions, reads the same config, and writes the same session. A person can quit one and open the other on the same files.

## Layout

```
<app>/
  Cargo.toml                 terminal workspace, edition 2021, rust-version 1.82
  crates/
    ldnddev_theme/           vendored copy, standalone version 0.1.0
    <app>_core/              files, config, session, domain rules
    <app>_tui/               ratatui shell
    <app>_gui/               its own Cargo workspace
      Cargo.toml             edition 2024, rust-version 1.92
      Cargo.lock
      deny.toml
      src/
        main.rs              CLI, then the window
        shell.rs             one GPUI entity: layout, input, paint
        chrome.rs            header, footer, toasts, pane sizes
        colors.rs            palette packed into u32 colors
        keys.rs              GPUI keystroke to the shared key enum
        scroll.rs            ScrollPort
        settings.rs          rows for the F1 sheet
        fonts.rs             [gui] faces in the shared config
```

Domain modules sit beside those. A preview, a git badge, or a command palette belongs in the window only when the terminal already has that behavior.

The root workspace members are the terminal crates. `<app>_gui` is not a member. `cargo test --workspace` from the repo root must keep building on Rust 1.82. The window is built on its own manifest:

```
CARGO_TARGET_DIR=target/<app>_gui cargo test --manifest-path crates/<app>_gui/Cargo.toml
CARGO_TARGET_DIR=target/<app>_gui cargo clippy --manifest-path crates/<app>_gui/Cargo.toml --all-targets -- -D warnings
```

Give the window its own target directory. The two workspaces share a cargo lock on the package cache, and a shared `target/` makes those builds wait on each other.

## What is shared

| Piece | Where it lives | Rule |
| --- | --- | --- |
| Files, config, session, domain rules | `<app>_core` | Both programs call these functions. The window does not reimplement them. |
| Editor buffer and caret | shared editor crate, ratatui-free | One insert, one undo step. The window maps keystrokes onto that crate's `Key`. |
| Rendered document | shared render crate | The terminal draws cells. The window lays the same blocks out in pixels. |
| Theme load, Omarchy list, hex editor | vendored `ldnddev_theme` | Ratatui-free. Copy vendor updates into the repo. Do not path-depend on another repo. |
| Toasts, footer fit, help text | `chrome.rs` in each UI | Same words and the same five-second lifetime. The window copy uses no ratatui types. |

The window crate does not depend on the terminal crate. Session state and anything both UIs resume live in the core.

## Pins

```toml
[package]
name = "<app>_gui"
edition = "2024"
rust-version = "1.92"
license = "MIT"
publish = false

[dependencies]
gpui-unofficial = "=1.27.0"
gpui_platform = { package = "gpui-platform-gpui-unofficial", version = "=1.27.0", features = ["wayland", "x11"] }
ldnddev_theme = { path = "../ldnddev_theme" }

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 16
panic = "abort"
strip = true
```

Pin GPUI with `=`. A floating upgrade changes text layout and input. Do not add a Zed git dependency, xgpui, fc-gpui, or guic.

The terminal release profile stays small (`opt-level = "z"`, fat LTO, one codegen unit). The window profile stays at `opt-level = 3` and thin LTO. A window binary that is stripped and aborted on panic matches the terminal's failure mode.

`deny.toml` lives in the window crate and is separate from the terminal workspace. GPL stays refused. Apache-2.0 is allowed because GPUI is Apache-2.0. Unknown git sources are denied. Advisories that ship inside the GPUI pin are ignored with a reason that names the pin, the same way the terminal ignore list names its own stack. Run `cargo deny check` from `crates/<app>_gui`.

The window needs Vulkan, Wayland or X11, fontconfig, and freetype. Say that in the installer. `init` and any no-window subcommand still run without a display.

## Process

`main` parses the same CLI as the terminal. Subcommands that only touch disk print and return. They do not open a window. The no-argument path, and `open <path>`, load the theme, the registry, and the shared paths, then call `shell::run`.

```rust
pub struct Boot {
    pub theme: GuiTheme,
    pub palette: Palette,
    pub warning: Option<String>,
    pub paths: Paths,
    pub registry: Registry,
    pub initial: Option<SessionRoot>,
}

pub fn run(boot: Boot) {
    application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("<app>".into()),
                    ..Default::default()
                }),
                focus: true,
                show: true,
                app_id: Some("dev.ldnddev.<app>".into()),
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
```

`app_id` is `dev.ldnddev.<app>`. On Wayland that is the xdg app id. On X11 it is the window class. The Linux desktop file uses that same string as `StartupWMClass`. The binary name stays `<app>-gui`.

One `Shell` entity owns the screen. `Render` keeps a `FocusHandle` on the root and focuses it whenever the window is not focused. Keys arrive on that root through `on_key_down`.

## Theme

Lookup order matches the terminal:

1. `./<app>_theme.yml`
2. `$XDG_CONFIG_HOME/ldnddev/<app>_theme.yml`, or `~/.config/ldnddev/<app>_theme.yml`
3. The built-in YAML compiled into the binary

Load is strict for a file the user wrote and lenient for the built-in text. When the YAML has an `omarchy` key, call `apply_omarchy`. A missing Omarchy theme keeps the built-in palette and surfaces the warning on stderr before the window opens.

Pack each `Rgb` into `0xRRGGBB` once, in `from_palette`. Paint with `rgb(theme.text_primary)`. After load, the window does not hard-code colors. The numeric fallbacks inside `from_palette` exist only for a token the palette omitted.

Keep the `Palette` beside the packed theme. The theme picker and the hex editor mutate the palette, then `from_palette` builds the next `GuiTheme`, then the window repaints. `j` and `k` preview. `y` writes the theme file. Esc restores the palette captured when the sheet opened.

Theme YAML stays colors only. Faces do not go in that file.

## Chrome

The window is a column: header, body, footer.

| Band | Height | Role |
| --- | --- | --- |
| Header | 28px | App name, tagline, busy mark. A fixed height in zen and in focus. |
| Body | the rest | The working panes. |
| Footer | 28px | `F1: Settings` and quit. Context hints drop from the right when the window is narrow. |

Status belongs in the pane title: file name, mode, dirty mark, git badge. The header is not a toolbar. The footer is not a status line.

Toasts stack above the footer, up to four, and expire after five seconds. Border, glyph, and message use the semantic color. Shift that color toward black or white until the text clears 4.5:1 on the modal background. The terminal uses the same rule.

Pane split sizes are stored in character columns and rows, then converted to pixels with the current face's advance and line height. Dragging a split writes those character counts back. Hovering the boundary shows the column or row resize cursor, and that cursor stays for the whole drag.

Body panes use `flex_basis 0` and `min_h(px(0.))` so a long document cannot stretch the window. The document itself is absolutely positioned inside the pane.

## Input

Handle a key in this order:

1. Ctrl+V. Paste text, or an image when the focused surface accepts one.
2. An open confirm or an open overlay, even while settings is up. Settings must not swallow a quit prompt.
3. The settings sheet, when it is open.
4. F1, when nothing modal is up. F1 opens the sheet.
5. Ctrl+Q and Ctrl+S.
6. The focused pane, the leader, or the command palette.

Closing the sheet on F1 happens inside the sheet handler. The opener only opens. One path that both opens and closes rebuilds the theme picker and drops the palette the user would revert to.

Re-entering the sheet while it is already open returns immediately. Building a new picker would replace the revert snapshot.

Ctrl+Q quits. A bare `q` does not. A dirty document confirms before quit. Esc cancels the top modal, then clears a search, then closes the sheet and reverts an unsaved theme.

Map GPUI keystrokes in `keys.rs`. Named keys (`up`, `escape`, `backspace`) become the editor crate's `Key`. Control plus a letter becomes `Ctrl(char)`. The rest of the shell matches on that enum, so the window and the terminal share one key meaning.

A one-line field (rename, prompt, search, hex, filter) keeps a caret in the real text. Left, Right, Home, End, and a click move it. Backspace deletes before the caret. Delete deletes the character after it. A masked field paints bullets and moves the caret in the underlying text. Up and Down stay list motion when the field is attached to a list. A lone field uses Up for the start and Down for the end.

Paste inserts the whole clipboard in one editor operation. It does not walk the text as typed characters. Bracketed paste and Ctrl+V call that same insert. Sanitize the string in the editor crate before it lands. An image paste writes a file through the core's confined write and inserts one link, when the focused surface accepts an image.

Clicks hit the deepest element first. A row that handles the click calls `stop_propagation`. The shell root still records caret hit boxes for fields that are painted inside that root, so a click in a search field or a hex field lands on a character. A click on padding does nothing.

The wheel on a pane scrolls that pane. It does not also move the keyboard selection. Parent `on_scroll` and a child scroller both observe the wheel. If the child owns the scroll, the parent returns without a second nudge.

## Scroll

GPUI's scroll handle is `0` at the top and negative after the user scrolls down. `ScrollPort` stores the positive distance and the handle:

- `push_px(y)` writes `set_offset(point(px(0.), px(-y)))` and remembers `y`.
- `pull_px` reads the handle on the next frame. It ignores a change that matches the last push, so a wheel and a key do not fight.
- `view_px` is empty until the scroller has been painted. A reveal that runs before the first paint uses a fallback viewport, about 400px.

```rust
fn y_scroll(id: &'static str, handle: &ScrollHandle, document: Div) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .size_full()
        .overflow_y_scroll()
        .track_scroll(handle)
        .child(document)
}
```

The document's height is the sum of its rows, set explicitly with `h(px(content_h))`. The thumb uses that same height, the viewport, and the positive offset. Hide the thumb when the content fits. The track has an explicit height. Dragging the thumb or clicking the track calls `push_px`.

Reveal a selected row only when the selection moved by keyboard or by click. Revealing on every frame fights the wheel.

## Settings

F1 opens one settings sheet. F2 and the other function-key panels are not separate windows. Help is a page of the sheet.

The sheet is docked on the right of the body, under the header and above the footer. Its width is `relative(0.7)`. A left column about 200px wide lists the sections. The right column shows only the selected section.

The left column is `body_background` with a right border. Search sits at the top of that column. Each section is one row, `> Label`, 32px tall. The selected row uses `selected_background` and `text_active_focus`. A non-empty search clears that highlight and replaces the right column with Results.

The pages are Themes, Editor, Window, AI, Keymap, and Help. Tab and Shift-Tab move between them and stop at the ends. `[` and `]` do the same. A click does the same. Tab still moves the section while search or a hex field is focused. `t` toggles the theme save target. Leaving Themes collapses an open font list. Coming back to a theme list lands on the highlighted theme. Coming back to the color editor keeps the color row.

The right column pins the section name and a one-line hint, then scrolls that section's rows. `/` filters every page into Results. The theme list is the Themes page. Moving the highlight previews the palette. Esc reverts. `y` saves. Editor and preview faces expand on that same page. Enter keeps them. `h` or Left collapses the list back onto the face row. With no Omarchy themes installed, Themes opens the hex editor directly. Esc returns to the list when a list exists. The command palette's Settings item opens Themes. Its Help item opens the Help page. Opening Help does not call itself.

Selectable hits are option rows only. Headings and notes are not hits. A page with no hits, such as Help, scrolls with j/k.

An option row is one 52px block. The title and the detail are inside that block. The whole block is `cursor_pointer`, hover uses `body_background`, and the selection uses `selected_background` with `text_active_focus`. The detail is not a second, non-clickable row.

The footer while the sheet is closed is `F1: Settings` plus quit. While it is open: `F1: Settings`, `Esc: Close`, `Ctrl+Q: Quit`. Below 80 columns those strings shorten, and they still name F1, Esc, and quit.

## Fonts

Faces live in the shared ldnddev config, `$XDG_CONFIG_HOME/ldnddev/config.toml` or `~/.config/ldnddev/config.toml`:

```toml
[gui]
mono = "DejaVu Sans Mono"
sans = "DejaVu Sans"
```

The monospace face is the editor, the lists, and the chrome. The proportional face is for reading view. Changing a face paints immediately and recomputes the caret column width. Saving merges `[gui]` and leaves every other table, including `[ai]`, in place. Defaults are DejaVu Sans Mono and DejaVu Sans. The family list comes from fontconfig.

Every windowed `dd_*` app on the machine reads this same `[gui]` table. Theme files stay per app.

## Background work

Git, indexing, file watches, and network calls run on `std` threads and return through `mpsc`. The window polls. The GPUI thread does not block on that work.

A busy word goes in the header, using the same four-frame mark as the terminal (`▃ ▅ █ ▅`, 150ms, label `{bar} d_d {bar}`). A finished job paints a toast or a confirm. A quiet index refresh does not.

Writes that the app itself just performed are marked so the watcher does not prompt to reload them. A change from outside prompts when the document is dirty and reloads when it is clean.

## Install

The installer asks which program to install: the terminal, the window, or both. Uninstall asks the same question.

| Choice | Installs or removes |
| --- | --- |
| `1`, `tui`, `terminal` | `<app>` and its `.desktop` |
| `2`, `gui`, `window` | `<app>-gui` and its `.desktop` |
| `3`, `both` | both binaries and both desktop files |

Flags `--tui`, `--gui`, and `--both` skip the prompt. More than one flag is an error. `INSTALL_GUI=1` with no target flag means both. `--gui` is the window only.

The prompt reads `/dev/tty`. A real terminal is `{ : <>/dev/tty; } 2>/dev/null`. A `-r` or `-w` test is true even when the process has no controlling terminal. Capture the choice with `target=$(choose_target ...)`, and send every notice to stderr. A `say` on stdout becomes part of the captured word.

When there is no terminal to answer, install selects the terminal app and says so on stderr. Uninstall removes both and says so. Empty input and EOF use the default: terminal for install, both for uninstall.

Accept `1` / `tui` / `terminal`, `2` / `gui` / `window`, and `3` / `both`. Lowercase with `tr`. macOS ships bash 3.2, which has no `${var,,}`.

Build the window with `--manifest-path crates/<app>_gui/Cargo.toml` and Rust 1.92 or newer. The terminal build stays on Rust 1.82. If `CARGO_TARGET_DIR` is set, the binary is `$CARGO_TARGET_DIR/release/<app>-gui`. Otherwise it is `crates/<app>_gui/target/release/<app>-gui`.

On Linux, write one desktop file per installed binary. User installs use `${XDG_DATA_HOME:-~/.local/share}/applications`. A prefix outside `$HOME` uses `$PREFIX/share/applications`. The terminal launcher sets `Terminal=true`. The window launcher sets `Terminal=false`, `StartupNotify=true`, and `StartupWMClass=dev.ldnddev.<app>`. Quote `Exec` so a path with spaces survives. There is no icon. macOS skips the desktop files.

Uninstall removes the selected binaries and their desktop files. It leaves the user's data and `~/.config/ldnddev` in place. Pass the same `PREFIX`, `BIN_DIR`, and `XDG_DATA_HOME` used to install.

Curl runs the script from a pipe. Guard the entry point so `BASH_SOURCE[0]` is safe under `set -u`:

```bash
if [[ -z "${BASH_SOURCE[0]:-}" || "${BASH_SOURCE[0]:-}" == "$0" ]]; then
  main "$@"
fi
```

`bash -s -- --tui` passes the flag through. `INSTALL_GUI` has to be set on the bash that runs the script.

## Checks

Test the window crate without opening a display. Key maps, footer strings, scroll math, theme packing, settings rows, and paste sanitizing are plain functions.

- The sheet's option rows are 52px and include the detail.
- The left list selects one page. The right column's `content_h` is only that page.
- F1 opens settings. A second F1 closes it and restores the palette from open.
- Wheel movement changes the scroll offset and leaves the selected row where it was.
- A click on a hex field places the caret after the `Hex: ` prefix.
- Font save merges `[gui]` and keeps `[ai]`.
- Paste of a multi-line string is one insert.

`cargo clippy --all-targets -- -D warnings` is clean on the window crate and on the terminal workspace. `cargo deny check` is clean in the window crate.

Open the window on the dev machine before calling a layout done. Exercise the changed path: keyboard, click, wheel, and the matching command-palette item. A unit test does not show whether a border clipped a row or a click landed on padding.

## Copy checklist

- Terminal workspace stays edition 2021 and Rust 1.82. The window is its own workspace, edition 2024, Rust 1.92, binary `<app>-gui`.
- The window depends on the core, the editor, the renderer, and the vendored theme. It does not depend on the terminal crate or on another repository.
- GPUI is pinned to `gpui-unofficial` 1.27.0 with the Wayland and X11 platform features.
- `init` and the other disk-only commands print and exit. The window opens only for interactive launch.
- The binary is `<app>-gui`. `app_id` and `StartupWMClass` are `dev.ldnddev.<app>`.
- Theme lookup is local, then global, then built-in, then `apply_omarchy`. Colors come from the packed palette.
- Header and footer are 28px and stay that tall in zen and focus. Status sits in the pane title.
- F1 is one settings sheet, about 70% of the body width. A left list picks one page. Option rows are 52px and fully clickable. Help is a page.
- Esc reverts an unsaved theme and unsaved faces. `y` writes the theme. Faces save into the shared `[gui]` table.
- Scroll uses `ScrollPort`: positive pixels in the model, a negative handle offset, reveal only when the selection moves.
- Paste is one insert. Background work is a thread and a channel.
- The installer asks terminal, window, or both, for install and for uninstall. The window `.desktop` sets `Terminal=false`.
- Tests and clippy pass on both workspaces. `cargo deny check` passes in the window crate. The changed flow has been used in a real window.
