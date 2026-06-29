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
//!
//! Note: a subscriber does not configure priority/congestion-control/reliability
//! — those are publisher-side QoS. Reliability is adapted from the publisher
//! automatically.
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
use gst::subclass::prelude::ObjectSubclassIsExt;

pub mod imp;

glib::wrapper! {
    /// A GStreamer source element that subscribes to data via Zenoh.
    ///
    /// This element subscribes to a Zenoh key expression and outputs
    /// received data as GStreamer buffers to downstream elements.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gst::prelude::*;
    /// use gstzenoh::zenohsrc::ZenohSrc;
    ///
    /// gst::init().unwrap();
    /// gstzenoh::plugin_register_static().unwrap();
    ///
    /// // Create with the builder pattern
    /// let src = ZenohSrc::builder("demo/video")
    ///     .receive_timeout_ms(500)
    ///     .apply_buffer_meta(true)
    ///     .build();
    ///
    /// // Or create and configure separately
    /// let src = ZenohSrc::new("demo/video");
    /// src.set_receive_timeout_ms(500);
    ///
    /// // Access statistics
    /// println!("Bytes received: {}", src.bytes_received());
    /// ```
    pub struct ZenohSrc(ObjectSubclass<imp::ZenohSrc>) @extends gst_base::PushSrc, gst_base::BaseSrc, gst::Element, gst::Object, @implements gst::URIHandler;
}

unsafe impl Send for ZenohSrc {}
unsafe impl Sync for ZenohSrc {}

impl Default for ZenohSrc {
    fn default() -> Self {
        gst::Object::builder().build().unwrap()
    }
}

impl ZenohSrc {
    /// Creates a new ZenohSrc with the specified key expression.
    ///
    /// # Arguments
    ///
    /// * `key_expr` - The Zenoh key expression for subscribing to data
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gstzenoh::zenohsrc::ZenohSrc;
    ///
    /// gst::init().unwrap();
    /// gstzenoh::plugin_register_static().unwrap();
    ///
    /// let src = ZenohSrc::new("demo/video/stream");
    /// ```
    pub fn new(key_expr: &str) -> Self {
        gst::Object::builder()
            .property("key-expr", key_expr)
            .build()
            .unwrap()
    }

    /// Returns a builder for creating a ZenohSrc with custom configuration.
    ///
    /// # Arguments
    ///
    /// * `key_expr` - The Zenoh key expression for subscribing to data
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gstzenoh::zenohsrc::ZenohSrc;
    ///
    /// gst::init().unwrap();
    /// gstzenoh::plugin_register_static().unwrap();
    ///
    /// let src = ZenohSrc::builder("demo/video")
    ///     .receive_timeout_ms(500)
    ///     .apply_buffer_meta(true)
    ///     .build();
    /// ```
    pub fn builder(key_expr: &str) -> ZenohSrcBuilder {
        ZenohSrcBuilder::new(key_expr)
    }

    // -------------------------------------------------------------------------
    // Property Setters
    // -------------------------------------------------------------------------

    /// Sets the Zenoh key expression for subscribing to data.
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

    /// Sets the receive timeout in milliseconds.
    ///
    /// Lower values increase responsiveness but use more CPU.
    /// Higher values reduce CPU usage but slow down state changes.
    /// Valid range: 10-5000ms, default: 100ms.
    pub fn set_receive_timeout_ms(&self, timeout: u64) {
        self.set_property("receive-timeout-ms", timeout);
    }

    /// Enables or disables applying buffer timing metadata from received messages.
    ///
    /// When enabled, PTS, DTS, duration, offset, and flags are restored
    /// from the sender's buffer metadata for proper A/V sync.
    pub fn set_apply_buffer_meta(&self, apply: bool) {
        self.set_property("apply-buffer-meta", apply);
    }

    /// Sets the subscriber receive-handler strategy.
    ///
    /// `"fifo"` (default) is lossless and unbounded; `"ring"` keeps only the most
    /// recent [`queue_depth`](Self::set_queue_depth) samples, dropping the oldest
    /// to bound latency on live sources. Must be set before the element starts.
    pub fn set_handler(&self, handler: &str) {
        self.set_property("handler", handler);
    }

    /// Sets the ring-handler depth (max samples retained before drop-oldest).
    ///
    /// Only affects the `"ring"` handler; ignored for `"fifo"`. Valid range:
    /// 1-100000, default: 30. Must be set before the element starts.
    pub fn set_queue_depth(&self, depth: u32) {
        self.set_property("queue-depth", depth);
    }

    /// Sets a shared Zenoh session for this element.
    ///
    /// This allows multiple elements to share a single Zenoh session,
    /// reducing network overhead and resource usage. The session must
    /// be set before the element transitions to the PLAYING state.
    ///
    /// Note: `zenoh::Session` is internally Arc-based and Clone, so you
    /// can simply clone the session when sharing it across multiple elements.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use gstzenoh::ZenohSrc;
    /// use zenoh::Wait;
    ///
    /// let session = zenoh::open(zenoh::Config::default()).wait()?;
    ///
    /// let src1 = ZenohSrc::new("demo/video");
    /// src1.set_session(session.clone());
    ///
    /// let src2 = ZenohSrc::new("demo/audio");
    /// src2.set_session(session);
    /// ```
    pub fn set_session(&self, session: zenoh::Session) {
        self.imp().set_external_session(session);
    }

    /// Sets the session group name for sharing sessions across elements.
    ///
    /// Elements with the same session-group name will share a single
    /// Zenoh session. This is useful for gst-launch pipelines where
    /// you can't share session objects directly.
    ///
    /// Must be set before the element transitions to the PLAYING state.
    pub fn set_session_group(&self, group: &str) {
        self.set_property("session-group", group);
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

    /// Returns the receive timeout in milliseconds.
    pub fn receive_timeout_ms(&self) -> u64 {
        self.property("receive-timeout-ms")
    }

    /// Returns whether buffer timing metadata is being applied.
    pub fn apply_buffer_meta(&self) -> bool {
        self.property("apply-buffer-meta")
    }

    /// Returns the receive-handler strategy (`"fifo"` or `"ring"`).
    pub fn handler(&self) -> String {
        self.property("handler")
    }

    /// Returns the ring-handler depth.
    pub fn queue_depth(&self) -> u32 {
        self.property("queue-depth")
    }

    /// Returns the session group name, if set.
    pub fn session_group(&self) -> Option<String> {
        self.property("session-group")
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

    /// Returns the total number of errors encountered.
    pub fn errors(&self) -> u64 {
        self.property("errors")
    }

    /// Returns the total samples dropped by the `"ring"` handler (0 for `"fifo"`).
    pub fn dropped(&self) -> u64 {
        self.property("dropped")
    }

    /// Returns whether the Zenoh session currently has any open transport.
    ///
    /// Reflects transport up/down as observed by Zenoh (which reconnects on its
    /// own); see the `zenoh-connectivity-changed` bus message for transitions.
    pub fn connected(&self) -> bool {
        self.property("connected")
    }
}

impl TryFrom<gst::Element> for ZenohSrc {
    type Error = gst::Element;

    /// Attempts to convert a generic GStreamer element to a ZenohSrc.
    ///
    /// Returns the original element as an error if it's not a ZenohSrc.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gst::prelude::*;
    /// use gstzenoh::zenohsrc::ZenohSrc;
    ///
    /// gst::init().unwrap();
    /// gstzenoh::plugin_register_static().unwrap();
    ///
    /// let element = gst::ElementFactory::make("zenohsrc")
    ///     .property("key-expr", "demo/video")
    ///     .build()
    ///     .unwrap();
    ///
    /// let src = ZenohSrc::try_from(element).expect("Should be a ZenohSrc");
    /// assert_eq!(src.key_expr(), "demo/video");
    /// ```
    fn try_from(element: gst::Element) -> Result<Self, Self::Error> {
        element.downcast()
    }
}

/// Builder for creating a [`ZenohSrc`] with custom configuration.
///
/// # Example
///
/// ```no_run
/// use gstzenoh::zenohsrc::ZenohSrc;
///
/// gst::init().unwrap();
/// gstzenoh::plugin_register_static().unwrap();
///
/// let src = ZenohSrc::builder("demo/video")
///     .receive_timeout_ms(500)
///     .apply_buffer_meta(true)
///     .build();
/// ```
pub struct ZenohSrcBuilder {
    key_expr: String,
    config: Option<String>,
    receive_timeout_ms: Option<u64>,
    apply_buffer_meta: Option<bool>,
    handler: Option<String>,
    queue_depth: Option<u32>,
    session: Option<zenoh::Session>,
    session_group: Option<String>,
}

impl ZenohSrcBuilder {
    /// Creates a new builder with the required key expression.
    pub fn new(key_expr: &str) -> Self {
        Self {
            key_expr: key_expr.to_string(),
            config: None,
            receive_timeout_ms: None,
            apply_buffer_meta: None,
            handler: None,
            queue_depth: None,
            session: None,
            session_group: None,
        }
    }

    /// Sets the path to a Zenoh configuration file.
    pub fn config(mut self, path: &str) -> Self {
        self.config = Some(path.to_string());
        self
    }

    /// Sets the receive timeout in milliseconds.
    pub fn receive_timeout_ms(mut self, timeout: u64) -> Self {
        self.receive_timeout_ms = Some(timeout);
        self
    }

    /// Enables or disables applying buffer timing metadata.
    pub fn apply_buffer_meta(mut self, apply: bool) -> Self {
        self.apply_buffer_meta = Some(apply);
        self
    }

    /// Sets the receive-handler strategy: `"fifo"` (lossless) or `"ring"`
    /// (drop-oldest, bounds latency for live sources).
    pub fn handler(mut self, handler: &str) -> Self {
        self.handler = Some(handler.to_string());
        self
    }

    /// Sets the ring-handler depth (max retained samples before drop-oldest).
    /// Only affects the `"ring"` handler.
    pub fn queue_depth(mut self, depth: u32) -> Self {
        self.queue_depth = Some(depth);
        self
    }

    /// Sets a shared Zenoh session for this element.
    ///
    /// This allows multiple elements to share a single Zenoh session,
    /// reducing network overhead and resource usage.
    ///
    /// Note: `zenoh::Session` is internally Arc-based and Clone, so you
    /// can simply clone the session when sharing it across elements.
    pub fn session(mut self, session: zenoh::Session) -> Self {
        self.session = Some(session);
        self
    }

    /// Sets the session group name for sharing sessions across elements.
    ///
    /// Elements with the same session-group name will share a single
    /// Zenoh session. This is useful for gst-launch pipelines.
    pub fn session_group(mut self, group: &str) -> Self {
        self.session_group = Some(group.to_string());
        self
    }

    /// Builds the ZenohSrc with the configured properties.
    pub fn build(self) -> ZenohSrc {
        let mut builder = gst::Object::builder::<ZenohSrc>().property("key-expr", &self.key_expr);

        if let Some(config) = self.config {
            builder = builder.property("config", config);
        }
        if let Some(timeout) = self.receive_timeout_ms {
            builder = builder.property("receive-timeout-ms", timeout);
        }
        if let Some(apply) = self.apply_buffer_meta {
            builder = builder.property("apply-buffer-meta", apply);
        }
        if let Some(ref handler) = self.handler {
            builder = builder.property("handler", handler);
        }
        if let Some(depth) = self.queue_depth {
            builder = builder.property("queue-depth", depth);
        }
        if let Some(ref sg) = self.session_group {
            builder = builder.property("session-group", sg);
        }

        let src: ZenohSrc = builder.build().unwrap();

        // Set the session directly (can't be done via properties)
        if let Some(session) = self.session {
            src.set_session(session);
        }

        src
    }
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "zenohsrc",
        gst::Rank::NONE + 100, // Higher than MARGINAL to be discoverable
        ZenohSrc::static_type(),
    )
}
