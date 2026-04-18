use std::time::Duration;

use rand::rngs::SmallRng;
use rand::SeedableRng;
use statrs::distribution::{ContinuousCDF, StudentsT};

use super::bootstrap::bootstrap_change_ci;
use super::{Side, StatisticalTest, TestResult};

/// Seed used for the bootstrap RNG inside `WelchTTest::analyze`. Fixed so that
/// results are reproducible on identical sample inputs across runs.
const BOOTSTRAP_SEED: u64 = 0xC0FFEE;

/// Number of bootstrap resamples to compute the change CI with. 10_000 is the
/// conventional default for percentile bootstrap; enough to stabilise the tail
/// percentile estimates without blowing up wall time (~ms per benchmark).
const BOOTSTRAP_N_RESAMPLES: usize = 10_000;

/// Welch's t-test for comparing two independent samples with potentially unequal variances.
///
/// This is the recommended t-test variant for benchmark comparisons because it does not
/// assume equal variances between the baseline and candidate measurements.
#[derive(Debug, Clone)]
pub struct WelchTTest {
    /// The confidence level for determining statistical significance (default: 0.95).
    pub confidence_level: f64,
    /// Minimum absolute effect size (as a percentage, matching `TestResult::effect_size`)
    /// required for a result to be flagged as statistically significant. Defaults to
    /// `0.0` which preserves the pre-existing behaviour of gating solely on the p-value.
    ///
    /// When non-zero, a result whose |effect_size| falls below this threshold has its
    /// `statistically_significant` flag forced to `false` and its `winner` cleared to
    /// `None`, even if `p < alpha`. This avoids flagging tiny cross-binary differences
    /// (branch-predictor drift, cache-line alignment, etc.) on large benchmark suites
    /// where small p-values are easy to achieve with moderate sample sizes.
    pub minimum_effect_size: f64,
}

impl Default for WelchTTest {
    fn default() -> Self {
        Self {
            confidence_level: 0.95,
            minimum_effect_size: 0.0,
        }
    }
}

impl WelchTTest {
    /// Create a new Welch's t-test with the specified confidence level.
    ///
    /// # Arguments
    /// * `confidence_level` - The confidence level (e.g., 0.95 for 95% confidence).
    ///
    /// # Panics
    /// Panics if confidence_level is not in the range (0, 1).
    pub fn new(confidence_level: f64) -> Self {
        assert!(
            confidence_level > 0.0 && confidence_level < 1.0,
            "confidence_level must be between 0 and 1 (exclusive)"
        );
        Self {
            confidence_level,
            minimum_effect_size: 0.0,
        }
    }

    /// Set the minimum absolute effect size (percent) required for a result to count
    /// as statistically significant. See [`Self::minimum_effect_size`].
    pub fn with_minimum_effect_size(mut self, threshold: f64) -> Self {
        assert!(threshold >= 0.0, "minimum_effect_size must be non-negative");
        self.minimum_effect_size = threshold;
        self
    }

    /// Calculate the sample mean of durations in nanoseconds.
    fn mean_ns(samples: &[Duration]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = samples.iter().map(|d| d.as_nanos() as f64).sum();
        sum / samples.len() as f64
    }

    /// Calculate the sample variance of durations in nanoseconds.
    /// Uses Bessel's correction (n-1 denominator) for unbiased estimation.
    fn variance_ns(samples: &[Duration], mean: f64) -> f64 {
        if samples.len() < 2 {
            return 0.0;
        }
        let sum_sq_diff: f64 = samples
            .iter()
            .map(|d| {
                let diff = d.as_nanos() as f64 - mean;
                diff * diff
            })
            .sum();
        sum_sq_diff / (samples.len() - 1) as f64
    }

    /// Calculate degrees of freedom using the Welch-Satterthwaite equation.
    ///
    /// df = (var1/n1 + var2/n2)^2 / ((var1/n1)^2/(n1-1) + (var2/n2)^2/(n2-1))
    fn welch_satterthwaite_df(var1: f64, n1: usize, var2: f64, n2: usize) -> f64 {
        let s1 = var1 / n1 as f64;
        let s2 = var2 / n2 as f64;
        let numerator = (s1 + s2).powi(2);
        let denominator = (s1.powi(2) / (n1 - 1) as f64) + (s2.powi(2) / (n2 - 1) as f64);

        if denominator == 0.0 {
            // Fallback to minimum df when variances are zero
            return (n1.min(n2) - 1) as f64;
        }

        numerator / denominator
    }
}

impl StatisticalTest for WelchTTest {
    fn analyze(&self, baseline: &[Duration], candidate: &[Duration]) -> TestResult {
        let n1 = baseline.len();
        let n2 = candidate.len();

        // Calculate means
        let mean1 = Self::mean_ns(baseline);
        let mean2 = Self::mean_ns(candidate);

        // Handle edge cases with insufficient data
        if n1 < 2 || n2 < 2 {
            return TestResult {
                p_value: 1.0,
                statistically_significant: false,
                effect_size: 0.0,
                change_ci_low: 0.0,
                change_ci_high: 0.0,
                confidence_level: self.confidence_level,
                winner: None,
                baseline_mean_ns: mean1,
                candidate_mean_ns: mean2,
            };
        }

        // Calculate variances
        let var1 = Self::variance_ns(baseline, mean1);
        let var2 = Self::variance_ns(candidate, mean2);

        // Calculate standard error of the difference
        let se = (var1 / n1 as f64 + var2 / n2 as f64).sqrt();

        // Handle case where both samples have zero variance
        if se == 0.0 {
            let effect_size = if mean1 != 0.0 {
                ((mean1 - mean2) / mean1) * 100.0
            } else {
                0.0
            };

            // Even with zero variance, the practical-significance gate applies:
            // two samples that differ by 0.01% aren't meaningful signal.
            let meaningful_diff = mean1 != mean2 && effect_size.abs() >= self.minimum_effect_size;

            let winner = if meaningful_diff {
                if mean1 > mean2 {
                    Some(Side::Candidate)
                } else {
                    Some(Side::Baseline)
                }
            } else {
                None
            };

            return TestResult {
                p_value: if mean1 == mean2 { 1.0 } else { 0.0 },
                statistically_significant: meaningful_diff,
                effect_size,
                // Zero-variance case: no noise to bootstrap over; CI collapses to the point estimate.
                change_ci_low: effect_size,
                change_ci_high: effect_size,
                confidence_level: self.confidence_level,
                winner,
                baseline_mean_ns: mean1,
                candidate_mean_ns: mean2,
            };
        }

        // Calculate Welch's t-statistic
        // t = (mean1 - mean2) / sqrt(var1/n1 + var2/n2)
        let t_statistic = (mean1 - mean2) / se;

        // Calculate degrees of freedom using Welch-Satterthwaite equation
        let df = Self::welch_satterthwaite_df(var1, n1, var2, n2);

        // Calculate two-tailed p-value from t-distribution
        let p_value = match StudentsT::new(0.0, 1.0, df) {
            Ok(t_dist) => {
                // Two-tailed test: p = 2 * P(T > |t|)
                2.0 * (1.0 - t_dist.cdf(t_statistic.abs()))
            }
            Err(_) => 1.0, // Conservative fallback if distribution creation fails
        };

        // Calculate effect size as percentage difference
        // Positive effect_size means candidate is faster (lower time)
        let effect_size = if mean1 != 0.0 {
            ((mean1 - mean2) / mean1) * 100.0
        } else {
            0.0
        };

        // Determine statistical significance. Two gates must pass:
        //   1. p-value below alpha (standard Welch's test)
        //   2. |effect_size| >= minimum_effect_size (practical-significance gate)
        // The second gate is a no-op when `minimum_effect_size == 0.0` (default).
        let alpha = 1.0 - self.confidence_level;
        let statistically_significant =
            p_value < alpha && effect_size.abs() >= self.minimum_effect_size;

        // Determine winner if statistically significant
        // Lower time is better, so:
        // - If mean1 > mean2: candidate is faster -> Candidate wins
        // - If mean2 > mean1: baseline is faster -> Baseline wins
        let winner = if statistically_significant {
            if mean1 > mean2 {
                Some(Side::Candidate)
            } else {
                Some(Side::Baseline)
            }
        } else {
            None
        };

        // Non-parametric bootstrap CI on the relative mean change. Uses a fixed
        // seed for reproducibility across runs on identical inputs.
        let mut rng = SmallRng::seed_from_u64(BOOTSTRAP_SEED);
        let (change_ci_low, change_ci_high) = bootstrap_change_ci(
            baseline,
            candidate,
            BOOTSTRAP_N_RESAMPLES,
            self.confidence_level,
            &mut rng,
        );

        TestResult {
            p_value,
            statistically_significant,
            effect_size,
            change_ci_low,
            change_ci_high,
            confidence_level: self.confidence_level,
            winner,
            baseline_mean_ns: mean1,
            candidate_mean_ns: mean2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn durations_from_nanos(nanos: &[u64]) -> Vec<Duration> {
        nanos.iter().map(|&n| Duration::from_nanos(n)).collect()
    }

    #[test]
    fn test_identical_samples() {
        let test = WelchTTest::default();
        let baseline = durations_from_nanos(&[100, 100, 100, 100, 100]);
        let candidate = durations_from_nanos(&[100, 100, 100, 100, 100]);

        let result = test.analyze(&baseline, &candidate);

        assert!(!result.statistically_significant);
        assert!(result.winner.is_none());
        assert_eq!(result.effect_size, 0.0);
    }

    #[test]
    fn test_clearly_different_samples() {
        let test = WelchTTest::default();
        // Baseline is much slower (higher values)
        let baseline = durations_from_nanos(&[1000, 1001, 1002, 999, 1000]);
        // Candidate is much faster (lower values)
        let candidate = durations_from_nanos(&[100, 101, 102, 99, 100]);

        let result = test.analyze(&baseline, &candidate);

        assert!(result.statistically_significant);
        assert_eq!(result.winner, Some(Side::Candidate));
        assert!(result.effect_size > 0.0); // Positive because candidate is faster
        assert!(result.p_value < 0.05);
    }

    #[test]
    fn test_candidate_slower() {
        let test = WelchTTest::default();
        // Baseline is faster
        let baseline = durations_from_nanos(&[100, 101, 102, 99, 100]);
        // Candidate is slower
        let candidate = durations_from_nanos(&[1000, 1001, 1002, 999, 1000]);

        let result = test.analyze(&baseline, &candidate);

        assert!(result.statistically_significant);
        assert_eq!(result.winner, Some(Side::Baseline));
        assert!(result.effect_size < 0.0); // Negative because candidate is slower
    }

    #[test]
    fn test_insufficient_samples() {
        let test = WelchTTest::default();
        let baseline = durations_from_nanos(&[100]);
        let candidate = durations_from_nanos(&[200]);

        let result = test.analyze(&baseline, &candidate);

        assert!(!result.statistically_significant);
        assert!(result.winner.is_none());
        assert_eq!(result.p_value, 1.0);
    }

    #[test]
    fn test_custom_confidence_level() {
        let test = WelchTTest::new(0.99);
        assert_eq!(test.confidence_level, 0.99);
    }

    #[test]
    #[should_panic(expected = "confidence_level must be between 0 and 1")]
    fn test_invalid_confidence_level() {
        WelchTTest::new(1.5);
    }

    #[test]
    fn test_effect_size_calculation() {
        let test = WelchTTest::default();
        // 50% improvement: baseline=200, candidate=100
        let baseline = durations_from_nanos(&[200, 200, 200, 200, 200]);
        let candidate = durations_from_nanos(&[100, 100, 100, 100, 100]);

        let result = test.analyze(&baseline, &candidate);

        // Effect size should be approximately 50% (candidate 50% faster)
        assert!((result.effect_size - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_minimum_effect_size_gate_blocks_tiny_effects() {
        // Baseline ~1000ns, candidate ~1015ns — ~1.5% difference with low noise.
        // Welch's will happily report p < 0.05 because SE is tiny, but the effect
        // is practically meaningless. Applying a 2% gate must flip significance to
        // false.
        let baseline = durations_from_nanos(&[1000, 1001, 999, 1000, 1001, 999, 1000, 1001]);
        let candidate = durations_from_nanos(&[1015, 1016, 1014, 1015, 1016, 1014, 1015, 1016]);

        let ungated = WelchTTest::default().analyze(&baseline, &candidate);
        assert!(
            ungated.statistically_significant,
            "without a gate, this tiny effect should look significant: p={}, effect={}%",
            ungated.p_value, ungated.effect_size
        );
        assert!(ungated.effect_size.abs() < 2.0);

        let gated = WelchTTest::new(0.95)
            .with_minimum_effect_size(2.0)
            .analyze(&baseline, &candidate);
        assert!(
            !gated.statistically_significant,
            "gate should have blocked effect={}%",
            gated.effect_size
        );
        assert!(gated.winner.is_none());
        // p-value and effect_size are unchanged — gate only toggles the flags.
        assert_eq!(gated.p_value, ungated.p_value);
        assert_eq!(gated.effect_size, ungated.effect_size);
    }

    #[test]
    fn test_minimum_effect_size_gate_passes_large_effects() {
        // 50% difference. Should still be flagged significant with a 2% gate.
        let baseline = durations_from_nanos(&[200, 200, 200, 200, 200]);
        let candidate = durations_from_nanos(&[100, 100, 100, 100, 100]);

        let result = WelchTTest::new(0.95)
            .with_minimum_effect_size(2.0)
            .analyze(&baseline, &candidate);
        assert!(result.statistically_significant);
        assert_eq!(result.winner, Some(Side::Candidate));
    }

    #[test]
    #[should_panic(expected = "minimum_effect_size must be non-negative")]
    fn test_invalid_minimum_effect_size() {
        let _ = WelchTTest::default().with_minimum_effect_size(-1.0);
    }
}
