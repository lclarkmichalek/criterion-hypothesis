use anyhow::{Context, Result};
use clap::Parser;
use hypobench::{
    apply_bonferroni, run_with_urls, BenchmarkComparison, BuildManager, Cli, Command, Config,
    ConfigSnapshot, GitWorktreeProvider, GithubPrCommentReporter, JsonReporter, Orchestrator,
    Report, ReportArgs, ReportFormat, ReportMetadata, Reporter, RunArgs, SampleStats,
    SourceProvider, StatisticalTest, TerminalReporter, WelchTTest,
};
use std::io::Read;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Report(args)) => run_report_subcommand(args).await,
        Some(Command::Run(args)) => run_benchmarks(args).await,
        None => run_benchmarks(cli.run).await,
    }
}

async fn run_benchmarks(run_args: RunArgs) -> Result<()> {
    run_args.validate().map_err(|msg| anyhow::anyhow!(msg))?;

    let mut config = Config::load_or_default()?;
    run_args.apply_to_config(&mut config);

    if run_args.verbose {
        eprintln!("Configuration: {:?}", config);
    }

    let samples = if run_args.is_manual_mode() {
        run_manual_mode(&run_args, &config).await?
    } else {
        run_automatic_mode(&run_args, &config).await?
    };

    eprintln!("Analyzing results...");
    let test = WelchTTest::new(config.hypothesis.confidence_level)
        .with_minimum_effect_size(config.hypothesis.minimum_effect_size);
    let mut comparisons = Vec::new();

    for sample in samples {
        let test_result = test.analyze(&sample.baseline_samples, &sample.candidate_samples);

        let baseline_stats = calculate_stats(&sample.baseline_samples);
        let candidate_stats = calculate_stats(&sample.candidate_samples);

        comparisons.push(BenchmarkComparison {
            name: sample.name,
            baseline_stats,
            candidate_stats,
            test_result,
        });
    }

    // Apply Bonferroni multiple-comparisons correction across the whole family.
    // Each test's effective α becomes (1 - confidence_level) / N, controlling
    // family-wise false-positive rate at the nominal level rather than letting
    // it scale with the number of benchmarks.
    if config.hypothesis.correct_multiple_comparisons && comparisons.len() > 1 {
        let family_alpha = 1.0 - config.hypothesis.confidence_level;
        let mut results: Vec<_> = comparisons.iter().map(|c| c.test_result.clone()).collect();
        apply_bonferroni(&mut results, family_alpha);
        for (c, updated) in comparisons.iter_mut().zip(results) {
            c.test_result = updated;
        }
        eprintln!(
            "Applied Bonferroni correction: effective α = {:.2e} across {} benchmarks",
            family_alpha / comparisons.len() as f64,
            comparisons.len(),
        );
    }

    let report = build_report(&run_args, &config, comparisons);
    render(&run_args.format, &report)?;
    Ok(())
}

fn build_report(
    run_args: &RunArgs,
    config: &Config,
    comparisons: Vec<BenchmarkComparison>,
) -> Report {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    Report {
        schema_version: Report::CURRENT_SCHEMA_VERSION.to_string(),
        metadata: ReportMetadata {
            hypobench_version: env!("CARGO_PKG_VERSION").to_string(),
            generated_at: format_epoch_utc(secs),
            baseline_ref: run_args
                .baseline
                .clone()
                .or_else(|| run_args.baseline_url.clone())
                .unwrap_or_default(),
            candidate_ref: run_args
                .candidate
                .clone()
                .or_else(|| run_args.candidate_url.clone())
                .unwrap_or_default(),
            config: ConfigSnapshot {
                confidence_level: config.hypothesis.confidence_level,
                minimum_effect_size: config.hypothesis.minimum_effect_size,
                sample_size: config.orchestration.sample_size,
                correct_multiple_comparisons: config.hypothesis.correct_multiple_comparisons,
            },
        },
        comparisons,
    }
}

fn format_epoch_utc(secs: u64) -> String {
    // Dependency-free RFC 3339 UTC timestamp. Adequate for a metadata field;
    // not used in any arithmetic downstream.
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

// Howard Hinnant's days-from-civil inverse. Chrono would be a surprisingly
// heavy dep just to format one timestamp string.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = y + if m <= 2 { 1 } else { 0 };
    (y as i32, m as u32, d as u32)
}

fn render(format: &ReportFormat, report: &Report) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match format {
        ReportFormat::Terminal => {
            // The TerminalReporter trait takes only the comparisons slice today.
            // Metadata is visible to the user via the CI logs / step summary; if
            // we later want it rendered inline, we can give TerminalReporter its
            // own write_report(&Report) method.
            TerminalReporter::new()
                .report(&report.comparisons)
                .context("terminal report failed")?;
        }
        ReportFormat::GithubPrComment => {
            GithubPrCommentReporter::new()
                .write(report, &mut out)
                .context("github-pr-comment report failed")?;
        }
        ReportFormat::Json => {
            JsonReporter::new()
                .write(report, &mut out)
                .context("json report failed")?;
        }
    }
    Ok(())
}

async fn run_report_subcommand(args: ReportArgs) -> Result<()> {
    let json = if args.input == Path::new("-") {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading stdin")?;
        buf
    } else {
        std::fs::read_to_string(&args.input)
            .with_context(|| format!("reading {}", args.input.display()))?
    };

    let report: Report = serde_json::from_str(&json).context("parsing JSON report")?;
    render(&args.format, &report)?;
    Ok(())
}

/// Run in manual mode - connect to pre-running harnesses at the specified URLs.
async fn run_manual_mode(
    run_args: &RunArgs,
    config: &Config,
) -> Result<Vec<hypobench::BenchmarkSamples>> {
    let baseline_url = run_args
        .baseline_url
        .as_ref()
        .expect("baseline_url required for manual mode");
    let candidate_url = run_args
        .candidate_url
        .as_ref()
        .expect("candidate_url required for manual mode");

    eprintln!("Running in manual mode...");
    eprintln!("  Baseline URL: {}", baseline_url);
    eprintln!("  Candidate URL: {}", candidate_url);

    let samples = run_with_urls(
        baseline_url,
        candidate_url,
        Duration::from_millis(config.network.harness_timeout_ms),
        config.orchestration.sample_size,
        Duration::from_millis(config.orchestration.interleave_interval_ms),
        Duration::from_millis(config.orchestration.target_sample_ms),
        config.orchestration.max_calibration_iters,
    )
    .await
    .context("Failed to run benchmarks with URLs")?;

    Ok(samples)
}

/// Run in automatic mode - checkout commits, build, spawn harnesses.
async fn run_automatic_mode(
    run_args: &RunArgs,
    config: &Config,
) -> Result<Vec<hypobench::BenchmarkSamples>> {
    let baseline = run_args
        .baseline
        .as_ref()
        .expect("baseline required for automatic mode");
    let candidate = run_args
        .candidate
        .as_ref()
        .expect("candidate required for automatic mode");

    // 1. Prepare sources
    eprintln!("Preparing sources...");
    let source_provider = GitWorktreeProvider::new()?;
    let (baseline_path, candidate_path) = source_provider
        .prepare_sources(baseline, candidate)
        .context("Failed to prepare sources")?;

    if run_args.verbose {
        eprintln!("Baseline: {:?}", baseline_path);
        eprintln!("Candidate: {:?}", candidate_path);
    }

    // 2. Build both
    eprintln!("Building benchmarks...");
    let builder = BuildManager::new(
        config.build.profile.clone(),
        config.build.cargo_flags.clone(),
    );

    let baseline_build_path = match &run_args.project_path {
        Some(p) => baseline_path.join(p),
        None => baseline_path.clone(),
    };
    let candidate_build_path = match &run_args.project_path {
        Some(p) => candidate_path.join(p),
        None => candidate_path.clone(),
    };

    if run_args.verbose {
        eprintln!("Baseline build path: {:?}", baseline_build_path);
        eprintln!("Candidate build path: {:?}", candidate_build_path);
    }

    // Determine bench targets: CLI overrides config
    let bench_targets = if !run_args.bench.is_empty() {
        run_args.bench.clone()
    } else {
        config.build.bench_targets.clone()
    };

    let mut all_samples = Vec::new();

    if bench_targets.is_empty() {
        let baseline_build = builder
            .build(&baseline_build_path, "baseline")
            .context("Failed to build baseline")?;
        let candidate_build = builder
            .build(&candidate_build_path, "candidate")
            .context("Failed to build candidate")?;

        eprintln!("Running benchmarks...");
        let orchestrator = Orchestrator::new(
            baseline_build.binary_path,
            candidate_build.binary_path,
            config.network.base_port,
            Duration::from_millis(config.network.harness_timeout_ms),
            config.orchestration.sample_size,
            Duration::from_millis(config.orchestration.interleave_interval_ms),
            Duration::from_millis(config.orchestration.target_sample_ms),
            config.orchestration.max_calibration_iters,
            run_args.harness_output,
        );

        all_samples.extend(
            orchestrator
                .run()
                .await
                .context("Failed to run benchmarks")?,
        );
    } else {
        for bench_name in &bench_targets {
            eprintln!("Building bench target: {}", bench_name);
            let baseline_label = format!("baseline {}", bench_name);
            let candidate_label = format!("candidate {}", bench_name);
            let baseline_build = builder
                .build_bench(&baseline_build_path, bench_name, &baseline_label)
                .with_context(|| format!("Failed to build baseline for bench '{}'", bench_name))?;
            let candidate_build = builder
                .build_bench(&candidate_build_path, bench_name, &candidate_label)
                .with_context(|| format!("Failed to build candidate for bench '{}'", bench_name))?;

            eprintln!("Running benchmarks for: {}", bench_name);
            let orchestrator = Orchestrator::new(
                baseline_build.binary_path,
                candidate_build.binary_path,
                config.network.base_port,
                Duration::from_millis(config.network.harness_timeout_ms),
                config.orchestration.sample_size,
                Duration::from_millis(config.orchestration.interleave_interval_ms),
                Duration::from_millis(config.orchestration.target_sample_ms),
                config.orchestration.max_calibration_iters,
                run_args.harness_output,
            );

            all_samples.extend(
                orchestrator.run().await.with_context(|| {
                    format!("Failed to run benchmarks for bench '{}'", bench_name)
                })?,
            );
        }
    };

    // 4. Cleanup
    eprintln!("Cleaning up...");
    source_provider
        .cleanup()
        .context("Failed to cleanup sources")?;

    Ok(all_samples)
}

fn calculate_stats(samples: &[Duration]) -> SampleStats {
    let ns_values: Vec<f64> = samples.iter().map(|d| d.as_nanos() as f64).collect();
    let n = ns_values.len();

    let mean = ns_values.iter().sum::<f64>() / n as f64;
    let variance = ns_values.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    let std_dev = variance.sqrt();

    let min = samples
        .iter()
        .map(|d| d.as_nanos() as u64)
        .min()
        .unwrap_or(0);
    let max = samples
        .iter()
        .map(|d| d.as_nanos() as u64)
        .max()
        .unwrap_or(0);

    SampleStats {
        mean_ns: mean,
        std_dev_ns: std_dev,
        min_ns: min,
        max_ns: max,
        sample_count: n,
    }
}
