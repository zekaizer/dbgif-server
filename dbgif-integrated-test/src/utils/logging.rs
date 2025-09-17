use std::io;
use tracing::Level;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

/// Logging configuration options
#[derive(Debug, Clone)]
pub struct LogConfig {
    pub level: Level,
    pub show_target: bool,
    pub show_thread_ids: bool,
    pub show_line_numbers: bool,
    pub show_span_events: FmtSpan,
    pub use_color: bool,
    pub component_filter: Option<ComponentFilter>,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: Level::INFO,
            show_target: true,
            show_thread_ids: false,
            show_line_numbers: false,
            show_span_events: FmtSpan::NONE,
            use_color: true,
            component_filter: None,
        }
    }
}

impl LogConfig {
    pub fn verbose() -> Self {
        Self {
            level: Level::DEBUG,
            show_target: true,
            show_thread_ids: false,
            show_line_numbers: false,
            show_span_events: FmtSpan::ENTER | FmtSpan::EXIT,
            use_color: true,
            component_filter: None,
        }
    }

    pub fn debug() -> Self {
        Self {
            level: Level::TRACE,
            show_target: true,
            show_thread_ids: true,
            show_line_numbers: true,
            show_span_events: FmtSpan::FULL,
            use_color: true,
            component_filter: None,
        }
    }

    pub fn quiet() -> Self {
        Self {
            level: Level::WARN,
            show_target: false,
            show_thread_ids: false,
            show_line_numbers: false,
            show_span_events: FmtSpan::NONE,
            use_color: false,
            component_filter: None,
        }
    }

    pub fn with_component_filter(mut self, filter: ComponentFilter) -> Self {
        self.component_filter = Some(filter);
        self
    }
}

/// Component-based filtering for targeted logging
#[derive(Debug, Clone)]
pub enum ComponentFilter {
    Client,
    Device,
    Scenario,
    Server,
    All,
    Custom(Vec<String>),
}

impl ComponentFilter {
    pub fn to_env_filter(&self) -> String {
        match self {
            ComponentFilter::Client => "dbgif_integrated_test::client=trace".to_string(),
            ComponentFilter::Device => "dbgif_integrated_test::device=trace".to_string(),
            ComponentFilter::Scenario => "dbgif_integrated_test::scenarios=trace".to_string(),
            ComponentFilter::Server => "dbgif_protocol=trace".to_string(),
            ComponentFilter::All => "dbgif_integrated_test=trace,dbgif_protocol=trace".to_string(),
            ComponentFilter::Custom(targets) => {
                targets.iter()
                    .map(|t| format!("{}=trace", t))
                    .collect::<Vec<_>>()
                    .join(",")
            }
        }
    }
}

/// Initialize logging with the provided configuration
pub fn init_logging(config: LogConfig) {
    let env_filter = if let Some(component) = config.component_filter {
        EnvFilter::new(format!("{},{}", config.level, component.to_env_filter()))
    } else {
        EnvFilter::new(config.level.to_string())
    };

    let fmt_layer = fmt::layer()
        .with_target(config.show_target)
        .with_thread_ids(config.show_thread_ids)
        .with_line_number(config.show_line_numbers)
        .with_span_events(config.show_span_events)
        .with_ansi(config.use_color)
        .with_writer(io::stdout);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}

/// Color-coded logging by component
pub struct ColorCodedLogger;

impl ColorCodedLogger {
    pub fn format_client(message: &str) -> String {
        format!("\x1b[36m[CLIENT]\x1b[0m {}", message) // Cyan
    }

    pub fn format_device(message: &str) -> String {
        format!("\x1b[35m[DEVICE]\x1b[0m {}", message) // Magenta
    }

    pub fn format_server(message: &str) -> String {
        format!("\x1b[34m[SERVER]\x1b[0m {}", message) // Blue
    }

    pub fn format_scenario(message: &str) -> String {
        format!("\x1b[33m[SCENARIO]\x1b[0m {}", message) // Yellow
    }

    pub fn format_success(message: &str) -> String {
        format!("\x1b[32m✅ {}\x1b[0m", message) // Green
    }

    pub fn format_error(message: &str) -> String {
        format!("\x1b[31m❌ {}\x1b[0m", message) // Red
    }

    pub fn format_warning(message: &str) -> String {
        format!("\x1b[33m⚠️  {}\x1b[0m", message) // Yellow
    }

    pub fn format_info(message: &str) -> String {
        format!("\x1b[36mℹ️  {}\x1b[0m", message) // Cyan
    }
}

/// Timing information helper
pub struct TimingLogger {
    start: std::time::Instant,
    name: String,
}

impl TimingLogger {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            start: std::time::Instant::now(),
            name: name.into(),
        }
    }

    pub fn elapsed(&self) -> std::time::Duration {
        self.start.elapsed()
    }

    pub fn log_elapsed(&self) {
        tracing::info!("{} took {:?}", self.name, self.elapsed());
    }
}

impl Drop for TimingLogger {
    fn drop(&mut self) {
        self.log_elapsed();
    }
}

/// Debug dump capability
pub struct DebugDumper;

impl DebugDumper {
    pub fn dump_hex(data: &[u8], label: &str) {
        tracing::debug!("=== {} ({}  bytes) ===", label, data.len());

        for (i, chunk) in data.chunks(16).enumerate() {
            let hex_str: String = chunk
                .iter()
                .map(|b| format!("{:02X} ", b))
                .collect();

            let ascii_str: String = chunk
                .iter()
                .map(|&b| {
                    if b >= 0x20 && b <= 0x7E {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();

            tracing::debug!("{:04X}: {:48} {}", i * 16, hex_str, ascii_str);
        }
    }

    pub fn dump_message(command: &str, args: &[(&str, String)]) {
        tracing::debug!("=== Message: {} ===", command);
        for (key, value) in args {
            tracing::debug!("  {}: {}", key, value);
        }
    }

    pub fn dump_state<T: std::fmt::Debug>(state: &T, label: &str) {
        tracing::debug!("=== {} ===", label);
        tracing::debug!("{:#?}", state);
    }
}

/// Progress reporter for long-running operations
pub struct ProgressReporter {
    total: usize,
    current: usize,
    name: String,
    last_report: std::time::Instant,
    report_interval: std::time::Duration,
}

impl ProgressReporter {
    pub fn new(name: impl Into<String>, total: usize) -> Self {
        Self {
            total,
            current: 0,
            name: name.into(),
            last_report: std::time::Instant::now(),
            report_interval: std::time::Duration::from_secs(1),
        }
    }

    pub fn with_interval(mut self, interval: std::time::Duration) -> Self {
        self.report_interval = interval;
        self
    }

    pub fn increment(&mut self) {
        self.current += 1;
        self.maybe_report();
    }

    pub fn set_current(&mut self, current: usize) {
        self.current = current;
        self.maybe_report();
    }

    fn maybe_report(&mut self) {
        if self.last_report.elapsed() >= self.report_interval {
            self.report();
            self.last_report = std::time::Instant::now();
        }
    }

    pub fn report(&self) {
        let percentage = (self.current as f64 / self.total as f64 * 100.0) as u32;
        tracing::info!(
            "{}: {}/{} ({}%)",
            self.name,
            self.current,
            self.total,
            percentage
        );
    }

    pub fn finish(&self) {
        tracing::info!("{}: Completed {}/{}", self.name, self.current, self.total);
    }
}

/// Macros for component-specific logging
#[macro_export]
macro_rules! log_client {
    ($($arg:tt)*) => {
        tracing::info!(target: "dbgif_integrated_test::client", $($arg)*);
    };
}

#[macro_export]
macro_rules! log_device {
    ($($arg:tt)*) => {
        tracing::info!(target: "dbgif_integrated_test::device", $($arg)*);
    };
}

#[macro_export]
macro_rules! log_scenario {
    ($($arg:tt)*) => {
        tracing::info!(target: "dbgif_integrated_test::scenarios", $($arg)*);
    };
}

#[macro_export]
macro_rules! log_timing {
    ($name:expr, $body:expr) => {{
        let _timer = $crate::utils::logging::TimingLogger::new($name);
        $body
    }};
}