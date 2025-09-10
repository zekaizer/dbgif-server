use anyhow::Result;
use futures::StreamExt;
use nusb::{DeviceId, DeviceInfo};
use nusb::hotplug::HotplugEvent;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use super::{get_device_info, TransportManager, UsbDeviceInfo, UsbTransportFactory};

/// USB device identifier (bus_number, address)
type DeviceKey = (u8, u8);

/// Device snapshot for ownership-safe processing
#[derive(Clone, Debug)]
struct DeviceSnapshot {
    pub info: UsbDeviceInfo,
    pub factory_name: Option<String>,
    pub validation_passed: bool,
}

/// Changes detected during polling
struct DeviceChanges {
    pub added: Vec<DeviceSnapshot>,
    pub removed: Vec<DeviceKey>,
}

/// Polling state management
struct PollingState {
    known_devices: HashMap<DeviceKey, DeviceSnapshot>,
    last_scan: Instant,
}

impl PollingState {
    fn new() -> Self {
        Self {
            known_devices: HashMap::new(),
            last_scan: Instant::now(),
        }
    }

    fn detect_changes(&self, current_snapshots: &[DeviceSnapshot]) -> DeviceChanges {
        let current_keys: HashSet<DeviceKey> = current_snapshots
            .iter()
            .map(|s| (s.info.bus_number, s.info.address))
            .collect();
        
        let known_keys: HashSet<DeviceKey> = self.known_devices.keys().cloned().collect();

        // Find added devices
        let added: Vec<DeviceSnapshot> = current_snapshots
            .iter()
            .filter(|s| {
                let key = (s.info.bus_number, s.info.address);
                !known_keys.contains(&key)
            })
            .cloned()
            .collect();

        // Find removed devices
        let removed: Vec<DeviceKey> = known_keys
            .difference(&current_keys)
            .cloned()
            .collect();

        DeviceChanges { added, removed }
    }

    fn update(&mut self, snapshots: Vec<DeviceSnapshot>) {
        self.known_devices.clear();
        for snapshot in snapshots {
            let key = (snapshot.info.bus_number, snapshot.info.address);
            self.known_devices.insert(key, snapshot);
        }
        self.last_scan = Instant::now();
    }
}

/// USB hotplug monitor for automatic transport creation
pub struct UsbMonitor {
    factories: Arc<Vec<Arc<dyn UsbTransportFactory>>>,
    transport_manager: Arc<TransportManager>,
    device_map: Arc<RwLock<HashMap<DeviceKey, String>>>, // (bus, addr) -> device_id
    // Hotplug mode fields
    hotplug_handle: Option<JoinHandle<()>>,
    // Polling mode fields
    polling_handle: Option<JoinHandle<()>>,
    polling_interval: Duration,
    // Shutdown mechanism
    shutdown_flag: Arc<AtomicBool>,
}

impl UsbMonitor {
    /// Create new USB monitor
    pub fn new(transport_manager: Arc<TransportManager>) -> Result<Self> {
        Ok(Self {
            factories: Arc::new(Vec::new()),
            transport_manager,
            device_map: Arc::new(RwLock::new(HashMap::new())),
            // Hotplug mode fields
            hotplug_handle: None,
            // Polling mode fields
            polling_handle: None,
            polling_interval: Duration::from_secs(5), // Default 5 seconds
            // Shutdown mechanism
            shutdown_flag: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Register a transport factory
    pub fn register_factory(&mut self, factory: Arc<dyn UsbTransportFactory>) {
        info!("Registered USB factory: {}", factory.name());
        Arc::make_mut(&mut self.factories).push(factory);
    }

    /// Start USB hotplug monitoring with real-time callbacks
    pub async fn start_monitoring(&mut self) -> Result<()> {
        // Check if already running
        if self.hotplug_handle.is_some() {
            warn!("USB monitoring is already running");
            return Ok(());
        }

        // Try hotplug mode first, fallback to polling
        match self.start_hotplug_mode().await {
            Ok(()) => {
                info!("USB hotplug monitoring started with real-time callbacks");
                Ok(())
            }
            Err(e) => {
                warn!("USB hotplug failed ({}), falling back to polling mode", e);
                self.start_polling_mode().await
            }
        }
    }

    /// Start hotplug monitoring using nusb::watch_devices
    async fn start_hotplug_mode(&mut self) -> Result<()> {
        let factories = Arc::clone(&self.factories);
        let transport_manager = Arc::clone(&self.transport_manager);
        let device_map = Arc::clone(&self.device_map);
        let shutdown_flag = Arc::clone(&self.shutdown_flag);

        let handle = tokio::spawn(async move {
            info!("USB hotplug monitoring task started");
            
            // Initialize with current devices
            if let Err(e) = Self::scan_and_process_devices(&factories, &transport_manager, &device_map).await {
                error!("Failed to scan initial devices: {}", e);
            }

            // Watch for device changes
            match nusb::watch_devices() {
                Ok(mut stream) => {
                    while let Some(event) = stream.next().await {
                        if shutdown_flag.load(Ordering::Relaxed) {
                            info!("USB hotplug monitoring received shutdown signal");
                            break;
                        }

                        match event {
                            HotplugEvent::Connected(device_info) => {
                                debug!("USB device arrived: {:04x}:{:04x}", device_info.vendor_id(), device_info.product_id());
                                if let Err(e) = Self::handle_device_arrived(&factories, &transport_manager, &device_map, device_info).await {
                                    error!("Failed to handle device arrival: {}", e);
                                }
                            }
                            HotplugEvent::Disconnected(device_id) => {
                                debug!("USB device left: {:?}", device_id);
                                if let Err(e) = Self::handle_device_left(&transport_manager, &device_map, device_id).await {
                                    error!("Failed to handle device removal: {}", e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to watch USB devices: {}", e);
                    return;
                }
            }
            
            info!("USB hotplug monitoring task ended");
        });

        self.hotplug_handle = Some(handle);
        Ok(())
    }

    /// Handle device arrival event
    async fn handle_device_arrived(
        factories: &Arc<Vec<Arc<dyn UsbTransportFactory>>>,
        transport_manager: &Arc<TransportManager>,
        device_map: &Arc<RwLock<HashMap<DeviceKey, String>>>,
        device_info: DeviceInfo,
    ) -> Result<()> {
        let udev_info = get_device_info(&device_info)?;
        let device_key = (udev_info.bus_number, udev_info.address);

        // Check if device is already registered
        if device_map.read().await.contains_key(&device_key) {
            debug!("Device {}:{} already registered, skipping", device_key.0, device_key.1);
            return Ok(());
        }

        // Check if device is supported by any factory
        for factory in factories.iter() {
            if factory.supported_devices().contains(&(udev_info.vendor_id, udev_info.product_id)) {
                info!(
                    "Creating transport for device {:04x}:{:04x} using factory {}",
                    udev_info.vendor_id, udev_info.product_id, factory.name()
                );

                match factory.create_transport(device_info.clone()).await {
                    Ok(transport) => {
                        let device_id = transport.device_id().to_string();
                        
                        // Register the device mapping
                        device_map.write().await.insert(device_key, device_id.clone());
                        
                        // Add transport to manager
                        transport_manager.add_transport(transport).await;
                        
                        info!("Successfully added transport for device {}", device_id);
                        return Ok(());
                    }
                    Err(e) => {
                        error!("Failed to create transport for device: {}", e);
                        continue;
                    }
                }
            }
        }

        debug!(
            "No factory found for device {:04x}:{:04x}",
            device_info.vendor_id(), device_info.product_id()
        );
        Ok(())
    }

    /// Handle device removal event
    async fn handle_device_left(
        transport_manager: &Arc<TransportManager>,
        device_map: &Arc<RwLock<HashMap<DeviceKey, String>>>,
        device_id: DeviceId,
    ) -> Result<()> {
        // Try to find device by device_id mapping (this is approximate)
        let mut map = device_map.write().await;
        let mut found_key = None;

        // Find the device key that might match this device_id
        for (key, mapped_id) in map.iter() {
            // This is a heuristic - we can't perfectly match device_id to our key
            // In practice, we might need to enhance this mapping
            if mapped_id.contains(&format!("{}:{}", key.0, key.1)) {
                found_key = Some(*key);
                break;
            }
        }

        if let Some(key) = found_key {
            if let Some(device_id_str) = map.remove(&key) {
                info!("Removing transport for device {}", device_id_str);
                transport_manager.remove_transport(&device_id_str).await;
            }
        }

        Ok(())
    }

    /// Scan devices and process them
    async fn scan_and_process_devices(
        factories: &Arc<Vec<Arc<dyn UsbTransportFactory>>>,
        transport_manager: &Arc<TransportManager>,
        device_map: &Arc<RwLock<HashMap<DeviceKey, String>>>,
    ) -> Result<()> {
        let devices = nusb::list_devices()?;
        
        for device_info in devices {
            let udev_info = get_device_info(&device_info)?;
            let device_key = (udev_info.bus_number, udev_info.address);

            // Skip if already processed
            if device_map.read().await.contains_key(&device_key) {
                continue;
            }

            // Check if supported
            for factory in factories.iter() {
                if factory.supported_devices().contains(&(udev_info.vendor_id, udev_info.product_id)) {
                    debug!(
                        "Found supported device {:04x}:{:04x} for factory {}",
                        udev_info.vendor_id, udev_info.product_id, factory.name()
                    );

                    match factory.create_transport(device_info.clone()).await {
                        Ok(transport) => {
                            let device_id = transport.device_id().to_string();
                            
                            // Register the device mapping
                            device_map.write().await.insert(device_key, device_id.clone());
                            
                            // Add transport to manager
                            transport_manager.add_transport(transport).await;
                            
                            info!("Added transport for device {}", device_id);
                            break; // Found a factory, stop trying others
                        }
                        Err(e) => {
                            warn!("Failed to create transport: {}", e);
                            continue;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Start polling mode for platforms without hotplug support
    async fn start_polling_mode(&mut self) -> Result<()> {
        let factories = Arc::clone(&self.factories);
        let transport_manager = Arc::clone(&self.transport_manager);
        let device_map = Arc::clone(&self.device_map);
        let shutdown_flag = Arc::clone(&self.shutdown_flag);
        let polling_interval = self.polling_interval;

        let handle = tokio::spawn(async move {
            info!("USB polling mode started (interval: {:?})", polling_interval);
            let mut polling_state = PollingState::new();

            loop {
                if shutdown_flag.load(Ordering::Relaxed) {
                    info!("USB polling received shutdown signal");
                    break;
                }

                // Scan current devices
                match Self::scan_devices_blocking(&factories).await {
                    Ok(current_snapshots) => {
                        let changes = polling_state.detect_changes(&current_snapshots);

                        // Process added devices
                        for snapshot in &changes.added {
                            info!(
                                "Detected new device: {:04x}:{:04x} ({}:{})",
                                snapshot.info.vendor_id,
                                snapshot.info.product_id,
                                snapshot.info.bus_number,
                                snapshot.info.address
                            );

                            if let Some(factory_name) = &snapshot.factory_name {
                                // Find the factory and create transport
                                for factory in factories.iter() {
                                    if factory.name() == factory_name {
                                        // Re-enumerate to get Device object
                                        if let Ok(devices) = nusb::list_devices() {
                                            for device_info in devices {
                                                if let Ok(info) = get_device_info(&device_info) {
                                                    if info.bus_number == snapshot.info.bus_number &&
                                                       info.address == snapshot.info.address {
                                                        match factory.create_transport(device_info.clone()).await {
                                                            Ok(transport) => {
                                                                let device_id = transport.device_id().to_string();
                                                                let device_key = (info.bus_number, info.address);
                                                                
                                                                device_map.write().await.insert(device_key, device_id.clone());
                                                                transport_manager.add_transport(transport).await;
                                                                info!("Added transport for device {}", device_id);
                                                                break;
                                                            }
                                                            Err(e) => {
                                                                error!("Failed to create transport: {}", e);
                                                            }
                                                        }
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        break;
                                    }
                                }
                            }
                        }

                        // Process removed devices
                        for device_key in &changes.removed {
                            let mut map = device_map.write().await;
                            if let Some(device_id) = map.remove(device_key) {
                                info!("Detected removed device: {}:{}", device_key.0, device_key.1);
                                transport_manager.remove_transport(&device_id).await;
                            }
                        }

                        // Update polling state
                        polling_state.update(current_snapshots);
                    }
                    Err(e) => {
                        error!("Failed to scan USB devices: {}", e);
                    }
                }

                // Wait for next polling interval
                tokio::time::sleep(polling_interval).await;
            }

            info!("USB polling task ended");
        });

        self.polling_handle = Some(handle);
        info!("USB polling mode started");
        Ok(())
    }

    /// Scan devices in blocking context for ownership safety
    async fn scan_devices_blocking(
        factories: &Arc<Vec<Arc<dyn UsbTransportFactory>>>
    ) -> Result<Vec<DeviceSnapshot>> {
        tokio::task::spawn_blocking({
            let factories = Arc::clone(factories);
            move || -> Result<Vec<DeviceSnapshot>> {
                let devices = nusb::list_devices()?;
                let mut snapshots = Vec::new();

                for device in devices {
                    match get_device_info(&device) {
                        Ok(info) => {
                            let mut factory_name = None;
                            let mut validation_passed = false;

                            // Check if device is supported by any factory
                            for factory in factories.iter() {
                                if factory.supported_devices().contains(&(info.vendor_id, info.product_id)) {
                                    factory_name = Some(factory.name().to_string());
                                    validation_passed = true;
                                    break;
                                }
                            }

                            snapshots.push(DeviceSnapshot {
                                info,
                                factory_name,
                                validation_passed,
                            });
                        }
                        Err(e) => {
                            warn!("Failed to get device info: {}", e);
                        }
                    }
                }

                Ok(snapshots)
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("Task join error: {:?}", e))?
    }

    /// Stop USB monitoring
    pub async fn stop_monitoring(&mut self) -> Result<()> {
        info!("Stopping USB monitoring");

        // Set shutdown flag
        self.shutdown_flag.store(true, Ordering::Relaxed);

        // Stop hotplug task
        if let Some(handle) = self.hotplug_handle.take() {
            handle.abort();
            let _ = handle.await;
        }

        // Stop polling task
        if let Some(handle) = self.polling_handle.take() {
            handle.abort();
            let _ = handle.await;
        }

        // Clear device map
        self.device_map.write().await.clear();

        // Reset shutdown flag for future use
        self.shutdown_flag.store(false, Ordering::Relaxed);

        info!("USB monitoring stopped");
        Ok(())
    }

    /// Set polling interval (for polling mode)
    pub fn set_polling_interval(&mut self, interval: Duration) {
        self.polling_interval = interval;
        info!("USB polling interval set to {:?}", interval);
    }

    /// Get current device count
    pub async fn device_count(&self) -> usize {
        self.device_map.read().await.len()
    }

    /// List currently monitored devices
    pub async fn list_devices(&self) -> Vec<(DeviceKey, String)> {
        self.device_map
            .read()
            .await
            .iter()
            .map(|(key, device_id)| (*key, device_id.clone()))
            .collect()
    }
}

impl Drop for UsbMonitor {
    fn drop(&mut self) {
        // Ensure monitoring is stopped when dropped
        self.shutdown_flag.store(true, Ordering::Relaxed);
    }
}