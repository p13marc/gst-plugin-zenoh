# gst-plugin-zenoh

[![Crates.io](https://img.shields.io/crates/v/gst-plugin-zenoh.svg)](https://crates.io/crates/gst-plugin-zenoh)
[![Documentation](https://docs.rs/gst-plugin-zenoh/badge.svg)](https://docs.rs/gst-plugin-zenoh)
[![License](https://img.shields.io/badge/License-MPL--2.0-blue.svg)](https://opensource.org/licenses/MPL-2.0)

A [GStreamer](https://gstreamer.freedesktop.org/) plugin for distributed media streaming using [Zenoh](https://zenoh.io/).

## Elements

| Element | Description | Documentation |
|---------|-------------|---------------|
| **zenohsink** | Publishes GStreamer buffers to Zenoh | [README](src/zenohsink/README.md) |
| **zenohsrc** | Subscribes to Zenoh and delivers to pipelines | [README](src/zenohsrc/README.md) |
| **zenohdemux** | Demultiplexes streams by key expression | [README](src/zenohdemux/README.md) |

## Quick Start

### Installation

```bash
# Ubuntu/Debian
sudo apt-get install libunwind-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev

# Fedora
sudo dnf install libunwind-devel gstreamer1-devel gstreamer1-plugins-base-devel

# Build
cargo build --release

# With compression support
cargo build --release --features compression
```

### Basic Usage

```bash
# Set plugin path
export GST_PLUGIN_PATH=target/release

# Sender
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video

# Receiver
gst-launch-1.0 zenohsrc key-expr=demo/video ! videoconvert ! autovideosink
```

## Features

- **QoS Control**: Reliability modes, congestion control, priority levels (1-7)
- **Low Latency**: Express mode, zero-copy paths, efficient session management
- **Subscriber Matching**: Detect subscriber presence via `has-subscribers` property, `matching-changed` signal, and bus messages
- **On-Demand Pipelines**: Start/stop pipelines based on subscriber presence — conserve resources when no one is listening
- **Session Sharing**: Share Zenoh sessions across elements to reduce overhead
- **Compression**: Optional Zstandard, LZ4, or Gzip (compile-time features)
- **Buffer Metadata**: PTS, DTS, duration, flags preserved for A/V sync
- **Caps Transmission**: Automatic format negotiation between sender/receiver
- **URI Handler**: Configure via `zenoh:key-expr?priority=2&reliability=reliable`
- **Statistics**: Real-time monitoring of bytes, messages, errors, dropped packets

## Rust API

```rust
use gstzenoh::{ZenohSink, ZenohSrc, ZenohDemux, PadNaming};

// Simple constructor
let sink = ZenohSink::new("demo/video");

// Builder pattern
let sink = ZenohSink::builder("demo/video")
    .reliability("reliable")
    .priority(2)
    .express(true)
    .build();

// Typed getters
println!("Sent: {} bytes", sink.bytes_sent());
```

See [docs.rs](https://docs.rs/gst-plugin-zenoh) for full API documentation.

## On-Demand Pipelines

Detect subscriber presence and start/stop pipelines automatically. The pipeline stays in READY (Zenoh resources active, no data flowing) until a subscriber connects:

```bash
# gst-launch: watch for matching changes via bus messages
gst-launch-1.0 videotestsrc is-live=true ! zenohsink key-expr=demo/video
# Bus posts "zenoh-matching-changed" messages with has-subscribers field
```

```rust
use gstzenoh::ZenohSink;

let sink = ZenohSink::builder("demo/video").build();

// React to subscriber presence changes
let pipeline_weak = pipeline.downgrade();
sink.connect_matching_changed(move |_sink, has_subscribers| {
    let Some(pipeline) = pipeline_weak.upgrade() else { return };
    if has_subscribers {
        let _ = pipeline.set_state(gst::State::Playing);
    } else {
        let _ = pipeline.set_state(gst::State::Ready);
    }
});

// Start in READY — matching detection works, no data flows
pipeline.set_state(gst::State::Ready)?;
```

See `examples/on_demand.rs` for a complete example.

## Compression

Build with compression features:

```bash
cargo build --release --features compression-zstd  # Zstandard (recommended)
cargo build --release --features compression-lz4   # LZ4 (fastest)
cargo build --release --features compression-gzip  # Gzip (compatible)
cargo build --release --features compression       # All algorithms
```

Usage:

```bash
# Sender with compression
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video compression=zstd

# Receiver (auto-decompresses)
gst-launch-1.0 zenohsrc key-expr=demo/video ! videoconvert ! autovideosink
```

### Feature Compatibility

| Sender | Receiver | Result |
|--------|----------|--------|
| `compression=none` | Any build | Works |
| `compression=zstd` | Built with `compression-zstd` | Works |
| `compression=zstd` | Built without compression | Error logged, raw compressed bytes delivered |

**Recommendation**: Build both sender and receiver with the same compression features, or use `--features compression` for full compatibility.

## Requirements

- Rust 1.85+ (edition 2024)
- GStreamer 1.20+

## License

Mozilla Public License 2.0 - see [LICENSE](LICENSE).
