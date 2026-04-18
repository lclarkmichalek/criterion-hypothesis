//! Command-line interface for hypobench.

use crate::config::Config;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "hypobench")]
#[command(about = "Statistically rigorous A/B testing of benchmarks across commits")]
#[command(version)]
pub struct Cli {
    /// The subcommand. If omitted, the `run` subcommand is used with the
    /// top-level arguments below (backward-compatible with pre-0.5 usage).
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Top-level args, used when no subcommand is given (defaults to `run`).
    #[command(flatten)]
    pub run: RunArgs,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run benchmarks and produce a report.
    Run(RunArgs),
    /// Re-render a previously produced JSON report without re-running benchmarks.
    Report(ReportArgs),
}

#[derive(Debug, Args, Clone)]
pub struct RunArgs {
    /// Baseline commit/branch to compare against (or use --baseline-url for manual mode)
    #[arg(short, long)]
    pub baseline: Option<String>,

    /// Candidate commit/branch to test (or use --candidate-url for manual mode)
    #[arg(short, long)]
    pub candidate: Option<String>,

    /// URL of already-running baseline harness (skips git/build)
    #[arg(long, conflicts_with = "baseline", requires = "candidate_url")]
    pub baseline_url: Option<String>,

    /// URL of already-running candidate harness (skips git/build)
    #[arg(long, conflicts_with = "candidate", requires = "baseline_url")]
    pub candidate_url: Option<String>,

    /// Print harness stdout/stderr for debugging
    #[arg(long)]
    pub harness_output: bool,

    /// Confidence level for statistical tests (0.0-1.0)
    #[arg(long)]
    pub confidence_level: Option<f64>,

    /// Number of sample iterations per benchmark
    #[arg(long)]
    pub sample_size: Option<u32>,

    /// Target minimum elapsed per sample in milliseconds (calibration target)
    #[arg(long)]
    pub target_sample_ms: Option<u64>,

    /// Path to config file
    #[arg(long, default_value = ".hypobench.toml")]
    pub config: String,

    /// Path to project within repo (for monorepos/subdirectories)
    #[arg(long)]
    pub project_path: Option<PathBuf>,

    /// Specific bench target(s) to build and run (repeatable)
    #[arg(long)]
    pub bench: Vec<String>,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Report format for stdout.
    #[arg(long, value_enum, default_value_t = ReportFormat::Terminal)]
    pub format: ReportFormat,
}

#[derive(Debug, Args, Clone)]
pub struct ReportArgs {
    /// Path to a JSON report file produced by `hypobench run --format json`.
    /// Use `-` for stdin.
    #[arg(long = "in")]
    pub input: PathBuf,

    /// Output format.
    #[arg(long, value_enum, default_value_t = ReportFormat::Terminal)]
    pub format: ReportFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReportFormat {
    Terminal,
    /// GitHub PR comment: collapsible table, pinned regressions/improvements.
    GithubPrComment,
    Json,
}

impl Cli {
    /// If this invocation is a run (either the `run` subcommand explicitly, or
    /// the default no-subcommand path), return the run args. `None` if the user
    /// asked for a different subcommand.
    pub fn as_run_args(&self) -> Option<&RunArgs> {
        match &self.command {
            None => Some(&self.run),
            Some(Command::Run(args)) => Some(args),
            Some(Command::Report(_)) => None,
        }
    }
}

impl RunArgs {
    /// Check if we're in manual URL mode (connecting to pre-running harnesses)
    pub fn is_manual_mode(&self) -> bool {
        self.baseline_url.is_some() && self.candidate_url.is_some()
    }

    /// Validate that either git-mode args or manual-mode args are present.
    pub fn validate(&self) -> Result<(), String> {
        if self.is_manual_mode() {
            return Ok(());
        }
        if self.baseline.is_none() || self.candidate.is_none() {
            return Err(
                "must supply --baseline and --candidate, or --baseline-url and --candidate-url"
                    .to_string(),
            );
        }
        Ok(())
    }

    /// Apply CLI overrides to the configuration.
    pub fn apply_to_config(&self, config: &mut Config) {
        if let Some(confidence_level) = self.confidence_level {
            config.hypothesis.confidence_level = confidence_level;
        }
        if let Some(sample_size) = self.sample_size {
            config.orchestration.sample_size = sample_size;
        }
        if let Some(target_sample_ms) = self.target_sample_ms {
            config.orchestration.target_sample_ms = target_sample_ms;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_run_args() -> RunArgs {
        RunArgs {
            baseline: None,
            candidate: None,
            baseline_url: None,
            candidate_url: None,
            harness_output: false,
            confidence_level: None,
            sample_size: None,
            target_sample_ms: None,
            config: ".hypobench.toml".to_string(),
            project_path: None,
            bench: vec![],
            verbose: false,
            format: ReportFormat::Terminal,
        }
    }

    #[test]
    fn test_apply_to_config_with_overrides() {
        let mut args = default_run_args();
        args.baseline = Some("main".to_string());
        args.candidate = Some("feature".to_string());
        args.confidence_level = Some(0.99);
        args.sample_size = Some(200);
        args.target_sample_ms = Some(20);

        let mut config = Config::default();
        args.apply_to_config(&mut config);

        assert_eq!(config.hypothesis.confidence_level, 0.99);
        assert_eq!(config.orchestration.sample_size, 200);
        assert_eq!(config.orchestration.target_sample_ms, 20);
    }

    #[test]
    fn test_default_subcommand_parses_legacy_flags() {
        let cli = Cli::parse_from(["hypobench", "--baseline", "main", "--candidate", "HEAD"]);
        let run = cli.as_run_args().expect("run mode");
        assert_eq!(run.baseline.as_deref(), Some("main"));
        assert_eq!(run.candidate.as_deref(), Some("HEAD"));
    }

    #[test]
    fn test_cli_format_defaults_to_terminal() {
        let cli = Cli::parse_from(["hypobench", "--baseline", "main", "--candidate", "HEAD"]);
        let run = cli.as_run_args().expect("run mode");
        assert_eq!(run.format, ReportFormat::Terminal);
    }

    #[test]
    fn test_cli_format_json() {
        let cli = Cli::parse_from([
            "hypobench",
            "--baseline",
            "main",
            "--candidate",
            "HEAD",
            "--format",
            "json",
        ]);
        let run = cli.as_run_args().expect("run mode");
        assert_eq!(run.format, ReportFormat::Json);
    }

    #[test]
    fn test_cli_report_subcommand() {
        let cli = Cli::parse_from([
            "hypobench",
            "report",
            "--in",
            "results.json",
            "--format",
            "github-pr-comment",
        ]);
        match cli.command {
            Some(Command::Report(args)) => {
                assert_eq!(args.input, PathBuf::from("results.json"));
                assert_eq!(args.format, ReportFormat::GithubPrComment);
            }
            _ => panic!("expected Report subcommand"),
        }
    }

    #[test]
    fn test_cli_report_subcommand_stdin() {
        let cli = Cli::parse_from(["hypobench", "report", "--in", "-", "--format", "terminal"]);
        match cli.command {
            Some(Command::Report(args)) => {
                assert_eq!(args.input, PathBuf::from("-"));
                assert_eq!(args.format, ReportFormat::Terminal);
            }
            _ => panic!("expected Report subcommand"),
        }
    }

    #[test]
    fn test_cli_manual_mode_parses() {
        let cli = Cli::parse_from([
            "hypobench",
            "--baseline-url",
            "http://localhost:9100",
            "--candidate-url",
            "http://localhost:9101",
        ]);
        let run = cli.as_run_args().expect("run mode");
        assert!(run.is_manual_mode());
        assert_eq!(run.baseline_url.as_deref(), Some("http://localhost:9100"));
    }

    #[test]
    fn test_cli_bench_targets() {
        let cli = Cli::parse_from([
            "hypobench",
            "--baseline",
            "main",
            "--candidate",
            "HEAD",
            "--bench",
            "ch_bench_foo",
            "--bench",
            "ch_bench_bar",
        ]);
        let run = cli.as_run_args().expect("run mode");
        assert_eq!(run.bench, vec!["ch_bench_foo", "ch_bench_bar"]);
    }

    #[test]
    fn test_run_args_validate_rejects_missing() {
        let args = default_run_args();
        assert!(args.validate().is_err());
    }

    #[test]
    fn test_run_args_validate_accepts_git_mode() {
        let mut args = default_run_args();
        args.baseline = Some("main".into());
        args.candidate = Some("HEAD".into());
        assert!(args.validate().is_ok());
    }

    #[test]
    fn test_run_args_validate_accepts_manual_mode() {
        let mut args = default_run_args();
        args.baseline_url = Some("http://localhost:9100".into());
        args.candidate_url = Some("http://localhost:9101".into());
        assert!(args.validate().is_ok());
    }
}
