# Tutorial screenshots

Used by `docs/index.html` (GitHub Pages).

| File | Scene |
|---|---|
| `01-install.jpg` | curl install + `dd_vault init` |
| `02-layout.jpg` | tree + editor + preview + AI chip |
| `08-editor.jpg` | INSERT, caret visible |
| `03-finder.jpg` | `Space ff` finder |
| `04-git.jpg` | `:git commit` modal |
| `05-ai.jpg` | AI chat card |
| `07-zen.jpg` | `Space h` zen |
| `06-help.jpg` | `F1` help |

## Replace an image

1. Capture the scene (16:9, ~1280×720, dark terminal).
2. Overwrite the same filename in this folder. JPEG or PNG; if you switch format, update `src` in `../index.html`.
3. Check `alt` text in `index.html` if on-screen copy changed.
4. Commit and push. Pages rebuilds from `docs/`.

Do not rename files unless you update every `img src` in `index.html`.

## Capture tips

- Match `dd_vault_theme.yml` (`#0F1114` background, `#64B4F5` focus, `#FFAF46` files).
- Hide the desktop wallpaper crop; a single terminal window is enough.
- Use a throwaway vault so real notes never land in git.
- Optional tools: your compositor’s screenshot, `grim`, `screencapture -w`, or [VHS](https://github.com/charmbracelet/vhs) if you add a tape later.
