//! Core types and utilities for hypobench.
//!
//! This crate provides shared types used by both the hypobench CLI
//! and the hypobench-harness runtime, ensuring protocol compatibility.

pub mod protocol;
pub mod report;
pub mod stats;

// Re-export main types for convenience
pub use protocol::{
    BenchmarkListResponse, HealthResponse, RunIterationRequest, RunIterationResponse,
    ShutdownResponse,
};
pub use report::{BenchmarkComparison, ConfigSnapshot, Report, ReportMetadata, SampleStats};
pub use stats::{Side, StatisticalTest, TestResult, WelchTTest};
