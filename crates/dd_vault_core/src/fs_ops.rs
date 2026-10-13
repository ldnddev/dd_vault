use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::Error;

#[cfg(any(target_os = "linux", target_os = "android"))]
const O_NOFOLLOW: i32 = 0x20000;
#[cfg(any(target_os = "macos", target_os = "ios"))]
const O_NOFOLLOW: i32 = 0x0100;
#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))
))]
const O_NOFOLLOW: i32 = 0;

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
///
/// This is a lexical check only. Callers that read or write must use
/// [`confined_path`] so a directory symlink cannot point the result outside the vault.
pub fn resolve_inside(root: &Path, rel: &Path) -> Result<PathBuf, Error> {
    ensure_lexical(rel)?;
    Ok(root.join(rel))
}

/// Absolute path of `rel` inside `root`.
///
/// Intermediate symlinks are resolved and must stay inside `root`. The final
/// component is not followed, so deleting or renaming removes the symlink itself.
pub fn vault_entry(root: &Path, rel: &Path) -> Result<PathBuf, Error> {
    walk(root, rel, false)
}

/// Like [`vault_entry`], but a final symlink is resolved when its target stays inside the vault.
pub fn confined_path(root: &Path, rel: &Path) -> Result<PathBuf, Error> {
    let path = walk(root, rel, true)?;
    if fs::symlink_metadata(&path).is_ok() {
        let root = fs::canonicalize(root)?;
        let canon =
            fs::canonicalize(&path).map_err(|_| Error::OutsideVault(rel.display().to_string()))?;
        if !canon.starts_with(&root) {
            return Err(Error::OutsideVault(rel.display().to_string()));
        }
        return Ok(canon);
    }
    Ok(path)
}

pub fn read_vault_bytes(root: &Path, rel: &Path, max_bytes: u64) -> Result<Vec<u8>, Error> {
    let path = confined_path(root, rel)?;
    read_nofollow(&path, max_bytes)
}

pub fn read_vault_text(root: &Path, rel: &Path, max_bytes: u64) -> Result<String, Error> {
    let path = confined_path(root, rel)?;
    let bytes = read_nofollow(&path, max_bytes)?;
    String::from_utf8(bytes).map_err(|_| Error::Config(format!("not utf-8: {}", path.display())))
}

pub fn write_vault_file(root: &Path, rel: &Path, bytes: &[u8]) -> Result<PathBuf, Error> {
    let path = confined_path(root, rel)?;
    write_nofollow(&path, bytes)?;
    Ok(path)
}

/// Read at most `max_bytes` without following a final symlink.
pub fn read_prefix_nofollow(path: &Path, max_bytes: u64) -> Result<Vec<u8>, Error> {
    read_limited(path, max_bytes, true)
}

pub fn symlink_target_escapes(root: &Path, link_path: &Path) -> Result<bool, Error> {
    let target = fs::read_link(link_path)?;
    let resolved = if target.is_absolute() {
        target
    } else {
        link_path.parent().unwrap_or(root).join(target)
    };
    let root = fs::canonicalize(root)?;
    match fs::canonicalize(&resolved) {
        Ok(canon) => Ok(!canon.starts_with(&root)),
        Err(_) => match normalize_lexical(&resolved) {
            Some(lexical) => Ok(!lexical.starts_with(&root)),
            None => Ok(true),
        },
    }
}

pub fn create_file(root: &Path, rel: &Path) -> Result<PathBuf, Error> {
    let dest = confined_path(root, rel)?;
    if fs::symlink_metadata(&dest).is_ok() {
        return Err(Error::AlreadyExists(dest));
    }
    write_nofollow(&dest, b"")?;
    Ok(dest)
}

pub fn create_dir(root: &Path, rel: &Path) -> Result<PathBuf, Error> {
    let dest = confined_path(root, rel)?;
    if fs::symlink_metadata(&dest).is_ok() {
        return Err(Error::AlreadyExists(dest));
    }
    fs::create_dir_all(&dest)?;
    Ok(dest)
}

pub fn rename_entry(root: &Path, from_rel: &Path, new_name: &str) -> Result<PathBuf, Error> {
    validate_entry_name(new_name)?;
    let from = vault_entry(root, from_rel)?;
    if fs::symlink_metadata(&from).is_err() {
        return Err(Error::NotFound(from));
    }
    let dest = from.parent().unwrap_or(root).join(new_name);
    if fs::symlink_metadata(&dest).is_ok() {
        return Err(Error::AlreadyExists(dest));
    }
    fs::rename(&from, &dest)?;
    Ok(dest)
}

pub fn delete_entry(root: &Path, rel: &Path) -> Result<(), Error> {
    let dest = vault_entry(root, rel)?;
    let meta = fs::symlink_metadata(&dest).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            Error::NotFound(dest.clone())
        } else {
            Error::Io(err)
        }
    })?;
    if meta.is_dir() {
        fs::remove_dir_all(&dest)?;
    } else {
        fs::remove_file(&dest)?;
    }
    Ok(())
}

fn ensure_lexical(rel: &Path) -> Result<(), Error> {
    if rel.as_os_str().is_empty()
        || rel.is_absolute()
        || rel.components().any(|c| {
            matches!(
                c,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
    {
        return Err(Error::InvalidEntryName(rel.display().to_string()));
    }
    let has_normal = rel.components().any(|c| matches!(c, Component::Normal(_)));
    if !has_normal {
        return Err(Error::InvalidEntryName(rel.display().to_string()));
    }
    Ok(())
}

fn normal_parts(rel: &Path) -> Result<Vec<std::ffi::OsString>, Error> {
    ensure_lexical(rel)?;
    let mut out = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(s) => out.push(s.to_os_string()),
            Component::CurDir => {}
            _ => return Err(Error::InvalidEntryName(rel.display().to_string())),
        }
    }
    Ok(out)
}

/// Walk `rel` under the canonical vault root.
///
/// `follow_final` resolves a final symlink when the target is still inside the vault.
fn walk(root: &Path, rel: &Path, follow_final: bool) -> Result<PathBuf, Error> {
    let parts = normal_parts(rel)?;
    let root = fs::canonicalize(root)?;
    let mut current = root.clone();
    for (i, part) in parts.iter().enumerate() {
        let is_final = i + 1 == parts.len();
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if is_final && !follow_final {
                    return Ok(current);
                }
                let target = fs::canonicalize(&current)
                    .map_err(|_| Error::OutsideVault(rel.display().to_string()))?;
                if !target.starts_with(&root) {
                    return Err(Error::OutsideVault(rel.display().to_string()));
                }
                current = target;
            }
            Ok(meta) => {
                if !is_final && !meta.is_dir() {
                    return Err(Error::InvalidEntryName(rel.display().to_string()));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                for rest in &parts[i + 1..] {
                    current.push(rest);
                }
                return Ok(current);
            }
            Err(err) => return Err(err.into()),
        }
    }
    Ok(current)
}

fn normalize_lexical(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

fn read_nofollow(path: &Path, max_bytes: u64) -> Result<Vec<u8>, Error> {
    read_limited(path, max_bytes, false)
}

fn read_limited(path: &Path, max_bytes: u64, prefix_ok: bool) -> Result<Vec<u8>, Error> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(Error::OutsideVault(path.display().to_string()));
    }
    if !prefix_ok && meta.len() > max_bytes {
        return Err(Error::TooLarge(path.to_path_buf()));
    }
    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(O_NOFOLLOW);
    }
    let file = opts.open(path)?;
    let mut buf = Vec::new();
    file.take(max_bytes).read_to_end(&mut buf)?;
    Ok(buf)
}

fn write_nofollow(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(O_NOFOLLOW);
    }
    let mut file = opts.open(path)?;
    file.write_all(bytes)?;
    Ok(())
}

pub fn rel_from_root(root: &Path, abs: &Path) -> PathBuf {
    abs.strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| abs.to_path_buf())
}

/// Path to create for a missing `[[target]]`, relative to the vault root.
pub fn wikilink_new_rel(current: Option<&Path>, dst: &str) -> Result<PathBuf, Error> {
    let dst = dst.trim().trim_start_matches('/');
    let stem = dst.strip_suffix(".md").unwrap_or(dst).trim();
    if stem.is_empty() {
        return Err(Error::InvalidEntryName(dst.to_string()));
    }
    let file = if dst.ends_with(".md") {
        PathBuf::from(dst)
    } else {
        PathBuf::from(format!("{stem}.md"))
    };
    if file.is_absolute()
        || file.components().any(|c| {
            matches!(
                c,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir
            )
        })
    {
        return Err(Error::InvalidEntryName(dst.to_string()));
    }
    if file.components().count() > 1 {
        if file.starts_with("notes") || file.starts_with("assets") {
            Ok(file)
        } else {
            Ok(PathBuf::from("notes").join(file))
        }
    } else {
        let parent = current
            .and_then(|p| p.parent())
            .unwrap_or_else(|| Path::new("notes"));
        Ok(parent.join(file))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Error;

    #[test]
    fn wikilink_new_rel_sibling_nested_and_rejects() {
        let cur = Path::new("notes/hello.md");
        assert_eq!(
            wikilink_new_rel(Some(cur), "World").unwrap(),
            PathBuf::from("notes/World.md")
        );
        assert_eq!(
            wikilink_new_rel(Some(cur), "projects/Thing").unwrap(),
            PathBuf::from("notes/projects/Thing.md")
        );
        assert_eq!(
            wikilink_new_rel(None, "Inbox").unwrap(),
            PathBuf::from("notes/Inbox.md")
        );
        assert_eq!(
            wikilink_new_rel(Some(cur), "assets/pic.md").unwrap(),
            PathBuf::from("assets/pic.md")
        );
        assert!(wikilink_new_rel(Some(cur), "../escape").is_err());
        assert!(wikilink_new_rel(Some(cur), "").is_err());
    }

    #[test]
    fn confined_path_rejects_parent_absolute_and_symlink_escape() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().join("vault");
        fs::create_dir_all(root.join("notes")).unwrap();
        fs::write(root.join("notes/inside.md"), "inside\n").unwrap();
        let outside = dir.path().join("outside.md");
        fs::write(&outside, "secret\n").unwrap();

        assert!(matches!(
            confined_path(&root, Path::new("../outside.md")),
            Err(Error::InvalidEntryName(_))
        ));
        assert!(matches!(
            confined_path(&root, Path::new("/etc/passwd")),
            Err(Error::InvalidEntryName(_))
        ));

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("notes/leak.md")).unwrap();
            std::os::unix::fs::symlink(dir.path(), root.join("out")).unwrap();
            assert!(matches!(
                confined_path(&root, Path::new("notes/leak.md")),
                Err(Error::OutsideVault(_))
            ));
            assert!(matches!(
                write_vault_file(&root, Path::new("out/pwned.md"), b"x"),
                Err(Error::OutsideVault(_))
            ));
            assert!(!dir.path().join("pwned.md").exists());
            assert!(matches!(
                read_vault_text(&root, Path::new("notes/leak.md"), 1000),
                Err(Error::OutsideVault(_))
            ));

            std::os::unix::fs::symlink(root.join("notes/inside.md"), root.join("notes/alias.md"))
                .unwrap();
            let text = read_vault_text(&root, Path::new("notes/alias.md"), 1000).unwrap();
            assert_eq!(text, "inside\n");
            write_vault_file(&root, Path::new("notes/alias.md"), b"updated\n").unwrap();
            assert_eq!(
                fs::read_to_string(root.join("notes/inside.md")).unwrap(),
                "updated\n"
            );
        }
    }
}
