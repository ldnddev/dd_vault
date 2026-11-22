# Install

Two programs read and write the same vault:

- `dd_vault` is the terminal app. It needs Rust 1.82 or newer.
- `dd_vault-gui` is the desktop window. It needs Rust 1.92 or newer, Vulkan (`libvulkan1` and a GPU driver), and a Wayland or X11 session.

With no arguments the installer asks which to build. Enter selects the terminal app. With no terminal to answer, it installs that app.

```bash
curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | bash
```

From a clone:

```bash
./install.sh
```

The question is:

```text
Install which program?
  1) terminal (dd_vault)
  2) desktop window (dd_vault-gui)
  3) both
Choice [1]:
```

The script reads that answer from the terminal, so a curl pipe still asks. Skip the question with one target:

```bash
./install.sh --tui
./install.sh --gui
./install.sh --both
curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | bash -s -- --both
```

`--tui` installs `dd_vault`. `--gui` installs `dd_vault-gui`. `--both` installs both. `INSTALL_GUI=1` on the bash that runs the script is the same as `--both` and skips the question. Set it on that bash: `curl … | INSTALL_GUI=1 bash`. An `INSTALL_GUI=1` prefix on curl itself stays with curl.

## Build

The terminal build is:

```bash
cargo build --release --locked
```

The window build is:

```bash
cargo build --release --locked --manifest-path crates/dd_gpui/Cargo.toml
```

`--both` runs the terminal build, then the window build. Binaries go to `~/.local/bin`. `PREFIX` sets the install root. `BIN_DIR` sets the binary directory. The desktop crate is its own workspace. Its binary is `crates/dd_gpui/target/release/dd_vault-gui` unless `CARGO_TARGET_DIR` is set.

```bash
dd_vault init ~/Notes
dd_vault open ~/Notes
dd_vault
dd_vault --reindex ~/Notes
dd_vault-gui init ~/Notes
dd_vault-gui open ~/Notes
dd_vault-gui
```

`init` on the window prints the vault path and does not open a window. `open` and a bare launch open the window. `--reindex` rebuilds the index and exits.

On Debian and Ubuntu the window needs `libvulkan1` and a driver such as `mesa-vulkan-drivers`. The process uses Wayland when `WAYLAND_DISPLAY` is set, and X11 otherwise.

## Launchers

On Linux the installer writes a desktop file for each of these binaries that is already in the binary directory. A later run that installs one program still refreshes the other launcher when that other binary is already there.

User installs publish those files in `${XDG_DATA_HOME:-~/.local/share}/applications`. A prefix outside `$HOME` (for example `/usr/local`) uses `$PREFIX/share/applications`. The terminal launcher sets `Terminal=true`. The window launcher sets `Terminal=false` and `StartupWMClass=dd_vault-gui`. macOS skips the desktop files.

## Uninstall

```bash
./install.sh --uninstall
```

That asks the same three choices. Enter removes both. With no terminal to answer, it removes both.

```text
Remove which program?
  1) terminal (dd_vault)
  2) desktop window (dd_vault-gui)
  3) both
Choice [3]:
```

Skip the question:

```bash
./install.sh --uninstall --tui
./install.sh --uninstall --gui
./install.sh --uninstall --both
curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | bash -s -- --uninstall --gui
```

Each choice removes that program's binary and its desktop file. Missing files are skipped. Use the same `PREFIX`, `BIN_DIR`, and `XDG_DATA_HOME` as the install. Vaults and `~/.config/ldnddev` stay.

The tutorial screenshots are the terminal app: https://ldnddev.github.io/dd_vault/
