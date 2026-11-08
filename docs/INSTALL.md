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

```bash
dd_vault init ~/Notes
dd_vault open ~/Notes
dd_vault
dd_vault --reindex ~/Notes
```

## Desktop window

```bash
INSTALL_GUI=1 ./install.sh
```

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

The tutorial screenshots are the terminal app: https://ldnddev.github.io/dd_vault/
