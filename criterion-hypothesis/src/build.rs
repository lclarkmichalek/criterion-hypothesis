//! Build manager for compiling benchmark binaries.
//!
//! This module handles building benchmark binaries with the custom harness.
//! It locates Cargo.toml, runs cargo build, and finds the resulting benchmark
//! binary in the target directory.

use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

/// Errors that can occur during benchmark building.
#[derive(Debug, Error)]
pub enum BuildError {
    /// No Cargo.toml found in the specified directory.
    #[error("Failed to find Cargo.toml in {0}")]
    NoCargoToml(PathBuf),
    /// Failed to read Cargo.toml.
    #[error("Failed to read Cargo.toml: {0}")]
    ReadError(String),
    /// Failed to write Cargo.toml.
    #[error("Failed to write Cargo.toml: {0}")]
    WriteError(String),
    /// Cargo build failed.
    #[error("Build failed: {0}")]
    BuildFailed(String),
    /// No benchmark binary found after building.
    #[error("No benchmark binary found")]
    NoBenchmarkBinary,
    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Manages building benchmark binaries.
///
/// The BuildManager handles compiling benchmark binaries with the appropriate
/// Cargo profile and flags. It locates the resulting binary in the target
/// directory after a successful build.
#[derive(Debug)]
pub struct BuildManager {
    /// The Cargo profile to use for building (e.g., "release", "bench").
    profile: String,
    /// Additional flags to pass to cargo.
    cargo_flags: Vec<String>,
}

/// Result of a successful build.
#[derive(Debug)]
pub struct BuildResult {
    /// Path to the compiled benchmark binary.
    pub binary_path: PathBuf,
}

impl BuildManager {
    /// Create a new BuildManager with the specified profile and cargo flags.
    ///
    /// # Arguments
    ///
    /// * `profile` - The Cargo profile to use (e.g., "release", "bench")
    /// * `cargo_flags` - Additional flags to pass to cargo
    pub fn new(profile: String, cargo_flags: Vec<String>) -> Self {
        Self {
            profile,
            cargo_flags,
        }
    }

    /// Build the benchmark binary for a source tree.
    ///
    /// This function:
    /// 1. Verifies that Cargo.toml exists in the source path
    /// 2. Runs `cargo build --profile {profile} --benches` with any additional flags
    /// 3. Finds the benchmark binary in `target/{profile}/deps/`
    /// 4. Returns the path to the most recently modified benchmark binary
    ///
    /// # Arguments
    ///
    /// * `source_path` - Path to the root of the source tree containing Cargo.toml
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Cargo.toml is not found
    /// - The build fails
    /// - No benchmark binary can be found after building
    pub fn build(&self, source_path: &Path) -> Result<BuildResult, BuildError> {
        // 1. Verify Cargo.toml exists
        let cargo_toml = source_path.join("Cargo.toml");
        if !cargo_toml.exists() {
            return Err(BuildError::NoCargoToml(source_path.to_path_buf()));
        }

        // 2. TODO: In future, inject harness dependency (for now, assume it exists)

        // 3. Run cargo build --profile {profile} --benches
        self.run_cargo_build(source_path)?;

        // 4. Find the benchmark binary in target/{profile}/deps/
        let binary_path = self.find_benchmark_binary(source_path)?;

        // 5. Return the path
        Ok(BuildResult { binary_path })
    }

    /// Run cargo build with the configured profile and flags.
    fn run_cargo_build(&self, source_path: &Path) -> Result<(), BuildError> {
        let mut cmd = Command::new("cargo");
        cmd.current_dir(source_path);
        cmd.arg("build");

        // Add profile flag
        // Note: "release" profile uses --release flag, others use --profile
        if self.profile == "release" {
            cmd.arg("--release");
        } else if self.profile != "dev" {
            cmd.arg("--profile");
            cmd.arg(&self.profile);
        }

        // Build benchmarks
        cmd.arg("--benches");

        // Add any additional cargo flags
        for flag in &self.cargo_flags {
            cmd.arg(flag);
        }

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(BuildError::BuildFailed(format!(
                "cargo build failed:\n{}\n{}",
                stdout.trim(),
                stderr.trim()
            )));
        }

        Ok(())
    }

    /// Find the benchmark binary in the target directory.
    ///
    /// Looks in `target/{profile}/deps/` for executable files matching the
    /// pattern `*bench*`. Returns the most recently modified binary.
    fn find_benchmark_binary(&self, source_path: &Path) -> Result<PathBuf, BuildError> {
        // Determine the target directory name based on profile
        let target_dir = self.target_dir_name();
        let deps_path = source_path.join("target").join(target_dir).join("deps");

        if !deps_path.exists() {
            return Err(BuildError::NoBenchmarkBinary);
        }

        // Find all benchmark binaries
        let binaries = self.find_benchmark_files(&deps_path)?;

        if binaries.is_empty() {
            return Err(BuildError::NoBenchmarkBinary);
        }

        // Return the most recently modified binary
        let newest = binaries
            .into_iter()
            .max_by_key(|path| {
                path.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            })
            .ok_or(BuildError::NoBenchmarkBinary)?;

        Ok(newest)
    }

    /// Get the target directory name for the current profile.
    fn target_dir_name(&self) -> &str {
        // Cargo uses "debug" for dev profile, profile name for others
        if self.profile == "dev" {
            "debug"
        } else {
            &self.profile
        }
    }

    /// Find benchmark executable files in the deps directory.
    fn find_benchmark_files(&self, deps_path: &Path) -> Result<Vec<PathBuf>, BuildError> {
        let entries = std::fs::read_dir(deps_path)?;
        let mut binaries = Vec::new();

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            // Skip if not a file
            if !path.is_file() {
                continue;
            }

            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name,
                None => continue,
            };

            // Check if it matches the benchmark pattern
            if !self.is_benchmark_binary(file_name, &path) {
                continue;
            }

            binaries.push(path);
        }

        Ok(binaries)
    }

    /// Check if a file is a benchmark binary.
    ///
    /// On Unix: executable files containing "bench" in the name, without .d extension
    /// On Windows: .exe files containing "bench" in the name
    fn is_benchmark_binary(&self, file_name: &str, path: &Path) -> bool {
        // Must contain "bench" in the name
        if !file_name.contains("bench") {
            return false;
        }

        // Skip .d files (dependency files)
        if file_name.ends_with(".d") {
            return false;
        }

        // Skip .rmeta files
        if file_name.ends_with(".rmeta") {
            return false;
        }

        // Skip .rlib files
        if file_name.ends_with(".rlib") {
            return false;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            // On Unix, check if executable (no extension, executable permission)
            if path.extension().is_some() {
                return false;
            }

            if let Ok(metadata) = path.metadata() {
                let mode = metadata.permissions().mode();
                // Check if any execute bit is set
                return mode & 0o111 != 0;
            }
            false
        }

        #[cfg(windows)]
        {
            // On Windows, look for .exe extension
            file_name.ends_with(".exe")
        }

        #[cfg(not(any(unix, windows)))]
        {
            // Fallback: just check it's not a known non-executable extension
            !file_name.ends_with(".d")
                && !file_name.ends_with(".rmeta")
                && !file_name.ends_with(".rlib")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_manager_new() {
        let manager = BuildManager::new("release".to_string(), vec!["--features".to_string(), "test".to_string()]);
        assert_eq!(manager.profile, "release");
        assert_eq!(manager.cargo_flags, vec!["--features", "test"]);
    }

    #[test]
    fn test_target_dir_name() {
        let release = BuildManager::new("release".to_string(), vec![]);
        assert_eq!(release.target_dir_name(), "release");

        let dev = BuildManager::new("dev".to_string(), vec![]);
        assert_eq!(dev.target_dir_name(), "debug");

        let bench = BuildManager::new("bench".to_string(), vec![]);
        assert_eq!(bench.target_dir_name(), "bench");
    }

    #[test]
    fn test_no_cargo_toml_error() {
        let manager = BuildManager::new("release".to_string(), vec![]);
        let result = manager.build(Path::new("/nonexistent/path"));

        assert!(matches!(result, Err(BuildError::NoCargoToml(_))));
    }

    #[cfg(unix)]
    #[test]
    fn test_is_benchmark_binary_unix() {
        use std::io::Write;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let manager = BuildManager::new("release".to_string(), vec![]);

        // Create a file that looks like a benchmark binary
        let bench_path = temp_dir.path().join("my_benchmark-abc123");
        {
            let mut file = std::fs::File::create(&bench_path).unwrap();
            file.write_all(b"fake binary").unwrap();
        }
        // Make it executable
        std::fs::set_permissions(&bench_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(manager.is_benchmark_binary("my_benchmark-abc123", &bench_path));

        // Create a .d file (should be rejected)
        let d_path = temp_dir.path().join("my_benchmark-abc123.d");
        std::fs::File::create(&d_path).unwrap();
        assert!(!manager.is_benchmark_binary("my_benchmark-abc123.d", &d_path));

        // Create a file without "bench" in name (should be rejected)
        let other_path = temp_dir.path().join("my_test-abc123");
        {
            let mut file = std::fs::File::create(&other_path).unwrap();
            file.write_all(b"fake binary").unwrap();
        }
        std::fs::set_permissions(&other_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!manager.is_benchmark_binary("my_test-abc123", &other_path));
    }

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
}
