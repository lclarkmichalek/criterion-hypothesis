//! criterion-hypothesis: Statistically rigorous A/B testing of benchmarks
//!
//! This library provides tools for comparing benchmark performance between
//! two commits using interleaved execution and hypothesis testing.

pub mod cli;
pub mod config;
pub mod protocol;
pub mod source;
pub mod stats;
pub mod report;
pub mod orchestrator;
pub mod build;

// Re-export main types
pub use cli::Cli;
pub use config::Config;
pub use source::{SourceProvider, GitWorktreeProvider};
pub use stats::{StatisticalTest, WelchTTest, TestResult, Side};
pub use report::{Reporter, TerminalReporter, BenchmarkComparison, SampleStats};
pub use orchestrator::{Orchestrator, BenchmarkSamples};
pub use build::BuildManager;
