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
    /// Lower bound of the bootstrap confidence interval on the relative mean change,
    /// in percent. Same sign convention as `effect_size`.
    pub change_ci_low: f64,
    /// Upper bound of the bootstrap confidence interval on the relative mean change,
    /// in percent. Same sign convention as `effect_size`.
    pub change_ci_high: f64,
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

pub mod bootstrap;
mod ttest;
pub use ttest::WelchTTest;

/// Apply a Bonferroni multiple-comparisons correction to a batch of test results.
///
/// Conservative family-wise error rate (FWER) control: each individual test's
/// effective significance threshold is divided by the number of tests, so the
/// probability of *any* false-positive across the whole family stays bounded by
/// `family_alpha`.
///
/// When we run 66 benches at α=0.05 each, expected false positives ≈ 3.3. With
/// Bonferroni we use α' = 0.05/66 ≈ 7.6e-4 per test instead, reducing the
/// expected family-wise FP count to ~0.05.
///
/// This is conservative — it may mask small real effects — but combined with
/// a practical-significance effect-size gate it produces substantially cleaner
/// per-PR bench output. Mutates each `TestResult` in place: re-evaluates
/// `statistically_significant` with the corrected threshold, and clears
/// `winner` to `None` for any result that flips from significant to not.
///
/// # Arguments
/// * `results` — the results to adjust, one per benchmark comparison.
/// * `family_alpha` — the family-wise α (e.g. `1.0 - confidence_level`; for
///   a 95% confidence level this is `0.05`).
///
/// # Behaviour
/// * Empty slice or single-element slice: no-op (no correction needed).
/// * Results already not-significant are unaffected.
/// * Results flipped from significant → not-significant also have their
///   `winner` cleared. `p_value` and `effect_size` are not modified.
pub fn apply_bonferroni(results: &mut [TestResult], family_alpha: f64) {
    let n = results.len();
    if n <= 1 {
        return;
    }
    let per_test_alpha = family_alpha / n as f64;
    for result in results.iter_mut() {
        if !result.statistically_significant {
            continue;
        }
        if result.p_value >= per_test_alpha {
            result.statistically_significant = false;
            result.winner = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(p_value: f64, significant: bool) -> TestResult {
        TestResult {
            p_value,
            statistically_significant: significant,
            effect_size: 5.0,
            change_ci_low: 4.0,
            change_ci_high: 6.0,
            confidence_level: 0.95,
            winner: if significant {
                Some(Side::Candidate)
            } else {
                None
            },
            baseline_mean_ns: 100.0,
            candidate_mean_ns: 95.0,
        }
    }

    #[test]
    fn test_bonferroni_empty_is_noop() {
        let mut results: Vec<TestResult> = Vec::new();
        apply_bonferroni(&mut results, 0.05);
        assert!(results.is_empty());
    }

    #[test]
    fn test_bonferroni_single_is_noop() {
        // Single-element families get no correction — there's nothing to guard
        // against. A p=0.04 result stays significant.
        let mut results = vec![result(0.04, true)];
        apply_bonferroni(&mut results, 0.05);
        assert!(results[0].statistically_significant);
        assert!(results[0].winner.is_some());
    }

    #[test]
    fn test_bonferroni_gates_marginal_results() {
        // 10 tests, α_family = 0.05 → α' = 0.005.
        // p=0.04 results should flip; p=0.001 should survive.
        let mut results = vec![
            result(0.04, true),  // marginal, should flip
            result(0.04, true),  // marginal, should flip
            result(0.04, true),  // marginal, should flip
            result(0.04, true),  // marginal, should flip
            result(0.04, true),  // marginal, should flip
            result(0.001, true), // strong, should survive
            result(0.001, true), // strong, should survive
            result(0.2, false),  // never significant; untouched
            result(0.2, false),  // never significant; untouched
            result(0.2, false),  // never significant; untouched
        ];
        apply_bonferroni(&mut results, 0.05);
        assert!(!results[0].statistically_significant);
        assert!(results[0].winner.is_none());
        assert!(!results[4].statistically_significant);
        assert!(results[5].statistically_significant);
        assert!(results[6].statistically_significant);
        // Originally non-significant rows are unchanged.
        for r in &results[7..] {
            assert!(!r.statistically_significant);
        }
    }

    #[test]
    fn test_bonferroni_does_not_touch_p_or_effect_size() {
        let mut results = vec![result(0.04, true); 10];
        let p_before = results[0].p_value;
        let effect_before = results[0].effect_size;
        apply_bonferroni(&mut results, 0.05);
        assert_eq!(results[0].p_value, p_before);
        assert_eq!(results[0].effect_size, effect_before);
    }
}
