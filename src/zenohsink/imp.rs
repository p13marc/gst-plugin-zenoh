use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use gst::subclass::prelude::URIHandlerImpl;
use gst::{glib, prelude::*, subclass::prelude::*};
use gst_base::prelude::BaseSinkExtManual;
use gst_base::subclass::prelude::*;
use zenoh::Wait;
use zenoh::key_expr::OwnedKeyExpr;
use zenoh::qos::{CongestionControl, Priority, Reliability};

use crate::error::{ErrorHandling, FlowErrorHandling, ZenohError};
use crate::metadata::MetadataBuilder;

static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new(
        "zenohsink",
        gst::DebugColorFlags::empty(),
        Some("Zenoh Sink"),
    )
});

/// Statistics tracking for ZenohSink
#[derive(Debug, Clone, Default)]
struct Statistics {
    bytes_sent: u64,
    messages_sent: u64,
    errors: u64,
    dropped: u64, // For congestion-control=drop mode
    #[allow(dead_code)]
    start_time: Option<gst::ClockTime>,
    #[cfg(any(
        feature = "compression-zstd",
        feature = "compression-lz4",
        feature = "compression-gzip"
    ))]
    bytes_before_compression: u64,
    #[cfg(any(
        feature = "compression-zstd",
        feature = "compression-lz4",
        feature = "compression-gzip"
    ))]
    bytes_after_compression: u64,
}

struct Started {
    // Keeping session field to maintain ownership and prevent session from being dropped
    // while publisher is still in use. This can be either owned or shared.
    #[allow(dead_code)]
    session: SessionWrapper,
    publisher: zenoh::pubsub::Publisher<'static>,
    /// Statistics tracking (shared for thread-safe updates)
    stats: Arc<Mutex<Statistics>>,
    /// Track if we've sent caps metadata yet (for first buffer)
    caps_sent: Arc<std::sync::atomic::AtomicBool>,
    /// Last time caps were sent (for periodic transmission)
    last_caps_time: Arc<Mutex<Option<std::time::Instant>>>,
    /// Last caps that were sent (for change detection)
    last_caps: Arc<Mutex<Option<gst::Caps>>>,
}

/// Wrapper to handle both owned and shared Zenoh sessions.
///
/// This allows the plugin to either create its own session or use
/// a shared session provided externally, enabling session reuse
/// across multiple GStreamer elements.
///
/// Note: `zenoh::Session` is internally Arc-based and Clone, so the
/// distinction between Owned and Shared is mainly for documentation
/// purposes - both variants use the same underlying type.
enum SessionWrapper {
    /// Element created this session (will be dropped when element stops)
    Owned(zenoh::Session),
    /// Element is using a shared session (may outlive this element)
    Shared(zenoh::Session),
}

impl SessionWrapper {
    /// Get a reference to the underlying Zenoh session
    fn as_session(&self) -> &zenoh::Session {
        match self {
            SessionWrapper::Owned(session) => session,
            SessionWrapper::Shared(session) => session,
        }
    }
}

#[derive(Default)]
enum State {
    #[default]
    Stopped,
    Starting, // Intermediate state during startup
    Started(Started),
    Stopping, // Intermediate state during shutdown
}

impl State {
    fn is_started(&self) -> bool {
        matches!(self, State::Started(_))
    }

    fn is_stopped(&self) -> bool {
        matches!(self, State::Stopped)
    }

    fn can_start(&self) -> bool {
        matches!(self, State::Stopped)
    }

    fn can_stop(&self) -> bool {
        matches!(self, State::Started(_))
    }
}

/// Configuration settings for the ZenohSink element.
///
/// These settings control how the element connects to and publishes
/// data via the Zenoh network protocol.
#[derive(Debug)]
struct Settings {
    /// Zenoh key expression for publishing data (required)
    key_expr: String,
    /// Optional path to Zenoh configuration file
    config_file: Option<String>,
    /// Publisher priority level (1-7: 1=RealTime, 2=InteractiveHigh, 3=InteractiveLow, 4=DataHigh, 5=Data(default), 6=DataLow, 7=Background)
    priority: u8,
    /// Congestion control policy: "block" or "drop"
    congestion_control: String,
    /// Reliability mode: "best-effort" or "reliable"
    reliability: String,
    /// Enable express mode for lower latency (bypasses some queues)
    express: bool,
    /// Send GStreamer caps as metadata with buffers (default: true)
    send_caps: bool,
    /// Interval in seconds to send caps periodically (0 = only on first buffer and changes, default: 1)
    caps_interval: u32,
    /// Send buffer timing metadata (PTS, DTS, duration, flags) with each buffer (default: true)
    send_buffer_meta: bool,
    /// Compression algorithm to use (requires compression features)
    #[cfg(any(
        feature = "compression-zstd",
        feature = "compression-lz4",
        feature = "compression-gzip"
    ))]
    compression: crate::compression::CompressionType,
    /// Compression level (1-9, higher = better compression but slower)
    #[cfg(any(
        feature = "compression-zstd",
        feature = "compression-lz4",
        feature = "compression-gzip"
    ))]
    compression_level: i32,
    /// Optional external Zenoh session to share with other elements (Rust API)
    external_session: Option<zenoh::Session>,
    /// Session group name for sharing sessions via property (gst-launch compatible)
    session_group: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            key_expr: String::new(),
            config_file: None,
            priority: 5, // Default to Priority::Data
            congestion_control: "block".into(),
            reliability: "best-effort".into(),
            express: false,
            send_caps: true,        // Default to sending caps for ease of use
            caps_interval: 1,       // Send caps every 1 second by default
            send_buffer_meta: true, // Default to sending buffer timing metadata
            #[cfg(any(
                feature = "compression-zstd",
                feature = "compression-lz4",
                feature = "compression-gzip"
            ))]
            compression: crate::compression::CompressionType::None,
            #[cfg(any(
                feature = "compression-zstd",
                feature = "compression-lz4",
                feature = "compression-gzip"
            ))]
            compression_level: 5, // Medium compression level
            external_session: None,
            session_group: None,
        }
    }
}

// Note: We don't define enums for Reliability and CongestionControl
// here since Zenoh already has them, but we expose string properties
// to the GStreamer API for compatibility and future extension

/// GStreamer ZenohSink element implementation.
///
/// This element receives buffers from upstream GStreamer elements
/// and publishes them to a Zenoh network using the configured
/// key expression and quality of service parameters.
///
/// The element supports:
/// - Configurable reliability (best-effort/reliable)
/// - Congestion control (block/drop)
/// - Express mode for low latency
/// - Priority-based message ordering
/// - Session sharing capabilities
pub struct ZenohSink {
    /// Element configuration settings
    settings: Mutex<Settings>,
    /// Current operational state
    state: Mutex<State>,
}

impl Default for ZenohSink {
    fn default() -> Self {
        Self {
            settings: Mutex::new(Settings::default()),
            state: Mutex::new(State::default()),
        }
    }
}

impl ZenohSink {
    /// Sets the external Zenoh session to use for this element.
    ///
    /// This is called from the public API to enable session sharing.
    pub(crate) fn set_external_session(&self, session: zenoh::Session) {
        let mut settings = self.settings.lock().unwrap();
        settings.external_session = Some(session);
    }
}

impl GstObjectImpl for ZenohSink {}

impl ElementImpl for ZenohSink {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static ELEMENT_METADATA: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
            gst::subclass::ElementMetadata::new(
                "Zenoh Network Sink",
                "Sink/Network/Protocol",
                "Publishes GStreamer buffers to Zenoh networks with configurable QoS (reliability, priority, express mode)",
                "Marc Pardo <p13marc@gmail.com>",
            )
        });
        Some(&*ELEMENT_METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static PAD_TEMPLATES: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
            let sink_pad_template = gst::PadTemplate::new(
                "sink",
                gst::PadDirection::Sink,
                gst::PadPresence::Always,
                &gst::Caps::new_any(),
            )
            .unwrap();

            vec![sink_pad_template]
        });

        PAD_TEMPLATES.as_ref()
    }

    fn change_state(
        &self,
        transition: gst::StateChange,
    ) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        self.parent_change_state(transition)
    }
}

impl ObjectImpl for ZenohSink {
    fn constructed(&self) {
        self.parent_constructed();
    }

    fn properties() -> &'static [gst::glib::ParamSpec] {
        static PROPERTIES: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| {
            vec![
                // Key expression property
                glib::ParamSpecString::builder("key-expr")
                    .nick("Zenoh Key Expression")
                    .blurb("Zenoh key expression for publishing data (e.g., 'demo/video/stream', 'sensors/{device_id}/**')")
                    .build(),
                // Config file property
                glib::ParamSpecString::builder("config")
                    .nick("Zenoh Configuration")
                    .blurb("Path to Zenoh configuration file for custom network settings (JSON5 format)")
                    .build(),
                // Priority property
                glib::ParamSpecUInt::builder("priority")
                    .nick("Publisher Priority")
                    .blurb("Message priority level: 1=RealTime(highest), 2=InteractiveHigh, 3=InteractiveLow, 4=DataHigh, 5=Data(default), 6=DataLow, 7=Background(lowest)")
                    .default_value(5)
                    .minimum(1)
                    .maximum(7)
                    .build(),
                // Congestion control property
                glib::ParamSpecString::builder("congestion-control")
                    .nick("Congestion Control")
                    .blurb("Network congestion handling: 'block' (wait for delivery, ensures reliability) or 'drop' (drop messages, maintains real-time performance)")
                    .default_value(Some("block"))
                    .build(),
                // Reliability property
                glib::ParamSpecString::builder("reliability")
                    .nick("Reliability Mode")
                    .blurb("Message delivery guarantee: 'best-effort' (lower latency, may lose messages) or 'reliable' (acknowledged delivery with retransmission)")
                    .default_value(Some("best-effort"))
                    .build(),
                // Express mode property
                glib::ParamSpecBoolean::builder("express")
                    .nick("Express Mode")
                    .blurb("Enable ultra-low latency mode by bypassing internal queues (increases CPU usage but reduces end-to-end latency)")
                    .default_value(false)
                    .build(),
                // Send caps property
                glib::ParamSpecBoolean::builder("send-caps")
                    .nick("Send Capabilities")
                    .blurb("Attach GStreamer caps as metadata to buffers for automatic format negotiation")
                    .default_value(true)
                    .build(),
                // Caps interval property
                glib::ParamSpecUInt::builder("caps-interval")
                    .nick("Caps Transmission Interval")
                    .blurb("Interval in seconds to send caps periodically (0 = only first buffer and format changes, reduces bandwidth)")
                    .default_value(1)
                    .minimum(0)
                    .maximum(3600)
                    .build(),
                // Buffer metadata property
                glib::ParamSpecBoolean::builder("send-buffer-meta")
                    .nick("Send Buffer Metadata")
                    .blurb("Send buffer timing metadata (PTS, DTS, duration, offset, flags) with each buffer for proper A/V sync")
                    .default_value(true)
                    .build(),
                // Compression properties (conditional on features)
                #[cfg(any(
                    feature = "compression-zstd",
                    feature = "compression-lz4",
                    feature = "compression-gzip"
                ))]
                glib::ParamSpecEnum::builder_with_default("compression", crate::compression::CompressionType::None)
                    .nick("Compression")
                    .blurb("Compression algorithm to use: none (default), zstd (best ratio), lz4 (fastest), or gzip (compatible)")
                    .build(),
                #[cfg(any(
                    feature = "compression-zstd",
                    feature = "compression-lz4",
                    feature = "compression-gzip"
                ))]
                glib::ParamSpecInt::builder("compression-level")
                    .nick("Compression Level")
                    .blurb("Compression level (1=fastest/largest, 9=slowest/smallest, 5=balanced default)")
                    .default_value(5)
                    .minimum(1)
                    .maximum(9)
                    .build(),
                // Session sharing property
                glib::ParamSpecString::builder("session-group")
                    .nick("Session Group")
                    .blurb("Name of the session group for sharing Zenoh sessions across elements. Elements with the same group name share a single session.")
                    .build(),
                // Statistics properties (read-only)
                glib::ParamSpecUInt64::builder("bytes-sent")
                    .nick("Bytes Sent")
                    .blurb("Total bytes sent since element started")
                    .read_only()
                    .build(),
                glib::ParamSpecUInt64::builder("messages-sent")
                    .nick("Messages Sent")
                    .blurb("Total messages sent since element started")
                    .read_only()
                    .build(),
                glib::ParamSpecUInt64::builder("errors")
                    .nick("Errors")
                    .blurb("Total number of errors encountered")
                    .read_only()
                    .build(),
                glib::ParamSpecUInt64::builder("dropped")
                    .nick("Dropped")
                    .blurb("Total messages dropped due to congestion (drop mode)")
                    .read_only()
                    .build(),
                // Compression statistics (conditional on features)
                #[cfg(any(
                    feature = "compression-zstd",
                    feature = "compression-lz4",
                    feature = "compression-gzip"
                ))]
                glib::ParamSpecUInt64::builder("bytes-before-compression")
                    .nick("Bytes Before Compression")
                    .blurb("Total bytes before compression")
                    .read_only()
                    .build(),
                #[cfg(any(
                    feature = "compression-zstd",
                    feature = "compression-lz4",
                    feature = "compression-gzip"
                ))]
                glib::ParamSpecUInt64::builder("bytes-after-compression")
                    .nick("Bytes After Compression")
                    .blurb("Total bytes after compression (actually sent over network)")
                    .read_only()
                    .build(),
            ]
        });

        PROPERTIES.as_ref()
    }

    fn set_property(&self, _id: usize, value: &gst::glib::Value, pspec: &gst::glib::ParamSpec) {
        // Check if we're in a state where property changes are allowed
        let state = self.state.lock().unwrap();
        if state.is_started()
            && matches!(
                pspec.name(),
                "key-expr"
                    | "config"
                    | "express"
                    | "reliability"
                    | "congestion-control"
                    | "priority"
                    | "session-group"
            )
        {
            gst::warning!(
                CAT,
                "Cannot change property '{}' while element is started",
                pspec.name()
            );
            return;
        }
        drop(state);

        // Note: priority, express, reliability, and congestion-control are locked after start
        // because Zenoh Publishers are immutable - QoS is set during publisher creation.
        // The Zenoh API does not support changing QoS parameters on publisher.put().
        // To implement runtime QoS changes would require recreating the publisher,
        // which adds significant complexity and risk of data loss during transition.
        //
        // Properties that CAN be changed at runtime:
        // - send-caps: Simple boolean check
        // - caps-interval: Simple integer check
        // - compression: Applied per-buffer
        // - compression-level: Applied per-buffer

        let mut settings = self.settings.lock().unwrap();

        match pspec.name() {
            "key-expr" => {
                settings.key_expr = value.get::<String>().expect("type checked upstream");
            }
            "config" => {
                settings.config_file = value
                    .get::<Option<String>>()
                    .expect("type checked upstream");
            }
            "priority" => {
                let priority_val = value.get::<u32>().expect("type checked upstream") as u8;
                // Validate priority range
                if (1..=7).contains(&priority_val) {
                    settings.priority = priority_val;
                } else {
                    gst::warning!(
                        CAT,
                        "Invalid priority value '{}', must be 1-7, using default",
                        priority_val
                    );
                    settings.priority = 5; // Default to Priority::Data
                }
            }
            "congestion-control" => {
                let control = value.get::<String>().expect("type checked upstream");
                // Validate value
                match control.as_str() {
                    "block" | "drop" => settings.congestion_control = control,
                    _ => gst::warning!(
                        CAT,
                        "Invalid congestion control value '{}', using default",
                        control
                    ),
                }
            }
            "reliability" => {
                let reliability = value.get::<String>().expect("type checked upstream");
                // Validate value
                match reliability.as_str() {
                    "best-effort" | "reliable" => settings.reliability = reliability,
                    _ => gst::warning!(
                        CAT,
                        "Invalid reliability value '{}', using default",
                        reliability
                    ),
                }
            }
            "express" => {
                settings.express = value.get::<bool>().expect("type checked upstream");
            }
            "send-caps" => {
                settings.send_caps = value.get::<bool>().expect("type checked upstream");
            }
            "caps-interval" => {
                settings.caps_interval = value.get::<u32>().expect("type checked upstream");
            }
            "send-buffer-meta" => {
                settings.send_buffer_meta = value.get::<bool>().expect("type checked upstream");
            }
            #[cfg(any(
                feature = "compression-zstd",
                feature = "compression-lz4",
                feature = "compression-gzip"
            ))]
            "compression" => {
                settings.compression = value
                    .get::<crate::compression::CompressionType>()
                    .expect("type checked upstream");
            }
            #[cfg(any(
                feature = "compression-zstd",
                feature = "compression-lz4",
                feature = "compression-gzip"
            ))]
            "compression-level" => {
                let level = value.get::<i32>().expect("type checked upstream");
                if (1..=9).contains(&level) {
                    settings.compression_level = level;
                } else {
                    gst::warning!(
                        CAT,
                        "Invalid compression level '{}', must be 1-9, using default",
                        level
                    );
                    settings.compression_level = 5;
                }
            }
            "session-group" => {
                settings.session_group = value
                    .get::<Option<String>>()
                    .expect("type checked upstream");
            }
            name => {
                gst::warning!(CAT, "Unknown property: {}", name);
            }
        }
    }

    fn property(&self, _id: usize, pspec: &gst::glib::ParamSpec) -> gst::glib::Value {
        match pspec.name() {
            // Configuration properties - read from settings
            "key-expr" | "config" | "priority" | "congestion-control" | "reliability"
            | "express" | "send-caps" | "caps-interval" | "send-buffer-meta" | "session-group" => {
                let settings = self.settings.lock().unwrap();
                match pspec.name() {
                    "key-expr" => settings.key_expr.to_value(),
                    "config" => settings.config_file.to_value(),
                    "priority" => (settings.priority as u32).to_value(),
                    "congestion-control" => settings.congestion_control.to_value(),
                    "reliability" => settings.reliability.to_value(),
                    "express" => settings.express.to_value(),
                    "send-caps" => settings.send_caps.to_value(),
                    "caps-interval" => settings.caps_interval.to_value(),
                    "send-buffer-meta" => settings.send_buffer_meta.to_value(),
                    "session-group" => settings.session_group.to_value(),
                    _ => unreachable!(),
                }
            }
            #[cfg(any(
                feature = "compression-zstd",
                feature = "compression-lz4",
                feature = "compression-gzip"
            ))]
            "compression" => {
                let settings = self.settings.lock().unwrap();
                settings.compression.to_value()
            }
            #[cfg(any(
                feature = "compression-zstd",
                feature = "compression-lz4",
                feature = "compression-gzip"
            ))]
            "compression-level" => {
                let settings = self.settings.lock().unwrap();
                settings.compression_level.to_value()
            }
            // Statistics properties - read from state
            "bytes-sent" => {
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    started.stats.lock().unwrap().bytes_sent.to_value()
                } else {
                    0u64.to_value()
                }
            }
            "messages-sent" => {
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    started.stats.lock().unwrap().messages_sent.to_value()
                } else {
                    0u64.to_value()
                }
            }
            "errors" => {
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    started.stats.lock().unwrap().errors.to_value()
                } else {
                    0u64.to_value()
                }
            }
            "dropped" => {
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    started.stats.lock().unwrap().dropped.to_value()
                } else {
                    0u64.to_value()
                }
            }
            #[cfg(any(
                feature = "compression-zstd",
                feature = "compression-lz4",
                feature = "compression-gzip"
            ))]
            "bytes-before-compression" => {
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    started
                        .stats
                        .lock()
                        .unwrap()
                        .bytes_before_compression
                        .to_value()
                } else {
                    0u64.to_value()
                }
            }
            #[cfg(any(
                feature = "compression-zstd",
                feature = "compression-lz4",
                feature = "compression-gzip"
            ))]
            "bytes-after-compression" => {
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    started
                        .stats
                        .lock()
                        .unwrap()
                        .bytes_after_compression
                        .to_value()
                } else {
                    0u64.to_value()
                }
            }
            name => {
                gst::warning!(CAT, "Unknown property: {}", name);
                // Return an empty string value as default
                "".to_value()
            }
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for ZenohSink {
    const NAME: &'static str = "GstZenohSink";
    type Type = super::ZenohSink;
    type ParentType = gst_base::BaseSink;
    type Interfaces = (gst::URIHandler,);
}

impl BaseSinkImpl for ZenohSink {
    fn start(&self) -> Result<(), gst::ErrorMessage> {
        let mut state = self.state.lock().unwrap();

        // Check if we can start from current state
        if !state.can_start() {
            let current_state = match *state {
                State::Stopped => "Stopped",
                State::Starting => "Starting",
                State::Started(_) => "Started",
                State::Stopping => "Stopping",
            };
            gst::warning!(
                CAT,
                "Cannot start ZenohSink from state: {}, ignoring start request",
                current_state
            );
            if state.is_started() {
                return Ok(()); // Already started is not an error
            } else {
                return Err(gst::error_msg!(
                    gst::ResourceError::Settings,
                    ["Cannot start from current state: {}", current_state]
                ));
            }
        }

        gst::debug!(CAT, "ZenohSink transitioning from Stopped to Starting");
        *state = State::Starting;
        drop(state); // Release state lock before potentially long operations

        let settings = self.settings.lock().unwrap();
        let key_expr = settings.key_expr.clone();
        let config_file = settings.config_file.clone();
        let priority = settings.priority;
        let congestion_control = settings.congestion_control.clone();
        let reliability = settings.reliability.clone();
        let express = settings.express;
        let external_session = settings.external_session.clone();
        let session_group = settings.session_group.clone();
        drop(settings);

        // Validate the key expression
        if key_expr.is_empty() {
            return Err(gst::error_msg!(
                gst::ResourceError::Settings,
                ["Key expression is required"]
            ));
        }

        // Determine session source: external (Rust API) > session-group (property) > new session
        let session_wrapper = if let Some(shared_session) = external_session {
            // Priority 1: External session provided via Rust API
            gst::debug!(CAT, "Using external shared session (Rust API)");
            SessionWrapper::Shared(shared_session)
        } else if let Some(ref group) = session_group {
            // Priority 2: Session group property (gst-launch compatible)
            gst::debug!(CAT, "Using session group '{}'", group);
            let session = crate::session::get_or_create_session(group, config_file.as_deref())
                .map_err(|e| ZenohError::InitError(e).to_error_message())?;
            SessionWrapper::Shared(session)
        } else {
            // Priority 3: Create a new owned session
            gst::debug!(CAT, "Creating new Zenoh session");
            let config = match config_file {
                Some(path) if !path.is_empty() => {
                    gst::debug!(CAT, "Loading Zenoh config from {}", path);
                    zenoh::Config::from_file(&path)
                        .map_err(|e| ZenohError::InitError(e).to_error_message())?
                }
                _ => zenoh::Config::default(),
            };
            let session = zenoh::open(config)
                .wait()
                .map_err(|e| ZenohError::InitError(e).to_error_message())?;
            SessionWrapper::Owned(session)
        };

        gst::debug!(
            CAT,
            "Creating publisher with key_expr='{}', priority={}, congestion_control='{}', reliability='{}', express={}",
            key_expr,
            priority,
            congestion_control,
            reliability,
            express
        );

        // Use owned key_expr for static lifetime, with proper error handling
        let owned = OwnedKeyExpr::try_from(key_expr.clone()).map_err(|e| {
            ZenohError::KeyExprError {
                key_expr: key_expr.clone(),
                reason: e.to_string(),
            }
            .to_error_message()
        })?;

        // Parse and validate configuration options for Zenoh publisher

        // Priority: Zenoh priority levels (lower numeric value = higher priority)
        // 1=RealTime, 2=InteractiveHigh, 3=InteractiveLow, 4=DataHigh, 5=Data, 6=DataLow, 7=Background
        let zenoh_priority = Priority::try_from(priority).unwrap_or(Priority::default());

        // Congestion control: How to handle network congestion
        // - Block: Wait until congestion clears (ensures delivery but may cause delays)
        // - Drop: Drop messages during congestion (maintains throughput but may lose data)
        let zenoh_congestion_control = match congestion_control.as_str() {
            "block" => CongestionControl::Block,
            "drop" => CongestionControl::Drop,
            _ => {
                gst::warning!(
                    CAT,
                    "Unknown congestion control '{}', using default",
                    congestion_control
                );
                CongestionControl::Block
            }
        };

        // Reliability: Message delivery guarantees
        // - Reliable: Messages are acknowledged and retransmitted if lost
        // - BestEffort: Messages sent once without delivery guarantees (lower latency)
        let zenoh_reliability = match reliability.as_str() {
            "reliable" => Reliability::Reliable,
            "best-effort" => Reliability::BestEffort,
            _ => {
                gst::warning!(CAT, "Unknown reliability '{}', using default", reliability);
                Reliability::BestEffort
            }
        };

        // Create publisher with full configuration
        // Start with the key expression and add QoS parameters
        let mut publisher_builder = session_wrapper
            .as_session()
            .declare_publisher(owned)
            .priority(zenoh_priority)
            .congestion_control(zenoh_congestion_control)
            .reliability(zenoh_reliability);

        // Express mode: Bypass some internal queues for reduced latency
        // Trade-off: Lower latency vs potentially higher CPU usage
        if express {
            publisher_builder = publisher_builder.express(true);
        }

        let publisher = publisher_builder.wait().map_err(|e| {
            ZenohError::PublishError {
                key_expr: key_expr.clone(),
                source: e,
            }
            .to_error_message()
        })?;

        gst::debug!(
            CAT,
            "Publisher created with key_expr='{}', priority={}, congestion_control='{}', reliability='{}', express={}",
            key_expr,
            priority,
            congestion_control,
            reliability,
            express
        );

        // Reacquire state lock to complete transition
        let mut state = self.state.lock().unwrap();

        // Verify we're still in Starting state (not stopped during initialization)
        if !matches!(*state, State::Starting) {
            gst::warning!(
                CAT,
                "State changed during startup, aborting start operation"
            );
            return Err(gst::error_msg!(
                gst::ResourceError::Settings,
                ["State changed during startup"]
            ));
        }

        *state = State::Started(Started {
            session: session_wrapper,
            publisher,
            stats: Arc::new(Mutex::new(Statistics {
                start_time: Some(gst::ClockTime::from_nseconds(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() as u64,
                )),
                ..Default::default()
            })),
            caps_sent: Arc::new(AtomicBool::new(false)),
            last_caps_time: Arc::new(Mutex::new(None)),
            last_caps: Arc::new(Mutex::new(None)),
        });
        gst::debug!(CAT, "ZenohSink successfully transitioned to Started state");

        Ok(())
    }

    fn render(&self, buffer: &gst::Buffer) -> Result<gst::FlowSuccess, gst::FlowError> {
        let state_locked = self.state.lock().unwrap();
        let State::Started(ref started) = *state_locked else {
            gst::element_imp_error!(self, gst::CoreError::Failed, ["Not started yet"]);
            return Err(gst::FlowError::Error);
        };

        // Get buffer data with proper error handling
        let b = buffer.clone().into_mapped_buffer_readable().map_err(|_| {
            gst::element_imp_error!(
                self,
                gst::ResourceError::Read,
                ["Failed to map buffer for reading"]
            );
            gst::FlowError::Error
        })?;

        // Get original size for compression statistics
        #[cfg(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        ))]
        let original_size = b.len();

        #[cfg(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        ))]
        let (compression_type, compression_level) = {
            let settings = self.settings.lock().unwrap();
            (settings.compression, settings.compression_level)
        };

        // Apply compression if enabled
        // Use Cow to avoid unnecessary copy when compression is disabled
        #[cfg(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        ))]
        let (data_to_send, compressed): (std::borrow::Cow<'_, [u8]>, bool) = if compression_type
            != crate::compression::CompressionType::None
        {
            match crate::compression::compress(b.as_slice(), compression_type, compression_level) {
                Ok(compressed_data) => {
                    gst::trace!(
                        CAT,
                        imp = self,
                        "Compressed {} bytes to {} bytes using {:?} (level {}), ratio: {:.2}%",
                        original_size,
                        compressed_data.len(),
                        compression_type,
                        compression_level,
                        (compressed_data.len() as f64 / original_size as f64) * 100.0
                    );
                    (std::borrow::Cow::Owned(compressed_data), true)
                }
                Err(e) => {
                    gst::warning!(
                        CAT,
                        imp = self,
                        "Compression failed: {}, sending uncompressed",
                        e
                    );
                    started.stats.lock().unwrap().errors += 1;
                    // No copy - borrow the original slice
                    (std::borrow::Cow::Borrowed(b.as_slice()), false)
                }
            }
        } else {
            // No compression - borrow the original slice (zero-copy)
            (std::borrow::Cow::Borrowed(b.as_slice()), false)
        };

        #[cfg(not(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        )))]
        // No compression features - borrow the original slice (zero-copy)
        let (data_to_send, compressed): (std::borrow::Cow<'_, [u8]>, bool) =
            (std::borrow::Cow::Borrowed(b.as_slice()), false);

        // Smart caps transmission: send caps when needed, not on every buffer
        let (send_caps, caps_interval, send_buffer_meta) = {
            let settings = self.settings.lock().unwrap();
            (
                settings.send_caps,
                settings.caps_interval,
                settings.send_buffer_meta,
            )
        };

        let attachment = if send_caps {
            if let Some(caps) = self.obj().sink_pad().current_caps() {
                let mut should_send = false;

                // Check if this is the first buffer (always send)
                if !started.caps_sent.load(Ordering::Acquire) {
                    gst::debug!(CAT, imp = self, "Sending caps on first buffer: {}", caps);
                    should_send = true;
                    started.caps_sent.store(true, Ordering::Release);
                    *started.last_caps.lock().unwrap() = Some(caps.clone());
                    *started.last_caps_time.lock().unwrap() = Some(std::time::Instant::now());
                } else {
                    // Check if caps have changed (always send on change)
                    let last_caps = started.last_caps.lock().unwrap();
                    if last_caps.as_ref() != Some(&caps) {
                        gst::debug!(
                            CAT,
                            imp = self,
                            "Caps changed, sending updated caps: {}",
                            caps
                        );
                        should_send = true;
                        drop(last_caps);
                        *started.last_caps.lock().unwrap() = Some(caps.clone());
                        *started.last_caps_time.lock().unwrap() = Some(std::time::Instant::now());
                    } else if caps_interval > 0 {
                        // Check if it's time for periodic transmission
                        let last_time = started.last_caps_time.lock().unwrap();
                        if let Some(last) = *last_time
                            && last.elapsed().as_secs() >= caps_interval as u64
                        {
                            gst::trace!(
                                CAT,
                                imp = self,
                                "Periodic caps transmission (interval: {}s)",
                                caps_interval
                            );
                            should_send = true;
                            drop(last_time);
                            *started.last_caps_time.lock().unwrap() =
                                Some(std::time::Instant::now());
                        }
                    }
                }

                if should_send {
                    let mut metadata_builder = MetadataBuilder::new().caps(&caps);

                    // Add buffer timing metadata if enabled
                    if send_buffer_meta {
                        metadata_builder = metadata_builder.buffer_timing(buffer);
                    }

                    // Add compression metadata if compressed
                    #[cfg(any(
                        feature = "compression-zstd",
                        feature = "compression-lz4",
                        feature = "compression-gzip"
                    ))]
                    if compressed {
                        metadata_builder = metadata_builder.user_metadata(
                            crate::metadata::keys::COMPRESSION,
                            compression_type.to_metadata_value(),
                        );
                    }

                    metadata_builder.build()
                } else {
                    // Not sending caps, but may still need buffer timing or compression metadata
                    let needs_metadata = send_buffer_meta || compressed;

                    if needs_metadata {
                        let mut metadata_builder = MetadataBuilder::new();

                        if send_buffer_meta {
                            metadata_builder = metadata_builder.buffer_timing(buffer);
                        }

                        #[cfg(any(
                            feature = "compression-zstd",
                            feature = "compression-lz4",
                            feature = "compression-gzip"
                        ))]
                        if compressed {
                            metadata_builder = metadata_builder.user_metadata(
                                crate::metadata::keys::COMPRESSION,
                                compression_type.to_metadata_value(),
                            );
                        }

                        metadata_builder.build()
                    } else {
                        None
                    }
                }
            } else {
                // No caps available, but may still need buffer timing or compression metadata
                let needs_metadata = send_buffer_meta || compressed;

                if needs_metadata {
                    let mut metadata_builder = MetadataBuilder::new();

                    if send_buffer_meta {
                        metadata_builder = metadata_builder.buffer_timing(buffer);
                    }

                    #[cfg(any(
                        feature = "compression-zstd",
                        feature = "compression-lz4",
                        feature = "compression-gzip"
                    ))]
                    if compressed {
                        metadata_builder = metadata_builder.user_metadata(
                            crate::metadata::keys::COMPRESSION,
                            compression_type.to_metadata_value(),
                        );
                    }

                    metadata_builder.build()
                } else {
                    None
                }
            }
        } else {
            // send_caps is false, but may still need buffer timing or compression metadata
            let needs_metadata = send_buffer_meta || compressed;

            if needs_metadata {
                let mut metadata_builder = MetadataBuilder::new();

                if send_buffer_meta {
                    metadata_builder = metadata_builder.buffer_timing(buffer);
                }

                #[cfg(any(
                    feature = "compression-zstd",
                    feature = "compression-lz4",
                    feature = "compression-gzip"
                ))]
                if compressed {
                    metadata_builder = metadata_builder.user_metadata(
                        crate::metadata::keys::COMPRESSION,
                        compression_type.to_metadata_value(),
                    );
                }

                metadata_builder.build()
            } else {
                None
            }
        };

        // Send with caps attachment
        // Note: Zenoh's wait() already handles timeouts internally
        let put_builder = started.publisher.put(&data_to_send);
        let result = if let Some(attachment) = attachment {
            put_builder.attachment(attachment).wait()
        } else {
            put_builder.wait()
        };

        match result {
            Ok(_) => {
                // Update statistics on success
                let mut stats = started.stats.lock().unwrap();
                stats.bytes_sent += data_to_send.len() as u64;
                stats.messages_sent += 1;

                #[cfg(any(
                    feature = "compression-zstd",
                    feature = "compression-lz4",
                    feature = "compression-gzip"
                ))]
                if compressed {
                    stats.bytes_before_compression += original_size as u64;
                    stats.bytes_after_compression += data_to_send.len() as u64;
                }

                Ok(gst::FlowSuccess::Ok)
            }
            Err(e) => {
                // Update error statistics
                started.stats.lock().unwrap().errors += 1;

                // Get key expression for better error reporting
                let key_expr = self.settings.lock().unwrap().key_expr.clone();

                // Check if this is a network-related error before consuming e
                let error_msg = format!("{}", e);
                let err = ZenohError::PublishError {
                    key_expr,
                    source: e,
                };

                if error_msg.contains("timeout")
                    || error_msg.contains("connection")
                    || error_msg.contains("network")
                {
                    gst::element_imp_error!(
                        self,
                        gst::ResourceError::Write,
                        ["Network error while publishing: {}", err]
                    );
                } else {
                    gst::element_imp_error!(self, gst::ResourceError::Write, ["{}", err]);
                }
                Err(err.to_flow_error())
            }
        }
    }

    fn render_list(&self, list: &gst::BufferList) -> Result<gst::FlowSuccess, gst::FlowError> {
        gst::debug!(
            CAT,
            imp = self,
            "Rendering buffer list with {} buffers",
            list.len()
        );

        let state_locked = self.state.lock().unwrap();
        let State::Started(ref started) = *state_locked else {
            gst::element_imp_error!(self, gst::CoreError::Failed, ["Not started yet"]);
            return Err(gst::FlowError::Error);
        };

        // Track statistics for the batch
        let mut total_bytes = 0u64;
        let mut total_messages = 0u64;
        let mut errors_count = 0u64;

        // Get caps settings
        let (send_caps, caps_interval) = {
            let settings = self.settings.lock().unwrap();
            (settings.send_caps, settings.caps_interval)
        };

        let caps_attachment = if send_caps {
            if let Some(caps) = self.obj().sink_pad().current_caps() {
                let mut should_send = false;

                if !started.caps_sent.load(Ordering::Acquire) {
                    gst::debug!(
                        CAT,
                        imp = self,
                        "Sending caps on first buffer list: {}",
                        caps
                    );
                    should_send = true;
                    started.caps_sent.store(true, Ordering::Release);
                    *started.last_caps.lock().unwrap() = Some(caps.clone());
                    *started.last_caps_time.lock().unwrap() = Some(std::time::Instant::now());
                } else {
                    let last_caps = started.last_caps.lock().unwrap();
                    if last_caps.as_ref() != Some(&caps) {
                        gst::debug!(CAT, imp = self, "Caps changed in buffer list: {}", caps);
                        should_send = true;
                        drop(last_caps);
                        *started.last_caps.lock().unwrap() = Some(caps.clone());
                        *started.last_caps_time.lock().unwrap() = Some(std::time::Instant::now());
                    } else if caps_interval > 0 {
                        let last_time = started.last_caps_time.lock().unwrap();
                        if let Some(last) = *last_time
                            && last.elapsed().as_secs() >= caps_interval as u64
                        {
                            should_send = true;
                            drop(last_time);
                            *started.last_caps_time.lock().unwrap() =
                                Some(std::time::Instant::now());
                        }
                    }
                }

                if should_send {
                    MetadataBuilder::new().caps(&caps).build()
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Process each buffer in the list
        for buffer in list.iter() {
            // Get buffer data
            let b = buffer.map_readable().map_err(|_| {
                gst::element_imp_error!(
                    self,
                    gst::ResourceError::Read,
                    ["Failed to map buffer for reading in buffer list"]
                );
                errors_count += 1;
                gst::FlowError::Error
            })?;

            // Send buffer with caps attachment
            let put_builder = started.publisher.put(b.as_slice());
            let result = if let Some(ref attachment) = caps_attachment {
                put_builder.attachment(attachment.clone()).wait()
            } else {
                put_builder.wait()
            };

            match result {
                Ok(_) => {
                    total_bytes += b.len() as u64;
                    total_messages += 1;
                }
                Err(e) => {
                    errors_count += 1;

                    // Get key expression for error reporting
                    let key_expr = self.settings.lock().unwrap().key_expr.clone();
                    let error_msg = format!("{}", e);
                    let err = ZenohError::PublishError {
                        key_expr,
                        source: e,
                    };

                    if error_msg.contains("timeout")
                        || error_msg.contains("connection")
                        || error_msg.contains("network")
                    {
                        gst::warning!(CAT, imp = self, "Network error in buffer list: {}", err);
                    } else {
                        gst::warning!(CAT, imp = self, "Error publishing buffer in list: {}", err);
                    }

                    // Continue processing remaining buffers instead of failing immediately
                    // This provides better resilience for batch operations
                }
            }
        }

        // Update statistics in a single operation for better performance
        {
            let mut stats = started.stats.lock().unwrap();
            stats.bytes_sent += total_bytes;
            stats.messages_sent += total_messages;
            stats.errors += errors_count;
        }

        if errors_count > 0 {
            gst::warning!(
                CAT,
                imp = self,
                "Completed buffer list with {} errors out of {} buffers",
                errors_count,
                list.len()
            );
        }

        // Return success if at least one buffer was sent successfully
        if total_messages > 0 {
            Ok(gst::FlowSuccess::Ok)
        } else if errors_count > 0 {
            // All buffers failed
            gst::element_imp_error!(
                self,
                gst::ResourceError::Write,
                ["Failed to send all buffers in list"]
            );
            Err(gst::FlowError::Error)
        } else {
            // Empty list
            Ok(gst::FlowSuccess::Ok)
        }
    }

    fn stop(&self) -> Result<(), gst::ErrorMessage> {
        let mut state = self.state.lock().unwrap();

        // Check if we can stop from current state
        if !state.can_stop() {
            let current_state = match *state {
                State::Stopped => "Stopped",
                State::Starting => "Starting",
                State::Started(_) => "Started",
                State::Stopping => "Stopping",
            };
            gst::debug!(CAT, "ZenohSink stop called from state: {}", current_state);
            if state.is_stopped() {
                return Ok(()); // Already stopped is not an error
            }
            // For Starting state, we should wait or error - for now just warn and continue
            gst::warning!(
                CAT,
                "Stopping ZenohSink from non-started state: {}",
                current_state
            );
        }

        if let State::Started(ref _started) = *state {
            gst::debug!(CAT, "ZenohSink transitioning from Started to Stopping");
            // Set to Stopping state temporarily
            let _started_data = match std::mem::replace(&mut *state, State::Stopping) {
                State::Started(started) => started,
                _ => unreachable!(),
            };

            // Resources will be cleaned up when _started_data is dropped
            gst::debug!(CAT, "ZenohSink resources cleaned up");
        }

        *state = State::Stopped;
        gst::debug!(CAT, "ZenohSink successfully transitioned to Stopped state");

        Ok(())
    }

    fn event(&self, event: gst::Event) -> bool {
        use gst::EventView;

        match event.view() {
            EventView::Eos(_) => {
                gst::debug!(CAT, imp = self, "End of stream");
                // Could optionally flush any pending data here
                self.parent_event(event)
            }
            EventView::FlushStart(_) => {
                gst::debug!(CAT, imp = self, "Flush start");
                // Could abort any pending publish operations if needed
                self.parent_event(event)
            }
            EventView::FlushStop(_) => {
                gst::debug!(CAT, imp = self, "Flush stop - ready for new data");
                self.parent_event(event)
            }
            _ => {
                gst::log!(CAT, imp = self, "Handling event {:?}", event);
                self.parent_event(event)
            }
        }
    }
}

impl URIHandlerImpl for ZenohSink {
    const URI_TYPE: gst::URIType = gst::URIType::Sink;

    fn protocols() -> &'static [&'static str] {
        &["zenoh"]
    }

    fn uri(&self) -> Option<String> {
        let settings = self.settings.lock().unwrap();
        if settings.key_expr.is_empty() {
            return None;
        }

        // Build URI in format: zenoh:key-expr?param1=value1&param2=value2
        let mut uri = format!("zenoh:{}", settings.key_expr);
        let mut params = Vec::new();

        if let Some(ref config) = settings.config_file {
            params.push(format!("config={}", urlencoding::encode(config)));
        }
        if settings.priority != 5 {
            params.push(format!("priority={}", settings.priority));
        }
        if settings.congestion_control != "block" {
            params.push(format!(
                "congestion-control={}",
                settings.congestion_control
            ));
        }
        if settings.reliability != "best-effort" {
            params.push(format!("reliability={}", settings.reliability));
        }
        if settings.express {
            params.push("express=true".to_string());
        }

        if !params.is_empty() {
            uri.push('?');
            uri.push_str(&params.join("&"));
        }

        Some(uri)
    }

    fn set_uri(&self, uri: &str) -> Result<(), glib::Error> {
        // Parse URI format: zenoh:key-expr?param1=value1&param2=value2
        if !uri.starts_with("zenoh:") {
            return Err(glib::Error::new(
                gst::URIError::BadUri,
                &format!("Invalid URI scheme, expected 'zenoh:', got: {}", uri),
            ));
        }

        let uri_content = &uri[6..]; // Skip "zenoh:"

        // Split into key expression and query parameters
        let (key_expr, query) = if let Some(pos) = uri_content.find('?') {
            (&uri_content[..pos], Some(&uri_content[pos + 1..]))
        } else {
            (uri_content, None)
        };

        if key_expr.is_empty() {
            return Err(glib::Error::new(
                gst::URIError::BadUri,
                "Key expression cannot be empty",
            ));
        }

        // Decode the key expression
        let key_expr = urlencoding::decode(key_expr)
            .map_err(|e| {
                glib::Error::new(
                    gst::URIError::BadUri,
                    &format!("Failed to decode key expression: {}", e),
                )
            })?
            .into_owned();

        let mut settings = self.settings.lock().unwrap();

        // Check if we can modify settings (not started)
        let state = self.state.lock().unwrap();
        if state.is_started() {
            drop(state);
            drop(settings);
            return Err(glib::Error::new(
                gst::URIError::BadState,
                "Cannot change URI while element is started",
            ));
        }
        drop(state);

        settings.key_expr = key_expr;

        // Parse query parameters
        if let Some(query) = query {
            for param in query.split('&') {
                if let Some(pos) = param.find('=') {
                    let key = &param[..pos];
                    let value = urlencoding::decode(&param[pos + 1..])
                        .map_err(|e| {
                            glib::Error::new(
                                gst::URIError::BadUri,
                                &format!("Failed to decode parameter value: {}", e),
                            )
                        })?
                        .into_owned();

                    match key {
                        "config" => settings.config_file = Some(value),
                        "priority" => {
                            settings.priority = value.parse().map_err(|_| {
                                glib::Error::new(
                                    gst::URIError::BadUri,
                                    &format!("Invalid priority value: {}", value),
                                )
                            })?;
                        }
                        "congestion-control" => {
                            if value != "block" && value != "drop" {
                                return Err(glib::Error::new(
                                    gst::URIError::BadUri,
                                    &format!("Invalid congestion-control value: {}", value),
                                ));
                            }
                            settings.congestion_control = value;
                        }
                        "reliability" => {
                            if value != "best-effort" && value != "reliable" {
                                return Err(glib::Error::new(
                                    gst::URIError::BadUri,
                                    &format!("Invalid reliability value: {}", value),
                                ));
                            }
                            settings.reliability = value;
                        }
                        "express" => {
                            settings.express = value.parse().map_err(|_| {
                                glib::Error::new(
                                    gst::URIError::BadUri,
                                    &format!("Invalid express value: {}", value),
                                )
                            })?;
                        }
                        _ => {
                            gst::warning!(CAT, imp = self, "Unknown URI parameter: {}", key);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
