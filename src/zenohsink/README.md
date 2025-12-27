# ZenohSink

A GStreamer sink element that publishes buffers to Zenoh networks.

## Usage

```bash
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video
```

## Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `key-expr` | String | *required* | Zenoh key expression for publishing |
| `config` | String | `null` | Path to Zenoh configuration file |
| `priority` | Integer | `5` | Priority (1-7, lower=higher). 1=RealTime, 5=Data, 7=Background |
| `reliability` | String | `"best-effort"` | `"best-effort"` or `"reliable"` |
| `congestion-control` | String | `"block"` | `"block"` (wait) or `"drop"` (discard) |
| `express` | Boolean | `false` | Ultra-low latency mode (bypasses queues) |
| `send-caps` | Boolean | `true` | Transmit GStreamer caps as metadata |
| `caps-interval` | Integer | `1` | Seconds between caps retransmission (0=first only) |
| `send-buffer-meta` | Boolean | `true` | Send PTS, DTS, duration, flags |
| `compression` | Enum | `none` | `none`, `zstd`, `lz4`, `gzip` |
| `compression-level` | Integer | `5` | Compression level (1-9) |

### Statistics (read-only)

| Property | Type | Description |
|----------|------|-------------|
| `bytes-sent` | UInt64 | Total bytes published |
| `messages-sent` | UInt64 | Total buffers published |
| `errors` | UInt64 | Publish errors |
| `dropped` | UInt64 | Buffers dropped (congestion-control=drop) |
| `bytes-before-compression` | UInt64 | Bytes before compression |
| `bytes-after-compression` | UInt64 | Bytes after compression |

## Examples

```bash
# Reliable streaming
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video reliability=reliable

# Low-latency with drop on congestion
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video \
  express=true congestion-control=drop priority=2

# With compression
gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video \
  compression=zstd compression-level=5

# URI syntax
gst-launch-1.0 videotestsrc ! \
  zenohsink uri="zenoh:demo/video?priority=2&reliability=reliable"
```

## Rust API

```rust
use gstzenoh::ZenohSink;

// Constructor
let sink = ZenohSink::new("demo/video");

// Builder
let sink = ZenohSink::builder("demo/video")
    .reliability("reliable")
    .priority(2)
    .express(true)
    .compression("zstd")
    .compression_level(5)
    .build();

// Setters
sink.set_reliability("reliable");
sink.set_priority(2);

// Getters
let bytes = sink.bytes_sent();
let msgs = sink.messages_sent();
```
