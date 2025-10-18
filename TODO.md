# TODO List for gst-plugin-zenoh

This document outlines the improvements and fixes needed for the gst-plugin-zenoh project.

## Recently Completed

- ✅ **Removed Tokio dependency**: Simplified the codebase by removing Tokio and using Zenoh's synchronous API
- ✅ **Simplified thread model**: Eliminated background thread and channel communication in zenohsink
- ✅ **Improved resource management**: Better handling of resource cleanup in stop() methods
- ✅ **Fixed example code**: Updated example to use GLib's MainLoop instead of Tokio
- ✅ **Switched to stable dependencies**: Replaced GitLab dependencies with stable crates.io versions
- ✅ **Fixed compiler warnings**: Added proper #[allow(dead_code)] annotations for unused fields
- ✅ **Improved error handling**: Added domain-specific error types and proper error propagation
- ✅ **Added comprehensive documentation**: Documented all elements and their properties
- ✅ **Refactored assertions**: Replaced unreachable! with proper error handling
- ✅ **Enhanced network resilience**: Added better error handling for network issues
- ✅ **Added comprehensive unit tests**: 14 tests covering core functionality, properties, and error handling

## Next Steps

1. **Integration Testing and Examples**:
   - [x] Add unit tests for core functionality (14 tests passing)
   - [x] Implement integration tests with actual Zenoh network (4 tests covering pipelines, config, properties)
   - [x] Create examples demonstrating various usage patterns (4 comprehensive examples + README)

2. **Advanced Features**:
   - Add reconnection logic for network failures
   - Support for dynamic reconfiguration when possible
   - Add statistics reporting for monitoring

3. **Performance Optimization**:
   - Better buffer management to reduce copies
   - Optimize serialization/deserialization
   - Test performance under various network conditions

4. **Configuration (completed)**:
   - [x] Add sink properties: key-expr, config, priority, congestion-control, reliability 
   - [x] Add source properties mirroring sink where applicable
   - [x] Wire configuration values to zenoh where applicable (config file)

## Remaining Tasks

### High Priority

### Error Handling

- [x] Replace all `unwrap()` and `expect()` calls with proper error handling
- [x] Implement proper error propagation from Zenoh operations to GStreamer
- [x] Add detailed error messages when Zenoh operations fail
- [x] Handle network disconnections gracefully

### Runtime Architecture

- [x] **COMPLETED**: Remove Tokio dependency
  - Replaced async patterns with Zenoh's synchronous API using `wait()`
  - Eliminated need for async runtime and background thread

### Resource Management

- [x] Improved Zenoh session cleanup in the `stop()` methods
- [x] Add error handling for resource cleanup
- [ ] Add timeout handling for Zenoh operations

## Medium Priority

### Thread Safety and Concurrency

- [x] Simplified thread model by removing background thread
- [x] Eliminated race conditions from multi-threaded communication
- [ ] Audit mutex usages for potential deadlocks
- [x] Improve state management (enhanced with intermediate states, validation, logging, and comprehensive tests)

### Configuration Flexibility

- [x] Add more configuration properties for Zenoh settings:
  - Config file path for both sink and source
  - Reliability mode, congestion control, and priority exposed as properties
- [x] Prepare for more sophisticated Zenoh configuration via API
- [ ] Support for dynamic reconfiguration when possible

### Code Quality

- [x] Removed commented-out code from zenohsrc
- [x] Fix remaining compiler warnings (unused session fields, unused CAT)
- [x] Replace `unimplemented!()` with proper error handling
- [x] Refactor unreachable assertions with better state checking

## Low Priority

### Compatibility and Maintenance

- [x] Pin gstreamer-rs dependency to a stable version from crates.io
- [ ] Create a compatibility matrix for supported versions
- [ ] Add continuous integration for different Zenoh/GStreamer versions

### Documentation

- [x] Add comprehensive inline documentation
- [x] Document expected behavior during error conditions
- [x] Create examples demonstrating various usage patterns (4 comprehensive examples + README)
- [x] Add architecture documentation explaining component interactions (comprehensive ARCHITECTURE.md)

### Testing

- [x] Add unit tests for core functionality (14 tests with cargo nextest)
- [x] Add tests for error conditions and recovery
- [ ] Implement integration tests with actual Zenoh network
- [ ] Test performance under various network conditions

## Future Enhancements

- [ ] Support for metadata in Zenoh samples
- [ ] Better buffer management to reduce copies
- [ ] Optimize serialization/deserialization
- [ ] Support for multicast and other Zenoh transport modes
- [ ] Add statistics reporting for monitoring