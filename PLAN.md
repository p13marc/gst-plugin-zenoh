# Implementation Plan

This document details the implementation plan for the selected improvements.

---

## 1. Configurable Receive Timeout

### Problem

In `src/zenohsrc/imp.rs:538`, the receive timeout is hardcoded:

```rust
match started.subscriber.recv_timeout(Duration::from_millis(100))
```

This 100ms value affects:
- **CPU usage**: Shorter timeout = more loop iterations = higher CPU
- **Responsiveness**: Longer timeout = slower response to flush/stop
- **Latency perception**: Affects how quickly state changes propagate

### Solution

Add a `receive-timeout-ms` property to `zenohsrc`.

### Implementation Steps

#### Step 1: Add property to Settings struct

**File**: `src/zenohsrc/imp.rs`

```rust
struct Settings {
    // ... existing fields ...
    /// Receive timeout in milliseconds for polling Zenoh subscriber
    receive_timeout_ms: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // ... existing fields ...
            receive_timeout_ms: 100,
        }
    }
}
```

#### Step 2: Add GStreamer property definition

**File**: `src/zenohsrc/imp.rs` in `ObjectImpl::properties()`

```rust
glib::ParamSpecUInt64::builder("receive-timeout-ms")
    .nick("Receive Timeout")
    .blurb("Timeout in milliseconds for polling Zenoh subscriber (affects CPU usage vs responsiveness tradeoff)")
    .default_value(100)
    .minimum(10)
    .maximum(5000)
    .build(),
```

#### Step 3: Implement set_property/property

Add handling in `set_property` and `property` methods.

#### Step 4: Use the property value in create()

Replace hardcoded value:

```rust
let receive_timeout_ms = {
    let settings = self.settings.lock().unwrap();
    settings.receive_timeout_ms
};

match started.subscriber.recv_timeout(Duration::from_millis(receive_timeout_ms))
```

### Testing

- Verify property can be set via gst-launch: `zenohsrc receive-timeout-ms=50`
- Verify property appears in gst-inspect
- Test responsiveness with different values
- Measure CPU usage difference between 10ms and 500ms

---

## 2. Buffer Metadata Preservation

### Problem

Currently, only GStreamer caps are transmitted via Zenoh attachments. Important buffer metadata is lost:

- **PTS (Presentation Timestamp)**: When the buffer should be displayed
- **DTS (Decoding Timestamp)**: When the buffer should be decoded
- **Duration**: How long the buffer represents
- **Flags**: Keyframe, delta, discont, header, gap, etc.
- **Offset**: Byte/sample offset in stream

This causes:
- A/V sync issues across network
- Seekability problems
- Decoder confusion (missing keyframe flags)

### Solution

Extend the metadata attachment format to include buffer metadata.

### Metadata Format Extension

Current format:
```
gst.version=1.0
gst.caps=video/x-raw,...
user.custom=value
```

Extended format:
```
gst.version=1.1
gst.caps=video/x-raw,...
gst.pts=1234567890
gst.dts=1234567800
gst.duration=33333333
gst.offset=0
gst.offset-end=1920
gst.flags=live,discont
gst.compression=zstd
user.custom=value
```

### Implementation Steps

#### Step 1: Add new metadata keys

**File**: `src/metadata.rs`

```rust
pub mod keys {
    pub const CAPS: &str = "gst.caps";
    pub const USER_PREFIX: &str = "user.";
    pub const VERSION: &str = "gst.version";
    pub const COMPRESSION: &str = "gst.compression";
    // New keys
    pub const PTS: &str = "gst.pts";
    pub const DTS: &str = "gst.dts";
    pub const DURATION: &str = "gst.duration";
    pub const OFFSET: &str = "gst.offset";
    pub const OFFSET_END: &str = "gst.offset-end";
    pub const FLAGS: &str = "gst.flags";
}

pub const METADATA_VERSION: &str = "1.1";
```

#### Step 2: Extend MetadataBuilder

**File**: `src/metadata.rs`

```rust
#[derive(Debug, Default)]
pub struct MetadataBuilder {
    caps: Option<gst::Caps>,
    pts: Option<gst::ClockTime>,
    dts: Option<gst::ClockTime>,
    duration: Option<gst::ClockTime>,
    offset: Option<u64>,
    offset_end: Option<u64>,
    flags: Option<gst::BufferFlags>,
    user_metadata: HashMap<String, String>,
}

impl MetadataBuilder {
    pub fn buffer_timing(mut self, buffer: &gst::Buffer) -> Self {
        self.pts = buffer.pts();
        self.dts = buffer.dts();
        self.duration = buffer.duration();
        self.offset = Some(buffer.offset());
        self.offset_end = Some(buffer.offset_end());
        self.flags = Some(buffer.flags());
        self
    }
    
    // ... update build() to include new fields ...
}
```

#### Step 3: Extend MetadataParser

**File**: `src/metadata.rs`

```rust
#[derive(Debug, Default)]
pub struct MetadataParser {
    caps: Option<gst::Caps>,
    pts: Option<gst::ClockTime>,
    dts: Option<gst::ClockTime>,
    duration: Option<gst::ClockTime>,
    offset: Option<u64>,
    offset_end: Option<u64>,
    flags: Option<gst::BufferFlags>,
    user_metadata: HashMap<String, String>,
    version: Option<String>,
}

impl MetadataParser {
    pub fn pts(&self) -> Option<gst::ClockTime> { self.pts }
    pub fn dts(&self) -> Option<gst::ClockTime> { self.dts }
    pub fn duration(&self) -> Option<gst::ClockTime> { self.duration }
    pub fn offset(&self) -> Option<u64> { self.offset }
    pub fn offset_end(&self) -> Option<u64> { self.offset_end }
    pub fn flags(&self) -> Option<gst::BufferFlags> { self.flags }
}
```

#### Step 4: Update zenohsink to send buffer metadata

**File**: `src/zenohsink/imp.rs` in `render()`

Add property to control metadata transmission:
```rust
send_buffer_meta: bool  // default: true
```

Update MetadataBuilder usage:
```rust
let metadata_builder = MetadataBuilder::new()
    .caps(&caps)
    .buffer_timing(buffer);  // Add buffer timing info
```

#### Step 5: Update zenohsrc to apply buffer metadata

**File**: `src/zenohsrc/imp.rs` in `create()`

```rust
// After creating buffer, apply metadata
if let Some(pts) = metadata.pts() {
    buffer_mut.set_pts(pts);
}
if let Some(dts) = metadata.dts() {
    buffer_mut.set_dts(dts);
}
if let Some(duration) = metadata.duration() {
    buffer_mut.set_duration(duration);
}
if let Some(offset) = metadata.offset() {
    buffer_mut.set_offset(offset);
}
if let Some(offset_end) = metadata.offset_end() {
    buffer_mut.set_offset_end(offset_end);
}
if let Some(flags) = metadata.flags() {
    buffer_mut.set_flags(flags);
}
```

#### Step 6: Add property to control behavior

**zenohsink**:
```
Property: send-buffer-meta (bool, default: true)
```

**zenohsrc**:
```
Property: apply-buffer-meta (bool, default: true)
```

### Flags Serialization

Buffer flags need string serialization:

```rust
fn flags_to_string(flags: gst::BufferFlags) -> String {
    let mut parts = Vec::new();
    if flags.contains(gst::BufferFlags::LIVE) { parts.push("live"); }
    if flags.contains(gst::BufferFlags::DISCONT) { parts.push("discont"); }
    if flags.contains(gst::BufferFlags::DELTA_UNIT) { parts.push("delta"); }
    if flags.contains(gst::BufferFlags::HEADER) { parts.push("header"); }
    if flags.contains(gst::BufferFlags::GAP) { parts.push("gap"); }
    if flags.contains(gst::BufferFlags::DROPPABLE) { parts.push("droppable"); }
    if flags.contains(gst::BufferFlags::MARKER) { parts.push("marker"); }
    parts.join(",")
}

fn string_to_flags(s: &str) -> gst::BufferFlags {
    let mut flags = gst::BufferFlags::empty();
    for part in s.split(',') {
        match part.trim() {
            "live" => flags |= gst::BufferFlags::LIVE,
            "discont" => flags |= gst::BufferFlags::DISCONT,
            "delta" => flags |= gst::BufferFlags::DELTA_UNIT,
            "header" => flags |= gst::BufferFlags::HEADER,
            "gap" => flags |= gst::BufferFlags::GAP,
            "droppable" => flags |= gst::BufferFlags::DROPPABLE,
            "marker" => flags |= gst::BufferFlags::MARKER,
            _ => {}
        }
    }
    flags
}
```

### Testing

- Unit tests for metadata serialization/deserialization with all fields
- Integration test: verify PTS is preserved end-to-end
- Integration test: verify flags (especially DELTA_UNIT for keyframes) are preserved
- Test backward compatibility: new receiver with old sender (missing fields)

---

## 3. Multi-Key Demuxing for Source

### Problem

Currently, `zenohsrc` can subscribe to wildcard expressions like `camera/**`, but all samples go to a single output pad. There's no way to separate streams by their actual key expression.

### Use Cases

1. **Multi-camera setup**: Subscribe to `camera/*` and route each camera to different sinks
2. **Sensor aggregation**: Subscribe to `sensors/**` and process each sensor type differently
3. **Channel selection**: Subscribe to `stream/*` and dynamically select which to display

### Solution

Create a new `zenohdemux` element that:
- Takes input from `zenohsrc` (or directly subscribes)
- Creates dynamic source pads based on key expressions
- Routes samples to the appropriate pad

### Architecture Options

#### Option A: Separate demux element (Recommended)

```
zenohsrc key-expr="camera/*" ! zenohdemux name=d
    d.camera_1 ! queue ! sink1
    d.camera_2 ! queue ! sink2
```

**Pros**: Clean separation, reusable, follows GStreamer patterns
**Cons**: Extra element in pipeline, needs key-expr passed somehow

#### Option B: Built into zenohsrc with request pads

```
zenohsrc key-expr="camera/*" name=src
    src.camera_1 ! queue ! sink1
    src.camera_2 ! queue ! sink2
```

**Pros**: Simpler pipeline syntax
**Cons**: Complicates zenohsrc significantly

### Chosen Approach: Option A with Enhancement

Use separate `zenohdemux` element, but enhance it to optionally create its own Zenoh subscription (avoiding double element for simple cases):

```
# With zenohsrc (for compatibility, shared session, etc.)
zenohsrc key-expr="camera/*" ! zenohdemux name=d

# Standalone (simpler)
zenohdemux key-expr="camera/*" name=d
    d.camera_1 ! queue ! sink1
```

### Implementation Steps

#### Step 1: Create new element files

**Files**:
- `src/zenohdemux/mod.rs`
- `src/zenohdemux/imp.rs`

#### Step 2: Define the element structure

```rust
pub struct ZenohDemux {
    settings: Mutex<Settings>,
    state: Mutex<State>,
    /// Map of key expression suffix -> src pad
    pads: Mutex<HashMap<String, gst::Pad>>,
}

struct Settings {
    /// Optional key expression (if acting as standalone subscriber)
    key_expr: Option<String>,
    /// Zenoh config file path
    config_file: Option<String>,
    /// Pad naming strategy
    pad_naming: PadNaming,
}

enum PadNaming {
    /// Use full key expression: "camera/front" -> "camera_front"
    FullPath,
    /// Use last segment only: "camera/front" -> "front"
    LastSegment,
    /// Use hash of key expression: "camera/front" -> "pad_a1b2c3"
    Hash,
}
```

#### Step 3: Key expression to pad name conversion

```rust
fn key_expr_to_pad_name(key_expr: &str, naming: PadNaming) -> String {
    match naming {
        PadNaming::FullPath => {
            key_expr.replace('/', "_").replace('*', "wildcard")
        }
        PadNaming::LastSegment => {
            key_expr.split('/').last().unwrap_or("unknown").to_string()
        }
        PadNaming::Hash => {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            key_expr.hash(&mut hasher);
            format!("pad_{:x}", hasher.finish() & 0xFFFFFF)
        }
    }
}
```

#### Step 4: Implement sink pad (for receiving from zenohsrc)

The sink pad receives buffers with key expression in metadata:

```rust
// In metadata.rs, add:
pub const KEY_EXPR: &str = "zenoh.key-expr";
```

Update `zenohsrc` to include key expression in metadata:
```rust
metadata_builder = metadata_builder.user_metadata(
    keys::KEY_EXPR,
    sample.key_expr().as_str()
);
```

#### Step 5: Dynamic pad creation

```rust
fn get_or_create_pad(&self, key_expr: &str) -> gst::Pad {
    let mut pads = self.pads.lock().unwrap();
    let pad_name = key_expr_to_pad_name(key_expr, self.settings.lock().unwrap().pad_naming);
    
    if let Some(pad) = pads.get(&pad_name) {
        return pad.clone();
    }
    
    // Create new pad
    let templ = self.obj().pad_template("src_%s").unwrap();
    let pad = gst::Pad::builder_from_template(&templ)
        .name(&pad_name)
        .build();
    
    // Add pad to element
    self.obj().add_pad(&pad).unwrap();
    
    // Emit pad-added signal
    self.obj().emit_by_name::<()>("pad-added", &[&pad]);
    
    pads.insert(pad_name, pad.clone());
    pad
}
```

#### Step 6: Chain function implementation

```rust
fn sink_chain(&self, pad: &gst::Pad, buffer: gst::Buffer) -> Result<gst::FlowSuccess, gst::FlowError> {
    // Extract key expression from buffer metadata or sample
    let key_expr = self.extract_key_expr(&buffer)?;
    
    // Get or create the appropriate src pad
    let src_pad = self.get_or_create_pad(&key_expr);
    
    // Push buffer to the pad
    src_pad.push(buffer)
}
```

#### Step 7: Standalone mode (optional Zenoh subscription)

If `key-expr` property is set, create internal subscriber:

```rust
fn start(&self) -> Result<(), gst::ErrorMessage> {
    let settings = self.settings.lock().unwrap();
    
    if let Some(key_expr) = &settings.key_expr {
        // Create Zenoh session and subscriber
        // Similar to zenohsrc implementation
    }
    // Otherwise, expect input from sink pad
}
```

#### Step 8: Pad templates

```rust
fn pad_templates() -> &'static [gst::PadTemplate] {
    static PAD_TEMPLATES: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
        // Sink pad (optional, for receiving from zenohsrc)
        let sink_template = gst::PadTemplate::new(
            "sink",
            gst::PadDirection::Sink,
            gst::PadPresence::Sometimes,  // Optional
            &gst::Caps::new_any(),
        ).unwrap();
        
        // Dynamic src pads
        let src_template = gst::PadTemplate::new(
            "src_%s",
            gst::PadDirection::Src,
            gst::PadPresence::Sometimes,
            &gst::Caps::new_any(),
        ).unwrap();
        
        vec![sink_template, src_template]
    });
    PAD_TEMPLATES.as_ref()
}
```

#### Step 9: Register element

**File**: `src/lib.rs`

```rust
mod zenohdemux;

fn plugin_init(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    zenohsink::register(plugin)?;
    zenohsrc::register(plugin)?;
    zenohdemux::register(plugin)?;  // Add this
    Ok(())
}
```

### Properties

| Property | Type | Default | Description |
|----------|------|---------|-------------|
| `key-expr` | String | None | Optional key expression for standalone mode |
| `config` | String | None | Zenoh config file path |
| `pad-naming` | Enum | full-path | How to name pads: full-path, last-segment, hash |

### Testing

- Test with zenohsrc input: multiple streams demuxed correctly
- Test standalone mode: direct Zenoh subscription works
- Test dynamic pad creation: pads created on first sample
- Test pad removal: pads cleaned up on EOS or timeout
- Test with wildcards: `camera/*`, `sensors/**`

---

## 4. Buffer Copies in Render Path (Investigation)

### Problem

**File**: `src/zenohsink/imp.rs` in `render()`

```rust
let b = buffer.clone().into_mapped_buffer_readable().map_err(...)?;
// ...
let data_to_send = b.as_slice().to_vec();  // COPY HERE
```

For high-throughput scenarios:
- 4K30 raw video ≈ 750 MB/s
- 4K60 raw video ≈ 1.5 GB/s
- Each frame copied before sending

### Investigation Needed

#### Question 1: Can we use Zenoh's zero-copy?

Zenoh supports `ZBytes::from(Vec<u8>)` which takes ownership. Current issue:
- We have a `MappedBuffer` (borrowed from GStreamer)
- Zenoh needs owned data or static lifetime

Options to investigate:
1. `ZBytes::from(slice)` - Does this copy internally?
2. `ZSlice` with custom allocator - Complex but truly zero-copy
3. Shared memory (shm feature) - Zero-copy for local peers

#### Question 2: Is the copy actually a bottleneck?

Need benchmarks:
- Profile render() with large buffers
- Compare time in copy vs network send
- Test with/without compression (compression already requires copy)

#### Question 3: GStreamer buffer pool integration?

Could use a custom buffer pool that allocates from Zenoh-compatible memory.

### Potential Solutions

#### Solution A: Accept the copy (simplest)

If benchmarks show copy is <5% of total time, not worth optimizing.

#### Solution B: Use Zenoh shared memory

```rust
// With shm feature enabled
let shm_provider = session.declare_shm_provider(...);
let shm_buffer = shm_provider.alloc(size);
shm_buffer.copy_from_slice(data);
publisher.put(shm_buffer).wait();
```

**Pros**: True zero-copy for local peers
**Cons**: Still copies for remote peers, adds complexity

#### Solution C: Custom GStreamer allocator

Create allocator that uses Zenoh-compatible memory:

```rust
struct ZenohAllocator {
    shm_provider: zenoh::shm::ShmProvider,
}

impl gst::Allocator for ZenohAllocator {
    fn alloc(&self, size: usize) -> gst::Memory {
        // Allocate from Zenoh SHM
    }
}
```

**Pros**: Zero-copy throughout pipeline
**Cons**: High complexity, requires upstream cooperation

### Recommendation

1. **First**: Add benchmarks to measure actual impact
2. **If significant**: Implement Solution B (Zenoh SHM) behind feature flag
3. **Defer**: Solution C for future if needed

### Benchmark Plan

```rust
#[cfg(test)]
mod benchmarks {
    #[test]
    fn bench_render_copy_overhead() {
        // Create pipeline with large buffers
        // Measure time in copy vs total render()
        // Report percentages
    }
}
```

---

## Implementation Order

1. **Configurable Receive Timeout** (1-2 hours)
   - Simple property addition
   - No architectural changes
   - Immediate usability improvement

2. **Buffer Metadata Preservation** (4-6 hours)
   - Extend metadata module
   - Update sink and source
   - Good test coverage needed
   - Backward compatibility consideration

3. **Buffer Copies Investigation** (2-3 hours)
   - Write benchmarks first
   - Decide if optimization needed
   - Implementation depends on findings

4. **Multi-Key Demuxing** (8-12 hours)
   - New element from scratch
   - Dynamic pad management
   - Two modes (standalone + chained)
   - Extensive testing needed

---

## Open Questions

1. **Buffer Metadata**: Should we use Zenoh timestamp OR GStreamer PTS? Currently we use Zenoh timestamp. Options:
   - Always use GStreamer PTS (sender's clock)
   - Always use Zenoh timestamp (network time)
   - Make it configurable

2. **Demux pad cleanup**: When should pads be removed?
   - Never (pads persist for stream lifetime)
   - On EOS from that key expression
   - After timeout of no data
   - Configurable

3. **Demux and caps**: How to handle different caps per stream?
   - Each pad gets its own caps from metadata
   - Caps negotiation per pad
   - What if caps change mid-stream?

4. **Backward compatibility**: When updating metadata version to 1.1:
   - Old receivers ignore unknown keys (safe)
   - New receivers handle missing keys (need defaults)
   - Should we add version negotiation?
