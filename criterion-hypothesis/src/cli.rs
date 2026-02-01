//! Command-line interface for criterion-hypothesis.

use crate::config::Config;
use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "criterion-hypothesis")]
#[command(about = "Statistically rigorous A/B testing of benchmarks across commits")]
#[command(version)]
pub struct Cli {
    /// Baseline commit/branch to compare against
    #[arg(short, long)]
    pub baseline: String,

    /// Candidate commit/branch to test
    #[arg(short, long)]
    pub candidate: String,

    /// Confidence level for statistical tests (0.0-1.0)
    #[arg(long)]
    pub confidence_level: Option<f64>,

    /// Number of sample iterations per benchmark
    #[arg(long)]
    pub sample_size: Option<u32>,

    /// Number of warmup iterations
    #[arg(long)]
    pub warmup_iterations: Option<u32>,

    /// Path to config file
    #[arg(long, default_value = ".criterion-hypothesis.toml")]
    pub config: String,

    /// Path to project within repo (for monorepos/subdirectories)
    #[arg(long)]
    pub project_path: Option<PathBuf>,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

impl Cli {
    /// Apply CLI overrides to the configuration.
    ///
    /// CLI arguments take precedence over config file values.
    /// Only non-None optional values will override the config.
    pub fn apply_to_config(&self, config: &mut Config) {
        if let Some(confidence_level) = self.confidence_level {
            config.hypothesis.confidence_level = confidence_level;
        }

        if let Some(sample_size) = self.sample_size {
            config.orchestration.sample_size = sample_size;
        }

        if let Some(warmup_iterations) = self.warmup_iterations {
            config.orchestration.warmup_iterations = warmup_iterations;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_to_config_with_overrides() {
        let cli = Cli {
            baseline: "main".to_string(),
            candidate: "feature".to_string(),
            confidence_level: Some(0.99),
            sample_size: Some(200),
            warmup_iterations: Some(20),
            config: "custom.toml".to_string(),
            project_path: None,
            verbose: true,
        };

        let mut config = Config::default();
        cli.apply_to_config(&mut config);

        assert_eq!(config.hypothesis.confidence_level, 0.99);
        assert_eq!(config.orchestration.sample_size, 200);
        assert_eq!(config.orchestration.warmup_iterations, 20);
    }

    #[test]
    fn test_apply_to_config_without_overrides() {
        let cli = Cli {
            baseline: "main".to_string(),
            candidate: "feature".to_string(),
            confidence_level: None,
            sample_size: None,
            warmup_iterations: None,
            config: ".criterion-hypothesis.toml".to_string(),
            project_path: None,
            verbose: false,
        };

        let mut config = Config::default();
        let original_confidence = config.hypothesis.confidence_level;
        let original_sample_size = config.orchestration.sample_size;
        let original_warmup = config.orchestration.warmup_iterations;

        cli.apply_to_config(&mut config);

        // Values should remain unchanged
        assert_eq!(config.hypothesis.confidence_level, original_confidence);
        assert_eq!(config.orchestration.sample_size, original_sample_size);
        assert_eq!(config.orchestration.warmup_iterations, original_warmup);
    }

    #[test]
    fn test_apply_to_config_partial_overrides() {
        let cli = Cli {
            baseline: "main".to_string(),
            candidate: "feature".to_string(),
            confidence_level: Some(0.90),
            sample_size: None,
            warmup_iterations: Some(5),
            config: ".criterion-hypothesis.toml".to_string(),
            project_path: None,
            verbose: false,
        };

        let mut config = Config::default();
        cli.apply_to_config(&mut config);

        // Only specified values should be overridden
        assert_eq!(config.hypothesis.confidence_level, 0.90);
        assert_eq!(config.orchestration.sample_size, 100); // Default unchanged
        assert_eq!(config.orchestration.warmup_iterations, 5);
    }

    #[test]
    fn test_cli_parse() {
        let cli = Cli::parse_from([
            "criterion-hypothesis",
            "--baseline",
            "main",
            "--candidate",
            "feature-branch",
            "--confidence-level",
            "0.99",
            "--sample-size",
            "50",
            "--verbose",
        ]);

        assert_eq!(cli.baseline, "main");
        assert_eq!(cli.candidate, "feature-branch");
        assert_eq!(cli.confidence_level, Some(0.99));
        assert_eq!(cli.sample_size, Some(50));
        assert!(cli.verbose);
    }

    #[test]
    fn test_cli_parse_minimal() {
        let cli = Cli::parse_from([
            "criterion-hypothesis",
            "--baseline",
            "v1.0.0",
            "--candidate",
            "HEAD",
        ]);

        assert_eq!(cli.baseline, "v1.0.0");
        assert_eq!(cli.candidate, "HEAD");
        assert_eq!(cli.confidence_level, None);
        assert_eq!(cli.sample_size, None);
        assert_eq!(cli.warmup_iterations, None);
        assert_eq!(cli.config, ".criterion-hypothesis.toml");
        assert!(!cli.verbose);
    }
}
