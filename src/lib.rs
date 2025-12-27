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
//! ### Strongly-Typed API (Recommended)
//!
//! Use the builder pattern for type-safe element creation.
//! Main types are re-exported at the crate root for convenience:
//!
//! ```no_run
//! use gst::prelude::*;
//! use gstzenoh::{ZenohSink, ZenohSrc};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     gst::init()?;
//!     gstzenoh::plugin_register_static()?;
//!
//!     // Create ZenohSink with the builder pattern
//!     let sink = ZenohSink::builder("demo/video")
//!         .reliability("reliable")
//!         .priority(2)  // InteractiveHigh
//!         .express(true)
//!         .send_caps(true)
//!         .build();
//!
//!     // Or use new() and setters
//!     let src = ZenohSrc::new("demo/video");
//!     src.set_receive_timeout_ms(500);
//!     src.set_apply_buffer_meta(true);
//!
//!     // Access statistics with typed getters
//!     println!("Bytes sent: {}", sink.bytes_sent());
//!     println!("Messages received: {}", src.messages_received());
//!
//!     Ok(())
//! }
//! ```
//!
//! ### Building Pipelines with Strongly-Typed Elements
//!
//! ```no_run
//! use gst::prelude::*;
//! use gstzenoh::ZenohSink;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     gst::init()?;
//!     gstzenoh::plugin_register_static()?;
//!
//!     let pipeline = gst::Pipeline::new();
//!     let src = gst::ElementFactory::make("videotestsrc").build()?;
//!
//!     // Create sink with strongly-typed API
//!     let sink = ZenohSink::builder("demo/video")
//!         .reliability("reliable")
//!         .priority(2)
//!         .express(true)
//!         .build();
//!
//!     // Add to pipeline (upcast to Element)
//!     pipeline.add_many([&src, sink.upcast_ref()])?;
//!     src.link(&sink)?;
//!
//!     pipeline.set_state(gst::State::Playing)?;
//!     // ... run pipeline ...
//!     pipeline.set_state(gst::State::Null)?;
//!     Ok(())
//! }
//! ```
//!
//! ### Converting from Generic Elements
//!
//! ```no_run
//! use gst::prelude::*;
//! use gstzenoh::ZenohSink;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     gst::init()?;
//!     gstzenoh::plugin_register_static()?;
//!
//!     // Create via ElementFactory
//!     let element = gst::ElementFactory::make("zenohsink")
//!         .property("key-expr", "demo/video")
//!         .build()?;
//!
//!     // Convert to strongly-typed wrapper
//!     let sink = ZenohSink::try_from(element).expect("Should be a ZenohSink");
//!
//!     // Now use typed API
//!     sink.set_reliability("reliable");
//!     println!("Key expression: {}", sink.key_expr());
//!
//!     Ok(())
//! }
//! ```
//!
//! ### Using the Generic Property API
//!
//! For compatibility or dynamic configuration:
//!
//! ```no_run
//! use gst::prelude::*;
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     gst::init()?;
//!     gstzenoh::plugin_register_static()?;
//!
//!     let sink = gst::ElementFactory::make("zenohsink")
//!         .property("key-expr", "demo/video")
//!         .property("reliability", "reliable")
//!         .property("priority", 2u32)
//!         .property("express", true)
//!         .build()?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ### Using parse_launch for Quick Prototyping
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

// Re-export main types at crate root for convenience
pub use zenohdemux::{PadNaming, ZenohDemux, ZenohDemuxBuilder};
pub use zenohsink::{ZenohSink, ZenohSinkBuilder};
pub use zenohsrc::{ZenohSrc, ZenohSrcBuilder};

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
