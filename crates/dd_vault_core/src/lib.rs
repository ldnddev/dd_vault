//! Vault model, file ops, git, SQLite index, search, and config.

mod config;
mod daily;
mod error;
mod fs_ops;
mod git;
mod index;
mod parse;
mod paths;
mod registry;
mod reindex;
mod skip;
mod tree;
mod vault;

pub use config::VaultConfig;
pub use daily::{daily_rel, ensure_daily, today_ymd};
pub use error::Error;
pub use fs_ops::{
    create_dir, create_file, delete_entry, rel_from_root, rename_entry, resolve_inside,
    validate_entry_name,
};
pub use git::{
    commit, load_secret_patterns, pull, push, status, usable_credentials, GitCtx, GitOpResult,
    GitState, GitStatus, BUILTIN_SECRET_NEEDLES,
};
pub use index::{FileHit, Index};
pub use parse::{parse_note, LinkKind, ParsedLink, ParsedNote};
pub use paths::Paths;
pub use registry::{Registry, VaultEntry};
pub use reindex::{reindex, ReindexReport};
pub use skip::{skip_dir_name, skip_entry_name, skip_file_name};
pub use tree::{walk_tree, FsNode, NodeKind};
pub use vault::{init, open, Vault, DEFAULT_VAULT_CONFIG, GITIGNORE};

#[cfg(test)]
mod tests;
