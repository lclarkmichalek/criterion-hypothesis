use std::path::{Path, PathBuf};
use std::process::Command;

use super::{SourceError, SourceProvider};

/// A source provider that uses git worktrees to prepare baseline and candidate sources.
///
/// This provider creates worktrees at `.criterion-hypothesis/{baseline,candidate}` relative
/// to the repository root. Each worktree checks out the specified commit or branch.
#[derive(Debug)]
pub struct GitWorktreeProvider {
    /// The root directory of the git repository.
    repo_root: PathBuf,
}

impl GitWorktreeProvider {
    /// Create a new GitWorktreeProvider by discovering the repository root.
    ///
    /// Uses `git rev-parse --show-toplevel` to find the root of the current repository.
    pub fn new() -> Result<Self, SourceError> {
        let repo_root = Self::find_repo_root()?;
        Ok(Self { repo_root })
    }

    /// Create a new GitWorktreeProvider with a specific repository root.
    pub fn with_repo_root(repo_root: PathBuf) -> Self {
        Self { repo_root }
    }

    /// Find the root of the git repository.
    fn find_repo_root() -> Result<PathBuf, SourceError> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .map_err(|e| SourceError::GitCommand(format!("Failed to run git: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SourceError::GitCommand(format!(
                "git rev-parse --show-toplevel failed: {}",
                stderr.trim()
            )));
        }

        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(PathBuf::from(path))
    }

    /// Get the base directory for worktrees.
    fn worktree_base(&self) -> PathBuf {
        self.repo_root.join(".criterion-hypothesis")
    }

    /// Get the path for the baseline worktree.
    fn baseline_path(&self) -> PathBuf {
        self.worktree_base().join("baseline")
    }

    /// Get the path for the candidate worktree.
    fn candidate_path(&self) -> PathBuf {
        self.worktree_base().join("candidate")
    }

    /// Run a git command in the repository root.
    fn run_git_command(&self, args: &[&str]) -> Result<String, SourceError> {
        let output = Command::new("git")
            .current_dir(&self.repo_root)
            .args(args)
            .output()
            .map_err(|e| SourceError::GitCommand(format!("Failed to run git: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SourceError::GitCommand(format!(
                "git {} failed: {}",
                args.join(" "),
                stderr.trim()
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Create a worktree at the specified path for the given ref.
    fn create_worktree(&self, path: &Path, git_ref: &str) -> Result<(), SourceError> {
        // Ensure the parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                SourceError::WorktreeCreation(format!(
                    "Failed to create parent directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        // Add the worktree
        let path_str = path.to_string_lossy();
        self.run_git_command(&["worktree", "add", &path_str, git_ref])
            .map_err(|e| SourceError::WorktreeCreation(format!("{}", e)))?;

        Ok(())
    }

    /// Remove a worktree at the specified path.
    fn remove_worktree(&self, path: &Path) -> Result<(), SourceError> {
        if !path.exists() {
            return Ok(());
        }

        let path_str = path.to_string_lossy();
        self.run_git_command(&["worktree", "remove", "--force", &path_str])
            .map_err(|e| SourceError::Cleanup(format!("{}", e)))?;

        Ok(())
    }

    /// Clean up any existing worktrees before creating new ones.
    fn cleanup_existing(&self) -> Result<(), SourceError> {
        // First, prune any stale worktree references
        let _ = self.run_git_command(&["worktree", "prune"]);

        // Remove existing worktrees if they exist
        self.remove_worktree(&self.baseline_path())?;
        self.remove_worktree(&self.candidate_path())?;

        Ok(())
    }
}

impl SourceProvider for GitWorktreeProvider {
    fn prepare_sources(
        &self,
        baseline: &str,
        candidate: &str,
    ) -> Result<(PathBuf, PathBuf), SourceError> {
        // Clean up any existing worktrees first
        self.cleanup_existing()?;

        let baseline_path = self.baseline_path();
        let candidate_path = self.candidate_path();

        // Create the baseline worktree
        self.create_worktree(&baseline_path, baseline)
            .map_err(|e| SourceError::Checkout(baseline.to_string(), format!("{}", e)))?;

        // Create the candidate worktree
        self.create_worktree(&candidate_path, candidate)
            .map_err(|e| {
                // Try to clean up the baseline worktree if candidate creation fails
                let _ = self.remove_worktree(&baseline_path);
                SourceError::Checkout(candidate.to_string(), format!("{}", e))
            })?;

        Ok((baseline_path, candidate_path))
    }

    fn cleanup(&self) -> Result<(), SourceError> {
        self.remove_worktree(&self.baseline_path())?;
        self.remove_worktree(&self.candidate_path())?;

        // Remove the .criterion-hypothesis directory if it's empty
        let worktree_base = self.worktree_base();
        if worktree_base.exists() {
            if let Ok(entries) = std::fs::read_dir(&worktree_base) {
                if entries.count() == 0 {
                    let _ = std::fs::remove_dir(&worktree_base);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worktree_paths() {
        let provider = GitWorktreeProvider::with_repo_root(PathBuf::from("/test/repo"));

        assert_eq!(
            provider.worktree_base(),
            PathBuf::from("/test/repo/.criterion-hypothesis")
        );
        assert_eq!(
            provider.baseline_path(),
            PathBuf::from("/test/repo/.criterion-hypothesis/baseline")
        );
        assert_eq!(
            provider.candidate_path(),
            PathBuf::from("/test/repo/.criterion-hypothesis/candidate")
        );
    }
}
