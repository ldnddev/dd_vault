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
#   PREFIX   install prefix (default: ~/.local)
#   BIN_DIR  override binary dir (default: $PREFIX/bin)
#   REPO     git URL (default: https://github.com/ldnddev/dd_vault)

set -euo pipefail

REPO="${REPO:-https://github.com/ldnddev/dd_vault}"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"
NEED_RUST="1.82"

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
say "Rust $NEED_RUST+ required. Optional: git (sync), XAI_API_KEY (SpaceXAI)."
say "Done. Try: dd_vault --help"
