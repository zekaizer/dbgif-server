use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use rusb::{Device, DeviceHandle};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::{ConnectionStatus, Transport, TransportType};
use crate::protocol::constants::MAXDATA;
use crate::protocol::message::Message;

// USB configuration and interface
const USB_CONFIGURATION: u8 = 1;
const USB_INTERFACE: u8 = 0;

// Timeout for USB operations
const USB_TIMEOUT: Duration = Duration::from_secs(5);

/// Bridge USB Transport
pub struct BridgeUsbTransport {
    device_id: String,
    handle: Arc<Mutex<DeviceHandle<rusb::GlobalContext>>>,
    in_endpoint: u8,
    out_endpoint: u8,
    interrupt_endpoint: u8,
    is_connected: bool,
}

impl BridgeUsbTransport {
    /// Create new Bridge USB transport for a device
    pub fn new(device: Device<rusb::GlobalContext>) -> Result<Self> {
        let device_descriptor = device
            .device_descriptor()
            .context("Failed to get device descriptor")?;

        // Get device serial number or use bus:address as fallback
        let device_id = match device.open() {
            Ok(handle) => match handle.read_serial_number_string_ascii(&device_descriptor) {
                Ok(serial) => format!("bridge_{}", serial),
                Err(_) => format!("bridge_{}:{}", device.bus_number(), device.address()),
            },
            Err(_) => format!("bridge_{}:{}", device.bus_number(), device.address()),
        };

        let handle = device.open().context("Failed to open USB device")?;

        // Set configuration
        handle
            .set_active_configuration(USB_CONFIGURATION)
            .context("Failed to set USB configuration")?;

        // Claim interface
        handle
            .claim_interface(USB_INTERFACE)
            .context("Failed to claim USB interface")?;

        // Find endpoints
        let config_descriptor = device
            .config_descriptor(0)
            .context("Failed to get configuration descriptor")?;

        let interface_descriptor = config_descriptor
            .interfaces()
            .next()
            .context("No interface found")?
            .descriptors()
            .next()
            .context("No interface descriptor found")?;

        let mut in_endpoint = None;
        let mut out_endpoint = None;
        let mut interrupt_endpoint = None;

        for endpoint in interface_descriptor.endpoint_descriptors() {
            match endpoint.transfer_type() {
                rusb::TransferType::Bulk => match endpoint.direction() {
                    rusb::Direction::In => in_endpoint = Some(endpoint.address()),
                    rusb::Direction::Out => out_endpoint = Some(endpoint.address()),
                },
                rusb::TransferType::Interrupt => {
                    if endpoint.direction() == rusb::Direction::In {
                        interrupt_endpoint = Some(endpoint.address());
                    }
                }
                _ => {}
            }
        }

        let in_endpoint = in_endpoint.context("No bulk IN endpoint found")?;
        let out_endpoint = out_endpoint.context("No bulk OUT endpoint found")?;
        let interrupt_endpoint = interrupt_endpoint
            .context("No interrupt IN endpoint found for bridge connection monitoring")?;

        info!(
            "Bridge USB device {} initialized with interrupt endpoint 0x{:02x}",
            device_id, interrupt_endpoint
        );

        Ok(BridgeUsbTransport {
            device_id,
            handle: Arc::new(Mutex::new(handle)),
            in_endpoint,
            out_endpoint,
            interrupt_endpoint,
            is_connected: true,
        })
    }

    /// Send message to USB device (header first, then data)
    async fn send_message_internal(&mut self, message: &Message) -> Result<()> {
        let serialized = message.serialize();

        // Split into header (24 bytes) and data
        let header = &serialized[..24];
        let data = &serialized[24..];

        let mut handle = self.handle.lock().await;

        // Send header first
        match self.send_bulk(&mut *handle, header) {
            Ok(_) => {}
            Err(e) => {
                self.is_connected = false;
                return Err(e);
            }
        }

        // Send data if present
        if !data.is_empty() {
            match self.send_bulk(&mut *handle, data) {
                Ok(_) => {}
                Err(e) => {
                    self.is_connected = false;
                    return Err(e);
                }
            }
        }

        debug!(
            "Sent message to bridge device {}: {:?}",
            self.device_id, message.command
        );
        Ok(())
    }

    /// Receive message from USB device
    async fn receive_message_internal(&mut self) -> Result<Message> {
        let mut handle = self.handle.lock().await;

        // Receive header first (24 bytes)
        let mut header_buf = vec![0u8; 24];
        let header_len = match self.receive_bulk(&mut *handle, &mut header_buf) {
            Ok(len) => len,
            Err(e) => {
                self.is_connected = false;
                return Err(e);
            }
        };

        if header_len != 24 {
            self.is_connected = false;
            let error = format!("Invalid header size: expected 24 bytes, got {}", header_len);
            bail!(error);
        }

        // Parse header to get data length
        let data_length = u32::from_le_bytes([
            header_buf[12],
            header_buf[13],
            header_buf[14],
            header_buf[15],
        ]) as usize;

        // Receive data if present
        let mut full_message = header_buf;
        if data_length > 0 {
            if data_length > MAXDATA {
                bail!("Data too large: {} bytes (max: {})", data_length, MAXDATA);
            }

            let mut data_buf = vec![0u8; data_length];
            let data_len = match self.receive_bulk(&mut *handle, &mut data_buf) {
                Ok(len) => len,
                Err(e) => {
                    self.is_connected = false;
                    return Err(e);
                }
            };

            if data_len != data_length {
                self.is_connected = false;
                let error = format!(
                    "Data length mismatch: expected {}, got {}",
                    data_length, data_len
                );
                bail!(error);
            }

            full_message.extend_from_slice(&data_buf);
        }

        let message = Message::deserialize(&full_message[..])?;
        debug!(
            "Received message from bridge device {}: {:?}",
            self.device_id, message.command
        );
        Ok(message)
    }

    /// Send bulk data in chunks
    fn send_bulk(&self, handle: &mut DeviceHandle<rusb::GlobalContext>, data: &[u8]) -> Result<()> {
        let mut offset = 0;
        while offset < data.len() {
            let chunk_size = std::cmp::min(16384, data.len() - offset); // 16KB chunks
            let chunk = &data[offset..offset + chunk_size];

            let written = handle
                .write_bulk(self.out_endpoint, chunk, USB_TIMEOUT)
                .context("Failed to write bulk data")?;

            if written != chunk_size {
                bail!("Partial write: expected {}, wrote {}", chunk_size, written);
            }

            offset += written;
        }
        Ok(())
    }

    /// Receive bulk data
    fn receive_bulk(
        &self,
        handle: &mut DeviceHandle<rusb::GlobalContext>,
        buffer: &mut [u8],
    ) -> Result<usize> {
        let read = handle
            .read_bulk(self.in_endpoint, buffer, USB_TIMEOUT)
            .context("Failed to read bulk data")?;

        Ok(read)
    }

    /// Read connection status from interrupt endpoint
    async fn read_connection_status(&self) -> Result<ConnectionStatus> {
        let handle = self.handle.lock().await;

        let mut status_buffer = [0u8; 4]; // Usually 1-4 bytes for status

        match handle.read_interrupt(
            self.interrupt_endpoint,
            &mut status_buffer,
            Duration::from_millis(100), // Short timeout for polling
        ) {
            Ok(bytes_read) => {
                if bytes_read == 0 {
                    return Ok(ConnectionStatus::Disconnected);
                }

                // Parse status packet (vendor-specific format)
                // This is a simplified interpretation - real implementation would
                // depend on the actual bridge cable's status packet format
                let status_byte = status_buffer[0];

                match status_byte {
                    0x00 => Ok(ConnectionStatus::Disconnected),
                    0x01 => Ok(ConnectionStatus::Ready),
                    0xFF => Ok(ConnectionStatus::Error("Bridge cable error".to_string())),
                    _ => {
                        debug!("Unknown bridge status byte: 0x{:02x}", status_byte);
                        Ok(ConnectionStatus::Disconnected) // Assume disconnected for safety
                    }
                }
            }
            Err(rusb::Error::Timeout) => {
                // Timeout is normal for polling - assume remote still disconnected
                Ok(ConnectionStatus::Disconnected)
            }
            Err(e) => {
                warn!("Failed to read bridge connection status: {}", e);
                Ok(ConnectionStatus::Error(format!(
                    "Status read failed: {}",
                    e
                )))
            }
        }
    }
}

#[async_trait]
impl Transport for BridgeUsbTransport {
    async fn send_message(&mut self, message: &Message) -> Result<()> {
        self.send_message_internal(message).await
    }

    async fn receive_message(&mut self) -> Result<Message> {
        self.receive_message_internal().await
    }

    async fn connect(&mut self) -> Result<()> {
        // Stage 1: USB connection is already established (done in constructor)
        info!(
            "Bridge USB device {} physically connected, checking remote connection...",
            self.device_id
        );

        // Check current remote status and set connection state accordingly
        match self.read_connection_status().await {
            Ok(ConnectionStatus::Ready) => {
                // Both USB and remote are connected - ready for communication
                self.is_connected = true;
                info!(
                    "Bridge USB device {} ready (remote already connected)",
                    self.device_id
                );
            }
            Ok(ConnectionStatus::Disconnected) => {
                // USB connected but remote disconnected - connected but not ready
                self.is_connected = false;
                info!(
                    "Bridge USB device {} connected (waiting for remote)",
                    self.device_id
                );
            }
            Ok(status) => {
                // Other status (Error) - not ready
                self.is_connected = false;
                info!(
                    "Bridge USB device {} connected, remote status: {}",
                    self.device_id, status
                );
            }
            Err(e) => {
                // Cannot determine remote status - connected with error info
                self.is_connected = false;
                warn!(
                    "Bridge USB device {} connected, remote status unknown: {}",
                    self.device_id, e
                );
            }
        }

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.is_connected = false;
        info!("Bridge USB device {} disconnected", self.device_id);
        Ok(())
    }

    async fn is_connected(&self) -> bool {
        // is_connected flag now accurately represents full connection state
        // (both USB and remote connected)
        self.is_connected
    }

    fn device_id(&self) -> &str {
        &self.device_id
    }

    fn transport_type(&self) -> TransportType {
        TransportType::BridgeUsb
    }

    async fn health_check(&self) -> Result<()> {
        if !self.is_connected {
            return Err(anyhow::anyhow!(
                "Bridge USB device {} is disconnected",
                self.device_id
            ));
        }

        // Check actual connection status via interrupt endpoint
        match self.read_connection_status().await? {
            ConnectionStatus::Ready => Ok(()),
            status => Err(anyhow::anyhow!(
                "Bridge USB device {} health check failed: {}",
                self.device_id,
                status
            )),
        }
    }

    async fn get_connection_status(&self) -> ConnectionStatus {
        // Read real-time status from interrupt endpoint
        match self.read_connection_status().await {
            Ok(status) => status,
            Err(e) => ConnectionStatus::Error(format!("Status check failed: {}", e)),
        }
    }
}

impl Drop for BridgeUsbTransport {
    fn drop(&mut self) {
        // Note: USB handle cleanup will be done by the Mutex Drop
        info!("Bridge USB transport {} dropped", self.device_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_status_display() {
        assert_eq!(ConnectionStatus::Connected.to_string(), "Connected");
        assert_eq!(ConnectionStatus::Disconnected.to_string(), "Disconnected");
        assert_eq!(
            ConnectionStatus::Error("test".to_string()).to_string(),
            "Error: test"
        );
    }

    #[test]
    fn test_transport_type() {
        // This would need a mock USB device to test fully
        // For now, just test the enum
        assert_eq!(TransportType::BridgeUsb.to_string(), "Bridge USB");
    }
}
