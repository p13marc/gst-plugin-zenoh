# gst-plugin-zenoh

[![Rust](https://img.shields.io/badge/rust-1.70+-blue.svg)](https://www.rust-lang.org/)
[![GStreamer](https://img.shields.io/badge/GStreamer-1.20+-green.svg)](https://gstreamer.freedesktop.org/)
[![Zenoh](https://img.shields.io/badge/Zenoh-1.0+-orange.svg)](https://zenoh.io/)
[![License](https://img.shields.io/badge/License-MPL--2.0-blue.svg)](https://opensource.org/licenses/MPL-2.0)

A high-performance [GStreamer](https://gstreamer.freedesktop.org/) plugin that enables distributed media streaming using [Zenoh](https://zenoh.io/) as the transport layer. Built with [zenoh-rs](https://github.com/eclipse-zenoh/zenoh) for maximum performance and reliability.

## Overview

The plugin provides two complementary GStreamer elements that bridge GStreamer pipelines with Zenoh networks:

- **`zenohsink`**: Publishes GStreamer buffers to Zenoh networks
- **`zenohsrc`**: Subscribes to Zenoh data and delivers it to GStreamer pipelines

Together, these elements enable distributed media applications, edge computing scenarios, robotics systems, IoT data streaming, and more.

## 🚀 Key Features

### Advanced Quality of Service (QoS)
- **Reliability Modes**: Choose between `best-effort` (low latency) and `reliable` (guaranteed delivery)
- **Congestion Control**: Handle network congestion with `block` (ensure delivery) or `drop` (maintain real-time performance)
- **Priority Management**: Message prioritization using Zenoh Priority levels (1-7) for intelligent bandwidth allocation

### Performance Optimization
- **Express Mode**: Ultra-low latency mode that bypasses internal queues
- **Session Sharing**: Efficient resource usage through shared Zenoh sessions
- **Batch Rendering**: Efficient buffer list processing for high-throughput scenarios
- **Responsive State Changes**: Sub-second response to pipeline state changes with proper unlock/flush support
- **Optimized Data Paths**: Minimal overhead with efficient memory handling
- **Optional Compression**: Reduce bandwidth usage with Zstandard, LZ4, or Gzip compression (compile-time optional)

### Flexible Configuration
- **URI Handler Support**: Configure elements using standard GStreamer URI syntax (e.g., `zenoh:demo/video?priority=2&reliability=reliable`)
- **Runtime Properties**: Configure QoS parameters dynamically
- **Zenoh Config Files**: Support for comprehensive Zenoh network configuration
- **Key Expression Patterns**: Flexible topic naming with wildcard support

### Automatic Format Negotiation
- **Caps Transmission**: GStreamer capabilities automatically transmitted with first buffer
- **Metadata Support**: Custom key-value metadata can be attached to streams
- **Zero Configuration**: Receiver automatically configures based on sender's format
- **Format Changes**: Supports dynamic format changes during streaming

### Production Monitoring
- **Real-time Statistics**: Track bytes sent/received, message counts, errors, and dropped packets
- **Read-only Properties**: Monitor performance without affecting operation
- **Thread-safe Updates**: Atomic statistics updates for accurate metrics

### Enterprise Ready
- **Rich Error Messages**: Contextual error messages with troubleshooting guidance
- **Comprehensive Error Handling**: 10 specific error types with helpful diagnostics
- **Thread Safety**: Safe concurrent access to all plugin components
- **Property Locking**: Runtime protection against invalid configuration changes
- **Extensive Testing**: 71 comprehensive tests ensuring reliability

## Quick Start

### Installation

1. **Install Dependencies** (Ubuntu/Debian):
   ```bash
   sudo apt-get update
   sudo apt-get install libunwind-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
   ```

   For Fedora/RHEL:
   ```bash
   sudo dnf install libunwind-devel gstreamer1-devel gstreamer1-plugins-base-devel
   ```

2. **Build the Plugin**:
   ```bash
   # Basic build (no compression)
   cargo build --release
   
   # With all compression algorithms
   cargo build --release --features compression
   
   # With specific compression algorithms
   cargo build --release --features compression-zstd
   cargo build --release --features compression-lz4
   cargo build --release --features compression-gzip
   ```

3. **Run Examples**:
   ```bash
   # Basic video streaming demonstration
   GST_PLUGIN_PATH=target/debug cargo run --example basic
   
   # Comprehensive QoS configuration showcase
   GST_PLUGIN_PATH=target/debug cargo run --example configuration
   ```

### Simple Streaming Example

```bash
# Terminal 1: Start video publisher
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video

# Terminal 2: Start video subscriber 
gst-launch-1.0 zenohsrc key-expr=demo/video ! videoconvert ! autovideosink
```

## 📋 Advanced Pipeline Examples

### High-Performance Video Streaming
```bash
# Ultra-low latency streaming with express mode
gst-launch-1.0 videotestsrc pattern=ball ! video/x-raw,width=1280,height=720,framerate=30/1 ! \
  x264enc tune=zerolatency speed-preset=ultrafast ! rtph264pay ! \
  zenohsink key-expr=demo/video/hd reliability=best-effort congestion-control=drop express=true priority=2

# Reliable HD streaming with error recovery
gst-launch-1.0 videotestsrc ! video/x-raw,width=1920,height=1080 ! \
  x264enc bitrate=5000 ! rtph264pay ! \
  zenohsink key-expr=demo/video/reliable reliability=reliable congestion-control=block priority=4
```

### Multi-Stream Applications
```bash
# Camera + Audio streaming
gst-launch-1.0 \
  v4l2src device=/dev/video0 ! videoconvert ! x264enc ! rtph264pay ! \
  zenohsink key-expr=demo/camera/video reliability=reliable \
  pulsesrc ! audioconvert ! audioresample ! opusenc ! rtpopuspay ! \
  zenohsink key-expr=demo/camera/audio reliability=reliable

# Multi-camera setup with priorities  
gst-launch-1.0 \
  v4l2src device=/dev/video0 ! zenohsink key-expr=demo/cam/main priority=2 \
  v4l2src device=/dev/video1 ! zenohsink key-expr=demo/cam/backup priority=6
```

### IoT and Sensor Data
```bash
# Sensor data with custom Zenoh configuration
gst-launch-1.0 appsrc ! \
  zenohsink key-expr=sensors/temperature/device-001 config=/etc/zenoh/iot.json5 \
  reliability=reliable priority=4

# Wildcard subscription for multiple sensors
gst-launch-1.0 zenohsrc key-expr="sensors/**" ! \
  appsink name=sensor_data
```

### Edge Computing Scenarios
```bash
# Edge AI processing pipeline
gst-launch-1.0 zenohsrc key-expr=edge/camera/raw ! \
  videoconvert ! videoscale ! video/x-raw,width=416,height=416 ! \
  tensor_converter ! tensor_transform mode=arithmetic option=typecast:float32,add:-127.5,div:127.5 ! \
  tensor_filter framework=tensorflow-lite model=detection.tflite ! \
  zenohsink key-expr=edge/ai/detections reliability=reliable express=true
```

## 🗜️ Compression Support

The plugin supports optional compression to reduce bandwidth usage. Compression is **compile-time optional** and must be enabled via Cargo features.

### Available Compression Algorithms

| Algorithm | Feature Flag | Characteristics | Best For |
|-----------|--------------|-----------------|----------|
| **Zstandard** | `compression-zstd` | Best compression ratio, good speed | General purpose, bandwidth-limited networks |
| **LZ4** | `compression-lz4` | Fastest compression, lower ratio | Low-latency, CPU-constrained systems |
| **Gzip** | `compression-gzip` | Widely compatible, moderate speed | Cross-platform compatibility |

### Building with Compression

```bash
# Enable all compression algorithms
cargo build --release --features compression

# Enable specific algorithms
cargo build --release --features compression-zstd
cargo build --release --features compression-lz4,compression-gzip
```

### Usage

Compression is configured on the **sender side** (`zenohsink`) and automatically detected and decompressed on the **receiver side** (`zenohsrc`).

```bash
# Sender with Zstandard compression (recommended)
gst-launch-1.0 videotestsrc ! \
  zenohsink key-expr=demo/compressed compression=zstd compression-level=5

# Receiver (automatically decompresses)
gst-launch-1.0 zenohsrc key-expr=demo/compressed ! videoconvert ! autovideosink
```

### Compression Levels

- **1-3**: Fast compression, larger output (low CPU usage)
- **4-6**: Balanced (recommended for most use cases)
- **7-9**: Maximum compression, slower (high CPU usage)

### Compression Statistics

When compression is enabled, `zenohsink` provides additional statistics:

| Property | Description |
|----------|-------------|
| `bytes-before-compression` | Total bytes before compression |
| `bytes-after-compression` | Total bytes after compression (network usage) |

**Calculate compression ratio:**
```bash
# Query compression statistics
gst-inspect-1.0 zenohsink | grep bytes-

# Example: 1GB before -> 300MB after = 70% bandwidth savings
```

### Performance Considerations

- **Zstandard**: Best all-around choice, excellent compression at level 5
- **LZ4**: Choose when CPU is limited or ultra-low latency is critical
- **Gzip**: Use for compatibility with non-Rust receivers

## 📊 Statistics Monitoring

Both `zenohsink` and `zenohsrc` provide real-time statistics for monitoring performance and debugging issues. All statistics properties are read-only and thread-safe.

### ZenohSink Statistics

| Property | Type | Description |
|----------|------|-------------|
| `bytes-sent` | UInt64 | Total bytes published to Zenoh network (after compression if enabled) |
| `messages-sent` | UInt64 | Total number of buffers published |
| `errors` | UInt64 | Number of publish errors encountered |
| `dropped` | UInt64 | Number of buffers dropped due to congestion (when `congestion-control=drop`) |
| `bytes-before-compression` | UInt64 | Total bytes before compression (compression features only) |
| `bytes-after-compression` | UInt64 | Total bytes after compression (compression features only) |

### ZenohSrc Statistics

| Property | Type | Description |
|----------|------|-------------|
| `bytes-received` | UInt64 | Total bytes received from Zenoh network |
| `messages-received` | UInt64 | Total number of buffers received |
| `errors` | UInt64 | Number of receive errors encountered |
| `dropped` | UInt64 | Number of samples dropped (reserved for future use) |

### Monitoring Examples

```bash
# Monitor statistics in real-time using gst-launch watch mode
GST_DEBUG=zenohsink:5 gst-launch-1.0 videotestsrc num-buffers=100 ! \
  zenohsink name=sink key-expr=demo/stats ! \
  fakesink

# Query statistics programmatically in a script
gst-launch-1.0 videotestsrc num-buffers=1000 ! \
  zenohsink name=mysink key-expr=demo/video ! fakesink & \
PIPELINE_PID=$!
sleep 5
# Use gst-inspect or property queries to read statistics
kill $PIPELINE_PID
```

### Programmatic Statistics Access (Rust)

```rust
use gst::prelude::*;

// Create pipeline with named sink
let pipeline = gst::parse_launch(
    "videotestsrc ! zenohsink name=sink key-expr=demo/monitor"
)?;

// Get the zenohsink element
let sink = pipeline
    .by_name("sink")
    .expect("Could not find sink element");

// Start pipeline
pipeline.set_state(gst::State::Playing)?;

// Monitor statistics periodically
loop {
    std::thread::sleep(std::time::Duration::from_secs(1));
    
    let bytes_sent: u64 = sink.property("bytes-sent");
    let messages_sent: u64 = sink.property("messages-sent");
    let errors: u64 = sink.property("errors");
    let dropped: u64 = sink.property("dropped");
    
    println!("Stats: {} bytes, {} msgs, {} errors, {} dropped",
             bytes_sent, messages_sent, errors, dropped);
    
    if messages_sent >= 1000 {
        break;
    }
}

pipeline.set_state(gst::State::Null)?;
```

## 🔗 URI Handler Support

Both elements implement the GStreamer `URIHandler` interface, allowing configuration via URI syntax. This provides a convenient alternative to setting individual properties.

### URI Syntax

```
zenoh:<key-expression>[?<parameter>=<value>&...]
```

### Supported URI Parameters

| Parameter | Values | Example |
|-----------|--------|---------|
| `priority` | 1-7 | `priority=2` |
| `reliability` | `best-effort`, `reliable` | `reliability=reliable` |
| `congestion-control` | `block`, `drop` | `congestion-control=drop` |
| `express` | `true`, `false` | `express=true` |
| `config` | File path | `config=/etc/zenoh/config.json5` |

### URI Examples

```bash
# Simple key expression only
gst-launch-1.0 videotestsrc ! zenohsink uri="zenoh:demo/video"

# With QoS parameters
gst-launch-1.0 videotestsrc ! \
  zenohsink uri="zenoh:demo/video?priority=2&reliability=reliable&express=true"

# Full configuration with custom Zenoh config
gst-launch-1.0 videotestsrc ! \
  zenohsink uri="zenoh:sensors/camera?priority=1&reliability=reliable&congestion-control=block&config=/etc/zenoh/edge.json5"

# Receiving with URI
gst-launch-1.0 \
  zenohsrc uri="zenoh:demo/video?priority=2" ! \
  videoconvert ! autovideosink

# Wildcard subscription
gst-launch-1.0 \
  zenohsrc uri="zenoh:sensors/**" ! \
  appsink
```

### URI vs Properties

Both methods are equivalent and can be mixed:

```bash
# Using individual properties
gst-launch-1.0 videotestsrc ! \
  zenohsink key-expr=demo/video priority=2 reliability=reliable

# Using URI (equivalent)
gst-launch-1.0 videotestsrc ! \
  zenohsink uri="zenoh:demo/video?priority=2&reliability=reliable"

# Mixed approach (URI sets base, properties override)
gst-launch-1.0 videotestsrc ! \
  zenohsink uri="zenoh:demo/video?priority=2" reliability=reliable express=true
```

### Programmatic URI Usage (Rust)

```rust
use gst::prelude::*;

// Create element and set URI
let sink = gst::ElementFactory::make("zenohsink").build()?;

// Set URI using URIHandler interface
if let Some(uri_handler) = sink.dynamic_cast_ref::<gst::URIHandler>() {
    uri_handler.set_uri("zenoh:demo/video?priority=2&reliability=reliable")?;
}

// Or use the uri property directly
sink.set_property("uri", "zenoh:demo/video?priority=2&reliability=reliable");

// Read back current URI
if let Some(uri_handler) = sink.dynamic_cast_ref::<gst::URIHandler>() {
    let current_uri = uri_handler.uri();
    println!("Current URI: {:?}", current_uri);
}
```

## ⚙️ Element Properties

### ZenohSink Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `key-expr` | String | *required* | Zenoh key expression for publishing (e.g., "demo/video/stream") |
| `config` | String | `null` | Path to Zenoh configuration file for custom network settings |
| `priority` | Integer | `5` | Publisher priority (1-7). Lower values = higher priority. 1=RealTime, 2=InteractiveHigh, 3=InteractiveLow, 4=DataHigh, 5=Data, 6=DataLow, 7=Background |
| `congestion-control` | String | `"block"` | Congestion handling: `"block"` (wait) or `"drop"` (discard messages) |
| `reliability` | String | `"best-effort"` | Delivery mode: `"best-effort"` (fast) or `"reliable"` (guaranteed) |
| `express` | Boolean | `false` | Enable express mode for ultra-low latency (bypasses internal queues) |
| `send-caps` | Boolean | `true` | Enable caps transmission as metadata (automatic format negotiation) |
| `caps-interval` | Integer | `1` | Interval in seconds to send caps periodically (0 = only first buffer and format changes) |
| `compression` | Enum | `none` | Compression algorithm: `none`, `zstd`, `lz4`, or `gzip` (requires compilation with compression features) |
| `compression-level` | Integer | `5` | Compression level (1=fastest/largest, 9=slowest/smallest, 5=balanced) |

#### Usage Examples:
```bash
# High priority reliable streaming (RealTime priority)
zenohsink key-expr=critical/data reliability=reliable priority=1 express=true

# Real-time best-effort streaming (InteractiveHigh priority)
zenohsink key-expr=realtime/video reliability=best-effort congestion-control=drop express=true priority=2

# Minimal bandwidth: send caps only on first buffer and format changes
zenohsink key-expr=optimized/stream caps-interval=0

# Disable caps entirely for absolute minimal overhead
zenohsink key-expr=nocaps/stream send-caps=false

# Compression examples (requires compression features enabled at compile time)
# High compression for bandwidth-limited networks (Zstandard)
zenohsink key-expr=compressed/video compression=zstd compression-level=9

# Balanced compression (recommended for most cases)
zenohsink key-expr=compressed/video compression=zstd compression-level=5

# Fast compression with minimal CPU overhead (LZ4)
zenohsink key-expr=compressed/video compression=lz4 compression-level=1

# Compatible compression (Gzip - widely supported)
zenohsink key-expr=compressed/video compression=gzip compression-level=6
```

### ZenohSrc Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `key-expr` | String | *required* | Zenoh key expression for subscription (supports wildcards: `*`, `**`) |
| `config` | String | `null` | Path to Zenoh configuration file |
|| `priority` | Integer | `5` | Subscriber priority (1-7). Lower values = higher priority. 1=RealTime, 2=InteractiveHigh, 3=InteractiveLow, 4=DataHigh, 5=Data, 6=DataLow, 7=Background |
| `congestion-control` | String | `"block"` | Informational only - actual behavior determined by publisher |
| `reliability` | String | `"best-effort"` | Expected reliability mode - actual mode matches publisher |

#### Wildcard Examples:
```bash
# Subscribe to all video streams from a device
zenohsrc key-expr="demo/device-01/video/*"

# Subscribe to all sensor data  
zenohsrc key-expr="sensors/**"

# Subscribe to specific data types across all devices
zenohsrc key-expr="**/temperature"
```

### Quality of Service (QoS) Guidelines

#### Reliability Modes
- **`best-effort`**: Minimal latency, no delivery guarantees
  - Use for: Live video, real-time sensor data, gaming
  - Latency: ~1-5ms additional
- **`reliable`**: Guaranteed delivery with acknowledgments  
  - Use for: Command & control, configuration updates, critical alerts
  - Latency: ~10-50ms additional (network dependent)

#### Congestion Control
- **`block`**: Pause publishing during network congestion
  - Use for: Critical data that cannot be lost
  - Behavior: May cause frame drops if buffers fill up
- **`drop`**: Discard messages during congestion
  - Use for: Real-time streams where recent data is most valuable
  - Behavior: Maintains smooth streaming with occasional quality loss

#### Priority Levels (Zenoh Priority Enum)

The plugin uses Zenoh's built-in Priority enum with 7 levels (lower number = higher priority):

| Value | Zenoh Priority | Use Case | Example Applications |
|-------|---------------|----------|---------------------|
| **1** | `RealTime` | Critical real-time systems | Safety systems, emergency alerts |
| **2** | `InteractiveHigh` | High-priority interactive | Live video calls, remote control |
| **3** | `InteractiveLow` | Standard interactive | User interfaces, notifications |
| **4** | `DataHigh` | Important data transfer | Configuration updates, commands |
| **5** | `Data` | Normal data (default) | Regular media streaming, telemetry |
| **6** | `DataLow` | Low-priority data | Logs, historical data |
| **7** | `Background` | Background tasks | File transfers, bulk operations |

**Note**: These priorities only take effect when Zenoh QoS is enabled in the network configuration.

#### Express Mode
- **Enabled**: Bypass internal queues for minimum latency
  - Use for: Ultra-low latency requirements (<1ms additional)
  - Trade-off: Higher CPU usage, potential message reordering
- **Disabled**: Standard processing path
  - Use for: Normal applications where latency is not critical
  - Benefit: Lower CPU usage, guaranteed message ordering

## 🧪 Development & Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test suites  
cargo test --test simple_config_test  # Property configuration tests
cargo test --test integration_tests   # Pipeline integration tests

# With verbose output
cargo test -- --nocapture
```

### Test Coverage

The comprehensive test suite includes:
- ✅ **Element Creation**: Plugin registration and factory tests
- ✅ **Property Validation**: QoS parameter validation and boundary testing
- ✅ **Configuration Management**: Settings validation and runtime property locking
- ✅ **Error Handling**: Network failure recovery and invalid input handling
- ✅ **State Management**: Element lifecycle and transition testing
- ✅ **Integration Testing**: End-to-end pipeline validation

### Code Quality

```bash
# Check code formatting
cargo fmt --check

# Run linting
cargo clippy -- -D warnings

# Run security audit
cargo audit
```

## 🏗️ Architecture

### Session Management

The plugin implements a flexible session architecture supporting both owned and shared Zenoh sessions:

```rust
// Architectural overview
enum SessionWrapper {
    Owned(zenoh::Session),    // Element manages its own session
    Shared(Arc<zenoh::Session>), // Element shares session with others
}
```

This design enables:
- **Resource Efficiency**: Multiple elements can share a single network connection
- **Scalability**: Reduced memory and network overhead for multi-element pipelines  
- **Flexibility**: Mix of shared and dedicated sessions based on requirements

### Thread Safety

All plugin components are designed for safe concurrent access:
- **Mutex-Protected State**: Element state and configuration are thread-safe
- **Lock-Free Data Paths**: Hot paths avoid locking where possible
- **Property Locking**: Runtime configuration changes are safely managed

### Error Handling

Robust error handling throughout the plugin:
- **Network Failures**: Automatic retry and reconnection logic
- **Invalid Configuration**: Graceful degradation with warning messages
- **Resource Exhaustion**: Proper cleanup and resource management
- **GStreamer Integration**: Native GStreamer error reporting

## 🤝 Contributing

### Development Setup

1. **Install Rust**: https://rustup.rs/
2. **Install GStreamer development libraries** (see Quick Start)
3. **Clone and build**:
   ```bash
   git clone https://github.com/your-repo/gst-plugin-zenoh.git
   cd gst-plugin-zenoh
   cargo build
   ```

### Coding Standards

- Follow Rust standard formatting (`cargo fmt`)
- Address all clippy warnings (`cargo clippy`)
- Add tests for new functionality
- Update documentation for API changes
- Follow semantic versioning for releases

### Pull Request Process

1. Fork the repository
2. Create a feature branch
3. Add tests for new functionality  
4. Ensure all tests pass
5. Update documentation
6. Submit pull request with clear description

## 📄 License

This project is licensed under the Mozilla Public License 2.0 - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- [Eclipse Zenoh](https://zenoh.io/) team for the excellent protocol and Rust implementation
- [GStreamer](https://gstreamer.freedesktop.org/) community for the multimedia framework
- [gtk-rs](https://gtk-rs.org/) team for GStreamer Rust bindings
