//! # ZenohDemux Element
//!
//! The ZenohDemux element demultiplexes Zenoh streams based on key expressions.
//! It subscribes to a wildcard key expression and creates dynamic source pads
//! for each unique key expression it receives data from.
//!
//! ## Use Cases
//!
//! - **Multi-camera setup**: Subscribe to `camera/*` and route each camera to different sinks
//! - **Sensor aggregation**: Subscribe to `sensors/**` and process each sensor type differently
//! - **Channel selection**: Subscribe to `stream/*` and dynamically select which to display
//!
//! ## Properties
//!
//! * `key-expr` - Zenoh key expression for subscribing (supports wildcards like `*` and `**`)
//! * `config` - Path to Zenoh configuration file (optional)
//! * `pad-naming` - How to name pads: "full-path", "last-segment", or "hash"
//!
//! ## Example Pipeline
//!
//! ```bash
//! # Subscribe to all cameras and display them
//! gst-launch-1.0 zenohdemux key-expr="camera/*" name=demux \
//!   demux.camera_front ! queue ! videoconvert ! autovideosink \
//!   demux.camera_rear ! queue ! videoconvert ! autovideosink
//! ```
//!
//! ## Dynamic Pads
//!
//! Source pads are created dynamically when data arrives from a new key expression.
//! Pad names are derived from the key expression based on the `pad-naming` property:
//!
//! - `full-path`: "camera/front" → "camera_front"
//! - `last-segment`: "camera/front" → "front"
//! - `hash`: "camera/front" → "pad_a1b2c3"

use gst::glib;
use gst::prelude::*;

pub mod imp;

glib::wrapper! {
    /// A GStreamer element that demultiplexes Zenoh streams by key expression.
    ///
    /// This element subscribes to a Zenoh wildcard key expression and creates
    /// dynamic source pads for each unique key expression it receives data from.
    pub struct ZenohDemux(ObjectSubclass<imp::ZenohDemux>) @extends gst::Element, gst::Object;
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "zenohdemux",
        gst::Rank::NONE,
        ZenohDemux::static_type(),
    )
}
