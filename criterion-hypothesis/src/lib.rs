//! criterion-hypothesis: Statistically rigorous A/B testing of benchmarks
//!
//! This library provides tools for comparing benchmark performance between
//! two commits using interleaved execution and hypothesis testing.

pub mod build;
pub mod cli;
pub mod config;
pub mod orchestrator;
pub mod protocol;
pub mod report;
pub mod source;
pub mod stats;

// Re-export main types
pub use build::BuildManager;
pub use cli::Cli;
pub use config::Config;
pub use orchestrator::{BenchmarkSamples, Orchestrator};
pub use report::{BenchmarkComparison, Reporter, SampleStats, TerminalReporter};
pub use source::{GitWorktreeProvider, SourceProvider};
pub use stats::{Side, StatisticalTest, TestResult, WelchTTest};
