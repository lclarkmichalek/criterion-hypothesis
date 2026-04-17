use std::time::Duration;

/// Identifies which side of a comparison (baseline or candidate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Baseline,
    Candidate,
}

/// The result of a statistical comparison between baseline and candidate measurements.
#[derive(Debug, Clone)]
pub struct TestResult {
    /// The p-value from the statistical test (probability of observing the difference by chance).
    pub p_value: f64,
    /// Whether the difference is statistically significant at the configured confidence level.
    pub statistically_significant: bool,
    /// Effect size as percent difference (positive = candidate is faster than baseline).
    pub effect_size: f64,
    /// The confidence level used for the test (e.g., 0.95 for 95% confidence).
    pub confidence_level: f64,
    /// The winner if statistically significant, None if no significant difference.
    pub winner: Option<Side>,
    /// Mean of baseline measurements in nanoseconds.
    pub baseline_mean_ns: f64,
    /// Mean of candidate measurements in nanoseconds.
    pub candidate_mean_ns: f64,
}

/// Trait for statistical tests that compare two sets of measurements.
pub trait StatisticalTest: Send + Sync {
    /// Analyze baseline and candidate measurements and return a statistical test result.
    fn analyze(&self, baseline: &[Duration], candidate: &[Duration]) -> TestResult;
}

mod ttest;
pub use ttest::WelchTTest;
