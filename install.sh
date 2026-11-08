#!/usr/bin/env bash
# Install dd_vault to PREFIX/bin (default: ~/.local/bin).
#
# From a clone:
#   ./install.sh
#
# One-liner (curl):
#   curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | bash
#
# Env:
#   PREFIX      install prefix (default: ~/.local)
#   BIN_DIR     override binary dir (default: $PREFIX/bin)
#   REPO        git URL (default: https://github.com/ldnddev/dd_vault)
#   INSTALL_GUI set to 1 to also build and install dd_vault-gui (Rust 1.92+)

set -euo pipefail

REPO="${REPO:-https://github.com/ldnddev/dd_vault}"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"
NEED_RUST="1.82"
NEED_RUST_GUI="1.92"

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing '$1' on PATH"
}

in_tree() {
  local dir="$1"
  [[ -f "$dir/Cargo.toml" && -d "$dir/crates" && -f "$dir/src/main.rs" ]]
}

build_and_install() {
  local src="$1"
  say "Building release binary (opt-level=z, LTO)…"
  cargo build --release --locked --manifest-path "$src/Cargo.toml"
  mkdir -p "$BIN_DIR"
  local bin="$src/target/release/dd_vault"
  [[ -x "$bin" ]] || die "build produced no binary at $bin"
  install -m755 "$bin" "$BIN_DIR/dd_vault"
  say "Installed $BIN_DIR/dd_vault"

  if [[ "${INSTALL_GUI:-}" == "1" ]]; then
    say "Building dd_vault-gui (Rust ${NEED_RUST_GUI}+, release)…"
    cargo build --release --locked --manifest-path "$src/crates/dd_gpui/Cargo.toml"
    local gui="$src/crates/dd_gpui/target/release/dd_vault-gui"
    if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
      gui="${CARGO_TARGET_DIR}/release/dd_vault-gui"
    fi
    [[ -x "$gui" ]] || die "build produced no binary at $gui"
    install -m755 "$gui" "$BIN_DIR/dd_vault-gui"
    say "Installed $BIN_DIR/dd_vault-gui"
    say "dd_vault-gui needs Vulkan (libvulkan1 and a driver) and Wayland or X11."
  fi
}

need git
need cargo
need rustc
need install

src=""
if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
  here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
  if in_tree "$here"; then
    src="$here"
  fi
fi

if [[ -z "$src" ]]; then
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  say "Cloning $REPO …"
  git clone --depth 1 "$REPO" "$tmp/dd_vault"
  src="$tmp/dd_vault"
fi

build_and_install "$src"

if ! command -v dd_vault >/dev/null 2>&1; then
  say "Add to PATH: export PATH=\"$BIN_DIR:\$PATH\""
fi
say "Rust $NEED_RUST+ required for dd_vault. Optional: git (sync), XAI_API_KEY (SpaceXAI)."
say "dd_vault-gui needs Rust $NEED_RUST_GUI+, Vulkan, and Wayland or X11. Set INSTALL_GUI=1 to build it. The default install does not."
say "Done. Try: dd_vault --help"
