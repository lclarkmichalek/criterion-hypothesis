//! Core types and utilities for criterion-hypothesis.
//!
//! This crate provides shared types used by both the criterion-hypothesis CLI
//! and the criterion-hypothesis-harness runtime, ensuring protocol compatibility.

pub mod protocol;
pub mod report;
pub mod stats;

// Re-export main types for convenience
pub use protocol::{
    BenchmarkListResponse, HealthResponse, RunIterationRequest, RunIterationResponse,
    ShutdownResponse,
};
pub use report::{BenchmarkComparison, ReportError, Reporter, SampleStats, TerminalReporter};
pub use stats::{Side, StatisticalTest, TestResult, WelchTTest};
