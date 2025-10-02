use axiom_sdk::ProgressCallback;
use serde::Serialize;

/// Output mode for CLI commands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Human-readable output with colors and progress bars
    Human,
    /// Machine-readable JSON output
    Json,
}

/// A no-op progress callback that suppresses all output.
/// Used in JSON mode to prevent progress messages from interfering with JSON output.
pub struct JsonProgressCallback;

impl ProgressCallback for JsonProgressCallback {
    fn on_header(&self, _text: &str) {}
    fn on_success(&self, _text: &str) {}
    fn on_info(&self, _text: &str) {}
    fn on_warning(&self, _text: &str) {}
    fn on_error(&self, _text: &str) {}
    fn on_section(&self, _title: &str) {}
    fn on_field(&self, _key: &str, _value: &str) {}
    fn on_status(&self, _text: &str) {}
    fn on_progress_start(&self, _message: &str, _total: Option<u64>) {}
    fn on_progress_update(&self, _current: u64) {}
    fn on_progress_update_message(&self, _message: &str) {}
    fn on_progress_finish(&self, _message: &str) {}
    fn on_clear_line(&self) {}
    fn on_clear_line_and_reset(&self) {}
}

/// Helper function to output data in JSON format
pub fn print_json<T: Serialize>(data: &T) -> eyre::Result<()> {
    let json = serde_json::to_string_pretty(data)?;
    println!("{}", json);
    Ok(())
}
