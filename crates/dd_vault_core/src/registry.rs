use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::Paths;
use crate::{Error, Vault};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub last_path: Option<String>,
    #[serde(default)]
    pub vaults: Vec<VaultEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultEntry {
    pub name: String,
    pub path: String,
}

impl Registry {
    pub fn load(paths: &Paths) -> Result<Self, Error> {
        let file = paths.vaults_toml();
        if !file.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&file)?;
        toml::from_str(&raw).map_err(|err| Error::Config(format!("{}: {err}", file.display())))
    }

    pub fn save(&self, paths: &Paths) -> Result<(), Error> {
        let dir = paths.ldnddev_dir();
        fs::create_dir_all(&dir)?;
        let file = paths.vaults_toml();
        let body = toml::to_string_pretty(self)
            .map_err(|err| Error::Config(format!("serialize vaults.toml: {err}")))?;
        let tmp = file.with_extension("toml.tmp");
        fs::write(&tmp, body)?;
        fs::rename(&tmp, &file)?;
        Ok(())
    }

    pub fn last_path(&self) -> Option<PathBuf> {
        self.last_path.as_ref().map(PathBuf::from)
    }

    pub fn register(&mut self, vault: &Vault) {
        let path = vault.root.to_string_lossy().into_owned();
        if let Some(existing) = self.vaults.iter_mut().find(|e| e.path == path) {
            existing.name = vault.name.clone();
        } else {
            self.vaults.push(VaultEntry {
                name: vault.name.clone(),
                path: path.clone(),
            });
        }
        self.last_path = Some(path);
    }

    pub fn lookup(&self, path: &Path) -> Option<&VaultEntry> {
        let s = path.to_string_lossy();
        self.vaults.iter().find(|e| e.path == s)
    }

    /// Drop a vault from the list. Does not delete files on disk.
    pub fn unregister(&mut self, path: &str) -> bool {
        let before = self.vaults.len();
        self.vaults.retain(|e| e.path != path);
        if self.last_path.as_deref() == Some(path) {
            self.last_path = self.vaults.first().map(|e| e.path.clone());
        }
        self.vaults.len() < before
    }
}
