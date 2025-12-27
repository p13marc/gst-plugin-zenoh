# ZenohSrc

A GStreamer source element that subscribes to Zenoh and delivers data to pipelines.

## Usage

```bash
gst-launch-1.0 zenohsrc key-expr=demo/video ! videoconvert ! autovideosink
```

## Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `key-expr` | String | *required* | Zenoh key expression (supports wildcards: `*`, `**`) |
| `config` | String | `null` | Path to Zenoh configuration file |
| `priority` | Integer | `5` | Priority (1-7, lower=higher). 1=RealTime, 5=Data, 7=Background |
| `reliability` | String | `"best-effort"` | Expected reliability (actual mode matches publisher) |
| `congestion-control` | String | `"block"` | Informational only |
| `receive-timeout-ms` | Integer | `1000` | Timeout for receiving samples |
| `apply-buffer-meta` | Boolean | `true` | Apply PTS, DTS, duration, flags from sender |

### Statistics (read-only)

| Property | Type | Description |
|----------|------|-------------|
| `bytes-received` | UInt64 | Total bytes received |
| `messages-received` | UInt64 | Total buffers received |
| `errors` | UInt64 | Receive errors |
| `dropped` | UInt64 | Samples dropped |

## Examples

```bash
# Basic subscription
gst-launch-1.0 zenohsrc key-expr=demo/video ! videoconvert ! autovideosink

# Wildcard subscription
gst-launch-1.0 zenohsrc key-expr="sensors/**" ! fakesink

# Single-level wildcard
gst-launch-1.0 zenohsrc key-expr="demo/*/video" ! fakesink

# Custom timeout
gst-launch-1.0 zenohsrc key-expr=demo/video receive-timeout-ms=500 ! fakesink

# URI syntax
gst-launch-1.0 zenohsrc uri="zenoh:demo/video?priority=2" ! fakesink
```

## Wildcards

| Pattern | Matches |
|---------|---------|
| `demo/*` | `demo/video`, `demo/audio` (single level) |
| `demo/**` | `demo/video`, `demo/a/b/c` (any depth) |
| `**/video` | `demo/video`, `a/b/video` (any prefix) |

## Rust API

```rust
use gstzenoh::ZenohSrc;

// Constructor
let src = ZenohSrc::new("demo/video");

// Builder
let src = ZenohSrc::builder("sensors/**")
    .receive_timeout_ms(500)
    .apply_buffer_meta(true)
    .build();

// Setters
src.set_receive_timeout_ms(2000);

// Getters
let bytes = src.bytes_received();
let msgs = src.messages_received();
```
