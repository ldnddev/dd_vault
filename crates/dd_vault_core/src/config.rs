//! Per-vault `config.toml` (not colors).

use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::paths::Paths;
use crate::vault::Vault;

#[derive(Clone, Debug, Deserialize)]
pub struct VaultConfig {
    #[serde(default = "default_true")]
    pub preview: bool,
    #[serde(default = "default_true")]
    pub wrap: bool,
    #[serde(default = "default_daily")]
    pub daily_note_path: String,
    #[serde(default)]
    pub secret_patterns: Vec<String>,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            preview: true,
            wrap: true,
            daily_note_path: default_daily(),
            secret_patterns: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_daily() -> String {
    "notes/daily".into()
}

impl VaultConfig {
    pub fn load(meta_dir: &Path) -> Self {
        load_toml(&meta_dir.join("config.toml")).unwrap_or_default()
    }

    pub fn load_for(vault: &Vault, paths: Option<&Paths>) -> Self {
        let mut cfg = Self::load(&vault.meta_dir);
        if let Some(p) = paths {
            let global = load_toml(&p.ldnddev_dir().join("config.toml")).unwrap_or_default();
            if cfg.secret_patterns.is_empty() {
                cfg.secret_patterns = global.secret_patterns;
            } else {
                cfg.secret_patterns.extend(global.secret_patterns);
                cfg.secret_patterns.sort();
                cfg.secret_patterns.dedup();
            }
        }
        if cfg.daily_note_path.trim().is_empty() {
            cfg.daily_note_path = default_daily();
        }
        cfg
    }
}

fn load_toml(path: &Path) -> Option<VaultConfig> {
    let text = fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}
