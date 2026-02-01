//! Custom harness runtime for criterion-hypothesis
//!
//! This replaces criterion's default harness with an HTTP-controlled one.
//! The harness exposes benchmark functions via HTTP endpoints, allowing
//! external orchestration of benchmark execution.

mod server;

pub use server::run_harness;

use std::collections::HashMap;
use std::time::Duration;

/// A benchmark function that can be run on demand.
///
/// The function should execute exactly one iteration of the benchmark
/// and return the duration it took to complete.
pub type BenchmarkFn = Box<dyn Fn() -> Duration + Send + Sync>;

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
    /// # Arguments
    ///
    /// * `name` - A unique identifier for the benchmark
    /// * `f` - The benchmark function that returns execution duration
    ///
    /// # Example
    ///
    /// ```ignore
    /// let mut registry = BenchmarkRegistry::new();
    /// registry.register("my_benchmark", || {
    ///     let start = std::time::Instant::now();
    ///     // ... do work ...
    ///     start.elapsed()
    /// });
    /// ```
    pub fn register<F>(&mut self, name: impl Into<String>, f: F)
    where
        F: Fn() -> Duration + Send + Sync + 'static,
    {
        self.benchmarks.insert(name.into(), Box::new(f));
    }

    /// List all registered benchmark names.
    pub fn list(&self) -> Vec<String> {
        self.benchmarks.keys().cloned().collect()
    }

    /// Run a benchmark by name and return its duration.
    ///
    /// Returns `None` if no benchmark with the given name exists.
    pub fn run(&self, name: &str) -> Option<Duration> {
        self.benchmarks.get(name).map(|f| f())
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
        registry.register("bench1", || Duration::from_millis(10));
        registry.register("bench2", || Duration::from_millis(20));

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
    fn test_registry_run() {
        let mut registry = BenchmarkRegistry::new();
        registry.register("fast", || Duration::from_millis(5));

        let result = registry.run("fast");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), Duration::from_millis(5));

        let missing = registry.run("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_registry_default() {
        let registry = BenchmarkRegistry::default();
        assert!(registry.is_empty());
    }
}
