# Tasks: Simple USB Hotplug Detection

**Input**: Design documents from `/specs/002-usb-hotplug/`
**Prerequisites**: plan.md, research.md, data-model.md, contracts/

## Summary
Replace polling-based USB device discovery with `nusb::watch_devices()` event-driven detection. Implement basic contracts for hotplug interfaces. Keep it simple for personal project - no over-engineering.

## Format: `[ID] [P?] Description`
- **[P]**: Can run in parallel (different files, no dependencies)
- Include exact file paths in descriptions

## Phase 3.1: Setup
- [ ] T001 Check nusb version in Cargo.toml matches requirements (>= 0.1.0)
- [ ] T002 Run cargo check to ensure all dependencies compile

## Phase 3.2: Tests First (TDD) ⚠️ MUST COMPLETE BEFORE 3.3
**CRITICAL: These tests MUST be written and MUST FAIL before ANY implementation**
- [ ] T003 [P] Contract test for HotplugEventProcessor in tests/contract/test_hotplug_events.rs
- [ ] T004 [P] Contract test for DiscoveryEventIntegration in tests/contract/test_discovery_integration.rs
- [ ] T005 [P] Basic hotplug detection test in tests/integration/test_hotplug_basic.rs
- [ ] T006 [P] Event conversion test in tests/unit/test_discovery_events.rs

## Phase 3.3: Core Implementation (ONLY after tests are failing)
- [ ] T007 [P] Implement HotplugEvent entity in src/transport/hotplug/events.rs
- [ ] T008 [P] Implement DetectionMechanism entity in src/transport/hotplug/detection.rs
- [ ] T009 Implement HotplugEventProcessor trait in src/transport/usb_monitor.rs
- [ ] T010 Add nusb::watch_devices() support to src/discovery/coordinator.rs
- [ ] T011 Modify discovery_loop() to use hotplug events instead of timer
- [ ] T012 Convert nusb HotplugEvent to existing DiscoveryEvent format
- [ ] T013 Remove or minimize polling timer usage

## Phase 3.4: Manual Testing & Validation
- [ ] T014 Test with real USB device using quickstart.md scenarios
- [ ] T015 Verify CPU usage drops when idle (no polling)
- [ ] T016 Verify existing DiscoveryEvent consumers still work unchanged

## Phase 3.5: Polish
- [ ] T017 [P] Update CLAUDE.md with hotplug implementation notes
- [ ] T018 Clean up any debug logging or temporary code
- [ ] T019 Run cargo clippy and fix any warnings

## Dependencies
- Setup (T001-T002) before tests
- Tests (T003-T006) before implementation (T007-T013)
- T007-T008 (entities) before T009 (trait implementation)
- T009 (trait) before T010-T013 (discovery integration)
- Core implementation before manual testing (T014-T016)
- Everything before polish (T017-T019)

## Parallel Example
```bash
# Launch T003-T006 together (different test files):
# Task: "Contract test for HotplugEventProcessor in tests/contract/test_hotplug_events.rs"
# Task: "Contract test for DiscoveryEventIntegration in tests/contract/test_discovery_integration.rs"
# Task: "Basic hotplug detection test in tests/integration/test_hotplug_basic.rs"
# Task: "Event conversion test in tests/unit/test_discovery_events.rs"

# Launch T007-T008 together (different entity files):
# Task: "Implement HotplugEvent entity in src/transport/hotplug/events.rs"
# Task: "Implement DetectionMechanism entity in src/transport/hotplug/detection.rs"
```

## Notes
- Keep it simple - this is a personal project
- Focus on replacing polling with hotplug events
- Use existing types (DeviceInfo, DiscoveryEvent)
- Implement basic contracts for hotplug interfaces
- Tests must fail before implementation (TDD)
- Commit after each task

## Key Files to Modify
- `src/transport/hotplug/` - New hotplug entity implementations
- `src/transport/usb_monitor.rs` - Trait implementations
- `src/discovery/coordinator.rs` - Main hotplug integration
- `tests/contract/` - Contract tests for interfaces
- `tests/integration/` - New integration tests
- `tests/unit/` - New unit tests
- `CLAUDE.md` - Documentation update

## Expected Outcome
- USB devices detected via events instead of polling
- Immediate detection (no 1-second delay)
- Lower CPU usage when idle
- Automatic fallback if hotplug fails
- All existing functionality preserved