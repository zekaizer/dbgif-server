//! USB hotplug detection module
//!
//! This module provides event-driven USB device detection using nusb::watch_devices()
//! as a replacement for polling-based discovery. It includes automatic fallback to
//! polling if hotplug detection fails.

pub mod events;
pub mod detection;

pub use events::{HotplugEvent, HotplugEventType};
pub use detection::{
    DetectionMechanism, DetectionStats, HotplugDetector, NusbHotplugDetector,
};