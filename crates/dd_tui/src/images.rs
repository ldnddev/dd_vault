//! Clipboard image paste → `assets/<note-stem>-<ts>.png`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use dd_edit::Mode;
use dd_render::encode_png_rgba;

use crate::app::{App, Pane};
use crate::toasts::ToastLevel;

impl App {
    /// Ctrl+V in NORMAL/INSERT: if the OS clipboard holds image pixels, save and insert.
    /// Returns true when the key was consumed (image handled, or image found but failed).
    pub fn try_paste_clipboard_image(&mut self) -> bool {
        if self.pane != Pane::Editor || !matches!(self.editor.mode, Mode::Normal | Mode::Insert) {
            return false;
        }
        let mut cb = match arboard::Clipboard::new() {
            Ok(c) => c,
            Err(_) => return false,
        };
        let img = match cb.get_image() {
            Ok(i) => i,
            Err(_) => return false,
        };
        let w = img.width as u32;
        let h = img.height as u32;
        let bytes = img.bytes.into_owned();
        self.paste_image_rgba(w, h, &bytes);
        true
    }

    pub fn paste_image_rgba(&mut self, width: u32, height: u32, rgba: &[u8]) {
        let Some(vault) = self.vault.clone() else {
            self.push_toast(ToastLevel::Error, "No vault open");
            return;
        };
        if self.editor.rel.is_none() {
            self.push_toast(ToastLevel::Warning, "Open a note before pasting an image");
            return;
        }
        let png = match encode_png_rgba(width, height, rgba) {
            Ok(p) => p,
            Err(err) => {
                self.push_toast(ToastLevel::Error, format!("Image encode failed: {err}"));
                return;
            }
        };
        let stem = note_stem(self.editor.rel.as_deref());
        let rel = unique_asset_rel(&vault.root, &stem);
        let abs = vault.root.join(&rel);
        if let Some(parent) = abs.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                self.push_toast(ToastLevel::Error, err.to_string());
                return;
            }
        }
        if let Err(err) = std::fs::write(&abs, png) {
            self.push_toast(ToastLevel::Error, err.to_string());
            return;
        }
        let link = format!("![]({})", rel_display(&rel));
        self.editor.insert_snippet(&link);
        self.reload_tree();
        self.refresh_git();
        self.push_toast(ToastLevel::Success, format!("Pasted {}", rel.display()));
    }

    /// `y`, `yy`, and visual `y`: put the yanked text on the OS clipboard.
    pub fn copy_text_to_clipboard(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text)) {
            Ok(()) => {}
            Err(err) => {
                self.push_toast(ToastLevel::Warning, format!("Clipboard copy failed: {err}"));
            }
        }
    }
}

fn note_stem(rel: Option<&str>) -> String {
    let raw = rel
        .and_then(|r| Path::new(r).file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or("paste");
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if cleaned.is_empty() {
        "paste".into()
    } else {
        cleaned
    }
}

fn unique_asset_rel(root: &Path, stem: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut n = 0u32;
    loop {
        let name = if n == 0 {
            format!("{stem}-{ts}.png")
        } else {
            format!("{stem}-{ts}-{n}.png")
        };
        let rel = PathBuf::from("assets").join(name);
        if !root.join(&rel).exists() {
            return rel;
        }
        n += 1;
        if n > 99 {
            return PathBuf::from("assets").join(format!("{stem}-{ts}-{n}.png"));
        }
    }
}

fn rel_display(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn resolve_image_path(root: &Path, note_rel: Option<&Path>, url: &str) -> Option<PathBuf> {
    let url = url.trim();
    if url.is_empty()
        || url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with("data:")
    {
        return None;
    }
    let mut candidates = Vec::new();
    candidates.push(root.join(url));
    if let Some(note) = note_rel {
        if let Some(parent) = note.parent() {
            candidates.push(root.join(parent).join(url));
        }
    }
    for c in candidates {
        if let Some(inside) = file_inside(root, &c) {
            return Some(inside);
        }
    }
    None
}

fn file_inside(root: &Path, path: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let path = path.canonicalize().ok()?;
    if path.starts_with(&root) && path.is_file() {
        Some(path)
    } else {
        None
    }
}
