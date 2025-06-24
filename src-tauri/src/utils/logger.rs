// src/utils/logger.rs
use crate::utils::path::get_log_dir;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use time::macros::format_description;
use tracing_appender::rolling;
use tracing_subscriber::{
    fmt::{self, time::LocalTime}, // Import LocalTime for custom time formatting
    layer::SubscriberExt,
    registry,
    util::SubscriberInitExt,
    EnvFilter,
};

const DEFAULT_FILE_PREFIX: &str = "app.log";
const DEFAULT_LEVEL: &str = "info";

#[derive(Debug)]
pub struct LoggerBuilder {
    log_dir: PathBuf,
    file_prefix: String,
    default_level: String,
}

impl LoggerBuilder {
    pub fn new() -> Self {
        LoggerBuilder {
            log_dir: get_log_dir().into(),
            file_prefix: DEFAULT_FILE_PREFIX.into(),
            default_level: DEFAULT_LEVEL.into(),
        }
    }

    pub fn log_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.log_dir = dir.into();
        self
    }

    pub fn file_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.file_prefix = prefix.into();
        self
    }

    pub fn default_level(mut self, level: impl Into<String>) -> Self {
        self.default_level = level.into();
        self
    }

    pub fn init(self) -> Result<(), Box<dyn Error>> {
        fs::create_dir_all(&self.log_dir)?;
        let file_appender = rolling::daily(&self.log_dir, &self.file_prefix);

        // 1. Define custom time format with millisecond precision
        // This includes the date. If you only want the time part, adjust the format string.
        let time_format = LocalTime::new(format_description!(
            "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]"
        ));
        // Example for only time: format_description!("[hour]:[minute]:[second].[subsecond digits:3]")

        // 2. Configure layers with thread IDs and custom timer
        let file_layer = fmt::layer()
            .with_writer(file_appender)
            .with_ansi(false)
            .with_thread_names(true)
            .with_thread_ids(true) // Log thread IDs
            .with_timer(time_format.clone()); // Apply custom timer

        let stdout_layer = fmt::layer()
            .with_writer(std::io::stdout)
            .with_ansi(false)
            .with_thread_names(true)
            .with_thread_ids(true) // Log thread IDs
            .with_timer(time_format); // Apply custom timer

        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(&self.default_level));

        registry()
            .with(filter)
            .with(file_layer)
            .with(stdout_layer)
            .try_init()?;
        Ok(())
    }
}
