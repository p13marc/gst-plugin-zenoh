# ZenohDemux

A GStreamer element that demultiplexes Zenoh streams by key expression, creating dynamic source pads for each unique key.

## Usage

```bash
gst-launch-1.0 zenohdemux key-expr="sensors/**" name=demux \
  demux. ! queue ! fakesink
```

## Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `key-expr` | String | *required* | Zenoh key expression (supports wildcards) |
| `config` | String | `null` | Path to Zenoh configuration file |
| `priority` | Integer | `5` | Priority (1-7, lower=higher) |
| `reliability` | String | `"best-effort"` | Expected reliability mode |
| `pad-naming` | Enum | `full-path` | Pad naming strategy (see below) |
| `apply-buffer-meta` | Boolean | `true` | Apply PTS, DTS, duration, flags from sender |

### Pad Naming Strategies

| Value | Description | Example |
|-------|-------------|---------|
| `full-path` | Full key expression | `sensors_device1_temperature` |
| `last-segment` | Last path segment only | `temperature` |
| `hash` | Hash of key expression | `a1b2c3d4` |

### Statistics (read-only)

| Property | Type | Description |
|----------|------|-------------|
| `bytes-received` | UInt64 | Total bytes received |
| `messages-received` | UInt64 | Total buffers received |
| `errors` | UInt64 | Receive errors |
| `pads-created` | UInt64 | Dynamic pads created |

## Examples

```bash
# Demux all sensors with full-path naming
gst-launch-1.0 zenohdemux key-expr="sensors/**" name=demux \
  demux. ! queue ! filesink location=data.bin

# Use last-segment naming for cleaner pad names
gst-launch-1.0 zenohdemux key-expr="sensors/**" pad-naming=last-segment name=demux \
  demux. ! queue ! fakesink

# Multi-camera demux with hash naming
gst-launch-1.0 zenohdemux key-expr="cameras/**" pad-naming=hash name=demux \
  demux. ! queue ! videoconvert ! autovideosink
```

## How It Works

1. Subscribe to a wildcard key expression (e.g., `sensors/**`)
2. For each unique key received, create a new source pad
3. Route data to the appropriate pad based on its key expression
4. Downstream elements can connect to specific pads

## Rust API

```rust
use gstzenoh::{ZenohDemux, PadNaming};

// Constructor
let demux = ZenohDemux::new("sensors/**");

// Builder
let demux = ZenohDemux::builder("sensors/**")
    .pad_naming(PadNaming::LastSegment)
    .apply_buffer_meta(true)
    .build();

// Setters
demux.set_pad_naming(PadNaming::Hash);

// Getters
let pads = demux.pads_created();
let bytes = demux.bytes_received();
```
