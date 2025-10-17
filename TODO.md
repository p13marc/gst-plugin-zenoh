# TODO List for gst-plugin-zenoh

This document outlines the improvements and fixes needed for the gst-plugin-zenoh project.

## High Priority

### Error Handling

- [ ] Replace all `unwrap()` and `expect()` calls with proper error handling
- [ ] Implement proper error propagation from Zenoh operations to GStreamer
- [ ] Add detailed error messages when Zenoh operations fail
- [ ] Handle network disconnections gracefully

### Runtime Architecture

- [ ] **IMPORTANT**: Remove Tokio dependency
  - The futures in the main function from GStreamer should be scheduled by GLib, not Tokio
  - Replace the shared Tokio runtime with GLib's MainContext for async operations
  - Use GLib's async utilities instead of Tokio-specific ones

### Resource Management

- [ ] Properly close Zenoh sessions in the `stop()` methods
- [ ] Ensure all resources are released when elements are destroyed
- [ ] Add timeout handling for Zenoh operations
- [ ] Implement cleanup for async tasks when pipeline state changes

## Medium Priority

### Thread Safety and Concurrency

- [ ] Audit all mutex usages for potential deadlocks
- [ ] Improve state management to avoid race conditions
- [ ] Replace blocking calls with non-blocking alternatives
- [ ] Add better synchronization between GStreamer and Zenoh threads

### Configuration Flexibility

- [ ] Add more configuration properties for Zenoh settings:
  - Connection parameters
  - QoS settings
  - Reliability modes
  - Timeout durations
- [ ] Allow customization of Zenoh config instead of using defaults
- [ ] Support for dynamic reconfiguration when possible

### Code Quality

- [ ] Fix compiler warnings (dead code, unused variables)
- [ ] Remove commented-out code
- [ ] Replace `unimplemented!()` with proper error handling
- [ ] Refactor unreachable assertions with better state checking

## Low Priority

### Compatibility and Maintenance

- [ ] Pin gstreamer-rs dependency to a stable version instead of branch
- [ ] Create a compatibility matrix for supported versions
- [ ] Add continuous integration for different Zenoh/GStreamer versions

### Documentation

- [ ] Add comprehensive inline documentation
- [ ] Document expected behavior during error conditions
- [ ] Create examples demonstrating various usage patterns
- [ ] Add architecture documentation explaining component interactions

### Testing

- [ ] Add unit tests for core functionality
- [ ] Implement integration tests with actual Zenoh network
- [ ] Add tests for error conditions and recovery
- [ ] Test performance under various network conditions

## Future Enhancements

- [ ] Support for metadata in Zenoh samples
- [ ] Better buffer management to reduce copies
- [ ] Optimize serialization/deserialization
- [ ] Support for multicast and other Zenoh transport modes
- [ ] Add statistics reporting for monitoring