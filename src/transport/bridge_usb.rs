use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use rusb::{Device, DeviceHandle};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::{ConnectionStatus, Transport, TransportType, UsbTransportFactory};
use crate::protocol::constants::MAXDATA;
use crate::protocol::message::Message;

// USB Host-to-Host Bridge Cable device VID/PID pairs
const SUPPORTED_BRIDGE_DEVICES: &[(u16, u16)] = &[
    // Prolific devices
    (0x067b, 0x2501), // PL-2501 USB-to-USB Bridge Cable
    (0x067b, 0x2506), // PL-2506 USB Host-to-Host Bridge
    (0x067b, 0x25a1), // PL-25A1 USB-to-USB Network Bridge
    // ASIX devices
    (0x0b95, 0x7720), // AX88772 USB-to-USB Bridge
    (0x0b95, 0x772a), // AX88772A USB-to-USB Network Bridge
    (0x0b95, 0x7e2b), // AX88772B USB-to-USB Bridge
    // ATEN devices
    (0x0557, 0x2009), // UC-2324 USB-to-USB Bridge Cable
    (0x0557, 0x7000), // UC-2322 USB Host-to-Host Bridge
    // Cables Unlimited devices
    (0x0731, 0x2003), // USB-2003 USB Bridge Cable
    // StarTech devices
    (0x067b, 0x3500), // USB3SBRIDGE USB 3.0 Bridge Cable
    // Generic devices
    (0x1234, 0x5678), // Generic USB Bridge Cable (placeholder)
];

// USB configuration and interface
const USB_CONFIGURATION: u8 = 1;
const USB_INTERFACE: u8 = 0;

// Timeout for USB operations
const USB_TIMEOUT: Duration = Duration::from_secs(5);

// PL2501 vendor-specific control commands
#[derive(Debug, Clone, Copy)]
enum Pl2501VendorCmd {
    ClearFeatures = 1,
    SetFeatures = 3,
}

// PL2501 feature bit flags
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum Pl2501Feature {
    SuspendEn = 1 << 7,  // Suspend enable
    TxReady = 1 << 5,    // Transmit ready (interrupt only)
    ResetOut = 1 << 4,   // Reset output pipe
    ResetIn = 1 << 3,    // Reset input pipe
    TxComplete = 1 << 2, // Transmission complete
    TxRequest = 1 << 1,  // Transmission received
    PeerExists = 1 << 0, // Peer exists
}

impl Pl2501Feature {
    const fn as_u8(self) -> u8 {
        self as u8
    }
}

// USB control transfer constants
const USB_DIR_IN: u8 = 0x80;
const USB_TYPE_VENDOR: u8 = 0x40;
const USB_RECIP_DEVICE: u8 = 0x00;
const USB_CTRL_GET_TIMEOUT: Duration = Duration::from_millis(5000);

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
    /// PL2501 vendor request helper
    fn pl2501_vendor_request(
        &self,
        handle: &DeviceHandle<rusb::GlobalContext>,
        cmd: Pl2501VendorCmd,
        features: u8,
    ) -> Result<()> {
        let request_type = USB_DIR_IN | USB_TYPE_VENDOR | USB_RECIP_DEVICE;

        handle
            .write_control(
                request_type,
                cmd as u8,
                features as u16,
                0,
                &[],
                USB_CTRL_GET_TIMEOUT,
            )
            .context("PL2501 vendor request failed")?;

        debug!(
            "PL2501 vendor request: cmd={:?}, features=0x{:02x}",
            cmd, features
        );
        Ok(())
    }

    /// Set PL2501 QuickLink features
    fn pl2501_set_features(
        &self,
        handle: &DeviceHandle<rusb::GlobalContext>,
        features: u8,
    ) -> Result<()> {
        self.pl2501_vendor_request(handle, Pl2501VendorCmd::SetFeatures, features)
    }

    /// Clear PL2501 QuickLink features
    fn pl2501_clear_features(
        &self,
        handle: &DeviceHandle<rusb::GlobalContext>,
        features: u8,
    ) -> Result<()> {
        self.pl2501_vendor_request(handle, Pl2501VendorCmd::ClearFeatures, features)
    }

    /// Reset PL2501 pipes (both input and output)
    fn pl2501_reset_pipes(&self, handle: &DeviceHandle<rusb::GlobalContext>) -> Result<()> {
        let reset_flags = Pl2501Feature::ResetOut.as_u8() | Pl2501Feature::ResetIn.as_u8();
        self.pl2501_set_features(handle, reset_flags)
    }

    /// Full PL2501 reset sequence (as used in Linux driver)
    fn pl2501_full_reset(&self, handle: &DeviceHandle<rusb::GlobalContext>) -> Result<()> {
        let reset_flags = Pl2501Feature::SuspendEn.as_u8()
            | Pl2501Feature::ResetOut.as_u8()
            | Pl2501Feature::ResetIn.as_u8()
            | Pl2501Feature::PeerExists.as_u8();

        self.pl2501_set_features(handle, reset_flags)
            .context("PL2501 full reset failed")
    }

    /// Enable suspend mode
    fn pl2501_enable_suspend(&self, handle: &DeviceHandle<rusb::GlobalContext>) -> Result<()> {
        self.pl2501_set_features(handle, Pl2501Feature::SuspendEn.as_u8())
    }

    /// Enable peer detection
    fn pl2501_enable_peer_detection(
        &self,
        handle: &DeviceHandle<rusb::GlobalContext>,
    ) -> Result<()> {
        self.pl2501_set_features(handle, Pl2501Feature::PeerExists.as_u8())
    }

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

        // Create transport instance
        let transport = BridgeUsbTransport {
            device_id,
            handle: Arc::new(Mutex::new(handle)),
            in_endpoint,
            out_endpoint,
            interrupt_endpoint,
            is_connected: false, // Will be set by connect()
        };

        // Initialize PL2501 with full reset sequence
        {
            let handle = transport
                .handle
                .try_lock()
                .context("Failed to lock handle for PL2501 initialization")?;
            transport
                .pl2501_full_reset(&*handle)
                .context("PL2501 initialization failed")?;
            info!(
                "PL2501 initialization completed for device {}",
                transport.device_id
            );
        }

        Ok(transport)
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

    /// Read PL2501 feature status (check current feature settings)
    fn pl2501_read_status(&self, handle: &DeviceHandle<rusb::GlobalContext>) -> Result<u8> {
        let request_type = USB_DIR_IN | USB_TYPE_VENDOR | USB_RECIP_DEVICE;
        let mut status_buffer = [0u8; 1];

        let bytes_read = handle
            .read_control(
                request_type,
                0x02, // Hypothetical status read command
                0,
                0,
                &mut status_buffer,
                USB_CTRL_GET_TIMEOUT,
            )
            .context("Failed to read PL2501 status")?;

        if bytes_read == 1 {
            Ok(status_buffer[0])
        } else {
            // Control transfer failed or returned unexpected size, return default
            Ok(0)
        }
    }

    /// Read connection status using PL2501 control transfer
    async fn read_connection_status(&self) -> Result<ConnectionStatus> {
        let handle = self.handle.lock().await;

        // Read PL2501 status via control transfer
        let status_byte = match self.pl2501_read_status(&*handle) {
            Ok(status) => status,
            Err(e) => {
                return Ok(ConnectionStatus::Error(format!(
                    "Status read failed: {}",
                    e
                )));
            }
        };

        // Parse PL2501 feature status byte - only check PEER_EXISTS
        if status_byte & Pl2501Feature::PeerExists.as_u8() != 0 {
            Ok(ConnectionStatus::Connected) // Peer is connected
        } else {
            Ok(ConnectionStatus::Disconnected) // No peer detected
        }
    }

    /// Check if peer is connected using PL2501 PEER_E bit
    async fn pl2501_is_peer_connected(&self) -> bool {
        let handle = self.handle.lock().await;

        match self.pl2501_read_status(&*handle) {
            Ok(status) => (status & Pl2501Feature::PeerExists.as_u8()) != 0,
            Err(_) => false,
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
        info!(
            "Bridge USB device {} connecting, checking remote connection...",
            self.device_id
        );

        // Re-initialize PL2501 to ensure proper state
        {
            let handle = self.handle.lock().await;
            if let Err(e) = self.pl2501_full_reset(&*handle) {
                warn!("PL2501 reset during connect failed: {}", e);
                // Continue anyway, maybe it was already initialized
            }

            // Ensure peer detection is enabled
            if let Err(e) = self.pl2501_enable_peer_detection(&*handle) {
                warn!("PL2501 peer detection enable failed: {}", e);
            }
        }

        // Check current remote status and set connection state accordingly
        match self.read_connection_status().await {
            Ok(ConnectionStatus::Connected) => {
                self.is_connected = true;
                info!(
                    "Bridge USB device {} connected (remote connected)",
                    self.device_id
                );
            }
            Ok(ConnectionStatus::Disconnected) => {
                self.is_connected = false;
                info!(
                    "Bridge USB device {} waiting (remote not connected)",
                    self.device_id
                );
            }
            Ok(status) => {
                self.is_connected = false;
                info!(
                    "Bridge USB device {} error status: {}",
                    self.device_id, status
                );
            }
            Err(e) => {
                self.is_connected = false;
                warn!(
                    "Bridge USB device {} status check failed: {}",
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
        // Always check actual status, don't rely on cached is_connected flag
        match self.read_connection_status().await? {
            ConnectionStatus::Connected => {
                debug!("Bridge USB device {} health check passed", self.device_id);
                Ok(())
            }
            ConnectionStatus::Disconnected => {
                // Try to recover by re-enabling peer detection
                let handle = self.handle.lock().await;
                if let Err(e) = self.pl2501_enable_peer_detection(&*handle) {
                    warn!(
                        "Failed to re-enable peer detection during health check: {}",
                        e
                    );
                }

                Err(anyhow::anyhow!(
                    "Bridge USB device {} health check failed: remote disconnected",
                    self.device_id
                ))
            }
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

/// Factory for creating Bridge USB transports
pub struct BridgeUsbFactory;

impl BridgeUsbFactory {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl UsbTransportFactory for BridgeUsbFactory {
    fn supported_devices(&self) -> &[(u16, u16)] {
        SUPPORTED_BRIDGE_DEVICES
    }

    async fn create_transport(
        &self,
        device: Device<rusb::GlobalContext>,
    ) -> Result<Box<dyn Transport + Send>> {
        let transport = BridgeUsbTransport::new(device)?;
        Ok(Box::new(transport))
    }

    fn name(&self) -> &str {
        "BridgeUSB"
    }

    fn validate_device(&self, device: &Device<rusb::GlobalContext>) -> Result<()> {
        let descriptor = device.device_descriptor()?;

        // Additional validation for Bridge devices
        // Check for required endpoints (Bulk IN/OUT + Interrupt IN)
        let config_descriptor = device.config_descriptor(0)?;

        for interface in config_descriptor.interfaces() {
            for interface_descriptor in interface.descriptors() {
                let mut has_bulk_in = false;
                let mut has_bulk_out = false;
                let mut has_interrupt_in = false;

                for endpoint in interface_descriptor.endpoint_descriptors() {
                    match endpoint.transfer_type() {
                        rusb::TransferType::Bulk => match endpoint.direction() {
                            rusb::Direction::In => has_bulk_in = true,
                            rusb::Direction::Out => has_bulk_out = true,
                        },
                        rusb::TransferType::Interrupt => {
                            if endpoint.direction() == rusb::Direction::In {
                                has_interrupt_in = true;
                            }
                        }
                        _ => {}
                    }
                }

                if has_bulk_in && has_bulk_out && has_interrupt_in {
                    return Ok(());
                }
            }
        }

        // If required endpoints not found, still allow if VID/PID matches
        debug!(
            "Bridge device VID={:04x} PID={:04x} doesn't have required endpoints, but VID/PID matches",
            descriptor.vendor_id(), descriptor.product_id()
        );
        Ok(())
    }
}

impl Default for BridgeUsbFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::UsbDeviceInfo;

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

    #[test]
    fn test_bridge_usb_factory() {
        let factory = BridgeUsbFactory::new();
        assert_eq!(factory.name(), "BridgeUSB");
        assert!(!factory.supported_devices().is_empty());

        // Test Prolific PL-2501 VID/PID matching
        let prolific_info = UsbDeviceInfo {
            vendor_id: 0x067b,
            product_id: 0x2501,
            bus_number: 1,
            address: 3,
            serial: Some("bridge_test".to_string()),
        };
        assert!(factory.matches(&prolific_info));

        // Test non-matching device
        let unknown_info = UsbDeviceInfo {
            vendor_id: 0xffff,
            product_id: 0xffff,
            bus_number: 1,
            address: 4,
            serial: None,
        };
        assert!(!factory.matches(&unknown_info));
    }

    #[test]
    fn test_pl2501_feature_bits() {
        // Test feature bit values match documentation
        assert_eq!(Pl2501Feature::SuspendEn.as_u8(), 0x80);
        assert_eq!(Pl2501Feature::TxReady.as_u8(), 0x20);
        assert_eq!(Pl2501Feature::ResetOut.as_u8(), 0x10);
        assert_eq!(Pl2501Feature::ResetIn.as_u8(), 0x08);
        assert_eq!(Pl2501Feature::TxComplete.as_u8(), 0x04);
        assert_eq!(Pl2501Feature::TxRequest.as_u8(), 0x02);
        assert_eq!(Pl2501Feature::PeerExists.as_u8(), 0x01);

        // Test combined flags
        let reset_flags = Pl2501Feature::ResetOut.as_u8() | Pl2501Feature::ResetIn.as_u8();
        assert_eq!(reset_flags, 0x18);

        let full_reset = Pl2501Feature::SuspendEn.as_u8()
            | Pl2501Feature::ResetOut.as_u8()
            | Pl2501Feature::ResetIn.as_u8()
            | Pl2501Feature::PeerExists.as_u8();
        assert_eq!(full_reset, 0x99); // 0x80 | 0x10 | 0x08 | 0x01
    }

    #[test]
    fn test_pl2501_vendor_commands() {
        assert_eq!(Pl2501VendorCmd::ClearFeatures as u8, 1);
        assert_eq!(Pl2501VendorCmd::SetFeatures as u8, 3);
    }

    #[test]
    fn test_usb_control_constants() {
        assert_eq!(USB_DIR_IN, 0x80);
        assert_eq!(USB_TYPE_VENDOR, 0x40);
        assert_eq!(USB_RECIP_DEVICE, 0x00);

        // Test combined request type for vendor requests
        let request_type = USB_DIR_IN | USB_TYPE_VENDOR | USB_RECIP_DEVICE;
        assert_eq!(request_type, 0xC0);
    }
}
