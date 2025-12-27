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

// Re-export PadNaming for public API
pub use imp::PadNaming;

glib::wrapper! {
    /// A GStreamer element that demultiplexes Zenoh streams by key expression.
    ///
    /// This element subscribes to a Zenoh wildcard key expression and creates
    /// dynamic source pads for each unique key expression it receives data from.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gst::prelude::*;
    /// use gstzenoh::zenohdemux::{ZenohDemux, PadNaming};
    ///
    /// gst::init().unwrap();
    /// gstzenoh::plugin_register_static().unwrap();
    ///
    /// // Create with the builder pattern
    /// let demux = ZenohDemux::builder("camera/*")
    ///     .pad_naming(PadNaming::LastSegment)
    ///     .receive_timeout_ms(200)
    ///     .build();
    ///
    /// // Or create and configure separately
    /// let demux = ZenohDemux::new("sensor/**");
    /// demux.set_pad_naming(PadNaming::FullPath);
    ///
    /// // Access statistics
    /// println!("Pads created: {}", demux.pads_created());
    /// ```
    pub struct ZenohDemux(ObjectSubclass<imp::ZenohDemux>) @extends gst::Element, gst::Object;
}

unsafe impl Send for ZenohDemux {}
unsafe impl Sync for ZenohDemux {}

impl Default for ZenohDemux {
    fn default() -> Self {
        gst::Object::builder().build().unwrap()
    }
}

impl ZenohDemux {
    /// Creates a new ZenohDemux with the specified key expression.
    ///
    /// # Arguments
    ///
    /// * `key_expr` - The Zenoh key expression for subscribing (supports wildcards)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gstzenoh::zenohdemux::ZenohDemux;
    ///
    /// gst::init().unwrap();
    /// gstzenoh::plugin_register_static().unwrap();
    ///
    /// let demux = ZenohDemux::new("camera/*");
    /// ```
    pub fn new(key_expr: &str) -> Self {
        gst::Object::builder()
            .property("key-expr", key_expr)
            .build()
            .unwrap()
    }

    /// Returns a builder for creating a ZenohDemux with custom configuration.
    ///
    /// # Arguments
    ///
    /// * `key_expr` - The Zenoh key expression for subscribing (supports wildcards)
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gstzenoh::zenohdemux::{ZenohDemux, PadNaming};
    ///
    /// gst::init().unwrap();
    /// gstzenoh::plugin_register_static().unwrap();
    ///
    /// let demux = ZenohDemux::builder("sensor/**")
    ///     .pad_naming(PadNaming::LastSegment)
    ///     .receive_timeout_ms(200)
    ///     .build();
    /// ```
    pub fn builder(key_expr: &str) -> ZenohDemuxBuilder {
        ZenohDemuxBuilder::new(key_expr)
    }

    // -------------------------------------------------------------------------
    // Property Setters
    // -------------------------------------------------------------------------

    /// Sets the Zenoh key expression for subscribing.
    ///
    /// Must be set before the element is started.
    /// Supports wildcards: `*` (single level) and `**` (multi-level).
    pub fn set_key_expr(&self, key_expr: &str) {
        self.set_property("key-expr", key_expr);
    }

    /// Sets the path to a Zenoh configuration file.
    ///
    /// The file should be in JSON5 format.
    pub fn set_config(&self, config_path: &str) {
        self.set_property("config", config_path);
    }

    /// Sets how pad names are derived from key expressions.
    ///
    /// - [`PadNaming::FullPath`]: "camera/front" → "camera_front"
    /// - [`PadNaming::LastSegment`]: "camera/front" → "front"
    /// - [`PadNaming::Hash`]: "camera/front" → "pad_a1b2c3"
    pub fn set_pad_naming(&self, naming: PadNaming) {
        self.set_property("pad-naming", naming);
    }

    /// Sets the receive timeout in milliseconds.
    ///
    /// Lower values increase responsiveness but use more CPU.
    /// Higher values reduce CPU usage but slow down state changes.
    /// Valid range: 10-5000ms, default: 100ms.
    pub fn set_receive_timeout_ms(&self, timeout: u64) {
        self.set_property("receive-timeout-ms", timeout);
    }

    // -------------------------------------------------------------------------
    // Property Getters
    // -------------------------------------------------------------------------

    /// Returns the current Zenoh key expression.
    pub fn key_expr(&self) -> String {
        self.property("key-expr")
    }

    /// Returns the path to the Zenoh configuration file, if set.
    pub fn config(&self) -> Option<String> {
        self.property("config")
    }

    /// Returns the current pad naming strategy.
    pub fn pad_naming(&self) -> PadNaming {
        self.property("pad-naming")
    }

    /// Returns the receive timeout in milliseconds.
    pub fn receive_timeout_ms(&self) -> u64 {
        self.property("receive-timeout-ms")
    }

    // -------------------------------------------------------------------------
    // Statistics (read-only)
    // -------------------------------------------------------------------------

    /// Returns the total number of bytes received since the element started.
    pub fn bytes_received(&self) -> u64 {
        self.property("bytes-received")
    }

    /// Returns the total number of messages received since the element started.
    pub fn messages_received(&self) -> u64 {
        self.property("messages-received")
    }

    /// Returns the number of dynamic pads created.
    pub fn pads_created(&self) -> u64 {
        self.property("pads-created")
    }
}

impl TryFrom<gst::Element> for ZenohDemux {
    type Error = gst::Element;

    /// Attempts to convert a generic GStreamer element to a ZenohDemux.
    ///
    /// Returns the original element as an error if it's not a ZenohDemux.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gst::prelude::*;
    /// use gstzenoh::zenohdemux::ZenohDemux;
    ///
    /// gst::init().unwrap();
    /// gstzenoh::plugin_register_static().unwrap();
    ///
    /// let element = gst::ElementFactory::make("zenohdemux")
    ///     .property("key-expr", "sensor/*")
    ///     .build()
    ///     .unwrap();
    ///
    /// let demux = ZenohDemux::try_from(element).expect("Should be a ZenohDemux");
    /// assert_eq!(demux.key_expr(), "sensor/*");
    /// ```
    fn try_from(element: gst::Element) -> Result<Self, Self::Error> {
        element.downcast()
    }
}

/// Builder for creating a [`ZenohDemux`] with custom configuration.
///
/// # Example
///
/// ```no_run
/// use gstzenoh::zenohdemux::{ZenohDemux, PadNaming};
///
/// gst::init().unwrap();
/// gstzenoh::plugin_register_static().unwrap();
///
/// let demux = ZenohDemux::builder("camera/*")
///     .pad_naming(PadNaming::LastSegment)
///     .receive_timeout_ms(200)
///     .build();
/// ```
pub struct ZenohDemuxBuilder {
    key_expr: String,
    config: Option<String>,
    pad_naming: Option<PadNaming>,
    receive_timeout_ms: Option<u64>,
}

impl ZenohDemuxBuilder {
    /// Creates a new builder with the required key expression.
    pub fn new(key_expr: &str) -> Self {
        Self {
            key_expr: key_expr.to_string(),
            config: None,
            pad_naming: None,
            receive_timeout_ms: None,
        }
    }

    /// Sets the path to a Zenoh configuration file.
    pub fn config(mut self, path: &str) -> Self {
        self.config = Some(path.to_string());
        self
    }

    /// Sets the pad naming strategy.
    pub fn pad_naming(mut self, naming: PadNaming) -> Self {
        self.pad_naming = Some(naming);
        self
    }

    /// Sets the receive timeout in milliseconds.
    pub fn receive_timeout_ms(mut self, timeout: u64) -> Self {
        self.receive_timeout_ms = Some(timeout);
        self
    }

    /// Builds the ZenohDemux with the configured properties.
    pub fn build(self) -> ZenohDemux {
        let mut builder = gst::Object::builder::<ZenohDemux>().property("key-expr", &self.key_expr);

        if let Some(config) = self.config {
            builder = builder.property("config", config);
        }
        if let Some(naming) = self.pad_naming {
            builder = builder.property("pad-naming", naming);
        }
        if let Some(timeout) = self.receive_timeout_ms {
            builder = builder.property("receive-timeout-ms", timeout);
        }

        builder.build().unwrap()
    }
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "zenohdemux",
        gst::Rank::NONE,
        ZenohDemux::static_type(),
    )
}
