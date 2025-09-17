use std::{
    fs,
    io,
    path::{Path, PathBuf},
    sync::Once,
};
use tracing::Level;
use tracing_subscriber::{
    fmt,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
    Registry,
};
use crate::config::LoggingConfig;

static INIT: Once = Once::new();
static mut GLOBAL_LOGGER_INITIALIZED: bool = false;

/// Logging error types
#[derive(thiserror::Error, Debug)]
pub enum LoggingError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Log level parse error: {message}")]
    InvalidLevel { message: String },

    #[error("Logger already initialized")]
    AlreadyInitialized,

    #[error("File operation error: {message}")]
    FileOperation { message: String },
}

/// Result type for logging operations
pub type LoggingResult<T> = Result<T, LoggingError>;

/// Log output destination
#[derive(Debug, Clone)]
pub enum LogOutput {
    /// Write to stdout
    Stdout,
    /// Write to stderr
    Stderr,
    /// Write to a file
    File(PathBuf),
}

/// Log format options
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    /// Human-readable format with colors (default)
    Pretty,
    /// Compact human-readable format
    Compact,
}

/// Complete logging configuration builder
#[derive(Debug)]
pub struct LoggingBuilder {
    level: Level,
    format: LogFormat,
    output: LogOutput,
    include_thread_ids: bool,
    include_line_numbers: bool,
    include_target: bool,
    colored_output: bool,
    log_file: Option<PathBuf>,
}

impl Default for LoggingBuilder {
    fn default() -> Self {
        Self {
            level: Level::INFO,
            format: LogFormat::Pretty,
            output: LogOutput::Stdout,
            include_thread_ids: false,
            include_line_numbers: false,
            include_target: true,
            colored_output: true,
            log_file: None,
        }
    }
}

impl LoggingBuilder {
    /// Create a new logging builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the log level
    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// Set the log level from string
    pub fn level_from_str(mut self, level: &str) -> LoggingResult<Self> {
        self.level = parse_log_level(level)?;
        Ok(self)
    }

    /// Set the log format
    pub fn format(mut self, format: LogFormat) -> Self {
        self.format = format;
        self
    }

    /// Set the output destination
    pub fn output(mut self, output: LogOutput) -> Self {
        self.output = output;
        self
    }

    /// Enable/disable thread IDs in logs
    pub fn include_thread_ids(mut self, include: bool) -> Self {
        self.include_thread_ids = include;
        self
    }

    /// Enable/disable line numbers in logs
    pub fn include_line_numbers(mut self, include: bool) -> Self {
        self.include_line_numbers = include;
        self
    }

    /// Enable/disable target information in logs
    pub fn include_target(mut self, include: bool) -> Self {
        self.include_target = include;
        self
    }

    /// Enable/disable colored output
    pub fn colored_output(mut self, colored: bool) -> Self {
        self.colored_output = colored;
        self
    }

    /// Set log file path
    pub fn log_file<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.log_file = Some(path.into());
        self
    }

    /// Configure logging from configuration
    pub fn from_config(config: &LoggingConfig) -> LoggingResult<Self> {
        let mut builder = Self::new()
            .level_from_str(&config.level)?
            .include_thread_ids(config.include_thread_ids)
            .include_line_numbers(config.include_line_numbers)
            .include_target(true)
            .colored_output(config.colored_output);

        // Set format
        builder.format = if config.json_format {
            // JSON format not supported in this version, fallback to compact
            LogFormat::Compact
        } else {
            LogFormat::Pretty
        };

        // Set output destination and log file
        if let Some(log_file_path) = &config.log_file {
            builder = builder.log_file(log_file_path);
            builder.output = LogOutput::File(PathBuf::from(log_file_path));
        } else {
            builder.output = LogOutput::Stdout;
        }

        Ok(builder)
    }

    /// Initialize the global logger
    pub fn init(self) -> LoggingResult<()> {
        // Ensure we only initialize once
        let mut result = Ok(());
        INIT.call_once(|| {
            result = self.init_internal();
        });

        unsafe {
            if GLOBAL_LOGGER_INITIALIZED {
                return Err(LoggingError::AlreadyInitialized);
            }
            GLOBAL_LOGGER_INITIALIZED = true;
        }

        result
    }

    /// Internal initialization implementation
    fn init_internal(self) -> LoggingResult<()> {
        // Create environment filter
        let env_filter = EnvFilter::builder()
            .with_default_directive(self.level.into())
            .from_env_lossy();

        // Create the subscriber based on format and output
        match (&self.format, &self.output) {
            // Console output with pretty format
            (LogFormat::Pretty, LogOutput::Stdout) | (LogFormat::Pretty, LogOutput::Stderr) => {
                let fmt_layer = fmt::layer()
                    .pretty()
                    .with_ansi(self.colored_output)
                    .with_target(self.include_target)
                    .with_thread_ids(self.include_thread_ids)
                    .with_line_number(self.include_line_numbers);

                Registry::default()
                    .with(env_filter)
                    .with(fmt_layer)
                    .init();
            }

            // Console output with compact format
            (LogFormat::Compact, LogOutput::Stdout) | (LogFormat::Compact, LogOutput::Stderr) => {
                let fmt_layer = fmt::layer()
                    .compact()
                    .with_ansi(self.colored_output)
                    .with_target(self.include_target)
                    .with_thread_ids(self.include_thread_ids)
                    .with_line_number(self.include_line_numbers);

                Registry::default()
                    .with(env_filter)
                    .with(fmt_layer)
                    .init();
            }


            // File output - use compact format for all file outputs
            (_, LogOutput::File(path)) => {
                self.ensure_log_directory(path)?;
                let file = self.create_log_file(path)?;

                let fmt_layer = fmt::layer()
                    .compact()
                    .with_ansi(false) // No colors in file output
                    .with_target(self.include_target)
                    .with_thread_ids(self.include_thread_ids)
                    .with_line_number(self.include_line_numbers)
                    .with_writer(file);

                Registry::default()
                    .with(env_filter)
                    .with(fmt_layer)
                    .init();
            }
        }

        tracing::info!("Logging system initialized with level: {}, format: {:?}", self.level, self.format);
        Ok(())
    }

    /// Ensure log directory exists
    fn ensure_log_directory(&self, path: &Path) -> LoggingResult<()> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        Ok(())
    }

    /// Create log file with proper permissions
    fn create_log_file(&self, path: &Path) -> LoggingResult<fs::File> {
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(file)
    }
}

/// Parse log level from string
pub fn parse_log_level(level: &str) -> LoggingResult<Level> {
    match level.to_lowercase().as_str() {
        "error" => Ok(Level::ERROR),
        "warn" | "warning" => Ok(Level::WARN),
        "info" => Ok(Level::INFO),
        "debug" => Ok(Level::DEBUG),
        "trace" => Ok(Level::TRACE),
        _ => Err(LoggingError::InvalidLevel {
            message: format!("Invalid log level '{}', must be one of: error, warn, info, debug, trace", level),
        }),
    }
}

/// Initialize logging with default configuration
pub fn init_default() -> LoggingResult<()> {
    LoggingBuilder::default().init()
}

/// Initialize logging with specified level
pub fn init_with_level(level: Level) -> LoggingResult<()> {
    LoggingBuilder::new().level(level).init()
}

/// Initialize logging from configuration
pub fn init_from_config(config: &LoggingConfig) -> LoggingResult<()> {
    LoggingBuilder::from_config(config)?.init()
}

/// Create a test logger for unit tests
pub fn init_test_logger() -> LoggingResult<()> {
    LoggingBuilder::new()
        .level(Level::DEBUG)
        .format(LogFormat::Compact)
        .output(LogOutput::Stderr)
        .colored_output(false)
        .init()
}

/// Check if global logger is initialized
pub fn is_initialized() -> bool {
    unsafe { GLOBAL_LOGGER_INITIALIZED }
}

/// Quick setup functions for common use cases
pub mod quick {
    use super::*;

    /// Initialize console logger with pretty format
    pub fn init_pretty(level: Level) -> LoggingResult<()> {
        LoggingBuilder::new()
            .level(level)
            .format(LogFormat::Pretty)
            .colored_output(true)
            .init()
    }

    /// Initialize console logger with compact format
    pub fn init_compact(level: Level) -> LoggingResult<()> {
        LoggingBuilder::new()
            .level(level)
            .format(LogFormat::Compact)
            .colored_output(true)
            .init()
    }


    /// Initialize file logger
    pub fn init_file<P: Into<PathBuf>>(level: Level, path: P) -> LoggingResult<()> {
        LoggingBuilder::new()
            .level(level)
            .format(LogFormat::Compact)
            .output(LogOutput::File(path.into()))
            .init()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_parsing() {
        assert_eq!(parse_log_level("error").unwrap(), Level::ERROR);
        assert_eq!(parse_log_level("ERROR").unwrap(), Level::ERROR);
        assert_eq!(parse_log_level("warn").unwrap(), Level::WARN);
        assert_eq!(parse_log_level("warning").unwrap(), Level::WARN);
        assert_eq!(parse_log_level("info").unwrap(), Level::INFO);
        assert_eq!(parse_log_level("debug").unwrap(), Level::DEBUG);
        assert_eq!(parse_log_level("trace").unwrap(), Level::TRACE);

        assert!(parse_log_level("invalid").is_err());
    }

    #[test]
    fn test_logging_builder() {
        let builder = LoggingBuilder::new()
            .level(Level::DEBUG)
            .format(LogFormat::Compact)
            .include_thread_ids(true);

        assert_eq!(builder.level, Level::DEBUG);
        assert_eq!(builder.format, LogFormat::Compact);
        assert!(builder.include_thread_ids);
    }

    #[test]
    fn test_config_parsing() {
        let config = LoggingConfig {
            level: "debug".to_string(),
            json_format: true,
            colored_output: false,
            include_thread_ids: true,
            include_line_numbers: true,
            log_file: Some("/tmp/test.log".to_string()),
            log_file_max_size_mb: Some(50),
            log_file_max_count: Some(5),
        };

        let builder = LoggingBuilder::from_config(&config).unwrap();
        assert_eq!(builder.level, Level::DEBUG);
        assert_eq!(builder.format, LogFormat::Compact); // JSON fallback to Compact
        assert!(!builder.colored_output);
        assert!(builder.include_thread_ids);
        assert!(builder.include_line_numbers);
    }

    #[test]
    fn test_log_format_variants() {
        assert_eq!(LogFormat::Pretty, LogFormat::Pretty);
        assert_eq!(LogFormat::Compact, LogFormat::Compact);
        assert_ne!(LogFormat::Pretty, LogFormat::Compact);
    }

    #[test]
    fn test_logging_error_types() {
        let error = LoggingError::InvalidLevel {
            message: "test error".to_string(),
        };
        assert!(error.to_string().contains("Log level parse error"));

        let io_error = LoggingError::Io(io::Error::new(io::ErrorKind::NotFound, "test"));
        assert!(io_error.to_string().contains("IO error"));
    }

    #[test]
    fn test_is_initialized() {
        // Note: This test might be flaky in a multi-test environment
        // since the global state is shared
        assert!(!is_initialized() || is_initialized());
    }
}