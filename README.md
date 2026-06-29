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

### Demultiplexing multiple streams (zenohdemux)

`zenohdemux` subscribes to a wildcard key expression and creates one dynamic
source pad per unique key it sees — ideal for fanning many publishers into a
single pipeline:

```bash
# Publishers on distinct keys under demo/cameras/*
gst-launch-1.0 videotestsrc pattern=ball ! zenohsink key-expr=demo/cameras/front
gst-launch-1.0 videotestsrc pattern=snow ! zenohsink key-expr=demo/cameras/back

# One demux subscribes to all of them and plugs each into decodebin
gst-launch-1.0 zenohdemux key-expr=demo/cameras/* \
  ! decodebin ! videoconvert ! autovideosink
```

Pad names follow the `pad-naming` property (`full-path`, `last-segment`, or
`hash`). Colliding names are disambiguated automatically so distinct keys never
share a pad. See the Rust API for `ZenohDemux`/`PadNaming`.

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
- **Statistics**: Real-time monitoring of bytes, messages, and errors

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
| `compression=zstd` | Built without the matching feature | Clear decode error (self-describing frame is detected; no garbage is delivered) |

Each compressed payload carries a small self-describing header (magic + algorithm + version), so a receiver missing the required feature fails with an explicit error instead of forwarding corrupt data.

**Recommendation**: Build both sender and receiver with the same compression features, or use `--features compression` for full compatibility.

## Configuration

By default each element opens a Zenoh session in **peer** mode with multicast
scouting, so senders and receivers on the same LAN discover each other with no
configuration. For other topologies, point the `config` property at a Zenoh
JSON5 config file:

```bash
gst-launch-1.0 zenohsrc key-expr=demo/video config=/etc/zenoh/client.json5 \
  ! videoconvert ! autovideosink
```

A minimal config that connects to a known router instead of relying on
multicast discovery:

```json5
{
  mode: "client",
  connect: {
    endpoints: ["tcp/192.168.1.10:7447"],
  },
}
```

The file accepts the full Zenoh configuration schema (transports, scouting,
QoS, access control, …); see the [Zenoh configuration docs](https://zenoh.io/docs/manual/configuration/).

### Multi-node / router deployment

For deployments that span subnets (where multicast scouting does not reach), run
a `zenohd` router and have each element connect to it via `config`:

```bash
# On the router host
zenohd

# Sender — connects to the router, publishes
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video config=router.json5

# Receiver anywhere that can reach the router
gst-launch-1.0 zenohsrc key-expr=demo/video config=router.json5 \
  ! videoconvert ! autovideosink
```

where `router.json5` contains `{ mode: "client", connect: { endpoints: ["tcp/ROUTER_HOST:7447"] } }`.

## Troubleshooting

| Symptom | Likely cause / fix |
|---------|--------------------|
| `gst-inspect-1.0 zenohsink` says "No such element" | Plugin not on the scan path — `export GST_PLUGIN_PATH=target/release` (or the install dir). |
| Receiver gets no data | Key expressions don't match, or the peers can't discover each other. Verify both sides use intersecting keys and, across subnets, a shared `config` pointing at a router. |
| Pipeline hangs on Ctrl-C with no subscriber | Fixed in 0.5.0 (bounded publish). On older builds, set a finite `publish-timeout-ms`. |
| Receiver logs a compression/decode error | The payload was compressed with an algorithm the receiver wasn't built with — rebuild it with the matching `compression-*` feature (or `--features compression`). |
| `zenohdemux` produces no pads | No samples matched `key-expr` yet; pads are created lazily on first sample per key. |
| Want protocol-level logs | Set `RUST_LOG=zenoh=debug` (and `GST_DEBUG=zenoh*:5` for element logs). |

## Requirements

- Rust 1.88+ (edition 2024)
- GStreamer 1.20+

## License

Mozilla Public License 2.0 - see [LICENSE](LICENSE).
