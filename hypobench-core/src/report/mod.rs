use crate::stats::TestResult;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

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

pub trait Reporter: Send + Sync {
    fn report(&self, results: &[BenchmarkComparison]) -> Result<(), ReportError>;
}

mod json;
mod markdown;
mod schema;
mod terminal;
pub use json::JsonReporter;
pub use markdown::MarkdownReporter;
pub use schema::{ConfigSnapshot, Report, ReportMetadata};
pub use terminal::TerminalReporter;

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
        assert_eq!(parsed.baseline_stats.mean_ns, original.baseline_stats.mean_ns);
        assert_eq!(parsed.baseline_stats.sample_count, original.baseline_stats.sample_count);
        assert_eq!(parsed.candidate_stats.mean_ns, original.candidate_stats.mean_ns);
        assert_eq!(parsed.test_result.p_value, original.test_result.p_value);
        assert_eq!(parsed.test_result.effect_size, original.test_result.effect_size);
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

#[cfg(test)]
mod json_reporter_tests {
    use super::*;
    use crate::stats::{Side, TestResult};

    fn sample_report() -> Report {
        Report {
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
        }
    }

    #[test]
    fn json_reporter_writes_pretty_valid_json() {
        let report = sample_report();
        let mut buf = Vec::new();
        JsonReporter::new().write(&report, &mut buf).expect("write");
        let out = String::from_utf8(buf).unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid json");
        assert_eq!(parsed["schema_version"], "1");
        assert_eq!(parsed["metadata"]["baseline_ref"], "abc123");
        assert_eq!(parsed["comparisons"][0]["name"], "bench_foo");
        assert!(out.contains('\n'), "should be pretty-printed");
    }
}

#[cfg(test)]
mod markdown_reporter_tests {
    use super::*;
    use crate::stats::{Side, TestResult};

    fn make_comparison(
        name: &str,
        baseline_ns: f64,
        candidate_ns: f64,
        effect: f64,
        p: f64,
        winner: Option<Side>,
        significant: bool,
    ) -> BenchmarkComparison {
        BenchmarkComparison {
            name: name.to_string(),
            baseline_stats: SampleStats {
                mean_ns: baseline_ns,
                std_dev_ns: baseline_ns * 0.05,
                min_ns: (baseline_ns * 0.9) as u64,
                max_ns: (baseline_ns * 1.1) as u64,
                sample_count: 50,
            },
            candidate_stats: SampleStats {
                mean_ns: candidate_ns,
                std_dev_ns: candidate_ns * 0.05,
                min_ns: (candidate_ns * 0.9) as u64,
                max_ns: (candidate_ns * 1.1) as u64,
                sample_count: 50,
            },
            test_result: TestResult {
                p_value: p,
                statistically_significant: significant,
                effect_size: effect,
                change_ci_low: effect - 1.0,
                change_ci_high: effect + 1.0,
                confidence_level: 0.99,
                winner,
                baseline_mean_ns: baseline_ns,
                candidate_mean_ns: candidate_ns,
            },
        }
    }

    fn sample_report() -> Report {
        Report {
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
            comparisons: vec![
                make_comparison("bench_fast", 1000.0, 800.0, 20.0, 0.001, Some(Side::Candidate), true),
                make_comparison("bench_slow", 1000.0, 1200.0, -20.0, 0.001, Some(Side::Baseline), true),
                make_comparison("bench_same", 1000.0, 1010.0, -1.0, 0.5, None, false),
            ],
        }
    }

    #[test]
    fn markdown_renders_summary_and_table() {
        let report = sample_report();
        let mut buf = Vec::new();
        MarkdownReporter::new().write(&report, &mut buf).expect("write");
        let out = String::from_utf8(buf).unwrap();

        assert!(out.contains("1 faster"), "missing faster count: {out}");
        assert!(out.contains("1 slower"), "missing slower count: {out}");
        assert!(out.contains("1 inconclusive"), "missing inconclusive count: {out}");

        assert!(out.contains("| Benchmark |"));
        assert!(out.contains("bench_fast"));
        assert!(out.contains("bench_slow"));
        assert!(out.contains("bench_same"));
        assert!(out.contains("faster"));
        assert!(out.contains("slower"));
        assert!(out.contains("inconclusive"));

        assert!(out.contains("abc123"));
        assert!(out.contains("def456"));
        assert!(out.contains("hypobench 0.5.0"));
    }

    #[test]
    fn markdown_escapes_pipes_in_benchmark_names() {
        let mut report = sample_report();
        report.comparisons[0].name = "bench|weird".to_string();
        let mut buf = Vec::new();
        MarkdownReporter::new().write(&report, &mut buf).expect("write");
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains(r"bench\|weird"), "pipe not escaped: {out}");
    }
}
