use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::SocketAddr,
    path::Path,
    time::Duration,
};
use thiserror::Error;
use dbgif_transport::TransportAddress;

/// Configuration error types
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parsing error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("TOML serialization error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("Invalid address: {message}")]
    InvalidAddress { message: String },

    #[error("Invalid configuration: {field}: {message}")]
    InvalidConfig { field: String, message: String },

    #[error("Validation error: {message}")]
    Validation { message: String },
}

/// Result type for configuration operations
pub type ConfigResult<T> = Result<T, ConfigError>;

/// Complete DBGIF server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbgifConfig {
    /// Server configuration
    pub server: ServerConfig,

    /// Transport configuration
    pub transport: TransportConfig,

    /// Device discovery configuration
    pub discovery: DiscoveryConfig,

    /// Logging configuration
    pub logging: LoggingConfig,

    /// Security configuration
    pub security: SecurityConfig,
}

/// Server configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Server bind address
    pub bind_address: String,

    /// Server bind port
    pub bind_port: u16,

    /// Maximum concurrent connections
    pub max_connections: usize,

    /// Connection timeout in seconds
    pub connection_timeout_secs: u64,

    /// Ping interval in seconds
    pub ping_interval_secs: u64,

    /// Ping timeout in seconds
    pub ping_timeout_secs: u64,

    /// Enable graceful shutdown
    pub graceful_shutdown: bool,

    /// Graceful shutdown timeout in seconds
    pub shutdown_timeout_secs: u64,
}

/// Transport configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    /// Buffer size for connections
    pub buffer_size: usize,

    /// TCP no-delay setting
    pub tcp_nodelay: bool,

    /// TCP keep-alive setting
    pub tcp_keepalive: bool,

    /// Socket reuse address
    pub reuse_addr: bool,

    /// Listen backlog size
    pub listen_backlog: u32,

    /// Maximum message size
    pub max_message_size: usize,
}

/// Device discovery configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// Enable automatic device discovery
    pub enable_auto_discovery: bool,

    /// Discovery interval in seconds
    pub discovery_interval_secs: u64,

    /// TCP ports to scan for devices
    pub tcp_discovery_ports: Vec<u16>,

    /// Device connection timeout in seconds
    pub device_connection_timeout_secs: u64,

    /// Device health check interval in seconds
    pub device_health_check_interval_secs: u64,

    /// Maximum device connections
    pub max_device_connections: usize,
}

/// Logging configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (error, warn, info, debug, trace)
    pub level: String,

    /// Enable JSON formatted logs
    pub json_format: bool,

    /// Enable colors in console output
    pub colored_output: bool,

    /// Include thread IDs in logs
    pub include_thread_ids: bool,

    /// Include line numbers in logs
    pub include_line_numbers: bool,

    /// Log file path (optional)
    pub log_file: Option<String>,

    /// Log file rotation size in MB
    pub log_file_max_size_mb: Option<u64>,

    /// Number of log files to keep
    pub log_file_max_count: Option<u32>,
}

/// Security configuration section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Enable authentication
    pub enable_auth: bool,

    /// Authentication timeout in seconds
    pub auth_timeout_secs: u64,

    /// Maximum authentication attempts
    pub max_auth_attempts: u32,

    /// Rate limiting: requests per second
    pub rate_limit_rps: Option<u32>,

    /// Allowed client addresses (CIDR format)
    pub allowed_clients: Vec<String>,

    /// Denied client addresses (CIDR format)
    pub denied_clients: Vec<String>,
}

impl Default for DbgifConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            transport: TransportConfig::default(),
            discovery: DiscoveryConfig::default(),
            logging: LoggingConfig::default(),
            security: SecurityConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".to_string(),
            bind_port: 5555,
            max_connections: 100,
            connection_timeout_secs: 30,
            ping_interval_secs: 60,
            ping_timeout_secs: 10,
            graceful_shutdown: true,
            shutdown_timeout_secs: 30,
        }
    }
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            buffer_size: 64 * 1024, // 64KB
            tcp_nodelay: true,
            tcp_keepalive: true,
            reuse_addr: true,
            listen_backlog: 128,
            max_message_size: 1024 * 1024, // 1MB
        }
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            enable_auto_discovery: true,
            discovery_interval_secs: 30,
            tcp_discovery_ports: vec![5557, 5558, 5559],
            device_connection_timeout_secs: 5,
            device_health_check_interval_secs: 60,
            max_device_connections: 10,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            json_format: false,
            colored_output: true,
            include_thread_ids: false,
            include_line_numbers: false,
            log_file: None,
            log_file_max_size_mb: Some(100),
            log_file_max_count: Some(10),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enable_auth: false,
            auth_timeout_secs: 30,
            max_auth_attempts: 3,
            rate_limit_rps: None,
            allowed_clients: vec![],
            denied_clients: vec![],
        }
    }
}

impl DbgifConfig {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> ConfigResult<Self> {
        let content = fs::read_to_string(&path)?;
        let config: DbgifConfig = toml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Save configuration to a TOML file
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> ConfigResult<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Load configuration from a TOML string
    pub fn from_str(content: &str) -> ConfigResult<Self> {
        let config: DbgifConfig = toml::from_str(content)?;
        config.validate()?;
        Ok(config)
    }

    /// Convert configuration to TOML string
    pub fn to_string(&self) -> ConfigResult<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Validate the configuration
    pub fn validate(&self) -> ConfigResult<()> {
        // Validate server configuration
        self.server.validate()?;

        // Validate transport configuration
        self.transport.validate()?;

        // Validate discovery configuration
        self.discovery.validate()?;

        // Validate logging configuration
        self.logging.validate()?;

        // Validate security configuration
        self.security.validate()?;

        Ok(())
    }

    /// Get server socket address
    pub fn server_socket_addr(&self) -> ConfigResult<SocketAddr> {
        let addr_str = format!("{}:{}", self.server.bind_address, self.server.bind_port);
        addr_str.parse().map_err(|e| ConfigError::InvalidAddress {
            message: format!("Invalid server address '{}': {}", addr_str, e),
        })
    }

    /// Get transport address
    pub fn transport_address(&self) -> ConfigResult<TransportAddress> {
        let socket_addr = self.server_socket_addr()?;
        Ok(TransportAddress::Tcp(socket_addr))
    }

    /// Get connection timeout as Duration
    pub fn connection_timeout(&self) -> Duration {
        Duration::from_secs(self.server.connection_timeout_secs)
    }

    /// Get ping interval as Duration
    pub fn ping_interval(&self) -> Duration {
        Duration::from_secs(self.server.ping_interval_secs)
    }

    /// Get ping timeout as Duration
    pub fn ping_timeout(&self) -> Duration {
        Duration::from_secs(self.server.ping_timeout_secs)
    }

    /// Get discovery interval as Duration
    pub fn discovery_interval(&self) -> Duration {
        Duration::from_secs(self.discovery.discovery_interval_secs)
    }

    /// Get device connection timeout as Duration
    pub fn device_connection_timeout(&self) -> Duration {
        Duration::from_secs(self.discovery.device_connection_timeout_secs)
    }

    /// Get device health check interval as Duration
    pub fn device_health_check_interval(&self) -> Duration {
        Duration::from_secs(self.discovery.device_health_check_interval_secs)
    }

    /// Get graceful shutdown timeout as Duration
    pub fn shutdown_timeout(&self) -> Duration {
        Duration::from_secs(self.server.shutdown_timeout_secs)
    }

    /// Apply CLI arguments to override configuration
    pub fn apply_cli_overrides(&mut self, cli_args: &CliOverrides) {
        if let Some(bind_address) = &cli_args.bind_address {
            self.server.bind_address = bind_address.clone();
        }

        if let Some(bind_port) = cli_args.bind_port {
            self.server.bind_port = bind_port;
        }

        if let Some(max_connections) = cli_args.max_connections {
            self.server.max_connections = max_connections;
        }

        if let Some(connection_timeout) = cli_args.connection_timeout_secs {
            self.server.connection_timeout_secs = connection_timeout;
        }

        if let Some(ping_interval) = cli_args.ping_interval_secs {
            self.server.ping_interval_secs = ping_interval;
        }

        if let Some(discovery_ports) = &cli_args.discovery_ports {
            self.discovery.tcp_discovery_ports = discovery_ports.clone();
        }

        if let Some(log_level) = &cli_args.log_level {
            self.logging.level = log_level.clone();
        }

        if let Some(enable_discovery) = cli_args.enable_auto_discovery {
            self.discovery.enable_auto_discovery = enable_discovery;
        }
    }
}

/// CLI argument overrides for configuration
#[derive(Debug, Default)]
pub struct CliOverrides {
    pub bind_address: Option<String>,
    pub bind_port: Option<u16>,
    pub max_connections: Option<usize>,
    pub connection_timeout_secs: Option<u64>,
    pub ping_interval_secs: Option<u64>,
    pub discovery_ports: Option<Vec<u16>>,
    pub log_level: Option<String>,
    pub enable_auto_discovery: Option<bool>,
}

impl ServerConfig {
    fn validate(&self) -> ConfigResult<()> {
        // Validate bind address
        if self.bind_address.is_empty() {
            return Err(ConfigError::InvalidConfig {
                field: "server.bind_address".to_string(),
                message: "Bind address cannot be empty".to_string(),
            });
        }

        // Validate port
        if self.bind_port == 0 {
            return Err(ConfigError::InvalidConfig {
                field: "server.bind_port".to_string(),
                message: "Port cannot be 0".to_string(),
            });
        }

        // Validate max connections
        if self.max_connections == 0 {
            return Err(ConfigError::InvalidConfig {
                field: "server.max_connections".to_string(),
                message: "Maximum connections must be greater than 0".to_string(),
            });
        }

        // Validate timeouts
        if self.connection_timeout_secs == 0 {
            return Err(ConfigError::InvalidConfig {
                field: "server.connection_timeout_secs".to_string(),
                message: "Connection timeout must be greater than 0".to_string(),
            });
        }

        if self.ping_timeout_secs >= self.ping_interval_secs {
            return Err(ConfigError::InvalidConfig {
                field: "server.ping_timeout_secs".to_string(),
                message: "Ping timeout must be less than ping interval".to_string(),
            });
        }

        Ok(())
    }
}

impl TransportConfig {
    fn validate(&self) -> ConfigResult<()> {
        if self.buffer_size == 0 {
            return Err(ConfigError::InvalidConfig {
                field: "transport.buffer_size".to_string(),
                message: "Buffer size must be greater than 0".to_string(),
            });
        }

        if self.max_message_size == 0 {
            return Err(ConfigError::InvalidConfig {
                field: "transport.max_message_size".to_string(),
                message: "Maximum message size must be greater than 0".to_string(),
            });
        }

        if self.buffer_size > self.max_message_size {
            return Err(ConfigError::InvalidConfig {
                field: "transport.buffer_size".to_string(),
                message: "Buffer size cannot be larger than maximum message size".to_string(),
            });
        }

        Ok(())
    }
}

impl DiscoveryConfig {
    fn validate(&self) -> ConfigResult<()> {
        if self.tcp_discovery_ports.is_empty() && self.enable_auto_discovery {
            return Err(ConfigError::InvalidConfig {
                field: "discovery.tcp_discovery_ports".to_string(),
                message: "Discovery ports cannot be empty when auto-discovery is enabled".to_string(),
            });
        }

        for &port in &self.tcp_discovery_ports {
            if port == 0 {
                return Err(ConfigError::InvalidConfig {
                    field: "discovery.tcp_discovery_ports".to_string(),
                    message: "Discovery ports cannot contain 0".to_string(),
                });
            }
        }

        if self.max_device_connections == 0 {
            return Err(ConfigError::InvalidConfig {
                field: "discovery.max_device_connections".to_string(),
                message: "Maximum device connections must be greater than 0".to_string(),
            });
        }

        Ok(())
    }
}

impl LoggingConfig {
    fn validate(&self) -> ConfigResult<()> {
        // Validate log level
        match self.level.to_lowercase().as_str() {
            "error" | "warn" | "info" | "debug" | "trace" => {}
            _ => {
                return Err(ConfigError::InvalidConfig {
                    field: "logging.level".to_string(),
                    message: format!("Invalid log level '{}', must be one of: error, warn, info, debug, trace", self.level),
                });
            }
        }

        // Validate log file configuration
        if let Some(max_size) = self.log_file_max_size_mb {
            if max_size == 0 {
                return Err(ConfigError::InvalidConfig {
                    field: "logging.log_file_max_size_mb".to_string(),
                    message: "Log file max size must be greater than 0".to_string(),
                });
            }
        }

        if let Some(max_count) = self.log_file_max_count {
            if max_count == 0 {
                return Err(ConfigError::InvalidConfig {
                    field: "logging.log_file_max_count".to_string(),
                    message: "Log file max count must be greater than 0".to_string(),
                });
            }
        }

        Ok(())
    }
}

impl SecurityConfig {
    fn validate(&self) -> ConfigResult<()> {
        if self.max_auth_attempts == 0 {
            return Err(ConfigError::InvalidConfig {
                field: "security.max_auth_attempts".to_string(),
                message: "Maximum authentication attempts must be greater than 0".to_string(),
            });
        }

        if let Some(rps) = self.rate_limit_rps {
            if rps == 0 {
                return Err(ConfigError::InvalidConfig {
                    field: "security.rate_limit_rps".to_string(),
                    message: "Rate limit must be greater than 0".to_string(),
                });
            }
        }

        // Validate CIDR notation for allowed and denied clients
        for addr in &self.allowed_clients {
            if !addr.is_empty() && !Self::validate_cidr_notation(addr) {
                return Err(ConfigError::Validation {
                    message: format!("Invalid CIDR notation in allowed_clients: {}", addr),
                });
            }
        }

        for addr in &self.denied_clients {
            if !addr.is_empty() && !Self::validate_cidr_notation(addr) {
                return Err(ConfigError::Validation {
                    message: format!("Invalid CIDR notation in denied_clients: {}", addr),
                });
            }
        }

        Ok(())
    }

    /// Validate CIDR notation
    fn validate_cidr_notation(addr: &str) -> bool {
        // Basic CIDR validation - accepts IP addresses with optional /prefix
        if addr.contains('/') {
            let parts: Vec<&str> = addr.split('/').collect();
            if parts.len() != 2 {
                return false;
            }

            // Validate IP part
            if parts[0].parse::<std::net::IpAddr>().is_err() {
                return false;
            }

            // Validate prefix part
            if let Ok(prefix) = parts[1].parse::<u8>() {
                // IPv4: 0-32, IPv6: 0-128
                if parts[0].contains(':') {
                    prefix <= 128 // IPv6
                } else {
                    prefix <= 32 // IPv4
                }
            } else {
                false
            }
        } else {
            // Just an IP address without prefix
            addr.parse::<std::net::IpAddr>().is_ok()
        }
    }
}

/// Generate a default configuration file template
pub fn generate_default_config_file<P: AsRef<Path>>(path: P) -> ConfigResult<()> {
    let config = DbgifConfig::default();
    config.to_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_creation() {
        let config = DbgifConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_serialization() {
        let config = DbgifConfig::default();
        let toml_str = config.to_string().unwrap();
        assert!(!toml_str.is_empty());

        // Verify round-trip
        let parsed_config = DbgifConfig::from_str(&toml_str).unwrap();
        assert!(parsed_config.validate().is_ok());
    }

    #[test]
    fn test_server_validation() {
        let mut config = ServerConfig::default();
        assert!(config.validate().is_ok());

        // Test invalid port
        config.bind_port = 0;
        assert!(config.validate().is_err());

        config.bind_port = 5555;

        // Test invalid max connections
        config.max_connections = 0;
        assert!(config.validate().is_err());

        config.max_connections = 100;

        // Test invalid timeout relationship
        config.ping_timeout_secs = config.ping_interval_secs;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_transport_validation() {
        let mut config = TransportConfig::default();
        assert!(config.validate().is_ok());

        // Test invalid buffer size
        config.buffer_size = 0;
        assert!(config.validate().is_err());

        config.buffer_size = 1024;

        // Test buffer size larger than max message size
        config.max_message_size = 512;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_discovery_validation() {
        let mut config = DiscoveryConfig::default();
        assert!(config.validate().is_ok());

        // Test empty discovery ports with auto-discovery enabled
        config.tcp_discovery_ports.clear();
        assert!(config.validate().is_err());

        // Test with auto-discovery disabled
        config.enable_auto_discovery = false;
        assert!(config.validate().is_ok());

        // Test invalid port
        config.tcp_discovery_ports = vec![0];
        config.enable_auto_discovery = true;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_logging_validation() {
        let mut config = LoggingConfig::default();
        assert!(config.validate().is_ok());

        // Test invalid log level
        config.level = "invalid".to_string();
        assert!(config.validate().is_err());

        config.level = "debug".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_cli_overrides() {
        let mut config = DbgifConfig::default();
        let cli_overrides = CliOverrides {
            bind_port: Some(8080),
            max_connections: Some(200),
            log_level: Some("debug".to_string()),
            ..Default::default()
        };

        config.apply_cli_overrides(&cli_overrides);

        assert_eq!(config.server.bind_port, 8080);
        assert_eq!(config.server.max_connections, 200);
        assert_eq!(config.logging.level, "debug");
    }

    #[test]
    fn test_duration_helpers() {
        let config = DbgifConfig::default();

        assert_eq!(config.connection_timeout(), Duration::from_secs(30));
        assert_eq!(config.ping_interval(), Duration::from_secs(60));
        assert_eq!(config.ping_timeout(), Duration::from_secs(10));
    }

    #[test]
    fn test_socket_addr_conversion() {
        let config = DbgifConfig::default();
        let addr = config.server_socket_addr().unwrap();
        assert_eq!(addr.port(), 5555);
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
    }
}