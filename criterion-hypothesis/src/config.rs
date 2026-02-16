//! Configuration loading for criterion-hypothesis.
//!
//! Supports loading configuration from TOML files, with sensible defaults
//! for all settings.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level configuration for criterion-hypothesis.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Settings for statistical hypothesis testing.
    pub hypothesis: HypothesisConfig,
    /// Settings for benchmark orchestration.
    pub orchestration: OrchestrationConfig,
    /// Settings for building benchmark binaries.
    pub build: BuildConfig,
    /// Network settings for harness communication.
    pub network: NetworkConfig,
}

/// Configuration for statistical hypothesis testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HypothesisConfig {
    /// Confidence level for statistical tests (e.g., 0.95 for 95% confidence).
    pub confidence_level: f64,
    /// Minimum effect size (in percent) to consider practically significant.
    pub minimum_effect_size: f64,
}

/// Configuration for benchmark orchestration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OrchestrationConfig {
    /// Interval in milliseconds between interleaved benchmark runs.
    pub interleave_interval_ms: u64,
    /// Number of warmup iterations before collecting measurements.
    pub warmup_iterations: u32,
    /// Number of samples to collect for each benchmark.
    pub sample_size: u32,
}

/// Configuration for building benchmark binaries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildConfig {
    /// Cargo profile to use for building benchmarks.
    pub profile: String,
    /// Additional flags to pass to cargo.
    pub cargo_flags: Vec<String>,
    /// Specific bench targets to build and run (if empty, builds all with --benches).
    pub bench_targets: Vec<String>,
}

/// Network configuration for harness communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    /// Base port for harness HTTP servers.
    pub base_port: u16,
    /// Timeout in milliseconds for harness communication.
    pub harness_timeout_ms: u64,
}

impl Default for HypothesisConfig {
    fn default() -> Self {
        Self {
            confidence_level: 0.95,
            minimum_effect_size: 1.0, // 1% minimum effect size
        }
    }
}

impl Default for OrchestrationConfig {
    fn default() -> Self {
        Self {
            interleave_interval_ms: 100,
            warmup_iterations: 3,
            sample_size: 100,
        }
    }
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            profile: "release".to_string(),
            cargo_flags: Vec::new(),
            bench_targets: Vec::new(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            base_port: 9100,
            harness_timeout_ms: 30_000, // 30 seconds
        }
    }
}

/// Default configuration file name.
const DEFAULT_CONFIG_FILE: &str = ".criterion-hypothesis.toml";

impl Config {
    /// Load configuration from a TOML file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the TOML configuration file
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Config> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        Ok(config)
    }

    /// Load configuration from the default file (`.criterion-hypothesis.toml`) or use defaults.
    ///
    /// This function searches for the configuration file in the current directory.
    /// If the file doesn't exist, default configuration is returned.
    /// If the file exists but cannot be parsed, an error is returned.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration file exists but cannot be parsed.
    pub fn load_or_default() -> Result<Config> {
        let path = Path::new(DEFAULT_CONFIG_FILE);

        if path.exists() {
            Self::load(path)
        } else {
            Ok(Config::default())
        }
    }

    /// Load configuration from the specified path, or try default locations.
    ///
    /// If a path is provided, loads from that path.
    /// Otherwise, tries to load from `.criterion-hypothesis.toml` or uses defaults.
    ///
    /// # Arguments
    ///
    /// * `path` - Optional path to a configuration file
    ///
    /// # Errors
    ///
    /// Returns an error if the specified file cannot be read or parsed.
    pub fn load_from(path: Option<&Path>) -> Result<Config> {
        match path {
            Some(p) => Self::load(p),
            None => Self::load_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = Config::default();

        assert_eq!(config.hypothesis.confidence_level, 0.95);
        assert_eq!(config.hypothesis.minimum_effect_size, 1.0);
        assert_eq!(config.orchestration.interleave_interval_ms, 100);
        assert_eq!(config.orchestration.warmup_iterations, 3);
        assert_eq!(config.orchestration.sample_size, 100);
        assert_eq!(config.build.profile, "release");
        assert!(config.build.cargo_flags.is_empty());
        assert!(config.build.bench_targets.is_empty());
        assert_eq!(config.network.base_port, 9100);
        assert_eq!(config.network.harness_timeout_ms, 30_000);
    }

    #[test]
    fn test_load_partial_config() {
        let toml_content = r#"
[hypothesis]
confidence_level = 0.99

[orchestration]
sample_size = 200
"#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(file.path()).unwrap();

        // Overridden values
        assert_eq!(config.hypothesis.confidence_level, 0.99);
        assert_eq!(config.orchestration.sample_size, 200);

        // Default values
        assert_eq!(config.hypothesis.minimum_effect_size, 1.0);
        assert_eq!(config.orchestration.warmup_iterations, 3);
        assert_eq!(config.build.profile, "release");
    }

    #[test]
    fn test_load_full_config() {
        let toml_content = r#"
[hypothesis]
confidence_level = 0.99
minimum_effect_size = 2.5

[orchestration]
interleave_interval_ms = 50
warmup_iterations = 5
sample_size = 200

[build]
profile = "bench"
cargo_flags = ["--features", "test-feature"]

[network]
base_port = 8000
harness_timeout_ms = 60000
"#;

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(toml_content.as_bytes()).unwrap();

        let config = Config::load(file.path()).unwrap();

        assert_eq!(config.hypothesis.confidence_level, 0.99);
        assert_eq!(config.hypothesis.minimum_effect_size, 2.5);
        assert_eq!(config.orchestration.interleave_interval_ms, 50);
        assert_eq!(config.orchestration.warmup_iterations, 5);
        assert_eq!(config.orchestration.sample_size, 200);
        assert_eq!(config.build.profile, "bench");
        assert_eq!(config.build.cargo_flags, vec!["--features", "test-feature"]);
        assert_eq!(config.network.base_port, 8000);
        assert_eq!(config.network.harness_timeout_ms, 60000);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let result = Config::load(Path::new("/nonexistent/path/config.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_invalid_toml() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"this is not valid toml {{{{").unwrap();

        let result = Config::load(file.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_load_or_default_no_file() {
        // This test assumes .criterion-hypothesis.toml doesn't exist in the test directory
        // In practice, this would use the default config
        let config = Config::load_or_default();
        // Should not panic, either loads file or returns default
        assert!(config.is_ok());
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(
            config.hypothesis.confidence_level,
            parsed.hypothesis.confidence_level
        );
        assert_eq!(
            config.orchestration.sample_size,
            parsed.orchestration.sample_size
        );
        assert_eq!(config.build.profile, parsed.build.profile);
        assert_eq!(config.network.base_port, parsed.network.base_port);
    }
}
