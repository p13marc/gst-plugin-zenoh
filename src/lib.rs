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

//! # gst-plugin-zenoh
//!
//! A high-performance GStreamer plugin for distributed media streaming using Zenoh.
//!
//! This plugin provides seamless integration between GStreamer multimedia pipelines
//! and Zenoh networks, enabling distributed applications, edge computing scenarios,
//! robotics systems, and IoT data streaming.
//!
//! ## Elements
//!
//! - [`zenohsink`]: Publishes GStreamer buffers to Zenoh networks
//! - [`zenohsrc`]: Subscribes to Zenoh data and delivers it to GStreamer pipelines
//!
//! ## Features
//!
//! - **Advanced QoS Control**: Configurable reliability, congestion control, and priority
//! - **Express Mode**: Ultra-low latency streaming with queue bypass
//! - **Session Sharing**: Efficient resource management across multiple elements
//! - **Thread Safety**: Safe concurrent access to all components
//! - **Error Recovery**: Comprehensive error handling and network resilience
//!
//! ## Quick Start
//!
//! ```bash
//! # Build the plugin
//! cargo build --release
//!
//! # Simple streaming example
//! gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video
//! gst-launch-1.0 zenohsrc key-expr=demo/video ! videoconvert ! autovideosink
//! ```
//!
//! ## Examples
//!
//! See the `examples/` directory for comprehensive usage demonstrations:
//! - `basic.rs`: Simple video streaming setup
//! - `configuration.rs`: Advanced QoS configuration showcase
//!
//! [`zenohsink`]: zenohsink
//! [`zenohsrc`]: zenohsrc

use gst::glib;

mod error;
pub mod utils;
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
    "MPL-2.0",
    env!("CARGO_PKG_NAME"),
    "gst-plugin-zenoh",
    env!("CARGO_PKG_REPOSITORY"),
    env!("BUILD_REL_DATE")
);
