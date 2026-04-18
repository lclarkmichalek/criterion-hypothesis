use crate::stats::TestResult;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SampleStats {
    pub mean_ns: f64,
    pub std_dev_ns: f64,
    pub min_ns: u64,
    pub max_ns: u64,
    pub sample_count: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchmarkComparison {
    pub name: String,
    pub baseline_stats: SampleStats,
    pub candidate_stats: SampleStats,
    pub test_result: TestResult,
}

mod schema;
pub use schema::{ConfigSnapshot, Report, ReportMetadata};

#[cfg(test)]
mod serde_tests {
    use super::*;
    use crate::stats::{Side, TestResult};

    fn sample_comparison() -> BenchmarkComparison {
        BenchmarkComparison {
            name: "bench_foo".to_string(),
            baseline_stats: SampleStats {
                mean_ns: 1000.0,
                std_dev_ns: 50.0,
                min_ns: 900,
                max_ns: 1100,
                sample_count: 50,
            },
            candidate_stats: SampleStats {
                mean_ns: 800.0,
                std_dev_ns: 40.0,
                min_ns: 720,
                max_ns: 880,
                sample_count: 50,
            },
            test_result: TestResult {
                p_value: 0.001,
                statistically_significant: true,
                effect_size: 20.0,
                change_ci_low: 18.0,
                change_ci_high: 22.0,
                confidence_level: 0.99,
                winner: Some(Side::Candidate),
                baseline_mean_ns: 1000.0,
                candidate_mean_ns: 800.0,
            },
        }
    }

    #[test]
    fn benchmark_comparison_roundtrips_through_json() {
        let original = sample_comparison();
        let json = serde_json::to_string(&original).expect("serialize");
        let parsed: BenchmarkComparison = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.name, original.name);
        assert_eq!(
            parsed.baseline_stats.mean_ns,
            original.baseline_stats.mean_ns
        );
        assert_eq!(
            parsed.baseline_stats.sample_count,
            original.baseline_stats.sample_count
        );
        assert_eq!(
            parsed.candidate_stats.mean_ns,
            original.candidate_stats.mean_ns
        );
        assert_eq!(parsed.test_result.p_value, original.test_result.p_value);
        assert_eq!(
            parsed.test_result.effect_size,
            original.test_result.effect_size
        );
        assert!(matches!(parsed.test_result.winner, Some(Side::Candidate)));
    }
}

#[cfg(test)]
mod report_tests {
    use super::*;
    use crate::stats::{Side, TestResult};

    #[test]
    fn report_roundtrips_through_json_with_metadata() {
        let report = Report {
            schema_version: "1".to_string(),
            metadata: ReportMetadata {
                hypobench_version: "0.5.0".to_string(),
                generated_at: "2026-04-18T10:00:00Z".to_string(),
                baseline_ref: "abc123".to_string(),
                candidate_ref: "def456".to_string(),
                config: ConfigSnapshot {
                    confidence_level: 0.99,
                    minimum_effect_size: 2.0,
                    sample_size: 50,
                    correct_multiple_comparisons: true,
                },
            },
            comparisons: vec![BenchmarkComparison {
                name: "bench_foo".to_string(),
                baseline_stats: SampleStats {
                    mean_ns: 1000.0,
                    std_dev_ns: 50.0,
                    min_ns: 900,
                    max_ns: 1100,
                    sample_count: 50,
                },
                candidate_stats: SampleStats {
                    mean_ns: 800.0,
                    std_dev_ns: 40.0,
                    min_ns: 720,
                    max_ns: 880,
                    sample_count: 50,
                },
                test_result: TestResult {
                    p_value: 0.001,
                    statistically_significant: true,
                    effect_size: 20.0,
                    change_ci_low: 18.0,
                    change_ci_high: 22.0,
                    confidence_level: 0.99,
                    winner: Some(Side::Candidate),
                    baseline_mean_ns: 1000.0,
                    candidate_mean_ns: 800.0,
                },
            }],
        };

        let json = serde_json::to_string_pretty(&report).expect("serialize");
        assert!(json.contains("\"schema_version\": \"1\""));
        assert!(json.contains("\"hypobench_version\": \"0.5.0\""));
        assert!(json.contains("\"baseline_ref\": \"abc123\""));

        let parsed: Report = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.schema_version, "1");
        assert_eq!(parsed.metadata.hypobench_version, "0.5.0");
        assert_eq!(parsed.metadata.config.sample_size, 50);
        assert_eq!(parsed.comparisons.len(), 1);
        assert_eq!(parsed.comparisons[0].name, "bench_foo");
    }
}
