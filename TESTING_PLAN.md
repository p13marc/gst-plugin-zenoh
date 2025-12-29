# Testing Plan for gst-plugin-zenoh

## Research: GStreamer Testing Best Practices

Based on research of official GStreamer documentation and gst-plugins-rs:

### Recommended Tools

1. **[GstHarness](https://gstreamer.freedesktop.org/documentation/check/gstharness.html)** - Primary testing framework
   - Treats element as "black box"
   - Two floating pads connect to element's src/sink
   - Deterministic data injection and output inspection
   - Already available via `gst-check` crate in our dependencies

2. **[TestClock](https://docs.rs/gstreamer-check)** - For time-sensitive testing
   - Control time progression deterministically
   - Useful for testing timing-related behavior

3. **AppSrc/AppSink** - For data flow testing
   - `appsrc`: Inject known data into pipeline
   - `appsink`: Capture and verify output data
   - Best for end-to-end integration tests

### Key GstHarness Methods

| Method | Purpose |
|--------|---------|
| `Harness::new(element_name)` | Create harness for element |
| `set_src_caps_str()` | Set input capabilities |
| `push()` | Send buffer to element |
| `pull()` | Get buffer from element (60s timeout) |
| `try_pull()` | Non-blocking buffer retrieval |
| `push_and_pull()` | Combined push/pull |
| `use_testclock()` | Install deterministic clock |

### Testing Patterns from gst-plugins-rs

```rust
// Pattern 1: Simple element test with Harness
let h = gst_check::Harness::new("element_name");
h.set_src_caps_str("video/x-raw,format=RGB");
let buf = h.create_buffer(1024);
h.push(buf);
let out = h.pull().unwrap();

// Pattern 2: AppSrc/AppSink for integration
let appsrc = gst_app::AppSrc::builder()
    .caps(&caps)
    .format(gst::Format::Time)
    .build();
appsrc.push_buffer(buffer);
```

### Sources
- [GstHarness Documentation](https://gstreamer.freedesktop.org/documentation/check/gstharness.html)
- [gstreamer-check crate](https://lib.rs/crates/gstreamer-check)
- [gst-plugins-rs repository](https://github.com/GStreamer/gst-plugins-rs)
- [gstreamer-rs examples](https://github.com/sdroege/gstreamer-rs/blob/main/examples/src/bin/appsrc.rs)

---

## Current State

- **Total tests**: 154 (21 unit tests + 133 integration tests)
- **Test files**: 12 files in `tests/` directory
- **Unit tests**: Located in `src/metadata.rs` and `src/session.rs`
- **All tests pass**: As of 2025-12-29

## Current Test Coverage

| Test File | Tests | What It Covers |
|-----------|-------|----------------|
| `plugin_tests.rs` | 8 | Element creation, property defaults, invalid properties |
| `integration_tests.rs` | 4 | Pipeline creation, config files, multiple key expressions |
| `error_tests.rs` | 6 | Missing key-expr, invalid config, property bounds |
| `state_management_tests.rs` | 5 | State transitions, double start/stop |
| `configuration_tests.rs` | 7 | QoS properties (priority, reliability, congestion-control, express) |
| `session_sharing_tests.rs` | 10 | Session groups, shared sessions via API |
| `statistics_tests.rs` | 8 | Stats properties, read-only enforcement |
| `uri_handler_tests.rs` | 28 | URI parsing for sink and src |
| `render_list_tests.rs` | 8 | Buffer list handling |
| `zenohdemux_tests.rs` | 7 | Demux element creation and properties |
| `simple_config_test.rs` | 5 | Config validation |
| Unit tests (metadata) | 18 | Metadata serialization, caps, buffer timing |
| Unit tests (session) | 2 | Session group reuse |

## Identified Gaps

### Critical Gaps (High Priority)

1. **No actual data flow verification**
   - Tests check element creation and state transitions
   - No test verifies data sent from `zenohsink` arrives at `zenohsrc`
   - No test verifies data integrity (what goes in == what comes out)

2. **No compression round-trip tests**
   - Compression feature flags exist but aren't tested in integration
   - No verification that compressed data decompresses correctly

3. **No buffer metadata preservation tests**
   - `send-buffer-meta` and `apply-buffer-meta` properties exist
   - No test verifies PTS/DTS/duration/flags are preserved across network

4. **No caps transmission tests**
   - `send-caps` property exists
   - No test verifies caps are correctly transmitted and applied

### Important Gaps (Medium Priority)

5. **No concurrent element tests**
   - Multiple sinks/sources sharing sessions not tested under load
   - Race conditions not verified

6. **No timeout and error recovery tests**
   - `receive-timeout-ms` property behavior not tested
   - Network failure simulation not tested

7. **No zenohdemux data flow tests**
   - Demux element only tested for creation/properties
   - Dynamic pad creation with actual data not tested

8. **Statistics integration test disabled**
   - Commented out due to timeout issues
   - Need reliable way to test stats update

### Nice-to-Have Gaps (Lower Priority)

9. **No performance/stress tests**
   - High throughput scenarios not tested
   - Large buffer handling not tested beyond creation

10. **No long-running stability tests**
    - Memory leak detection
    - Resource cleanup verification

## Testing Strategy

### Phase 1: End-to-End Data Flow (Priority: Critical)

Create `tests/data_flow_tests.rs`:

```rust
// Test 1: Basic data round-trip
// - Send known data pattern through zenohsink
// - Receive via zenohsrc  
// - Verify exact match

// Test 2: Multiple buffers
// - Send sequence of numbered buffers
// - Verify all received in order
// - Verify count matches

// Test 3: Various buffer sizes
// - Small buffers (< 100 bytes)
// - Medium buffers (1KB - 100KB)
// - Large buffers (1MB+)
```

**Implementation approach**:
- Use `appsrc` to inject known data
- Use `appsink` to capture received data
- Compare byte-for-byte
- Use unique key expressions per test to avoid interference

### Phase 2: Metadata Preservation (Priority: Critical)

Create `tests/metadata_tests.rs`:

```rust
// Test 1: Buffer timing preservation
// - Set specific PTS, DTS, duration on buffer
// - Send through zenohsink with send-buffer-meta=true
// - Receive via zenohsrc with apply-buffer-meta=true
// - Verify timing values match

// Test 2: Caps transmission
// - Set specific caps on pipeline
// - Verify caps received on other end

// Test 3: Buffer flags preservation
// - Set various buffer flags
// - Verify flags preserved
```

### Phase 3: Compression Tests (Priority: High)

Create `tests/compression_tests.rs`:

```rust
// Test 1: Each compression algorithm
// - zstd round-trip
// - lz4 round-trip  
// - gzip round-trip

// Test 2: Compression levels
// - Level 1 (fastest)
// - Level 5 (balanced)
// - Level 9 (best compression)

// Test 3: Mixed compression
// - Sender uses compression
// - Receiver handles decompression automatically
```

**Note**: Tests should be conditional on compression features being enabled.

### Phase 4: ZenohDemux Data Flow (Priority: Medium)

Create `tests/demux_data_flow_tests.rs`:

```rust
// Test 1: Single stream demux
// - Send to "test/stream1"
// - Subscribe to "test/*"
// - Verify pad created and data received

// Test 2: Multiple streams
// - Send to "test/stream1" and "test/stream2"
// - Verify separate pads created
// - Verify data routed correctly

// Test 3: Pad naming strategies
// - full-path naming
// - last-segment naming
// - hash naming
```

### Phase 5: Error Handling & Edge Cases (Priority: Medium)

Create `tests/error_handling_tests.rs`:

```rust
// Test 1: Receive timeout behavior
// - Configure short timeout
// - No data sent
// - Verify element handles timeout gracefully

// Test 2: Invalid key expression at runtime
// - Attempt operations with malformed key-expr

// Test 3: Session disconnection handling
// - Start pipeline
// - Simulate network issues
// - Verify graceful degradation
```

### Phase 6: Concurrent Operations (Priority: Medium)

Create `tests/concurrent_tests.rs`:

```rust
// Test 1: Multiple sinks same session
// - Create 3 sinks sharing session group
// - Send data from all simultaneously
// - Verify no crashes or data corruption

// Test 2: Multiple sources same key
// - Multiple sources subscribing to same key
// - Verify all receive the data

// Test 3: Mixed sink/source operations
// - Rapid create/destroy cycles
// - Verify resource cleanup
```

## Implementation Order

| Phase | Effort | Impact | Order |
|-------|--------|--------|-------|
| Phase 1: Data Flow | Medium | Critical | 1st |
| Phase 2: Metadata | Medium | Critical | 2nd |
| Phase 3: Compression | Low | High | 3rd |
| Phase 4: Demux Flow | Medium | Medium | 4th |
| Phase 5: Error Handling | Medium | Medium | 5th |
| Phase 6: Concurrent | High | Medium | 6th |

## Test Infrastructure Needs

### 1. Test Helpers

Create `tests/common/helpers.rs`:

```rust
/// Create a pipeline that sends known data through Zenoh
pub fn create_sender_pipeline(key_expr: &str, data: &[u8]) -> gst::Pipeline;

/// Create a pipeline that receives data from Zenoh
pub fn create_receiver_pipeline(key_expr: &str) -> (gst::Pipeline, Receiver<Vec<u8>>);

/// Wait for pipeline to process N buffers with timeout
pub fn wait_for_buffers(pipeline: &gst::Pipeline, count: usize, timeout: Duration) -> Result<(), Error>;

/// Generate unique key expression for test isolation
pub fn unique_key_expr(prefix: &str) -> String;
```

### 2. Test Timeout Handling

All data flow tests need proper timeout handling to avoid hanging:

```rust
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

fn with_timeout<F, T>(f: F) -> Result<T, Error>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    // Run in thread with timeout
}
```

### 3. Feature-Gated Tests

Compression tests should be conditional:

```rust
#[cfg(feature = "compression-zstd")]
#[test]
fn test_zstd_compression() {
    // ...
}
```

## Success Criteria

After implementing this plan:

1. **Data integrity verified**: Prove data sent == data received
2. **Metadata preserved**: PTS/DTS/duration/flags verified
3. **Compression working**: All algorithms tested
4. **Demux functional**: Dynamic pads tested with real data
5. **Error cases handled**: Graceful degradation verified
6. **Concurrent usage safe**: No race conditions

## Running Tests

```bash
# All tests
cargo test

# With compression features
cargo test --features compression

# Specific test file
cargo test --test data_flow_tests

# With output
cargo test -- --nocapture

# Single test
cargo test test_basic_data_roundtrip -- --nocapture
```

## CI Integration

Tests should run in CI with:
- Standard features
- All compression features enabled
- Both debug and release modes

```yaml
# Example CI matrix
test:
  strategy:
    matrix:
      features: ["", "compression"]
      profile: ["dev", "release"]
```
