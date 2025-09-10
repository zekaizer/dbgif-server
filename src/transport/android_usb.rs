use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use nusb::{DeviceInfo, Interface, MaybeFuture};
use nusb::transfer::{Bulk, In, Out};
use nusb::io::{EndpointRead, EndpointWrite};
// Control transfer imports not needed for Android USB
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, info};

use super::{ConnectionStatus, Transport, TransportType, UsbTransportFactory};
use crate::protocol::constants::MAXDATA;

// Android device VID/PID pairs for ADB interface
const SUPPORTED_ANDROID_DEVICES: &[(u16, u16)] = &[
    // Google devices
    (0x18d1, 0x4ee7), // Pixel series (ADB)
    (0x18d1, 0xd001), // Android Accessory
    (0x18d1, 0x4ee1), // Nexus 4/5/6 (ADB)
    (0x18d1, 0x4ee2), // Nexus 7 (2012)
    (0x18d1, 0x4ee4), // Nexus 10
    
    // Samsung devices  
    (0x04e8, 0x6860), // Galaxy series (ADB)
    (0x04e8, 0x6863), // Galaxy S3
    (0x04e8, 0x685d), // Galaxy Note series
    (0x04e8, 0x6866), // Galaxy Tab series
    
    // LG devices
    (0x1004, 0x631c), // G series phones
    (0x1004, 0x633e), // Optimus series
    (0x1004, 0x6344), // V series phones
    
    // HTC devices
    (0x0bb4, 0x0c02), // Dream/Magic/Hero
    (0x0bb4, 0x0c03), // One series
    (0x0bb4, 0x0ff9), // Desire series
    
    // Xiaomi devices
    (0x2717, 0xff40), // Mi/Redmi series
    (0x2717, 0xff48), // POCO series
    
    // Huawei devices
    (0x12d1, 0x1038), // P/Mate series
    (0x12d1, 0x1057), // Honor series
    
    // OnePlus devices
    (0x2a70, 0x4ee7), // OnePlus series
    
    // OPPO devices  
    (0x22d9, 0x2764), // Find/Reno series
    
    // Vivo devices
    (0x2d95, 0x600a), // V/X series
    
    // Motorola devices
    (0x22b8, 0x2e76), // Moto series
    (0x22b8, 0x2e82), // Edge series
    
    // Sony devices
    (0x054c, 0x0dcd), // Xperia series
    (0x054c, 0x0fce), // Xperia Z series
    
    // Asus devices
    (0x0b05, 0x4daf), // ZenFone series
    (0x0b05, 0x7772), // Transformer series
    
    // Lenovo devices
    (0x17ef, 0x7497), // Tab series
    (0x17ef, 0x741c), // ThinkPad Tablet
    
    // Acer devices
    (0x0502, 0x3325), // Iconia series
    (0x0502, 0x3341), // Liquid series
    
    // Dell devices
    (0x413c, 0xb06e), // Venue series
    
    // Generic Android devices
    (0x18d1, 0x0001), // Generic Android (fastboot)
    (0x18d1, 0x4ee0), // Generic ADB interface
];

// USB interface configuration
const USB_INTERFACE: u8 = 0;

// Endpoint addresses (typical for Android ADB)
const BULK_OUT_EP: u8 = 0x01;  // Host -> Device
const BULK_IN_EP: u8 = 0x81;   // Device -> Host

// Timeout for USB operations (kept for future use)
const USB_TIMEOUT: Duration = Duration::from_secs(5);

// USB packet size alignment (typical bulk endpoint max packet size)
const USB_BULK_MAX_PACKET_SIZE: usize = 64;

/// Android USB device transport
pub struct AndroidUsbTransport {
    device_id: String,
    interface: Arc<Mutex<Interface>>,
    // Pre-created endpoints for better performance
    bulk_out_writer: Arc<Mutex<EndpointWrite<Bulk>>>,
    bulk_in_reader: Arc<Mutex<EndpointRead<Bulk>>>,
    is_connected: bool,
    vendor_id: u16,
    product_id: u16,
}

impl AndroidUsbTransport {
    /// Create new Android USB transport for a device
    pub async fn new(device_info: DeviceInfo) -> Result<Self> {
        let vendor_id = device_info.vendor_id();
        let product_id = device_info.product_id();
        
        // Get device serial number or use bus:address as fallback
        let device_id = match device_info.serial_number() {
            Some(serial) => serial.to_string(),
            None => format!("android_{}:{}", device_info.bus_id(), device_info.device_address()),
        };

        // Open device and claim interface
        let device = device_info.open().wait().context("Failed to open USB device")?;
        let interface = device
            .claim_interface(USB_INTERFACE)
            .wait()
            .context("Failed to claim USB interface")?;

        // Create pre-configured endpoints for better performance
        let bulk_out_endpoint = interface.endpoint::<Bulk, Out>(BULK_OUT_EP)
            .context("Failed to open bulk out endpoint")?;
        let bulk_out_writer = bulk_out_endpoint.writer(MAXDATA);
        
        let bulk_in_endpoint = interface.endpoint::<Bulk, In>(BULK_IN_EP)
            .context("Failed to open bulk in endpoint")?;
        let bulk_in_reader = bulk_in_endpoint.reader(MAXDATA);

        info!(
            "Android USB device {} ({:04x}:{:04x}) initialized",
            device_id, vendor_id, product_id
        );

        Ok(Self {
            device_id,
            interface: Arc::new(Mutex::new(interface)),
            bulk_out_writer: Arc::new(Mutex::new(bulk_out_writer)),
            bulk_in_reader: Arc::new(Mutex::new(bulk_in_reader)),
            is_connected: true,
            vendor_id,
            product_id,
        })
    }

    /// Send data to USB device
    async fn send_bytes_internal(&mut self, data: &[u8]) -> Result<()> {
        let mut writer = self.bulk_out_writer.lock().await;

        // Send all data at once
        if let Err(e) = writer.write_all(data).await {
            self.is_connected = false;
            return Err(anyhow::anyhow!("USB send failed: {}", e));
        }

        debug!(
            "Sent {} bytes to Android device {}",
            data.len(), self.device_id
        );
        Ok(())
    }

    /// Receive data from USB device with specified buffer size
    /// Returns (data, actual_received_size)
    async fn receive_bytes_internal(&mut self, buffer_size: usize) -> Result<(Vec<u8>, usize)> {

        // Align buffer size down to USB packet boundary for optimal transfer
        let aligned_size = (buffer_size / USB_BULK_MAX_PACKET_SIZE) * USB_BULK_MAX_PACKET_SIZE;
        let final_size = if aligned_size == 0 { 
            USB_BULK_MAX_PACKET_SIZE 
        } else { 
            aligned_size.min(MAXDATA) 
        };
        
        let mut reader = self.bulk_in_reader.lock().await;
        
        let mut buffer = vec![0u8; final_size];
        let actual_size = match reader.read(&mut buffer).await {
            Ok(size) => size,
            Err(e) => {
                self.is_connected = false;
                return Err(anyhow::anyhow!("USB receive failed: {}", e));
            }
        };
        
        buffer.truncate(actual_size);
        let received_data = buffer;

        debug!(
            "Received {} bytes from Android device {} (requested: {}, aligned: {})",
            actual_size, self.device_id, buffer_size, final_size
        );
        
        Ok((received_data, actual_size))
    }

    /// Check connection status by attempting a simple operation
    async fn check_connection_status(&self) -> Result<ConnectionStatus> {
        // For Android devices, we assume they're ready if they're not flagged as disconnected
        // A more sophisticated implementation could send a PING message
        if self.is_connected {
            Ok(ConnectionStatus::Ready)
        } else {
            Ok(ConnectionStatus::Disconnected)
        }
    }

    /// Get device information string
    pub fn device_info(&self) -> String {
        format!(
            "Android USB Device {} ({:04x}:{:04x})",
            self.device_id, self.vendor_id, self.product_id
        )
    }
}

#[async_trait]
impl Transport for AndroidUsbTransport {
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        if !self.is_connected {
            bail!("Android device {} is not connected", self.device_id);
        }
        self.send_bytes_internal(data).await
    }

    async fn receive(&mut self, buffer_size: usize) -> Result<Vec<u8>> {
        if !self.is_connected {
            bail!("Android device {} is not connected", self.device_id);
        }
        let (data, _actual_size) = self.receive_bytes_internal(buffer_size).await?;
        Ok(data)
    }

    fn device_id(&self) -> &str {
        &self.device_id
    }

    async fn connect(&mut self) -> Result<ConnectionStatus> {
        // Android devices are ready immediately after USB enumeration
        // No additional handshake or logical connection establishment needed
        info!("Android device {} is ready for ADB protocol", self.device_id);
        Ok(ConnectionStatus::Ready)
    }

    fn transport_type(&self) -> TransportType {
        TransportType::AndroidUsb
    }

    async fn is_connected(&self) -> bool {
        self.is_connected
    }

    async fn disconnect(&mut self) -> Result<()> {
        if self.is_connected {
            info!("Disconnecting Android device {}", self.device_id);
            self.is_connected = false;
        }
        Ok(())
    }
}

/// Android USB Transport Factory
pub struct AndroidUsbFactory;

#[async_trait]
impl UsbTransportFactory for AndroidUsbFactory {
    fn supported_devices(&self) -> &[(u16, u16)] {
        SUPPORTED_ANDROID_DEVICES
    }

    async fn create_transport(
        &self,
        device_info: DeviceInfo,
    ) -> Result<Box<dyn Transport + Send>> {
        let transport = AndroidUsbTransport::new(device_info).await?;
        Ok(Box::new(transport))
    }

    fn name(&self) -> &str {
        "Android USB"
    }
}