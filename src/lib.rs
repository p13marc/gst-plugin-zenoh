//! # GStreamer Zenoh Plugin
//!
//! This plugin provides GStreamer elements for sending and receiving data via the Zenoh protocol.
//! Zenoh is a pub/sub, storage and query protocol that provides excellent performance
//! and low latency for real-time data streaming.
//!
//! ## Elements
//!
//! * [`zenohsink`] - Publishes GStreamer buffers to a Zenoh key expression
//! * [`zenohsrc`] - Subscribes to a Zenoh key expression and outputs GStreamer buffers
//!
//! ## Example Usage
//!
//! ### Sending data
//! ```bash
//! gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video/stream
//! ```
//!
//! ### Receiving data
//! ```bash
//! gst-launch-1.0 zenohsrc key-expr=demo/video/stream ! videoconvert ! autovideosink
//! ```
//!
//! ## Configuration
//!
//! Both elements support Zenoh configuration through:
//! - `config` property: Path to a Zenoh configuration file
//! - Built-in properties for common settings (priority, reliability, congestion control)
//!
//! [`zenohsink`]: zenohsink::ZenohSink
//! [`zenohsrc`]: zenohsrc::ZenohSrc

use gst::glib;

mod error;
mod utils;
mod zenohsink;
mod zenohsrc;

fn plugin_init(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    zenohsink::register(plugin)?;
    zenohsrc::register(plugin)?;
    Ok(())
}

gst::plugin_define!(
    zenoh,
    env!("CARGO_PKG_DESCRIPTION"),
    plugin_init,
    concat!(env!("CARGO_PKG_VERSION"), "-", env!("COMMIT_ID")),
    "MPL",
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_NAME"),
    env!("CARGO_PKG_REPOSITORY"),
    env!("BUILD_REL_DATE")
);
