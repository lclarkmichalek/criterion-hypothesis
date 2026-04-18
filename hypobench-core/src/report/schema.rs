//! Versioned, serde-friendly schema for benchmark reports.
//!
//! This is the stable artifact that both the CLI renderers and any downstream
//! consumers (web UIs, dashboards) read. Bump `schema_version` on any breaking
//! shape change.

use super::BenchmarkComparison;

/// A complete benchmark comparison report, ready to serialize to JSON.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Report {
    /// Schema version for this report format. Bumped on breaking changes.
    pub schema_version: String,
    /// Metadata about how the report was produced.
    pub metadata: ReportMetadata,
    /// The per-benchmark comparisons.
    pub comparisons: Vec<BenchmarkComparison>,
}

/// Metadata describing the context in which a report was produced.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReportMetadata {
    /// Version of hypobench that produced the report (from `env!("CARGO_PKG_VERSION")`).
    pub hypobench_version: String,
    /// RFC 3339 UTC timestamp of when the report was generated.
    pub generated_at: String,
    /// Git ref (sha or branch) for the baseline.
    pub baseline_ref: String,
    /// Git ref (sha or branch) for the candidate.
    pub candidate_ref: String,
    /// Snapshot of the statistical configuration used.
    pub config: ConfigSnapshot,
}

/// Statistical configuration captured at report time.
///
/// Kept explicit (rather than embedding the full `hypobench::Config`) so the
/// JSON schema can evolve independently of the CLI's internal config shape.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConfigSnapshot {
    /// Confidence level used for statistical tests.
    pub confidence_level: f64,
    /// Minimum effect size (percent) for practical significance.
    pub minimum_effect_size: f64,
    /// Number of samples per benchmark.
    pub sample_size: u32,
    /// Whether Bonferroni correction was applied.
    pub correct_multiple_comparisons: bool,
}

impl Report {
    /// Current schema version emitted by this hypobench build.
    pub const CURRENT_SCHEMA_VERSION: &'static str = "1";
}
