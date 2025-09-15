# API Contracts for USB Hotplug Detection

This directory contains the API contracts that define the interfaces for the USB hotplug detection feature. These contracts ensure that:

1. **Performance requirements** are embedded in the API
2. **Backward compatibility** is maintained with existing code
3. **Test scenarios** can be written against well-defined interfaces
4. **Implementation flexibility** is preserved while meeting requirements

## Contract Files

### hotplug_events.rs
Defines the core interfaces for processing USB hotplug events:
- `HotplugEventProcessor`: Interface for handling device connect/disconnect events
- `DetectionMechanismManager`: Interface for switching between hotplug and polling modes
- `MigrationCoordinator`: Interface for safely migrating from polling to hotplug

**Performance Requirements**:
- Event processing must complete within 500ms
- Idle CPU usage must be <1%
- Device detection delay must be <500ms

### discovery_events.rs
Defines the integration interfaces with the existing discovery system:
- `DiscoveryEventIntegration`: Converting hotplug events to existing DiscoveryEvent format
- `ClientApiCompatibility`: Maintaining existing client API behavior

**Compatibility Requirements**:
- All existing DiscoveryEvent consumers must continue working unchanged
- TransportManager interface must remain identical
- Host service commands must return identical response formats

## Usage

These contracts should be implemented by:
1. **UsbMonitor** - Implements HotplugEventProcessor
2. **DiscoveryCoordinator** - Implements DiscoveryEventIntegration
3. **MigrationManager** - Implements MigrationCoordinator
4. **HostService** - Implements ClientApiCompatibility

## Testing

Each contract interface should have:
1. **Contract tests** that verify the interface behavior
2. **Integration tests** that test real implementations
3. **Performance tests** that verify timing requirements
4. **Compatibility tests** that ensure backward compatibility

These tests must be written BEFORE implementation (TDD approach) and must initially fail.