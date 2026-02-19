//! # ZenohSink Element
//!
//! The ZenohSink element sends GStreamer buffers over the Zenoh network protocol.
//! It acts as a bridge between GStreamer pipelines and Zenoh networks, enabling
//! distributed media streaming and data sharing across different applications
//! and systems.
//!
//! ## Features
//!
//! * **Quality of Service (QoS) Control**: Configurable reliability and congestion control
//! * **Low Latency Mode**: Express mode for time-critical applications
//! * **Priority Management**: Message prioritization for bandwidth management
//! * **Session Sharing**: Support for shared Zenoh sessions across elements
//! * **Flexible Configuration**: Support for Zenoh config files and runtime parameters
//!
//! ## Properties
//!
//! * `key-expr` - Zenoh key expression for publishing data (required)
//!   - Example: "demo/video/stream" or "sensors/temperature/{device_id}"
//! * `config` - Path to Zenoh configuration file (optional)
//!   - Allows custom Zenoh network configuration (endpoints, discovery, etc.)
//! * `priority` - Publisher priority level (1-7, default: 5)
//!   - 1=RealTime (highest), 2=InteractiveHigh, 3=InteractiveLow, 4=DataHigh, 5=Data(default), 6=DataLow, 7=Background(lowest)
//! * `congestion-control` - Congestion control policy (default: "block")
//!   - `"block"`: Wait for network congestion to clear (ensures delivery)
//!   - `"drop"`: Drop messages during congestion (maintains real-time performance)
//! * `reliability` - Reliability mode (default: "best-effort")
//!   - `"best-effort"`: Fire-and-forget delivery (lower latency)
//!   - `"reliable"`: Acknowledged delivery with retransmission (higher reliability)
//! * `express` - Enable express mode for lower latency (default: false)
//!   - Bypasses some internal queues for reduced end-to-end latency
//!   - May increase CPU usage but improves responsiveness
//!
//! ## Example Pipelines
//!
//! ### Basic Video Streaming
//! ```bash
//! # Simple video streaming
//! gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video/stream
//! ```
//!
//! ### High-Quality Reliable Streaming
//! ```bash
//! # Reliable delivery with high priority and express mode for low latency
//! gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video/reliable \
//!   reliability=reliable congestion-control=block express=true priority=2
//! ```
//!
//! ### Real-Time Streaming with Quality Trade-offs
//! ```bash
//! # Best-effort delivery optimized for real-time performance
//! gst-launch-1.0 videotestsrc ! zenohsink key-expr=demo/video/realtime \
//!   reliability=best-effort congestion-control=drop express=true
//! ```
//!
//! ### Audio Streaming with Custom Configuration
//! ```bash
//! # Audio with custom Zenoh configuration
//! gst-launch-1.0 audiotestsrc ! audioconvert ! zenohsink \
//!   key-expr=demo/audio/stream config=/path/to/zenoh.json5 priority=4
//! ```
//!
//! ### Encoded Video with H.264
//! ```bash
//! # H.264 encoded video streaming
//! gst-launch-1.0 videotestsrc ! x264enc ! rtph264pay ! zenohsink \
//!   key-expr=demo/video/h264 reliability=reliable
//! ```

use gst::glib;
use gst::prelude::*;
use gst::subclass::prelude::ObjectSubclassIsExt;

pub mod imp;

glib::wrapper! {
    /// A GStreamer sink element that publishes data via Zenoh.
    ///
    /// This element receives buffers from upstream elements and publishes
    /// them to a Zenoh network using the configured key expression.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gst::prelude::*;
    /// use gstzenoh::zenohsink::ZenohSink;
    ///
    /// gst::init().unwrap();
    /// gstzenoh::plugin_register_static().unwrap();
    ///
    /// // Create with the builder pattern
    /// let sink = ZenohSink::builder("demo/video")
    ///     .reliability("reliable")
    ///     .priority(2)
    ///     .express(true)
    ///     .build();
    ///
    /// // Or create and configure separately
    /// let sink = ZenohSink::new("demo/video");
    /// sink.set_reliability("reliable");
    /// sink.set_express(true);
    ///
    /// // Access statistics
    /// println!("Bytes sent: {}", sink.bytes_sent());
    /// ```
    pub struct ZenohSink(ObjectSubclass<imp::ZenohSink>) @extends gst_base::BaseSink, gst::Element, gst::Object, @implements gst::URIHandler;
}

unsafe impl Send for ZenohSink {}
unsafe impl Sync for ZenohSink {}

impl Default for ZenohSink {
    fn default() -> Self {
        gst::Object::builder().build().unwrap()
    }
}

impl ZenohSink {
    /// Creates a new ZenohSink with the specified key expression.
    ///
    /// # Arguments
    ///
    /// * `key_expr` - The Zenoh key expression for publishing data
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gstzenoh::zenohsink::ZenohSink;
    ///
    /// gst::init().unwrap();
    /// gstzenoh::plugin_register_static().unwrap();
    ///
    /// let sink = ZenohSink::new("demo/video/stream");
    /// ```
    pub fn new(key_expr: &str) -> Self {
        gst::Object::builder()
            .property("key-expr", key_expr)
            .build()
            .unwrap()
    }

    /// Returns a builder for creating a ZenohSink with custom configuration.
    ///
    /// # Arguments
    ///
    /// * `key_expr` - The Zenoh key expression for publishing data
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gstzenoh::zenohsink::ZenohSink;
    ///
    /// gst::init().unwrap();
    /// gstzenoh::plugin_register_static().unwrap();
    ///
    /// let sink = ZenohSink::builder("demo/video")
    ///     .reliability("reliable")
    ///     .priority(2)
    ///     .express(true)
    ///     .congestion_control("block")
    ///     .build();
    /// ```
    pub fn builder(key_expr: &str) -> ZenohSinkBuilder {
        ZenohSinkBuilder::new(key_expr)
    }

    // -------------------------------------------------------------------------
    // Property Setters
    // -------------------------------------------------------------------------

    /// Sets the Zenoh key expression for publishing data.
    ///
    /// Must be set before the element is started.
    pub fn set_key_expr(&self, key_expr: &str) {
        self.set_property("key-expr", key_expr);
    }

    /// Sets the path to a Zenoh configuration file.
    ///
    /// The file should be in JSON5 format.
    pub fn set_config(&self, config_path: &str) {
        self.set_property("config", config_path);
    }

    /// Sets the publisher priority level.
    ///
    /// Valid values: 1-7
    /// - 1: RealTime (highest priority)
    /// - 2: InteractiveHigh
    /// - 3: InteractiveLow
    /// - 4: DataHigh
    /// - 5: Data (default)
    /// - 6: DataLow
    /// - 7: Background (lowest priority)
    pub fn set_priority(&self, priority: u32) {
        self.set_property("priority", priority);
    }

    /// Sets the congestion control policy.
    ///
    /// - `"block"`: Wait for network congestion to clear (default)
    /// - `"drop"`: Drop messages during congestion
    pub fn set_congestion_control(&self, mode: &str) {
        self.set_property("congestion-control", mode);
    }

    /// Sets the reliability mode.
    ///
    /// - `"best-effort"`: Fire-and-forget delivery (default)
    /// - `"reliable"`: Acknowledged delivery with retransmission
    pub fn set_reliability(&self, mode: &str) {
        self.set_property("reliability", mode);
    }

    /// Enables or disables express mode.
    ///
    /// Express mode bypasses internal queues for lower latency,
    /// but may increase CPU usage.
    pub fn set_express(&self, express: bool) {
        self.set_property("express", express);
    }

    /// Enables or disables sending GStreamer caps as metadata.
    pub fn set_send_caps(&self, send_caps: bool) {
        self.set_property("send-caps", send_caps);
    }

    /// Sets the interval in seconds for periodic caps transmission.
    ///
    /// Set to 0 to only send caps on the first buffer and on changes.
    pub fn set_caps_interval(&self, interval: u32) {
        self.set_property("caps-interval", interval);
    }

    /// Enables or disables sending buffer timing metadata (PTS, DTS, duration, flags).
    pub fn set_send_buffer_meta(&self, send: bool) {
        self.set_property("send-buffer-meta", send);
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
    /// use gstzenoh::ZenohSink;
    /// use zenoh::Wait;
    ///
    /// let session = zenoh::open(zenoh::Config::default()).wait()?;
    ///
    /// let sink1 = ZenohSink::new("demo/video");
    /// sink1.set_session(session.clone());
    ///
    /// let sink2 = ZenohSink::new("demo/audio");
    /// sink2.set_session(session);
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

    /// Returns the current priority level (1-7).
    pub fn priority(&self) -> u32 {
        self.property("priority")
    }

    /// Returns the current congestion control mode.
    pub fn congestion_control(&self) -> String {
        self.property("congestion-control")
    }

    /// Returns the current reliability mode.
    pub fn reliability(&self) -> String {
        self.property("reliability")
    }

    /// Returns whether express mode is enabled.
    pub fn express(&self) -> bool {
        self.property("express")
    }

    /// Returns whether caps are being sent as metadata.
    pub fn send_caps(&self) -> bool {
        self.property("send-caps")
    }

    /// Returns the caps transmission interval in seconds.
    pub fn caps_interval(&self) -> u32 {
        self.property("caps-interval")
    }

    /// Returns whether buffer timing metadata is being sent.
    pub fn send_buffer_meta(&self) -> bool {
        self.property("send-buffer-meta")
    }

    /// Returns the session group name, if set.
    pub fn session_group(&self) -> Option<String> {
        self.property("session-group")
    }

    // -------------------------------------------------------------------------
    // Matching Status
    // -------------------------------------------------------------------------

    /// Returns whether there are currently matching Zenoh subscribers.
    ///
    /// This reflects the last known state from Zenoh's matching listener.
    /// Returns `false` when the element is not started.
    pub fn has_subscribers(&self) -> bool {
        self.property("has-subscribers")
    }

    /// Connects to the `matching-changed` signal.
    ///
    /// The callback receives `true` when at least one matching subscriber
    /// appears, and `false` when the last matching subscriber disappears.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use gstzenoh::ZenohSink;
    ///
    /// let sink = ZenohSink::new("demo/video");
    /// sink.connect_matching_changed(|sink, has_subscribers| {
    ///     if has_subscribers {
    ///         println!("Subscribers connected for {}", sink.key_expr());
    ///     } else {
    ///         println!("No more subscribers for {}", sink.key_expr());
    ///     }
    /// });
    /// ```
    pub fn connect_matching_changed<F: Fn(&Self, bool) + Send + Sync + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect("matching-changed", false, move |values| {
            let element = values[0].get::<gst::Element>().unwrap();
            let sink = ZenohSink::try_from(element).unwrap();
            let matching = values[1].get::<bool>().unwrap();
            f(&sink, matching);
            None
        })
    }

    // -------------------------------------------------------------------------
    // Statistics (read-only)
    // -------------------------------------------------------------------------

    /// Returns the total number of bytes sent since the element started.
    pub fn bytes_sent(&self) -> u64 {
        self.property("bytes-sent")
    }

    /// Returns the total number of messages sent since the element started.
    pub fn messages_sent(&self) -> u64 {
        self.property("messages-sent")
    }

    /// Returns the total number of errors encountered.
    pub fn errors(&self) -> u64 {
        self.property("errors")
    }

    /// Returns the total number of messages dropped due to congestion.
    pub fn dropped(&self) -> u64 {
        self.property("dropped")
    }
}

impl TryFrom<gst::Element> for ZenohSink {
    type Error = gst::Element;

    /// Attempts to convert a generic GStreamer element to a ZenohSink.
    ///
    /// Returns the original element as an error if it's not a ZenohSink.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use gst::prelude::*;
    /// use gstzenoh::zenohsink::ZenohSink;
    ///
    /// gst::init().unwrap();
    /// gstzenoh::plugin_register_static().unwrap();
    ///
    /// let element = gst::ElementFactory::make("zenohsink")
    ///     .property("key-expr", "demo/video")
    ///     .build()
    ///     .unwrap();
    ///
    /// let sink = ZenohSink::try_from(element).expect("Should be a ZenohSink");
    /// assert_eq!(sink.key_expr(), "demo/video");
    /// ```
    fn try_from(element: gst::Element) -> Result<Self, Self::Error> {
        element.downcast()
    }
}

/// Builder for creating a [`ZenohSink`] with custom configuration.
///
/// # Example
///
/// ```no_run
/// use gstzenoh::zenohsink::ZenohSink;
///
/// gst::init().unwrap();
/// gstzenoh::plugin_register_static().unwrap();
///
/// let sink = ZenohSink::builder("demo/video")
///     .reliability("reliable")
///     .priority(2)
///     .express(true)
///     .send_caps(true)
///     .build();
/// ```
pub struct ZenohSinkBuilder {
    key_expr: String,
    config: Option<String>,
    priority: Option<u32>,
    congestion_control: Option<String>,
    reliability: Option<String>,
    express: Option<bool>,
    send_caps: Option<bool>,
    caps_interval: Option<u32>,
    send_buffer_meta: Option<bool>,
    session: Option<zenoh::Session>,
    session_group: Option<String>,
}

impl ZenohSinkBuilder {
    /// Creates a new builder with the required key expression.
    pub fn new(key_expr: &str) -> Self {
        Self {
            key_expr: key_expr.to_string(),
            config: None,
            priority: None,
            congestion_control: None,
            reliability: None,
            express: None,
            send_caps: None,
            caps_interval: None,
            send_buffer_meta: None,
            session: None,
            session_group: None,
        }
    }

    /// Sets the path to a Zenoh configuration file.
    pub fn config(mut self, path: &str) -> Self {
        self.config = Some(path.to_string());
        self
    }

    /// Sets the publisher priority level (1-7).
    pub fn priority(mut self, priority: u32) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Sets the congestion control policy ("block" or "drop").
    pub fn congestion_control(mut self, mode: &str) -> Self {
        self.congestion_control = Some(mode.to_string());
        self
    }

    /// Sets the reliability mode ("best-effort" or "reliable").
    pub fn reliability(mut self, mode: &str) -> Self {
        self.reliability = Some(mode.to_string());
        self
    }

    /// Enables or disables express mode.
    pub fn express(mut self, express: bool) -> Self {
        self.express = Some(express);
        self
    }

    /// Enables or disables sending caps as metadata.
    pub fn send_caps(mut self, send: bool) -> Self {
        self.send_caps = Some(send);
        self
    }

    /// Sets the caps transmission interval in seconds.
    pub fn caps_interval(mut self, interval: u32) -> Self {
        self.caps_interval = Some(interval);
        self
    }

    /// Enables or disables sending buffer timing metadata.
    pub fn send_buffer_meta(mut self, send: bool) -> Self {
        self.send_buffer_meta = Some(send);
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

    /// Builds the ZenohSink with the configured properties.
    pub fn build(self) -> ZenohSink {
        let mut builder = gst::Object::builder::<ZenohSink>().property("key-expr", &self.key_expr);

        if let Some(config) = self.config {
            builder = builder.property("config", config);
        }
        if let Some(priority) = self.priority {
            builder = builder.property("priority", priority);
        }
        if let Some(cc) = self.congestion_control {
            builder = builder.property("congestion-control", cc);
        }
        if let Some(rel) = self.reliability {
            builder = builder.property("reliability", rel);
        }
        if let Some(exp) = self.express {
            builder = builder.property("express", exp);
        }
        if let Some(sc) = self.send_caps {
            builder = builder.property("send-caps", sc);
        }
        if let Some(ci) = self.caps_interval {
            builder = builder.property("caps-interval", ci);
        }
        if let Some(sbm) = self.send_buffer_meta {
            builder = builder.property("send-buffer-meta", sbm);
        }
        if let Some(ref sg) = self.session_group {
            builder = builder.property("session-group", sg);
        }

        let sink: ZenohSink = builder.build().unwrap();

        // Set the session directly (can't be done via properties)
        if let Some(session) = self.session {
            sink.set_session(session);
        }

        sink
    }
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "zenohsink",
        gst::Rank::NONE + 100, // Higher than MARGINAL to be discoverable
        ZenohSink::static_type(),
    )
}
