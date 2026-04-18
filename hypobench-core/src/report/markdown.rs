//! Markdown reporter: writes a GitHub-flavored Markdown summary of a `Report`.

use std::io::Write;

use super::{BenchmarkComparison, Report, ReportError, SampleStats};
use crate::stats::Side;

/// A reporter that formats a `Report` as GitHub-flavored Markdown.
///
/// Suitable for posting as a PR comment body.
#[derive(Debug, Default, Clone)]
pub struct MarkdownReporter;

impl MarkdownReporter {
    pub fn new() -> Self {
        Self
    }

    pub fn write(&self, report: &Report, writer: &mut impl Write) -> Result<(), ReportError> {
        let (faster, slower, inconclusive) = tally(&report.comparisons);
        let total = report.comparisons.len();

        writeln!(writer, "## Benchmark Results")?;
        writeln!(writer)?;
        writeln!(
            writer,
            "**{faster} faster, {slower} slower, {inconclusive} inconclusive** across {total} benchmarks."
        )?;
        writeln!(writer)?;
        writeln!(
            writer,
            "| Benchmark | Baseline | Candidate | Change | {ci_pct:.0}% CI | p | Result |",
            ci_pct = report.metadata.config.confidence_level * 100.0
        )?;
        writeln!(
            writer,
            "|-----------|----------|-----------|--------|--------|---|--------|"
        )?;

        for cmp in &report.comparisons {
            write_row(writer, cmp)?;
        }

        writeln!(writer)?;
        writeln!(
            writer,
            "Baseline: `{}` · Candidate: `{}`",
            report.metadata.baseline_ref, report.metadata.candidate_ref
        )?;
        writeln!(writer)?;
        writeln!(writer, "<details>")?;
        writeln!(writer, "<summary>Statistical parameters</summary>")?;
        writeln!(writer)?;
        let cfg = &report.metadata.config;
        writeln!(writer, "- Confidence level: {}", cfg.confidence_level)?;
        writeln!(writer, "- Minimum effect size: {}%", cfg.minimum_effect_size)?;
        writeln!(writer, "- Sample size: {}", cfg.sample_size)?;
        writeln!(
            writer,
            "- Multiple-comparisons correction: {}",
            if cfg.correct_multiple_comparisons {
                "Bonferroni"
            } else {
                "none"
            }
        )?;
        writeln!(writer, "- Generated at: {}", report.metadata.generated_at)?;
        writeln!(writer)?;
        writeln!(writer, "</details>")?;
        writeln!(writer)?;
        writeln!(
            writer,
            "<sub>Produced by hypobench {}</sub>",
            report.metadata.hypobench_version
        )?;
        Ok(())
    }
}

fn tally(comparisons: &[BenchmarkComparison]) -> (usize, usize, usize) {
    let mut faster = 0;
    let mut slower = 0;
    let mut inconclusive = 0;
    for c in comparisons {
        if !c.test_result.statistically_significant {
            inconclusive += 1;
            continue;
        }
        match c.test_result.winner {
            Some(Side::Candidate) => faster += 1,
            Some(Side::Baseline) => slower += 1,
            None => inconclusive += 1,
        }
    }
    (faster, slower, inconclusive)
}

fn write_row(writer: &mut impl Write, cmp: &BenchmarkComparison) -> Result<(), ReportError> {
    let name = escape_pipes(&cmp.name);
    let baseline = format_stats(&cmp.baseline_stats);
    let candidate = format_stats(&cmp.candidate_stats);
    let change = format_change(cmp.test_result.effect_size);
    let ci = format!(
        "[{:+.2}%, {:+.2}%]",
        -cmp.test_result.change_ci_high,
        -cmp.test_result.change_ci_low
    );
    let p = format!("{:.4}", cmp.test_result.p_value);
    let result = classify(cmp);

    writeln!(
        writer,
        "| {name} | {baseline} | {candidate} | {change} | {ci} | {p} | **{result}** |"
    )?;
    Ok(())
}

fn escape_pipes(s: &str) -> String {
    s.replace('|', r"\|")
}

fn format_stats(stats: &SampleStats) -> String {
    format!(
        "{} (± {})",
        format_time(stats.mean_ns),
        format_time(stats.std_dev_ns)
    )
}

fn format_time(ns: f64) -> String {
    if ns >= 1_000_000_000.0 {
        format!("{:.3} s", ns / 1_000_000_000.0)
    } else if ns >= 1_000_000.0 {
        format!("{:.3} ms", ns / 1_000_000.0)
    } else if ns >= 1_000.0 {
        format!("{:.3} µs", ns / 1_000.0)
    } else {
        format!("{:.3} ns", ns)
    }
}

fn format_change(effect_size: f64) -> String {
    // Flip sign to match the "%-change of runtime" reader expectation:
    // positive effect_size (candidate faster) → negative percent change.
    if effect_size > 0.0 {
        format!("-{:.2}%", effect_size.abs())
    } else if effect_size < 0.0 {
        format!("+{:.2}%", effect_size.abs())
    } else {
        "0.00%".to_string()
    }
}

fn classify(cmp: &BenchmarkComparison) -> &'static str {
    if !cmp.test_result.statistically_significant {
        return "inconclusive";
    }
    match cmp.test_result.winner {
        Some(Side::Candidate) => "faster",
        Some(Side::Baseline) => "slower",
        None => "inconclusive",
    }
}
