//! Custom harness runtime for hypobench
//!
//! This replaces criterion's default harness with an HTTP-controlled one.
//! The harness exposes benchmark functions via HTTP endpoints, allowing
//! external orchestration of benchmark execution.

mod server;

pub use server::{run_harness, run_harness_async};

use std::collections::HashMap;
use std::time::Duration;

/// A benchmark function that runs `n` inner iterations and returns total elapsed.
///
/// The closure is expected to perform its work `n` times inside a tight loop
/// and return the total elapsed duration. The orchestrator divides by `n` to
/// obtain the per-iteration mean, which is the statistical unit the t-test
/// operates on.
///
/// Using a per-iteration loop amortises clock-read overhead (`Instant::now` is
/// ~20–50 ns) and gives meaningful variance estimates for fast functions.
pub type BenchmarkFn = Box<dyn Fn(u64) -> Duration + Send + Sync>;

/// Registry of discovered benchmarks.
///
/// This stores all benchmark functions that have been registered with the harness.
/// Each benchmark is identified by a unique string name.
pub struct BenchmarkRegistry {
    benchmarks: HashMap<String, BenchmarkFn>,
}

impl BenchmarkRegistry {
    /// Create a new empty benchmark registry.
    pub fn new() -> Self {
        Self {
            benchmarks: HashMap::new(),
        }
    }

    /// Register a benchmark function with the given name.
    ///
    /// The closure receives an iteration count `n` and should execute the work
    /// `n` times before returning total elapsed.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut registry = BenchmarkRegistry::new();
    /// registry.register("my_benchmark", |n| {
    ///     let start = std::time::Instant::now();
    ///     for _ in 0..n {
    ///         std::hint::black_box(do_work());
    ///     }
    ///     start.elapsed()
    /// });
    /// ```
    pub fn register<F>(&mut self, name: impl Into<String>, f: F)
    where
        F: Fn(u64) -> Duration + Send + Sync + 'static,
    {
        self.benchmarks.insert(name.into(), Box::new(f));
    }

    /// List all registered benchmark names.
    pub fn list(&self) -> Vec<String> {
        self.benchmarks.keys().cloned().collect()
    }

    /// Run a benchmark by name for `iterations` inner iterations.
    ///
    /// Returns `None` if no benchmark with the given name exists.
    pub fn run(&self, name: &str, iterations: u64) -> Option<Duration> {
        self.benchmarks.get(name).map(|f| f(iterations))
    }

    /// Check if a benchmark with the given name exists.
    pub fn contains(&self, name: &str) -> bool {
        self.benchmarks.contains_key(name)
    }

    /// Get the number of registered benchmarks.
    pub fn len(&self) -> usize {
        self.benchmarks.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.benchmarks.is_empty()
    }
}

impl Default for BenchmarkRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_new() {
        let registry = BenchmarkRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_registry_register_and_list() {
        let mut registry = BenchmarkRegistry::new();
        registry.register("bench1", |_n| Duration::from_millis(10));
        registry.register("bench2", |_n| Duration::from_millis(20));

        assert_eq!(registry.len(), 2);
        assert!(registry.contains("bench1"));
        assert!(registry.contains("bench2"));
        assert!(!registry.contains("bench3"));

        let names = registry.list();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"bench1".to_string()));
        assert!(names.contains(&"bench2".to_string()));
    }

    #[test]
    fn test_registry_run_passes_iterations() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::sync::Arc;

        let observed = Arc::new(AtomicU64::new(0));
        let observed_clone = Arc::clone(&observed);

        let mut registry = BenchmarkRegistry::new();
        registry.register("iter_echo", move |n| {
            observed_clone.store(n, Ordering::SeqCst);
            Duration::from_nanos(n * 100)
        });

        let result = registry.run("iter_echo", 42);
        assert_eq!(result, Some(Duration::from_nanos(4200)));
        assert_eq!(observed.load(Ordering::SeqCst), 42);
    }

    #[test]
    fn test_registry_run_missing() {
        let mut registry = BenchmarkRegistry::new();
        registry.register("exists", |_n| Duration::from_millis(5));

        assert!(registry.run("missing", 1).is_none());
    }

    #[test]
    fn test_registry_default() {
        let registry = BenchmarkRegistry::default();
        assert!(registry.is_empty());
    }
}
