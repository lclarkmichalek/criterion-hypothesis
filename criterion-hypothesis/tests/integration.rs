//! Integration tests for criterion-hypothesis.
//!
//! These tests verify the interaction between the orchestrator and harness
//! without requiring git worktrees or cargo builds.

use criterion_hypothesis::{HarnessHandle, OrchestratorError};

/// Test that HarnessHandle::connect validates URLs correctly.
#[test]
fn test_harness_handle_connect_validation() {
    // Valid URLs should work
    assert!(HarnessHandle::connect("http://localhost:9100").is_ok());
    assert!(HarnessHandle::connect("https://localhost:9100").is_ok());
    assert!(HarnessHandle::connect("http://127.0.0.1:9100").is_ok());

    // Invalid URLs should fail
    let result = HarnessHandle::connect("localhost:9100");
    assert!(matches!(result, Err(OrchestratorError::InvalidUrl(_))));

    let result = HarnessHandle::connect("not-a-url");
    assert!(matches!(result, Err(OrchestratorError::InvalidUrl(_))));

    let result = HarnessHandle::connect("ftp://localhost:9100");
    assert!(matches!(result, Err(OrchestratorError::InvalidUrl(_))));
}

/// Test that trailing slashes are handled correctly.
#[test]
fn test_harness_handle_trailing_slash() {
    let handle = HarnessHandle::connect("http://localhost:9100/").unwrap();
    // The handle should normalize the URL
    assert!(!handle.is_managed());
}

/// Test that remote handles are not marked as managed.
#[test]
fn test_remote_handle_not_managed() {
    let handle = HarnessHandle::connect("http://localhost:9100").unwrap();
    assert!(!handle.is_managed());
    // pid() should return None for remote handles
    assert!(handle.pid().is_none());
}

#[cfg(test)]
mod protocol_tests {
    use criterion_hypothesis_core::protocol::*;
    use std::time::Duration;

    /// Test that protocol types serialize and deserialize correctly.
    #[test]
    fn test_health_response_roundtrip() {
        let original = HealthResponse::healthy();
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: HealthResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.status, "healthy");
    }

    #[test]
    fn test_benchmark_list_response_roundtrip() {
        let original = BenchmarkListResponse::new(vec![
            "bench1".to_string(),
            "bench2".to_string(),
            "bench3".to_string(),
        ]);
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: BenchmarkListResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.benchmarks.len(), 3);
        assert!(deserialized.benchmarks.contains(&"bench1".to_string()));
    }

    #[test]
    fn test_run_iteration_request_roundtrip() {
        let original = RunIterationRequest::new("my_benchmark");
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: RunIterationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.benchmark_id, "my_benchmark");
    }

    #[test]
    fn test_run_iteration_response_success_roundtrip() {
        let duration = Duration::from_micros(1234);
        let original = RunIterationResponse::success(duration);
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: RunIterationResponse = serde_json::from_str(&json).unwrap();

        assert!(deserialized.success);
        assert_eq!(deserialized.duration(), duration);
        assert!(deserialized.error.is_none());
    }

    #[test]
    fn test_run_iteration_response_failure_roundtrip() {
        let original = RunIterationResponse::failure("benchmark panicked");
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: RunIterationResponse = serde_json::from_str(&json).unwrap();

        assert!(!deserialized.success);
        assert_eq!(deserialized.duration_ns, 0);
        assert_eq!(deserialized.error, Some("benchmark panicked".to_string()));
    }

    #[test]
    fn test_shutdown_response_roundtrip() {
        let original = ShutdownResponse::acknowledged();
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: ShutdownResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.status, "shutting_down");
    }

    /// Test that the error field is omitted when None (for smaller JSON payloads).
    #[test]
    fn test_error_field_omitted_when_none() {
        let response = RunIterationResponse::success(Duration::from_nanos(100));
        let json = serde_json::to_string(&response).unwrap();
        assert!(!json.contains("error"));
    }

    /// Test that the error field is included when Some.
    #[test]
    fn test_error_field_included_when_some() {
        let response = RunIterationResponse::failure("some error");
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("error"));
        assert!(json.contains("some error"));
    }
}

#[cfg(test)]
mod stats_tests {
    use criterion_hypothesis::{Side, StatisticalTest, WelchTTest};
    use std::time::Duration;

    fn durations_from_nanos(nanos: &[u64]) -> Vec<Duration> {
        nanos.iter().map(|&n| Duration::from_nanos(n)).collect()
    }

    /// Test that identical samples produce no significant difference.
    #[test]
    fn test_identical_samples() {
        let test = WelchTTest::default();
        let baseline = durations_from_nanos(&[1000, 1000, 1000, 1000, 1000]);
        let candidate = durations_from_nanos(&[1000, 1000, 1000, 1000, 1000]);

        let result = test.analyze(&baseline, &candidate);

        assert!(!result.statistically_significant);
        assert!(result.winner.is_none());
        assert_eq!(result.effect_size, 0.0);
    }

    /// Test that clearly different samples are detected.
    #[test]
    fn test_clearly_different_samples_candidate_faster() {
        let test = WelchTTest::default();
        // Baseline is much slower
        let baseline = durations_from_nanos(&[10000, 10100, 10200, 9900, 10000]);
        // Candidate is much faster
        let candidate = durations_from_nanos(&[1000, 1010, 1020, 990, 1000]);

        let result = test.analyze(&baseline, &candidate);

        assert!(result.statistically_significant);
        assert_eq!(result.winner, Some(Side::Candidate));
        assert!(result.effect_size > 0.0); // Positive because candidate is faster
        assert!(result.p_value < 0.05);
    }

    /// Test that clearly different samples are detected (candidate slower).
    #[test]
    fn test_clearly_different_samples_candidate_slower() {
        let test = WelchTTest::default();
        // Baseline is faster
        let baseline = durations_from_nanos(&[1000, 1010, 1020, 990, 1000]);
        // Candidate is slower
        let candidate = durations_from_nanos(&[10000, 10100, 10200, 9900, 10000]);

        let result = test.analyze(&baseline, &candidate);

        assert!(result.statistically_significant);
        assert_eq!(result.winner, Some(Side::Baseline));
        assert!(result.effect_size < 0.0); // Negative because candidate is slower
    }

    /// Test that insufficient samples result in inconclusive result.
    #[test]
    fn test_insufficient_samples() {
        let test = WelchTTest::default();
        let baseline = durations_from_nanos(&[1000]);
        let candidate = durations_from_nanos(&[2000]);

        let result = test.analyze(&baseline, &candidate);

        assert!(!result.statistically_significant);
        assert!(result.winner.is_none());
        assert_eq!(result.p_value, 1.0);
    }

    /// Test custom confidence level.
    #[test]
    fn test_custom_confidence_level() {
        let test = WelchTTest::new(0.99);
        assert_eq!(test.confidence_level, 0.99);
    }

    /// Test effect size calculation (50% improvement).
    #[test]
    fn test_effect_size_calculation() {
        let test = WelchTTest::default();
        let baseline = durations_from_nanos(&[2000, 2000, 2000, 2000, 2000]);
        let candidate = durations_from_nanos(&[1000, 1000, 1000, 1000, 1000]);

        let result = test.analyze(&baseline, &candidate);

        // Effect size should be approximately 50%
        assert!((result.effect_size - 50.0).abs() < 0.1);
    }
}

#[cfg(test)]
mod report_tests {
    use criterion_hypothesis::{BenchmarkComparison, Reporter, SampleStats, TerminalReporter};
    use criterion_hypothesis_core::stats::{Side, TestResult};

    fn make_comparison(
        name: &str,
        baseline_mean_ns: f64,
        candidate_mean_ns: f64,
        effect_size: f64,
        p_value: f64,
        winner: Option<Side>,
    ) -> BenchmarkComparison {
        BenchmarkComparison {
            name: name.to_string(),
            baseline_stats: SampleStats {
                mean_ns: baseline_mean_ns,
                std_dev_ns: baseline_mean_ns * 0.05,
                min_ns: (baseline_mean_ns * 0.9) as u64,
                max_ns: (baseline_mean_ns * 1.1) as u64,
                sample_count: 100,
            },
            candidate_stats: SampleStats {
                mean_ns: candidate_mean_ns,
                std_dev_ns: candidate_mean_ns * 0.05,
                min_ns: (candidate_mean_ns * 0.9) as u64,
                max_ns: (candidate_mean_ns * 1.1) as u64,
                sample_count: 100,
            },
            test_result: TestResult {
                p_value,
                statistically_significant: p_value < 0.05,
                effect_size,
                confidence_level: 0.95,
                winner,
                baseline_mean_ns,
                candidate_mean_ns,
            },
        }
    }

    /// Test that reporter can handle a mix of results.
    #[test]
    fn test_reporter_with_mixed_results() {
        let reporter = TerminalReporter::without_colors();
        let results = vec![
            make_comparison(
                "bench_faster",
                1_000_000.0,
                800_000.0,
                20.0,
                0.001,
                Some(Side::Candidate),
            ),
            make_comparison(
                "bench_slower",
                1_000_000.0,
                1_200_000.0,
                -20.0,
                0.001,
                Some(Side::Baseline),
            ),
            make_comparison("bench_same", 1_000_000.0, 1_010_000.0, -1.0, 0.5, None),
        ];

        // Just verify it doesn't panic
        let result = reporter.report(&results);
        assert!(result.is_ok());
    }

    /// Test that reporter handles empty results.
    #[test]
    fn test_reporter_with_empty_results() {
        let reporter = TerminalReporter::without_colors();
        let results: Vec<BenchmarkComparison> = vec![];

        let result = reporter.report(&results);
        assert!(result.is_ok());
    }
}
