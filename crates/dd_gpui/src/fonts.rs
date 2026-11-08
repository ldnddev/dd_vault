//! GUI faces stored in the shared ldnddev `config.toml`, not the theme YAML.
//!
//! The editor, tree, and chrome use a monospace family. Preview prose uses a
//! proportional family. Defaults are the two faces the window shipped with.

use std::fs;
use std::path::Path;
use std::process::Command;

use toml::Value;

pub const DEFAULT_MONO: &str = "DejaVu Sans Mono";
pub const DEFAULT_SANS: &str = "DejaVu Sans";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuiFonts {
    pub mono: String,
    pub sans: String,
}

impl GuiFonts {
    pub fn load(config_toml: &Path) -> Self {
        let Ok(text) = fs::read_to_string(config_toml) else {
            return Self::defaults();
        };
        let Ok(root) = text.parse::<Value>() else {
            return Self::defaults();
        };
        Self {
            mono: named(&root, "mono", DEFAULT_MONO),
            sans: named(&root, "sans", DEFAULT_SANS),
        }
    }

    /// Merge `gui.mono` and `gui.sans` into the file. Other tables, including `ai`, stay.
    pub fn save(&self, config_toml: &Path) -> Result<(), String> {
        if let Some(dir) = config_toml.parent() {
            fs::create_dir_all(dir).map_err(|err| err.to_string())?;
        }
        let mut root: Value = fs::read_to_string(config_toml)
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or_else(|| Value::Table(toml::map::Map::new()));
        let table = root
            .as_table_mut()
            .ok_or_else(|| "config.toml is not a table".to_string())?;
        if !matches!(table.get("gui"), Some(Value::Table(_))) {
            table.insert("gui".into(), Value::Table(toml::map::Map::new()));
        }
        let gui = table
            .get_mut("gui")
            .and_then(Value::as_table_mut)
            .ok_or_else(|| "gui is not a table".to_string())?;
        gui.insert("mono".into(), Value::String(self.mono.clone()));
        gui.insert("sans".into(), Value::String(self.sans.clone()));
        let body = toml::to_string_pretty(&root).map_err(|err| err.to_string())?;
        fs::write(config_toml, body).map_err(|err| err.to_string())?;
        Ok(())
    }

    fn defaults() -> Self {
        Self {
            mono: DEFAULT_MONO.to_string(),
            sans: DEFAULT_SANS.to_string(),
        }
    }
}

fn named(root: &Value, key: &str, fallback: &str) -> String {
    root.get("gui")
        .and_then(|gui| gui.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

/// Families reported by fontconfig. One name per face, case-insensitive, sorted.
pub fn installed_families() -> Vec<String> {
    let listed = Command::new("fc-list")
        .args(["-f", "%{family}\n"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned());
    let names = listed.as_deref().map(parse_families).unwrap_or_default();
    if names.is_empty() {
        fallback_families()
    } else {
        names
    }
}

fn fallback_families() -> Vec<String> {
    let mut names = vec![DEFAULT_MONO.to_string(), DEFAULT_SANS.to_string()];
    names.sort();
    names.dedup();
    names
}

fn parse_families(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        for part in line.split(',') {
            let name = part.trim();
            if name.is_empty() {
                continue;
            }
            names.push(name.to_string());
        }
    }
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup_by(|a, b| a.to_lowercase() == b.to_lowercase());
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_families_splits_commas_and_dedupes() {
        let names = parse_families("Noto Sans,Noto Sans Medium\nDejaVu Sans\nnoto sans\n");
        assert_eq!(
            names,
            vec![
                "DejaVu Sans".to_string(),
                "Noto Sans".to_string(),
                "Noto Sans Medium".to_string(),
            ]
        );
    }

    #[test]
    fn missing_config_uses_the_two_defaults() {
        let path = std::env::temp_dir().join(format!(
            "dd-vault-fonts-missing-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_file(&path);
        let loaded = GuiFonts::load(&path);
        assert_eq!(loaded.mono, DEFAULT_MONO);
        assert_eq!(loaded.sans, DEFAULT_SANS);
    }

    #[test]
    fn save_merges_gui_fonts_and_keeps_ai() {
        let dir =
            std::env::temp_dir().join(format!("dd-vault-fonts-{}-{}", std::process::id(), line!()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp");
        let path = dir.join("config.toml");
        fs::write(
            &path,
            "[ai]\nenabled = true\nprovider = \"ollama\"\n\n[gui]\nzoom = 1\n",
        )
        .expect("seed");
        GuiFonts {
            mono: "JetBrains Mono".into(),
            sans: "DejaVu Sans".into(),
        }
        .save(&path)
        .expect("save");
        let text = fs::read_to_string(&path).expect("read");
        assert!(text.contains("ollama"));
        assert!(text.contains("zoom"));
        assert!(text.contains("JetBrains Mono"));
        let loaded = GuiFonts::load(&path);
        assert_eq!(loaded.mono, "JetBrains Mono");
        assert_eq!(loaded.sans, "DejaVu Sans");
        let _ = fs::remove_dir_all(&dir);
    }
}
