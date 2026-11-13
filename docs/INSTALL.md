# Install

Two programs read and write the same vault:

- `dd_vault` is the terminal app. It needs Rust 1.82 or newer.
- `dd_vault-gui` is the desktop window. It needs Rust 1.92 or newer, Vulkan (`libvulkan1` and a GPU driver), and a Wayland or X11 session.

The default install builds only the terminal app.

## Terminal

```bash
curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | bash
```

From a clone:

```bash
./install.sh
```

The script runs `cargo build --release --locked` and installs `dd_vault` to `~/.local/bin`. `PREFIX` sets the install root. `BIN_DIR` sets the binary directory.

On Linux the same run writes `dd_vault.desktop` for the terminal app. With `INSTALL_GUI=1`, or when `dd_vault-gui` is already in the binary directory, it also writes `dd_vault-gui.desktop`. User installs publish those files in `${XDG_DATA_HOME:-~/.local/share}/applications`. A prefix outside `$HOME` (for example `/usr/local`) uses `$PREFIX/share/applications`. The terminal launcher sets `Terminal=true`. The window launcher sets `Terminal=false` and `StartupWMClass=dd_vault-gui`. macOS skips the desktop files.

```bash
dd_vault init ~/Notes
dd_vault open ~/Notes
dd_vault
dd_vault --reindex ~/Notes
```

## Desktop window

```bash
curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | INSTALL_GUI=1 bash
```

From a clone:

```bash
INSTALL_GUI=1 ./install.sh
```

`INSTALL_GUI=1` has to be set on the `bash` that runs the script.

That also runs:

```bash
cargo build --release --locked --manifest-path crates/dd_gpui/Cargo.toml
```

and installs `dd_vault-gui` next to `dd_vault`. The desktop crate is its own workspace. Its binary is `crates/dd_gpui/target/release/dd_vault-gui` unless `CARGO_TARGET_DIR` is set.

```bash
dd_vault-gui init ~/Notes
dd_vault-gui open ~/Notes
dd_vault-gui
```

`init` prints the vault path and does not open a window. `open` and a bare launch open the window. `--reindex` rebuilds the index and exits.

On Debian and Ubuntu the window needs `libvulkan1` and a driver such as `mesa-vulkan-drivers`. The process uses Wayland when `WAYLAND_DISPLAY` is set, and X11 otherwise.

## Uninstall

```bash
./install.sh --uninstall
```

From the one-liner:

```bash
curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | bash -s -- --uninstall
```

That removes `$BIN_DIR/dd_vault`, `$BIN_DIR/dd_vault-gui`, and the two desktop files named above. Missing files are skipped. Use the same `PREFIX`, `BIN_DIR`, and `XDG_DATA_HOME` as the install. Vaults and `~/.config/ldnddev` stay.

The tutorial screenshots are the terminal app: https://ldnddev.github.io/dd_vault/
