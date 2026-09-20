# dd_vault

Keyboard-first, vim-flavored markdown knowledge vault TUI. Files on disk are the source of truth.

## Install

```
curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_vault/main/install.sh | bash
```

From a clone: `./install.sh` (installs to `~/.local/bin`; override with `PREFIX` / `BIN_DIR`).

Needs Rust 1.82+, `cargo`, and `git`. Optional: `XAI_API_KEY` for SpaceXAI; `ollama` / `grok` / `llm` for local AI.

```
dd_vault init ./my-vault
dd_vault open ./my-vault
dd_vault
dd_vault --reindex ./my-vault
```

**Global:** `F1` help · `F2` theme · `Ctrl+Q` quit · `<Space>vv` vaults · `<Space>z` focus · `<Space>h` zen · `<Space>e` tree · `<Space>ai` AI · `<Space><Space>` palette

**Editor:** vim motions, `:w` / `Ctrl+S` save, `:git pull|push|commit`, `:daily`, click to place the caret, drag to select, drag pane borders to resize.

Linux + macOS. MIT.
