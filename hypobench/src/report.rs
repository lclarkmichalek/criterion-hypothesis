//! Report rendering for the hypobench CLI.
//!
//! The JSON *schema* (serde-derived types) lives in `hypobench-core` so external
//! consumers can deserialize reports without depending on the CLI. The concrete
//! renderers and the `Reporter` trait live here — they're purely CLI concerns.

use std::io;

use hypobench_core::BenchmarkComparison;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

/// A renderer for a slice of benchmark comparisons.
///
/// The `Reporter` trait is deliberately narrow — it sees only the per-benchmark
/// comparisons, not the full `Report` (which also carries run metadata). The
/// JSON and PR-comment renderers expose their own top-level entry points that
/// take a `&Report`, because they need the metadata.
pub trait Reporter: Send + Sync {
    fn report(&self, results: &[BenchmarkComparison]) -> Result<(), ReportError>;
}

mod github_pr_comment;
mod json;
mod terminal;
pub use github_pr_comment::GithubPrCommentReporter;
pub use json::JsonReporter;
pub use terminal::TerminalReporter;

#[cfg(test)]
mod json_reporter_tests {
    use hypobench_core::stats::{Side, TestResult};
    use hypobench_core::{
        BenchmarkComparison, ConfigSnapshot, Report, ReportMetadata, SampleStats,
    };

    use super::JsonReporter;

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
mod github_pr_comment_tests {
    use hypobench_core::stats::{Side, TestResult};
    use hypobench_core::{
        BenchmarkComparison, ConfigSnapshot, Report, ReportMetadata, SampleStats,
    };

    use super::GithubPrCommentReporter;

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
                make_comparison(
                    "bench_fast",
                    1000.0,
                    800.0,
                    20.0,
                    0.001,
                    Some(Side::Candidate),
                    true,
                ),
                make_comparison(
                    "bench_slow",
                    1000.0,
                    1200.0,
                    -20.0,
                    0.001,
                    Some(Side::Baseline),
                    true,
                ),
                make_comparison("bench_same", 1000.0, 1010.0, -1.0, 0.5, None, false),
            ],
        }
    }

    #[test]
    fn renders_summary_and_pinned_sections_and_table() {
        let report = sample_report();
        let mut buf = Vec::new();
        GithubPrCommentReporter::new()
            .write(&report, &mut buf)
            .expect("write");
        let out = String::from_utf8(buf).unwrap();

        assert!(out.contains("1 faster"), "missing faster count: {out}");
        assert!(out.contains("1 slower"), "missing slower count: {out}");
        assert!(
            out.contains("1 inconclusive"),
            "missing inconclusive count: {out}"
        );

        assert!(out.contains("Regressions"), "missing regressions header");
        assert!(out.contains("Improvements"), "missing improvements header");

        // Full-table details block is always collapsed — reviewers click to expand.
        assert!(
            out.contains("<details>"),
            "missing closed details tag: {out}"
        );
        assert!(
            !out.contains("<details open>"),
            "details should not be auto-opened even with regressions: {out}"
        );
        assert!(out.contains("<summary>Full results (3 benchmarks)</summary>"));
        assert!(out.contains("| Benchmark |"));
        assert!(out.contains("bench_fast"));
        assert!(out.contains("bench_slow"));
        assert!(out.contains("bench_same"));
        // Emojis in the leftmost table column conveying verdict (the dedicated
        // Result column was dropped — the emoji now carries that signal).
        assert!(
            out.contains("| :rocket: | bench_fast"),
            "missing rocket emoji on faster row: {out}"
        );
        assert!(
            out.contains("| :warning: | bench_slow"),
            "missing warning emoji on slower row: {out}"
        );
        assert!(
            out.contains("| :heavy_minus_sign: | bench_same"),
            "missing neutral emoji on inconclusive row: {out}"
        );

        assert!(out.contains("abc123"));
        assert!(out.contains("def456"));
        assert!(out.contains("hypobench 0.5.0"));
    }

    #[test]
    fn details_stays_closed_when_no_regressions() {
        let mut report = sample_report();
        // Drop the Baseline-winner row so there are no regressions.
        report
            .comparisons
            .retain(|c| !matches!(c.test_result.winner, Some(Side::Baseline)));

        let mut buf = Vec::new();
        GithubPrCommentReporter::new()
            .write(&report, &mut buf)
            .expect("write");
        let out = String::from_utf8(buf).unwrap();

        assert!(
            !out.contains("Regressions"),
            "no regressions header expected"
        );
        // The full-results details block should be present but closed.
        assert!(
            out.contains("<details>"),
            "missing closed details tag: {out}"
        );
        assert!(
            !out.contains("<details open>"),
            "details should not be open: {out}"
        );
    }

    #[test]
    fn escapes_pipes_in_benchmark_names() {
        let mut report = sample_report();
        report.comparisons[0].name = "bench|weird".to_string();
        let mut buf = Vec::new();
        GithubPrCommentReporter::new()
            .write(&report, &mut buf)
            .expect("write");
        let out = String::from_utf8(buf).unwrap();
        assert!(out.contains(r"bench\|weird"), "pipe not escaped: {out}");
    }
}
