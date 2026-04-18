//! JSON reporter: writes a `Report` as pretty-printed JSON.

use std::io::Write;

use hypobench_core::Report;

use super::ReportError;

/// A reporter that serializes a full `Report` (with metadata) as JSON.
///
/// Unlike the trait-based reporters that only see `&[BenchmarkComparison]`,
/// this reporter has its own top-level entry point because the JSON artifact
/// is the primary exchange format and must preserve all run metadata.
#[derive(Debug, Default, Clone)]
pub struct JsonReporter;

impl JsonReporter {
    pub fn new() -> Self {
        Self
    }

    /// Write the report as pretty-printed JSON followed by a trailing newline.
    pub fn write(&self, report: &Report, writer: &mut impl Write) -> Result<(), ReportError> {
        serde_json::to_writer_pretty(&mut *writer, report)
            .map_err(|e| ReportError::Io(std::io::Error::other(e)))?;
        writeln!(writer)?;
        Ok(())
    }
}
