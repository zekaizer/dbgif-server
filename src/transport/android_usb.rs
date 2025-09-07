use std::collections::HashMap;
use std::time::Duration;
use anyhow::{Result, bail, Context};
use rusb::{Context as UsbContext, Device, DeviceHandle};
use tokio::sync::RwLock;
use tracing::{info, warn, debug};
use async_trait::async_trait;

use crate::protocol::message::Message;
use crate::protocol::constants::MAXDATA;
use super::{Transport, TransportType};

// Android/Google vendor ID
const ANDROID_VENDOR_ID: u16 = 0x18d1;

// USB configuration and interface
const USB_CONFIGURATION: u8 = 1;
const USB_INTERFACE: u8 = 0;

// Timeout for USB operations (5 seconds)
const USB_TIMEOUT: Duration = Duration::from_secs(5);

/// Android USB device transport
pub struct AndroidUsbTransport {
    device_id: String,
    handle: DeviceHandle<rusb::GlobalContext>,
    in_endpoint: u8,
    out_endpoint: u8,
    is_connected: bool,
}

impl AndroidUsbTransport {
    /// Create new Android USB transport for a device
    pub fn new(device: Device<rusb::GlobalContext>) -> Result<Self> {
        let device_descriptor = device.device_descriptor()
            .context("Failed to get device descriptor")?;
        
        // Get device serial number or use bus:address as fallback
        let device_id = match device.open() {
            Ok(handle) => {
                match handle.read_serial_number_string_ascii(&device_descriptor) {
                    Ok(serial) => serial,
                    Err(_) => format!("{}:{}", device.bus_number(), device.address()),
                }
            }
            Err(_) => format!("{}:{}", device.bus_number(), device.address()),
        };

        let handle = device.open()
            .context("Failed to open USB device")?;

        // Set configuration
        handle.set_active_configuration(USB_CONFIGURATION)
            .context("Failed to set USB configuration")?;

        // Claim interface
        handle.claim_interface(USB_INTERFACE)
            .context("Failed to claim USB interface")?;

        // Find bulk endpoints
        let config_descriptor = device.config_descriptor(0)
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

        for endpoint in interface_descriptor.endpoint_descriptors() {
            if endpoint.transfer_type() == rusb::TransferType::Bulk {
                match endpoint.direction() {
                    rusb::Direction::In => in_endpoint = Some(endpoint.address()),
                    rusb::Direction::Out => out_endpoint = Some(endpoint.address()),
                }
            }
        }

        let in_endpoint = in_endpoint
            .context("No bulk IN endpoint found")?;
        let out_endpoint = out_endpoint
            .context("No bulk OUT endpoint found")?;

        Ok(AndroidUsbTransport {
            device_id,
            handle,
            in_endpoint,
            out_endpoint,
            is_connected: true,
        })
    }

    /// Get device ID
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Send message to USB device (header first, then data)
    pub fn send_message(&mut self, message: &Message) -> Result<()> {
        let serialized = message.serialize();
        
        // Split into header (24 bytes) and data
        let header = &serialized[..24];
        let data = &serialized[24..];

        // Send header first
        match self.send_bulk(header) {
            Ok(_) => {},
            Err(e) => {
                self.is_connected = false;
                return Err(e);
            }
        }

        // Send data if present
        if !data.is_empty() {
            match self.send_bulk(data) {
                Ok(_) => {},
                Err(e) => {
                    self.is_connected = false;
                    return Err(e);
                }
            }
        }

        debug!("Sent message to device {}: {:?}", self.device_id, message.command);
        Ok(())
    }

    /// Receive message from USB device
    pub fn receive_message(&mut self) -> Result<Message> {
        // Receive header first (24 bytes)
        let mut header_buf = vec![0u8; 24];
        let header_len = match self.receive_bulk(&mut header_buf) {
            Ok(len) => len,
            Err(e) => {
                self.is_connected = false;
                return Err(e);
            }
        };
        
        if header_len != 24 {
            self.is_connected = false;
            bail!("Invalid header size: expected 24 bytes, got {}", header_len);
        }

        // Parse header to get data length
        let data_length = u32::from_le_bytes([
            header_buf[12], header_buf[13], header_buf[14], header_buf[15]
        ]) as usize;

        // Receive data if present
        let mut full_message = header_buf;
        if data_length > 0 {
            if data_length > MAXDATA {
                bail!("Data too large: {} bytes (max: {})", data_length, MAXDATA);
            }

            let mut data_buf = vec![0u8; data_length];
            let data_len = match self.receive_bulk(&mut data_buf) {
                Ok(len) => len,
                Err(e) => {
                    self.is_connected = false;
                    return Err(e);
                }
            };
            
            if data_len != data_length {
                self.is_connected = false;
                bail!("Data length mismatch: expected {}, got {}", data_length, data_len);
            }

            full_message.extend_from_slice(&data_buf);
        }

        let message = Message::deserialize(&full_message[..])?;
        debug!("Received message from device {}: {:?}", self.device_id, message.command);
        Ok(message)
    }

    /// Send bulk data in chunks
    fn send_bulk(&mut self, data: &[u8]) -> Result<()> {
        let mut offset = 0;
        while offset < data.len() {
            let chunk_size = std::cmp::min(16384, data.len() - offset); // 16KB chunks
            let chunk = &data[offset..offset + chunk_size];
            
            let written = self.handle.write_bulk(
                self.out_endpoint,
                chunk,
                USB_TIMEOUT
            ).context("Failed to write bulk data")?;

            if written != chunk_size {
                bail!("Partial write: expected {}, wrote {}", chunk_size, written);
            }

            offset += written;
        }
        Ok(())
    }

    /// Receive bulk data
    fn receive_bulk(&mut self, buffer: &mut [u8]) -> Result<usize> {
        let read = self.handle.read_bulk(
            self.in_endpoint,
            buffer,
            USB_TIMEOUT
        ).context("Failed to read bulk data")?;

        Ok(read)
    }
}

#[async_trait]
impl Transport for AndroidUsbTransport {
    async fn send_message(&mut self, message: &Message) -> Result<()> {
        // Call the existing method
        Self::send_message(self, message)
    }

    async fn receive_message(&mut self) -> Result<Message> {
        // Call the existing method
        Self::receive_message(self)
    }

    async fn connect(&mut self) -> Result<()> {
        // USB devices are connected upon creation
        // This is mainly for reconnection scenarios
        self.is_connected = true;
        info!("USB device {} connected", self.device_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.is_connected = false;
        info!("USB device {} marked as disconnected", self.device_id);
        Ok(())
    }

    async fn is_connected(&self) -> bool {
        self.is_connected
    }

    fn device_id(&self) -> &str {
        &self.device_id
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Usb
    }

    async fn health_check(&self) -> Result<()> {
        if self.is_connected {
            // Could add actual USB device validation here
            Ok(())
        } else {
            Err(anyhow::anyhow!("USB device {} is not connected", self.device_id))
        }
    }
}

impl Drop for AndroidUsbTransport {
    fn drop(&mut self) {
        if let Err(e) = self.handle.release_interface(USB_INTERFACE) {
            warn!("Failed to release USB interface for device {}: {}", self.device_id, e);
        }
    }
}

/// Manager for multiple Android USB devices
pub struct AndroidUsbTransportManager {
    devices: RwLock<HashMap<String, AndroidUsbTransport>>,
    context: UsbContext,
}

impl AndroidUsbTransportManager {
    /// Create new Android USB transport manager
    pub fn new() -> Result<Self> {
        let context = UsbContext::new()
            .context("Failed to create USB context")?;

        Ok(AndroidUsbTransportManager {
            devices: RwLock::new(HashMap::new()),
            context,
        })
    }

    /// Scan for connected DBGIF-compatible devices
    pub async fn scan_devices(&self) -> Result<Vec<String>> {
        // Note: This is a simplified scan that just returns device IDs
        // In a real implementation, we'd need to properly handle the context types
        let found_devices = Vec::new();
        
        // For now, return empty list as device scanning needs more complex USB context handling
        debug!("USB device scan requested - returning empty list for now");
        
        Ok(found_devices)
    }

    /// Add a new Android USB device
    pub async fn add_device(&self, device: Device<rusb::GlobalContext>) -> Result<String> {
        let transport = AndroidUsbTransport::new(device)?;
        let device_id = transport.device_id().to_string();

        let mut devices = self.devices.write().await;
        devices.insert(device_id.clone(), transport);

        info!("Added USB device: {}", device_id);
        Ok(device_id)
    }

    /// Remove USB device
    pub async fn remove_device(&self, device_id: &str) -> Result<()> {
        let mut devices = self.devices.write().await;
        
        if devices.remove(device_id).is_some() {
            info!("Removed USB device: {}", device_id);
            Ok(())
        } else {
            bail!("Device not found: {}", device_id);
        }
    }

    /// Send message to specific device
    pub async fn send_to_device(&self, device_id: &str, message: &Message) -> Result<()> {
        let mut devices = self.devices.write().await;
        
        match devices.get_mut(device_id) {
            Some(transport) => transport.send_message(message),
            None => bail!("Device not found: {}", device_id),
        }
    }

    /// Receive message from specific device
    pub async fn receive_from_device(&self, device_id: &str) -> Result<Message> {
        let mut devices = self.devices.write().await;
        
        match devices.get_mut(device_id) {
            Some(transport) => transport.receive_message(),
            None => bail!("Device not found: {}", device_id),
        }
    }

    /// Get list of connected device IDs
    pub async fn get_device_ids(&self) -> Vec<String> {
        let devices = self.devices.read().await;
        devices.keys().cloned().collect()
    }

    /// Monitor for device connections/disconnections
    pub async fn monitor_devices(&self) -> Result<()> {
        // This would typically run in a background task
        // For now, this is a placeholder for hot-plug detection
        info!("USB device monitoring started");
        
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            
            // Scan for new devices
            if let Ok(current_devices) = self.scan_devices().await {
                let existing_devices = self.get_device_ids().await;
                
                // Check for new devices
                for device_id in &current_devices {
                    if !existing_devices.contains(device_id) {
                        info!("New USB device detected: {}", device_id);
                        // Note: In a real implementation, you'd need to re-scan 
                        // and add the actual Device object
                    }
                }

                // Check for removed devices
                for device_id in existing_devices {
                    if !current_devices.contains(&device_id) {
                        warn!("USB device disconnected: {}", device_id);
                        let _ = self.remove_device(&device_id).await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_android_usb_manager_creation() {
        let manager = AndroidUsbTransportManager::new();
        assert!(manager.is_ok());
    }

    #[tokio::test]
    async fn test_device_list_empty() {
        let manager = AndroidUsbTransportManager::new().unwrap();
        let devices = manager.get_device_ids().await;
        assert!(devices.is_empty());
    }

    #[tokio::test]
    async fn test_scan_devices() {
        let manager = AndroidUsbTransportManager::new().unwrap();
        let result = manager.scan_devices().await;
        // This should not fail even if no devices are connected
        assert!(result.is_ok());
    }
}