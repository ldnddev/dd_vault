# dd_vault — Revised Master Plan (visual-standard aligned)

A keyboard-first, vim-flavored markdown knowledge vault TUI in Rust on `ratatui`, in the ldnddev family.

This revises `docs/PLAN.md` so it can actually be built as a sibling of `dd_ftp` / `dd_emailforge` / `dd_siteforge`. Spec questions were declined; **assumed defaults are marked ★** and should be overridden before code if they are wrong.

- **Binary / product name:** `dd_vault` (not TermiLando)
- **Config root:** `$XDG_CONFIG_HOME/ldnddev/` else `~/.config/ldnddev/`
- **Vault metadata:** `<vault>/.dd_vault-<name>/`
- **Theme file:** `dd_vault_theme.yml` (YAML, schema `version: 1`)
- **License:** MIT
- **Platforms:** Linux + macOS (v1), Windows (v4)
- **MSRV:** Rust 1.82, edition 2021 (theme crate currently declares 1.74; workspace MSRV wins)
- **Author model:** Solo
- **Status:** Plan revised — no code until "go"

---

## 0. What was reviewed

| Artifact | Role |
|---|---|
| `docs/PLAN.md` | Original product/architecture lock (Q5–Q60). Keep domain decisions; replace chrome/theme/crate-shape where they fight the family standard. |
| `LDNDDEV_TUI_VISUAL_STANDARD.md` | **Source of truth for chrome, tokens, F1/F2, toasts, mouse, lookup order.** Body layout is app-defined. |
| `dd_vault_theme.yml` | App default palette. Comments still say `dd_emailforge` — fix on first theme PR. |
| `crates/ldnddev_theme` | Shared YAML load/save + live `ThemeEditor`. Already copied into this repo, same as siblings. Canonical copy also lives at `/home/jlyvers/Projects/ldnddev_theme`. |
| `dd_vault.png` | Light-theme **layout mockup** labeled TERMILANDO. Not a logo. Use for body geometry (tree / editor / preview / AI card / toasts), not for header toolbar or product name. |

### Conflicts in the original plan (resolved below)

| Original lock | Conflict | Resolution ★ |
|---|---|---|
| Q49 TOML palettes `dd_dark` / `dd_light` / `dd_solarized` / `dd_gruvbox` | Standard + crate + YAML are canonical tokens, not named TOML palettes | YAML + `ldnddev_theme` only. Extra palettes later as more YAML files with the same keys. |
| Q50 footer = mode / git / AI status | Standard: footer is key hints only, starts `F1:Help` `F2:Theme`, no persistent status | Status lives in **editor pane title** + header title-right (vault name). Git/AI changes are toasts. |
| §7 header = git buttons, vault, `?` | Standard: 3-line decorative header, tagline, no mouse/keyboard. Header is not a toolbar. | Family header. Vault name on title-right (same exception `dd_ftp` uses for connection). Git via `:git` / leader keys. |
| Q52 toasts 3s | Standard ~5s | **5s** auto-dismiss. |
| Q46 zen hides header/footer | Standard: header 3 / footer 1, heights not dynamic | Focus mode collapses tree. Zen hides tree + preview + AI, **keeps chrome**. |
| Crate list omits `ldnddev_theme` | Crate already in tree | First-class workspace member. |
| v1 milestone omits SQLite; Q9/Q10/Q22 require FTS5 for `<Space>sg` | Internal contradiction | **SQLite + FTS5 in v1.** Backlinks pane / outline / rename-refactor / RAG stay v2. |
| v3 milestone lists toasts | Standard requires toasts in every app | Toasts in v1. |
| `dd_vault.png` unused | Mockup is the body brief | Body + AI card + toast stack follow the PNG; chrome does not. |
| Theme YAML comments say `dd_emailforge` | Copy-paste | Rewrite comments to `dd_vault`. |

---

## 1. Product summary

`dd_vault` is a keyboard-first, vim-flavored markdown knowledge vault TUI. Files on disk are the source of truth; SQLite is a rebuildable derived cache. Offline-first. Privacy-first. Git is the only sync mechanism in v1. AI is opt-in, provider-agnostic (API key or local CLI), never sends vault context without explicit per-request consent.

It must **feel like an ldnddev TUI**: same tokens, same 3+1 chrome, same F1/F2, same toasts/modals/inputs/scrollbars/mouse contract. The body is a vault (tree + editor + preview), not a copy of `dd_ftp`'s two-pane browser.

---

## 2. Assumed defaults ★ (override before "go")

These were the recommended answers to the unanswered spec questions.

1. **Chrome:** Family 3-line tagline header + F1/F2 footer. Mockup body. No header toolbar.
2. **Theme:** `ldnddev_theme` YAML only. F2 = live color editor (save local/global). No TOML palettes.
3. **Identity:** Product/binary `dd_vault`. PNG is layout reference, not a logo, not TermiLando branding.
4. **Index:** SQLite + FTS5 in v1. Backlinks UI / outline / rename-refactor / RAG in v2.
5. **CLI:** `dd_vault` opens last vault or picker; `init` / `open` / `--reindex`. In-app `<Space>vv` switches registered vaults (one visible). Multi-vault split stays v3.
6. **Buffers:** One open note. Explicit `:w` / `<C-s>`. Dirty confirm on navigate-away. **Live watcher** (inotify/kqueue): reload if clean; toast if dirty.
7. **Obsidian-facing v1 extras (on top of locked GFM/wikilinks/frontmatter/callouts/tags):**
   - `[[` nucleo picker + `[[note#heading]]`
   - Frontmatter first-class: `title`, `tags`, `aliases`, `date`. Title = frontmatter → first H1 → filename. Aliases participate in wikilink + search.
   - Line numbers + markdown syntax highlighting in the editor
   - Clipboard image paste → `assets/<note-stem>-<ts>.png` + markdown image link
   - `:daily` / `<Space>nd` → `notes/daily/YYYY-MM-DD.md`
   - Command palette `<Space><Space>` (and `:`) over commands + leader actions
   - `![[note]]` renders in **preview only** (no transclusion editing)
8. **AI:** Provider trait. First named API preset is **SpaceXAI** (`XAI_API_KEY`, `https://api.x.ai/v1`, model from live xAI docs at implement time). Also a generic OpenAI-compatible preset + CLI providers (`ollama`, grok CLI). Off until the user enables a provider.
9. **Crates:** Fewer than the original 14. See §4.

Not in v1: macros, multi-cursor, sixel/kitty/iTerm2, mermaid/DBML inline, rclone, Windows, `*.assets/` sidecar mode, plugin surface, canvas editor, drag/drop, multi-vault split.

---

## 3. Locked domain decisions (kept from original)

Keep original Q5–Q8, Q11–Q48, Q51, Q53–Q60 except where §0 overrides Q46/Q49/Q50/Q52.

| # | Decision |
|---|---|
| Q5 | Vault = folder + `.dd_vault-<name>/` metadata dir |
| Q8 | No folder-per-note. Optional `*.assets/` mode deferred to v2 |
| Q9 | SQLite derived cache; WAL + `synchronous=NORMAL`; FTS5; `sqlite-vec` later |
| Q10 | DB-first startup, background `ignore`-crate reindex, event-driven UI updates |
| Q11 | Split right pane: editor top, collapsible preview bottom |
| Q12 | GFM + `[[wikilinks]]` + YAML frontmatter + Obsidian callouts |
| Q13 | `comrak` AST → custom `dd_render` for `ratatui` |
| Q14 | `dd_term_image` module; half-block in v1, sixel/kitty/iTerm2 in v2 |
| Q15 | Mermaid via `mmdc` shell-out, hash-cached PNG — **v3** |
| Q16 | DBML via `@dbml/cli` shell-out, hash-cached SVG→raster — **v3** |
| Q17 | Vim-flavored subset (not full emulation) |
| Q18 | `ropey` + custom vim mode layer in `dd_edit` |
| Q19 | Undo/redo, search/replace, marks, registers in v1; macros v2; multi-cursor v3 |
| Q21 | `nucleo` in-process, no `fzf` shell-out |
| Q25 | `#tag`, `#nested/tag` |
| Q26 | `[[wikilinks]]` + `[md](path.md)` |
| Q30 | Git via CLI (`git status --porcelain=v2`) |
| Q31 | Ignore `.dd_vault-*/`, `*.db*`, caches, OS junk; never ignore notes/attachments |
| Q32 | Manual pull/push/commit only; conflict modal |
| Q33 | SSH agent → OS credential helper → `0600` credential-store file; never in vault; pre-commit secret scan |
| Q38 | 3-tier conflicts: auto 3-way → `.conflict-<ts>.md` sidecar → binary always duplicates |
| Q40–Q45 | AI API+CLI, drafts/rewrites/summarize, streaming, off by default, no RAG in v1 |
| Q47 | Full mouse: focus, click tree, scroll, drag-resize panes |
| Q48 | Image input via clipboard paste in v1; drag/drop deferred |
| Q51 | Keybindings TOML with per-vault override |
| Q54 | `tokio` async runtime |
| Q55 | `tracing` + `tracing-appender`, off by default |
| Q56 | `insta` + `proptest` + `tempfile`; CI test + clippy + `cargo deny` |
| Q57 | AUR + prebuilt binaries |
| Q58 | MIT |
| Q59 | Rust 1.82, edition 2021 |
| Q60 | `thiserror` in libs, `anyhow` at binary boundary |

`ldnddev_theme` already uses `anyhow` internally; do not rewrite it to `thiserror`. App crates follow Q60.

**ratatui:** `0.30` with `crossterm`, matching `dd_ftp` / `dd_emailforge`. Theme crate stays ratatui-free; convert with `Color::Rgb(r,g,b)`.

---

## 4. Crate layout ★

Original 14-crate split is too much for a solo v1. Keep the *module names*; ship fewer packages.

```
dd_vault/                 # binary — CLI, wiring, event loop, tokio runtime
crates/
  ldnddev_theme/          # EXISTING — YAML tokens, lookup, ThemeEditor (do not fork API)
  dd_vault_core/          # vault model, files, git, sqlite/FTS5, search, config, events
  dd_tui/                 # ratatui shell (header/footer/F1/F2/toasts/modals), body layout, mouse
  dd_edit/                # ropey buffer + vim mode + registers + undo tree
  dd_render/              # comrak AST → ratatui Text; half-block images
  dd_ai/                  # provider trait, streaming, redacted log
```

Workspace `Cargo.toml` with `[workspace.dependencies]`. `ldnddev_theme = { path = "crates/ldnddev_theme" }`.

Later splits (when a boundary hurts): `dd_index`, `dd_search`, `dd_git`, `dd_sync` (v5), `dd_term_image`, `dd_diagram` (v3), `dd_config`, `dd_events`.

Do not path-depend on `/home/jlyvers/Projects/ldnddev_theme`. Vendor like every sibling. If the canonical crate gains a fix, copy it in (do not silently diverge on token names or lookup order).

---

## 5. Visual system (this is the new §10)

### 5.1 Lookup (exact, from the standard)

1. `./dd_vault_theme.yml` — project-local override (cwd when launched, **not** the vault folder)
2. `$XDG_CONFIG_HOME/ldnddev/dd_vault_theme.yml` else `~/.config/ldnddev/dd_vault_theme.yml`
3. Built-in defaults from `Palette::builtin()` / `dd_vault_theme.yml` embedded in the binary

Reject missing or non-`1` `version` with a **warning toast** and fall through. Never silently ignore a bad file.

Per-vault color overrides are **not** a thing in v1. Vault config can still override keybindings and app settings.

### 5.2 Tokens

Use the canonical keys in `LDNDDEV_TUI_VISUAL_STANDARD.md` and `COLOR_FIELDS` in `ldnddev_theme`. `dd_vault_theme.yml` already has the family dark palette.

Required extras for this app (pass into `ThemeEditor::new`):

- `EXTRA_MODAL_HEADER` — F1/F2 section headers (YAML already has `modal_header`)
- `EXTRA_TEXT_DISABLED` / `EXTRA_TEXT_INVERSE` if the editor uses them
- `EXTRA_SELECTION` for visual-mode highlight if it is distinct from `selected_background`

Do not invent keys (`editor_gutter`, `git_dirty`, …). Map:

| UI | Tokens |
|---|---|
| App shell, header, footer | `base_background` + `text_primary` → `app_shell` |
| Tree, editor, preview | `body_background` |
| Focused pane border | `border_active` |
| Idle pane border | `border_default` |
| Selected tree row | `selected_background` + `text_active_focus` |
| Dirs / files / wikilinks in tree & pickers | `folders` / `files` / `links` |
| Inputs, command line, search | `input_*` + `cursor` |
| Toasts, git dirty, AI local/network | `success` / `warning` / `error` / `info` |
| Modals | `modal_background` / `modal_text` / `modal_labels` / `modal_header` |
| Scrollbars | `scrollbar` / `scrollbar_hover` |

Never hard-code a color after load.

### 5.3 F1 / F2

- **F1 Help:** centered modal, full keyboard + mouse + app notes, wrap + scroll + scrollbar. Same chrome as siblings.
- **F2 Theme:** live `ThemeEditor` (j/k, `[` `]` channel, `+`/`-` nudge, Enter hex, Tab local/global save target, `y`/`s` save, `R` reset builtin, Esc revert+close). Header of the modal still shows source (`local`/`global`/`default`), schema version, load status. This matches `dd_emailforge` / `dd_siteforge`, which is stricter than the standard's "inspector-only" F2 and is what the crate is for.

`:theme` reloads lookup (or opens F2). No named-palette switcher in v1.

### 5.4 Toasts and modals

- Toasts: bottom-right, four semantic colors only, **5s**, stack above the AI card (see mockup). Non-blocking only.
- Modals: centered, dim underneath, Esc cancels unless the error must be dismissed.
- Blocking: git conflicts, delete confirm, dirty-navigate, first-run vault picker.

### 5.5 Header quotes ★

Pick one at startup (time XOR pid). Override via `header_quotes` in the YAML.

Built-in list (vault personality, not TermiLando):

- "Files on disk. Opinions in git."
- "Wikilinks, not vendor lock-in."
- "The vault is the folder. The rest is a cache."
- "hjkl through your second brain."
- "Preview is a courtesy. Markdown is the truth."
- "AI stays in the corner until you ask."
- "Offline first. Sync is a git push."
- "One note, one file, no surprises."

### 5.6 `dd_vault.png` mapping

Use the mockup as the **body** brief:

```
header (3)  — title "dd_vault" | title-right vault name | inner tagline
body        — tree | editor
            —       | preview (collapsible)
            — AI card + toasts overlay bottom-right
footer (1)  — F1:Help  F2:Theme  … adaptive hints
```

From the PNG, keep: tree with folder/file coloring, editor with line numbers, collapsible preview, toast stack, AI card (collapsed / chat / full-split states already locked). Drop: GUI toolbar, light-theme chrome, "TERMILANDO" wordmark, CMD PALETTE button (palette is `<Space><Space>`, not a header control), layout-mode buttons in the header (`<Space>z` / `<Space>h` instead).

No TUI logo. Do not half-block `dd_vault.png` into the header.

---

## 6. Shell layout (designer + implementer contract)

Fixed vertical split from the standard:

```rust
let outer = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Length(3), // header
        Constraint::Min(0),    // body
        Constraint::Length(1), // footer
    ])
    .split(frame.area());
```

Full-screen `Block` with `app_shell`. Header always 3 (including borders). Footer always 1. Body is the product.

**Header**

- Bordered `Block`, `.title("dd_vault")`, `borders(ALL)`, `border_style(theme.active_border)`, `style(theme.app_shell)`
- Title-right: ` vault:<name> ` in `text_secondary`, or `success` when git is clean after a pull/push toast — **not** a clickable control
- Inner line: the session tagline (`text_primary` or `text_secondary`)
- Decorative. No mouse hit targets. No git buttons.

**Footer** (width-adaptive, always starts `F1:Help`)

```
# narrow
F1:Help  F2:Theme  ^Q:Quit  :w  <Space>ff

# medium
F1: Help   F2: Theme   j/k: Nav   i: Insert   :w Save   ^Q: Quit

# wide
F1: Help   F2: Theme   j/k: Nav   i: Insert   :w Save   <Space>ff Files   <Space>ai AI   ^Q: Quit   (mouse: click/scroll)
```

**Body — Option A default (from PLAN + PNG)**

```
┌─ tree ──────────┬─ note.md — NORMAL ±3  git:clean ──────────┐
│ ▾ notes/        │  1  # Heading                             │
│   ▸ daily/      │  2                                        │
│   note.md       │  3  Body…                                 │
│ ▸ assets/       │                                           │
│                 ├─ preview ─────────────────────────────────┤
│                 │  rendered markdown                        │
└─────────────────┴───────────────────────────────────────────┘
```

- Tree: Unicode prefixes, `folders`/`files`/`links`, `h`/`l` collapse, Enter open, `a` new, `r` rename, `d` delete (confirm modal), `/` filter, mouse click/double-click per standard tree recipe.
- Editor title carries **mode, filename, dirty ±N, optional git short status**. This replaces the old status footer.
- Preview collapsible (`<Space>p`). When collapsed, editor takes the full right column.
- Pane borders: idle `border_default`, focused `border_active`.
- Drag-resize the tree/editor split and the editor/preview split (Q47). Capture `Rect`s every frame.
- AI card: bottom-right overlay, 3×1 collapsed / ~40×12 chat / full-split. Toasts stack above it.
- Narrow terminals (<80 cols): tree becomes a toggle (`<Space>e` explore); never clip chrome.

**Option B — Focus (`<Space>z`):** tree collapses to a 1-col icon rail (or hides). Editor full width. Preview overlay or still bottom-split (prefer still bottom-split to avoid a second layout language).

**Option C — Zen (`<Space>h`):** hide tree, preview, AI. **Keep header and footer.** Pure editor in the body.

**Option D — Split tree:** v3+.

---

## 7. Vault layout on disk

```
my-vault/
├── .dd_vault-my-vault/
│   ├── config.toml
│   ├── keybindings.toml
│   ├── index.db              # + -wal, -shm
│   ├── cache/
│   │   ├── mermaid/<hash>.png    # v3
│   │   ├── dbml/<hash>.png       # v3
│   │   └── preview/<hash>.txt
│   ├── sync/queue.jsonl          # v5 (create dir, unused in v1)
│   └── logs/
│       ├── app.log
│       └── ai.jsonl
├── .gitignore
├── notes/
│   ├── daily/
│   └── projects/
├── assets/
└── *.md, images, attachments
```

`*.canvas` / `*.mermaid` / `*.dbml` may exist on disk and show in the tree as files. No editors for them in v1; mermaid/dbml fenced blocks in markdown get syntax-highlighted code fallback until v3.

Global: `~/.config/ldnddev/vaults.toml`, `dd_vault_theme.yml`, `config.toml`, `keybindings.toml`, `credentials` (0600).

`.gitignore` at vault init:

```
.dd_vault-*/
*.db
*.db-wal
*.db-shm
.dd_vault-cache/
.DS_Store
```

---

## 8. Data model (SQLite, v1)

All tables derived and rebuildable. `dd_vault --reindex [path]`.

- `files(id, path, mtime, size, hash, kind)` — kind ∈ {note, asset, other}
- `notes(file_id, title, frontmatter_json)`
- `aliases(note_id, alias)`
- `tags(id, name, parent_id)` + `note_tags(note_id, tag_id)`
- `links(id, src_note_id, dst_raw, dst_note_id, dst_heading, kind)` — kind ∈ {wiki, md, embed}
- `headings(id, note_id, level, text, line)`
- `fts_notes(note_id, title, aliases, body)` — FTS5
- `embeddings(...)` — v2, do not create yet

**Startup**

1. Open last vault or picker.
2. Read DB if present; paint tree immediately.
3. Spawn background walker (`ignore`) comparing mtime/size/hash.
4. Reindex changed files; `tokio::sync::watch` / broadcast to UI.
5. No DB → full walk, progress via **toasts / editor-title**, never the footer.
6. Start filesystem watcher after first walk.

Skip files over a size cap (★ **5 MiB** for FTS body; still listed in `files`). Binary assets hashed but not FTS-indexed.

---

## 9. CLI ★

```
dd_vault                    # last vault from vaults.toml, else picker
dd_vault init [path]        # create vault + metadata + .gitignore + notes/ + assets/
dd_vault open <path>        # register and open
dd_vault --reindex [path]   # rebuild index, no TUI
dd_vault --help
```

`vaults.toml` stores `{ name, path, last_opened }`. `<Space>vv` picker lists them.

Init name = folder basename. Metadata dir = `.dd_vault-<basename>/`.

---

## 10. Keybindings (v1 defaults)

**Global (even in INSERT, except where noted)**

| Key | Action |
|---|---|
| `F1` | Help modal |
| `F2` | Theme editor |
| `<C-q>` | Quit (confirm if dirty) |
| `<C-s>` | Save |
| `<Space>ff` | File finder (nucleo) |
| `<Space>sg` | Content search (FTS5 + nucleo re-rank) |
| `<Space>tg` | Tag search |
| `<Space>vv` | Vault picker |
| `<Space>p` | Toggle preview |
| `<Space>ai` | Toggle AI card |
| `<Space>z` | Focus mode |
| `<Space>h` | Zen (body-only) |
| `<Space>b` | Toggle backlinks — **stub in v1, real pane v2** |
| `<Space>o` | Toggle outline — **stub v1 / real v2** |
| `<Space>nd` | Daily note |
| `<Space>e` | Toggle tree (narrow or any) |
| `<Space><Space>` | Command palette |
| `<Esc>` | Cancel modal / AI stream / return to NORMAL |

Leader `<Space>` is NORMAL/tree only, not INSERT (except F-keys, `<C-s>`, `<C-q>`, `<Esc>`).

**Editor NORMAL** — keep original: `hjkl`, `wb e`, `0$`, `gg G`, `i a o O`, `v V`, `dd yy p P`, `u` `<C-r>`, `/ n N`, `m{a-z}` `` `{a-z} ``, `"{a-z}`, `:`.

**Tree** — `jk`, Enter, `hl`, `a r d`, `/` filter.

**`:` commands**

`:w` `:q` `:wq` `:q!` `:e <file>` `:tag <t>` `:tags` `:link <note>` `:backlinks` `:rename <new>` `:daily` `:git pull|push|commit` `:ai <prompt>` `:theme` `:config` `:help`

`:rename` in v1 renames the file and updates the DB; **link rewrite across the vault is v2** (Q29 stays v2). v1 warn-toast: "links not rewritten yet".

---

## 11. Subsystems

### 11.1 Git (`dd_vault_core::git`)

- Shell out to `git`. `git status --porcelain=v2 --branch` + `git log`.
- Auth: SSH agent → OS credential helper → `0600` `~/.config/ldnddev/credentials`.
- Pre-commit scan: `ghp_`, `github_pat_`, `AKIA`, plus config patterns. Reject on match.
- No header buttons. `:git commit` opens a modal for message. Pull/push/commit also in the command palette.
- Status string in editor title: `git:clean` / `git:±N` / `git:conflict`. Details via toasts.

### 11.2 Conflicts

1. Non-overlapping → `git merge-file` 3-way.
2. Overlapping → `note.conflict-<timestamp>.md`, keep original, modal lists them.
3. Binary → always duplicate.

Never last-write-wins. Never overwrite silently.

### 11.3 AI (`dd_ai`)

- Trait: `complete_streaming(request) -> Stream<Delta>`.
- Presets: SpaceXAI (first), generic OpenAI-compatible, CLI (`ollama`, grok CLI, `llm`).
- v1 tasks: draft from prompt, rewrite selection, summarize note.
- Output: insert at cursor, replace selection.
- Streaming: SSE (`reqwest` event source) / stdout pipe. `<Esc>` cancels.
- Context: current note + selection + 1-hop links. Confirm modal lists what will be sent (per-request consent).
- Privacy: off by default; per-provider allowlist; redacted `logs/ai.jsonl`; `local` vs `network` badge on the card.
- Card states: collapsed 3×1, chat ~40×12, full split (hides preview).

At implement time, fetch current xAI model names from https://docs.x.ai/developers/models — do not hard-code a stale `grok-4.5` if docs have moved.

### 11.4 Rendering

- `comrak` AST → `ratatui::Text`. Callouts, GFM tables, task lists, code blocks (syntect), images via half-block.
- Missing `mmdc` / dbml: fenced block as highlighted code (v1). v3 adds hash-cached raster.
- Preview images: `dd_render::term_image` half-block. Runtime protocol detection is v2.

### 11.5 Editing

- `ropey` + custom vim layer. No `edtui` / `tui-textarea`.
- Undo tree, search/replace, marks, registers in v1.
- Line numbers in gutter (`text_secondary`).
- Syntax highlight markdown with `syntect`; map as close as possible onto tokens (`text_primary`, `text_labels`, `links`, `files`, semantic colors for headings/code). Accept that syntect themes will not be 1:1 with YAML tokens; pick a dark syntect theme that does not fight `body_background`.
- Clipboard image paste only when NORMAL/INSERT in editor and clipboard is image data.

### 11.6 Search

- Files: `nucleo` over `files.path`.
- Content: FTS5 → nucleo re-rank on snippets. No DB yet (first launch) → `grep-searcher` fallback.
- Tags: DB + fuzzy.
- Previews: first ~50 lines at `cache/preview/<hash>.txt`.
- Pickers are centered modals, not a fourth layout.

### 11.7 Watcher

- `notify` crate, debounced (~200ms).
- Update index + tree.
- If the event is the open file: reload when clean; if dirty, `warning` toast with Reload / Keep.

---

## 12. Config

**Global** `~/.config/ldnddev/config.toml` (app settings, not colors): last vault, AI providers, logging, size caps, git scan patterns.

**Theme** YAML as §5. **Not TOML.**

**Keybindings** TOML, per-vault override in `.dd_vault-<name>/keybindings.toml`.

**Vault** `.dd_vault-<name>/config.toml`: preview default on/off, wrap, daily-note path, ignore globs extra.

Logging: `tracing` + `tracing-appender` to `logs/app.log`. Off by default; toggle in `:config`.

---

## 13. Testing & CI

- `ldnddev_theme` tests already cover lookup, strict/lenient parse, editor save/revert — keep them.
- `insta` snapshots of `ratatui::TestBackend` for: default layout, focus, zen, F1, F2, toast stack, AI collapsed/chat, narrow 80-col, dirty editor title.
- `proptest` for rope/edit ops.
- `tempfile` vault fixtures for index, wikilink resolve, git ignore, theme lookup (local/global/default/bad version).
- Theme checklist from the standard (8 items) as an integration test.
- CI: `cargo test --workspace`, `clippy -D warnings`, `cargo deny`.

Screenshot/docs later, same `docs/capture.sh` pattern as `dd_ftp` if desired — not a v1 blocker.

---

## 14. Milestones (corrected)

**v1 — MVP (this plan)**

Family chrome (header/footer/F1/F2/toasts/modals/theme YAML). Vault init/open/registry. Tree. Vim-flavored editor with line numbers + md highlight. Explicit save. Live watcher. Collapsible preview (GFM, wikilinks, callouts, half-block images). SQLite + FTS5. nucleo file/content/tag search. `[[` picker. Daily note. Command palette. Git pull/push/commit via CLI + secret scan. Mouse + drag-resize. Clipboard image paste. AI card (SpaceXAI preset + generic + CLI, streaming, off by default).

**v2**

Backlinks pane, outline, wikilink rename-refactor (transactional rewrite), `![[ ]]` richer embeds, macros, `*.assets/` mode, terminal image protocols, local embeddings RAG (`fastembed-rs` + `sqlite-vec`).

**v3**

Mermaid + DBML inline, multi-cursor, multi-vault split (Option D).

**v4**

Conflict polish, offline queue completion, Windows.

**v5**

rclone behind `dd_sync` trait, plugin surface.

---

## 15. Implementation order (PR plan)

Each PR independently reviewable. No giant "everything" PR.

| PR | Title | Depends | What |
|---|---|---|---|
| 0 | Workspace + `ldnddev_theme` | — | Root `Cargo.toml`, members, `workspace.dependencies` (ratatui 0.30, tokio, thiserror, anyhow, serde, serde_yaml). Leave theme crate API unchanged. Fix `dd_vault_theme.yml` comments (`dd_vault` not `dd_emailforge`). Embed YAML as builtin fallback. |
| 1 | Shell chrome | 0 | `dd_tui` + binary: app_shell, 3-line header + taglines, 1-line footer, F1 help, F2 ThemeEditor wired to `ldnddev_theme`, toasts 5s, empty body pane, mouse rects, narrow/wide footer. Snapshot tests. **This is the family-contract PR; do not skip it.** |
| 2 | Vault core + CLI | 0 | `dd_vault_core`: init/open/registry, `.dd_vault-<name>/`, gitignore template, `vaults.toml`. CLI `init`/`open`/`--reindex`. No TUI yet beyond opening a path into state. |
| 3 | Tree | 1, 2 | Vault walker, tree widget, folder/file/link colors, expand/collapse, new/rename/delete confirm, mouse. |
| 4 | Editor | 1 | `dd_edit`: ropey, vim subset, undo, marks, registers, line numbers, syntect md, dirty flag, `:w` `<C-s>`, `:e`. Bind into right pane. |
| 5 | Preview | 4 | `dd_render`: comrak → ratatui, callouts, task lists, collapsible split, `<Space>p`. |
| 6 | Index + search | 2, 3 | SQLite schema/migrations, FTS5, background reindex, nucleo pickers (`ff`/`sg`/`tg`), `[[` picker, heading fragments. |
| 7 | Watcher + single-buffer reload | 4, 6 | `notify`, dirty vs clean, toasts. |
| 8 | Git | 2, 4 | status in editor title, `:git pull\|push\|commit` modal, secret scan, conflict modal. |
| 9 | Images | 5 | Clipboard paste → `assets/`, half-block in preview. |
| 10 | Daily + palette + extras | 4, 6 | `:daily`, `<Space><Space>` palette, aliases, frontmatter title resolution. |
| 11 | AI card | 4 | `dd_ai` trait, SpaceXAI + generic + CLI, streaming, consent modal, redacted log, card states. |
| 12 | Focus / zen / polish | 1–11 | `<Space>z` / `<Space>h`, drag-resize, help text complete, theme lookup tests, `cargo deny`, install/AUR notes. |

Do not start PR 3+ until PR 1 snapshots prove chrome matches the standard.

---

## 16. Docs to write during implementation (not before "go")

Replace original "docs to write next" with files that match this plan:

1. `docs/ARCHITECTURE.md` — crate graph, event flow, startup
2. `docs/DATA_MODEL.md` — DDL, migrations, reindex
3. `docs/KEYBINDINGS.md` — defaults + TOML schema
4. `docs/CONFIG.md` — global TOML + theme YAML schema
5. `docs/GIT.md` — auth, scan, conflicts
6. `docs/AI.md` — provider trait, log format, privacy
7. `docs/LAYOUT.md` — this §6, not the old Option A ASCII with git header
8. `docs/ROADMAP.md` — §14 with acceptance criteria
9. Keep `LDNDDEV_TUI_VISUAL_STANDARD.md` at repo root (do not fork app-specific layout into it)

After "go", also update `docs/PLAN.md` in-repo so it no longer contradicts the standard.

---

## 17. Acceptance criteria for v1 chrome (must pass)

Copied from the standard's ship checklist, specialized:

1. `./dd_vault_theme.yml` loads as `local`
2. Missing local → `~/.config/ldnddev/dd_vault_theme.yml` as `global`
3. Missing both → builtin
4. Bad/missing `version` falls back + warning toast
5. Every required key parses
6. Every token maps to a real widget (file-role colors used in the tree)
7. Loaded values win over builtin hex
8. Focus uses `*_focus` / `text_active_focus` / `border_active`
9. Header is 3 rows, footer is 1, both non-dynamic even in zen
10. Footer always starts `F1:Help` then `F2:Theme`
11. No hard-coded colors in render paths after load
12. F2 can save global and local and reload
13. Toasts 5s, four semantic colors, above AI card
14. 80-col and 120-col snapshots do not clip chrome

---

## 18. Out of scope until a later version

Canvas editing, dataview-like queries, encryption, spellcheck, multi-cursor, macros, sixel/kitty, mermaid/dbml render, rclone, Windows, plugins, folder-per-note, drag/drop, header toolbar, TOML color palettes, TermiLando rename, embedding a logo in the TUI.

---

*No code until "go." If any ★ assumption is wrong, say so before implementation.*
