# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A GStreamer plugin that enables distributed media streaming using Zenoh as the transport layer. Provides three elements:
- **zenohsink**: Publishes GStreamer buffers to Zenoh networks
- **zenohsrc**: Subscribes to Zenoh data and delivers it to GStreamer pipelines
- **zenohdemux**: Demultiplexes Zenoh streams by key expression, creating dynamic pads for each unique key

## Build Commands

```bash
# Basic build
cargo build --release

# With compression support
cargo build --release --features compression          # All algorithms
cargo build --release --features compression-zstd     # Zstandard only
cargo build --release --features compression-lz4      # LZ4 only
cargo build --release --features compression-gzip     # Gzip only

# Run tests
cargo test

# Run specific test file
cargo test --test plugin_tests
cargo test --test integration_tests

# Run examples (must set plugin path)
GST_PLUGIN_PATH=target/debug cargo run --example basic
GST_PLUGIN_PATH=target/debug cargo run --example configuration

# Code quality
cargo fmt --check
cargo clippy -- -D warnings
```

## Architecture

```
src/
├── lib.rs              # Plugin registration entry point, re-exports main types
├── utils.rs            # Shared utilities
├── error.rs            # ZenohError type with thiserror
├── metadata.rs         # Caps/metadata transmission helpers (includes buffer timing)
├── compression.rs      # Optional compression (zstd/lz4/gzip)
├── zenohsink/
│   ├── mod.rs          # Element registration and strongly-typed API (ZenohSink, ZenohSinkBuilder)
│   └── imp.rs          # BaseSink implementation
├── zenohsrc/
│   ├── mod.rs          # Element registration and strongly-typed API (ZenohSrc, ZenohSrcBuilder)
│   └── imp.rs          # PushSrc implementation
└── zenohdemux/
    ├── mod.rs          # Element registration and strongly-typed API (ZenohDemux, ZenohDemuxBuilder, PadNaming)
    └── imp.rs          # Element implementation with dynamic pads
```

### Key Implementation Details

- **ZenohSink** (`zenohsink/imp.rs`): Extends `gst_base::BaseSink`. On `start()`, creates a Zenoh session and publisher. The `render()` method maps GStreamer buffers and publishes via `publisher.put().wait()`.

- **ZenohSrc** (`zenohsrc/imp.rs`): Extends `gst_base::PushSrc`. On `start()`, creates a Zenoh session and subscriber with FIFO handler. The `create()` method uses `subscriber.recv_timeout()` (configurable via `receive-timeout-ms`) to get samples. Supports buffer metadata restoration via `apply-buffer-meta` property.

- **ZenohDemux** (`zenohdemux/imp.rs`): Extends `gst::Element`. Creates dynamic source pads based on incoming key expressions. Uses a receiver thread for Zenoh subscription. Supports three pad naming strategies: `full-path`, `last-segment`, and `hash`. Attaches key expression as buffer metadata.

- **Synchronous API**: Uses Zenoh's `.wait()` for synchronous operations. No Tokio runtime.

- **State Management**: Simple `Stopped`/`Started(resources)` enum. Resources cleaned up via `Drop`.

- **Caps Transmission**: First buffer sends GStreamer caps as Zenoh attachment metadata (controlled by `send-caps` property).

- **Buffer Metadata**: PTS, DTS, duration, offset, and flags can be transmitted via Zenoh attachments (`send-buffer-meta` on sink, `apply-buffer-meta` on src/demux). Uses `metadata.rs` with versioned format (v1.0).

- **Zero-Copy Optimization**: When compression is disabled, `render()` uses `Cow::Borrowed` to avoid copying buffer data.

## Strongly-Typed Rust API

Main types are re-exported at crate root for convenience:

```rust
use gstzenoh::{ZenohSink, ZenohSinkBuilder, ZenohSrc, ZenohSrcBuilder, ZenohDemux, ZenohDemuxBuilder, PadNaming};
```

Each element provides:
- **Constructor**: `ZenohSink::new("key-expr")` - creates element with required key expression
- **Builder**: `ZenohSink::builder("key-expr").reliability("reliable").build()` - fluent configuration
- **Typed setters**: `sink.set_reliability("reliable")`, `sink.set_priority(2)`
- **Typed getters**: `sink.key_expr()`, `sink.bytes_sent()`, `sink.messages_sent()`
- **TryFrom conversion**: `ZenohSink::try_from(element)` - convert from `gst::Element`

Example:

```rust
use gstzenoh::ZenohSink;

let sink = ZenohSink::builder("demo/video")
    .reliability("reliable")
    .priority(2)
    .express(true)
    .build();

// Access statistics with typed getters
println!("Sent: {} bytes, {} messages", sink.bytes_sent(), sink.messages_sent());
```

## Feature Flags

| Feature | Purpose |
|---------|---------|
| `compression-zstd` | Zstandard compression |
| `compression-lz4` | LZ4 compression |
| `compression-gzip` | Gzip compression |
| `compression` | All compression algorithms |

## Testing

Tests use `serial_test` for isolation since GStreamer plugin registration is global:

```bash
cargo test                              # All tests (101 tests)
cargo test --test plugin_tests          # Element creation, properties
cargo test --test integration_tests     # Pipeline integration
cargo test --test uri_handler_tests     # URI parsing, buffer metadata properties
cargo test --test statistics_tests      # Stats properties
cargo test --test zenohdemux_tests      # Demux element tests
```

## Common Development Tasks

### Testing with gst-launch

```bash
# Set plugin path
export GST_PLUGIN_PATH=target/debug

# Sender
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video

# Receiver
gst-launch-1.0 zenohsrc key-expr=demo/video ! videoconvert ! autovideosink
```

### Inspecting elements

```bash
GST_PLUGIN_PATH=target/debug gst-inspect-1.0 zenohsink
GST_PLUGIN_PATH=target/debug gst-inspect-1.0 zenohsrc
```

## Element Properties

Both elements share these properties:
- `key-expr` (String, required): Zenoh key expression
- `config` (String): Path to Zenoh JSON5 config file
- `priority` (1-7): Message priority (1=RealTime, 7=Background)
- `reliability`: `"best-effort"` or `"reliable"`
- `congestion-control`: `"block"` or `"drop"`

ZenohSink additional:
- `express` (bool): Ultra-low latency mode
- `send-caps` (bool): Transmit GStreamer caps as metadata
- `caps-interval` (int): Seconds between caps retransmission
- `compression`: `none`, `zstd`, `lz4`, `gzip`
- `compression-level` (1-9): Compression level
- `send-buffer-meta` (bool): Send buffer timing metadata (PTS, DTS, duration, flags)

ZenohSrc additional:
- `receive-timeout-ms` (int): Timeout for receiving samples (default: 1000)
- `apply-buffer-meta` (bool): Apply buffer timing from Zenoh attachments

ZenohDemux additional:
- `pad-naming`: `full-path`, `last-segment`, or `hash`
- `apply-buffer-meta` (bool): Apply buffer timing from Zenoh attachments

Statistics (read-only): `bytes-sent`/`bytes-received`, `messages-sent`/`messages-received`, `errors`, `dropped`, `pads-created` (demux only)

## Dependencies

System packages needed:
```bash
# Ubuntu/Debian
sudo apt-get install libunwind-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev

# Fedora
sudo dnf install libunwind-devel gstreamer1-devel gstreamer1-plugins-base-devel
```
