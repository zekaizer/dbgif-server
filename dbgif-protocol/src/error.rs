use thiserror::Error;
use std::io;

/// Comprehensive error types for the DBGIF protocol
#[derive(Error, Debug)]
pub enum ProtocolError {
    // Message parsing and validation errors
    #[error("Invalid message format: {0}")]
    InvalidMessage(String),

    #[error("Message too short: expected at least {expected} bytes, got {actual}")]
    MessageTooShort { expected: usize, actual: usize },

    #[error("Data length mismatch: header claims {claimed} bytes, but {actual} bytes available")]
    DataLengthMismatch { claimed: u32, actual: usize },

    #[error("Invalid magic number: expected {expected:#x}, got {actual:#x}")]
    InvalidMagic { expected: u32, actual: u32 },

    #[error("CRC validation failed: expected {expected:#x}, calculated {calculated:#x}")]
    CrcValidationFailed { expected: u32, calculated: u32 },

    // Command-related errors
    #[error("Unknown command: {command:#x}")]
    UnknownCommand { command: u32 },

    #[error("Invalid command in current context: {command} not allowed when connected={connected}")]
    InvalidCommandContext { command: String, connected: bool },

    #[error("Command expects data but none provided: {command}")]
    MissingRequiredData { command: String },

    #[error("Command should not have data but data provided: {command}")]
    UnexpectedData { command: String },

    // Connection and transport errors
    #[error("Connection error: {message}")]
    ConnectionError { message: String },

    #[error("Transport error: {0}")]
    TransportError(String),

    #[error("IO error: {0}")]
    IoError(#[from] io::Error),

    #[error("Connection timeout: operation took longer than {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    #[error("Connection closed unexpectedly")]
    ConnectionClosed,

    #[error("Maximum data size exceeded: {size} bytes > {max_size} bytes")]
    DataSizeExceeded { size: usize, max_size: usize },

    // Stream management errors
    #[error("Stream not found: local_id={local_id}, remote_id={remote_id}")]
    StreamNotFound { local_id: u32, remote_id: u32 },

    #[error("Stream already exists: local_id={local_id}")]
    StreamAlreadyExists { local_id: u32 },

    #[error("Invalid stream state: expected {expected}, got {actual}")]
    InvalidStreamState { expected: String, actual: String },

    #[error("Stream buffer overflow: size={size}, capacity={capacity}")]
    StreamBufferOverflow { size: usize, capacity: usize },

    // Service and device errors
    #[error("Service not found: {service_name}")]
    ServiceNotFound { service_name: String },

    #[error("Device not found: {device_id}")]
    DeviceNotFound { device_id: String },

    #[error("Device connection failed: {device_id} - {reason}")]
    DeviceConnectionFailed { device_id: String, reason: String },

    #[error("Device disconnected: {device_id}")]
    DeviceDisconnected { device_id: String },

    #[error("No device selected")]
    NoDeviceSelected,

    // Host service errors
    #[error("Host service error: {service} - {message}")]
    HostServiceError { service: String, message: String },

    #[error("Permission denied for service: {service}")]
    PermissionDenied { service: String },

    // Protocol version and capability errors
    #[error("Unsupported protocol version: {version:#x}")]
    UnsupportedVersion { version: u32 },

    #[error("Feature not supported: {feature}")]
    FeatureNotSupported { feature: String },

    #[error("Capability mismatch: required {required}, available {available}")]
    CapabilityMismatch { required: String, available: String },

    // Server limits and resources
    #[error("Maximum connections exceeded: {count}/{max}")]
    MaxConnectionsExceeded { count: usize, max: usize },

    #[error("Maximum streams per connection exceeded: {count}/{max}")]
    MaxStreamsExceeded { count: usize, max: usize },

    #[error("Resource exhausted: {resource}")]
    ResourceExhausted { resource: String },

    #[error("Rate limit exceeded: {operation}")]
    RateLimitExceeded { operation: String },

    // Configuration and initialization errors
    #[error("Configuration error: {message}")]
    ConfigurationError { message: String },

    #[error("Initialization failed: {component} - {reason}")]
    InitializationFailed { component: String, reason: String },

    // Generic fallback
    #[error("Internal error: {message}")]
    InternalError { message: String },
}

impl ProtocolError {
    /// Check if error is recoverable (connection can continue)
    pub fn is_recoverable(&self) -> bool {
        match self {
            // Recoverable errors - can continue with connection
            ProtocolError::StreamNotFound { .. } => true,
            ProtocolError::StreamAlreadyExists { .. } => true,
            ProtocolError::ServiceNotFound { .. } => true,
            ProtocolError::DeviceNotFound { .. } => true,
            ProtocolError::NoDeviceSelected => true,
            ProtocolError::PermissionDenied { .. } => true,
            ProtocolError::FeatureNotSupported { .. } => true,
            ProtocolError::RateLimitExceeded { .. } => true,
            ProtocolError::MissingRequiredData { .. } => true,
            ProtocolError::UnexpectedData { .. } => true,

            // Non-recoverable errors - should close connection
            ProtocolError::InvalidMessage(_) => false,
            ProtocolError::MessageTooShort { .. } => false,
            ProtocolError::DataLengthMismatch { .. } => false,
            ProtocolError::InvalidMagic { .. } => false,
            ProtocolError::CrcValidationFailed { .. } => false,
            ProtocolError::UnknownCommand { .. } => false,
            ProtocolError::InvalidCommandContext { .. } => false,
            ProtocolError::ConnectionError { .. } => false,
            ProtocolError::TransportError(_) => false,
            ProtocolError::IoError(_) => false,
            ProtocolError::Timeout { .. } => false,
            ProtocolError::ConnectionClosed => false,
            ProtocolError::DataSizeExceeded { .. } => false,
            ProtocolError::InvalidStreamState { .. } => false,
            ProtocolError::StreamBufferOverflow { .. } => false,
            ProtocolError::DeviceConnectionFailed { .. } => false,
            ProtocolError::DeviceDisconnected { .. } => false,
            ProtocolError::HostServiceError { .. } => false,
            ProtocolError::UnsupportedVersion { .. } => false,
            ProtocolError::CapabilityMismatch { .. } => false,
            ProtocolError::MaxConnectionsExceeded { .. } => false,
            ProtocolError::MaxStreamsExceeded { .. } => false,
            ProtocolError::ResourceExhausted { .. } => false,
            ProtocolError::ConfigurationError { .. } => false,
            ProtocolError::InitializationFailed { .. } => false,
            ProtocolError::InternalError { .. } => false,
        }
    }

    /// Check if error should be reported to client
    pub fn should_report_to_client(&self) -> bool {
        match self {
            // Report to client
            ProtocolError::ServiceNotFound { .. } => true,
            ProtocolError::DeviceNotFound { .. } => true,
            ProtocolError::NoDeviceSelected => true,
            ProtocolError::PermissionDenied { .. } => true,
            ProtocolError::FeatureNotSupported { .. } => true,
            ProtocolError::UnsupportedVersion { .. } => true,
            ProtocolError::MaxStreamsExceeded { .. } => true,
            ProtocolError::RateLimitExceeded { .. } => true,

            // Don't report internal details
            ProtocolError::InternalError { .. } => false,
            ProtocolError::ConfigurationError { .. } => false,
            ProtocolError::InitializationFailed { .. } => false,
            ProtocolError::ResourceExhausted { .. } => false,

            // Protocol violations - close connection
            _ => false,
        }
    }

    /// Get error category for logging and metrics
    pub fn category(&self) -> &'static str {
        match self {
            ProtocolError::InvalidMessage(_) |
            ProtocolError::MessageTooShort { .. } |
            ProtocolError::DataLengthMismatch { .. } |
            ProtocolError::InvalidMagic { .. } |
            ProtocolError::CrcValidationFailed { .. } => "protocol_violation",

            ProtocolError::UnknownCommand { .. } |
            ProtocolError::InvalidCommandContext { .. } |
            ProtocolError::MissingRequiredData { .. } |
            ProtocolError::UnexpectedData { .. } => "command_error",

            ProtocolError::ConnectionError { .. } |
            ProtocolError::TransportError(_) |
            ProtocolError::IoError(_) |
            ProtocolError::Timeout { .. } |
            ProtocolError::ConnectionClosed => "transport_error",

            ProtocolError::StreamNotFound { .. } |
            ProtocolError::StreamAlreadyExists { .. } |
            ProtocolError::InvalidStreamState { .. } |
            ProtocolError::StreamBufferOverflow { .. } => "stream_error",

            ProtocolError::ServiceNotFound { .. } |
            ProtocolError::DeviceNotFound { .. } |
            ProtocolError::DeviceConnectionFailed { .. } |
            ProtocolError::DeviceDisconnected { .. } |
            ProtocolError::NoDeviceSelected => "device_error",

            ProtocolError::HostServiceError { .. } |
            ProtocolError::PermissionDenied { .. } => "service_error",

            ProtocolError::UnsupportedVersion { .. } |
            ProtocolError::FeatureNotSupported { .. } |
            ProtocolError::CapabilityMismatch { .. } => "capability_error",

            ProtocolError::MaxConnectionsExceeded { .. } |
            ProtocolError::MaxStreamsExceeded { .. } |
            ProtocolError::ResourceExhausted { .. } |
            ProtocolError::RateLimitExceeded { .. } |
            ProtocolError::DataSizeExceeded { .. } => "resource_error",

            ProtocolError::ConfigurationError { .. } |
            ProtocolError::InitializationFailed { .. } => "configuration_error",

            ProtocolError::InternalError { .. } => "internal_error",
        }
    }
}

/// Result type alias for protocol operations
pub type ProtocolResult<T> = Result<T, ProtocolError>;

/// Convert message error to protocol error
impl From<crate::message::MessageError> for ProtocolError {
    fn from(err: crate::message::MessageError) -> Self {
        match err {
            crate::message::MessageError::InvalidFormat => {
                ProtocolError::InvalidMessage("Invalid message format".to_string())
            }
            crate::message::MessageError::InvalidMagic => {
                ProtocolError::InvalidMagic {
                    expected: 0, // Will be filled in by caller
                    actual: 0,
                }
            }
            crate::message::MessageError::DataLengthMismatch => {
                ProtocolError::DataLengthMismatch {
                    claimed: 0, // Will be filled in by caller
                    actual: 0,
                }
            }
        }
    }
}