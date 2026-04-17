//! Test orchestrator for managing benchmark harness processes.
//!
//! The orchestrator spawns baseline and candidate harness processes, manages their
//! lifecycle, and collects interleaved benchmark samples for statistical comparison.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use uuid::Uuid;

use criterion_hypothesis_core::protocol::{
    BenchmarkListResponse, ClaimRequest, ClaimResponse, HealthResponse, ReleaseRequest,
    RunIterationRequest, RunIterationResponse, ShutdownResponse, CLAIM_HEADER, PROTOCOL_VERSION,
};

/// Errors that can occur during orchestration.
#[derive(Debug, Error)]
pub enum OrchestratorError {
    /// Failed to spawn a harness process.
    #[error("Failed to spawn harness: {0}")]
    SpawnError(String),

    /// Harness did not become ready within the timeout period.
    #[error("Harness at {url} not ready after {timeout_secs}s timeout. Last error: {last_error}")]
    TimeoutError {
        url: String,
        timeout_secs: u64,
        last_error: String,
    },

    /// HTTP request to harness failed.
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// Baseline and candidate have different benchmark sets.
    #[error("Benchmark mismatch: baseline has {baseline:?}, candidate has {candidate:?}")]
    BenchmarkMismatch {
        baseline: Vec<String>,
        candidate: Vec<String>,
    },

    /// Requested benchmark was not found.
    #[error("Benchmark not found: {0}")]
    BenchmarkNotFound(String),

    /// Harness reported an error during execution.
    #[error("Harness error: {0}")]
    HarnessError(String),

    /// Invalid URL provided.
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Failed to claim harness (already claimed by another orchestrator).
    #[error("Failed to claim harness: {0}")]
    ClaimError(String),

    /// Harness reports a protocol version this orchestrator cannot talk to.
    #[error(
        "Protocol mismatch at {url}: orchestrator speaks v{expected}, harness speaks v{actual}. \
         Update `criterion-hypothesis-harness` in the benchmarked project to a version that \
         implements protocol v{expected} (typically by matching the `criterion-hypothesis` \
         CLI version), or downgrade the orchestrator to v{actual}."
    )]
    ProtocolVersionMismatch {
        url: String,
        expected: u32,
        actual: u32,
    },
}

/// Handle to a running harness process (spawned by us).
pub struct HarnessHandle {
    /// The child process (None for remote harnesses, uses std::process).
    process: Option<Child>,
    /// Tokio child process (for async output streaming).
    tokio_process: Option<tokio::process::Child>,
    /// Base URL for the harness.
    base_url: String,
    /// HTTP client for communication.
    client: reqwest::Client,
    /// Whether this is a managed process (spawned by us) or remote.
    is_managed: bool,
    /// Output streaming tasks (if enabled).
    output_tasks: Vec<JoinHandle<()>>,
    /// Claim nonce for exclusive access (None if not claimed).
    claim_nonce: Option<String>,
}

impl HarnessHandle {
    /// Spawn a new harness process.
    ///
    /// # Arguments
    ///
    /// * `binary` - Path to the harness binary
    /// * `port` - Port for the harness to listen on
    ///
    /// # Errors
    ///
    /// Returns an error if the process cannot be spawned.
    pub async fn spawn(binary: &Path, port: u16) -> Result<Self, OrchestratorError> {
        Self::spawn_with_output(binary, port, None).await
    }

    /// Spawn a new harness process with optional output streaming.
    ///
    /// # Arguments
    ///
    /// * `binary` - Path to the harness binary
    /// * `port` - Port for the harness to listen on
    /// * `output_label` - If Some, stream stdout/stderr with this prefix to stderr
    ///
    /// # Errors
    ///
    /// Returns an error if the process cannot be spawned.
    pub async fn spawn_with_output(
        binary: &Path,
        port: u16,
        output_label: Option<&str>,
    ) -> Result<Self, OrchestratorError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| {
                OrchestratorError::SpawnError(format!("Failed to create HTTP client: {}", e))
            })?;

        if let Some(label) = output_label {
            // Use tokio::process for async output streaming
            let mut child = TokioCommand::new(binary)
                .env("CH_PORT", port.to_string())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| {
                    OrchestratorError::SpawnError(format!(
                        "Failed to spawn {}: {}",
                        binary.display(),
                        e
                    ))
                })?;

            let mut output_tasks = Vec::new();

            // Spawn task to stream stdout
            if let Some(stdout) = child.stdout.take() {
                let label = label.to_string();
                output_tasks.push(tokio::spawn(async move {
                    let reader = BufReader::new(stdout);
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        eprintln!("[{} stdout] {}", label, line);
                    }
                }));
            }

            // Spawn task to stream stderr
            if let Some(stderr) = child.stderr.take() {
                let label = label.to_string();
                output_tasks.push(tokio::spawn(async move {
                    let reader = BufReader::new(stderr);
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        eprintln!("[{} stderr] {}", label, line);
                    }
                }));
            }

            Ok(Self {
                process: None,
                tokio_process: Some(child),
                base_url: format!("http://127.0.0.1:{}", port),
                client,
                is_managed: true,
                output_tasks,
                claim_nonce: None,
            })
        } else {
            // Use std::process without output streaming
            let process = Command::new(binary)
                .env("CH_PORT", port.to_string())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| {
                    OrchestratorError::SpawnError(format!(
                        "Failed to spawn {}: {}",
                        binary.display(),
                        e
                    ))
                })?;

            Ok(Self {
                process: Some(process),
                tokio_process: None,
                base_url: format!("http://127.0.0.1:{}", port),
                client,
                is_managed: true,
                output_tasks: Vec::new(),
                claim_nonce: None,
            })
        }
    }

    /// Connect to an already-running harness at the given URL.
    ///
    /// # Arguments
    ///
    /// * `url` - The base URL of the running harness (e.g., "http://localhost:9100")
    ///
    /// # Errors
    ///
    /// Returns an error if the URL is invalid or the client cannot be created.
    pub fn connect(url: &str) -> Result<Self, OrchestratorError> {
        // Validate URL format
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(OrchestratorError::InvalidUrl(format!(
                "URL must start with http:// or https://: {}",
                url
            )));
        }

        // Remove trailing slash if present
        let base_url = url.trim_end_matches('/').to_string();

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| {
                OrchestratorError::SpawnError(format!("Failed to create HTTP client: {}", e))
            })?;

        Ok(Self {
            process: None,
            tokio_process: None,
            base_url,
            client,
            is_managed: false,
            output_tasks: Vec::new(),
            claim_nonce: None,
        })
    }

    /// Get the base URL for this harness.
    fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Check if the harness is healthy.
    ///
    /// # Errors
    ///
    /// Returns an error if the health check fails.
    pub async fn health_check(&self) -> Result<HealthResponse, OrchestratorError> {
        let url = format!("{}/health", self.base_url());
        let response: HealthResponse = self.client.get(&url).send().await?.json().await?;

        if response.status == "healthy" {
            Ok(response)
        } else {
            Err(OrchestratorError::HarnessError(format!(
                "Unhealthy status: {}",
                response.status
            )))
        }
    }

    /// Claim exclusive access to the harness.
    ///
    /// # Errors
    ///
    /// Returns an error if the harness is already claimed by another orchestrator.
    pub async fn claim(&mut self) -> Result<(), OrchestratorError> {
        let nonce = Uuid::new_v4().to_string();
        let url = format!("{}/claim", self.base_url());
        let request = ClaimRequest::new(&nonce);

        let response: ClaimResponse = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await?
            .json()
            .await?;

        if response.success {
            self.claim_nonce = Some(nonce);
            Ok(())
        } else {
            Err(OrchestratorError::ClaimError(
                response
                    .error
                    .unwrap_or_else(|| "Unknown claim error".to_string()),
            ))
        }
    }

    /// Release the claim on the harness.
    ///
    /// # Errors
    ///
    /// Returns an error if the release request fails.
    pub async fn release(&mut self) -> Result<(), OrchestratorError> {
        if let Some(nonce) = self.claim_nonce.take() {
            let url = format!("{}/release", self.base_url());
            let request = ReleaseRequest::new(&nonce);
            let _ = self.client.post(&url).json(&request).send().await?;
        }
        Ok(())
    }

    /// Get the list of available benchmarks.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn list_benchmarks(&self) -> Result<Vec<String>, OrchestratorError> {
        let url = format!("{}/benchmarks", self.base_url());
        let mut req = self.client.get(&url);
        if let Some(nonce) = &self.claim_nonce {
            req = req.header(CLAIM_HEADER, nonce);
        }
        let response: BenchmarkListResponse = req.send().await?.json().await?;
        Ok(response.benchmarks)
    }

    /// Run a benchmark on this handle for `iterations` inner iterations.
    ///
    /// Returns total elapsed across all iterations. Callers are responsible
    /// for dividing by `iterations` to obtain a per-iteration mean.
    ///
    /// # Errors
    ///
    /// Returns an error if the iteration fails.
    pub async fn run_iteration(
        &self,
        benchmark_id: &str,
        iterations: u64,
    ) -> Result<Duration, OrchestratorError> {
        let url = format!("{}/run", self.base_url());
        let request = RunIterationRequest::new(benchmark_id, iterations);

        let mut req = self.client.post(&url).json(&request);
        if let Some(nonce) = &self.claim_nonce {
            req = req.header(CLAIM_HEADER, nonce);
        }

        let response: RunIterationResponse = req.send().await?.json().await?;

        if response.success {
            Ok(response.duration())
        } else {
            Err(OrchestratorError::HarnessError(
                response
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ))
        }
    }

    /// Request the harness to shut down gracefully.
    ///
    /// # Errors
    ///
    /// Returns an error if the shutdown request fails.
    pub async fn shutdown(&mut self) -> Result<(), OrchestratorError> {
        // Release claim first
        self.release().await?;

        let url = format!("{}/shutdown", self.base_url());
        let mut req = self.client.post(&url);
        if let Some(nonce) = &self.claim_nonce {
            req = req.header(CLAIM_HEADER, nonce);
        }
        let _response: ShutdownResponse = req.send().await?.json().await?;
        Ok(())
    }

    /// Kill the harness process forcefully (only for managed processes).
    pub fn kill(&mut self) {
        // Abort output streaming tasks
        for task in self.output_tasks.drain(..) {
            task.abort();
        }

        // Kill std::process
        if let Some(ref mut process) = self.process {
            let _ = process.kill();
        }

        // Kill tokio::process (note: this is sync, use start_kill)
        if let Some(ref mut process) = self.tokio_process {
            let _ = process.start_kill();
        }
    }

    /// Get the process ID of the harness (only for managed processes).
    pub fn pid(&self) -> Option<u32> {
        self.process
            .as_ref()
            .map(|p| p.id())
            .or_else(|| self.tokio_process.as_ref().and_then(|p| p.id()))
    }

    /// Check if this is a managed (spawned) harness.
    pub fn is_managed(&self) -> bool {
        self.is_managed
    }
}

impl Drop for HarnessHandle {
    fn drop(&mut self) {
        // Only kill managed processes
        if self.is_managed {
            self.kill();
        }
    }
}

/// Orchestrator for running comparative benchmarks.
///
/// The orchestrator manages the lifecycle of baseline and candidate harness
/// processes, collects interleaved benchmark samples, and returns the results
/// for statistical analysis.
pub struct Orchestrator {
    /// Path to the baseline harness binary.
    baseline_binary: PathBuf,
    /// Path to the candidate harness binary.
    candidate_binary: PathBuf,
    /// Base port for harness communication.
    base_port: u16,
    /// Timeout for waiting for harnesses to become ready.
    timeout: Duration,
    /// Number of samples to collect per benchmark after calibration.
    sample_size: u32,
    /// Interval between interleaved benchmark runs.
    interleave_interval: Duration,
    /// Target minimum elapsed duration for a single sample.
    target_sample: Duration,
    /// Safety cap on iteration count chosen during calibration.
    max_calibration_iters: u64,
    /// Whether to show harness stdout/stderr output.
    show_output: bool,
}

/// Collected benchmark samples for a single benchmark.
#[derive(Debug, Clone)]
pub struct BenchmarkSamples {
    /// Name of the benchmark.
    pub name: String,
    /// Samples collected from the baseline.
    pub baseline_samples: Vec<Duration>,
    /// Samples collected from the candidate.
    pub candidate_samples: Vec<Duration>,
}

impl BenchmarkSamples {
    /// Create a new empty sample collection.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            baseline_samples: Vec::new(),
            candidate_samples: Vec::new(),
        }
    }

    /// Add a baseline sample.
    pub fn add_baseline(&mut self, duration: Duration) {
        self.baseline_samples.push(duration);
    }

    /// Add a candidate sample.
    pub fn add_candidate(&mut self, duration: Duration) {
        self.candidate_samples.push(duration);
    }
}

impl Orchestrator {
    /// Create a new orchestrator.
    ///
    /// # Arguments
    ///
    /// * `baseline_binary` - Path to the baseline harness binary
    /// * `candidate_binary` - Path to the candidate harness binary
    /// * `base_port` - Base port for harness communication (baseline uses base_port, candidate uses base_port + 1)
    /// * `timeout` - Timeout for waiting for harnesses to become ready
    /// * `sample_size` - Number of samples to collect per benchmark after calibration
    /// * `interleave_interval` - Interval between interleaved benchmark runs
    /// * `target_sample` - Target minimum elapsed for a single sample (calibration target)
    /// * `max_calibration_iters` - Safety cap on the chosen iteration count
    /// * `show_output` - Whether to show harness stdout/stderr
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        baseline_binary: PathBuf,
        candidate_binary: PathBuf,
        base_port: u16,
        timeout: Duration,
        sample_size: u32,
        interleave_interval: Duration,
        target_sample: Duration,
        max_calibration_iters: u64,
        show_output: bool,
    ) -> Self {
        Self {
            baseline_binary,
            candidate_binary,
            base_port,
            timeout,
            sample_size,
            interleave_interval,
            target_sample,
            max_calibration_iters,
            show_output,
        }
    }

    /// Run the benchmark comparison.
    ///
    /// This method:
    /// 1. Spawns both harnesses
    /// 2. Waits for health checks
    /// 3. Gets benchmark lists and validates they match
    /// 4. For each benchmark:
    ///    a. Runs warmup iterations (discarded)
    ///    b. Collects interleaved samples
    /// 5. Shuts down harnesses
    /// 6. Returns results
    ///
    /// # Errors
    ///
    /// Returns an error if any step fails.
    pub async fn run(&self) -> Result<Vec<BenchmarkSamples>, OrchestratorError> {
        // 1. Spawn both harnesses
        let baseline_label = if self.show_output {
            Some("baseline")
        } else {
            None
        };
        let candidate_label = if self.show_output {
            Some("candidate")
        } else {
            None
        };

        let mut baseline =
            HarnessHandle::spawn_with_output(&self.baseline_binary, self.base_port, baseline_label)
                .await?;
        let mut candidate = HarnessHandle::spawn_with_output(
            &self.candidate_binary,
            self.base_port + 1,
            candidate_label,
        )
        .await?;

        // Use a guard to ensure harnesses are killed on error
        let result = self
            .run_with_harnesses(&mut baseline, &mut candidate, self.timeout)
            .await;

        // 5. Shutdown harnesses (attempt graceful shutdown, then kill)
        let _ = baseline.shutdown().await;
        let _ = candidate.shutdown().await;

        // Give processes a moment to exit gracefully
        sleep(Duration::from_millis(100)).await;

        // Force kill if still running
        baseline.kill();
        candidate.kill();

        result
    }

    /// Run benchmarks with already-spawned harnesses.
    async fn run_with_harnesses(
        &self,
        baseline: &mut HarnessHandle,
        candidate: &mut HarnessHandle,
        timeout: Duration,
    ) -> Result<Vec<BenchmarkSamples>, OrchestratorError> {
        // 2. Wait for health checks
        eprint!("  Waiting for baseline harness... ");
        wait_for_health(baseline, timeout).await?;
        eprintln!("ready");

        eprint!("  Waiting for candidate harness... ");
        wait_for_health(candidate, timeout).await?;
        eprintln!("ready");

        // 3. Claim exclusive access to both harnesses
        eprint!("  Claiming baseline harness... ");
        baseline.claim().await?;
        eprintln!("claimed");

        eprint!("  Claiming candidate harness... ");
        candidate.claim().await?;
        eprintln!("claimed");

        // 3. Get benchmark lists and validate they match
        let baseline_benchmarks = baseline.list_benchmarks().await?;
        let candidate_benchmarks = candidate.list_benchmarks().await?;

        // Compare as sets (order doesn't matter)
        let mut baseline_sorted = baseline_benchmarks.clone();
        let mut candidate_sorted = candidate_benchmarks.clone();
        baseline_sorted.sort();
        candidate_sorted.sort();

        if baseline_sorted != candidate_sorted {
            return Err(OrchestratorError::BenchmarkMismatch {
                baseline: baseline_benchmarks,
                candidate: candidate_benchmarks,
            });
        }

        eprintln!(
            "  Found {} benchmark(s): {}",
            baseline_sorted.len(),
            baseline_sorted.join(", ")
        );

        // 4. For each benchmark, collect samples
        let mut results = Vec::new();
        let total_benchmarks = baseline_benchmarks.len();

        for (idx, benchmark_name) in baseline_benchmarks.iter().enumerate() {
            eprintln!("  [{}/{}] {}", idx + 1, total_benchmarks, benchmark_name);
            let samples = self
                .collect_benchmark_samples(benchmark_name, baseline, candidate)
                .await?;
            results.push(samples);
        }

        Ok(results)
    }

    /// Collect interleaved samples for a single benchmark.
    ///
    /// Calibrates the iteration count on the baseline handle (geometric
    /// doubling up to `target_sample`), then collects `sample_size` samples
    /// at that fixed iteration count on both sides. Records per-iteration
    /// means (total elapsed / n) so Welch's t-test operates on comparable
    /// units regardless of the chosen `n`.
    async fn collect_benchmark_samples(
        &self,
        benchmark_name: &str,
        baseline: &HarnessHandle,
        candidate: &HarnessHandle,
    ) -> Result<BenchmarkSamples, OrchestratorError> {
        let mut samples = BenchmarkSamples::new(benchmark_name);

        // Calibrate iteration count on baseline; reuse for candidate.
        eprint!("      calibrating... ");
        let iters = calibrate_iterations(
            baseline,
            benchmark_name,
            self.target_sample,
            self.max_calibration_iters,
        )
        .await?;
        eprintln!("n={}", iters);

        // Collect interleaved samples at fixed `iters`.
        eprint!(
            "      collecting {} samples (n={})... ",
            self.sample_size, iters
        );
        for i in 0..self.sample_size {
            self.run_interleaved_pair(
                benchmark_name,
                baseline,
                candidate,
                i % 2 == 0,
                iters,
                &mut samples,
            )
            .await?;

            // Progress indicator every 10 samples
            if (i + 1) % 10 == 0 {
                eprint!("{}", i + 1);
                if i + 1 < self.sample_size {
                    eprint!("...");
                }
            }
        }
        eprintln!(" done");

        Ok(samples)
    }

    /// Run one baseline/candidate pair, alternating which side goes first.
    ///
    /// Both durations are divided by `iters` and recorded as per-iteration
    /// means.
    #[allow(clippy::too_many_arguments)]
    async fn run_interleaved_pair(
        &self,
        benchmark_name: &str,
        baseline: &HarnessHandle,
        candidate: &HarnessHandle,
        baseline_first: bool,
        iters: u64,
        samples: &mut BenchmarkSamples,
    ) -> Result<(), OrchestratorError> {
        let (first_handle, second_handle, first_is_baseline) = if baseline_first {
            (baseline, candidate, true)
        } else {
            (candidate, baseline, false)
        };

        let first_elapsed = first_handle.run_iteration(benchmark_name, iters).await?;
        sleep(self.interleave_interval).await;
        let second_elapsed = second_handle.run_iteration(benchmark_name, iters).await?;
        sleep(self.interleave_interval).await;

        let first_per_iter = per_iter_mean(first_elapsed, iters);
        let second_per_iter = per_iter_mean(second_elapsed, iters);

        if first_is_baseline {
            samples.add_baseline(first_per_iter);
            samples.add_candidate(second_per_iter);
        } else {
            samples.add_candidate(first_per_iter);
            samples.add_baseline(second_per_iter);
        }

        Ok(())
    }
}

/// Divide an elapsed duration by an iteration count to get per-iteration mean.
fn per_iter_mean(elapsed: Duration, iters: u64) -> Duration {
    if iters == 0 {
        Duration::ZERO
    } else {
        Duration::from_nanos((elapsed.as_nanos() as u64) / iters)
    }
}

/// Calibrate iteration count so one sample meets the target elapsed duration.
///
/// Starts at `n = 1` and grows geometrically (up to 10× per step) until one
/// call to the benchmark returns elapsed ≥ `target`, or until `max_iters`.
/// Returns the chosen `n`.
///
/// The calibration runs only on the baseline handle; the same `n` is reused
/// for candidate samples so that per-iteration means are directly comparable.
async fn calibrate_iterations(
    handle: &HarnessHandle,
    benchmark_name: &str,
    target: Duration,
    max_iters: u64,
) -> Result<u64, OrchestratorError> {
    let mut n: u64 = 1;
    loop {
        let elapsed = handle.run_iteration(benchmark_name, n).await?;
        if elapsed >= target || n >= max_iters {
            return Ok(n);
        }
        // Pick next n so that elapsed * (next_n / n) is roughly target.
        // Cap growth at 10× per step to avoid overshooting on very fast benches.
        let elapsed_ns = elapsed.as_nanos().max(1) as f64;
        let target_ns = target.as_nanos() as f64;
        let scale = (target_ns / elapsed_ns).min(10.0);
        let next = ((n as f64) * scale).ceil() as u64;
        n = next.max(n + 1).min(max_iters);
    }
}

/// Wait for a harness to become healthy, with retries. Also verifies that the
/// harness reports a protocol version the orchestrator knows how to talk to.
///
/// Protocol mismatch is *not* retried — a v1 harness won't become v2 by waiting.
pub async fn wait_for_health(
    harness: &HarnessHandle,
    timeout: Duration,
) -> Result<(), OrchestratorError> {
    let start = std::time::Instant::now();
    let retry_interval = Duration::from_millis(100);
    let mut last_error: Option<OrchestratorError> = None;

    loop {
        match harness.health_check().await {
            Ok(response) => {
                if response.protocol_version != PROTOCOL_VERSION {
                    return Err(OrchestratorError::ProtocolVersionMismatch {
                        url: harness.base_url().to_string(),
                        expected: PROTOCOL_VERSION,
                        actual: response.protocol_version,
                    });
                }
                return Ok(());
            }
            Err(e) if start.elapsed() < timeout => {
                last_error = Some(e);
                sleep(retry_interval).await;
            }
            Err(e) => {
                let error_msg = last_error
                    .map(|le| le.to_string())
                    .unwrap_or_else(|| e.to_string());
                return Err(OrchestratorError::TimeoutError {
                    url: harness.base_url().to_string(),
                    timeout_secs: timeout.as_secs(),
                    last_error: error_msg,
                });
            }
        }
    }
}

/// Run benchmark comparison using pre-running harnesses at the given URLs.
///
/// This function connects to already-running harnesses instead of spawning new ones.
/// The harnesses are NOT shut down after the comparison completes.
///
/// # Arguments
///
/// * `baseline_url` - URL of the baseline harness (e.g., "http://localhost:9100")
/// * `candidate_url` - URL of the candidate harness (e.g., "http://localhost:9101")
/// * `timeout` - Timeout for waiting for harnesses to become healthy
/// * `sample_size` - Number of samples to collect per benchmark after calibration
/// * `interleave_interval` - Interval between interleaved benchmark runs
/// * `target_sample` - Target minimum elapsed for a single sample (calibration target)
/// * `max_calibration_iters` - Safety cap on the iteration count chosen during calibration
pub async fn run_with_urls(
    baseline_url: &str,
    candidate_url: &str,
    timeout: Duration,
    sample_size: u32,
    interleave_interval: Duration,
    target_sample: Duration,
    max_calibration_iters: u64,
) -> Result<Vec<BenchmarkSamples>, OrchestratorError> {
    // Connect to remote harnesses
    let mut baseline = HarnessHandle::connect(baseline_url)?;
    let mut candidate = HarnessHandle::connect(candidate_url)?;

    // Wait for health checks (also verifies protocol version)
    eprint!("  Waiting for baseline harness... ");
    wait_for_health(&baseline, timeout).await?;
    eprintln!("ready");

    eprint!("  Waiting for candidate harness... ");
    wait_for_health(&candidate, timeout).await?;
    eprintln!("ready");

    // Claim exclusive access to both harnesses
    eprint!("  Claiming baseline harness... ");
    baseline.claim().await?;
    eprintln!("claimed");

    eprint!("  Claiming candidate harness... ");
    candidate.claim().await?;
    eprintln!("claimed");

    // Get benchmark lists and validate they match
    let baseline_benchmarks = baseline.list_benchmarks().await?;
    let candidate_benchmarks = candidate.list_benchmarks().await?;

    // Compare as sets (order doesn't matter)
    let mut baseline_sorted = baseline_benchmarks.clone();
    let mut candidate_sorted = candidate_benchmarks.clone();
    baseline_sorted.sort();
    candidate_sorted.sort();

    if baseline_sorted != candidate_sorted {
        return Err(OrchestratorError::BenchmarkMismatch {
            baseline: baseline_benchmarks,
            candidate: candidate_benchmarks,
        });
    }

    eprintln!(
        "  Found {} benchmark(s): {}",
        baseline_sorted.len(),
        baseline_sorted.join(", ")
    );

    // Collect samples for each benchmark
    let mut results = Vec::new();
    let total_benchmarks = baseline_benchmarks.len();

    for (idx, benchmark_name) in baseline_benchmarks.iter().enumerate() {
        eprintln!("  [{}/{}] {}", idx + 1, total_benchmarks, benchmark_name);

        let mut samples = BenchmarkSamples::new(benchmark_name);

        eprint!("      calibrating... ");
        let iters = calibrate_iterations(
            &baseline,
            benchmark_name,
            target_sample,
            max_calibration_iters,
        )
        .await?;
        eprintln!("n={}", iters);

        eprint!("      collecting {} samples (n={})... ", sample_size, iters);
        for i in 0..sample_size {
            let baseline_first = i % 2 == 0;
            let (first_handle, second_handle, first_is_baseline) = if baseline_first {
                (&baseline, &candidate, true)
            } else {
                (&candidate, &baseline, false)
            };

            let first_elapsed = first_handle.run_iteration(benchmark_name, iters).await?;
            sleep(interleave_interval).await;
            let second_elapsed = second_handle.run_iteration(benchmark_name, iters).await?;
            sleep(interleave_interval).await;

            let first_per_iter = per_iter_mean(first_elapsed, iters);
            let second_per_iter = per_iter_mean(second_elapsed, iters);

            if first_is_baseline {
                samples.add_baseline(first_per_iter);
                samples.add_candidate(second_per_iter);
            } else {
                samples.add_candidate(first_per_iter);
                samples.add_baseline(second_per_iter);
            }

            // Progress indicator every 10 samples
            if (i + 1) % 10 == 0 {
                eprint!("{}", i + 1);
                if i + 1 < sample_size {
                    eprint!("...");
                }
            }
        }
        eprintln!(" done");

        results.push(samples);
    }

    // Release claims (but don't shutdown - remote harnesses are managed externally)
    let _ = baseline.release().await;
    let _ = candidate.release().await;

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_benchmark_samples_new() {
        let samples = BenchmarkSamples::new("test_benchmark");
        assert_eq!(samples.name, "test_benchmark");
        assert!(samples.baseline_samples.is_empty());
        assert!(samples.candidate_samples.is_empty());
    }

    #[test]
    fn test_benchmark_samples_add() {
        let mut samples = BenchmarkSamples::new("test");

        samples.add_baseline(Duration::from_micros(100));
        samples.add_baseline(Duration::from_micros(110));
        samples.add_candidate(Duration::from_micros(95));
        samples.add_candidate(Duration::from_micros(105));

        assert_eq!(samples.baseline_samples.len(), 2);
        assert_eq!(samples.candidate_samples.len(), 2);
        assert_eq!(samples.baseline_samples[0], Duration::from_micros(100));
        assert_eq!(samples.candidate_samples[1], Duration::from_micros(105));
    }

    #[test]
    fn test_orchestrator_new() {
        let orchestrator = Orchestrator::new(
            PathBuf::from("/path/to/baseline"),
            PathBuf::from("/path/to/candidate"),
            9100,
            Duration::from_secs(30),
            100,
            Duration::from_millis(100),
            Duration::from_millis(10),
            1_000_000_000,
            false,
        );

        assert_eq!(orchestrator.base_port, 9100);
        assert!(!orchestrator.show_output);
        assert_eq!(orchestrator.sample_size, 100);
        assert_eq!(orchestrator.target_sample, Duration::from_millis(10));
        assert_eq!(orchestrator.max_calibration_iters, 1_000_000_000);
    }

    #[test]
    fn test_per_iter_mean() {
        assert_eq!(
            per_iter_mean(Duration::from_nanos(1000), 10),
            Duration::from_nanos(100)
        );
        assert_eq!(per_iter_mean(Duration::from_nanos(100), 0), Duration::ZERO);
    }

    #[test]
    fn test_harness_handle_connect_valid() {
        let handle = HarnessHandle::connect("http://localhost:9100").unwrap();
        assert!(!handle.is_managed());
        assert_eq!(handle.base_url(), "http://localhost:9100");
    }

    #[test]
    fn test_harness_handle_connect_trailing_slash() {
        let handle = HarnessHandle::connect("http://localhost:9100/").unwrap();
        assert_eq!(handle.base_url(), "http://localhost:9100");
    }

    #[test]
    fn test_harness_handle_connect_invalid_url() {
        let result = HarnessHandle::connect("not-a-url");
        assert!(result.is_err());
        match result {
            Err(OrchestratorError::InvalidUrl(_)) => {}
            _ => panic!("Expected InvalidUrl error"),
        }
    }

    #[test]
    fn test_orchestrator_error_display() {
        let err = OrchestratorError::SpawnError("test error".to_string());
        assert_eq!(err.to_string(), "Failed to spawn harness: test error");

        let err = OrchestratorError::TimeoutError {
            url: "http://localhost:9100".to_string(),
            timeout_secs: 30,
            last_error: "connection refused".to_string(),
        };
        assert!(err.to_string().contains("not ready after"));
        assert!(err.to_string().contains("30s timeout"));
        assert!(err.to_string().contains("connection refused"));

        let err = OrchestratorError::BenchmarkMismatch {
            baseline: vec!["a".to_string(), "b".to_string()],
            candidate: vec!["a".to_string(), "c".to_string()],
        };
        assert!(err.to_string().contains("Benchmark mismatch"));

        let err = OrchestratorError::BenchmarkNotFound("missing".to_string());
        assert_eq!(err.to_string(), "Benchmark not found: missing");

        let err = OrchestratorError::HarnessError("crash".to_string());
        assert_eq!(err.to_string(), "Harness error: crash");

        let err = OrchestratorError::InvalidUrl("bad-url".to_string());
        assert_eq!(err.to_string(), "Invalid URL: bad-url");
    }
}
