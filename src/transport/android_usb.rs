use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use nusb::{DeviceInfo, Interface};
use nusb::transfer::RequestBuffer;
// Control transfer imports not needed for Android USB
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info};

use super::{ConnectionStatus, Transport, TransportType, UsbTransportFactory};
use crate::protocol::constants::MAXDATA;
use crate::protocol::message::Message;

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

// Timeout for USB operations
const USB_TIMEOUT: Duration = Duration::from_secs(5);

/// Android USB device transport
pub struct AndroidUsbTransport {
    device_id: String,
    interface: Arc<Mutex<Interface>>,
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
            None => format!("android_{}:{}", device_info.bus_number(), device_info.device_address()),
        };

        // Open device and claim interface
        let device = device_info.open().context("Failed to open USB device")?;
        let interface = device
            .claim_interface(USB_INTERFACE)
            .context("Failed to claim USB interface")?;

        info!(
            "Android USB device {} ({:04x}:{:04x}) initialized",
            device_id, vendor_id, product_id
        );

        Ok(Self {
            device_id,
            interface: Arc::new(Mutex::new(interface)),
            is_connected: true,
            vendor_id,
            product_id,
        })
    }

    /// Send message to USB device (header first, then data)
    async fn send_message_internal(&mut self, message: &Message) -> Result<()> {
        let serialized = message.serialize();

        // Split into header (24 bytes) and data
        let header = &serialized[..24];
        let data = &serialized[24..];

        let interface = self.interface.lock().await;

        // Send header first
        let completion = interface.bulk_out(BULK_OUT_EP, header.to_vec()).await;
        if let Err(e) = completion.status {
            self.is_connected = false;
            return Err(anyhow::anyhow!("Header send failed: {}", e));
        }
        
        // Note: nusb bulk transfers should send the complete buffer
        // No need to check partial sends as in rusb

        // Send data if present
        if !data.is_empty() {
            let completion = interface.bulk_out(BULK_OUT_EP, data.to_vec()).await;
            if let Err(e) = completion.status {
                self.is_connected = false;
                return Err(anyhow::anyhow!("Data send failed: {}", e));
            }
        }

        debug!(
            "Sent message to Android device {}: {:?}",
            self.device_id, message.command
        );
        Ok(())
    }

    /// Receive message from USB device
    async fn receive_message_internal(&mut self) -> Result<Message> {
        let interface = self.interface.lock().await;

        // Read header first (24 bytes)
        // RequestBuffer already imported at top
        let request_buffer = RequestBuffer::new(24);
        let completion = interface.bulk_in(BULK_IN_EP, request_buffer).await;
        
        if let Err(e) = completion.status {
            self.is_connected = false;
            return Err(anyhow::anyhow!("Header receive failed: {}", e));
        }
        
        if completion.data.len() != 24 {
            self.is_connected = false;
            bail!(
                "Invalid header size received: {} (expected 24)",
                completion.data.len()
            );
        }
        
        let header_buffer = completion.data;

        // Parse header to get data length
        use bytes::Buf;
        let mut header_cursor = std::io::Cursor::new(&header_buffer);
        let _command_raw = header_cursor.get_u32_le();
        let _arg0 = header_cursor.get_u32_le();
        let _arg1 = header_cursor.get_u32_le();
        let data_length = header_cursor.get_u32_le();

        let mut full_data = header_buffer;

        // Read data if present
        if data_length > 0 {
            if data_length as usize > MAXDATA {
                bail!("Data too large: {} bytes (max: {})", data_length, MAXDATA);
            }

            let request_buffer = RequestBuffer::new(data_length as usize);
            let completion = interface.bulk_in(BULK_IN_EP, request_buffer).await;
            
            if let Err(e) = completion.status {
                self.is_connected = false;
                return Err(anyhow::anyhow!("Data receive failed: {}", e));
            }
            
            if completion.data.len() != data_length as usize {
                self.is_connected = false;
                bail!(
                    "Invalid data size received: {} (expected {})",
                    completion.data.len(),
                    data_length
                );
            }
            
            full_data.extend_from_slice(&completion.data);
        }

        let message = Message::deserialize(full_data.as_slice()).context("Failed to deserialize message")?;
        debug!(
            "Received message from Android device {}: {:?}",
            self.device_id, message.command
        );
        Ok(message)
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
    async fn send_message(&mut self, message: &Message) -> Result<()> {
        if !self.is_connected {
            bail!("Android device {} is not connected", self.device_id);
        }
        self.send_message_internal(message).await
    }

    async fn receive_message(&mut self) -> Result<Message> {
        if !self.is_connected {
            bail!("Android device {} is not connected", self.device_id);
        }
        self.receive_message_internal().await
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