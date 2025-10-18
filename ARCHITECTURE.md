# GStreamer Zenoh Plugin Architecture

This document provides a comprehensive overview of the gst-plugin-zenoh architecture, explaining how the plugin integrates GStreamer with Zenoh for distributed media streaming.

## Table of Contents

- [Overview](#overview)
- [Plugin Structure](#plugin-structure)
- [Component Architecture](#component-architecture)
- [Data Flow](#data-flow)
- [State Management](#state-management)
- [Error Handling](#error-handling)
- [Configuration System](#configuration-system)
- [Threading Model](#threading-model)
- [Integration Points](#integration-points)

## Overview

The GStreamer Zenoh plugin enables GStreamer pipelines to send and receive media data over the network using the [Zenoh](https://zenoh.io/) protocol. Zenoh provides a unified data plane for IoT, edge computing, and cloud applications with features like:

- **Publish/Subscribe messaging**: Decoupled communication between producers and consumers
- **Efficient networking**: Optimized for low latency and high throughput
- **Automatic discovery**: Dynamic peer discovery and routing
- **Quality of Service**: Configurable reliability, priority, and congestion control

The plugin consists of two main elements:
- **`zenohsink`**: Publishes GStreamer data to Zenoh network
- **`zenohsrc`**: Subscribes to Zenoh data and provides it to GStreamer pipelines

## Plugin Structure

```
src/
├── lib.rs                  # Plugin registration and entry point
├── utils.rs                # Shared utilities and runtime management
├── error.rs                # Error types and handling
├── zenohsink/
│   ├── mod.rs             # ZenohSink element definition and registration
│   └── imp.rs             # ZenohSink implementation (BaseSink)
└── zenohsrc/
    ├── mod.rs             # ZenohSrc element definition and registration
    └── imp.rs             # ZenohSrc implementation (PushSrc)
```

### Module Responsibilities

- **`lib.rs`**: Plugin entry point, registers elements with GStreamer
- **`utils.rs`**: Shared utilities, currently minimal but extensible
- **`error.rs`**: Centralized error handling with domain-specific error types
- **`zenohsink/`**: Sink element that publishes data to Zenoh
- **`zenohsrc/`**: Source element that receives data from Zenoh

## Component Architecture

### ZenohSink Architecture

```
GStreamer Pipeline → ZenohSink → Zenoh Session → Zenoh Publisher → Network
```

**Key Components:**

1. **BaseSink Implementation**: Inherits from `gst_base::BaseSink` for standard sink behavior
2. **Settings**: Thread-safe configuration storage (`Mutex<Settings>`)
3. **State**: Runtime state management (`Mutex<State>`)
4. **Zenoh Publisher**: Handles data publication to Zenoh network

**State Transitions:**
- `Stopped` → `Started`: Creates Zenoh session and publisher
- `Started` → `Stopped`: Cleans up resources (automatic via Drop)

### ZenohSrc Architecture

```
Network → Zenoh Subscriber → Zenoh Session → ZenohSrc → GStreamer Pipeline
```

**Key Components:**

1. **PushSrc Implementation**: Inherits from `gst_base::PushSrc` for active source behavior
2. **Settings**: Thread-safe configuration storage (`Mutex<Settings>`)
3. **State**: Runtime state management (`Mutex<State>`)
4. **Zenoh Subscriber**: Receives data from Zenoh network using FIFO channel handler

**State Transitions:**
- `Stopped` → `Started`: Creates Zenoh session and subscriber
- `Started` → `Stopped`: Cleans up resources (automatic via Drop)

## Data Flow

### Sink Data Flow

```mermaid
graph LR
    A[GStreamer Buffer] --> B[render()]
    B --> C[Map Buffer]
    C --> D[Zenoh Publisher]
    D --> E[publisher.put()]
    E --> F[Zenoh Network]
```

**Steps:**
1. GStreamer calls `render()` with a `gst::Buffer`
2. Buffer is mapped to readable memory
3. Raw bytes are extracted from buffer
4. Zenoh publisher sends data using synchronous `put().wait()`
5. Data propagates through Zenoh network to subscribers

### Source Data Flow

```mermaid
graph LR
    A[Zenoh Network] --> B[Zenoh Subscriber]
    B --> C[subscriber.recv()]
    C --> D[create()]
    D --> E[Zenoh Sample]
    E --> F[Extract Payload]
    F --> G[Create GStreamer Buffer]
    G --> H[GStreamer Pipeline]
```

**Steps:**
1. Zenoh subscriber receives data from network
2. `create()` is called by GStreamer
3. `subscriber.recv()` retrieves next sample (blocking)
4. Sample payload is extracted as bytes
5. New `gst::Buffer` is created with payload data
6. Buffer is passed to downstream GStreamer elements

## State Management

### State Enum Design

Both elements use a simple state enum:

```rust
enum State {
    Stopped,                    // Initial state, no resources allocated
    Started(Started),           // Active state with Zenoh resources
}

struct Started {
    session: zenoh::Session,    // Zenoh session (kept for ownership)
    publisher: zenoh::Publisher, // or subscriber for ZenohSrc
}
```

### State Transition Logic

**Start Sequence:**
1. Validate configuration (key expression required)
2. Load Zenoh configuration (file or default)
3. Create Zenoh session using `zenoh::open(config).wait()`
4. Create publisher/subscriber using `session.declare_*().wait()`
5. Store resources in `Started` state

**Stop Sequence:**
1. Replace state with `Stopped`
2. Resources automatically cleaned up via `Drop` implementations
3. No explicit cleanup code needed

### Thread Safety

- All shared state protected by `Mutex`
- Lock acquisition order prevents deadlocks
- Short critical sections minimize contention
- No shared mutable state between elements

## Error Handling

### Error Type Hierarchy

```rust
#[derive(Debug, thiserror::Error)]
pub enum ZenohError {
    #[error("Zenoh initialization error: {0}")]
    InitError(#[from] zenoh::Error),
    
    #[error("Key expression error: {0}")]
    KeyExprError(String),
    
    #[error("Publish error: {0}")]
    PublishError(#[from] zenoh::PublishError),
    
    #[error("Receive error: {0}")]
    ReceiveError(String),
}
```

### Error Propagation

1. **Zenoh Errors** → `ZenohError` → `gst::ErrorMessage` → GStreamer
2. **Network Detection**: Specific handling for timeout/connection errors
3. **User-Friendly Messages**: Clear error descriptions for common issues
4. **Proper Error Classification**: Resource, settings, or stream errors

### Error Recovery

- **Session Failures**: Element transitions to error state
- **Network Issues**: Logged as resource errors
- **Configuration Errors**: Validated during property setting
- **Runtime Errors**: Propagated to GStreamer bus

## Configuration System

### Property System

Both elements expose identical properties for consistency:

| Property | Type | Description | Default |
|----------|------|-------------|---------|
| `key-expr` | String | Zenoh key expression | `""` (required) |
| `config` | String | Zenoh config file path | `None` |
| `priority` | Int | Message priority (-100 to 100) | `0` |
| `congestion-control` | String | `"block"` or `"drop"` | `"block"` |
| `reliability` | String | `"reliable"` or `"best-effort"` | `"best-effort"` |

### Configuration Loading

```rust
let config = match config_file {
    Some(path) if !path.is_empty() => {
        zenoh::Config::from_file(&path)?
    }
    _ => zenoh::Config::default(),
};
```

### Property Validation

- **Key Expression**: Must not be empty
- **Config File**: Path existence checked by Zenoh
- **Enum Values**: Validated against known options
- **Priority Range**: Clamped to valid bounds

## Threading Model

### Simplified Architecture

The plugin uses a **simplified synchronous model**:

- **No Background Threads**: All operations use Zenoh's synchronous API
- **No Async Runtime**: Removed Tokio dependency for simplicity
- **Blocking Operations**: Use `.wait()` for synchronous operation
- **GStreamer Threading**: Relies on GStreamer's thread management

### Thread Safety Guarantees

- **Mutex Protection**: All mutable state protected
- **Send + Sync**: All shared types implement thread safety traits
- **No Data Races**: Careful lock ordering prevents deadlocks
- **Resource Cleanup**: Automatic via Rust's ownership system

## Integration Points

### GStreamer Integration

**Element Registration:**
```rust
fn plugin_init(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    zenohsink::register(plugin)?;
    zenohsrc::register(plugin)?;
    Ok(())
}
```

**Base Class Integration:**
- `zenohsink`: Extends `gst_base::BaseSink`
- `zenohsrc`: Extends `gst_base::PushSrc`

### Zenoh Integration

**Session Management:**
```rust
// Synchronous session creation
let session = zenoh::open(config).wait()?;

// Publisher/Subscriber creation
let publisher = session.declare_publisher(key_expr).wait()?;
let subscriber = session.declare_subscriber(key_expr).wait()?;
```

**Data Operations:**
```rust
// Publishing (zenohsink)
publisher.put(data).wait()?;

// Receiving (zenohsrc)
let sample = subscriber.recv()?;
```

### Caps Integration

- **Any Caps**: Elements accept/produce any media type
- **No Negotiation**: Pass-through caps handling
- **Format Preservation**: Raw byte transmission preserves all data

## Usage Patterns

### Basic Pipeline

```bash
# Sender
gst-launch-1.0 videotestsrc ! zenohsink key-expr=video/stream

# Receiver
gst-launch-1.0 zenohsrc key-expr=video/stream ! autovideosink
```

### Complex Pipeline

```bash
# With encoding
gst-launch-1.0 videotestsrc ! videoconvert ! x264enc ! \
  zenohsink key-expr=encoded/video

# With decoding
gst-launch-1.0 zenohsrc key-expr=encoded/video ! \
  decodebin ! videoconvert ! autovideosink
```

### Configuration Examples

```bash
# With config file
gst-launch-1.0 videotestsrc ! \
  zenohsink key-expr=video config=/path/to/zenoh.json5

# With QoS settings
gst-launch-1.0 videotestsrc ! \
  zenohsink key-expr=video reliability=reliable priority=5
```

## Performance Considerations

### Optimization Strategies

1. **Zero-Copy**: Minimize buffer copying where possible
2. **Efficient Serialization**: Direct byte transmission
3. **Resource Pooling**: Future enhancement for buffer management
4. **Network Efficiency**: Leverage Zenoh's optimized transport

### Current Limitations

- **Buffer Copies**: Some copying unavoidable in current design
- **Synchronous API**: May limit throughput vs async operations
- **No Compression**: Raw data transmission (can be added upstream)

### Future Optimizations

- **Buffer Management**: Custom allocators and pooling
- **Metadata Support**: Zenoh metadata for additional information
- **Compression**: Integration with GStreamer compression elements
- **Statistics**: Performance monitoring and reporting

## Testing Architecture

### Test Structure

```
tests/
├── plugin_tests.rs        # Basic plugin functionality
├── error_tests.rs         # Error handling scenarios  
└── integration_tests.rs   # Cross-element integration
```

### Test Categories

1. **Unit Tests**: Individual element behavior
2. **Property Tests**: Configuration validation
3. **State Tests**: State transition handling
4. **Integration Tests**: End-to-end pipeline testing
5. **Error Tests**: Error condition handling

This architecture provides a solid foundation for reliable, efficient media streaming over Zenoh networks while maintaining compatibility with the broader GStreamer ecosystem.