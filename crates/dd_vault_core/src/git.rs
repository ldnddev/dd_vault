//! Git via the `git` CLI: status, pull/push/commit, secret scan, 3-tier conflicts.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::VaultConfig;
use crate::fs_ops::{
    read_prefix_nofollow, symlink_target_escapes, vault_entry, write_vault_nofollow,
};
use crate::paths::{is_metadata_dirname, Paths};
use crate::skip::{skip_dir_name, skip_file_name};
use crate::vault::{Vault, GITIGNORE};
use crate::Error;

/// Built-in pre-commit needles. Extra patterns come from config.
pub const BUILTIN_SECRET_NEEDLES: &[&str] =
    &["ghp_", "github_pat_", "AKIA", "sk-", "xai-", "-----BEGIN "];

const SCAN_CAP: usize = 5 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitState {
    NotRepo,
    Clean,
    Dirty(usize),
    Conflict,
}

impl GitState {
    pub fn title_badge_owned(self) -> Option<String> {
        match self {
            Self::NotRepo => None,
            Self::Clean => Some("git:clean".into()),
            Self::Dirty(n) => Some(format!("git:±{n}")),
            Self::Conflict => Some("git:conflict".into()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitStatus {
    pub state: GitState,
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
}

impl Default for GitStatus {
    fn default() -> Self {
        Self {
            state: GitState::NotRepo,
            branch: None,
            upstream: None,
            ahead: 0,
            behind: 0,
        }
    }
}

impl GitStatus {
    pub fn detail(&self) -> String {
        let ab = match (self.ahead, self.behind) {
            (0, 0) => String::new(),
            (a, b) => format!(" ↑{a} ↓{b}"),
        };
        let up = self
            .upstream
            .as_deref()
            .map(|u| format!(" -> {u}"))
            .unwrap_or_default();
        match self.state {
            GitState::NotRepo => "Not a git repository".into(),
            GitState::Clean => {
                let b = self.branch.as_deref().unwrap_or("HEAD");
                format!("{b}{up}{ab} git:clean")
            }
            GitState::Dirty(n) => {
                let b = self.branch.as_deref().unwrap_or("HEAD");
                format!("{b}{up}{ab} git:±{n}")
            }
            GitState::Conflict => {
                let b = self.branch.as_deref().unwrap_or("HEAD");
                format!("{b}{up}{ab} git:conflict")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitOpResult {
    pub message: String,
    pub sidecars: Vec<PathBuf>,
    pub auto_merged: Vec<PathBuf>,
}

pub struct GitCtx<'a> {
    pub root: &'a Path,
    pub credentials: Option<&'a Path>,
    pub extra_secret_patterns: &'a [String],
}

/// Use the credential-store file only when it exists and is mode `0600`.
pub fn usable_credentials(path: &Path) -> Result<Option<PathBuf>, Error> {
    if !path.is_file() {
        return Ok(None);
    }
    if path
        .to_string_lossy()
        .chars()
        .any(|c| c == '\n' || c == '\r' || c == '\0')
    {
        return Err(Error::Git(
            "credentials path contains a newline; refusing to pass it to git".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path)?.permissions().mode() & 0o777;
        if mode != 0o600 {
            return Err(Error::Git(format!(
                "credentials file {} must be mode 0600 (found {mode:04o}); ignoring",
                path.display()
            )));
        }
    }
    Ok(Some(path.to_path_buf()))
}

pub fn load_secret_patterns(vault: &Vault, paths: Option<&Paths>) -> Vec<String> {
    VaultConfig::load_for(vault, paths)
        .secret_patterns
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn status(root: &Path) -> Result<GitStatus, Error> {
    if !root.join(".git").exists() {
        return Ok(GitStatus::default());
    }
    let out = run_git(root, &["status", "--porcelain=v2", "--branch"], None)?;
    if !out.status.success() {
        let err = stderr_lossy(&out);
        if err.contains("not a git repository") {
            return Ok(GitStatus::default());
        }
        return Err(Error::Git(err_or(&out, "status failed")));
    }
    Ok(parse_porcelain_v2(&stdout_lossy(&out)))
}

pub fn commit(ctx: &GitCtx<'_>, message: &str) -> Result<GitOpResult, Error> {
    let message = message.trim();
    if message.is_empty() {
        return Err(Error::Git("commit message is empty".into()));
    }
    ensure_repo(ctx.root)?;
    require_identity(ctx.root)?;
    let st = status(ctx.root)?;
    if matches!(st.state, GitState::Conflict) {
        return Err(Error::Git(
            "unmerged files; pull/resolve conflicts before commit".into(),
        ));
    }
    ensure_meta_gitignore(ctx.root)?;
    run_git_ok(ctx.root, &["add", "-A"], ctx.credentials)?;
    unstage_skipped(ctx.root, ctx.credentials);
    let hits = scan_staged(ctx.root, ctx.extra_secret_patterns)?;
    if !hits.is_empty() {
        let _ = run_git(ctx.root, &["reset"], ctx.credentials);
        return Err(Error::Git(format!(
            "commit blocked: possible secret in {}",
            hits.join(", ")
        )));
    }
    let staged = run_git(ctx.root, &["diff", "--cached", "--quiet"], ctx.credentials)?;
    if staged.status.success() {
        return Ok(GitOpResult {
            message: "Nothing to commit".into(),
            sidecars: Vec::new(),
            auto_merged: Vec::new(),
        });
    }
    // --no-gpg-sign: pinentry cannot run inside the TUI.
    run_git_ok(
        ctx.root,
        &["commit", "--no-gpg-sign", "-m", message],
        ctx.credentials,
    )?;
    Ok(GitOpResult {
        message: format!("Committed: {message}"),
        sidecars: Vec::new(),
        auto_merged: Vec::new(),
    })
}

pub fn push(ctx: &GitCtx<'_>) -> Result<GitOpResult, Error> {
    ensure_repo(ctx.root)?;
    let st = status(ctx.root)?;
    if matches!(st.state, GitState::Conflict) {
        return Err(Error::Git(
            "unmerged files; pull/resolve before push".into(),
        ));
    }
    push_now(ctx)
}

fn push_now(ctx: &GitCtx<'_>) -> Result<GitOpResult, Error> {
    let out = run_git(ctx.root, &["push"], ctx.credentials)?;
    if out.status.success() {
        return Ok(GitOpResult {
            message: push_message(&out),
            sidecars: Vec::new(),
            auto_merged: Vec::new(),
        });
    }
    let err = err_or(&out, "push failed");
    if looks_like_no_upstream(&err) {
        let remotes = run_git_ok(ctx.root, &["remote"], ctx.credentials).unwrap_or_default();
        if remotes.lines().any(|r| r.trim() == "origin") {
            let out2 = run_git(ctx.root, &["push", "-u", "origin", "HEAD"], ctx.credentials)?;
            if out2.status.success() {
                return Ok(GitOpResult {
                    message: "Pushed (upstream set to origin)".into(),
                    sidecars: Vec::new(),
                    auto_merged: Vec::new(),
                });
            }
            return Err(Error::Git(err_or(&out2, "push failed")));
        }
        return Err(Error::Git(
            "no upstream branch (git remote add origin <url>, then :git push)".into(),
        ));
    }
    Err(Error::Git(err))
}

fn push_message(out: &std::process::Output) -> String {
    let err = stderr_lossy(out);
    let out_s = stdout_lossy(out);
    first_line(&err)
        .or_else(|| first_line(&out_s))
        .unwrap_or("Pushed")
        .to_string()
}

fn looks_like_no_upstream(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("no upstream") || e.contains("has no upstream branch")
}

fn require_identity(root: &Path) -> Result<(), Error> {
    let name = git_config_get(root, "user.name");
    let email = git_config_get(root, "user.email");
    if name.is_none() || email.is_none() {
        return Err(Error::Git(
            "git user.name and user.email are not set. Run: git config --global user.name \"Your Name\" && git config --global user.email you@example.com".into(),
        ));
    }
    Ok(())
}

fn git_config_get(root: &Path, key: &str) -> Option<String> {
    let out = run_git(root, &["config", "--get", key], None).ok()?;
    if !out.status.success() {
        return None;
    }
    let s = stdout_lossy(&out);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub fn pull(ctx: &GitCtx<'_>) -> Result<GitOpResult, Error> {
    ensure_repo(ctx.root)?;
    let st = status(ctx.root)?;
    if matches!(st.state, GitState::Conflict) {
        return finish_conflicts(ctx);
    }
    let out = run_git(
        ctx.root,
        &["pull", "--no-rebase", "--no-edit"],
        ctx.credentials,
    )?;
    if out.status.success() {
        let text = stdout_lossy(&out);
        let message = if text.is_empty() {
            "Pulled".into()
        } else {
            first_line(&text).unwrap_or("Pulled").to_string()
        };
        return Ok(GitOpResult {
            message,
            sidecars: Vec::new(),
            auto_merged: Vec::new(),
        });
    }
    let after = status(ctx.root)?;
    if matches!(after.state, GitState::Conflict) {
        return finish_conflicts(ctx);
    }
    Err(Error::Git(err_or(&out, "pull failed")))
}

fn finish_conflicts(ctx: &GitCtx<'_>) -> Result<GitOpResult, Error> {
    let (sidecars, auto_merged) = resolve_unmerged(ctx.root)?;
    let unmerged = unmerged_paths(ctx.root)?;
    if !unmerged.is_empty() {
        return Err(Error::Git(format!(
            "could not resolve: {}",
            unmerged.join(", ")
        )));
    }
    let hits = scan_staged(ctx.root, ctx.extra_secret_patterns)?;
    if !hits.is_empty() {
        return Err(Error::Git(format!(
            "merge not committed: possible secret in {}",
            hits.join(", ")
        )));
    }
    let commit_out = run_git(
        ctx.root,
        &["commit", "--no-edit", "--no-gpg-sign"],
        ctx.credentials,
    )?;
    if !commit_out.status.success() {
        let err = stderr_lossy(&commit_out);
        if err.contains("nothing to commit") {
            return Ok(conflict_result("Resolved conflicts", sidecars, auto_merged));
        }
        return Err(Error::Git(err_or(&commit_out, "merge commit failed")));
    }
    let n = sidecars.len();
    let message = if n == 0 {
        "Pulled (conflicts auto-merged)".into()
    } else {
        format!("Pulled; {n} overlapping conflict(s) kept as sidecars")
    };
    Ok(conflict_result(message, sidecars, auto_merged))
}

fn conflict_result(
    message: impl Into<String>,
    sidecars: Vec<PathBuf>,
    auto_merged: Vec<PathBuf>,
) -> GitOpResult {
    GitOpResult {
        message: message.into(),
        sidecars,
        auto_merged,
    }
}

/// 3-tier: non-overlapping `git merge-file`; overlapping / no-base → sidecar;
/// binary always sidecar. Never last-write-wins.
fn resolve_unmerged(root: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>), Error> {
    let paths = unmerged_paths(root)?;
    let mut sidecars = Vec::new();
    let mut auto_merged = Vec::new();
    let ts = unix_ts();
    for rel in paths {
        let rel_path = Path::new(&rel);
        let abs = conflict_entry(root, rel_path)?;
        let ours = show_stage(root, 2, &rel);
        let theirs = show_stage(root, 3, &rel);
        let base = show_stage(root, 1, &rel);
        match (ours.as_deref(), theirs.as_deref()) {
            (None, None) => {}
            (None, Some(theirs_bytes)) => {
                let dest = write_conflict_sidecar(root, rel_path, theirs_bytes, ts)?;
                if fs::symlink_metadata(&abs).is_ok() {
                    fs::remove_file(&abs)?;
                }
                run_git_ok(root, &["rm", "--", &rel], None)?;
                sidecars.push(dest);
            }
            (Some(ours_bytes), None) => {
                write_conflict_bytes(root, rel_path, ours_bytes)?;
                run_git_ok(root, &["add", "--", &rel], None)?;
            }
            (Some(ours_bytes), Some(theirs_bytes)) => {
                let binary = is_binary(ours_bytes) || is_binary(theirs_bytes);
                if binary {
                    write_conflict_bytes(root, rel_path, ours_bytes)?;
                    let dest = write_conflict_sidecar(root, rel_path, theirs_bytes, ts)?;
                    run_git_ok(root, &["add", "--", &rel], None)?;
                    sidecars.push(dest);
                    continue;
                }
                if let Some(base_bytes) = base.as_deref() {
                    if let Some(merged) = try_merge_file(ours_bytes, base_bytes, theirs_bytes) {
                        write_conflict_bytes(root, rel_path, merged.as_bytes())?;
                        run_git_ok(root, &["add", "--", &rel], None)?;
                        auto_merged.push(PathBuf::from(&rel));
                        continue;
                    }
                }
                write_conflict_bytes(root, rel_path, ours_bytes)?;
                let dest = write_conflict_sidecar(root, rel_path, theirs_bytes, ts)?;
                run_git_ok(root, &["add", "--", &rel], None)?;
                sidecars.push(dest);
            }
        }
    }
    Ok((sidecars, auto_merged))
}

fn try_merge_file(ours: &[u8], base: &[u8], theirs: &[u8]) -> Option<String> {
    let dir = tempfile::Builder::new()
        .prefix("dd_vault-merge-")
        .tempdir()
        .ok()?;
    let ours_p = dir.path().join("ours");
    let base_p = dir.path().join("base");
    let theirs_p = dir.path().join("theirs");
    write_private(&ours_p, ours)?;
    write_private(&base_p, base)?;
    write_private(&theirs_p, theirs)?;
    let ours_s = ours_p.to_string_lossy().into_owned();
    let base_s = base_p.to_string_lossy().into_owned();
    let theirs_s = theirs_p.to_string_lossy().into_owned();
    let out = run_git(
        dir.path(),
        &["merge-file", "-p", &ours_s, &base_s, &theirs_s],
        None,
    )
    .ok()?;
    if out.status.success() {
        String::from_utf8(out.stdout).ok()
    } else {
        None
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Option<()> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path).ok()?;
    file.write_all(bytes).ok()?;
    Some(())
}

fn conflict_entry(root: &Path, rel: &Path) -> Result<PathBuf, Error> {
    let path = vault_entry(root, rel)
        .map_err(|err| Error::Git(format!("{} is outside the vault ({err})", rel.display())))?;
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            return Err(Error::Git(format!(
                "refusing to write conflict through symlink {}",
                rel.display()
            )));
        }
    }
    Ok(path)
}

fn write_conflict_bytes(root: &Path, rel: &Path, bytes: &[u8]) -> Result<(), Error> {
    write_vault_nofollow(root, rel, bytes).map(|_| ())
}

fn write_conflict_sidecar(
    root: &Path,
    rel: &Path,
    theirs: &[u8],
    ts: u64,
) -> Result<PathBuf, Error> {
    let parent = rel.parent().unwrap_or_else(|| Path::new(""));
    let stem = rel
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "note".into());
    let ext = rel
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 0..1000u32 {
        let name = if n == 0 {
            format!("{stem}.conflict-{ts}{ext}")
        } else {
            format!("{stem}.conflict-{ts}-{n}{ext}")
        };
        crate::fs_ops::validate_entry_name(&name)
            .map_err(|err| Error::Git(format!("refusing sidecar name {name}: {err}")))?;
        let dest_rel = if parent.as_os_str().is_empty() {
            PathBuf::from(&name)
        } else {
            parent.join(&name)
        };
        let dest = vault_entry(root, &dest_rel).map_err(|err| {
            Error::Git(format!(
                "sidecar {} is outside the vault ({err})",
                dest_rel.display()
            ))
        })?;
        match fs::symlink_metadata(&dest) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(Error::Git(format!(
                    "refusing to write sidecar through symlink {}",
                    dest_rel.display()
                )));
            }
            Ok(_) => continue,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                write_vault_nofollow(root, &dest_rel, theirs)?;
                return Ok(dest);
            }
            Err(err) => return Err(err.into()),
        }
    }
    Err(Error::Git(format!(
        "could not pick a sidecar name for {}",
        rel.display()
    )))
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

fn show_stage(root: &Path, stage: u8, rel: &str) -> Option<Vec<u8>> {
    let spec = format!(":{stage}:{rel}");
    let out = run_git(root, &["show", &spec], None).ok()?;
    if out.status.success() {
        Some(out.stdout)
    } else {
        None
    }
}

fn unmerged_paths(root: &Path) -> Result<Vec<String>, Error> {
    let out = run_git(
        root,
        &["diff", "--name-only", "-z", "--diff-filter=U"],
        None,
    )?;
    if !out.status.success() {
        return Err(Error::Git(err_or(&out, "list unmerged failed")));
    }
    Ok(nul_paths(&out.stdout))
}

fn skip_git_rel(rel: &str) -> bool {
    rel.split(['/', '\\'])
        .any(|part| skip_dir_name(part) || skip_file_name(part) || is_metadata_dirname(part))
}

fn ensure_meta_gitignore(root: &Path) -> Result<(), Error> {
    let rel = Path::new(".gitignore");
    let path = vault_entry(root, rel)?;
    let existing = match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(Error::Git(
                ".gitignore is a symlink; refusing to update it".into(),
            ));
        }
        Ok(meta) if meta.is_file() => {
            if meta.len() > 1_048_576 {
                return Ok(());
            }
            if meta.len() == 0 {
                String::new()
            } else {
                let bytes = read_prefix_nofollow(&path, meta.len())?;
                match String::from_utf8(bytes) {
                    Ok(text) => text,
                    Err(_) => return Ok(()),
                }
            }
        }
        Ok(_) => String::new(),
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err.into()),
    };
    if existing.lines().any(|l| l.trim() == ".dd_vault-*/") {
        return Ok(());
    }
    let mut body = existing;
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(GITIGNORE);
    write_vault_nofollow(root, rel, body.as_bytes())?;
    Ok(())
}

fn unstage_skipped(root: &Path, credentials: Option<&Path>) {
    let _ = run_git(
        root,
        &[
            "rm",
            "-r",
            "--cached",
            "--ignore-unmatch",
            "-q",
            "--",
            ".dd_vault-*",
        ],
        credentials,
    );
    let _ = run_git(
        root,
        &[
            "reset",
            "-q",
            "--",
            ".dd_vault-*",
            "*.db",
            "*.db-wal",
            "*.db-shm",
            ".DS_Store",
        ],
        credentials,
    );
}

fn scan_staged(root: &Path, extra: &[String]) -> Result<Vec<String>, Error> {
    let out = run_git(
        root,
        &[
            "diff",
            "--cached",
            "--name-only",
            "-z",
            "--diff-filter=ACMR",
        ],
        None,
    )?;
    if !out.status.success() {
        return Err(Error::Git(err_or(&out, "list staged failed")));
    }
    let mut needles: Vec<&str> = BUILTIN_SECRET_NEEDLES.to_vec();
    for p in extra {
        needles.push(p.as_str());
    }
    let mut hits = Vec::new();
    for rel in nul_paths(&out.stdout) {
        if skip_git_rel(&rel) {
            continue;
        }
        let entry = match vault_entry(root, Path::new(&rel)) {
            Ok(path) => path,
            Err(_) => {
                hits.push(format!("{rel} (path escapes vault)"));
                continue;
            }
        };
        let meta = match fs::symlink_metadata(&entry) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            match symlink_target_escapes(root, &entry) {
                Ok(true) | Err(_) => hits.push(format!("{rel} (symlink escapes vault)")),
                Ok(false) => {}
            }
            continue;
        }
        let bytes = match read_prefix_nofollow(&entry, SCAN_CAP as u64) {
            Ok(bytes) => bytes,
            Err(Error::OutsideVault(_)) => {
                hits.push(format!("{rel} (path escapes vault)"));
                continue;
            }
            Err(_) => continue,
        };
        if is_binary(&bytes) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        if let Some(needle) = needles.iter().find(|n| text.contains(**n)) {
            hits.push(format!("{rel} ({needle})"));
        }
    }
    Ok(hits)
}

fn nul_paths(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect()
}

fn parse_porcelain_v2(text: &str) -> GitStatus {
    let mut branch = None;
    let mut upstream = None;
    let mut ahead = 0i64;
    let mut behind = 0i64;
    let mut dirty = 0usize;
    let mut conflict = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            let h = rest.trim();
            if h != "(detached)" {
                branch = Some(h.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            upstream = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            parse_ab(rest, &mut ahead, &mut behind);
        } else if line.starts_with('u') {
            conflict = true;
            dirty += 1;
        } else if line.starts_with('1') || line.starts_with('2') || line.starts_with('?') {
            dirty += 1;
        }
    }
    let state = if conflict {
        GitState::Conflict
    } else if dirty == 0 {
        GitState::Clean
    } else {
        GitState::Dirty(dirty)
    };
    GitStatus {
        state,
        branch,
        upstream,
        ahead,
        behind,
    }
}

fn parse_ab(rest: &str, ahead: &mut i64, behind: &mut i64) {
    for tok in rest.split_whitespace() {
        if let Some(n) = tok.strip_prefix('+') {
            *ahead = n.parse().unwrap_or(0);
        } else if let Some(n) = tok.strip_prefix('-') {
            *behind = n.parse().unwrap_or(0);
        }
    }
}

fn ensure_repo(root: &Path) -> Result<(), Error> {
    match status(root)?.state {
        GitState::NotRepo => Err(Error::Git(
            "not a git repository (run git init in the vault to enable sync)".into(),
        )),
        _ => Ok(()),
    }
}

fn run_git(root: &Path, args: &[&str], credentials: Option<&Path>) -> Result<Output, Error> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root);
    for arg in harden_args(root, credentials)? {
        cmd.arg(arg);
    }
    cmd.args(args);
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("GIT_MERGE_AUTOEDIT", "no");
    cmd.env("GIT_EDITOR", "true");
    cmd.env("GIT_SEQUENCE_EDITOR", "true");
    cmd.env("GIT_PAGER", "cat");
    cmd.env("GIT_ASKPASS", "true");
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    cmd.stdin(Stdio::null());
    match cmd.output() {
        Ok(o) => Ok(o),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            Err(Error::Git("git not found on PATH".into()))
        }
        Err(err) => Err(err.into()),
    }
}

/// Command-line config that overrides repo-local execution hooks.
///
/// A vault's `.git/config` is attacker-controlled when the folder is shared.
/// `git status` runs `core.fsmonitor`, `git add` runs clean filters, `git pull`
/// runs merge drivers and can honor `ext::` remotes, and `git push` runs every
/// `credential.helper`. Those are cleared or replaced here. Repo aliases are
/// cleared because older Git runs an alias that hides a real command. `url.*.insteadOf`
/// and a custom `uploadpack` / `receivepack` cannot be overridden reliably, so
/// those configs refuse the git invocation.
fn harden_args(root: &Path, credentials: Option<&Path>) -> Result<Vec<String>, Error> {
    let hooks = empty_hooks_dir()?;
    let mut args = vec![
        "-c".into(),
        "core.fsmonitor=".into(),
        "-c".into(),
        "core.fsmonitorHookVersion=".into(),
        "-c".into(),
        format!("core.hooksPath={}", hooks.display()),
        "-c".into(),
        "protocol.ext.allow=never".into(),
        "-c".into(),
        "diff.external=".into(),
        "-c".into(),
        "core.gitProxy=".into(),
        "-c".into(),
        "core.pager=cat".into(),
        "-c".into(),
        "core.askPass=true".into(),
        "-c".into(),
        "submodule.recurse=false".into(),
    ];
    args.extend(neutralize_repo_commands(root)?);
    args.push("-c".into());
    args.push("credential.helper=".into());
    if let Some(path) = credentials {
        args.push("-c".into());
        args.push(format!(
            "credential.helper=store --file={}",
            quote_helper_path(&path.display().to_string())
        ));
    }
    Ok(args)
}

fn quote_helper_path(path: &str) -> String {
    let mut out = String::from("\"");
    for c in path.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn empty_hooks_dir() -> Result<PathBuf, Error> {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    if let Some(path) = DIR.get() {
        return Ok(path.clone());
    }
    let created = tempfile::Builder::new()
        .prefix("dd_vault-hooks-")
        .tempdir()
        .map_err(|err| Error::Git(format!("cannot create empty hooks dir: {err}")))?;
    let path = created.path().to_path_buf();
    std::mem::forget(created);
    let _ = DIR.set(path.clone());
    Ok(path)
}

struct GitKey {
    top: String,
    sub: Option<String>,
    leaf: String,
    value: String,
}

impl GitKey {
    fn full(&self) -> String {
        match &self.sub {
            Some(sub) => format!("{}.{}.{}", self.top, sub, self.leaf),
            None => format!("{}.{}", self.top, self.leaf),
        }
    }

    fn safe_override(&self) -> Result<Option<String>, Error> {
        if self.top == "alias" {
            let full = self.full();
            if !safe_config_key(&full) {
                return Err(Error::Git(format!(
                    "refusing to run git: unsafe alias name {full:?}"
                )));
            }
            return Ok(Some(format!("{full}=")));
        }
        if self.top == "url" && matches!(self.leaf.as_str(), "insteadof" | "pushinsteadof") {
            return Err(Error::Git(
                "refusing to run git: repo config rewrites remotes with url.*.insteadOf".into(),
            ));
        }
        if self.top == "remote" && matches!(self.leaf.as_str(), "uploadpack" | "receivepack") {
            if is_default_transport_program(&self.leaf, &self.value) {
                return Ok(None);
            }
            return Err(Error::Git(
                "refusing to run git: repo config sets a custom upload-pack or receive-pack program"
                    .into(),
            ));
        }
        if !is_command_key(&self.top, &self.leaf) {
            return Ok(None);
        }
        let full = self.full();
        if !safe_config_key(&full) {
            return Err(Error::Git(format!(
                "refusing to run git: unsafe config key {full:?}"
            )));
        }
        if self.top == "credential" {
            return Ok(None);
        }
        let value = if full == "http.sslverify" {
            if is_git_false(&self.value) {
                format!("{full}=true")
            } else {
                return Ok(None);
            }
        } else if self.leaf == "required" {
            format!("{full}=false")
        } else if full == "core.pager" {
            format!("{full}=cat")
        } else {
            format!("{full}=")
        };
        Ok(Some(value))
    }
}

fn is_command_key(top: &str, leaf: &str) -> bool {
    matches!(
        (top, leaf),
        ("filter", "clean" | "smudge" | "process" | "required")
            | ("diff", "textconv" | "command")
            | ("merge", "driver")
            | ("core", "pager" | "askpass")
            | ("http", "proxy" | "sslverify")
            | ("remote", "proxy")
            | ("gpg", "program")
    )
}

fn is_default_transport_program(leaf: &str, value: &str) -> bool {
    let value = value.trim();
    match leaf {
        "uploadpack" => matches!(value, "git-upload-pack" | "git upload-pack"),
        "receivepack" => matches!(value, "git-receive-pack" | "git receive-pack"),
        _ => false,
    }
}

fn safe_config_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 200
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn is_git_false(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "false" | "no" | "0" | "off"
    )
}

fn neutralize_repo_commands(root: &Path) -> Result<Vec<String>, Error> {
    let Some(config) = repo_config_path(root) else {
        return Ok(Vec::new());
    };
    let mut keys = Vec::new();
    let mut seen = HashSet::new();
    collect_git_keys(&config, 0, &mut seen, &mut keys)?;
    let mut args = Vec::new();
    for key in keys {
        if let Some(value) = key.safe_override()? {
            args.push("-c".into());
            args.push(value);
        }
    }
    Ok(args)
}

fn repo_config_path(root: &Path) -> Option<PathBuf> {
    let git = root.join(".git");
    if git.is_dir() {
        return Some(git.join("config"));
    }
    if !git.is_file() {
        return None;
    }
    let text = fs::read_to_string(&git).ok()?;
    let rest = text.lines().next()?.trim().strip_prefix("gitdir:")?.trim();
    let dir = PathBuf::from(rest);
    let dir = if dir.is_absolute() {
        dir
    } else {
        root.join(dir)
    };
    Some(dir.join("config"))
}

fn collect_git_keys(
    path: &Path,
    depth: usize,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<GitKey>,
) -> Result<(), Error> {
    if depth > 6 || seen.len() > 32 {
        return Err(Error::Git(
            "refusing to run git: config include tree is too deep".into(),
        ));
    }
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(Error::Git(format!(
                "refusing to run git: cannot read {}: {err}",
                path.display()
            )))
        }
    };
    if !meta.is_file() || meta.len() > 1_048_576 {
        return Err(Error::Git(format!(
            "refusing to run git: config {} is not a small file",
            path.display()
        )));
    }
    let canon = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(canon.clone()) {
        return Ok(());
    }
    let text = fs::read_to_string(&canon).map_err(|err| {
        Error::Git(format!(
            "refusing to run git: cannot read {}: {err}",
            canon.display()
        ))
    })?;
    let mut section_top = String::new();
    let mut section_sub: Option<String> = None;
    let mut includes = Vec::new();
    for line in logical_config_lines(&text) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[') {
            let inner = inner.trim_end_matches(']').trim();
            let (top, sub) = split_section(inner);
            section_top = top;
            section_sub = sub;
            continue;
        }
        let Some((leaf, value)) = line.split_once('=') else {
            continue;
        };
        let leaf = leaf.trim().to_ascii_lowercase();
        let value = unquote_git(value.trim());
        if (section_top == "include" || section_top == "includeif") && leaf == "path" {
            includes.push(value);
            continue;
        }
        if section_top.is_empty() {
            continue;
        }
        out.push(GitKey {
            top: section_top.clone(),
            sub: section_sub.clone(),
            leaf,
            value,
        });
    }
    for inc in includes {
        let next = resolve_include(&canon, &inc);
        collect_git_keys(&next, depth + 1, seen, out)?;
    }
    Ok(())
}

fn logical_config_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if let Some(stripped) = line.strip_suffix('\\') {
            cur.push_str(stripped);
            cur.push(' ');
        } else {
            cur.push_str(line);
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn split_section(inner: &str) -> (String, Option<String>) {
    let mut parts = inner.splitn(2, char::is_whitespace);
    let top = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    let sub = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(unquote_git);
    if top.is_empty() {
        (String::new(), None)
    } else if sub.as_deref().is_some_and(|s| s.is_empty()) {
        (top, None)
    } else {
        (top, sub)
    }
}

fn unquote_git(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        let inner = &value[1..value.len() - 1];
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        value.to_string()
    }
}

fn resolve_include(from_file: &Path, spec: &str) -> PathBuf {
    if let Some(rest) = spec.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    let spec = PathBuf::from(spec);
    if spec.is_absolute() {
        spec
    } else {
        from_file
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(spec)
    }
}

fn run_git_ok(root: &Path, args: &[&str], credentials: Option<&Path>) -> Result<String, Error> {
    let out = run_git(root, args, credentials)?;
    if out.status.success() {
        Ok(stdout_lossy(&out))
    } else {
        Err(Error::Git(err_or(&out, &format!("git {} failed", args[0]))))
    }
}

fn stdout_lossy(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn stderr_lossy(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).trim().to_string()
}

fn err_or(out: &Output, fallback: &str) -> String {
    let err = stderr_lossy(out);
    let out_s = stdout_lossy(out);
    if !err.is_empty() {
        err
    } else if !out_s.is_empty() {
        out_s
    } else {
        fallback.into()
    }
}

fn first_line(s: &str) -> Option<&str> {
    s.lines().find(|l| !l.trim().is_empty())
}

fn unix_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    fn git_init(root: &Path) {
        assert!(Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .status()
            .unwrap()
            .success());
        for (k, v) in [
            ("user.email", "vault-test@example.com"),
            ("user.name", "dd_vault test"),
            ("commit.gpgsign", "false"),
            ("core.hooksPath", "/dev/null"),
        ] {
            assert!(Command::new("git")
                .args(["config", k, v])
                .current_dir(root)
                .status()
                .unwrap()
                .success());
        }
    }

    fn ctx<'a>(root: &'a Path, extra: &'a [String]) -> GitCtx<'a> {
        GitCtx {
            root,
            credentials: None,
            extra_secret_patterns: extra,
        }
    }

    fn vault_git() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().join("notes");
        init(&root).expect("init");
        git_init(&root);
        (dir, root)
    }

    #[test]
    fn status_not_a_repo() {
        let dir = tempfile::tempdir().expect("tmp");
        let st = status(dir.path()).expect("status");
        assert_eq!(st.state, GitState::NotRepo);
        assert_eq!(st.state.title_badge_owned(), None);
    }

    #[test]
    fn status_clean_and_dirty_and_commit() {
        let (_dir, root) = vault_git();
        let extra: Vec<String> = Vec::new();
        let c = ctx(&root, &extra);
        fs::write(root.join("notes/a.md"), "hello\n").unwrap();
        commit(&c, "first").expect("commit");
        let st = status(&root).expect("status");
        assert_eq!(st.state, GitState::Clean);
        assert_eq!(st.state.title_badge_owned().as_deref(), Some("git:clean"));
        assert_eq!(st.branch.as_deref(), Some("main"));

        fs::write(root.join("notes/a.md"), "hello world\n").unwrap();
        let st = status(&root).expect("dirty");
        assert_eq!(st.state, GitState::Dirty(1));
        assert_eq!(st.state.title_badge_owned().as_deref(), Some("git:±1"));

        assert!(Command::new("git")
            .args(["config", "commit.gpgsign", "true"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::write(root.join("notes/a.md"), "hello again\n").unwrap();
        commit(&c, "unsigned").expect("commit despite gpgsign");
        assert_eq!(status(&root).unwrap().state, GitState::Clean);
    }

    #[test]
    fn commit_rejects_secrets_and_resets() {
        let (_dir, root) = vault_git();
        let extra: Vec<String> = Vec::new();
        let c = ctx(&root, &extra);
        fs::write(root.join("notes/a.md"), "ok\n").unwrap();
        commit(&c, "base").unwrap();
        fs::write(root.join("notes/a.md"), "token ghp_SECRETEXAMPLE\n").unwrap();
        let err = commit(&c, "leak").expect_err("blocked");
        let msg = err.to_string();
        assert!(msg.contains("ghp_"), "{msg}");
        assert!(msg.contains("notes/a.md"), "{msg}");
        let st = status(&root).expect("status");
        assert!(matches!(st.state, GitState::Dirty(_)), "{st:?}");
        let cached = Command::new("git")
            .args(["diff", "--cached", "--name-only"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&cached.stdout).trim().is_empty(),
            "should unstage on reject"
        );
    }

    #[test]
    fn commit_skips_metadata_dir_even_if_it_mentions_needles() {
        let (_dir, root) = vault_git();
        let extra: Vec<String> = Vec::new();
        let c = ctx(&root, &extra);
        fs::write(root.join("notes/a.md"), "ok\n").unwrap();
        commit(&c, "base").unwrap();

        let meta = fs::read_dir(&root)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(".dd_vault-"))
            })
            .expect("meta dir");
        let cfg = meta.join("config.toml");
        assert!(
            fs::read_to_string(&cfg).unwrap().contains("ghp_"),
            "fixture must mention a needle"
        );
        assert!(Command::new("git")
            .args(["add", "-f"])
            .arg(&cfg)
            .current_dir(&root)
            .status()
            .unwrap()
            .success());

        fs::write(root.join("notes/a.md"), "still ok\n").unwrap();
        commit(&c, "note only").expect("must not treat metadata comments as secrets");
        let tracked = Command::new("git")
            .args(["ls-files"])
            .current_dir(&root)
            .output()
            .unwrap();
        let tracked = String::from_utf8_lossy(&tracked.stdout);
        assert!(
            !tracked.contains(".dd_vault-"),
            "metadata must not stay tracked: {tracked}"
        );
    }

    #[test]
    fn commit_rejects_config_pattern() {
        let (_dir, root) = vault_git();
        let extra = vec!["FAMILY_SECRET_".to_string()];
        let c = ctx(&root, &extra);
        fs::write(root.join("notes/a.md"), "FAMILY_SECRET_xyz\n").unwrap();
        let err = commit(&c, "nope").expect_err("blocked");
        assert!(err.to_string().contains("FAMILY_SECRET_"), "{err}");
    }

    #[test]
    fn overlapping_conflict_writes_sidecar_keeps_ours() {
        let (_dir, root) = vault_git();
        let extra: Vec<String> = Vec::new();
        let c = ctx(&root, &extra);
        fs::write(root.join("notes/a.md"), "line1\nline2\nline3\n").unwrap();
        commit(&c, "base").unwrap();

        assert!(Command::new("git")
            .args(["checkout", "-b", "other"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::write(root.join("notes/a.md"), "line1\ntheirs\nline3\n").unwrap();
        commit(&c, "theirs").unwrap();

        assert!(Command::new("git")
            .args(["checkout", "main"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::write(root.join("notes/a.md"), "line1\nours\nline3\n").unwrap();
        commit(&c, "ours").unwrap();

        let merge = Command::new("git")
            .args(["merge", "--no-edit", "other"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(!merge.status.success(), "expected conflict");
        assert_eq!(status(&root).unwrap().state, GitState::Conflict);
        assert_eq!(
            status(&root).unwrap().state.title_badge_owned().as_deref(),
            Some("git:conflict")
        );

        let result = pull(&c).expect("resolve via pull");
        assert_eq!(result.sidecars.len(), 1, "{result:?}");
        let ours = fs::read_to_string(root.join("notes/a.md")).unwrap();
        assert!(ours.contains("ours"), "{ours}");
        assert!(!ours.contains("<<<<<<<"), "{ours}");
        let side = fs::read_to_string(&result.sidecars[0]).unwrap();
        assert!(side.contains("theirs"), "{side}");
        let name = result.sidecars[0].file_name().unwrap().to_string_lossy();
        assert!(name.contains("conflict-"), "{name}");
        assert_eq!(status(&root).unwrap().state, GitState::Dirty(1));
    }

    #[test]
    fn non_overlapping_conflict_auto_merges() {
        let (_dir, root) = vault_git();
        let extra: Vec<String> = Vec::new();
        let c = ctx(&root, &extra);
        fs::write(root.join("notes/a.md"), "top\n\nmiddle\n\nbottom\n").unwrap();
        commit(&c, "base").unwrap();

        assert!(Command::new("git")
            .args(["checkout", "-b", "other"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::write(root.join("notes/a.md"), "top-theirs\n\nmiddle\n\nbottom\n").unwrap();
        commit(&c, "theirs").unwrap();

        assert!(Command::new("git")
            .args(["checkout", "main"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::write(root.join("notes/a.md"), "top\n\nmiddle\n\nbottom-ours\n").unwrap();
        commit(&c, "ours").unwrap();

        let merge = Command::new("git")
            .args(["merge", "--no-edit", "other"])
            .current_dir(&root)
            .output()
            .unwrap();
        if merge.status.success() {
            let body = fs::read_to_string(root.join("notes/a.md")).unwrap();
            assert!(body.contains("top-theirs"), "{body}");
            assert!(body.contains("bottom-ours"), "{body}");
            return;
        }
        let result = pull(&c).expect("auto merge");
        assert!(result.sidecars.is_empty(), "{result:?}");
        assert_eq!(result.auto_merged.len(), 1, "{result:?}");
        let body = fs::read_to_string(root.join("notes/a.md")).unwrap();
        assert!(body.contains("top-theirs"), "{body}");
        assert!(body.contains("bottom-ours"), "{body}");
        assert!(!body.contains("<<<<<<<"), "{body}");
    }

    #[test]
    fn binary_conflict_duplicates() {
        let (_dir, root) = vault_git();
        let extra: Vec<String> = Vec::new();
        let c = ctx(&root, &extra);
        fs::write(root.join("assets/x.bin"), b"a\0b").unwrap();
        commit(&c, "base").unwrap();

        assert!(Command::new("git")
            .args(["checkout", "-b", "other"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::write(root.join("assets/x.bin"), b"t\0b").unwrap();
        commit(&c, "theirs").unwrap();

        assert!(Command::new("git")
            .args(["checkout", "main"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        fs::write(root.join("assets/x.bin"), b"o\0b").unwrap();
        commit(&c, "ours").unwrap();

        let merge = Command::new("git")
            .args(["merge", "--no-edit", "other"])
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(!merge.status.success());
        let result = pull(&c).expect("binary sidecar");
        assert_eq!(result.sidecars.len(), 1, "{result:?}");
        assert_eq!(fs::read(root.join("assets/x.bin")).unwrap(), b"o\0b");
        assert_eq!(fs::read(&result.sidecars[0]).unwrap(), b"t\0b");
    }

    #[test]
    fn pull_push_local_remote() {
        let dir = tempfile::tempdir().expect("tmp");
        let bare = dir.path().join("remote.git");
        assert!(Command::new("git")
            .args(["init", "--bare", "-b", "main"])
            .arg(&bare)
            .status()
            .unwrap()
            .success());

        let a = dir.path().join("a");
        init(&a).unwrap();
        git_init(&a);
        let extra: Vec<String> = Vec::new();
        let ca = ctx(&a, &extra);
        fs::write(a.join("notes/a.md"), "from-a\n").unwrap();
        commit(&ca, "a1").unwrap();
        assert!(Command::new("git")
            .args(["remote", "add", "origin"])
            .arg(&bare)
            .current_dir(&a)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["push", "-u", "origin", "main"])
            .current_dir(&a)
            .status()
            .unwrap()
            .success());

        let b = dir.path().join("b");
        assert!(Command::new("git")
            .args(["clone"])
            .arg(&bare)
            .arg(&b)
            .status()
            .unwrap()
            .success());
        git_init(&b);
        init(&b).unwrap();
        let cb = ctx(&b, &extra);
        fs::write(a.join("notes/a.md"), "from-a-2\n").unwrap();
        commit(&ca, "a2").unwrap();
        push(&ca).expect("push");
        let pulled = pull(&cb).expect("pull");
        assert!(
            pulled.message.to_lowercase().contains("update")
                || pulled.message.to_lowercase().contains("fast-forward")
                || pulled.message.to_lowercase().contains("pulled")
                || b.join("notes/a.md").exists(),
            "{pulled:?}"
        );
        let body = fs::read_to_string(b.join("notes/a.md")).unwrap();
        assert!(body.contains("from-a-2"), "{body}");
    }

    #[test]
    fn credentials_require_0600() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("credentials");
        fs::write(&path, "https://example.com\n").unwrap();
        let mut perm = fs::metadata(&path).unwrap().permissions();
        perm.set_mode(0o644);
        fs::set_permissions(&path, perm).unwrap();
        let err = usable_credentials(&path).expect_err("mode");
        assert!(err.to_string().contains("0600"), "{err}");

        let mut perm = fs::metadata(&path).unwrap().permissions();
        perm.set_mode(0o600);
        fs::set_permissions(&path, perm).unwrap();
        assert_eq!(
            usable_credentials(&path).unwrap().as_deref(),
            Some(path.as_path())
        );
    }

    #[test]
    fn parse_porcelain_samples() {
        let clean = parse_porcelain_v2("# branch.head main\n# branch.ab +0 -0\n");
        assert_eq!(clean.state, GitState::Clean);
        let dirty = parse_porcelain_v2(
            "# branch.head main\n1 .M N... 0 0 0 h h h notes/a.md\n? extra.md\n",
        );
        assert_eq!(dirty.state, GitState::Dirty(2));
        let conflict =
            parse_porcelain_v2("# branch.head main\nu UU N... 0 0 0 0 h h h h notes/a.md\n");
        assert_eq!(conflict.state, GitState::Conflict);
    }

    #[test]
    fn load_patterns_from_vault_config() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path().join("notes");
        let vault = init(&root).unwrap();
        fs::write(
            vault.meta_dir.join("config.toml"),
            "preview = true\nsecret_patterns = [\"CUSTOMTOK_\"]\n",
        )
        .unwrap();
        let pats = load_secret_patterns(&vault, None);
        assert_eq!(pats, vec!["CUSTOMTOK_".to_string()]);
    }

    fn git_config(root: &Path, key: &str, value: &str) {
        assert!(Command::new("git")
            .args(["config", key, value])
            .current_dir(root)
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn credential_helper_path_is_quoted() {
        assert_eq!(quote_helper_path("/tmp/my keys"), "\"/tmp/my keys\"");
        assert_eq!(quote_helper_path(r"/tmp/a\b"), r#""/tmp/a\\b""#);
    }

    #[test]
    fn status_does_not_run_fsmonitor_or_pager() {
        let (_dir, root) = vault_git();
        let marker = root.join("pwned-status");
        git_config(
            &root,
            "core.fsmonitor",
            &format!("touch {}", marker.display()),
        );
        git_config(&root, "pager.status", "true");
        git_config(&root, "core.pager", &format!("touch {}", marker.display()));
        status(&root).expect("status");
        assert!(
            !marker.exists(),
            "git status executed a command from repo config"
        );
    }

    #[test]
    fn commit_does_not_run_hook_or_clean_filter() {
        let (_dir, root) = vault_git();
        let marker = root.join("pwned-commit");
        let hooks = root.join(".git/hooks");
        fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        fs::write(&hook, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
        let mut perm = fs::metadata(&hook).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&hook, perm).unwrap();
        git_config(
            &root,
            "filter.evil.clean",
            &format!("touch {}; cat", marker.display()),
        );
        git_config(&root, "filter.evil.required", "true");
        fs::write(root.join(".gitattributes"), "* filter=evil\n").unwrap();
        fs::write(root.join("notes/a.md"), "hello\n").unwrap();
        let extra = Vec::new();
        commit(&ctx(&root, &extra), "no hooks").expect("commit without repo commands");
        assert!(!marker.exists(), "pre-commit hook or clean filter ran");
    }

    #[test]
    fn pull_does_not_run_ext_remote() {
        let (_dir, root) = vault_git();
        let marker = root.join("pwned-ext");
        fs::write(root.join("notes/a.md"), "hello\n").unwrap();
        let extra = Vec::new();
        let c = ctx(&root, &extra);
        commit(&c, "base").unwrap();
        let url = format!("ext::touch% {}", marker.display());
        assert!(Command::new("git")
            .args(["remote", "add", "origin", &url])
            .current_dir(&root)
            .status()
            .unwrap()
            .success());
        git_config(&root, "protocol.ext.allow", "always");
        let err = pull(&c).expect_err("ext remote");
        assert!(!marker.exists(), "ext:: remote executed during pull: {err}");
    }

    #[test]
    fn commit_rejects_private_key_and_escaping_symlink() {
        let (_dir, root) = vault_git();
        let extra = Vec::new();
        let c = ctx(&root, &extra);
        fs::write(
            root.join("notes/a.md"),
            "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n",
        )
        .unwrap();
        let err = commit(&c, "key").expect_err("key");
        assert!(
            err.to_string().contains("BEGIN") || err.to_string().contains("secret"),
            "{err}"
        );
        fs::write(root.join("notes/a.md"), "ok\n").unwrap();
        commit(&c, "clean").unwrap();
        let outside = root.parent().unwrap().join("secret.txt");
        fs::write(&outside, "no-token\n").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("notes/leak.md")).unwrap();
        let err = commit(&c, "link").expect_err("symlink");
        assert!(err.to_string().contains("symlink"), "{err}");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "no-token\n");
    }

    #[test]
    fn harden_args_clears_aliases_and_proxy_and_refuses_rewrites() {
        let (_dir, root) = vault_git();
        git_config(&root, "alias.pwn", "!touch /tmp/dd-vault-should-not-run");
        git_config(&root, "remote.origin.proxy", "http://127.0.0.1:9");
        let args = harden_args(&root, None).expect("harden");
        assert!(
            args.iter().any(|a| a == "alias.pwn="),
            "repo alias was not cleared: {args:?}"
        );
        assert!(
            args.iter().any(|a| a == "remote.origin.proxy="),
            "remote proxy was not cleared: {args:?}"
        );

        git_config(&root, "url.evil.insteadof", "https://github.com/");
        let err = harden_args(&root, None).expect_err("insteadOf");
        assert!(err.to_string().contains("insteadOf"), "{err}");
    }

    #[test]
    fn harden_args_refuses_custom_uploadpack() {
        let (_dir, root) = vault_git();
        let marker = root.join("pwned-upload");
        git_config(
            &root,
            "remote.origin.uploadpack",
            &format!("touch {}", marker.display()),
        );
        let err = status(&root).expect_err("custom uploadpack");
        assert!(err.to_string().contains("upload-pack"), "{err}");
        assert!(!marker.exists(), "upload-pack program ran");

        git_config(&root, "remote.origin.uploadpack", "git-upload-pack");
        status(&root).expect("default upload-pack is allowed");
    }

    #[test]
    fn commit_does_not_follow_gitignore_symlink() {
        let (_dir, root) = vault_git();
        let outside = root.parent().unwrap().join("outside-gitignore");
        fs::write(&outside, "original\n").unwrap();
        fs::remove_file(root.join(".gitignore")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join(".gitignore")).unwrap();
        fs::write(root.join("notes/a.md"), "hello\n").unwrap();
        let extra = Vec::new();
        let err = commit(&ctx(&root, &extra), "nope").expect_err("symlink gitignore");
        assert!(err.to_string().contains("symlink"), "{err}");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "original\n");
    }

    #[test]
    fn conflict_write_does_not_follow_symlink() {
        let (_dir, root) = vault_git();
        let outside = root.parent().unwrap().join("conflict-secret.txt");
        fs::write(&outside, "safe\n").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("notes/a.md")).unwrap();
        let err = write_conflict_bytes(&root, Path::new("notes/a.md"), b"theirs\n")
            .expect_err("symlink write");
        assert!(
            err.to_string().contains("symlink") || err.to_string().contains("escapes"),
            "{err}"
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "safe\n");
        let dest = write_conflict_sidecar(&root, Path::new("notes/a.md"), b"side\n", 9).unwrap();
        assert_eq!(fs::read_to_string(&dest).unwrap(), "side\n");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "safe\n");
    }
}
