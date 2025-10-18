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
