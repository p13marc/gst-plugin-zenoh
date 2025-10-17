# TODO List for gst-plugin-zenoh

This document outlines the improvements and fixes needed for the gst-plugin-zenoh project.

## Recently Completed

- ✅ **Removed Tokio dependency**: Simplified the codebase by removing Tokio and using Zenoh's synchronous API
- ✅ **Simplified thread model**: Eliminated background thread and channel communication in zenohsink
- ✅ **Improved resource management**: Better handling of resource cleanup in stop() methods
- ✅ **Fixed example code**: Updated example to use GLib's MainLoop instead of Tokio
- ✅ **Switched to stable dependencies**: Replaced GitLab dependencies with stable crates.io versions

## Next Steps

1. **Error Handling Improvements**:
   - Replace `unwrap()` calls with proper error handling
   - Improve error messages and propagation
   - Add error recovery mechanisms

2. **Add Configuration Options**:
   - Add Zenoh-specific configuration properties
   - Support different reliability modes
   - Allow timeouts to be configured

3. **Clean Up Warnings**:
   - Fix unused field warnings
   - Clean up debug category usage
   - Improve state management

## Remaining Tasks

### High Priority

### Error Handling

- [ ] Replace all `unwrap()` and `expect()` calls with proper error handling
- [ ] Implement proper error propagation from Zenoh operations to GStreamer
- [ ] Add detailed error messages when Zenoh operations fail
- [ ] Handle network disconnections gracefully

### Runtime Architecture

- [x] **COMPLETED**: Remove Tokio dependency
  - Replaced async patterns with Zenoh's synchronous API using `wait()`
  - Eliminated need for async runtime and background thread

### Resource Management

- [x] Improved Zenoh session cleanup in the `stop()` methods
- [ ] Add error handling for resource cleanup
- [ ] Add timeout handling for Zenoh operations

## Medium Priority

### Thread Safety and Concurrency

- [x] Simplified thread model by removing background thread
- [x] Eliminated race conditions from multi-threaded communication
- [ ] Audit mutex usages for potential deadlocks
- [ ] Improve state management

### Configuration Flexibility

- [ ] Add more configuration properties for Zenoh settings:
  - Connection parameters
  - QoS settings
  - Reliability modes
  - Timeout durations
- [ ] Allow customization of Zenoh config instead of using defaults
- [ ] Support for dynamic reconfiguration when possible

### Code Quality

- [x] Removed commented-out code from zenohsrc
- [ ] Fix remaining compiler warnings (unused session fields, unused CAT)
- [ ] Replace `unimplemented!()` with proper error handling
- [ ] Refactor unreachable assertions with better state checking

## Low Priority

### Compatibility and Maintenance

- [x] Pin gstreamer-rs dependency to a stable version from crates.io
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