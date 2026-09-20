use crate::index::Index;
use crate::{Error, Vault};

pub use crate::index::ReindexReport;

/// Rebuild the SQLite + FTS5 derived index.
pub fn reindex(vault: &Vault) -> Result<ReindexReport, Error> {
    Index::rebuild(vault)
}
