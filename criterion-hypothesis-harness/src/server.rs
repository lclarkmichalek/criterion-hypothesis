//! HTTP server for the benchmark harness.
//!
//! This module provides an HTTP server that exposes benchmark functions
//! for external orchestration. The server supports health checks, listing
//! benchmarks, running individual iterations, and graceful shutdown.
//!
//! The harness supports exclusive claiming via nonce to prevent multiple
//! orchestrators from accidentally using the same harness simultaneously.

use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use criterion_hypothesis_core::protocol::{
    BenchmarkListResponse, ClaimRequest, ClaimResponse, HealthResponse, ReleaseRequest,
    ReleaseResponse, RunIterationRequest, RunIterationResponse, ShutdownResponse, CLAIM_HEADER,
};
use tokio::sync::{watch, Mutex};

use crate::BenchmarkRegistry;

/// Shared state for the HTTP server.
struct AppState {
    /// The benchmark registry containing all registered benchmarks.
    registry: Arc<BenchmarkRegistry>,
    /// Sender to signal shutdown.
    shutdown_tx: watch::Sender<bool>,
    /// Current claim nonce (None if unclaimed).
    claim: Mutex<Option<String>>,
}

/// Health check endpoint.
///
/// GET /health
/// Returns: { "status": "healthy" }
async fn health() -> Json<HealthResponse> {
    Json(HealthResponse::healthy())
}

/// List all available benchmarks.
///
/// GET /benchmarks
/// Returns: { "benchmarks": ["bench1", "bench2", ...] }
async fn list_benchmarks(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Check claim if harness is claimed
    if let Err(response) = check_claim(&state, &headers).await {
        return response;
    }

    let benchmarks = state.registry.list();
    eprintln!("[harness] Listed {} benchmark(s)", benchmarks.len());
    (StatusCode::OK, Json(BenchmarkListResponse::new(benchmarks))).into_response()
}

/// Run a single iteration of a benchmark.
///
/// POST /run
/// Body: { "benchmark_id": "..." }
/// Returns: { "duration_ns": ..., "success": true/false, "error": "..." }
async fn run_iteration(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<RunIterationRequest>,
) -> impl IntoResponse {
    // Check claim if harness is claimed
    if let Err(response) = check_claim(&state, &headers).await {
        return response;
    }

    match state.registry.run(&request.benchmark_id) {
        Some(duration) => {
            eprintln!(
                "[harness] Ran '{}': {:.3}ms",
                request.benchmark_id,
                duration.as_secs_f64() * 1000.0
            );
            (StatusCode::OK, Json(RunIterationResponse::success(duration))).into_response()
        }
        None => {
            eprintln!("[harness] Benchmark '{}' not found", request.benchmark_id);
            (
                StatusCode::NOT_FOUND,
                Json(RunIterationResponse::failure(format!(
                    "Benchmark '{}' not found",
                    request.benchmark_id
                ))),
            )
                .into_response()
        }
    }
}

/// Trigger graceful shutdown of the server.
///
/// POST /shutdown
/// Returns: { "status": "shutting_down" }
async fn shutdown(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    // Check claim if harness is claimed
    if let Err(response) = check_claim(&state, &headers).await {
        return response;
    }

    // Signal shutdown to the server
    let _ = state.shutdown_tx.send(true);
    (StatusCode::OK, Json(ShutdownResponse::acknowledged())).into_response()
}

/// Claim exclusive access to the harness.
///
/// POST /claim
/// Body: { "nonce": "unique-session-id" }
/// Returns: { "success": true/false, "error": "..." }
async fn claim(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ClaimRequest>,
) -> impl IntoResponse {
    let mut claim = state.claim.lock().await;

    match &*claim {
        Some(existing) if existing != &request.nonce => {
            // Already claimed by someone else
            eprintln!("[harness] Claim rejected - already claimed by another orchestrator");
            (StatusCode::CONFLICT, Json(ClaimResponse::already_claimed()))
        }
        Some(_) => {
            // Already claimed by us (idempotent)
            eprintln!("[harness] Claim refreshed (same nonce)");
            (StatusCode::OK, Json(ClaimResponse::success()))
        }
        None => {
            // Claim it
            eprintln!("[harness] Claimed by orchestrator (nonce: {}...)", &request.nonce[..8.min(request.nonce.len())]);
            *claim = Some(request.nonce);
            (StatusCode::OK, Json(ClaimResponse::success()))
        }
    }
}

/// Release a claim on the harness.
///
/// POST /release
/// Body: { "nonce": "unique-session-id" }
/// Returns: { "success": true/false }
async fn release(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ReleaseRequest>,
) -> impl IntoResponse {
    let mut claim = state.claim.lock().await;

    match &*claim {
        Some(existing) if existing == &request.nonce => {
            // Release the claim
            eprintln!("[harness] Released by orchestrator");
            *claim = None;
            (StatusCode::OK, Json(ReleaseResponse::success()))
        }
        _ => {
            // Not claimed by this nonce
            eprintln!("[harness] Release rejected - wrong nonce or not claimed");
            (
                StatusCode::BAD_REQUEST,
                Json(ReleaseResponse { success: false }),
            )
        }
    }
}

/// Check if the request has a valid claim header (if harness is claimed).
async fn check_claim(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), axum::response::Response> {
    let claim = state.claim.lock().await;

    if let Some(expected_nonce) = &*claim {
        // Harness is claimed, check the header
        match headers.get(CLAIM_HEADER) {
            Some(value) => {
                let provided = value.to_str().unwrap_or("");
                if provided != expected_nonce {
                    return Err((
                        StatusCode::FORBIDDEN,
                        Json(serde_json::json!({
                            "error": "Invalid claim nonce"
                        })),
                    )
                        .into_response());
                }
            }
            None => {
                return Err((
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error": "Harness is claimed, X-Harness-Claim header required"
                    })),
                )
                    .into_response());
            }
        }
    }

    Ok(())
}

/// Build the router with all endpoints.
fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/benchmarks", get(list_benchmarks))
        .route("/run", post(run_iteration))
        .route("/shutdown", post(shutdown))
        .route("/claim", post(claim))
        .route("/release", post(release))
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
///
/// Use this when you're already in a tokio runtime (e.g., in async tests).
/// For standalone use, prefer `run_harness` which creates its own runtime.
pub async fn run_harness_async(registry: BenchmarkRegistry, port: u16) -> anyhow::Result<()> {
    // Create shutdown channel
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    // Create shared state
    let state = Arc::new(AppState {
        registry: Arc::new(registry),
        shutdown_tx,
        claim: Mutex::new(None),
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
            claim: Mutex::new(None),
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
        let benchmarks: BenchmarkListResponse = serde_json::from_slice(&body).unwrap();
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
        assert_eq!(result.status, "shutting_down");
    }
}
