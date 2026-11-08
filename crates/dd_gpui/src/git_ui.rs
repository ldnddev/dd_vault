//! Git badge, job results, and conflict copy. Calls `dd_vault_core`; does not reimplement git.

use std::path::{Path, PathBuf};

use dd_vault_core::{GitCtx, GitOpResult, GitState};

pub fn notes_title(name: &str, mode: &str, dirty: bool, state: GitState) -> String {
    let dirty = if dirty { " ±" } else { "" };
    match state.title_badge_owned() {
        Some(badge) => format!(" {name} — {mode}{dirty}  {badge} "),
        None => format!(" {name} — {mode}{dirty} "),
    }
}

pub fn busy_word(op: &str) -> &'static str {
    match op {
        "pull" => "pulling",
        "push" => "pushing",
        "commit" => "committing",
        _ => "working",
    }
}

pub fn run_git(
    root: &Path,
    credentials: Option<&Path>,
    extra: &[String],
    name: &str,
    op: fn(&GitCtx<'_>) -> Result<GitOpResult, dd_vault_core::Error>,
) -> Result<(String, GitOpResult), String> {
    let ctx = GitCtx {
        root,
        credentials,
        extra_secret_patterns: extra,
    };
    op(&ctx)
        .map(|result| (name.to_string(), result))
        .map_err(|err| err.to_string())
}

pub fn commit_then_maybe_push(
    root: &Path,
    credentials: Option<&Path>,
    extra: &[String],
    message: &str,
    then_push: bool,
) -> Result<(String, GitOpResult), String> {
    let ctx = GitCtx {
        root,
        credentials,
        extra_secret_patterns: extra,
    };
    let committed = dd_vault_core::commit(&ctx, message).map_err(|err| err.to_string())?;
    if !then_push {
        return Ok(("commit".to_string(), committed));
    }
    match dd_vault_core::push(&ctx) {
        Ok(pushed) => Ok((
            "push".to_string(),
            GitOpResult {
                message: format!("{}; {}", committed.message, pushed.message),
                sidecars: committed.sidecars,
                auto_merged: committed.auto_merged,
            },
        )),
        Err(err) => Err(format!("{}; push failed: {err}", committed.message)),
    }
}

pub fn sidecar_labels(root: &Path, sidecars: &[PathBuf]) -> Vec<String> {
    sidecars
        .iter()
        .map(|path| {
            path.strip_prefix(root)
                .unwrap_or(path)
                .display()
                .to_string()
        })
        .collect()
}

pub fn conflict_notice(message: &str, rels: &[String]) -> String {
    let listed = rels
        .iter()
        .map(|rel| format!("  • {rel}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{message}\n\nOriginal kept. Incoming copies:\n{listed}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dd_vault_core::GitState;

    #[test]
    fn title_badge_matches_the_terminal() {
        assert_eq!(
            notes_title("a.md", "NORMAL", false, GitState::NotRepo),
            " a.md — NORMAL "
        );
        assert_eq!(
            notes_title("a.md", "NORMAL", true, GitState::Clean),
            " a.md — NORMAL ±  git:clean "
        );
        assert_eq!(
            notes_title("a.md", "INSERT", false, GitState::Dirty(3)),
            " a.md — INSERT  git:±3 "
        );
        assert!(notes_title("a.md", "NORMAL", false, GitState::Conflict).contains("git:conflict"));
    }

    #[test]
    fn conflict_notice_lists_sidecars() {
        let text = conflict_notice("merged", &["notes/a.conflict-1.md".into()]);
        assert!(text.contains("Original kept. Incoming copies:"));
        assert!(text.contains("• notes/a.conflict-1.md"));
    }

    #[test]
    fn commit_refuses_a_staged_secret() {
        let dir = std::env::temp_dir().join(format!("dd-gui-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init"]);
        git(&dir, &["config", "user.email", "t@example.com"]);
        git(&dir, &["config", "user.name", "Test"]);
        std::fs::write(dir.join("note.md"), "token sk-example\n").unwrap();
        let err = commit_then_maybe_push(&dir, None, &[], "save", false).unwrap_err();
        assert!(err.contains("commit blocked"), "{err}");
        assert!(err.contains("secret"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?}");
    }
}
