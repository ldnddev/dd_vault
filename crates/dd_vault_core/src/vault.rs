use std::fs;
use std::path::{Path, PathBuf};

use crate::paths::{is_metadata_dirname, metadata_dir};
use crate::Error;

pub const GITIGNORE: &str = "\
.dd_vault-*/
*.db
*.db-wal
*.db-shm
.dd_vault-cache/
.DS_Store
";

pub const DEFAULT_VAULT_CONFIG: &str = "\
# Per-vault settings. Colors live in dd_vault_theme.yml, not here.
preview = true
wrap = true
daily_note_path = \"notes/daily\"
# Extra pre-commit secret needles (substrings), on top of ghp_ / github_pat_ / AKIA.
# secret_patterns = []
";

/// An opened vault on disk. Files in `root` are the source of truth;
/// `meta_dir` holds derived cache and config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vault {
    pub name: String,
    pub root: PathBuf,
    pub meta_dir: PathBuf,
}

impl Vault {
    pub fn header_label(&self) -> String {
        format!(" vault:{} ", self.name)
    }
}

pub fn vault_name_from_root(root: &Path) -> Result<String, Error> {
    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::InvalidName(root.display().to_string()))?;
    validate_name(name)?;
    Ok(name.to_string())
}

pub fn validate_name(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err(Error::InvalidName(name.to_string()));
    }
    Ok(())
}

/// Create a vault (or ensure an existing one). Does not register it.
pub fn init(path: &Path) -> Result<Vault, Error> {
    if path.exists() && !path.is_dir() {
        return Err(Error::NotDirectory(path.to_path_buf()));
    }
    fs::create_dir_all(path)?;
    let root = fs::canonicalize(path)?;
    let name = vault_name_from_root(&root)?;
    let meta = match find_metadata_dir(&root)? {
        Some(existing) => existing,
        None => metadata_dir(&root, &name),
    };
    fs::create_dir_all(&meta)?;
    ensure_layout(&root, &meta)?;
    Ok(Vault {
        name,
        root,
        meta_dir: meta,
    })
}

/// Open an existing vault. Errors if the folder has no `.dd_vault-*` dir.
pub fn open(path: &Path) -> Result<Vault, Error> {
    if !path.exists() {
        return Err(Error::NotFound(path.to_path_buf()));
    }
    if !path.is_dir() {
        return Err(Error::NotDirectory(path.to_path_buf()));
    }
    let root = fs::canonicalize(path)?;
    let name = vault_name_from_root(&root)?;
    let meta = find_metadata_dir(&root)?.ok_or_else(|| Error::NotAVault(root.clone()))?;
    ensure_layout(&root, &meta)?;
    Ok(Vault {
        name,
        root,
        meta_dir: meta,
    })
}

pub fn find_metadata_dir(root: &Path) -> Result<Option<PathBuf>, Error> {
    let mut found = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let fname = entry.file_name();
        let name = fname.to_string_lossy();
        if is_metadata_dirname(&name) {
            found.push(entry.path());
        }
    }
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        _ => Err(Error::Ambiguous(root.to_path_buf())),
    }
}

pub fn ensure_layout(root: &Path, meta: &Path) -> Result<(), Error> {
    fs::create_dir_all(meta)?;
    for rel in [
        "cache",
        "cache/preview",
        "cache/mermaid",
        "cache/dbml",
        "sync",
        "logs",
    ] {
        fs::create_dir_all(meta.join(rel))?;
    }
    let config = meta.join("config.toml");
    if !config.exists() {
        fs::write(&config, DEFAULT_VAULT_CONFIG)?;
    }
    for rel in ["notes", "notes/daily", "notes/projects", "assets"] {
        fs::create_dir_all(root.join(rel))?;
    }
    let gitignore = root.join(".gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, GITIGNORE)?;
    }
    Ok(())
}
