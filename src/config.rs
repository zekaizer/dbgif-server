use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

/// Main daemon configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonConfig {
    /// Server configuration
    pub server: ServerConfig,

    /// Transport configuration
    pub transport: TransportConfig,

    /// Session management configuration
    pub session: SessionConfig,

    /// Logging configuration
    pub logging: LoggingConfig,

    /// Performance tuning
    pub performance: PerformanceConfig,
}

/// Server configuration for ADB daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Address to bind the ADB server to
    pub bind_address: SocketAddr,

    /// Maximum number of concurrent client connections
    pub max_connections: usize,

    /// Connection timeout for idle clients
    pub connection_timeout: Duration,

    /// Server shutdown timeout
    pub shutdown_timeout: Duration,

    /// Enable graceful shutdown
    pub graceful_shutdown: bool,
}

/// Transport layer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Enable TCP transport
    pub enable_tcp: bool,

    /// Enable USB device transport
    pub enable_usb_device: bool,

    /// Enable USB bridge transport
    pub enable_usb_bridge: bool,

    /// USB polling interval for device discovery
    pub usb_poll_interval: Duration,

    /// Transport connection timeout
    pub transport_timeout: Duration,

    /// Maximum transfer size per operation
    pub max_transfer_size: usize,

    /// Enable transport hotplug detection
    pub enable_hotplug: bool,
}

/// Session management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Maximum concurrent sessions per client
    pub max_sessions_per_client: usize,

    /// Global maximum concurrent sessions
    pub max_total_sessions: usize,

    /// Session idle timeout
    pub session_timeout: Duration,

    /// Maximum streams per session
    pub max_streams_per_session: usize,

    /// Authentication settings
    pub auth: AuthConfig,
}

/// Authentication configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Enable authentication (simplified for this implementation)
    pub enabled: bool,

    /// Auto-accept all authentication attempts
    pub auto_accept: bool,

    /// Authentication timeout
    pub timeout: Duration,

    /// Require authentication for device access
    pub require_auth_for_devices: bool,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    pub level: String,

    /// Log format (compact, pretty, json)
    pub format: String,

    /// Enable file logging
    pub enable_file: bool,

    /// Log file path (optional)
    pub file_path: Option<PathBuf>,

    /// Maximum log file size in MB
    pub max_file_size_mb: u64,

    /// Number of log files to retain
    pub file_retention_count: usize,

    /// Enable console logging
    pub enable_console: bool,
}

/// Performance tuning configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Number of worker threads for async runtime
    pub worker_threads: Option<usize>,

    /// Thread stack size in KB
    pub thread_stack_size_kb: usize,

    /// Enable high priority for server threads
    pub high_priority: bool,

    /// TCP buffer sizes
    pub tcp_buffer_size: usize,

    /// USB transfer buffer size
    pub usb_buffer_size: usize,

    /// Maximum concurrent USB transfers
    pub max_usb_transfers: usize,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            transport: TransportConfig::default(),
            session: SessionConfig::default(),
            logging: LoggingConfig::default(),
            performance: PerformanceConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:5037".parse().unwrap(), // Standard ADB port
            max_connections: 100,
            connection_timeout: Duration::from_secs(300), // 5 minutes
            shutdown_timeout: Duration::from_secs(30),
            graceful_shutdown: true,
        }
    }
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            enable_tcp: true,
            enable_usb_device: true,
            enable_usb_bridge: true,
            usb_poll_interval: Duration::from_secs(2),
            transport_timeout: Duration::from_secs(30),
            max_transfer_size: 256 * 1024, // 256KB
            enable_hotplug: true,
        }
    }
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_sessions_per_client: 10,
            max_total_sessions: 100,
            session_timeout: Duration::from_secs(600), // 10 minutes
            max_streams_per_session: 50,
            auth: AuthConfig::default(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Simplified authentication disabled by default
            auto_accept: true,
            timeout: Duration::from_secs(30),
            require_auth_for_devices: false,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: "compact".to_string(),
            enable_file: false,
            file_path: None,
            max_file_size_mb: 100,
            file_retention_count: 5,
            enable_console: true,
        }
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            worker_threads: None, // Use default (number of CPU cores)
            thread_stack_size_kb: 2048, // 2MB
            high_priority: false,
            tcp_buffer_size: 64 * 1024, // 64KB
            usb_buffer_size: 16 * 1024, // 16KB
            max_usb_transfers: 8,
        }
    }
}

impl DaemonConfig {
    /// Load configuration from file
    pub fn load_from_file(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .context(format!("Failed to read config file: {}", path.display()))?;

        let config: DaemonConfig = toml::from_str(&content)
            .context("Failed to parse TOML configuration")?;

        config.validate()
            .context("Configuration validation failed")?;

        Ok(config)
    }

    /// Save configuration to file
    pub fn save_to_file(&self, path: &PathBuf) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .context("Failed to serialize configuration")?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create config directory")?;
        }

        std::fs::write(path, content)
            .context(format!("Failed to write config file: {}", path.display()))?;

        Ok(())
    }

    /// Create development configuration
    pub fn development() -> Self {
        Self {
            server: ServerConfig {
                bind_address: "127.0.0.1:5037".parse().unwrap(),
                max_connections: 50,
                connection_timeout: Duration::from_secs(600), // 10 minutes
                shutdown_timeout: Duration::from_secs(60),
                graceful_shutdown: true,
            },
            transport: TransportConfig {
                enable_tcp: true,
                enable_usb_device: true,
                enable_usb_bridge: true,
                usb_poll_interval: Duration::from_secs(1), // Faster polling for development
                transport_timeout: Duration::from_secs(60),
                max_transfer_size: 1024 * 1024, // 1MB for testing
                enable_hotplug: true,
            },
            session: SessionConfig {
                max_sessions_per_client: 20,
                max_total_sessions: 50,
                session_timeout: Duration::from_secs(1200), // 20 minutes
                max_streams_per_session: 100,
                auth: AuthConfig {
                    enabled: false,
                    auto_accept: true,
                    timeout: Duration::from_secs(60),
                    require_auth_for_devices: false,
                },
            },
            logging: LoggingConfig {
                level: "debug".to_string(),
                format: "pretty".to_string(),
                enable_file: true,
                file_path: Some("logs/dbgif-server.log".into()),
                max_file_size_mb: 50,
                file_retention_count: 3,
                enable_console: true,
            },
            performance: PerformanceConfig {
                worker_threads: Some(4),
                thread_stack_size_kb: 4096, // 4MB for development
                high_priority: false,
                tcp_buffer_size: 128 * 1024, // 128KB
                usb_buffer_size: 32 * 1024, // 32KB
                max_usb_transfers: 16,
            },
        }
    }

    /// Create production configuration
    pub fn production() -> Self {
        Self {
            server: ServerConfig {
                bind_address: "0.0.0.0:5037".parse().unwrap(), // Listen on all interfaces
                max_connections: 200,
                connection_timeout: Duration::from_secs(180), // 3 minutes
                shutdown_timeout: Duration::from_secs(15),
                graceful_shutdown: true,
            },
            transport: TransportConfig {
                enable_tcp: true,
                enable_usb_device: true,
                enable_usb_bridge: true,
                usb_poll_interval: Duration::from_secs(3), // Conservative polling
                transport_timeout: Duration::from_secs(15),
                max_transfer_size: 256 * 1024, // 256KB
                enable_hotplug: true,
            },
            session: SessionConfig {
                max_sessions_per_client: 5,
                max_total_sessions: 200,
                session_timeout: Duration::from_secs(300), // 5 minutes
                max_streams_per_session: 25,
                auth: AuthConfig {
                    enabled: false, // Still simplified for this implementation
                    auto_accept: true,
                    timeout: Duration::from_secs(15),
                    require_auth_for_devices: false,
                },
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                format: "compact".to_string(),
                enable_file: true,
                file_path: Some("/var/log/dbgif-server/dbgif-server.log".into()),
                max_file_size_mb: 200,
                file_retention_count: 10,
                enable_console: false, // No console logging in production
            },
            performance: PerformanceConfig {
                worker_threads: None, // Use all available cores
                thread_stack_size_kb: 1024, // 1MB
                high_priority: true,
                tcp_buffer_size: 32 * 1024, // 32KB
                usb_buffer_size: 8 * 1024, // 8KB
                max_usb_transfers: 4,
            },
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Validate server config
        if self.server.max_connections == 0 {
            return Err(anyhow::anyhow!("max_connections must be greater than 0"));
        }

        if self.server.connection_timeout.as_secs() == 0 {
            return Err(anyhow::anyhow!("connection_timeout must be greater than 0"));
        }

        // Validate transport config
        if !self.transport.enable_tcp && !self.transport.enable_usb_device && !self.transport.enable_usb_bridge {
            return Err(anyhow::anyhow!("At least one transport must be enabled"));
        }

        if self.transport.max_transfer_size == 0 {
            return Err(anyhow::anyhow!("max_transfer_size must be greater than 0"));
        }

        // Validate session config
        if self.session.max_total_sessions == 0 {
            return Err(anyhow::anyhow!("max_total_sessions must be greater than 0"));
        }

        if self.session.max_sessions_per_client > self.session.max_total_sessions {
            return Err(anyhow::anyhow!("max_sessions_per_client cannot exceed max_total_sessions"));
        }

        // Validate logging config
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.logging.level.as_str()) {
            return Err(anyhow::anyhow!("Invalid log level: {}", self.logging.level));
        }

        let valid_formats = ["compact", "pretty", "json"];
        if !valid_formats.contains(&self.logging.format.as_str()) {
            return Err(anyhow::anyhow!("Invalid log format: {}", self.logging.format));
        }

        // Validate performance config
        if let Some(threads) = self.performance.worker_threads {
            if threads == 0 {
                return Err(anyhow::anyhow!("worker_threads must be greater than 0 if specified"));
            }
        }

        if self.performance.thread_stack_size_kb < 512 {
            return Err(anyhow::anyhow!("thread_stack_size_kb must be at least 512KB"));
        }

        Ok(())
    }

    /// Get configuration file path based on environment
    pub fn default_config_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            let mut path = PathBuf::from(
                std::env::var("APPDATA").unwrap_or_else(|_| "C:\\ProgramData".to_string())
            );
            path.push("dbgif-server");
            path.push("config.toml");
            path
        }

        #[cfg(not(target_os = "windows"))]
        {
            let mut path = PathBuf::from(
                std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
                    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                    format!("{}/.config", home)
                })
            );
            path.push("dbgif-server");
            path.push("config.toml");
            path
        }
    }

    /// Override configuration with environment variables
    pub fn apply_env_overrides(&mut self) {
        // Server overrides
        if let Ok(addr) = std::env::var("DBGIF_BIND_ADDRESS") {
            if let Ok(socket_addr) = addr.parse() {
                self.server.bind_address = socket_addr;
            }
        }

        if let Ok(max_conn) = std::env::var("DBGIF_MAX_CONNECTIONS") {
            if let Ok(num) = max_conn.parse() {
                self.server.max_connections = num;
            }
        }

        // Logging overrides
        if let Ok(level) = std::env::var("DBGIF_LOG_LEVEL") {
            self.logging.level = level;
        }

        if let Ok(format) = std::env::var("DBGIF_LOG_FORMAT") {
            self.logging.format = format;
        }

        // Transport overrides
        if let Ok(enable) = std::env::var("DBGIF_ENABLE_TCP") {
            self.transport.enable_tcp = enable.parse().unwrap_or(true);
        }

        if let Ok(enable) = std::env::var("DBGIF_ENABLE_USB_DEVICE") {
            self.transport.enable_usb_device = enable.parse().unwrap_or(true);
        }

        if let Ok(enable) = std::env::var("DBGIF_ENABLE_USB_BRIDGE") {
            self.transport.enable_usb_bridge = enable.parse().unwrap_or(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = DaemonConfig::default();
        assert!(config.validate().is_ok());
        assert_eq!(config.server.bind_address.port(), 5037);
        assert!(!config.session.auth.enabled);
    }

    #[test]
    fn test_development_config() {
        let config = DaemonConfig::development();
        assert!(config.validate().is_ok());
        assert_eq!(config.logging.level, "debug");
        assert!(config.logging.enable_file);
    }

    #[test]
    fn test_production_config() {
        let config = DaemonConfig::production();
        assert!(config.validate().is_ok());
        assert_eq!(config.logging.level, "info");
        assert!(!config.logging.enable_console);
        assert!(config.performance.high_priority);
    }

    #[test]
    fn test_config_validation() {
        let mut config = DaemonConfig::default();

        // Test invalid max_connections
        config.server.max_connections = 0;
        assert!(config.validate().is_err());

        // Test no transports enabled
        config.server.max_connections = 100;
        config.transport.enable_tcp = false;
        config.transport.enable_usb_device = false;
        config.transport.enable_usb_bridge = false;
        assert!(config.validate().is_err());

        // Test invalid log level
        config.transport.enable_tcp = true;
        config.logging.level = "invalid".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_file_operations() {
        let config = DaemonConfig::development();
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // Test save
        assert!(config.save_to_file(&path).is_ok());

        // Test load
        let loaded_config = DaemonConfig::load_from_file(&path).unwrap();
        assert_eq!(loaded_config.logging.level, "debug");
        assert_eq!(loaded_config.server.max_connections, config.server.max_connections);
    }

    #[test]
    fn test_env_overrides() {
        std::env::set_var("DBGIF_LOG_LEVEL", "trace");
        std::env::set_var("DBGIF_MAX_CONNECTIONS", "42");
        std::env::set_var("DBGIF_ENABLE_TCP", "false");

        let mut config = DaemonConfig::default();
        config.apply_env_overrides();

        assert_eq!(config.logging.level, "trace");
        assert_eq!(config.server.max_connections, 42);
        assert!(!config.transport.enable_tcp);

        // Cleanup
        std::env::remove_var("DBGIF_LOG_LEVEL");
        std::env::remove_var("DBGIF_MAX_CONNECTIONS");
        std::env::remove_var("DBGIF_ENABLE_TCP");
    }
}