use anyhow::{Context, Result};
use clap::Parser;
use criterion_hypothesis::{
    run_with_urls, BenchmarkComparison, BuildManager, Cli, Config, GitWorktreeProvider,
    Orchestrator, Reporter, SampleStats, SourceProvider, StatisticalTest, TerminalReporter,
    WelchTTest,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load config and apply CLI overrides
    let mut config = Config::load_or_default()?;
    cli.apply_to_config(&mut config);

    if cli.verbose {
        eprintln!("Configuration: {:?}", config);
    }

    // Run in the appropriate mode
    let samples = if cli.is_manual_mode() {
        run_manual_mode(&cli, &config).await?
    } else {
        run_automatic_mode(&cli, &config).await?
    };

    // Analyze results
    eprintln!("Analyzing results...");
    let test = WelchTTest::new(config.hypothesis.confidence_level);
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

    // Report results
    let reporter = TerminalReporter::new();
    reporter.report(&comparisons)?;

    Ok(())
}

/// Run in manual mode - connect to pre-running harnesses at the specified URLs.
async fn run_manual_mode(
    cli: &Cli,
    config: &Config,
) -> Result<Vec<criterion_hypothesis::BenchmarkSamples>> {
    let baseline_url = cli
        .baseline_url
        .as_ref()
        .expect("baseline_url required for manual mode");
    let candidate_url = cli
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
        config.orchestration.warmup_iterations,
        config.orchestration.sample_size,
        Duration::from_millis(config.orchestration.interleave_interval_ms),
    )
    .await
    .context("Failed to run benchmarks with URLs")?;

    Ok(samples)
}

/// Run in automatic mode - checkout commits, build, spawn harnesses.
async fn run_automatic_mode(
    cli: &Cli,
    config: &Config,
) -> Result<Vec<criterion_hypothesis::BenchmarkSamples>> {
    let baseline = cli
        .baseline
        .as_ref()
        .expect("baseline required for automatic mode");
    let candidate = cli
        .candidate
        .as_ref()
        .expect("candidate required for automatic mode");

    // 1. Prepare sources
    eprintln!("Preparing sources...");
    let source_provider = GitWorktreeProvider::new()?;
    let (baseline_path, candidate_path) = source_provider
        .prepare_sources(baseline, candidate)
        .context("Failed to prepare sources")?;

    if cli.verbose {
        eprintln!("Baseline: {:?}", baseline_path);
        eprintln!("Candidate: {:?}", candidate_path);
    }

    // 2. Build both
    eprintln!("Building benchmarks...");
    let builder = BuildManager::new(
        config.build.profile.clone(),
        config.build.cargo_flags.clone(),
    );

    // If project_path is specified, build from the subdirectory within each worktree
    let baseline_build_path = match &cli.project_path {
        Some(p) => baseline_path.join(p),
        None => baseline_path.clone(),
    };
    let candidate_build_path = match &cli.project_path {
        Some(p) => candidate_path.join(p),
        None => candidate_path.clone(),
    };

    if cli.verbose {
        eprintln!("Baseline build path: {:?}", baseline_build_path);
        eprintln!("Candidate build path: {:?}", candidate_build_path);
    }

    let baseline_build = builder
        .build(&baseline_build_path)
        .context("Failed to build baseline")?;
    let candidate_build = builder
        .build(&candidate_build_path)
        .context("Failed to build candidate")?;

    // 3. Run orchestrator
    eprintln!("Running benchmarks...");
    let orchestrator = Orchestrator::new(
        baseline_build.binary_path,
        candidate_build.binary_path,
        config.network.base_port,
        Duration::from_millis(config.network.harness_timeout_ms),
        config.orchestration.warmup_iterations,
        config.orchestration.sample_size,
        Duration::from_millis(config.orchestration.interleave_interval_ms),
        cli.harness_output,
    );

    let samples = orchestrator
        .run()
        .await
        .context("Failed to run benchmarks")?;

    // 4. Cleanup
    eprintln!("Cleaning up...");
    source_provider
        .cleanup()
        .context("Failed to cleanup sources")?;

    Ok(samples)
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
