# gst-plugin-zenoh

This is a [GStreamer](https://gstreamer.freedesktop.org/) plugin for using [Zenoh](https://zenoh.io/) as the transport build using [zenoh-rs](https://github.com/eclipse-zenoh/zenoh).

The plugin provides two key elements:
- `zenohsink`: A sink element that sends data from GStreamer to Zenoh
- `zenohsrc`: A source element that receives data from Zenoh into GStreamer

## Features

- **Configurable Quality of Service (QoS)**: Support for different reliability modes (best-effort, reliable) and congestion control policies (block, drop)
- **Express Mode**: Low-latency mode that bypasses some internal queues for faster delivery
- **Priority Control**: Configurable message priorities from -100 to 100
- **Flexible Configuration**: Support for both default configuration and external Zenoh config files
- **Session Sharing**: Architecture prepared for sharing Zenoh sessions between multiple elements (external session support)

## Examples

### Basic Usage

```bash
# Run the basic video streaming example
GST_PLUGIN_PATH=target/debug cargo run --example basic

# Run the configuration example to see all QoS options
GST_PLUGIN_PATH=target/debug cargo run --example configuration
```

### GStreamer Pipeline Examples

```bash
# Basic data streaming
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video/stream

# With reliability and express mode for low latency
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video/reliable \
  reliability=reliable congestion-control=block express=true priority=10

# Best-effort with drop congestion control
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video/besteffort \
  reliability=best-effort congestion-control=drop

# Receiving data
gst-launch-1.0 zenohsrc key-expr=demo/video/stream ! videoconvert ! autovideosink
```

## Properties

### ZenohSink Properties

- `key-expr` (string): Zenoh key expression for publishing data (required)
- `config` (string): Path to Zenoh configuration file (optional)
- `priority` (int): Publisher priority (-100 to 100, default: 0)
- `congestion-control` (string): "block" or "drop" (default: "block")
- `reliability` (string): "best-effort" or "reliable" (default: "best-effort")
- `express` (boolean): Enable express mode for lower latency (default: false)

### ZenohSrc Properties

- `key-expr` (string): Zenoh key expression for receiving data (required)
- `config` (string): Path to Zenoh configuration file (optional)
- `priority` (int): Subscriber priority (-100 to 100, default: 0)
- `congestion-control` (string): "block" or "drop" (default: "block")
- `reliability` (string): "best-effort" or "reliable" (default: "best-effort")

## Testing

Run the unit tests using cargo nextest:
```bash
cargo nextest run
```

Or use regular cargo test:
```bash
cargo test
```

The test suite includes:
- Plugin registration and element creation tests
- Property validation and setting tests 
- Error handling and edge case tests
- Pipeline integration tests

All tests are designed to run without requiring a running Zenoh network.
