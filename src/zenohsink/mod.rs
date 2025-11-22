//! # ZenohSink Element
//!
//! The ZenohSink element sends GStreamer buffers over the Zenoh network protocol.
//! It acts as a bridge between GStreamer pipelines and Zenoh networks, enabling
//! distributed media streaming and data sharing across different applications
//! and systems.
//!
//! ## Features
//!
//! * **Quality of Service (QoS) Control**: Configurable reliability and congestion control
//! * **Low Latency Mode**: Express mode for time-critical applications
//! * **Priority Management**: Message prioritization for bandwidth management
//! * **Session Sharing**: Support for shared Zenoh sessions across elements
//! * **Flexible Configuration**: Support for Zenoh config files and runtime parameters
//!
//! ## Properties
//!
//! * `key-expr` - Zenoh key expression for publishing data (required)
//!   - Example: "demo/video/stream" or "sensors/temperature/{device_id}"
//! * `config` - Path to Zenoh configuration file (optional)
//!   - Allows custom Zenoh network configuration (endpoints, discovery, etc.)
//! * `priority` - Publisher priority level (1-7, default: 5)
//!   - 1=RealTime (highest), 2=InteractiveHigh, 3=InteractiveLow, 4=DataHigh, 5=Data(default), 6=DataLow, 7=Background(lowest)
//! * `congestion-control` - Congestion control policy (default: "block")
//!   - `"block"`: Wait for network congestion to clear (ensures delivery)
//!   - `"drop"`: Drop messages during congestion (maintains real-time performance)
//! * `reliability` - Reliability mode (default: "best-effort")
//!   - `"best-effort"`: Fire-and-forget delivery (lower latency)
//!   - `"reliable"`: Acknowledged delivery with retransmission (higher reliability)
//! * `express` - Enable express mode for lower latency (default: false)
//!   - Bypasses some internal queues for reduced end-to-end latency
//!   - May increase CPU usage but improves responsiveness
//!
//! ## Example Pipelines
//!
//! ### Basic Video Streaming
//! ```bash
//! # Simple video streaming
//! gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video/stream
//! ```
//!
//! ### High-Quality Reliable Streaming
//! ```bash
//! # Reliable delivery with high priority and express mode for low latency
//! gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video/reliable \
//!   reliability=reliable congestion-control=block express=true priority=2
//! ```
//!
//! ### Real-Time Streaming with Quality Trade-offs
//! ```bash
//! # Best-effort delivery optimized for real-time performance
//! gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video/realtime \
//!   reliability=best-effort congestion-control=drop express=true
//! ```
//!
//! ### Audio Streaming with Custom Configuration
//! ```bash
//! # Audio with custom Zenoh configuration
//! gst-launch-1.0 audiotestsrc ! audioconvert ! zenohsink \
//!   key-expr=demo/audio/stream config=/path/to/zenoh.json5 priority=4
//! ```
//!
//! ### Encoded Video with H.264
//! ```bash
//! # H.264 encoded video streaming
//! gst-launch-1.0 videotestsrc ! x264enc ! rtph264pay ! zenohsink \
//!   key-expr=demo/video/h264 reliability=reliable
//! ```

use gst::glib;
use gst::prelude::*;

pub mod imp;

glib::wrapper! {
    /// A GStreamer sink element that publishes data via Zenoh.
    ///
    /// This element receives buffers from upstream elements and publishes
    /// them to a Zenoh network using the configured key expression.
    pub struct ZenohSink(ObjectSubclass<imp::ZenohSink>) @extends gst_base::BaseSink, gst::Element, gst::Object, @implements gst::URIHandler;
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "zenohsink",
        gst::Rank::NONE + 100, // Higher than MARGINAL to be discoverable
        ZenohSink::static_type(),
    )
}
