use anyhow::Result;
use bytes::Bytes;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{debug, info, warn};

use crate::protocol::host_commands::HostCommand;
use crate::transport::{TransportManager, TransportType};

pub struct HostService {
    transport_manager: Arc<TransportManager>,
}

impl HostService {
    pub fn new(transport_manager: Arc<TransportManager>) -> Self {
        Self { transport_manager }
    }

    /// Execute a host command and return response data
    pub async fn execute_command(&self, command: HostCommand) -> Result<HostServiceResponse> {
        match command {
            HostCommand::Devices => {
                let device_list = self.format_device_list(false).await;
                Ok(HostServiceResponse::SingleResponse(device_list))
            }
            HostCommand::DevicesLong => {
                let device_list = self.format_device_list(true).await;
                Ok(HostServiceResponse::SingleResponse(device_list))
            }
            HostCommand::TrackDevices => {
                // For now, return initial device list
                // TODO: Implement streaming updates
                let device_list = self.format_device_list(false).await;
                Ok(HostServiceResponse::SingleResponse(device_list))
            }
            HostCommand::Transport(device_id) => self.select_device(&device_id).await,
            HostCommand::TransportAny => self.select_any_device().await,
        }
    }

    /// Format device list for response
    async fn format_device_list(&self, long_format: bool) -> Bytes {
        let transport_info = self.transport_manager.get_all_transport_info().await;

        if transport_info.is_empty() {
            debug!("No devices available for listing");
            return Bytes::new();
        }

        let mut output = String::new();

        let device_count = transport_info.len();

        for (device_id, (transport_type, is_connected)) in transport_info {
            let status = if is_connected { "device" } else { "offline" };

            if long_format {
                // Format: serial status transport_info
                let transport_info = match transport_type {
                    TransportType::Tcp => "tcp",
                    TransportType::AndroidUsb => "usb",
                    TransportType::BridgeUsb => "bridge_usb",
                };
                output.push_str(&format!("{}\t{}\t{}\n", device_id, status, transport_info));
            } else {
                // Format: serial status
                output.push_str(&format!("{}\t{}\n", device_id, status));
            }
        }

        info!("Device list requested: {} devices", device_count);
        debug!("Device list response:\n{}", output.trim_end());

        Bytes::from(output)
    }

    /// Select specific device for transport
    async fn select_device(&self, device_id: &str) -> Result<HostServiceResponse> {
        let transport_info = self.transport_manager.get_transport_info(device_id).await;

        match transport_info {
            Some((_, is_connected)) => {
                if is_connected {
                    info!("Selected device: {}", device_id);
                    Ok(HostServiceResponse::TransportSelected(
                        device_id.to_string(),
                    ))
                } else {
                    warn!("Device {} is offline", device_id);
                    Err(anyhow::anyhow!("Device {} is offline", device_id))
                }
            }
            None => {
                warn!("Device {} not found", device_id);
                Err(anyhow::anyhow!("Device {} not found", device_id))
            }
        }
    }

    /// Select any available device
    async fn select_any_device(&self) -> Result<HostServiceResponse> {
        let transport_ids = self.transport_manager.get_transport_ids().await;

        for device_id in transport_ids {
            if let Some((_, is_connected)) =
                self.transport_manager.get_transport_info(&device_id).await
            {
                if is_connected {
                    info!("Selected any available device: {}", device_id);
                    return Ok(HostServiceResponse::TransportSelected(device_id));
                }
            }
        }

        warn!("No devices available for transport-any");
        Err(anyhow::anyhow!("No devices available"))
    }
}

#[derive(Debug)]
pub enum HostServiceResponse {
    /// Single response data to send back to client
    SingleResponse(Bytes),
    /// Device transport was selected - contains device ID
    TransportSelected(String),
    /// Streaming response - contains watch receiver for updates
    StreamingResponse(watch::Receiver<Bytes>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::TransportManager;

    #[tokio::test]
    async fn test_empty_device_list() {
        let manager = Arc::new(TransportManager::new());
        let service = HostService::new(manager);

        let response = service.execute_command(HostCommand::Devices).await.unwrap();

        match response {
            HostServiceResponse::SingleResponse(data) => {
                assert_eq!(data, Bytes::new());
            }
            _ => panic!("Expected SingleResponse"),
        }
    }

    #[tokio::test]
    async fn test_select_nonexistent_device() {
        let manager = Arc::new(TransportManager::new());
        let service = HostService::new(manager);

        let result = service
            .execute_command(HostCommand::Transport("nonexistent".to_string()))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_transport_any_no_devices() {
        let manager = Arc::new(TransportManager::new());
        let service = HostService::new(manager);

        let result = service.execute_command(HostCommand::TransportAny).await;
        assert!(result.is_err());
    }
}
