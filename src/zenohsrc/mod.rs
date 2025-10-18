//! # ZenohSrc Element
//!
//! The ZenohSrc element receives data from the Zenoh network protocol and outputs GStreamer buffers.
//! 
//! ## Properties
//!
//! * `key-expr` - Zenoh key expression for subscribing to data (required)
//! * `config` - Path to Zenoh configuration file (optional)
//! * `priority` - Subscriber priority (-100 to 100, default: 0)
//! * `congestion-control` - Congestion control policy: "block" or "drop" (default: "block")
//! * `reliability` - Reliability mode: "best-effort" or "reliable" (default: "best-effort")
//!
//! ## Example Pipeline
//! 
//! ```bash
//! gst-launch-1.0 zenohsrc key-expr=demo/video/stream ! videoconvert ! autovideosink
//! ```

//! # ZenohSrc Element
//!
//! The ZenohSrc element receives data from the Zenoh network protocol and delivers
//! it as GStreamer buffers to downstream elements. It acts as a bridge that brings
//! data from distributed Zenoh networks into GStreamer pipelines.
//!
//! ## Features
//!
//! * **Automatic Reliability Adaptation**: Matches publisher reliability settings
//! * **Session Sharing**: Support for shared Zenoh sessions across elements
//! * **Flexible Configuration**: Support for Zenoh config files and runtime parameters
//! * **Real-time Streaming**: Optimized for low-latency data delivery
//! * **Multiple Data Formats**: Works with any data type (video, audio, binary, etc.)
//! 
//! ## Properties
//!
//! * `key-expr` - Zenoh key expression for subscribing to data (required)
//!   - Example: "demo/video/stream" or "sensors/temperature/{device_id}"
//!   - Supports Zenoh key expression wildcards like "*" and "**"
//! * `config` - Path to Zenoh configuration file (optional)
//!   - Allows custom Zenoh network configuration (endpoints, discovery, etc.)
//! * `priority` - Subscriber priority level (1-7, default: 5)
//!   - 1=RealTime (highest), 2=InteractiveHigh, 3=InteractiveLow, 4=DataHigh, 5=Data(default), 6=DataLow, 7=Background(lowest)
//! * `congestion-control` - Congestion control policy (informational, default: "block")
//!   - Mainly for configuration consistency with zenohsink
//! * `reliability` - Expected reliability mode (informational, default: "best-effort")
//!   - Actual reliability is determined by the matching publisher
//!   - Used for documentation and pipeline validation
//!
//! ## Example Pipelines
//! 
//! ### Basic Video Receiving
//! ```bash
//! # Simple video receiving and display
//! gst-launch-1.0 zenohsrc key-expr=demo/video/stream ! videoconvert ! autovideosink
//! ```
//!
//! ### H.264 Video Pipeline
//! ```bash
//! # Receive and decode H.264 video
//! gst-launch-1.0 zenohsrc key-expr=demo/video/h264 ! \
//!   "application/x-rtp,media=video,clock-rate=90000,encoding-name=H264,payload=96" ! \
//!   rtph264depay ! h264parse ! decodebin ! videoconvert ! autovideosink
//! ```
//!
//! ### Audio Pipeline
//! ```bash
//! # Receive audio and play through speakers
//! gst-launch-1.0 zenohsrc key-expr=demo/audio/stream ! audioconvert ! autoaudiosink
//! ```
//!
//! ### Multiple Stream Subscription with Wildcards
//! ```bash
//! # Subscribe to all streams from a specific device
//! gst-launch-1.0 zenohsrc key-expr="demo/device-01/**" ! \
//!   videoconvert ! videoscale ! autovideosink
//! ```

use gst::glib;
use gst::prelude::*;

pub mod imp;

glib::wrapper! {
    /// A GStreamer source element that subscribes to data via Zenoh.
    /// 
    /// This element subscribes to a Zenoh key expression and outputs
    /// received data as GStreamer buffers to downstream elements.
    pub struct ZenohSrc(ObjectSubclass<imp::ZenohSrc>) @extends gst_base::PushSrc, gst_base::BaseSrc, gst::Element, gst::Object;
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "zenohsrc",
        gst::Rank::MARGINAL,
        ZenohSrc::static_type(),
    )
}
