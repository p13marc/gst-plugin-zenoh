//! # ZenohSink Element
//!
//! The ZenohSink element sends GStreamer buffers over the Zenoh network protocol.
//! 
//! ## Properties
//!
//! * `key-expr` - Zenoh key expression for publishing data (required)
//! * `config` - Path to Zenoh configuration file (optional)
//! * `priority` - Publisher priority (-100 to 100, default: 0)
//! * `congestion-control` - Congestion control policy: "block" or "drop" (default: "block")
//! * `reliability` - Reliability mode: "best-effort" or "reliable" (default: "best-effort")
//!
//! ## Example Pipeline
//! 
//! ```
//! gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video/stream
//! ```

use gst::glib;
use gst::prelude::*;

pub mod imp;

glib::wrapper! {
    /// A GStreamer sink element that publishes data via Zenoh.
    /// 
    /// This element receives buffers from upstream elements and publishes
    /// them to a Zenoh network using the configured key expression.
    pub struct ZenohSink(ObjectSubclass<imp::ZenohSink>) @extends gst_base::BaseSink, gst::Element, gst::Object;
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "zenohsink",
        gst::Rank::MARGINAL,
        ZenohSink::static_type(),
    )
}
