use std::{io::Write, time::Duration};

use console::{Term, style};
use indicatif::{ProgressBar, ProgressStyle};

/// Terminal formatting utilities using the console crate
pub struct Formatter;

impl Formatter {
    /// Print a header with emphasis
    pub fn print_header(text: &str) {
        println!("\n{}", style(text).bold());
    }

    /// Print a success message
    pub fn print_success(text: &str) {
        println!("{} {}", style("✓").green().bold(), text);
    }

    /// Print an info message
    pub fn print_info(text: &str) {
        println!("{} {}", style("ℹ").blue().bold(), text);
    }

    /// Print a warning message
    pub fn print_warning(text: &str) {
        println!("{} {}", style("⚠").yellow().bold(), text);
    }

    /// Print an error message
    pub fn print_error(text: &str) {
        println!("{} {}", style("✗").red().bold(), text);
    }

    /// Print a section header
    pub fn print_section(title: &str) {
        println!("\n{}:", style(title).bold());
    }

    /// Print a key-value pair with proper indentation
    pub fn print_field(key: &str, value: &str) {
        println!("  {}: {}", style(key).dim(), value);
    }

    /// Print a status update that overwrites the current line
    pub fn print_status(text: &str) {
        let term = Term::stdout();
        term.clear_line().ok();
        print!("\r{}", style(text).dim());
        std::io::stdout().flush().unwrap();
    }

    /// Clear the current line for status updates
    pub fn clear_line() {
        let term = Term::stdout();
        term.clear_line().ok();
        print!("\r");
        std::io::stdout().flush().unwrap();
    }

    /// Clear the current line and ensure we're on a new line for fresh output
    pub fn clear_line_and_reset() {
        let term = Term::stdout();
        term.clear_line().ok();
        println!();
        std::io::stdout().flush().unwrap();
    }

    /// Create a progress bar for file uploads/downloads
    pub fn create_download_progress(total_bytes: u64) -> ProgressBar {
        let pb = ProgressBar::new(total_bytes);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
                .expect("Invalid progress template")
                .progress_chars("█▉▊▋▌▍▎▏  "),
        );
        pb.set_message("Downloading");
        pb
    }

    /// Create a progress bar for file uploads (keeping the old method for compatibility)
    pub fn create_upload_progress(total_bytes: u64) -> ProgressBar {
        let pb = ProgressBar::new(total_bytes);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
                .expect("Invalid progress template")
                .progress_chars("█▉▊▋▌▍▎▏  "),
        );
        pb.set_message("Uploading");
        pb
    }

    /// Create a spinner for polling operations (build/prove/run/verify)
    pub fn create_spinner(message: &str) -> ProgressBar {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"])
                .template("{spinner:.cyan} {msg} [{elapsed}]")
                .expect("Invalid spinner template"),
        );
        pb.set_message(message.to_string());
        pb.enable_steady_tick(Duration::from_millis(80));
        pb
    }
}
