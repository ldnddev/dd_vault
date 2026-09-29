//! Last-open layout for a vault (`session.toml` under the metadata dir).

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Error;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub cursor: usize,
    #[serde(default)]
    pub scroll: usize,
    #[serde(default)]
    pub scroll_off: usize,
    #[serde(default)]
    pub wrap: Option<bool>,
    #[serde(default)]
    pub preview: Option<bool>,
    #[serde(default)]
    pub preview_scroll: u16,
    #[serde(default)]
    pub preview_split: u16,
    #[serde(default)]
    pub tree_split: u16,
    #[serde(default)]
    pub zen: bool,
    #[serde(default)]
    pub focus: bool,
    #[serde(default)]
    pub explore: bool,
    #[serde(default)]
    pub pane: Option<String>,
}

impl Session {
    pub fn path(meta_dir: &Path) -> std::path::PathBuf {
        meta_dir.join("session.toml")
    }

    pub fn load(meta_dir: &Path) -> Option<Self> {
        let raw = fs::read_to_string(Self::path(meta_dir)).ok()?;
        toml::from_str(&raw).ok()
    }

    pub fn save(&self, meta_dir: &Path) -> Result<(), Error> {
        fs::create_dir_all(meta_dir)?;
        let body = toml::to_string_pretty(self)
            .map_err(|err| Error::Config(format!("serialize session.toml: {err}")))?;
        let file = Self::path(meta_dir);
        let tmp = file.with_extension("toml.tmp");
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &file)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_roundtrip() {
        let dir = tempfile::tempdir().expect("tmp");
        let s = Session {
            note: Some("notes/hello.md".into()),
            cursor: 12,
            scroll: 2,
            wrap: Some(true),
            preview: Some(false),
            pane: Some("editor".into()),
            ..Session::default()
        };
        s.save(dir.path()).expect("save");
        let loaded = Session::load(dir.path()).expect("load");
        assert_eq!(loaded.note.as_deref(), Some("notes/hello.md"));
        assert_eq!(loaded.cursor, 12);
        assert_eq!(loaded.scroll, 2);
        assert_eq!(loaded.wrap, Some(true));
        assert_eq!(loaded.preview, Some(false));
        assert_eq!(loaded.pane.as_deref(), Some("editor"));
    }
}
