# gst-plugin-zenoh Analysis Report

This document provides an analysis of the current codebase, identifying areas for improvement and potential features.

## Current Strengths

1. **Solid Architecture**: Clean separation between sink/src, proper state management with intermediate states (Stopped/Starting/Started/Stopping), comprehensive error handling with domain-specific error types

2. **Feature-Rich**:
   - Optional compression support (Zstd, LZ4, Gzip)
   - Automatic caps transmission and negotiation
   - Full QoS configuration (reliability, priority, congestion control, express mode)
   - URI handler support for convenient configuration
   - Real-time statistics tracking
   - Session sharing capability

3. **Well-Tested**: Comprehensive test suite covering plugin registration, integration, configuration, URI handling, statistics, and state management

4. **Production-Ready**: Proper flush handling, unlock support for responsive state changes, thread-safe statistics, property locking during operation

---

## Areas for Improvement

### 1. Reconnection/Resilience

**Location**: `src/zenohsink/imp.rs`, `src/zenohsrc/imp.rs`

Currently, if the Zenoh session fails, the element goes to error state with no recovery path.

**Suggested Fix**: Add reconnection logic with configurable retry behavior:
```
Property: reconnect-attempts (0 = disabled, -1 = infinite)
Property: reconnect-delay-ms (default: 1000)
```

---

### 2. LZ4 Decompression Hardcoded Limit

**Location**: `src/compression.rs:161`

```rust
const MAX_DECOMPRESSED_SIZE: i32 = 16 * 1024 * 1024;
```

This 16MB limit could fail for large video frames (4K raw ≈ 24MB, 8K ≈ 100MB).

**Suggested Fix**: 
- Option A: Store original size in metadata attachment
- Option B: Add configurable `max-decompressed-size` property
- Option C: Use a more generous default (e.g., 128MB)

---

### 3. Buffer Copies in Render Path

**Location**: `src/zenohsink/imp.rs` in `render()`

```rust
let data_to_send = b.as_slice().to_vec();
```

For high-throughput scenarios (4K60 raw video = ~1.5GB/s), this copy adds significant overhead.

**Suggested Fix**: Use Zenoh's zero-copy capabilities with `ZSlice::from()` where the buffer lifetime allows, or investigate `ZBytes::from()` with owned data.

---

### 4. Unused `dropped` Statistic in ZenohSrc

**Location**: `src/zenohsrc/imp.rs`

The `Statistics` struct has a `dropped` field but it's never updated. The FIFO channel handler could overflow.

**Suggested Fix**: 
- Use a bounded channel and track drops when full
- Or add a property to configure channel capacity

---

### 5. Hardcoded Receive Timeout

**Location**: `src/zenohsrc/imp.rs:538`

```rust
started.subscriber.recv_timeout(Duration::from_millis(100))
```

This 100ms timeout affects responsiveness vs CPU usage tradeoff.

**Suggested Fix**: Add configurable property:
```
Property: receive-timeout-ms (default: 100)
```

---

### 6. Missing PTS/DTS Preservation

**Location**: `src/zenohsink/imp.rs`, `src/zenohsrc/imp.rs`

Currently only Zenoh's timestamp is used. GStreamer buffer PTS/DTS are lost in transmission.

**Suggested Fix**: Include PTS/DTS in metadata attachment for proper A/V sync across network.

---

## Potential New Features

### High Priority

#### 1. Shared Memory Transport

Zenoh supports shared memory for ultra-low latency local IPC. This would dramatically improve performance for pipelines on the same machine.

```
Property: shm (bool, default: false) - Enable shared memory transport
Property: shm-size (uint, default: 64MB) - Shared memory pool size
```

**Benefits**:
- Near-zero copy for local subscribers
- Sub-millisecond latency for local streaming
- Significant CPU reduction

**Effort**: Medium | **Impact**: Very High

---

#### 2. Reconnection Logic

Automatic session recovery on network failures.

```
Property: reconnect-attempts (int, default: 0, -1 = infinite)
Property: reconnect-delay-ms (uint, default: 1000)
Property: reconnect-backoff-multiplier (float, default: 2.0)
```

**Effort**: Low | **Impact**: High

---

### Medium Priority

#### 3. Liveliness Tracking

Zenoh has liveliness tokens to detect publisher presence.

**For zenohsrc**:
- Property: `publisher-alive` (bool, read-only)
- Signal: `publisher-appeared` / `publisher-disappeared`
- Auto-recovery when publisher returns

**Use Cases**: 
- UI feedback when source disconnects
- Automatic failover to backup stream

**Effort**: Medium | **Impact**: Medium

---

#### 4. Buffer Metadata Preservation

Transmit full GStreamer buffer metadata, not just caps:

- PTS/DTS timestamps
- Duration
- Buffer flags (keyframe, delta, header, etc.)
- Offset/offset-end

**Metadata Format Extension**:
```
gst.pts=<nanoseconds>
gst.dts=<nanoseconds>
gst.duration=<nanoseconds>
gst.flags=<comma-separated flags>
```

**Effort**: Medium | **Impact**: Medium

---

#### 5. Configurable Channel Capacity

Control the internal buffer between Zenoh and GStreamer.

```
Property: queue-size (uint, default: 16) - Number of samples to buffer
Property: queue-leaky (enum: none/upstream/downstream, default: none)
```

**Effort**: Low | **Impact**: Medium

---

#### 6. Adaptive QoS Events

When `congestion-control=drop` triggers drops, emit GStreamer QoS events upstream to signal encoders to reduce bitrate.

```
Property: adaptive-qos (bool, default: false)
```

**Effort**: Medium | **Impact**: Medium

---

### Lower Priority / Advanced Features

#### 7. Queryable Element

New element `zenohqueryable` that responds to Zenoh queries.

**Use Cases**:
- Last-frame queries (get latest frame on demand)
- Keyframe requests for stream joining
- Stream metadata queries

```bash
# Publisher stores latest frame
gst-launch-1.0 videotestsrc ! zenohqueryable key-expr=camera/latest

# Client queries on-demand
zenoh_client.get("camera/latest")
```

**Effort**: High | **Impact**: Medium

---

#### 8. Multi-Key Demuxing for Source

Allow `zenohsrc` to subscribe to multiple key expressions and route to different pads:

```bash
zenohsrc key-expr="camera/*/video" ! zenohdemux name=d \
  d.camera_1 ! queue ! autovideosink \
  d.camera_2 ! queue ! autovideosink
```

**Effort**: High | **Impact**: Medium

---

#### 9. QoS History (Late-Join Support)

Zenoh supports keeping history of published values. New subscribers can receive last N samples.

```
Property: history (uint, default: 0) - Samples to keep for late joiners
```

**Use Cases**:
- New subscribers get recent keyframe immediately
- Catch-up on missed sensor readings

**Effort**: Low | **Impact**: Medium

---

#### 10. Batching/Aggregation

For high-frequency small messages (sensor data), batch multiple samples:

```
Property: batch-size (uint, default: 0, 0 = disabled)
Property: batch-timeout-ms (uint, default: 10)
```

**Effort**: Medium | **Impact**: Low-Medium

---

#### 11. TLS/Encryption Properties

Expose TLS configuration as element properties (alternative to config file):

```
Property: tls-cert - Path to TLS certificate
Property: tls-key - Path to TLS private key
Property: tls-ca - Path to CA certificate
Property: tls-skip-verify (bool) - Skip certificate verification
```

**Effort**: Low | **Impact**: Low (config file already works)

---

#### 12. Admin/Monitoring Element

New element `zenohmonitor` that exposes Zenoh network state:

- Subscribe to `@/router/**` admin space
- Expose peer list, link quality, network topology
- Emit GStreamer messages on topology changes

**Effort**: High | **Impact**: Low

---

## Priority Matrix

| Priority | Feature | Effort | Impact | Complexity |
|----------|---------|--------|--------|------------|
| **P0** | Fix LZ4 size limit | Low | Medium | Simple |
| **P0** | Configurable receive timeout | Low | Medium | Simple |
| **P1** | Shared Memory Transport | Medium | Very High | Medium |
| **P1** | Reconnection Logic | Low | High | Medium |
| **P1** | Update dropped statistic | Low | Low | Simple |
| **P2** | Buffer metadata preservation | Medium | Medium | Medium |
| **P2** | Liveliness tracking | Medium | Medium | Medium |
| **P2** | Configurable channel capacity | Low | Medium | Simple |
| **P3** | Adaptive QoS events | Medium | Medium | Medium |
| **P3** | QoS History | Low | Medium | Simple |
| **P3** | TLS properties | Low | Low | Simple |
| **P4** | Queryable element | High | Medium | Complex |
| **P4** | Multi-key demuxing | High | Medium | Complex |
| **P4** | Batching | Medium | Low | Medium |
| **P4** | Admin/Monitor element | High | Low | Complex |

---

## Quick Wins (Can Be Done Quickly)

1. **Fix LZ4 limit**: Change constant or add property
2. **Add receive-timeout-ms property**: Simple property addition
3. **Add queue-size property**: Change FIFO handler capacity
4. **Add history property**: Zenoh API supports this directly
5. **Update dropped statistic**: Track channel overflow

---

## Conclusion

The plugin is well-architected and feature-complete for basic use cases. The highest-impact improvements would be:

1. **Shared memory transport** - Transforms local streaming performance
2. **Reconnection logic** - Essential for production reliability
3. **Buffer metadata preservation** - Important for proper A/V sync

The quick wins (LZ4 fix, configurable timeouts) should be addressed first as they're simple changes that improve correctness and flexibility.
