use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no file name")]
    NoPath,
    #[error("file too large to open: {}", .0.display())]
    TooLarge(PathBuf),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}
