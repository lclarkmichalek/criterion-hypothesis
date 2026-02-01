use std::io::{self, Write};

use colored::Colorize;

use super::{BenchmarkComparison, ReportError, Reporter, SampleStats};
use crate::stats::Side;

/// A reporter that outputs benchmark comparison results to the terminal.
#[derive(Debug, Clone, Default)]
pub struct TerminalReporter {
    /// Whether to use colors in output (defaults to true).
    use_colors: bool,
}

impl TerminalReporter {
    /// Create a new terminal reporter with default settings.
    pub fn new() -> Self {
        Self { use_colors: true }
    }

    /// Create a terminal reporter with color output disabled.
    pub fn without_colors() -> Self {
        Self { use_colors: false }
    }

    /// Format a duration in nanoseconds to a human-readable string.
    fn format_time(ns: f64) -> String {
        if ns >= 1_000_000_000.0 {
            format!("{:.3} s", ns / 1_000_000_000.0)
        } else if ns >= 1_000_000.0 {
            format!("{:.3} ms", ns / 1_000_000.0)
        } else if ns >= 1_000.0 {
            format!("{:.3} us", ns / 1_000.0)
        } else {
            format!("{:.3} ns", ns)
        }
    }

    /// Format a duration with standard deviation.
    fn format_time_with_stddev(stats: &SampleStats) -> String {
        let mean = Self::format_time(stats.mean_ns);
        let stddev = Self::format_time(stats.std_dev_ns);
        format!("{} (+/- {})", mean, stddev)
    }

    /// Format the percent change between baseline and candidate.
    fn format_change(effect_size: f64) -> String {
        if effect_size > 0.0 {
            format!("-{:.2}%", effect_size.abs())
        } else if effect_size < 0.0 {
            format!("+{:.2}%", effect_size.abs())
        } else {
            "0.00%".to_string()
        }
    }

    /// Format the result column with appropriate coloring.
    fn format_result(&self, comparison: &BenchmarkComparison) -> String {
        let result = &comparison.test_result;

        if !result.statistically_significant {
            let text = "inconclusive";
            if self.use_colors {
                text.yellow().to_string()
            } else {
                text.to_string()
            }
        } else {
            match result.winner {
                Some(Side::Candidate) => {
                    let text = "faster";
                    if self.use_colors {
                        text.green().bold().to_string()
                    } else {
                        text.to_string()
                    }
                }
                Some(Side::Baseline) => {
                    let text = "slower";
                    if self.use_colors {
                        text.red().bold().to_string()
                    } else {
                        text.to_string()
                    }
                }
                None => {
                    let text = "inconclusive";
                    if self.use_colors {
                        text.yellow().to_string()
                    } else {
                        text.to_string()
                    }
                }
            }
        }
    }

    /// Format the change column with appropriate coloring.
    fn format_change_colored(&self, comparison: &BenchmarkComparison) -> String {
        let change = Self::format_change(comparison.test_result.effect_size);
        let result = &comparison.test_result;

        if !result.statistically_significant {
            if self.use_colors {
                change.yellow().to_string()
            } else {
                change
            }
        } else {
            match result.winner {
                Some(Side::Candidate) => {
                    if self.use_colors {
                        change.green().to_string()
                    } else {
                        change
                    }
                }
                Some(Side::Baseline) => {
                    if self.use_colors {
                        change.red().to_string()
                    } else {
                        change
                    }
                }
                None => {
                    if self.use_colors {
                        change.yellow().to_string()
                    } else {
                        change
                    }
                }
            }
        }
    }

    /// Print the table header.
    fn print_header(&self, writer: &mut impl Write) -> io::Result<()> {
        writeln!(writer)?;
        let header = format!(
            "{:<40} {:>24} {:>24} {:>12} {:>10} {:>14}",
            "Benchmark", "Baseline", "Candidate", "Change", "p-value", "Result"
        );
        if self.use_colors {
            writeln!(writer, "{}", header.bold())?;
        } else {
            writeln!(writer, "{}", header)?;
        }
        writeln!(writer, "{}", "-".repeat(130))?;
        Ok(())
    }

    /// Print a single benchmark row.
    fn print_row(
        &self,
        writer: &mut impl Write,
        comparison: &BenchmarkComparison,
    ) -> io::Result<()> {
        let name = if comparison.name.len() > 38 {
            format!("{}...", &comparison.name[..35])
        } else {
            comparison.name.clone()
        };

        let baseline = Self::format_time_with_stddev(&comparison.baseline_stats);
        let candidate = Self::format_time_with_stddev(&comparison.candidate_stats);
        let change = self.format_change_colored(comparison);
        let p_value = format!("{:.4}", comparison.test_result.p_value);
        let result = self.format_result(comparison);

        // Calculate visible widths accounting for ANSI escape codes
        let change_visible_len = Self::format_change(comparison.test_result.effect_size).len();
        let result_visible_len = if comparison.test_result.statistically_significant {
            match comparison.test_result.winner {
                Some(Side::Candidate) => 6, // "faster"
                Some(Side::Baseline) => 6,  // "slower"
                None => 12,                 // "inconclusive"
            }
        } else {
            12 // "inconclusive"
        };

        // Pad the colored strings to achieve proper alignment
        let change_padding = 12_usize.saturating_sub(change_visible_len);
        let result_padding = 14_usize.saturating_sub(result_visible_len);

        writeln!(
            writer,
            "{:<40} {:>24} {:>24} {:>width_change$}{} {:>10} {:>width_result$}{}",
            name,
            baseline,
            candidate,
            "",
            change,
            p_value,
            "",
            result,
            width_change = change_padding,
            width_result = result_padding,
        )?;
        Ok(())
    }

    /// Print the summary footer.
    fn print_summary(
        &self,
        writer: &mut impl Write,
        results: &[BenchmarkComparison],
    ) -> io::Result<()> {
        let mut faster = 0;
        let mut slower = 0;
        let mut inconclusive = 0;

        for comparison in results {
            if !comparison.test_result.statistically_significant {
                inconclusive += 1;
            } else {
                match comparison.test_result.winner {
                    Some(Side::Candidate) => faster += 1,
                    Some(Side::Baseline) => slower += 1,
                    None => inconclusive += 1,
                }
            }
        }

        writeln!(writer)?;
        writeln!(writer, "{}", "-".repeat(130))?;

        let summary_label = "Summary:";
        if self.use_colors {
            write!(writer, "{} ", summary_label.bold())?;
        } else {
            write!(writer, "{} ", summary_label)?;
        }

        let faster_text = format!("{} faster", faster);
        let slower_text = format!("{} slower", slower);
        let inconclusive_text = format!("{} inconclusive", inconclusive);

        if self.use_colors {
            writeln!(
                writer,
                "{}, {}, {}",
                faster_text.green(),
                slower_text.red(),
                inconclusive_text.yellow()
            )?;
        } else {
            writeln!(
                writer,
                "{}, {}, {}",
                faster_text, slower_text, inconclusive_text
            )?;
        }

        writeln!(writer)?;
        Ok(())
    }
}

impl Reporter for TerminalReporter {
    fn report(&self, results: &[BenchmarkComparison]) -> Result<(), ReportError> {
        let stdout = io::stdout();
        let mut writer = stdout.lock();

        self.print_header(&mut writer)?;

        for comparison in results {
            self.print_row(&mut writer, comparison)?;
        }

        self.print_summary(&mut writer, results)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::TestResult;

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

    #[test]
    fn test_format_time_nanoseconds() {
        assert_eq!(TerminalReporter::format_time(123.456), "123.456 ns");
        assert_eq!(TerminalReporter::format_time(999.999), "999.999 ns");
    }

    #[test]
    fn test_format_time_microseconds() {
        assert_eq!(TerminalReporter::format_time(1234.567), "1.235 us");
        assert_eq!(TerminalReporter::format_time(999_999.0), "999.999 us");
    }

    #[test]
    fn test_format_time_milliseconds() {
        assert_eq!(TerminalReporter::format_time(1_234_567.0), "1.235 ms");
        assert_eq!(TerminalReporter::format_time(999_999_999.0), "1000.000 ms");
    }

    #[test]
    fn test_format_time_seconds() {
        assert_eq!(TerminalReporter::format_time(1_234_567_890.0), "1.235 s");
    }

    #[test]
    fn test_format_change_faster() {
        // Positive effect_size means candidate is faster (negative change)
        assert_eq!(TerminalReporter::format_change(10.5), "-10.50%");
    }

    #[test]
    fn test_format_change_slower() {
        // Negative effect_size means candidate is slower (positive change)
        assert_eq!(TerminalReporter::format_change(-10.5), "+10.50%");
    }

    #[test]
    fn test_format_change_no_change() {
        assert_eq!(TerminalReporter::format_change(0.0), "0.00%");
    }

    #[test]
    fn test_report_to_buffer() {
        let reporter = TerminalReporter::without_colors();
        let results = vec![
            make_comparison(
                "bench_fast",
                1000.0,
                800.0,
                20.0,
                0.001,
                Some(Side::Candidate),
            ),
            make_comparison(
                "bench_slow",
                1000.0,
                1200.0,
                -20.0,
                0.001,
                Some(Side::Baseline),
            ),
            make_comparison("bench_same", 1000.0, 1010.0, -1.0, 0.5, None),
        ];

        let mut buffer = Vec::new();
        reporter.print_header(&mut buffer).unwrap();
        for comparison in &results {
            reporter.print_row(&mut buffer, comparison).unwrap();
        }
        reporter.print_summary(&mut buffer, &results).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Benchmark"));
        assert!(output.contains("Baseline"));
        assert!(output.contains("Candidate"));
        assert!(output.contains("bench_fast"));
        assert!(output.contains("bench_slow"));
        assert!(output.contains("bench_same"));
        assert!(output.contains("Summary:"));
        assert!(output.contains("1 faster"));
        assert!(output.contains("1 slower"));
        assert!(output.contains("1 inconclusive"));
    }
}
