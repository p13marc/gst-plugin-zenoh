# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A GStreamer plugin that enables distributed media streaming using Zenoh as the transport layer. Provides two elements:
- **zenohsink**: Publishes GStreamer buffers to Zenoh networks
- **zenohsrc**: Subscribes to Zenoh data and delivers it to GStreamer pipelines

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
├── lib.rs              # Plugin registration entry point
├── utils.rs            # Shared utilities
├── error.rs            # ZenohError type with thiserror
├── metadata.rs         # Caps/metadata transmission helpers
├── compression.rs      # Optional compression (zstd/lz4/gzip)
├── zenohsink/
│   ├── mod.rs          # Element registration
│   └── imp.rs          # BaseSink implementation
└── zenohsrc/
    ├── mod.rs          # Element registration
    └── imp.rs          # PushSrc implementation
```

### Key Implementation Details

- **ZenohSink** (`zenohsink/imp.rs`): Extends `gst_base::BaseSink`. On `start()`, creates a Zenoh session and publisher. The `render()` method maps GStreamer buffers and publishes via `publisher.put().wait()`.

- **ZenohSrc** (`zenohsrc/imp.rs`): Extends `gst_base::PushSrc`. On `start()`, creates a Zenoh session and subscriber with FIFO handler. The `create()` method calls `subscriber.recv()` (blocking) to get samples.

- **Synchronous API**: Uses Zenoh's `.wait()` for synchronous operations. No Tokio runtime.

- **State Management**: Simple `Stopped`/`Started(resources)` enum. Resources cleaned up via `Drop`.

- **Caps Transmission**: First buffer sends GStreamer caps as Zenoh attachment metadata (controlled by `send-caps` property).

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
cargo test                              # All tests
cargo test --test plugin_tests          # Element creation, properties
cargo test --test integration_tests     # Pipeline integration
cargo test --test uri_handler_tests     # URI parsing
cargo test --test statistics_tests      # Stats properties
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

Statistics (read-only): `bytes-sent`, `messages-sent`, `errors`, `dropped`

## Dependencies

System packages needed:
```bash
# Ubuntu/Debian
sudo apt-get install libunwind-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev

# Fedora
sudo dnf install libunwind-devel gstreamer1-devel gstreamer1-plugins-base-devel
```
