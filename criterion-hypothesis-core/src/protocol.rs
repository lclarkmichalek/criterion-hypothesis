use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Protocol version spoken by this crate.
///
/// Bump this integer whenever the wire format of any request/response changes
/// in a way that is not forward- or backward-compatible. Orchestrators check
/// the harness's reported version at connect time and refuse to proceed on
/// mismatch so the failure surfaces as a clear upgrade instruction rather
/// than a cryptic JSON parse error.
///
/// History:
/// - `1` — original protocol (single `Fn() -> Duration` samples).
/// - `2` — harness accepts `iterations` and returns total elapsed for a batch.
pub const PROTOCOL_VERSION: u32 = 2;

/// Default protocol version assumed when the harness doesn't report one.
/// This is 1 so that a v2 orchestrator talking to a pre-versioning harness
/// (which doesn't emit the field) is correctly identified as a v1 peer.
fn default_protocol_version() -> u32 {
    1
}

/// Health check response from the benchmark harness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    /// Protocol version the harness implements.
    #[serde(default = "default_protocol_version")]
    pub protocol_version: u32,
}

impl HealthResponse {
    /// Create a healthy response advertising this crate's protocol version.
    pub fn healthy() -> Self {
        Self {
            status: "healthy".to_string(),
            protocol_version: PROTOCOL_VERSION,
        }
    }
}

/// Response containing the list of available benchmarks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkListResponse {
    pub benchmarks: Vec<String>,
}

impl BenchmarkListResponse {
    /// Create a new benchmark list response.
    pub fn new(benchmarks: Vec<String>) -> Self {
        Self { benchmarks }
    }
}

/// Request to run a benchmark for a specified number of inner iterations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunIterationRequest {
    pub benchmark_id: String,
    /// Number of inner iterations the harness should loop the benchmark before
    /// reporting elapsed. Must be >= 1.
    pub iterations: u64,
}

impl RunIterationRequest {
    /// Create a new run iteration request.
    pub fn new(benchmark_id: impl Into<String>, iterations: u64) -> Self {
        Self {
            benchmark_id: benchmark_id.into(),
            iterations,
        }
    }
}

/// Response from running a benchmark for some number of inner iterations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunIterationResponse {
    /// Whether the iteration completed successfully.
    pub success: bool,
    /// Echo of the iteration count the harness actually ran.
    pub iterations: u64,
    /// Total elapsed duration across all `iterations` calls, in nanoseconds.
    pub duration_ns: u64,
    /// Error message if the iteration failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RunIterationResponse {
    /// Create a successful iteration response.
    pub fn success(iterations: u64, duration: Duration) -> Self {
        Self {
            success: true,
            iterations,
            duration_ns: duration.as_nanos() as u64,
            error: None,
        }
    }

    /// Create a failed iteration response.
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            iterations: 0,
            duration_ns: 0,
            error: Some(error.into()),
        }
    }

    /// Get the total elapsed duration as a `Duration`.
    pub fn duration(&self) -> Duration {
        Duration::from_nanos(self.duration_ns)
    }

    /// Per-iteration mean duration (`duration / iterations`).
    /// Returns `Duration::ZERO` if `iterations == 0`.
    pub fn per_iter(&self) -> Duration {
        if self.iterations == 0 {
            Duration::ZERO
        } else {
            Duration::from_nanos(self.duration_ns / self.iterations)
        }
    }
}

/// Response to a shutdown request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShutdownResponse {
    pub status: String,
}

impl ShutdownResponse {
    /// Create a shutdown acknowledgment response.
    pub fn acknowledged() -> Self {
        Self {
            status: "shutting_down".to_string(),
        }
    }
}

/// Request to claim exclusive access to the harness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimRequest {
    /// Unique nonce identifying this orchestrator session.
    pub nonce: String,
}

impl ClaimRequest {
    /// Create a new claim request.
    pub fn new(nonce: impl Into<String>) -> Self {
        Self {
            nonce: nonce.into(),
        }
    }
}

/// Response to a claim request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimResponse {
    /// Whether the claim was successful.
    pub success: bool,
    /// Error message if claim failed (e.g., already claimed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ClaimResponse {
    /// Create a successful claim response.
    pub fn success() -> Self {
        Self {
            success: true,
            error: None,
        }
    }

    /// Create a failed claim response (already claimed by another orchestrator).
    pub fn already_claimed() -> Self {
        Self {
            success: false,
            error: Some("Harness is already claimed by another orchestrator".to_string()),
        }
    }
}

/// Request to release a claim on the harness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseRequest {
    /// The nonce that was used to claim the harness.
    pub nonce: String,
}

impl ReleaseRequest {
    /// Create a new release request.
    pub fn new(nonce: impl Into<String>) -> Self {
        Self {
            nonce: nonce.into(),
        }
    }
}

/// Response to a release request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseResponse {
    /// Whether the release was successful.
    pub success: bool,
}

impl ReleaseResponse {
    /// Create a successful release response.
    pub fn success() -> Self {
        Self { success: true }
    }
}

/// Header name for the claim nonce.
pub const CLAIM_HEADER: &str = "X-Harness-Claim";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_response_healthy() {
        let response = HealthResponse::healthy();
        assert_eq!(response.status, "healthy");
        assert_eq!(response.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn test_health_response_missing_version_defaults_to_one() {
        // A pre-versioning harness emits `{"status": "healthy"}` with no
        // protocol_version field. serde must coerce that to version 1.
        let legacy: HealthResponse = serde_json::from_str(r#"{"status":"healthy"}"#).unwrap();
        assert_eq!(legacy.status, "healthy");
        assert_eq!(legacy.protocol_version, 1);
    }

    #[test]
    fn test_health_response_roundtrip_preserves_version() {
        let response = HealthResponse::healthy();
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"protocol_version\""));
        let parsed: HealthResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn test_benchmark_list_response() {
        let benchmarks = vec!["bench1".to_string(), "bench2".to_string()];
        let response = BenchmarkListResponse::new(benchmarks.clone());
        assert_eq!(response.benchmarks, benchmarks);
    }

    #[test]
    fn test_run_iteration_request() {
        let request = RunIterationRequest::new("my_benchmark", 1);
        assert_eq!(request.benchmark_id, "my_benchmark");
        assert_eq!(request.iterations, 1);
    }

    #[test]
    fn test_run_iteration_request_with_iterations() {
        let request = RunIterationRequest::new("bench_a", 128);
        assert_eq!(request.benchmark_id, "bench_a");
        assert_eq!(request.iterations, 128);
    }

    #[test]
    fn test_run_iteration_request_roundtrip_includes_iterations() {
        let original = RunIterationRequest::new("my_bench", 42);
        let json = serde_json::to_string(&original).unwrap();
        let parsed: RunIterationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.benchmark_id, "my_bench");
        assert_eq!(parsed.iterations, 42);
    }

    #[test]
    fn test_run_iteration_response_success() {
        let duration = Duration::from_micros(1500);
        let response = RunIterationResponse::success(10, duration);

        assert!(response.success);
        assert_eq!(response.iterations, 10);
        assert_eq!(response.duration_ns, 1_500_000);
        assert!(response.error.is_none());
        assert_eq!(response.duration(), duration);
    }

    #[test]
    fn test_run_iteration_response_per_iter() {
        let response = RunIterationResponse::success(10, Duration::from_nanos(1000));
        assert_eq!(response.per_iter(), Duration::from_nanos(100));

        let zero = RunIterationResponse::failure("oops");
        assert_eq!(zero.per_iter(), Duration::ZERO);
    }

    #[test]
    fn test_run_iteration_response_echoes_iterations() {
        let response = RunIterationResponse::success(64, Duration::from_nanos(12800));
        assert!(response.success);
        assert_eq!(response.iterations, 64);
        assert_eq!(response.duration_ns, 12800);
    }

    #[test]
    fn test_run_iteration_response_failure() {
        let response = RunIterationResponse::failure("benchmark panicked");

        assert!(!response.success);
        assert_eq!(response.iterations, 0);
        assert_eq!(response.duration_ns, 0);
        assert_eq!(response.error, Some("benchmark panicked".to_string()));
    }

    #[test]
    fn test_shutdown_response() {
        let response = ShutdownResponse::acknowledged();
        assert_eq!(response.status, "shutting_down");
    }

    #[test]
    fn test_serialization_roundtrip() {
        let response = RunIterationResponse::success(5, Duration::from_nanos(12345));
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: RunIterationResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(response.duration_ns, deserialized.duration_ns);
        assert_eq!(response.iterations, deserialized.iterations);
        assert_eq!(response.success, deserialized.success);
        assert_eq!(response.error, deserialized.error);
    }

    #[test]
    fn test_error_field_skipped_when_none() {
        let response = RunIterationResponse::success(1, Duration::from_nanos(100));
        let json = serde_json::to_string(&response).unwrap();

        // The error field should not be present in the JSON
        assert!(!json.contains("error"));
    }
}
