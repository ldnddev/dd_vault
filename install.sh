#!/usr/bin/env bash
# Install dd_vault to PREFIX/bin (default: ~/.local/bin).
#
# From a clone:
#   ./install.sh
#   ./install.sh --uninstall
#
# One-liner (curl):
#   curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | bash -s -- --uninstall
#
# Env:
#   PREFIX      install prefix (default: ~/.local)
#   BIN_DIR     override binary dir (default: $PREFIX/bin)
#   REPO        git URL (default: https://github.com/ldnddev/dd_vault)
#   INSTALL_GUI set to 1 to also build and install dd_vault-gui (Rust 1.92+)
#
# On Linux, install writes a desktop launcher for each binary in BIN_DIR.
# --uninstall removes the binaries and those launchers. It leaves vaults and
# ldnddev config alone. Pass the same PREFIX, BIN_DIR, and XDG_DATA_HOME
# you used to install.

set -euo pipefail

REPO="${REPO:-https://github.com/ldnddev/dd_vault}"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"
NEED_RUST="1.82"
NEED_RUST_GUI="1.92"

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<EOF
Usage: install.sh [--uninstall]

  ./install.sh              build and install dd_vault
  INSTALL_GUI=1 ./install.sh
                            also build and install dd_vault-gui
  ./install.sh --uninstall  remove installed binaries and desktop launchers

Env: PREFIX, BIN_DIR, REPO, INSTALL_GUI=1
EOF
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing '$1' on PATH"
}

on_linux() {
  [[ "$(uname -s)" == "Linux" ]]
}

in_tree() {
  local dir="$1"
  [[ -f "$dir/Cargo.toml" && -d "$dir/crates" && -f "$dir/src/main.rs" ]]
}

# Desktop menus search XDG_DATA_HOME, then the system data dirs.
# A prefix under $HOME still publishes launchers where the session looks.
apps_dir() {
  if [[ -n "${XDG_DATA_HOME:-}" ]]; then
    printf '%s/applications\n' "$XDG_DATA_HOME"
    return
  fi
  case "$PREFIX" in
    "$HOME" | "$HOME"/*)
      printf '%s/.local/share/applications\n' "$HOME"
      ;;
    *)
      printf '%s/share/applications\n' "$PREFIX"
      ;;
  esac
}

# Quote one Exec argument. A literal % is %%.
desktop_quote() {
  local s=$1
  [[ "$s" != *$'\n'* ]] || die "desktop path contains a newline"
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//\`/\\\`}
  s=${s//\$/\\\$}
  s=${s//%/%%}
  printf '"%s"' "$s"
}

write_desktop() {
  local dest=$1 name=$2 comment=$3 bin=$4 terminal=$5 startup=$6 wmclass=${7:-}
  local exec_q
  exec_q="$(desktop_quote "$bin")"
  {
    printf '%s\n' "[Desktop Entry]"
    printf '%s\n' "Type=Application"
    printf '%s\n' "Version=1.0"
    printf '%s\n' "Name=${name}"
    printf '%s\n' "GenericName=Markdown vault"
    printf '%s\n' "Comment=${comment}"
    printf '%s\n' "Exec=${exec_q}"
    printf '%s\n' "TryExec=${bin}"
    printf '%s\n' "Terminal=${terminal}"
    printf '%s\n' "Categories=Utility;TextEditor;"
    printf '%s\n' "Keywords=markdown;notes;vault;"
    printf '%s\n' "StartupNotify=${startup}"
    if [[ -n "$wmclass" ]]; then
      printf '%s\n' "StartupWMClass=${wmclass}"
    fi
  } >"$dest"
  chmod 644 "$dest"
}

install_launchers() {
  if ! on_linux; then
    say "Desktop launchers are written on Linux. Skipped on $(uname -s)."
    return 0
  fi
  local dir
  dir="$(apps_dir)"
  mkdir -p "$dir"
  if [[ -x "$BIN_DIR/dd_vault" ]]; then
    write_desktop \
      "$dir/dd_vault.desktop" \
      "dd_vault" \
      "Open a markdown vault in the terminal" \
      "$BIN_DIR/dd_vault" \
      "true" \
      "false"
    say "Launcher $dir/dd_vault.desktop"
  fi
  if [[ -x "$BIN_DIR/dd_vault-gui" ]]; then
    write_desktop \
      "$dir/dd_vault-gui.desktop" \
      "dd_vault GUI" \
      "Open a markdown vault in a window" \
      "$BIN_DIR/dd_vault-gui" \
      "false" \
      "true" \
      "dd_vault-gui"
    say "Launcher $dir/dd_vault-gui.desktop"
  fi
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$dir" >/dev/null 2>&1 || true
  fi
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

uninstall() {
  local removed=0 path
  local -a paths=(
    "$BIN_DIR/dd_vault"
    "$BIN_DIR/dd_vault-gui"
    "$(apps_dir)/dd_vault.desktop"
    "$(apps_dir)/dd_vault-gui.desktop"
  )
  for path in "${paths[@]}"; do
    if [[ -e "$path" || -L "$path" ]]; then
      rm -f -- "$path"
      say "Removed $path"
      removed=1
    fi
  done
  if [[ "$removed" -eq 0 ]]; then
    say "No dd_vault binaries or launchers found."
  fi
  say "Vaults and ldnddev config were left in place."
}

main() {
  local action=install
  case "${1:-}" in
    "") ;;
    --uninstall | uninstall) action=uninstall ;;
    -h | --help | help) usage; exit 0 ;;
    *) die "unknown argument '$1' (try --help)" ;;
  esac
  if [[ $# -gt 1 ]]; then
    die "unexpected argument '$2'"
  fi

  if [[ "$action" == uninstall ]]; then
    uninstall
    return 0
  fi

  need git
  need cargo
  need rustc
  need install

  local src=""
  if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
    local here
    here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
    if in_tree "$here"; then
      src="$here"
    fi
  fi

  if [[ -z "$src" ]]; then
    local tmp
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    say "Cloning $REPO …"
    git clone --depth 1 "$REPO" "$tmp/dd_vault"
    src="$tmp/dd_vault"
  fi

  build_and_install "$src"
  install_launchers

  if ! command -v dd_vault >/dev/null 2>&1; then
    say "Add to PATH: export PATH=\"$BIN_DIR:\$PATH\""
  fi
  say "Rust $NEED_RUST+ required for dd_vault. Optional: git (sync), XAI_API_KEY (SpaceXAI)."
  if [[ ! -x "$BIN_DIR/dd_vault-gui" ]]; then
    say "dd_vault-gui needs Rust $NEED_RUST_GUI+, Vulkan, and Wayland or X11. Set INSTALL_GUI=1 to build it. The default install does not."
  fi
  say "To remove the binaries and launchers, run this script again with --uninstall."
  say "Done. Try: dd_vault --help"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
