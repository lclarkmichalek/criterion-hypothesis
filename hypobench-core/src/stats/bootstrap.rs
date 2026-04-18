//! Non-parametric bootstrap CI on the relative mean difference between two
//! sample sets.
//!
//! Given baseline and candidate sample vectors, resample each with replacement
//! `n_resamples` times (independently), compute the per-resample
//! `(mean(baseline') - mean(candidate')) / mean(baseline') * 100.0` (percent
//! change; positive means candidate is faster — same sign convention as
//! [`crate::stats::TestResult::effect_size`]), sort the resulting distribution,
//! and return the `(confidence_level / 2)` and `1 - (confidence_level / 2)`
//! percentiles.
//!
//! This is a direct complement to the p-value produced by Welch's t-test: a p
//! = 0.0001 effect with a change CI of `[-3.47%, -2.94%, -2.44%]` tells a very
//! different story from the same p-value with `[-0.5%, -0.1%, +0.3%]`.

use rand::seq::IndexedRandom;
use rand::Rng;
use std::time::Duration;

/// Compute a non-parametric bootstrap confidence interval on the relative
/// mean change between baseline and candidate samples.
///
/// Returns `(ci_low, ci_high)` in percent. Same sign convention as
/// [`crate::stats::TestResult::effect_size`]: positive values mean the
/// candidate is faster than the baseline.
///
/// # Arguments
/// * `baseline`, `candidate` — the sample sets to compare.
/// * `n_resamples` — number of bootstrap iterations. 10_000 is a reasonable
///   default; below ~1000 the percentile estimates get noisy.
/// * `confidence` — the desired confidence level (e.g. `0.95` for a 95% CI).
///   Must be in `(0, 1)`.
/// * `rng` — a random number generator. Pass a seeded RNG for reproducibility.
///
/// # Edge cases
/// * Either side has fewer than 2 samples → returns the point estimate
///   `(point, point)` (CI collapses, since resampling a 0- or 1-element
///   vector is degenerate).
/// * Baseline mean is zero → returns `(0.0, 0.0)` (the relative change is
///   undefined; pick the least-wrong thing).
/// * `n_resamples == 0` → also returns the point estimate.
///
/// # Panics
/// Panics if `confidence` is not in `(0, 1)`.
pub fn bootstrap_change_ci<R: Rng + ?Sized>(
    baseline: &[Duration],
    candidate: &[Duration],
    n_resamples: usize,
    confidence: f64,
    rng: &mut R,
) -> (f64, f64) {
    assert!(
        confidence > 0.0 && confidence < 1.0,
        "confidence must be between 0 and 1 (exclusive)"
    );

    let base_mean = mean_ns(baseline);
    let cand_mean = mean_ns(candidate);
    let point_estimate = relative_change(base_mean, cand_mean);

    if baseline.len() < 2 || candidate.len() < 2 || n_resamples == 0 {
        return (point_estimate, point_estimate);
    }
    if base_mean == 0.0 {
        return (0.0, 0.0);
    }

    // Convert samples to ns-f64 up front so resampling just copies floats.
    let baseline_ns: Vec<f64> = baseline.iter().map(|d| d.as_nanos() as f64).collect();
    let candidate_ns: Vec<f64> = candidate.iter().map(|d| d.as_nanos() as f64).collect();

    let mut diffs: Vec<f64> = Vec::with_capacity(n_resamples);
    for _ in 0..n_resamples {
        let b_mean = resample_mean(&baseline_ns, rng);
        let c_mean = resample_mean(&candidate_ns, rng);
        diffs.push(relative_change(b_mean, c_mean));
    }

    diffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Equal-tailed percentile CI.
    let tail = (1.0 - confidence) / 2.0;
    let lo = percentile(&diffs, tail);
    let hi = percentile(&diffs, 1.0 - tail);
    (lo, hi)
}

/// Sample mean in nanoseconds. Returns 0.0 for an empty slice.
fn mean_ns(samples: &[Duration]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|d| d.as_nanos() as f64).sum();
    sum / samples.len() as f64
}

/// Relative change from baseline to candidate, in percent.
/// Positive means candidate is faster (same sign as `TestResult::effect_size`).
fn relative_change(base_mean: f64, cand_mean: f64) -> f64 {
    if base_mean == 0.0 {
        0.0
    } else {
        ((base_mean - cand_mean) / base_mean) * 100.0
    }
}

/// Resample `data` with replacement to the original length and return the mean.
fn resample_mean<R: Rng + ?Sized>(data: &[f64], rng: &mut R) -> f64 {
    let n = data.len();
    let mut sum = 0.0;
    for _ in 0..n {
        // `choose` on a slice samples with replacement equivalently when called
        // repeatedly; amortised O(1) per draw.
        sum += *data.choose(rng).expect("data is non-empty (guarded above)");
    }
    sum / n as f64
}

/// Given a pre-sorted vector, return the value at `fraction` (0..=1) using
/// linear interpolation between adjacent elements.
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let f = fraction.clamp(0.0, 1.0);
    let pos = f * (sorted.len() - 1) as f64;
    let lo_idx = pos.floor() as usize;
    let hi_idx = pos.ceil() as usize;
    if lo_idx == hi_idx {
        return sorted[lo_idx];
    }
    let frac = pos - lo_idx as f64;
    sorted[lo_idx] * (1.0 - frac) + sorted[hi_idx] * frac
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    fn durations(nanos: &[u64]) -> Vec<Duration> {
        nanos.iter().map(|&n| Duration::from_nanos(n)).collect()
    }

    fn seeded() -> SmallRng {
        SmallRng::seed_from_u64(0xC0FFEE)
    }

    #[test]
    fn identical_samples_ci_contains_zero() {
        // Both sides drawn from the same distribution — CI on relative change
        // should straddle zero with high probability.
        let baseline = durations(&[1000, 1002, 999, 1001, 998, 1000, 1003, 997]);
        let candidate = durations(&[1000, 1001, 999, 1002, 998, 1000, 1002, 998]);
        let (lo, hi) = bootstrap_change_ci(&baseline, &candidate, 5_000, 0.95, &mut seeded());
        assert!(lo <= 0.0 && hi >= 0.0, "CI [{lo}, {hi}] should contain 0");
    }

    #[test]
    fn clearly_faster_candidate_ci_excludes_zero() {
        // Candidate is 10x faster, both sides low-variance. CI should sit
        // entirely in the positive (candidate-faster) region.
        let baseline = durations(&[1000, 1001, 999, 1000, 1001, 999, 1000, 1001]);
        let candidate = durations(&[100, 101, 99, 100, 101, 99, 100, 101]);
        let (lo, hi) = bootstrap_change_ci(&baseline, &candidate, 5_000, 0.95, &mut seeded());
        assert!(
            lo > 50.0,
            "CI should be deep in positive territory, got [{lo}, {hi}]"
        );
        assert!(hi < 100.0, "sanity upper bound: got [{lo}, {hi}]");
    }

    #[test]
    fn seeded_rng_is_deterministic() {
        let baseline = durations(&[100, 110, 90, 105, 95, 100, 102, 98]);
        let candidate = durations(&[120, 130, 110, 125, 115, 120, 122, 118]);
        let (a_lo, a_hi) = bootstrap_change_ci(&baseline, &candidate, 1_000, 0.95, &mut seeded());
        let (b_lo, b_hi) = bootstrap_change_ci(&baseline, &candidate, 1_000, 0.95, &mut seeded());
        assert_eq!(a_lo, b_lo);
        assert_eq!(a_hi, b_hi);
    }

    #[test]
    fn too_few_samples_returns_point_estimate() {
        let baseline = durations(&[100]);
        let candidate = durations(&[200, 201]);
        let (lo, hi) = bootstrap_change_ci(&baseline, &candidate, 1_000, 0.95, &mut seeded());
        assert_eq!(lo, hi);
        // Point estimate: (100 - 200.5) / 100 * 100 = -100.5
        assert!((lo - (-100.5)).abs() < 1e-9);
    }

    #[test]
    fn zero_resamples_returns_point_estimate() {
        let baseline = durations(&[100, 110, 90]);
        let candidate = durations(&[80, 90, 70]);
        let (lo, hi) = bootstrap_change_ci(&baseline, &candidate, 0, 0.95, &mut seeded());
        assert_eq!(lo, hi);
    }

    #[test]
    fn zero_baseline_mean_yields_zero_ci() {
        let baseline = durations(&[0, 0, 0]);
        let candidate = durations(&[100, 100, 100]);
        let (lo, hi) = bootstrap_change_ci(&baseline, &candidate, 1_000, 0.95, &mut seeded());
        assert_eq!(lo, 0.0);
        assert_eq!(hi, 0.0);
    }

    #[test]
    #[should_panic(expected = "confidence must be between 0 and 1")]
    fn invalid_confidence_panics() {
        let baseline = durations(&[100, 110]);
        let candidate = durations(&[90, 100]);
        let _ = bootstrap_change_ci(&baseline, &candidate, 100, 1.5, &mut seeded());
    }
}
