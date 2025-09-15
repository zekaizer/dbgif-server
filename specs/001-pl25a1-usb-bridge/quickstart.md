# Quickstart Guide: Multi-Transport ADB Daemon

## Prerequisites

### Hardware (Choose One or More)
- **TCP Connection**: Network-connected device running ADB daemon
- **USB Device**: Android device with USB debugging enabled
- **USB Bridge**: PL25A1 USB Host-to-Host bridge cable connecting two computers
- Windows 10+ or Linux (Ubuntu 20.04+)

### Software Dependencies
- Rust 1.75+ with cargo
- Git for version control
- Transport-specific drivers:
  - **TCP**: No additional drivers needed
  - **USB Device**: Android USB drivers (usually automatic)
  - **USB Bridge**: WinUSB driver for PL25A1 (Windows), usbfs support (Linux)

## Installation

### 1. Clone Repository
```bash
git clone <repository-url>
cd dbgif-server
git checkout 001-pl25a1-usb-bridge
```

### 2. Build Daemon
```bash
# Debug build for development
cargo build

# Release build for production
cargo build --release
```

### 3. USB Permissions (Linux only)
```bash
# Add udev rule for PL25A1 device access
sudo tee /etc/udev/rules.d/99-pl25a1.rules << EOF
SUBSYSTEM=="usb", ATTR{idVendor}=="067b", ATTR{idProduct}=="25a1", MODE="0666"
EOF

# Reload udev rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

## Hardware Setup

### 1. TCP Transport Setup
```bash
# Enable ADB over TCP on target device (Android)
adb tcpip 5555

# Or manually configure target device to listen on port 5555
# Ensure both devices are on same network
```

### 2. USB Device Setup
```bash
# Enable USB debugging on Android device
# Settings -> Developer Options -> USB Debugging

# Connect via USB cable and verify
adb devices
```

### 3. USB Bridge Setup
```bash
# Connect PL25A1 cable between PC and target device
# Ensure both USB connectors are firmly seated
# Wait for USB device enumeration

# Verify detection (Linux)
lsusb | grep "067b:25a1"

# Verify detection (Windows)
# Check Device Manager for "Prolific USB-to-USB Bridge"
```

## Running the Daemon

### 1. Start Daemon
```bash
# Run with debug logging
RUST_LOG=debug cargo run

# Run in background (Linux)
cargo run --release > daemon.log 2>&1 &
```

### 2. Verify Daemon Status
```bash
# Check if daemon is listening on port 5037
netstat -ln | grep 5037

# Or on Windows
netstat -an | findstr 5037
```

## Basic Usage

### 1. Test Connection
```bash
# Using adb client (if available)
adb devices

# Using telnet for manual testing
telnet localhost 5037
```

### 2. Send ADB Commands
```bash
# List all connected devices (all transport types)
echo "host:devices" | nc localhost 5037

# Expected output format:
# tcp:192.168.1.100:5555    device
# usb:18d1:4ee7             device
# bridge:067b:25a1          device

# Get specific device state
echo "host:get-state" | nc localhost 5037

# Check daemon version
echo "host:version" | nc localhost 5037
```

### 3. Establish Debug Session
```bash
# Connect to device transport
echo "host:transport:<device-id>" | nc localhost 5037

# Open shell stream (after transport connection)
# This would be handled by ADB client normally
```

## Validation Tests

### 1. Connection Test
```bash
# Test script to validate basic functionality
cat > test_connection.sh << 'EOF'
#!/bin/bash
echo "Testing PL25A1 connection..."

# Check USB device presence
if lsusb | grep -q "067b:25a1"; then
    echo "✓ PL25A1 device detected"
else
    echo "✗ PL25A1 device not found"
    exit 1
fi

# Check daemon listening
if netstat -ln | grep -q ":5037"; then
    echo "✓ Daemon listening on port 5037"
else
    echo "✗ Daemon not listening"
    exit 1
fi

# Test device list command
RESPONSE=$(echo "host:devices" | nc -w 1 localhost 5037)
if [ -n "$RESPONSE" ]; then
    echo "✓ Device list command successful"
    echo "Devices: $RESPONSE"
else
    echo "✗ Device list command failed"
    exit 1
fi

echo "✓ All tests passed!"
EOF

chmod +x test_connection.sh
./test_connection.sh
```

### 2. Performance Test
```bash
# Simple throughput test
cat > test_performance.sh << 'EOF'
#!/bin/bash
echo "Testing USB bridge performance..."

# Create test data
dd if=/dev/urandom of=test_data.bin bs=1024 count=256

# Time data transfer (placeholder - actual implementation needed)
echo "Throughput test would be implemented here"

# Cleanup
rm test_data.bin
EOF

chmod +x test_performance.sh
./test_performance.sh
```

## Troubleshooting

### Common Issues

#### 1. Device Not Detected
```bash
# Check USB enumeration
dmesg | grep -i usb | tail -20

# Verify VID/PID
lsusb -v -d 067b:25a1
```

#### 2. Permission Errors (Linux)
```bash
# Check current user groups
groups $USER

# Add user to dialout group if needed
sudo usermod -a -G dialout $USER
# Logout and login again
```

#### 3. Daemon Won't Start
```bash
# Check if port is already in use
sudo lsof -i :5037

# Kill existing process if needed
sudo pkill -f dbgif-server
```

#### 4. Connection Drops
```bash
# Monitor USB connection events
dmesg -w | grep usb

# Check daemon logs for errors
tail -f daemon.log
```

### Debug Mode
```bash
# Enable maximum logging
RUST_LOG=trace cargo run

# Log USB communication details
RUST_LOG=dbgif_server::transport::bridge_usb=trace cargo run
```

## Next Steps

### Development Workflow
1. Make code changes
2. Run tests: `cargo test`
3. Build and test: `cargo run`
4. Validate with quickstart tests

### Extending Functionality
- Add new ADB services (shell, file sync)
- Implement additional transport types
- Add monitoring and metrics
- Create GUI client application

### Integration Testing
- Test with real Android devices
- Validate protocol compatibility
- Performance benchmarking
- Cross-platform testing

## Support

### Getting Help
- Check project documentation in `docs/`
- Review implementation plan in `specs/001-pl25a1-usb-bridge/`
- Submit issues for bugs or feature requests

### Useful Resources
- [ADB Protocol Documentation](../../../docs/ADB_Architecture_Protocol.md)
- [PL25A1 Hardware Specifications](../../../docs/PL25A1.md)
- [nusb Crate Documentation](https://docs.rs/nusb/)
- [Tokio Async Runtime Guide](https://tokio.rs/tokio/tutorial)