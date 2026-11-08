//! Open a URL or a file inside the vault with the operating system's default app.
//!
//! The desktop window calls this directly. The terminal app keeps a test hook in
//! `dd_tui` that records the target instead of spawning.

use std::path::{Component, Path};
use std::process::{Command, Stdio};

use crate::is_external_url;

pub fn launch_default_app(target: &str) -> Result<(), String> {
    if target.chars().any(|c| c.is_control()) {
        return Err("refusing to open a target with control characters".into());
    }
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(program)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("could not open with {program}: {err}"))
}

/// Resolve `raw` for the external opener.
///
/// `http`, `https`, and `mailto` return as written, even when no vault is open.
/// Anything else needs `resolved`, a vault-relative file. Absolute paths, `..`,
/// a missing file, and a path that canonicalizes outside `root` are refused.
pub fn external_open_target(
    root: Option<&Path>,
    resolved: Option<&Path>,
    raw: &str,
) -> Result<String, String> {
    let dst = raw
        .split('#')
        .next()
        .unwrap_or(raw)
        .split('|')
        .next()
        .unwrap_or(raw)
        .trim();
    if dst.is_empty() {
        return Err("No link under the caret".into());
    }
    if is_external_url(dst) {
        return Ok(dst.to_string());
    }
    let Some(root) = root else {
        return Err("No vault open".into());
    };
    let Some(rel) = resolved else {
        return Err(format!("No file for {dst}"));
    };
    if rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("That link points outside the vault".into());
    }
    let abs = root.join(rel);
    if !abs.is_file() {
        return Err(format!("{} is not a file", rel.display()));
    }
    let root_canon = root.canonicalize().map_err(|err| err.to_string())?;
    let canon = abs.canonicalize().map_err(|err| err.to_string())?;
    if !canon.starts_with(&root_canon) {
        return Err("That link points outside the vault".into());
    }
    Ok(canon.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_pass_through_and_parent_paths_are_refused() {
        assert_eq!(
            external_open_target(None, None, "https://example.com/docs").unwrap(),
            "https://example.com/docs"
        );
        assert_eq!(
            external_open_target(None, None, "mailto:a@example.com").unwrap(),
            "mailto:a@example.com"
        );
        let err = external_open_target(Some(Path::new("/tmp")), Some(Path::new("../x")), "../x")
            .unwrap_err();
        assert!(err.contains("outside"), "{err}");
        assert!(launch_default_app("bad\nurl").is_err());
    }

    #[test]
    fn a_file_inside_the_vault_resolves_and_a_missing_one_does_not() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("a.md"), "hi").expect("write");
        let opened =
            external_open_target(Some(dir.path()), Some(Path::new("a.md")), "a.md").unwrap();
        assert!(opened.ends_with("a.md"), "{opened}");
        let missing = external_open_target(Some(dir.path()), None, "gone").unwrap_err();
        assert!(missing.contains("No file"), "{missing}");
    }
}
