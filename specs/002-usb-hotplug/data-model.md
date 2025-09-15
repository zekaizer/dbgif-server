# Data Model: USB Hotplug Detection

## Core Entities

### HotplugEvent
**Purpose**: Represents a USB device hotplug event from the operating system
**Source**: nusb library event stream

**Fields**:
- `device_id: DeviceId` - Unique identifier for the USB device
- `event_type: HotplugEventType` - Connection or disconnection
- `device_info: Option<DeviceInfo>` - Device details (present for connections)
- `timestamp: Instant` - When the event occurred

**States**: N/A (immutable event)
**Validation**: Device ID must be valid, timestamp must be recent

### DetectionMechanism
**Purpose**: Tracks the current active detection method and capabilities
**Source**: System configuration and runtime detection

**Fields**:
- `current_mode: DetectionMode` - Active detection mechanism
- `hotplug_available: bool` - Whether hotplug is supported on this platform
- `polling_enabled: bool` - Whether polling fallback is active
- `last_hotplug_event: Option<Instant>` - Time of last successful hotplug event
- `fallback_interval: Duration` - Polling interval when used as backup

**States**:
- `HotplugOnly` - Pure event-driven detection
- `HotplugWithBackup` - Hotplug primary, polling backup
- `PollingOnly` - Fallback to original polling mode

**Validation**: At least one detection method must be enabled

### MigrationState
**Purpose**: Tracks the transition from polling to hotplug detection
**Source**: Runtime state management during feature activation

**Fields**:
- `phase: MigrationPhase` - Current migration step
- `connected_devices_before: HashSet<String>` - Device IDs before migration
- `connected_devices_after: HashSet<String>` - Device IDs after migration
- `migration_started: Instant` - When migration began
- `devices_lost: Vec<String>` - Any devices lost during migration

**States**:
- `NotStarted` - Using original polling
- `InProgress` - Transitioning detection mechanisms
- `Completed` - Hotplug active, polling disabled/minimized
- `RolledBack` - Reverted to polling due to issues

**Validation**: Device sets must be consistent before/after migration

### ResourceUsageMetrics
**Purpose**: Measurements of system resource consumption for performance validation
**Source**: Runtime monitoring of CPU, memory usage

**Fields**:
- `cpu_usage_percent: f64` - CPU utilization during idle periods
- `memory_bytes: u64` - Memory consumed by detection mechanism
- `polling_cycles: u64` - Number of polling iterations (for comparison)
- `hotplug_events: u64` - Number of hotplug events received
- `measurement_period: Duration` - Time window for measurements

**States**: N/A (measurement data)
**Validation**: CPU percentage must be 0.0-100.0, memory must be positive

## Enumerations

### HotplugEventType
```rust
pub enum HotplugEventType {
    Connected,    // Device plugged in
    Disconnected, // Device unplugged
}
```

### DetectionMode
```rust
pub enum DetectionMode {
    HotplugOnly,         // Pure event-driven
    HotplugWithBackup,   // Hotplug + periodic polling backup
    PollingOnly,         // Original polling mechanism
}
```

### MigrationPhase
```rust
pub enum MigrationPhase {
    NotStarted,
    CapturingInitialState,
    ActivatingHotplug,
    VerifyingDevices,
    DisablingPolling,
    Completed,
    RolledBack(String), // Reason for rollback
}
```

## Entity Relationships

### Event Flow
1. **nusb HotplugWatch** → generates → **HotplugEvent**
2. **HotplugEvent** → processed by → **DetectionMechanism**
3. **DetectionMechanism** → updates → **ResourceUsageMetrics**
4. **MigrationState** → coordinates → **DetectionMechanism** transitions

### State Transitions

#### DetectionMechanism Transitions
- `PollingOnly` → `HotplugWithBackup` (when hotplug becomes available)
- `HotplugWithBackup` → `HotplugOnly` (when polling can be safely disabled)
- `HotplugOnly` → `HotplugWithBackup` (when reliability issues detected)
- Any state → `PollingOnly` (emergency fallback)

#### MigrationState Transitions
- `NotStarted` → `CapturingInitialState` → `ActivatingHotplug` → `VerifyingDevices` → `DisablingPolling` → `Completed`
- Any state → `RolledBack` (on error or validation failure)

## Data Persistence
- **In-Memory Only**: All entities are runtime state, no persistent storage required
- **Lifetime**: Entities exist only while the DBGIF server is running
- **Recovery**: State is rebuilt from current USB device enumeration on startup

## Integration Points
- **Existing DiscoveryEvent**: HotplugEvent maps to existing event types
- **TransportManager**: Receives processed events through existing interface
- **Configuration**: DetectionMode can be configured via server settings