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

## Session Sharing

Multiple elements can share a single Zenoh session to reduce network overhead:

### Using session-group property (gst-launch compatible)

```bash
# Elements with same session-group share a session
gst-launch-1.0 \
  videotestsrc ! zenohsink key-expr=demo/video session-group=main \
  audiotestsrc ! zenohsink key-expr=demo/audio session-group=main
```

### Using Rust API

```rust
use gstzenoh::ZenohSink;
use zenoh::Wait;

let session = zenoh::open(zenoh::Config::default()).wait()?;

let sink1 = ZenohSink::builder("demo/video")
    .session(session.clone())
    .build();

let sink2 = ZenohSink::builder("demo/audio")
    .session(session)
    .build();
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
cargo test                              # All tests (~120 tests)
cargo test --test plugin_tests          # Element creation, properties
cargo test --test integration_tests     # Pipeline integration
cargo test --test uri_handler_tests     # URI parsing, buffer metadata properties
cargo test --test statistics_tests      # Stats properties
cargo test --test zenohdemux_tests      # Demux element tests
cargo test --test data_flow_tests       # End-to-end data transmission
cargo test --test metadata_tests        # Buffer metadata preservation (PTS, DTS, duration)
cargo test --test compression_tests     # Compression round-trip (requires compression feature)
cargo test --test demux_flow_tests      # Demux pad creation and data routing
cargo test --test matching_tests       # Subscriber matching status (has-subscribers, signal, bus message)
```

### Test Architecture Note

**Important:** `gst_check::Harness` cannot be used with zenoh elements because:
- `Harness::new()` calls `gst_harness_play()` which expects `GST_STATE_CHANGE_SUCCESS`
- Zenoh elements return `GST_STATE_CHANGE_ASYNC` (network connection required)
- This causes assertion failures in the GStreamer test harness

Instead, tests use manual pipeline construction with pad probes or direct Zenoh session sharing for data verification. See `TESTING_PLAN.md` for the comprehensive test strategy.

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

All elements share these properties:
- `key-expr` (String, required): Zenoh key expression
- `config` (String): Path to Zenoh JSON5 config file
- `priority` (1-7): Message priority (1=RealTime, 7=Background)
- `reliability`: `"best-effort"` or `"reliable"`
- `congestion-control`: `"block"` or `"drop"`
- `session-group` (String): Session group name for sharing sessions across elements

ZenohSink additional:
- `express` (bool): Ultra-low latency mode
- `send-caps` (bool): Transmit GStreamer caps as metadata
- `caps-interval` (int): Seconds between caps retransmission
- `compression`: `none`, `zstd`, `lz4`, `gzip`
- `compression-level` (1-9): Compression level
- `send-buffer-meta` (bool): Send buffer timing metadata (PTS, DTS, duration, flags)
- `has-subscribers` (bool, read-only): Whether matching Zenoh subscribers currently exist
- Signal `matching-changed(bool)`: Emitted when subscriber presence changes
- Bus message `zenoh-matching-changed`: Posted with `has-subscribers` field on matching changes

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

## Packaging

The project supports building distributable packages for various Linux distributions.

### Building Packages Locally

```bash
# Build binary tarball (works on any Linux)
./scripts/build-tarball.sh

# Build source tarball
./scripts/build-source-tarball.sh

# Build RPM (on Fedora/RHEL)
./scripts/build-rpm.sh

# Build .deb (on Debian/Ubuntu)
./scripts/build-deb.sh

# Build all packages
./scripts/build-packages.sh --all
```

### Cross-Building with Docker

```bash
# Build .deb in Debian container
./scripts/docker-build.sh debian

# Build .deb in Ubuntu container
./scripts/docker-build.sh ubuntu

# Build .rpm in Fedora container
./scripts/docker-build.sh fedora

# Build .rpm in Oracle Linux container
./scripts/docker-build.sh oracle

# Build all packages
./scripts/docker-build.sh all
```

### Package Output

All packages are written to the `dist/` directory:
- `gst-plugin-zenoh-VERSION-linux-ARCH.tar.gz` - Binary tarball
- `gst-plugin-zenoh-VERSION-src.tar.gz` - Source tarball
- `gst-plugin-zenoh_VERSION_amd64.deb` - Debian/Ubuntu package
- `gst-plugin-zenoh-VERSION.x86_64.rpm` - Fedora/RHEL/Oracle package

### Installing from Packages

```bash
# Debian/Ubuntu
sudo dpkg -i dist/gst-plugin-zenoh_*.deb

# Fedora/RHEL/Oracle
sudo rpm -i dist/gst-plugin-zenoh-*.rpm

# Binary tarball
tar xzf dist/gst-plugin-zenoh-*-linux-*.tar.gz
cd gst-plugin-zenoh-*/
./install.sh
```

### GitHub Releases

Tagged releases (v*) automatically build and publish packages via GitHub Actions.
See `.github/workflows/release.yml` for the release workflow.
