use std::sync::{Arc, LazyLock, Mutex};

use gst::subclass::prelude::URIHandlerImpl;
use gst::{glib, prelude::*, subclass::prelude::*};
use gst_base::subclass::prelude::*;
use zenoh::key_expr::OwnedKeyExpr;
use zenoh::qos::{CongestionControl, Priority, Reliability};
use zenoh::Wait;

use crate::error::{ErrorHandling, FlowErrorHandling, ZenohError};

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
    start_time: Option<gst::ClockTime>,
}

struct Started {
    // Keeping session field to maintain ownership and prevent session from being dropped
    // while publisher is still in use. This can be either owned or shared.
    #[allow(dead_code)]
    session: SessionWrapper,
    publisher: zenoh::pubsub::Publisher<'static>,
    /// Statistics tracking (shared for thread-safe updates)
    stats: Arc<Mutex<Statistics>>,
}

/// Wrapper to handle both owned and shared Zenoh sessions.
///
/// This allows the plugin to either create its own session or use
/// a shared session provided externally, enabling session reuse
/// across multiple GStreamer elements.
enum SessionWrapper {
    /// Element owns the session exclusively
    Owned(zenoh::Session),
    /// Element shares a session with other components
    Shared(Arc<zenoh::Session>),
}

impl SessionWrapper {
    /// Get a reference to the underlying Zenoh session
    fn as_session(&self) -> &zenoh::Session {
        match self {
            SessionWrapper::Owned(session) => session,
            SessionWrapper::Shared(session) => session.as_ref(),
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
    /// Optional external Zenoh session to share with other elements
    external_session: Option<Arc<zenoh::Session>>,
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
            external_session: None,
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
            name => {
                gst::warning!(CAT, "Unknown property: {}", name);
            }
        }
    }

    fn property(&self, _id: usize, pspec: &gst::glib::ParamSpec) -> gst::glib::Value {
        match pspec.name() {
            // Configuration properties - read from settings
            "key-expr" | "config" | "priority" | "congestion-control" | "reliability"
            | "express" => {
                let settings = self.settings.lock().unwrap();
                match pspec.name() {
                    "key-expr" => settings.key_expr.to_value(),
                    "config" => settings.config_file.to_value(),
                    "priority" => (settings.priority as u32).to_value(),
                    "congestion-control" => settings.congestion_control.to_value(),
                    "reliability" => settings.reliability.to_value(),
                    "express" => settings.express.to_value(),
                    _ => unreachable!(),
                }
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
        drop(settings);

        // Validate the key expression
        if key_expr.is_empty() {
            return Err(gst::error_msg!(
                gst::ResourceError::Settings,
                ["Key expression is required"]
            ));
        }

        // Set up Zenoh config with either default or from file
        let config = match config_file {
            Some(path) if !path.is_empty() => {
                gst::debug!(CAT, "Loading Zenoh config from {}", path);
                zenoh::Config::from_file(&path)
                    .map_err(|e| ZenohError::InitError(e).to_error_message())?
            }
            _ => zenoh::Config::default(),
        };

        // Use external session if provided, otherwise create a new one
        let session_wrapper = match external_session {
            Some(shared_session) => {
                gst::debug!(CAT, "Using external shared session");
                SessionWrapper::Shared(shared_session)
            }
            None => {
                gst::debug!(CAT, "Creating new Zenoh session");
                let session = zenoh::open(config)
                    .wait()
                    .map_err(|e| ZenohError::InitError(e).to_error_message())?;
                SessionWrapper::Owned(session)
            }
        };

        gst::debug!(CAT, "Creating publisher with key_expr='{}', priority={}, congestion_control='{}', reliability='{}', express={}",
                  key_expr, priority, congestion_control, reliability, express);

        // Use owned key_expr for static lifetime, with proper error handling
        let owned = OwnedKeyExpr::try_from(key_expr.clone())
            .map_err(|e| ZenohError::KeyExprError(e.to_string()).to_error_message())?;

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

        let publisher = publisher_builder
            .wait()
            .map_err(|e| ZenohError::PublishError(e).to_error_message())?;

        gst::debug!(CAT, "Publisher created with key_expr='{}', priority={}, congestion_control='{}', reliability='{}', express={}",
            key_expr, priority, congestion_control, reliability, express);

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

        // Send directly using synchronous API with proper error handling
        // Note: Zenoh's wait() already handles timeouts internally
        match started.publisher.put(b.as_slice()).wait() {
            Ok(_) => {
                // Update statistics on success
                let mut stats = started.stats.lock().unwrap();
                stats.bytes_sent += b.len() as u64;
                stats.messages_sent += 1;
                Ok(gst::FlowSuccess::Ok)
            }
            Err(e) => {
                // Update error statistics
                started.stats.lock().unwrap().errors += 1;

                // Check if this is a network-related error before consuming e
                let error_msg = format!("{}", e);
                let err = ZenohError::PublishError(e);
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
