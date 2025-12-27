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
//! - [`zenohdemux`]: Demultiplexes Zenoh streams by key expression, creating dynamic pads
//!
//! ## Features
//!
//! - **Advanced QoS Control**: Configurable reliability, congestion control, and priority
//! - **Express Mode**: Ultra-low latency streaming with queue bypass
//! - **Session Sharing**: Efficient resource management across multiple elements
//! - **Thread Safety**: Safe concurrent access to all components
//! - **Error Recovery**: Comprehensive error handling and network resilience
//! - **Optional Compression**: zstd, lz4, and gzip support via feature flags
//!
//! ## Usage
//!
//! ### Using the Rust API
//!
//! Create elements programmatically with full type safety:
//!
//! ```no_run
//! use gst::prelude::*;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     gst::init()?;
//!     gstzenoh::plugin_register_static()?;
//!
//!     // Create a pipeline with zenohsink
//!     let pipeline = gst::Pipeline::new();
//!     let src = gst::ElementFactory::make("videotestsrc").build()?;
//!     let sink = gst::ElementFactory::make("zenohsink")
//!         .property("key-expr", "demo/video")
//!         .property("reliability", "reliable")
//!         .property("priority", 2i32)  // InteractiveHigh
//!         .property("express", true)
//!         .build()?;
//!
//!     pipeline.add_many([&src, &sink])?;
//!     src.link(&sink)?;
//!
//!     pipeline.set_state(gst::State::Playing)?;
//!     // ... run pipeline ...
//!     pipeline.set_state(gst::State::Null)?;
//!     Ok(())
//! }
//! ```
//!
//! ### Receiving data with zenohsrc
//!
//! ```no_run
//! use gst::prelude::*;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     gst::init()?;
//!     gstzenoh::plugin_register_static()?;
//!
//!     let pipeline = gst::Pipeline::new();
//!     let src = gst::ElementFactory::make("zenohsrc")
//!         .property("key-expr", "demo/video")
//!         .property("receive-timeout-ms", 5000i32)
//!         .build()?;
//!     let convert = gst::ElementFactory::make("videoconvert").build()?;
//!     let sink = gst::ElementFactory::make("autovideosink").build()?;
//!
//!     pipeline.add_many([&src, &convert, &sink])?;
//!     gst::Element::link_many([&src, &convert, &sink])?;
//!
//!     pipeline.set_state(gst::State::Playing)?;
//!     // ... run pipeline ...
//!     pipeline.set_state(gst::State::Null)?;
//!     Ok(())
//! }
//! ```
//!
//! ### Using parse_launch for quick prototyping
//!
//! ```no_run
//! use gst::prelude::*;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     gst::init()?;
//!     gstzenoh::plugin_register_static()?;
//!
//!     let pipeline = gst::parse::launch(
//!         "videotestsrc ! zenohsink key-expr=demo/video"
//!     )?;
//!
//!     pipeline.set_state(gst::State::Playing)?;
//!     // ... run pipeline ...
//!     pipeline.set_state(gst::State::Null)?;
//!     Ok(())
//! }
//! ```
//!
//! ## Command Line Usage
//!
//! ```bash
//! # Set plugin path
//! export GST_PLUGIN_PATH=/path/to/target/release
//!
//! # Simple streaming
//! gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video
//! gst-launch-1.0 zenohsrc key-expr=demo/video ! videoconvert ! autovideosink
//!
//! # Demultiplexing multiple streams
//! gst-launch-1.0 zenohdemux key-expr="demo/**" ! queue ! autovideosink
//! ```
//!
//! ## Examples
//!
//! See the `examples/` directory for comprehensive usage demonstrations:
//! - `basic.rs`: Simple video streaming setup
//! - `configuration.rs`: Advanced QoS configuration showcase
//! - `video_stream.rs`: Full video streaming pipeline
//!
//! [`zenohsink`]: zenohsink
//! [`zenohsrc`]: zenohsrc
//! [`zenohdemux`]: zenohdemux

use gst::glib;

mod error;
pub mod metadata;
pub mod utils;
pub mod zenohdemux;
pub mod zenohsink;
pub mod zenohsrc;

#[cfg(any(
    feature = "compression-zstd",
    feature = "compression-lz4",
    feature = "compression-gzip"
))]
pub mod compression;

fn plugin_init(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    zenohsink::register(plugin)?;
    zenohsrc::register(plugin)?;
    zenohdemux::register(plugin)?;
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
