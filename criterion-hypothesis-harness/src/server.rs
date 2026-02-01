//! HTTP server for the benchmark harness.
//!
//! This module provides an HTTP server that exposes benchmark functions
//! for external orchestration. The server supports health checks, listing
//! benchmarks, running individual iterations, and graceful shutdown.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::BenchmarkRegistry;

/// Shared state for the HTTP server.
struct AppState {
    /// The benchmark registry containing all registered benchmarks.
    registry: Arc<BenchmarkRegistry>,
    /// Sender to signal shutdown.
    shutdown_tx: watch::Sender<bool>,
}

/// Health check response.
#[derive(Serialize, Deserialize)]
struct HealthResponse {
    status: String,
}

/// Response containing the list of available benchmarks.
#[derive(Serialize, Deserialize)]
struct BenchmarksResponse {
    benchmarks: Vec<String>,
}

/// Request to run a single benchmark iteration.
#[derive(Deserialize)]
struct RunIterationRequest {
    benchmark_id: String,
}

/// Response from running a benchmark iteration.
#[derive(Serialize, Deserialize)]
struct RunIterationResponse {
    duration_ns: u64,
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Response from shutdown request.
#[derive(Serialize, Deserialize)]
struct ShutdownResponse {
    message: String,
}

/// Health check endpoint.
///
/// GET /health
/// Returns: { "status": "ready" }
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
    })
}

/// List all available benchmarks.
///
/// GET /benchmarks
/// Returns: { "benchmarks": ["bench1", "bench2", ...] }
async fn list_benchmarks(State(state): State<Arc<AppState>>) -> Json<BenchmarksResponse> {
    let benchmarks = state.registry.list();
    Json(BenchmarksResponse { benchmarks })
}

/// Run a single iteration of a benchmark.
///
/// POST /run_iteration
/// Body: { "benchmark_id": "..." }
/// Returns: { "duration_ns": ..., "success": true/false, "error": "..." }
async fn run_iteration(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RunIterationRequest>,
) -> impl IntoResponse {
    match state.registry.run(&request.benchmark_id) {
        Some(duration) => (
            StatusCode::OK,
            Json(RunIterationResponse {
                duration_ns: duration.as_nanos() as u64,
                success: true,
                error: None,
            }),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(RunIterationResponse {
                duration_ns: 0,
                success: false,
                error: Some(format!("Benchmark '{}' not found", request.benchmark_id)),
            }),
        ),
    }
}

/// Trigger graceful shutdown of the server.
///
/// POST /shutdown
/// Returns: { "message": "Shutting down" }
async fn shutdown(State(state): State<Arc<AppState>>) -> Json<ShutdownResponse> {
    // Signal shutdown to the server
    let _ = state.shutdown_tx.send(true);
    Json(ShutdownResponse {
        message: "Shutting down".to_string(),
    })
}

/// Build the router with all endpoints.
fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/benchmarks", get(list_benchmarks))
        .route("/run", post(run_iteration))
        .route("/shutdown", post(shutdown))
        .with_state(state)
}

/// Run the harness HTTP server.
///
/// This function starts an HTTP server on the specified port and blocks
/// until shutdown is requested via the `/shutdown` endpoint.
///
/// # Arguments
///
/// * `registry` - The benchmark registry containing all benchmarks to expose
/// * `port` - The port to listen on (binds to 0.0.0.0)
///
/// # Errors
///
/// Returns an error if the server fails to bind or encounters a runtime error.
///
/// # Example
///
/// ```ignore
/// use criterion_hypothesis_harness::{BenchmarkRegistry, run_harness};
///
/// let mut registry = BenchmarkRegistry::new();
/// registry.register("my_bench", || {
///     let start = std::time::Instant::now();
///     // ... benchmark code ...
///     start.elapsed()
/// });
///
/// // This will block until /shutdown is called
/// run_harness(registry, 8080).unwrap();
/// ```
pub fn run_harness(registry: BenchmarkRegistry, port: u16) -> anyhow::Result<()> {
    // Create a tokio runtime for the async server
    let runtime = tokio::runtime::Runtime::new()?;

    runtime.block_on(async { run_harness_async(registry, port).await })
}

/// Async implementation of the harness server.
async fn run_harness_async(registry: BenchmarkRegistry, port: u16) -> anyhow::Result<()> {
    // Create shutdown channel
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    // Create shared state
    let state = Arc::new(AppState {
        registry: Arc::new(registry),
        shutdown_tx,
    });

    // Build the router
    let app = build_router(state);

    // Create the listener
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    eprintln!("Benchmark harness listening on {}", addr);

    // Run the server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            // Wait for shutdown signal
            while !*shutdown_rx.borrow() {
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
            eprintln!("Shutting down benchmark harness");
        })
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use std::time::Duration;
    use tower::ServiceExt;

    fn create_test_state() -> Arc<AppState> {
        let mut registry = BenchmarkRegistry::new();
        registry.register("test_bench", || Duration::from_millis(42));

        let (shutdown_tx, _) = watch::channel(false);

        Arc::new(AppState {
            registry: Arc::new(registry),
            shutdown_tx,
        })
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = create_test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let health: HealthResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(health.status, "healthy");
    }

    #[tokio::test]
    async fn test_list_benchmarks_endpoint() {
        let state = create_test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/benchmarks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let benchmarks: BenchmarksResponse = serde_json::from_slice(&body).unwrap();
        assert!(benchmarks.benchmarks.contains(&"test_bench".to_string()));
    }

    #[tokio::test]
    async fn test_run_iteration_success() {
        let state = create_test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/run")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"benchmark_id": "test_bench"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: RunIterationResponse = serde_json::from_slice(&body).unwrap();
        assert!(result.success);
        assert_eq!(result.duration_ns, 42_000_000); // 42ms in nanoseconds
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn test_run_iteration_not_found() {
        let state = create_test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/run")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"benchmark_id": "nonexistent"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: RunIterationResponse = serde_json::from_slice(&body).unwrap();
        assert!(!result.success);
        assert_eq!(result.duration_ns, 0);
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("nonexistent"));
    }

    #[tokio::test]
    async fn test_shutdown_endpoint() {
        let state = create_test_state();
        let app = build_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/shutdown")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: ShutdownResponse = serde_json::from_slice(&body).unwrap();
        assert_eq!(result.message, "Shutting down");
    }
}
