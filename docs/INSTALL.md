# Install

`dd_vault` is MIT-licensed and targets **Linux and macOS** (v1). Windows is v4.

## From source

Requires [Rust](https://rustup.rs/) 1.82 or newer.

```
git clone https://github.com/ldnddev/dd_vault
cd dd_vault
cargo build --release
install -Dm755 target/release/dd_vault ~/.local/bin/dd_vault
```

Or without a clone:

```
cargo install --git https://github.com/ldnddev/dd_vault --locked
```

`git` should be on `PATH` for vault sync (`:git pull|push|commit`). Optional: `ollama`, `grok`, or `llm` for local AI; `XAI_API_KEY` for SpaceXAI.

## Config

- Themes: `./dd_vault_theme.yml`, then `~/.config/ldnddev/dd_vault_theme.yml`, then the built-in palette
- App settings: `~/.config/ldnddev/config.toml` (`[ai]`, `secret_patterns`)
- Vault list: `~/.config/ldnddev/vaults.toml`
- Git credentials (optional, mode `0600`): `~/.config/ldnddev/credentials`

## Arch Linux (AUR)

A dedicated AUR package is not published yet. Until it is:

```
# PKGBUILD sketch (git, rust, make)
pkgname=dd_vault-git
pkgver=0.1.0
pkgrel=1
pkgdesc='Keyboard-first vim-flavored markdown vault TUI'
arch=('x86_64' 'aarch64')
url='https://github.com/ldnddev/dd_vault'
license=('MIT')
depends=('gcc-libs' 'git')
makedepends=('cargo' 'git')
# build() { cargo build --release --locked }
# package() { install -Dm755 target/release/dd_vault "$pkgdir/usr/bin/dd_vault" }
```

Build locally with `makepkg -si` from a PKGBUILD that clones this repo, or `cargo install --path .` after `git clone`.

## Verify

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
```
