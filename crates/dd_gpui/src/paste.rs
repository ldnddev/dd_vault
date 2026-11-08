//! Clipboard image filenames. The shell reads the clipboard and writes the file.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn note_stem(rel: Option<&str>) -> String {
    let raw = rel
        .and_then(|name| Path::new(name).file_stem())
        .and_then(|stem| stem.to_str())
        .unwrap_or("paste");
    let cleaned: String = raw
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect();
    if cleaned.is_empty() {
        "paste".into()
    } else {
        cleaned
    }
}

pub fn unique_asset_rel(root: &Path, stem: &str) -> PathBuf {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_secs())
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

pub fn image_markdown(rel: &Path) -> String {
    let shown = rel
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    format!("![]({shown})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_keeps_ascii_and_falls_back() {
        assert_eq!(note_stem(Some("notes/My Note!.md")), "MyNote");
        assert_eq!(note_stem(Some("###.md")), "paste");
        assert_eq!(note_stem(None), "paste");
    }

    #[test]
    fn asset_name_and_markdown_link() {
        let dir = std::env::temp_dir().join(format!("dd-gui-paste-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        let first = unique_asset_rel(&dir, "hello");
        assert!(first.starts_with("assets"));
        assert!(image_markdown(&first).starts_with("![](assets/hello-"));
        std::fs::write(dir.join(&first), b"png").unwrap();
        let second = unique_asset_rel(&dir, "hello");
        assert_ne!(first, second);
        assert!(image_markdown(&second).contains("assets/hello-"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
