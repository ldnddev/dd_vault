# dd_vault

Keyboard-first, vim-flavored markdown knowledge vault TUI. Files on disk are the source of truth.

See [docs/PLAN.md](docs/PLAN.md) for the product spec, [docs/INSTALL.md](docs/INSTALL.md) for install and AUR notes, and [LDNDDEV_TUI_VISUAL_STANDARD.md](LDNDDEV_TUI_VISUAL_STANDARD.md) for chrome and theme tokens.

```
cargo run -p dd_vault -- init ./my-vault
cargo run -p dd_vault -- open ./my-vault
cargo run -p dd_vault
cargo run -p dd_vault -- --reindex ./my-vault
```

**Global:** `F1` help · `F2` theme · `Ctrl+Q` quit · `<Space>vv` vaults · `<Space>z` focus · `<Space>h` zen · `<Space>e` tree · `<Space>ai` AI · `<Space><Space>` palette

**Editor:** vim motions, `:w` / `Ctrl+S` save, `:git pull|push|commit`, `:daily`, click to place the caret, drag to select, drag pane borders to resize.

Linux + macOS. MIT. Rust 1.82+.
