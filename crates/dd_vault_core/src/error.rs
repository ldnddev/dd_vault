use std::io;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("path is not a directory: {}", .0.display())]
    NotDirectory(PathBuf),
    #[error("path not found: {}", .0.display())]
    NotFound(PathBuf),
    #[error("not a dd_vault (no .dd_vault-* metadata dir): {}", .0.display())]
    NotAVault(PathBuf),
    #[error("multiple .dd_vault-* metadata dirs in {}", .0.display())]
    Ambiguous(PathBuf),
    #[error("invalid vault name: {0}")]
    InvalidName(String),
    #[error("HOME and XDG_CONFIG_HOME are unset; cannot locate ~/.config/ldnddev")]
    NoConfigHome,
    #[error("{0}")]
    Config(String),
    #[error("no vault to reindex (pass a path or open a vault first)")]
    NoVaultToReindex,
    #[error("invalid name: {0}")]
    InvalidEntryName(String),
    #[error("already exists: {}", .0.display())]
    AlreadyExists(PathBuf),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("index: {0}")]
    Index(String),
    #[error("git: {0}")]
    Git(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}
