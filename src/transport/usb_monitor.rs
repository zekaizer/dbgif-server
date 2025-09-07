use anyhow::Result;
use rusb::{Context, Device, GlobalContext, Hotplug, HotplugBuilder, Registration, UsbContext};
use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use super::{get_device_info, Transport, TransportManager, UsbDeviceInfo, UsbTransportFactory};

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
        let removed: Vec<DeviceKey> = known_keys.difference(&current_keys).cloned().collect();

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
    context: Option<Context>,
    registration: Option<Registration<Context>>,
    monitor_handle: Option<JoinHandle<()>>,
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
            context: None,
            registration: None,
            monitor_handle: None,
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
        if self.context.is_some() {
            warn!("USB monitoring is already running");
            return Ok(());
        }

        // Check if hotplug is supported on this platform
        if !rusb::has_hotplug() {
            warn!("USB hotplug not supported on this platform, using polling fallback");
            return self.start_polling_mode().await;
        }

        // Create USB context
        let context =
            Context::new().map_err(|e| anyhow::anyhow!("Failed to create USB context: {}", e))?;

        // Create hotplug handler
        let handler = UsbHotplugHandler::new(
            Arc::clone(&self.factories),
            Arc::clone(&self.transport_manager),
            Arc::clone(&self.device_map),
        );

        // Register hotplug callbacks
        let registration = HotplugBuilder::new()
            .enumerate(true) // Also process existing devices
            .register(&context, Box::new(handler))
            .map_err(|e| anyhow::anyhow!("Failed to register hotplug callback: {}", e))?;

        // Start background task to handle USB events
        let ctx_clone = context.clone();
        let shutdown_flag_clone = Arc::clone(&self.shutdown_flag);
        let handle = tokio::task::spawn_blocking(move || {
            info!("USB event handling task started");
            loop {
                // Check shutdown flag before processing events
                if shutdown_flag_clone.load(Ordering::Relaxed) {
                    info!("USB event handling task received shutdown signal");
                    break;
                }

                match ctx_clone.handle_events(Some(Duration::from_millis(100))) {
                    Ok(_) => {
                        // Continue processing events
                    }
                    Err(rusb::Error::Interrupted) => {
                        info!("USB event handling interrupted");
                        break;
                    }
                    Err(e) => {
                        error!("USB event handling error: {}", e);
                        // Continue despite errors, but add a small delay
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
            info!("USB event handling task ended");
        });

        // Store state
        self.context = Some(context);
        self.registration = Some(registration);
        self.monitor_handle = Some(handle);

        info!("USB hotplug monitoring started with real-time callbacks");
        Ok(())
    }

    /// Scan devices in blocking context for ownership safety
    async fn scan_devices_blocking(
        factories: &Arc<Vec<Arc<dyn UsbTransportFactory>>>,
    ) -> Result<Vec<DeviceSnapshot>> {
        tokio::task::spawn_blocking({
            let factories = Arc::clone(factories);
            move || -> Result<Vec<DeviceSnapshot>> {
                let context = rusb::GlobalContext {};
                let devices = context.devices()?;
                let mut snapshots = Vec::new();

                for device in devices.iter() {
                    if let Ok(info) = get_device_info(&device) {
                        let mut snapshot = DeviceSnapshot {
                            info: info.clone(),
                            factory_name: None,
                            validation_passed: false,
                        };

                        // Check factory matching and validation
                        for factory in factories.iter() {
                            if factory.matches(&info) {
                                snapshot.factory_name = Some(factory.name().to_string());
                                snapshot.validation_passed =
                                    factory.validate_device(&device).is_ok();
                                break; // Only use first matching factory
                            }
                        }

                        snapshots.push(snapshot);
                    }
                }

                Ok(snapshots)
            }
        })
        .await?
    }

    /// Handle new device detection
    async fn handle_new_device(
        snapshot: DeviceSnapshot,
        factories: &Arc<Vec<Arc<dyn UsbTransportFactory>>>,
        transport_manager: &Arc<TransportManager>,
        device_map: &Arc<RwLock<HashMap<DeviceKey, String>>>,
    ) -> Result<()> {
        if let Some(factory_name) = &snapshot.factory_name {
            if snapshot.validation_passed {
                debug!(
                    "Polling detected new device: {} (matched by {})",
                    snapshot.info, factory_name
                );

                // Create transport by re-enumerating devices
                let transport =
                    Self::create_transport_for_device(&snapshot.info, factory_name, factories)
                        .await?;

                if let Some(transport) = transport {
                    let device_id = transport.device_id().to_string();
                    let key = (snapshot.info.bus_number, snapshot.info.address);

                    // Update device map
                    {
                        let mut device_map_lock = device_map.write().await;
                        device_map_lock.insert(key, device_id.clone());
                    }

                    // Add to transport manager
                    transport_manager
                        .add_transport_auto_debug(transport)
                        .await?;

                    info!("Transport created via polling: {}", device_id);
                }
            }
        }

        Ok(())
    }

    /// Handle device removal
    async fn handle_removed_device(
        device_key: DeviceKey,
        transport_manager: &Arc<TransportManager>,
        device_map: &Arc<RwLock<HashMap<DeviceKey, String>>>,
    ) -> Result<()> {
        let device_id = {
            let mut device_map_lock = device_map.write().await;
            device_map_lock.remove(&device_key)
        };

        if let Some(device_id) = device_id {
            info!(
                "USB device disconnected via polling: {} ({}:{})",
                device_id, device_key.0, device_key.1
            );

            // Handle disconnect in transport manager
            transport_manager.handle_usb_disconnect(&device_id).await?;
        } else {
            debug!(
                "Unknown device disconnected at {}:{}",
                device_key.0, device_key.1
            );
        }

        Ok(())
    }

    /// Create transport for device by re-enumeration
    async fn create_transport_for_device(
        device_info: &UsbDeviceInfo,
        factory_name: &str,
        factories: &Arc<Vec<Arc<dyn UsbTransportFactory>>>,
    ) -> Result<Option<Box<dyn Transport + Send>>> {
        tokio::task::spawn_blocking({
            let device_info = device_info.clone();
            let factory_name = factory_name.to_string();
            let factories = Arc::clone(factories);

            move || -> Result<Option<Box<dyn Transport + Send>>> {
                // Re-enumerate devices to find the matching one
                let context = rusb::GlobalContext {};
                let devices = context.devices()?;

                for device in devices.iter() {
                    if let Ok(info) = get_device_info(&device) {
                        if info.bus_number == device_info.bus_number
                            && info.address == device_info.address
                            && info.vendor_id == device_info.vendor_id
                            && info.product_id == device_info.product_id
                        {
                            // Find matching factory
                            for factory in factories.iter() {
                                if factory.name() == factory_name && factory.matches(&info) {
                                    // Create transport synchronously using block_on
                                    let transport = futures::executor::block_on(
                                        factory.create_transport(device),
                                    )?;
                                    return Ok(Some(transport));
                                }
                            }
                        }
                    }
                }

                // Device not found during re-enumeration
                warn!("Device not found during re-enumeration: {}", device_info);
                Ok(None)
            }
        })
        .await?
    }

    /// Start polling mode for platforms without hotplug support
    async fn start_polling_mode(&mut self) -> Result<()> {
        if self.polling_handle.is_some() {
            return Ok(()); // Already running
        }

        let factories = Arc::clone(&self.factories);
        let transport_manager = Arc::clone(&self.transport_manager);
        let device_map = Arc::clone(&self.device_map);
        let shutdown_flag_clone = Arc::clone(&self.shutdown_flag);
        let interval = self.polling_interval;

        info!(
            "Starting USB polling fallback mode with {}s interval",
            interval.as_secs()
        );

        let handle = tokio::spawn(async move {
            let mut state = PollingState::new();

            loop {
                // Check shutdown flag before processing
                if shutdown_flag_clone.load(Ordering::Relaxed) {
                    info!("USB polling task received shutdown signal");
                    break;
                }
                // 1. Scan devices in blocking context
                let snapshots = match Self::scan_devices_blocking(&factories).await {
                    Ok(snapshots) => snapshots,
                    Err(e) => {
                        warn!("Failed to scan USB devices during polling: {}", e);
                        tokio::time::sleep(interval).await;
                        continue;
                    }
                };

                // 2. Detect changes
                let changes = state.detect_changes(&snapshots);

                // 3. Handle new devices
                for new_device in changes.added {
                    if let Err(e) = Self::handle_new_device(
                        new_device,
                        &factories,
                        &transport_manager,
                        &device_map,
                    )
                    .await
                    {
                        warn!("Failed to handle new device: {}", e);
                    }
                }

                // 4. Handle removed devices
                for removed_key in changes.removed {
                    if let Err(e) =
                        Self::handle_removed_device(removed_key, &transport_manager, &device_map)
                            .await
                    {
                        warn!("Failed to handle removed device: {}", e);
                    }
                }

                // 5. Update state
                state.update(snapshots);

                // 6. Sleep until next polling cycle
                tokio::time::sleep(interval).await;
            }
        });

        self.polling_handle = Some(handle);
        Ok(())
    }

    /// Stop USB hotplug monitoring
    pub async fn stop_monitoring(&mut self) -> Result<()> {
        // Set shutdown flag first
        self.shutdown_flag.store(true, Ordering::Relaxed);

        // Stop polling task if running
        if let Some(handle) = self.polling_handle.take() {
            // Wait for graceful shutdown first
            match tokio::time::timeout(Duration::from_secs(3), handle).await {
                Ok(Ok(_)) => {
                    info!("USB polling task terminated gracefully");
                }
                Ok(Err(e)) => {
                    warn!("USB polling task finished with error: {}", e);
                }
                Err(_) => {
                    warn!("USB polling task did not terminate in time, it was likely aborted");
                }
            }
        }

        // Stop background event handling task
        if let Some(handle) = self.monitor_handle.take() {
            // Wait for graceful shutdown first
            match tokio::time::timeout(Duration::from_secs(3), handle).await {
                Ok(Ok(_)) => {
                    info!("USB event handling task terminated gracefully");
                }
                Ok(Err(e)) => {
                    warn!("USB event handling task finished with error: {}", e);
                }
                Err(_) => {
                    warn!(
                        "USB event handling task did not terminate in time, it was likely aborted"
                    );
                }
            }
        }

        // Unregister hotplug callback
        if let (Some(context), Some(registration)) = (self.context.take(), self.registration.take())
        {
            context.unregister_callback(registration);
            info!("USB hotplug callback unregistered");
        }

        info!("USB monitoring stopped");
        Ok(())
    }

    /// Get list of currently tracked USB devices
    pub async fn get_tracked_devices(&self) -> HashMap<DeviceKey, String> {
        self.device_map.read().await.clone()
    }

    /// Manual device scan for existing devices
    pub async fn scan_existing_devices(&self) -> Result<usize> {
        info!("Scanning for existing USB devices...");

        // Use global context for device enumeration
        let devices = match rusb::devices() {
            Ok(devices) => devices,
            Err(e) => {
                warn!("Failed to enumerate USB devices: {}", e);
                return Ok(0);
            }
        };

        let mut added_count = 0;

        for device in devices.iter() {
            if let Ok(info) = get_device_info(&device) {
                for factory in self.factories.iter() {
                    if factory.matches(&info) {
                        match self.handle_device_arrived(device, factory.as_ref()).await {
                            Ok(true) => {
                                added_count += 1;
                                break; // Device handled, don't try other factories
                            }
                            Ok(false) => {
                                // Device already exists, skip
                                break;
                            }
                            Err(e) => {
                                warn!("Failed to add existing device {}: {}", info, e);
                                break;
                            }
                        }
                    }
                }
            }
        }

        info!("Device scan complete: {} devices added", added_count);
        Ok(added_count)
    }

    /// Handle device arrival (returns true if new device added)
    async fn handle_device_arrived(
        &self,
        device: Device<GlobalContext>,
        factory: &dyn UsbTransportFactory,
    ) -> Result<bool> {
        let info = get_device_info(&device)?;
        let device_key = (info.bus_number, info.address);

        // Check if device is already tracked
        {
            let device_map = self.device_map.read().await;
            if device_map.contains_key(&device_key) {
                debug!("Device {} already tracked", info);
                return Ok(false);
            }
        }

        info!("USB device found: {} (matched by {})", info, factory.name());

        // Validate device before creating transport
        if let Err(e) = factory.validate_device(&device) {
            warn!("Device validation failed for {}: {}", info, e);
            return Err(e);
        }

        // Create transport
        let transport = factory.create_transport(device).await?;
        let device_id = transport.device_id().to_string();

        // Add to device map
        {
            let mut device_map = self.device_map.write().await;
            device_map.insert(device_key, device_id.clone());
        }

        // Add to transport manager
        self.transport_manager
            .add_transport_auto_debug(transport)
            .await?;

        info!("Transport created and added: {}", device_id);
        Ok(true)
    }

    /// Handle device removal (for future hotplug implementation)
    #[allow(dead_code)]
    async fn handle_device_left(&self, bus: u8, address: u8) -> Result<()> {
        let device_key = (bus, address);

        let device_id = {
            let mut device_map = self.device_map.write().await;
            device_map.remove(&device_key)
        };

        if let Some(device_id) = device_id {
            info!(
                "USB device disconnected: {} ({}:{})",
                device_id, bus, address
            );

            // Handle disconnect in transport manager
            self.transport_manager
                .handle_usb_disconnect(&device_id)
                .await?;
        } else {
            debug!("Unknown device disconnected at {}:{}", bus, address);
        }

        Ok(())
    }
}

impl Drop for UsbMonitor {
    fn drop(&mut self) {
        // Clean up resources when UsbMonitor is dropped
        if let Some(handle) = self.polling_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.monitor_handle.take() {
            handle.abort();
        }

        if let (Some(context), Some(registration)) = (self.context.take(), self.registration.take())
        {
            context.unregister_callback(registration);
        }

        debug!("UsbMonitor dropped and cleaned up");
    }
}

/// USB hotplug handler that implements the rusb::Hotplug trait
#[derive(Clone)]
struct UsbHotplugHandler {
    factories: Arc<Vec<Arc<dyn UsbTransportFactory>>>,
    transport_manager: Arc<TransportManager>,
    device_map: Arc<RwLock<HashMap<DeviceKey, String>>>,
}

impl UsbHotplugHandler {
    fn new(
        factories: Arc<Vec<Arc<dyn UsbTransportFactory>>>,
        transport_manager: Arc<TransportManager>,
        device_map: Arc<RwLock<HashMap<DeviceKey, String>>>,
    ) -> Self {
        Self {
            factories,
            transport_manager,
            device_map,
        }
    }

    /// Handle device arrival event asynchronously
    async fn handle_device_arrived_async(&self, device: Device<Context>) -> Result<()> {
        // Convert Context device to GlobalContext device for compatibility
        let global_device = match self.context_device_to_global(device).await {
            Ok(dev) => dev,
            Err(e) => {
                warn!("Failed to convert device context: {}", e);
                return Err(e);
            }
        };

        let info = get_device_info(&global_device)?;
        let device_key = (info.bus_number, info.address);

        // Check if device is already tracked
        {
            let device_map = self.device_map.read().await;
            if device_map.contains_key(&device_key) {
                debug!("Device {} already tracked", info);
                return Ok(());
            }
        }

        // Find matching factory
        for factory in self.factories.iter() {
            if factory.matches(&info) {
                info!(
                    "USB device connected: {} (matched by {})",
                    info,
                    factory.name()
                );

                // Validate device
                if let Err(e) = factory.validate_device(&global_device) {
                    warn!("Device validation failed for {}: {}", info, e);
                    continue;
                }

                // Create transport
                match factory.create_transport(global_device).await {
                    Ok(transport) => {
                        let device_id = transport.device_id().to_string();

                        // Add to device map
                        {
                            let mut device_map = self.device_map.write().await;
                            device_map.insert(device_key, device_id.clone());
                        }

                        // Add to transport manager
                        match self
                            .transport_manager
                            .add_transport_auto_debug(transport)
                            .await
                        {
                            Ok(_) => {
                                info!("Transport created and added: {}", device_id);
                                return Ok(());
                            }
                            Err(e) => {
                                error!("Failed to add transport to manager: {}", e);
                                // Remove from device map on failure
                                self.device_map.write().await.remove(&device_key);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to create transport for {}: {}", info, e);
                    }
                }
                break; // Only try first matching factory
            }
        }

        Ok(())
    }

    /// Handle device removal event asynchronously  
    async fn handle_device_left_async(&self, bus: u8, address: u8) -> Result<()> {
        let device_key = (bus, address);

        let device_id = {
            let mut device_map = self.device_map.write().await;
            device_map.remove(&device_key)
        };

        if let Some(device_id) = device_id {
            info!(
                "USB device disconnected: {} ({}:{})",
                device_id, bus, address
            );

            // Handle disconnect in transport manager
            if let Err(e) = self
                .transport_manager
                .handle_usb_disconnect(&device_id)
                .await
            {
                error!("Failed to handle USB disconnect for {}: {}", device_id, e);
            }
        } else {
            debug!("Unknown device disconnected at {}:{}", bus, address);
        }

        Ok(())
    }

    /// Convert Context device to GlobalContext device
    /// This is a workaround for type compatibility between rusb Context types
    async fn context_device_to_global(
        &self,
        device: Device<Context>,
    ) -> Result<Device<GlobalContext>> {
        // Get device descriptor info
        let descriptor = device.device_descriptor()?;
        let vendor_id = descriptor.vendor_id();
        let product_id = descriptor.product_id();
        let bus = device.bus_number();
        let address = device.address();

        // Find the same device in global context
        let devices = rusb::devices()?;
        for global_device in devices.iter() {
            let global_desc = global_device.device_descriptor()?;
            if global_desc.vendor_id() == vendor_id
                && global_desc.product_id() == product_id
                && global_device.bus_number() == bus
                && global_device.address() == address
            {
                return Ok(global_device);
            }
        }

        Err(anyhow::anyhow!(
            "Could not find matching global device for {}:{} (VID={:04x} PID={:04x})",
            bus,
            address,
            vendor_id,
            product_id
        ))
    }
}

impl Hotplug<Context> for UsbHotplugHandler {
    fn device_arrived(&mut self, device: Device<Context>) {
        let handler = self.clone();
        tokio::spawn(async move {
            if let Err(e) = handler.handle_device_arrived_async(device).await {
                error!("Error handling device arrival: {}", e);
            }
        });
    }

    fn device_left(&mut self, device: Device<Context>) {
        let handler = self.clone();
        let bus = device.bus_number();
        let address = device.address();

        tokio::spawn(async move {
            if let Err(e) = handler.handle_device_left_async(bus, address).await {
                error!("Error handling device removal: {}", e);
            }
        });
    }
}

// TODO: Implement proper hotplug callbacks in future iterations
// The current implementation focuses on initial device scanning
// Full hotplug support would require:
// 1. Proper hotplug callback registration
// 2. Background monitoring task
// 3. Device arrival/removal event handling

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::AndroidUsbFactory;

    #[tokio::test]
    async fn test_usb_monitor_creation() {
        let transport_manager = Arc::new(TransportManager::new());
        let monitor = UsbMonitor::new(transport_manager);
        assert!(monitor.is_ok());
    }

    #[tokio::test]
    async fn test_factory_registration() {
        let transport_manager = Arc::new(TransportManager::new());
        let mut monitor = UsbMonitor::new(transport_manager).unwrap();

        let factory = Arc::new(AndroidUsbFactory::new());
        monitor.register_factory(factory);

        // Should have one factory registered
        assert_eq!(monitor.factories.len(), 1);
        assert_eq!(monitor.factories[0].name(), "AndroidUSB");
    }

    #[tokio::test]
    async fn test_device_map_empty() {
        let transport_manager = Arc::new(TransportManager::new());
        let monitor = UsbMonitor::new(transport_manager).unwrap();

        let tracked = monitor.get_tracked_devices().await;
        assert!(tracked.is_empty());
    }

    #[tokio::test]
    async fn test_start_stop_monitoring() {
        let transport_manager = Arc::new(TransportManager::new());
        let mut monitor = UsbMonitor::new(transport_manager).unwrap();

        // Should start without error (even if hotplug not supported)
        assert!(monitor.start_monitoring().await.is_ok());

        // Should stop without error
        assert!(monitor.stop_monitoring().await.is_ok());
    }

    #[tokio::test]
    async fn test_double_start_monitoring() {
        let transport_manager = Arc::new(TransportManager::new());
        let mut monitor = UsbMonitor::new(transport_manager).unwrap();

        // First start should succeed
        assert!(monitor.start_monitoring().await.is_ok());

        // Second start should succeed but warn (no error)
        assert!(monitor.start_monitoring().await.is_ok());

        // Cleanup
        assert!(monitor.stop_monitoring().await.is_ok());
    }

    #[test]
    fn test_hotplug_handler_creation() {
        let transport_manager = Arc::new(TransportManager::new());
        let factories = Arc::new(Vec::new());
        let device_map = Arc::new(RwLock::new(HashMap::new()));

        let _handler = UsbHotplugHandler::new(factories, transport_manager, device_map);
        // If we get here without panic, the handler was created successfully
    }

    #[test]
    fn test_device_snapshot_creation() {
        use crate::transport::UsbDeviceInfo;

        let info = UsbDeviceInfo {
            vendor_id: 0x18d1,
            product_id: 0x4ee7,
            bus_number: 1,
            address: 2,
            serial: Some("test123".to_string()),
        };

        let snapshot = DeviceSnapshot {
            info: info.clone(),
            factory_name: Some("TestFactory".to_string()),
            validation_passed: true,
        };

        assert_eq!(snapshot.info.vendor_id, 0x18d1);
        assert_eq!(snapshot.factory_name.as_ref().unwrap(), "TestFactory");
        assert!(snapshot.validation_passed);
    }

    #[test]
    fn test_polling_state_changes() {
        use crate::transport::UsbDeviceInfo;

        let mut state = PollingState::new();

        // Create test device snapshots
        let device1 = DeviceSnapshot {
            info: UsbDeviceInfo {
                vendor_id: 0x18d1,
                product_id: 0x4ee7,
                bus_number: 1,
                address: 2,
                serial: Some("device1".to_string()),
            },
            factory_name: Some("TestFactory".to_string()),
            validation_passed: true,
        };

        let device2 = DeviceSnapshot {
            info: UsbDeviceInfo {
                vendor_id: 0x18d1,
                product_id: 0x4ee7,
                bus_number: 1,
                address: 3,
                serial: Some("device2".to_string()),
            },
            factory_name: Some("TestFactory".to_string()),
            validation_passed: true,
        };

        // Initial state - no devices
        assert!(state.known_devices.is_empty());

        // Add first device
        state.update(vec![device1.clone()]);
        assert_eq!(state.known_devices.len(), 1);

        // Test change detection - add second device
        let changes = state.detect_changes(&[device1.clone(), device2.clone()]);
        assert_eq!(changes.added.len(), 1);
        assert_eq!(changes.removed.len(), 0);

        // Update state with both devices
        state.update(vec![device1.clone(), device2.clone()]);
        assert_eq!(state.known_devices.len(), 2);

        // Test change detection - remove first device
        let changes = state.detect_changes(&[device2.clone()]);
        assert_eq!(changes.added.len(), 0);
        assert_eq!(changes.removed.len(), 1);
    }

    #[tokio::test]
    async fn test_usb_monitor_with_polling_fields() {
        let transport_manager = Arc::new(TransportManager::new());
        let monitor = UsbMonitor::new(transport_manager).unwrap();

        // Check that polling fields are initialized correctly
        assert!(monitor.polling_handle.is_none());
        assert_eq!(monitor.polling_interval, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn test_start_monitoring_fallback_simulation() {
        let transport_manager = Arc::new(TransportManager::new());
        let mut monitor = UsbMonitor::new(transport_manager).unwrap();

        // Note: This test will succeed regardless of hotplug support
        // If hotplug is supported, it will use hotplug
        // If not supported, it will use polling fallback
        let result = monitor.start_monitoring().await;
        assert!(result.is_ok());

        // Clean up
        let stop_result = monitor.stop_monitoring().await;
        assert!(stop_result.is_ok());
    }
}
