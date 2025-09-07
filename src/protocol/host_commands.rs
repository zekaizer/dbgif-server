use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum HostCommand {
    /// List all devices: "host:devices"
    Devices,
    /// List devices with details: "host:devices-l"  
    DevicesLong,
    /// Track device connections: "host:track-devices"
    TrackDevices,
    /// Select specific device: "host:transport:<serial>"
    Transport(String),
    /// Select any available device: "host:transport-any"
    TransportAny,
}

impl HostCommand {
    /// Parse host command from service string
    pub fn parse(service: &str) -> Result<Self> {
        if !service.starts_with("host:") {
            bail!("Not a host command: {}", service);
        }

        let command_part = &service[5..]; // Remove "host:" prefix

        match command_part {
            "devices" => Ok(Self::Devices),
            "devices-l" => Ok(Self::DevicesLong),
            "track-devices" => Ok(Self::TrackDevices),
            "transport-any" => Ok(Self::TransportAny),
            _ => {
                if let Some(serial) = command_part.strip_prefix("transport:") {
                    if serial.is_empty() {
                        bail!("Empty device serial in transport command");
                    }
                    Ok(Self::Transport(serial.to_string()))
                } else {
                    bail!("Unknown host command: {}", command_part);
                }
            }
        }
    }

    /// Convert back to service string
    pub fn to_service_string(&self) -> String {
        match self {
            Self::Devices => "host:devices".to_string(),
            Self::DevicesLong => "host:devices-l".to_string(),
            Self::TrackDevices => "host:track-devices".to_string(),
            Self::TransportAny => "host:transport-any".to_string(),
            Self::Transport(serial) => format!("host:transport:{}", serial),
        }
    }

    /// Check if this command requires streaming response (multiple WRTE messages)
    pub fn is_streaming(&self) -> bool {
        matches!(self, Self::TrackDevices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_devices() {
        let cmd = HostCommand::parse("host:devices").unwrap();
        assert_eq!(cmd, HostCommand::Devices);
        assert_eq!(cmd.to_service_string(), "host:devices");
    }

    #[test]
    fn test_parse_devices_long() {
        let cmd = HostCommand::parse("host:devices-l").unwrap();
        assert_eq!(cmd, HostCommand::DevicesLong);
        assert_eq!(cmd.to_service_string(), "host:devices-l");
    }

    #[test]
    fn test_parse_track_devices() {
        let cmd = HostCommand::parse("host:track-devices").unwrap();
        assert_eq!(cmd, HostCommand::TrackDevices);
        assert!(cmd.is_streaming());
    }

    #[test]
    fn test_parse_transport() {
        let cmd = HostCommand::parse("host:transport:0123456789ABCDEF").unwrap();
        assert_eq!(cmd, HostCommand::Transport("0123456789ABCDEF".to_string()));
        assert_eq!(cmd.to_service_string(), "host:transport:0123456789ABCDEF");
    }

    #[test]
    fn test_parse_transport_any() {
        let cmd = HostCommand::parse("host:transport-any").unwrap();
        assert_eq!(cmd, HostCommand::TransportAny);
    }

    #[test]
    fn test_parse_not_host_command() {
        let result = HostCommand::parse("shell:ls");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_transport() {
        let result = HostCommand::parse("host:transport:");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_unknown_command() {
        let result = HostCommand::parse("host:unknown");
        assert!(result.is_err());
    }

    #[test]
    fn test_streaming_commands() {
        assert!(!HostCommand::Devices.is_streaming());
        assert!(!HostCommand::DevicesLong.is_streaming());
        assert!(HostCommand::TrackDevices.is_streaming());
        assert!(!HostCommand::TransportAny.is_streaming());
    }
}
