use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Not a git worktree: {0}")]
    NotWorktree(PathBuf),

    #[error("Path is a main repository, not a worktree: {0}")]
    IsMainRepo(PathBuf),

    #[error("Failed to read git file {path}: {source}")]
    GitFileRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Invalid .git file format in {0}")]
    InvalidGitFile(PathBuf),

    #[error("Git command failed: {0}")]
    GitCommand(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
