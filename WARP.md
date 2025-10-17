# WARP.md

This file provides guidance to WARP (warp.dev) when working with code in this repository.

## Project Overview

gst-plugin-zenoh is a GStreamer plugin that enables using [Zenoh](https://zenoh.io/) as a transport mechanism for GStreamer pipelines. The plugin is built using Rust, leveraging the zenoh-rs client library and GStreamer's Rust bindings.

The plugin provides two key elements:
- `zenohsink`: A sink element that sends data from GStreamer to Zenoh
- `zenohsrc`: A source element that receives data from Zenoh into GStreamer

## Development Setup

### Dependencies

To work with this codebase, you'll need the following dependencies installed:

```bash
# For Ubuntu/Debian-based distributions:
sudo apt-get update
sudo apt-get install --yes libunwind-dev
sudo apt-get install --yes libgstreamer1.0-0 libgstreamer-plugins-base1.0-dev

# For Fedora/RHEL-based distributions:
sudo dnf install libunwind-devel
sudo dnf install gstreamer1-devel gstreamer1-plugins-base-devel
```

### Common Commands

#### Building the Plugin

```bash
# Build the plugin in debug mode
cargo build

# Build the plugin in release mode
cargo build --release
```

#### Running Tests

```bash
# Run all tests
cargo test

# Run a specific test
cargo test test_name
```

#### Running Examples

The example provided in the repository demonstrates a basic pipeline with the zenohsink and zenohsrc elements:

```bash
# Make sure the plugin is in the GStreamer plugin path
GST_PLUGIN_PATH=target/debug cargo run --example basic
```

#### Cargo Commands

```bash
# Check code without building
cargo check

# Format code
cargo fmt

# Run linter
cargo clippy
```

## Code Architecture

### Plugin Structure

The codebase follows a standard Rust GStreamer plugin architecture:

1. **Main Library Module** (`lib.rs`):
   - Registers the plugin with GStreamer
   - Imports the zenohsink and zenohsrc modules

2. **Utilities** (`utils.rs`):
   - Contains shared code between elements
   - Maintains a shared Tokio runtime for async operations

3. **Zenoh Sink** (`zenohsink/`):
   - `mod.rs`: Defines the GLib wrapper and registration function
   - `imp.rs`: Implements the BaseSink functionality and Zenoh publishing

4. **Zenoh Source** (`zenohsrc/`):
   - `mod.rs`: Defines the GLib wrapper and registration function
   - `imp.rs`: Implements the PushSrc functionality and Zenoh subscription

### Key Components

1. **Zenoh Session Management**:
   - Each element creates and manages its own Zenoh session
   - Sessions are initialized in the `start()` method and cleaned up in `stop()`

2. **GStreamer Integration**:
   - Elements implement the appropriate GStreamer base classes
   - `ZenohSink` extends `BaseSink` for sending data
   - `ZenohSrc` extends `PushSrc` for receiving data

3. **Async Runtime**:
   - Uses Tokio for asynchronous operations
   - A single shared runtime handles all async tasks via a LazyLock

## Element Properties

Both elements expose a `key-expr` property that defines the Zenoh key expression to publish to or subscribe from:

```
# Example usage in a GStreamer pipeline
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video/test
```

```
# Receiving data in another pipeline
gst-launch-1.0 zenohsrc key-expr=demo/video/test ! videoconvert ! autovideosink
```