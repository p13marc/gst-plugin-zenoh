# GStreamer Zenoh Plugin Examples

This directory contains examples demonstrating how to use the GStreamer Zenoh plugin in various scenarios.

## Prerequisites

Before running the examples, make sure you have:

1. **Rust and Cargo** installed
2. **GStreamer development libraries** installed:
   ```bash
   # On Ubuntu/Debian:
   sudo apt-get install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
   
   # On Fedora/RHEL:
   sudo dnf install gstreamer1-devel gstreamer1-plugins-base-devel
   ```
3. **GStreamer plugins** for multimedia:
   ```bash
   # On Ubuntu/Debian:
   sudo apt-get install gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly
   
   # On Fedora/RHEL:
   sudo dnf install gstreamer1-plugins-good gstreamer1-plugins-bad-free gstreamer1-plugins-ugly-free
   ```

## Building and Running

Build the plugin first:
```bash
cargo build --release
```

Set the GStreamer plugin path:
```bash
export GST_PLUGIN_PATH="target/release:$GST_PLUGIN_PATH"
```

Run any example:
```bash
cargo run --example <example_name>
```

## Available Examples

### 1. Basic Video Streaming (`basic.rs`)
**Complex H.264 video streaming example**

This is the original example that demonstrates H.264 video encoding and streaming through Zenoh.

```bash
cargo run --example basic
```

**Features:**
- H.264 video encoding with `openh264enc`
- RTP packetization
- Real-time video display
- Dual pipeline architecture (sender + receiver)

**Requirements:** GStreamer video plugins, H.264 encoder

### 2. Simple Data Streaming (`simple_data.rs`)
**Basic data transmission example**

Perfect for understanding the fundamentals of the plugin without codec complexity.

```bash
cargo run --example simple_data
```

**Features:**
- Simple test data transmission
- No video/audio codecs required
- Clear logging of data flow
- Minimal dependencies

**Use case:** Learning the plugin basics, testing network connectivity

### 3. Video Streaming (`video_stream.rs`)
**Enhanced video streaming with encoder fallback**

Improved video streaming example with better error handling and encoder options.

```bash
cargo run --example video_stream
```

**Features:**
- Automatic encoder selection (x264enc → openh264enc fallback)
- Dynamic pad linking for `decodebin`
- Better error messages
- SMPTE color bars test pattern

**Requirements:** At least one H.264 encoder (`x264enc` or `openh264enc`)

### 4. Configuration Examples (`configuration.rs`)
**Zenoh property and configuration demonstration**

Shows all available Zenoh configuration options without network streaming.

```bash
cargo run --example configuration
```

**Features:**
- Property inspection and validation
- Config file usage examples
- Reliability settings (reliable vs best-effort)
- Congestion control options (block vs drop)
- Priority levels demonstration
- Temporary config file creation

**Use case:** Understanding plugin configuration, testing setup validation

## Configuration Options

All examples support the following Zenoh properties:

### Common Properties
- `key-expr`: Zenoh key expression for pub/sub (required)
- `config`: Path to Zenoh configuration file (optional)

### Quality of Service
- `reliability`: `"reliable"` or `"best-effort"` (default: `"best-effort"`)
- `congestion-control`: `"block"` or `"drop"` (default: `"block"`)
- `priority`: Integer from -7 to 7 (default: 0)

### Example Configuration File
```json5
{
  "mode": "peer",
  "connect": {
    "endpoints": ["tcp/127.0.0.1:7447"]
  },
  "scouting": {
    "multicast": {
      "enabled": false
    }
  }
}
```

## GStreamer Command Line Usage

You can also use the plugin directly with `gst-launch-1.0`:

```bash
# Simple data sender
gst-launch-1.0 fakesrc num-buffers=100 ! zenohsink key-expr=test/data

# Simple data receiver  
gst-launch-1.0 zenohsrc key-expr=test/data ! fakesink

# Video streaming sender
gst-launch-1.0 videotestsrc ! videoconvert ! x264enc ! zenohsink key-expr=video/stream

# Video streaming receiver
gst-launch-1.0 zenohsrc key-expr=video/stream ! decodebin ! videoconvert ! autovideosink
```

## Troubleshooting

### Plugin Not Found
```
ERROR: No such element or plugin 'zenohsink'
```
**Solution:** Make sure `GST_PLUGIN_PATH` includes your build directory:
```bash
export GST_PLUGIN_PATH="target/release:$GST_PLUGIN_PATH"
```

### Missing GStreamer Elements
```
Error: Failed to create element 'x264enc'
```
**Solution:** Install the required GStreamer plugins:
```bash
# Ubuntu/Debian
sudo apt-get install gstreamer1.0-plugins-ugly

# Fedora/RHEL  
sudo dnf install gstreamer1-plugins-ugly-free
```

### Network Connection Issues
Check that Zenoh can create sessions by running the configuration example first:
```bash
cargo run --example configuration
```

### Permission Issues with Temporary Files
The configuration example creates files in `/tmp/`. Make sure you have write permissions, or modify the example to use a different path.

## Next Steps

- Experiment with different key expressions
- Try various GStreamer elements (audio, different video formats)
- Test network configurations with multiple machines
- Explore Zenoh's advanced features through configuration files

For more information about Zenoh configuration, visit: https://zenoh.io/docs/