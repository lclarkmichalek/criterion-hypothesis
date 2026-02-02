//! criterion-hypothesis: Statistically rigorous A/B testing of benchmarks
//!
//! This library provides tools for comparing benchmark performance between
//! two commits using interleaved execution and hypothesis testing.

pub mod build;
pub mod cli;
pub mod config;
pub mod orchestrator;
pub mod source;

// Re-export core types for convenience
pub use criterion_hypothesis_core::protocol;
pub use criterion_hypothesis_core::report::{
    BenchmarkComparison, ReportError, Reporter, SampleStats, TerminalReporter,
};
pub use criterion_hypothesis_core::stats::{Side, StatisticalTest, TestResult, WelchTTest};

// Re-export main types from this crate
pub use build::BuildManager;
pub use cli::Cli;
pub use config::Config;
pub use orchestrator::{
    run_with_urls, BenchmarkSamples, HarnessHandle, Orchestrator, OrchestratorError,
};
pub use source::{GitWorktreeProvider, SourceProvider};
