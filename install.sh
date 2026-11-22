#!/usr/bin/env bash
# Install dd_vault to PREFIX/bin (default: ~/.local/bin).
#
# From a clone:
#   ./install.sh                 # ask: terminal, window, or both
#   ./install.sh --tui
#   ./install.sh --gui
#   ./install.sh --both
#   ./install.sh --uninstall     # ask the same question
#
# One-liner (curl):
#   curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | bash -s -- --both
#   curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | bash -s -- --uninstall
#
# Env:
#   PREFIX      install prefix (default: ~/.local)
#   BIN_DIR     override binary dir (default: $PREFIX/bin)
#   REPO        git URL (default: https://github.com/ldnddev/dd_vault)
#   INSTALL_GUI set to 1 for the same result as --both
#
# On Linux, install writes a desktop launcher for each binary it installs.
# --uninstall asks which programs to remove, then leaves vaults and ldnddev
# config alone. Pass the same PREFIX, BIN_DIR, and XDG_DATA_HOME you used
# to install. With no terminal to answer, install selects the terminal app
# and uninstall removes both.

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
Usage: install.sh [--tui | --gui | --both] [--uninstall]

  ./install.sh
      ask: terminal app, desktop window, or both
  ./install.sh --tui
      install dd_vault
  ./install.sh --gui
      install dd_vault-gui
  ./install.sh --both
      install both
  ./install.sh --uninstall
      ask which of those to remove
  ./install.sh --uninstall --tui
  ./install.sh --uninstall --gui
  ./install.sh --uninstall --both

With no terminal to answer, install selects the terminal app and uninstall
removes both. INSTALL_GUI=1 is the same as --both.

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

# Read 1, 2, or 3 from the terminal. stdout is tui, gui, or both.
# $1 is the verb shown in the question. $2 is the default number.
# Returns 1 when the terminal cannot be used. main captures stdout, and a
# failed redirect in that capture does not abort under set -e, so every
# /dev/tty failure has to be an explicit status.
ask_choice() {
  local verb=$1 default=$2 answer choice
  while true; do
    {
      printf '%s which program?\n' "$verb"
      printf '%s\n' "  1) terminal (dd_vault)"
      printf '%s\n' "  2) desktop window (dd_vault-gui)"
      printf '%s\n' "  3) both"
      printf 'Choice [%s]: ' "$default"
    } >/dev/tty 2>/dev/null || return 1
    if ! IFS= read -r answer </dev/tty 2>/dev/null; then
      printf '\n' >/dev/tty 2>/dev/null || true
      answer=$default
    fi
    choice=$(printf '%s' "$answer" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]')
    case "$choice" in
      '' ) choice=$default ;;
    esac
    case "$choice" in
      1 | tui | terminal) printf '%s\n' tui; return 0 ;;
      2 | gui | window) printf '%s\n' gui; return 0 ;;
      3 | both) printf '%s\n' both; return 0 ;;
      *) printf '%s\n' "Enter 1, 2, or 3." >/dev/tty 2>/dev/null || return 1 ;;
    esac
  done
}

# The device node is mode rw-rw-rw, so -r/-w stay true with no controlling
# terminal. Opening it is the check that matters.
have_tty() {
  { : <>/dev/tty; } 2>/dev/null
}

build_tui() {
  local src=$1
  say "Building release binary (opt-level=z, LTO)…"
  cargo build --release --locked --manifest-path "$src/Cargo.toml"
  local bin="$src/target/release/dd_vault"
  [[ -x "$bin" ]] || die "build produced no binary at $bin"
  install -m755 "$bin" "$BIN_DIR/dd_vault"
  say "Installed $BIN_DIR/dd_vault"
}

build_gui() {
  local src=$1
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
}

build_and_install() {
  local src=$1 target=$2
  mkdir -p "$BIN_DIR"
  case "$target" in
    tui) build_tui "$src" ;;
    gui) build_gui "$src" ;;
    both)
      build_tui "$src"
      build_gui "$src"
      ;;
    *) die "internal: unknown target '$target'" ;;
  esac
}

uninstall() {
  local target=$1
  local removed=0 path
  local -a paths=()
  case "$target" in
    tui)
      paths=(
        "$BIN_DIR/dd_vault"
        "$(apps_dir)/dd_vault.desktop"
      )
      ;;
    gui)
      paths=(
        "$BIN_DIR/dd_vault-gui"
        "$(apps_dir)/dd_vault-gui.desktop"
      )
      ;;
    both)
      paths=(
        "$BIN_DIR/dd_vault"
        "$BIN_DIR/dd_vault-gui"
        "$(apps_dir)/dd_vault.desktop"
        "$(apps_dir)/dd_vault-gui.desktop"
      )
      ;;
    *) die "internal: unknown target '$target'" ;;
  esac
  for path in "${paths[@]}"; do
    if [[ -e "$path" || -L "$path" ]]; then
      rm -f -- "$path"
      say "Removed $path"
      removed=1
    fi
  done
  if [[ "$removed" -eq 0 ]]; then
    say "No matching binaries or launchers found."
  fi
  say "Vaults and ldnddev config were left in place."
}

choose_target() {
  local action=$1 explicit=$2
  if [[ -n "$explicit" ]]; then
    printf '%s\n' "$explicit"
    return 0
  fi
  if have_tty; then
    if [[ "$action" == uninstall ]]; then
      ask_choice "Remove" 3 && return 0
    else
      ask_choice "Install" 1 && return 0
    fi
  fi
  # say writes to stdout, and the caller captures this function.
  if [[ "$action" == uninstall ]]; then
    say "No terminal for a prompt. Removing both programs." >&2
    printf '%s\n' both
  else
    say "No terminal for a prompt. Installing the terminal app." >&2
    printf '%s\n' tui
  fi
}

main() {
  local action=install
  local target=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --tui | --gui | --both)
        if [[ -n "$target" ]]; then
          die "pass only one of --tui, --gui, or --both"
        fi
        target=${1#--}
        ;;
      --uninstall | uninstall) action=uninstall ;;
      -h | --help | help) usage; exit 0 ;;
      *) die "unknown argument '$1' (try --help)" ;;
    esac
    shift
  done
  if [[ -z "$target" && "${INSTALL_GUI:-}" == "1" ]]; then
    target=both
  fi
  target=$(choose_target "$action" "$target")

  if [[ "$action" == uninstall ]]; then
    uninstall "$target"
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

  build_and_install "$src" "$target"
  install_launchers

  if { [[ "$target" != gui ]] && ! command -v dd_vault >/dev/null 2>&1; } \
    || { [[ "$target" != tui ]] && ! command -v dd_vault-gui >/dev/null 2>&1; }; then
    say "Add to PATH: export PATH=\"$BIN_DIR:\$PATH\""
  fi
  if [[ "$target" != gui ]]; then
    say "Rust $NEED_RUST+ required for dd_vault. Optional: git (sync), XAI_API_KEY (SpaceXAI)."
  fi
  if [[ "$target" != tui ]]; then
    say "Rust $NEED_RUST_GUI+ required for dd_vault-gui, plus Vulkan and Wayland or X11."
  fi
  say "To remove what you installed, run this script again with --uninstall."
  case "$target" in
    tui) say "Done. Try: dd_vault --help" ;;
    gui) say "Done. Try: dd_vault-gui --help" ;;
    both) say "Done. Try: dd_vault --help    or    dd_vault-gui --help" ;;
  esac
}

# `curl | bash` reads this file from stdin. BASH_SOURCE is unset then, and
# `set -u` aborts on a bare ${BASH_SOURCE[0]}. An empty source means the
# file is being executed, not sourced.
if [[ -z "${BASH_SOURCE[0]:-}" || "${BASH_SOURCE[0]:-}" == "$0" ]]; then
  main "$@"
fi
