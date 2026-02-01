use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Health check response from the benchmark harness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
}

impl HealthResponse {
    /// Create a healthy response.
    pub fn healthy() -> Self {
        Self {
            status: "healthy".to_string(),
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

/// Request to run a single iteration of a benchmark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunIterationRequest {
    pub benchmark_id: String,
}

impl RunIterationRequest {
    /// Create a new run iteration request.
    pub fn new(benchmark_id: impl Into<String>) -> Self {
        Self {
            benchmark_id: benchmark_id.into(),
        }
    }
}

/// Response from running a single benchmark iteration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunIterationResponse {
    /// Duration of the iteration in nanoseconds.
    pub duration_ns: u64,
    /// Whether the iteration completed successfully.
    pub success: bool,
    /// Error message if the iteration failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RunIterationResponse {
    /// Create a successful iteration response.
    pub fn success(duration: Duration) -> Self {
        Self {
            duration_ns: duration.as_nanos() as u64,
            success: true,
            error: None,
        }
    }

    /// Create a failed iteration response.
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            duration_ns: 0,
            success: false,
            error: Some(error.into()),
        }
    }

    /// Get the duration as a `Duration` type.
    pub fn duration(&self) -> Duration {
        Duration::from_nanos(self.duration_ns)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_response_healthy() {
        let response = HealthResponse::healthy();
        assert_eq!(response.status, "healthy");
    }

    #[test]
    fn test_benchmark_list_response() {
        let benchmarks = vec!["bench1".to_string(), "bench2".to_string()];
        let response = BenchmarkListResponse::new(benchmarks.clone());
        assert_eq!(response.benchmarks, benchmarks);
    }

    #[test]
    fn test_run_iteration_request() {
        let request = RunIterationRequest::new("my_benchmark");
        assert_eq!(request.benchmark_id, "my_benchmark");
    }

    #[test]
    fn test_run_iteration_response_success() {
        let duration = Duration::from_micros(1500);
        let response = RunIterationResponse::success(duration);

        assert!(response.success);
        assert_eq!(response.duration_ns, 1_500_000);
        assert!(response.error.is_none());
        assert_eq!(response.duration(), duration);
    }

    #[test]
    fn test_run_iteration_response_failure() {
        let response = RunIterationResponse::failure("benchmark panicked");

        assert!(!response.success);
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
        let response = RunIterationResponse::success(Duration::from_nanos(12345));
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: RunIterationResponse = serde_json::from_str(&json).unwrap();

        assert_eq!(response.duration_ns, deserialized.duration_ns);
        assert_eq!(response.success, deserialized.success);
        assert_eq!(response.error, deserialized.error);
    }

    #[test]
    fn test_error_field_skipped_when_none() {
        let response = RunIterationResponse::success(Duration::from_nanos(100));
        let json = serde_json::to_string(&response).unwrap();

        // The error field should not be present in the JSON
        assert!(!json.contains("error"));
    }
}
