//! GitHub PR-comment reporter: specializes the output for posting as a PR comment body.
//!
//! Layout:
//! - Header line with counts
//! - Pinned lists of regressions and improvements (above the fold)
//! - Full per-bench table inside a collapsible `<details>` block
//! - Baseline/candidate SHA line
//! - Collapsible statistical parameters block
//! - Small footer crediting hypobench

use std::io::Write;

use hypobench_core::stats::Side;
use hypobench_core::{BenchmarkComparison, Report, SampleStats};

use super::ReportError;

#[derive(Debug, Default, Clone)]
pub struct GithubPrCommentReporter;

impl GithubPrCommentReporter {
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

        // Pinned regressions and improvements above the fold.
        let regressions: Vec<&BenchmarkComparison> = report
            .comparisons
            .iter()
            .filter(|c| classify(c) == Verdict::Slower)
            .collect();
        let improvements: Vec<&BenchmarkComparison> = report
            .comparisons
            .iter()
            .filter(|c| classify(c) == Verdict::Faster)
            .collect();

        if !regressions.is_empty() {
            writeln!(writer, "### :warning: Regressions")?;
            writeln!(writer)?;
            for cmp in &regressions {
                writeln!(writer, "{}", format_pinned_row(cmp))?;
            }
            writeln!(writer)?;
        }

        if !improvements.is_empty() {
            writeln!(writer, "### :rocket: Improvements")?;
            writeln!(writer)?;
            for cmp in &improvements {
                writeln!(writer, "{}", format_pinned_row(cmp))?;
            }
            writeln!(writer)?;
        }

        // Full table, collapsed by default. Auto-open when there are regressions
        // so reviewers see the context without having to click.
        let open_attr = if regressions.is_empty() { "" } else { " open" };
        writeln!(writer, "<details{open_attr}>")?;
        writeln!(
            writer,
            "<summary>Full results ({total} benchmarks)</summary>"
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
        writeln!(writer, "</details>")?;
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
        writeln!(
            writer,
            "- Minimum effect size: {}%",
            cfg.minimum_effect_size
        )?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Faster,
    Slower,
    Inconclusive,
}

fn classify(cmp: &BenchmarkComparison) -> Verdict {
    if !cmp.test_result.statistically_significant {
        return Verdict::Inconclusive;
    }
    match cmp.test_result.winner {
        Some(Side::Candidate) => Verdict::Faster,
        Some(Side::Baseline) => Verdict::Slower,
        None => Verdict::Inconclusive,
    }
}

fn verdict_word(v: Verdict) -> &'static str {
    match v {
        Verdict::Faster => "faster",
        Verdict::Slower => "slower",
        Verdict::Inconclusive => "inconclusive",
    }
}

fn tally(comparisons: &[BenchmarkComparison]) -> (usize, usize, usize) {
    let mut faster = 0;
    let mut slower = 0;
    let mut inconclusive = 0;
    for c in comparisons {
        match classify(c) {
            Verdict::Faster => faster += 1,
            Verdict::Slower => slower += 1,
            Verdict::Inconclusive => inconclusive += 1,
        }
    }
    (faster, slower, inconclusive)
}

fn format_pinned_row(cmp: &BenchmarkComparison) -> String {
    let change = format_change(cmp.test_result.effect_size);
    let p = format!("{:.4}", cmp.test_result.p_value);
    let ci = format!(
        "[{:+.2}%, {:+.2}%]",
        -cmp.test_result.change_ci_high, -cmp.test_result.change_ci_low
    );
    format!(
        "- `{}` — **{change}** (p={p}, CI {ci})",
        escape_backticks(&cmp.name)
    )
}

fn write_row(writer: &mut impl Write, cmp: &BenchmarkComparison) -> Result<(), ReportError> {
    let name = escape_pipes(&cmp.name);
    let baseline = format_stats(&cmp.baseline_stats);
    let candidate = format_stats(&cmp.candidate_stats);
    let change = format_change(cmp.test_result.effect_size);
    let ci = format!(
        "[{:+.2}%, {:+.2}%]",
        -cmp.test_result.change_ci_high, -cmp.test_result.change_ci_low
    );
    let p = format!("{:.4}", cmp.test_result.p_value);
    let result = verdict_word(classify(cmp));

    writeln!(
        writer,
        "| {name} | {baseline} | {candidate} | {change} | {ci} | {p} | **{result}** |"
    )?;
    Ok(())
}

fn escape_pipes(s: &str) -> String {
    s.replace('|', r"\|")
}

fn escape_backticks(s: &str) -> String {
    // `foo` in a markdown inline-code span can't contain a backtick. Benchmark
    // names are very unlikely to, but escape defensively so the rendered output
    // doesn't break if one ever sneaks in.
    s.replace('`', "'")
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
