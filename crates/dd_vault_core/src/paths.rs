use std::path::{Path, PathBuf};

use crate::Error;

/// XDG/home config root (`$XDG_CONFIG_HOME` or `~/.config`), not including `ldnddev/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub config_home: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Self, Error> {
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .ok_or(Error::NoConfigHome)?;
        Ok(Self { config_home })
    }

    pub fn new(config_home: impl Into<PathBuf>) -> Self {
        Self {
            config_home: config_home.into(),
        }
    }

    pub fn ldnddev_dir(&self) -> PathBuf {
        self.config_home.join("ldnddev")
    }

    pub fn vaults_toml(&self) -> PathBuf {
        self.ldnddev_dir().join("vaults.toml")
    }

    /// Git credential-store file (`0600`). Never stored inside a vault.
    pub fn credentials_file(&self) -> PathBuf {
        self.ldnddev_dir().join("credentials")
    }
}

pub fn metadata_dir_name(vault_name: &str) -> String {
    format!(".dd_vault-{vault_name}")
}

pub fn metadata_dir(root: &Path, vault_name: &str) -> PathBuf {
    root.join(metadata_dir_name(vault_name))
}

pub fn is_metadata_dirname(name: &str) -> bool {
    name.starts_with(".dd_vault-") && name.len() > ".dd_vault-".len()
}
