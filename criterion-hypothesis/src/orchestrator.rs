//! Test orchestrator for managing benchmark harness processes.
//!
//! The orchestrator spawns baseline and candidate harness processes, manages their
//! lifecycle, and collects interleaved benchmark samples for statistical comparison.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use thiserror::Error;
use tokio::time::sleep;

use criterion_hypothesis_core::protocol::{
    BenchmarkListResponse, HealthResponse, RunIterationRequest, RunIterationResponse,
    ShutdownResponse,
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
}

/// Handle to a running harness process (spawned by us).
pub struct HarnessHandle {
    /// The child process (None for remote harnesses).
    process: Option<Child>,
    /// Base URL for the harness.
    base_url: String,
    /// HTTP client for communication.
    client: reqwest::Client,
    /// Whether this is a managed process (spawned by us) or remote.
    is_managed: bool,
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

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| {
                OrchestratorError::SpawnError(format!("Failed to create HTTP client: {}", e))
            })?;

        Ok(Self {
            process: Some(process),
            base_url: format!("http://127.0.0.1:{}", port),
            client,
            is_managed: true,
        })
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
            base_url,
            client,
            is_managed: false,
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

    /// Get the list of available benchmarks.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn list_benchmarks(&self) -> Result<Vec<String>, OrchestratorError> {
        let url = format!("{}/benchmarks", self.base_url());
        let response: BenchmarkListResponse = self.client.get(&url).send().await?.json().await?;
        Ok(response.benchmarks)
    }

    /// Run a single iteration of a benchmark.
    ///
    /// # Arguments
    ///
    /// * `benchmark_id` - The identifier of the benchmark to run
    ///
    /// # Errors
    ///
    /// Returns an error if the iteration fails.
    pub async fn run_iteration(&self, benchmark_id: &str) -> Result<Duration, OrchestratorError> {
        let url = format!("{}/run", self.base_url());
        let request = RunIterationRequest::new(benchmark_id);

        let response: RunIterationResponse = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await?
            .json()
            .await?;

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
    pub async fn shutdown(&self) -> Result<(), OrchestratorError> {
        let url = format!("{}/shutdown", self.base_url());
        let _response: ShutdownResponse = self.client.post(&url).send().await?.json().await?;
        Ok(())
    }

    /// Kill the harness process forcefully (only for managed processes).
    pub fn kill(&mut self) {
        if let Some(ref mut process) = self.process {
            let _ = process.kill();
        }
    }

    /// Get the process ID of the harness (only for managed processes).
    pub fn pid(&self) -> Option<u32> {
        self.process.as_ref().map(|p| p.id())
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
    /// Number of warmup iterations to discard.
    warmup_iterations: u32,
    /// Number of samples to collect.
    sample_size: u32,
    /// Interval between interleaved benchmark runs.
    interleave_interval: Duration,
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
    /// * `warmup_iterations` - Number of warmup iterations to discard
    /// * `sample_size` - Number of samples to collect
    /// * `interleave_interval` - Interval between interleaved benchmark runs
    pub fn new(
        baseline_binary: PathBuf,
        candidate_binary: PathBuf,
        base_port: u16,
        timeout: Duration,
        warmup_iterations: u32,
        sample_size: u32,
        interleave_interval: Duration,
    ) -> Self {
        Self {
            baseline_binary,
            candidate_binary,
            base_port,
            timeout,
            warmup_iterations,
            sample_size,
            interleave_interval,
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
        let mut baseline = HarnessHandle::spawn(&self.baseline_binary, self.base_port).await?;
        let mut candidate =
            HarnessHandle::spawn(&self.candidate_binary, self.base_port + 1).await?;

        // Use a guard to ensure harnesses are killed on error
        let result = self
            .run_with_harnesses(&baseline, &candidate, self.timeout)
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
        baseline: &HarnessHandle,
        candidate: &HarnessHandle,
        timeout: Duration,
    ) -> Result<Vec<BenchmarkSamples>, OrchestratorError> {
        // 2. Wait for health checks
        wait_for_health(baseline, timeout).await?;
        wait_for_health(candidate, timeout).await?;

        // 3. Get benchmark lists and validate they match
        let baseline_benchmarks = baseline.list_benchmarks().await?;
        let candidate_benchmarks = candidate.list_benchmarks().await?;

        if baseline_benchmarks != candidate_benchmarks {
            return Err(OrchestratorError::BenchmarkMismatch {
                baseline: baseline_benchmarks,
                candidate: candidate_benchmarks,
            });
        }

        // 4. For each benchmark, collect samples
        let mut results = Vec::new();

        for benchmark_name in &baseline_benchmarks {
            let samples = self
                .collect_benchmark_samples(benchmark_name, baseline, candidate)
                .await?;
            results.push(samples);
        }

        Ok(results)
    }

    /// Collect interleaved samples for a single benchmark.
    async fn collect_benchmark_samples(
        &self,
        benchmark_name: &str,
        baseline: &HarnessHandle,
        candidate: &HarnessHandle,
    ) -> Result<BenchmarkSamples, OrchestratorError> {
        let mut samples = BenchmarkSamples::new(benchmark_name);

        // Run warmup iterations (discarded)
        for _ in 0..self.warmup_iterations {
            baseline.run_iteration(benchmark_name).await?;
            sleep(self.interleave_interval).await;
            candidate.run_iteration(benchmark_name).await?;
            sleep(self.interleave_interval).await;
        }

        // Collect interleaved samples
        for _ in 0..self.sample_size {
            // Run baseline
            let baseline_duration = baseline.run_iteration(benchmark_name).await?;
            samples.add_baseline(baseline_duration);

            // Wait between runs
            sleep(self.interleave_interval).await;

            // Run candidate
            let candidate_duration = candidate.run_iteration(benchmark_name).await?;
            samples.add_candidate(candidate_duration);

            // Wait before next pair
            sleep(self.interleave_interval).await;
        }

        Ok(samples)
    }
}

/// Wait for a harness to become healthy, with retries.
pub async fn wait_for_health(
    harness: &HarnessHandle,
    timeout: Duration,
) -> Result<(), OrchestratorError> {
    let start = std::time::Instant::now();
    let retry_interval = Duration::from_millis(100);
    let mut last_error: Option<OrchestratorError> = None;

    loop {
        match harness.health_check().await {
            Ok(_) => return Ok(()),
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
/// * `warmup_iterations` - Number of warmup iterations to discard
/// * `sample_size` - Number of samples to collect
/// * `interleave_interval` - Interval between interleaved benchmark runs
pub async fn run_with_urls(
    baseline_url: &str,
    candidate_url: &str,
    timeout: Duration,
    warmup_iterations: u32,
    sample_size: u32,
    interleave_interval: Duration,
) -> Result<Vec<BenchmarkSamples>, OrchestratorError> {
    // Connect to remote harnesses
    let baseline = HarnessHandle::connect(baseline_url)?;
    let candidate = HarnessHandle::connect(candidate_url)?;

    // Wait for health checks
    wait_for_health(&baseline, timeout).await?;
    wait_for_health(&candidate, timeout).await?;

    // Get benchmark lists and validate they match
    let baseline_benchmarks = baseline.list_benchmarks().await?;
    let candidate_benchmarks = candidate.list_benchmarks().await?;

    if baseline_benchmarks != candidate_benchmarks {
        return Err(OrchestratorError::BenchmarkMismatch {
            baseline: baseline_benchmarks,
            candidate: candidate_benchmarks,
        });
    }

    // Collect samples for each benchmark
    let mut results = Vec::new();

    for benchmark_name in &baseline_benchmarks {
        let mut samples = BenchmarkSamples::new(benchmark_name);

        // Run warmup iterations (discarded)
        for _ in 0..warmup_iterations {
            baseline.run_iteration(benchmark_name).await?;
            sleep(interleave_interval).await;
            candidate.run_iteration(benchmark_name).await?;
            sleep(interleave_interval).await;
        }

        // Collect interleaved samples
        for _ in 0..sample_size {
            let baseline_duration = baseline.run_iteration(benchmark_name).await?;
            samples.add_baseline(baseline_duration);

            sleep(interleave_interval).await;

            let candidate_duration = candidate.run_iteration(benchmark_name).await?;
            samples.add_candidate(candidate_duration);

            sleep(interleave_interval).await;
        }

        results.push(samples);
    }

    // Note: We do NOT shutdown remote harnesses - they're managed externally

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
            3,
            100,
            Duration::from_millis(100),
        );

        assert_eq!(orchestrator.base_port, 9100);
        assert_eq!(orchestrator.warmup_iterations, 3);
        assert_eq!(orchestrator.sample_size, 100);
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
