use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use nusb::{DeviceInfo, Interface};
use nusb::transfer::RequestBuffer;
use nusb::transfer::{ControlIn, ControlOut, ControlType, Recipient};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::{ConnectionStatus, Transport, TransportType, UsbTransportFactory};
use crate::protocol::constants::MAXDATA;

// PL-25A1 USB Host-to-Host Bridge Controller
const PL25A1_VID: u16 = 0x067b;
const PL25A1_PID: u16 = 0x25a1;

// Vendor Control Commands (from PL25A1.md)
const VENDOR_CMD_QUERY_STATUS: u8 = 0xF7;     // Query feature status
const VENDOR_CMD_SET_CONFIG: u8 = 0xF8;       // Set feature config
const VENDOR_CMD_POWER_OFF: u8 = 0xF9;        // Power off device
const VENDOR_CMD_RESET: u8 = 0xFA;            // Reset device
const VENDOR_CMD_CONNECTION_STATE: u8 = 0xFB; // Get connection state

// Connection state bits
const STATUS_DISCONNECTED: u8 = 0x02;
const STATUS_READY: u8 = 0x04;
const STATUS_CONNECTOR_ID: u8 = 0x08;

// USB interface configuration
const USB_INTERFACE: u8 = 0;

// Endpoint addresses (typical for PL-25A1)
const BULK_OUT_EP: u8 = 0x02;
const BULK_IN_EP: u8 = 0x81;
const INTERRUPT_IN_EP: u8 = 0x83;

// Timeouts
const USB_TIMEOUT: Duration = Duration::from_secs(5);
const INTERRUPT_TIMEOUT: Duration = Duration::from_millis(100);

// USB packet size alignment (typical bulk endpoint max packet size)
const USB_BULK_MAX_PACKET_SIZE: usize = 64;

/// PL-25A1 Connection State
#[derive(Debug, Clone, Copy)]
pub struct PL25A1ConnectionState {
    pub local_state: u8,
    pub remote_state: u8,
}

impl PL25A1ConnectionState {
    pub fn from_bytes(bytes: [u8; 2]) -> Self {
        Self {
            local_state: bytes[0],
            remote_state: bytes[1],
        }
    }
    
    pub fn is_disconnected(&self) -> bool {
        (self.local_state & STATUS_DISCONNECTED) != 0 || 
        (self.remote_state & STATUS_DISCONNECTED) != 0
    }
    
    pub fn is_ready(&self) -> bool {
        (self.local_state & STATUS_READY) != 0 && 
        (self.remote_state & STATUS_READY) != 0
    }
    
    pub fn get_connector_id(&self) -> (bool, bool) {
        let local_connector = (self.local_state & STATUS_CONNECTOR_ID) != 0;
        let remote_connector = (self.remote_state & STATUS_CONNECTOR_ID) != 0;
        (local_connector, remote_connector)
    }
}

/// PL-25A1 USB Transport
pub struct BridgeUsbTransport {
    device_id: String,
    interface: Arc<Mutex<Interface>>,
    is_connected: bool,
}

impl std::fmt::Debug for BridgeUsbTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BridgeUsbTransport")
            .field("device_id", &self.device_id)
            .field("is_connected", &self.is_connected)
            .finish_non_exhaustive()
    }
}

impl BridgeUsbTransport {
    /// Create new PL-25A1 USB transport for a device
    pub async fn new(device_info: DeviceInfo) -> Result<Self> {
        // Generate device ID from serial or bus:address
        let device_id = match device_info.serial_number() {
            Some(serial) => format!("pl25a1_{}", serial),
            None => format!("pl25a1_{}:{}", device_info.bus_number(), device_info.device_address()),
        };

        // Open device and claim interface
        let device = device_info.open().context("Failed to open USB device")?;
        let interface = device
            .claim_interface(USB_INTERFACE)
            .context("Failed to claim USB interface")?;

        info!("PL-25A1 USB device {} initialized", device_id);

        let transport = Self {
            device_id,
            interface: Arc::new(Mutex::new(interface)),
            is_connected: true,
        };

        // Check initial connection state
        match transport.get_connection_state().await {
            Ok(state) => {
                let (local_id, remote_id) = state.get_connector_id();
                info!(
                    "PL-25A1 connection state - Local: 0x{:02x}, Remote: 0x{:02x}, Ready: {}, Connector IDs: ({}, {})",
                    state.local_state, state.remote_state, state.is_ready(), local_id, remote_id
                );
            }
            Err(e) => warn!("Failed to get initial connection state: {}", e),
        }

        Ok(transport)
    }

    /// Get connection state using vendor command 0xFB
    pub async fn get_connection_state(&self) -> Result<PL25A1ConnectionState> {
        let interface = self.interface.lock().await;
        
        let control_req = ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: VENDOR_CMD_CONNECTION_STATE,
            value: 0,
            index: 0,
            length: 2,
        };
        
        let completion = interface
            .control_in(control_req)
            .await;
        
        completion.status.context("Failed to read connection state")?;
        
        if completion.data.len() != 2 {
            bail!("Invalid connection state response length: {}", completion.data.len());
        }

        Ok(PL25A1ConnectionState::from_bytes([completion.data[0], completion.data[1]]))
    }

    /// Query feature status using vendor command 0xF7
    pub async fn query_feature_status(&self) -> Result<[u8; 2]> {
        let interface = self.interface.lock().await;
        
        let control_req = ControlIn {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: VENDOR_CMD_QUERY_STATUS,
            value: 0,
            index: 0,
            length: 2,
        };
        
        let completion = interface
            .control_in(control_req)
            .await;
        
        completion.status.context("Failed to query feature status")?;
        
        if completion.data.len() != 2 {
            bail!("Invalid feature status response length: {}", completion.data.len());
        }

        Ok([completion.data[0], completion.data[1]])
    }

    /// Set feature configuration using vendor command 0xF8
    pub async fn set_feature_config(&self, config: [u8; 2]) -> Result<()> {
        let interface = self.interface.lock().await;
        
        let control_req = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: VENDOR_CMD_SET_CONFIG,
            value: 0,
            index: 0,
            data: config.as_slice(),
        };
        
        let completion = interface
            .control_out(control_req)
            .await;
        
        completion.status.context("Failed to set feature config")?;

        Ok(())
    }

    /// Power off device using vendor command 0xF9
    pub async fn power_off(&self) -> Result<()> {
        let interface = self.interface.lock().await;
        
        let control_req = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: VENDOR_CMD_POWER_OFF,
            value: 0,
            index: 0,
            data: &[],
        };
        
        let completion = interface
            .control_out(control_req)
            .await;
        
        completion.status.context("Failed to power off device")?;

        Ok(())
    }

    /// Reset device using vendor command 0xFA
    pub async fn reset_device(&self) -> Result<()> {
        let interface = self.interface.lock().await;
        
        let control_req = ControlOut {
            control_type: ControlType::Vendor,
            recipient: Recipient::Device,
            request: VENDOR_CMD_RESET,
            value: 0,
            index: 0,
            data: &[],
        };
        
        let completion = interface
            .control_out(control_req)
            .await;
        
        completion.status.context("Failed to reset device")?;

        Ok(())
    }

    /// Poll interrupt endpoint for connection status updates
    pub async fn poll_interrupt(&self) -> Result<Vec<u8>> {
        let interface = self.interface.lock().await;
        let request_buffer = RequestBuffer::new(64);
        
        match tokio::time::timeout(
            INTERRUPT_TIMEOUT,
            interface.interrupt_in(INTERRUPT_IN_EP, request_buffer),
        ).await {
            Ok(completion) => {
                match completion.status {
                    Ok(()) => Ok(completion.data),
                    Err(e) => Err(e.into()),
                }
            }
            Err(_) => Ok(Vec::new()), // Timeout, no data
        }
    }

    /// Monitor connection status by periodically checking interrupt endpoint and connection state
    pub async fn monitor_connection_status(&mut self) -> Result<bool> {
        // First, poll interrupt endpoint for immediate status updates
        match self.poll_interrupt().await {
            Ok(data) if !data.is_empty() => {
                debug!(
                    "Interrupt data received ({}): {:02x?}",
                    data.len(),
                    data
                );
                // Any interrupt data suggests connection state change
            }
            Ok(_) => {
                // No interrupt data, continue with regular status check
            }
            Err(e) => {
                warn!("Interrupt endpoint read error: {}", e);
            }
        }

        // Check actual connection state using vendor command
        match self.get_connection_state().await {
            Ok(state) => {
                let was_connected = self.is_connected;
                self.is_connected = !state.is_disconnected();
                
                if was_connected != self.is_connected {
                    if self.is_connected {
                        info!("PL-25A1 device {} connected (Ready: {})", self.device_id, state.is_ready());
                    } else {
                        info!("PL-25A1 device {} disconnected", self.device_id);
                    }
                }
                
                Ok(self.is_connected)
            }
            Err(e) => {
                warn!("Failed to check connection state: {}", e);
                self.is_connected = false;
                Ok(false)
            }
        }
    }

    /// Send data to USB device (header first, then remaining data)
    async fn send_bytes_internal(&mut self, data: &[u8]) -> Result<()> {
        if data.len() < 24 {
            return Err(anyhow::anyhow!("Data too short, expected at least 24 bytes for header"));
        }

        // Split into header (24 bytes) and remaining data
        let header = &data[..24];
        let payload = &data[24..];

        let interface = self.interface.lock().await;

        // Send header first
        let completion = interface.bulk_out(BULK_OUT_EP, header.to_vec()).await;
        if let Err(e) = completion.status {
            self.is_connected = false;
            return Err(anyhow::anyhow!("Header send failed: {}", e));
        }

        // Send remaining data if present
        if !payload.is_empty() {
            let completion = interface.bulk_out(BULK_OUT_EP, payload.to_vec()).await;
            if let Err(e) = completion.status {
                self.is_connected = false;
                return Err(anyhow::anyhow!("Data send failed: {}", e));
            }
        }

        debug!(
            "Sent {} bytes to PL-25A1 device {}",
            data.len(), self.device_id
        );
        Ok(())
    }

    /// Receive data from USB device with specified buffer size
    /// Returns (data, actual_received_size)
    async fn receive_bytes_internal(&mut self, buffer_size: usize) -> Result<(Vec<u8>, usize)> {
        let interface = self.interface.lock().await;

        // Align buffer size down to USB packet boundary for optimal transfer
        let aligned_size = (buffer_size / USB_BULK_MAX_PACKET_SIZE) * USB_BULK_MAX_PACKET_SIZE;
        let final_size = if aligned_size == 0 { 
            USB_BULK_MAX_PACKET_SIZE 
        } else { 
            aligned_size.min(MAXDATA) 
        };
        
        let request_buffer = RequestBuffer::new(final_size);
        let completion = interface.bulk_in(BULK_IN_EP, request_buffer).await;
        
        if let Err(e) = completion.status {
            self.is_connected = false;
            return Err(anyhow::anyhow!("USB receive failed: {}", e));
        }
        
        let received_data = completion.data;
        let actual_size = received_data.len();

        debug!(
            "Received {} bytes from PL-25A1 device {} (requested: {}, aligned: {})",
            actual_size, self.device_id, buffer_size, final_size
        );
        
        Ok((received_data, actual_size))
    }

    /// Read connection status from interrupt endpoint or vendor command
    async fn read_connection_status(&self) -> Result<ConnectionStatus> {
        // First try to get detailed connection state
        match self.get_connection_state().await {
            Ok(state) => {
                if state.is_disconnected() {
                    Ok(ConnectionStatus::Disconnected)
                } else if state.is_ready() {
                    Ok(ConnectionStatus::Ready)
                } else {
                    Ok(ConnectionStatus::Connected)
                }
            }
            Err(e) => {
                warn!("Failed to get connection state via vendor command: {}", e);
                
                // Fallback to interrupt endpoint polling
                match self.poll_interrupt().await {
                    Ok(data) if !data.is_empty() => {
                        // Basic status interpretation based on interrupt data
                        // This is simplified - real implementation depends on interrupt data format
                        debug!("Interrupt status data: {:02x?}", data);
                        Ok(ConnectionStatus::Ready) // Assume ready if we got interrupt data
                    }
                    Ok(_) => Ok(ConnectionStatus::Disconnected), // No interrupt data
                    Err(_) => Ok(ConnectionStatus::Disconnected), // Error reading interrupt
                }
            }
        }
    }
}

#[async_trait]
impl Transport for BridgeUsbTransport {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        if !self.is_connected {
            bail!("PL-25A1 device {} is not connected", self.device_id);
        }
        self.send_bytes_internal(data).await
    }

    async fn receive(&mut self, buffer_size: usize) -> Result<Vec<u8>> {
        if !self.is_connected {
            bail!("PL-25A1 device {} is not connected", self.device_id);
        }
        let (data, _actual_size) = self.receive_bytes_internal(buffer_size).await?;
        Ok(data)
    }

    fn device_id(&self) -> &str {
        &self.device_id
    }

    async fn connect(&mut self) -> Result<ConnectionStatus> {
        // PL-25A1 Bridge Cable requires continuous monitoring regardless of initial state
        // The cable can be connected/disconnected on either side at any time
        // Always return Connected to ensure continuous polling is started
        info!("PL-25A1 device {} added with continuous monitoring", self.device_id);
        
        // Log initial state for debugging, but always return Connected
        match self.get_connection_state().await {
            Ok(state) => {
                let (local_id, remote_id) = state.get_connector_id();
                debug!(
                    "PL-25A1 initial state - Local: 0x{:02x}, Remote: 0x{:02x}, Ready: {}, Disconnected: {}, Connector IDs: ({}, {})",
                    state.local_state, state.remote_state, state.is_ready(), state.is_disconnected(), local_id, remote_id
                );
            }
            Err(e) => {
                debug!("Failed to get initial PL-25A1 connection state: {}", e);
            }
        }
        
        // Always return Connected to trigger continuous polling
        Ok(ConnectionStatus::Connected)
    }

    fn transport_type(&self) -> TransportType {
        TransportType::BridgeUsb
    }

    async fn is_connected(&self) -> bool {
        // Check actual connection state, not just the flag
        match self.get_connection_state().await {
            Ok(state) => !state.is_disconnected(),
            Err(_) => false,
        }
    }

    async fn disconnect(&mut self) -> Result<()> {
        if self.is_connected {
            info!("Disconnecting PL-25A1 device {}", self.device_id);
            self.is_connected = false;
        }
        Ok(())
    }

    /// Override get_connection_status to provide accurate PL-25A1 state
    async fn get_connection_status(&self) -> ConnectionStatus {
        // Check physical USB connection first
        if !self.is_connected {
            return ConnectionStatus::Disconnected;
        }
        
        // Check PL-25A1 connection state using vendor command
        match self.get_connection_state().await {
            Ok(state) => {
                if state.is_disconnected() {
                    ConnectionStatus::Disconnected
                } else if state.is_ready() {
                    ConnectionStatus::Ready
                } else {
                    ConnectionStatus::Connected
                }
            }
            Err(e) => {
                // Connection state check failed - return error
                ConnectionStatus::Error(format!("Failed to get connection state: {}", e))
            }
        }
    }
}

/// PL-25A1 USB Transport Factory
pub struct BridgeUsbFactory;

#[async_trait]
impl UsbTransportFactory for BridgeUsbFactory {
    fn supported_devices(&self) -> &[(u16, u16)] {
        &[(PL25A1_VID, PL25A1_PID)]
    }

    async fn create_transport(
        &self,
        device_info: DeviceInfo,
    ) -> Result<Box<dyn Transport + Send>> {
        let transport = BridgeUsbTransport::new(device_info).await?;
        Ok(Box::new(transport))
    }

    fn name(&self) -> &str {
        "PL-25A1"
    }
}