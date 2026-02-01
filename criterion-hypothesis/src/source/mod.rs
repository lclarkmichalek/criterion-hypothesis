use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("Failed to create worktree: {0}")]
    WorktreeCreation(String),
    #[error("Failed to checkout ref '{0}': {1}")]
    Checkout(String, String),
    #[error("Failed to cleanup: {0}")]
    Cleanup(String),
    #[error("Git command failed: {0}")]
    GitCommand(String),
}

pub trait SourceProvider: Send + Sync {
    fn prepare_sources(&self, baseline: &str, candidate: &str)
        -> Result<(PathBuf, PathBuf), SourceError>;
    fn cleanup(&self) -> Result<(), SourceError>;
}

mod git;
pub use git::GitWorktreeProvider;
