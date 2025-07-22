use std::io::{self, Write};

/// Simple terminal formatting utilities
pub struct Formatter;

impl Formatter {
    /// Print a header with emphasis
    pub fn print_header(text: &str) {
        println!("\n{}", Self::bold(text));
    }

    /// Print a success message
    pub fn print_success(text: &str) {
        println!("{} {}", Self::green("✓"), text);
    }

    /// Print an info message
    pub fn print_info(text: &str) {
        println!("{} {}", Self::blue("ℹ"), text);
    }

    /// Print a section header
    pub fn print_section(title: &str) {
        println!("\n{}:", Self::bold(title));
    }

    /// Print a key-value pair with proper indentation
    pub fn print_field(key: &str, value: &str) {
        println!("  {}: {}", key, value);
    }

    /// Print a status update that overwrites the current line
    pub fn print_status(text: &str) {
        print!("\r\x1b[K{}", text); // \r moves to start, \x1b[K clears to end of line
        io::stdout().flush().unwrap();
    }

    /// Clear the current line for status updates
    pub fn clear_line() {
        print!("\r\x1b[K");
        io::stdout().flush().unwrap();
    }

    /// Clear the current line and ensure we're on a new line for fresh output
    pub fn clear_line_and_reset() {
        print!("\r\x1b[K");
        println!(); // Move to new line
        io::stdout().flush().unwrap();
    }

    /// Apply bold formatting if terminal supports it
    fn bold(text: &str) -> String {
        if Self::supports_colors() {
            format!("\x1b[1m{}\x1b[0m", text)
        } else {
            text.to_string()
        }
    }

    /// Apply green color if terminal supports it
    fn green(text: &str) -> String {
        if Self::supports_colors() {
            format!("\x1b[32m{}\x1b[0m", text)
        } else {
            text.to_string()
        }
    }

    /// Apply blue color if terminal supports it
    fn blue(text: &str) -> String {
        if Self::supports_colors() {
            format!("\x1b[34m{}\x1b[0m", text)
        } else {
            text.to_string()
        }
    }

    /// Check if terminal supports colors
    fn supports_colors() -> bool {
        // Simple check for color support
        std::env::var("NO_COLOR").is_err()
            && std::env::var("TERM").map_or(false, |term| term != "dumb")
    }
}

/// Parse ISO 8601 timestamp and calculate duration
pub fn calculate_duration(start: &str, end: &str) -> Result<String, String> {
    use chrono::DateTime;

    let start_time = DateTime::parse_from_rfc3339(start).map_err(|_| "Invalid start timestamp")?;
    let end_time = DateTime::parse_from_rfc3339(end).map_err(|_| "Invalid end timestamp")?;

    let duration = end_time.signed_duration_since(start_time);
    let total_seconds = duration.num_seconds();

    if total_seconds < 60 {
        Ok(format!("{}s", total_seconds))
    } else if total_seconds < 3600 {
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        Ok(format!("{}m {}s", minutes, seconds))
    } else {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        Ok(format!("{}h {}m {}s", hours, minutes, seconds))
    }
}

/// Format a timestamp for display
pub fn format_timestamp(timestamp: &str) -> String {
    use chrono::DateTime;

    match DateTime::parse_from_rfc3339(timestamp) {
        Ok(dt) => dt.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        Err(_) => timestamp.to_string(),
    }
}
