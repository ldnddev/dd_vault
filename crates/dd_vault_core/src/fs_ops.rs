use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::Error;

pub fn validate_entry_name(name: &str) -> Result<(), Error> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err(Error::InvalidEntryName(name.to_string()));
    }
    Ok(())
}

/// Join `rel` onto `root`, rejecting absolute paths and `..`.
pub fn resolve_inside(root: &Path, rel: &Path) -> Result<PathBuf, Error> {
    if rel.is_absolute()
        || rel.components().any(|c| {
            matches!(
                c,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
    {
        return Err(Error::InvalidEntryName(rel.display().to_string()));
    }
    Ok(root.join(rel))
}

pub fn create_file(root: &Path, rel: &Path) -> Result<PathBuf, Error> {
    let dest = resolve_inside(root, rel)?;
    if dest.exists() {
        return Err(Error::AlreadyExists(dest));
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&dest, "")?;
    Ok(dest)
}

pub fn create_dir(root: &Path, rel: &Path) -> Result<PathBuf, Error> {
    let dest = resolve_inside(root, rel)?;
    if dest.exists() {
        return Err(Error::AlreadyExists(dest));
    }
    fs::create_dir_all(&dest)?;
    Ok(dest)
}

pub fn rename_entry(root: &Path, from_rel: &Path, new_name: &str) -> Result<PathBuf, Error> {
    validate_entry_name(new_name)?;
    let from = resolve_inside(root, from_rel)?;
    if !from.exists() {
        return Err(Error::NotFound(from));
    }
    let dest = from.parent().unwrap_or(root).join(new_name);
    if dest.exists() {
        return Err(Error::AlreadyExists(dest));
    }
    fs::rename(&from, &dest)?;
    Ok(dest)
}

pub fn delete_entry(root: &Path, rel: &Path) -> Result<(), Error> {
    let dest = resolve_inside(root, rel)?;
    if !dest.exists() {
        return Err(Error::NotFound(dest));
    }
    let meta = fs::symlink_metadata(&dest)?;
    if meta.is_dir() {
        fs::remove_dir_all(&dest)?;
    } else {
        fs::remove_file(&dest)?;
    }
    Ok(())
}

pub fn rel_from_root(root: &Path, abs: &Path) -> PathBuf {
    abs.strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| abs.to_path_buf())
}
