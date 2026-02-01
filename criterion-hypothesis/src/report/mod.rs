use crate::stats::TestResult;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReportError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct SampleStats {
    pub mean_ns: f64,
    pub std_dev_ns: f64,
    pub min_ns: u64,
    pub max_ns: u64,
    pub sample_count: usize,
}

#[derive(Debug, Clone)]
pub struct BenchmarkComparison {
    pub name: String,
    pub baseline_stats: SampleStats,
    pub candidate_stats: SampleStats,
    pub test_result: TestResult,
}

pub trait Reporter: Send + Sync {
    fn report(&self, results: &[BenchmarkComparison]) -> Result<(), ReportError>;
}

mod terminal;
pub use terminal::TerminalReporter;
