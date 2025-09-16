use std::sync::Mutex;

use axiom_sdk::ProgressCallback;
use indicatif::ProgressBar;

use crate::formatting::Formatter;

pub struct CliProgressCallback {
    progress_bar: Mutex<Option<ProgressBar>>,
}

impl CliProgressCallback {
    pub fn new() -> Self {
        Self {
            progress_bar: Mutex::new(None),
        }
    }
}

impl ProgressCallback for CliProgressCallback {
    fn on_header(&self, text: &str) {
        Formatter::print_header(text);
    }

    fn on_success(&self, text: &str) {
        Formatter::print_success(text);
    }

    fn on_info(&self, text: &str) {
        Formatter::print_info(text);
    }

    fn on_warning(&self, text: &str) {
        Formatter::print_warning(text);
    }

    fn on_error(&self, text: &str) {
        Formatter::print_error(text);
    }

    fn on_section(&self, title: &str) {
        Formatter::print_section(title);
    }

    fn on_field(&self, key: &str, value: &str) {
        Formatter::print_field(key, value);
    }

    fn on_status(&self, text: &str) {
        Formatter::print_status(text);
    }

    fn on_progress_start(&self, message: &str, total: Option<u64>) {
        let pb = if let Some(total_bytes) = total {
            if message.contains("Downloading") || message.contains("download") {
                Formatter::create_download_progress(total_bytes)
            } else {
                Formatter::create_upload_progress(total_bytes)
            }
        } else {
            Formatter::create_spinner(message)
        };
        *self.progress_bar.lock().unwrap() = Some(pb);
    }

    fn on_progress_update(&self, current: u64) {
        if let Some(pb) = self.progress_bar.lock().unwrap().as_ref() {
            pb.set_position(current);
        }
    }

    fn on_progress_update_message(&self, message: &str) {
        if let Some(pb) = self.progress_bar.lock().unwrap().as_ref() {
            pb.set_message(message.to_string());
        }
    }

    fn on_progress_finish(&self, message: &str) {
        if let Some(pb) = self.progress_bar.lock().unwrap().take() {
            pb.finish_with_message(message.to_string());
        }
    }

    fn on_clear_line(&self) {
        Formatter::clear_line();
    }

    fn on_clear_line_and_reset(&self) {
        Formatter::clear_line_and_reset();
    }
}
