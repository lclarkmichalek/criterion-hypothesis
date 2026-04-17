use std::time::Duration;

use statrs::distribution::{ContinuousCDF, StudentsT};

use super::{Side, StatisticalTest, TestResult};

/// Welch's t-test for comparing two independent samples with potentially unequal variances.
///
/// This is the recommended t-test variant for benchmark comparisons because it does not
/// assume equal variances between the baseline and candidate measurements.
#[derive(Debug, Clone)]
pub struct WelchTTest {
    /// The confidence level for determining statistical significance (default: 0.95).
    pub confidence_level: f64,
}

impl Default for WelchTTest {
    fn default() -> Self {
        Self {
            confidence_level: 0.95,
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
        Self { confidence_level }
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

            let winner = if mean1 > mean2 {
                Some(Side::Candidate)
            } else if mean2 > mean1 {
                Some(Side::Baseline)
            } else {
                None
            };

            return TestResult {
                p_value: if mean1 == mean2 { 1.0 } else { 0.0 },
                statistically_significant: mean1 != mean2,
                effect_size,
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

        // Determine statistical significance
        let alpha = 1.0 - self.confidence_level;
        let statistically_significant = p_value < alpha;

        // Calculate effect size as percentage difference
        // Positive effect_size means candidate is faster (lower time)
        let effect_size = if mean1 != 0.0 {
            ((mean1 - mean2) / mean1) * 100.0
        } else {
            0.0
        };

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

        TestResult {
            p_value,
            statistically_significant,
            effect_size,
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
}
