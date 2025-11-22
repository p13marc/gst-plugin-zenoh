use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use gst::subclass::prelude::URIHandlerImpl;
use gst::{glib, prelude::*, subclass::prelude::*};
use gst_base::{
    prelude::BaseSrcExt,
    subclass::{base_src::CreateSuccess, prelude::*},
};
use zenoh::Wait;

use crate::error::{ErrorHandling, FlowErrorHandling, ZenohError};
use crate::metadata::MetadataParser;

// Define debug category for logging
#[allow(dead_code)]
static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new("zenohsrc", gst::DebugColorFlags::empty(), Some("Zenoh Src"))
});

/// Statistics tracking for ZenohSrc
#[derive(Debug, Clone, Default)]
struct Statistics {
    bytes_received: u64,
    messages_received: u64,
    errors: u64,
    start_time: Option<gst::ClockTime>,
}

struct Started {
    // Keeping session field to maintain ownership and prevent session from being dropped
    // while subscriber is still in use. This can be either owned or shared.
    #[allow(dead_code)]
    session: SessionWrapper,
    subscriber:
        zenoh::pubsub::Subscriber<zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>>,
    /// Flag to signal that the element is flushing and should cancel blocking operations
    flushing: Arc<AtomicBool>,
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

/// Configuration settings for the ZenohSrc element.
///
/// These settings control how the element connects to and subscribes
/// to data from the Zenoh network protocol.
#[derive(Debug)]
struct Settings {
    /// Zenoh key expression for subscribing to data (required)
    key_expr: String,
    /// Optional path to Zenoh configuration file
    config_file: Option<String>,
    /// Subscriber priority level (1-7: 1=RealTime, 2=InteractiveHigh, 3=InteractiveLow, 4=DataHigh, 5=Data(default), 6=DataLow, 7=Background)
    priority: u8,
    /// Congestion control policy: "block" or "drop" (informational for subscriber)
    congestion_control: String,
    /// Reliability mode: "best-effort" or "reliable" (matches publisher settings)
    reliability: String,
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
            external_session: None,
        }
    }
}

/// GStreamer ZenohSrc element implementation.
///
/// This element subscribes to data from a Zenoh network using the
/// configured key expression and delivers it as GStreamer buffers
/// to downstream elements.
///
/// The element supports:
/// - Configurable subscription parameters
/// - Session sharing capabilities
/// - Automatic reliability adaptation (matches publisher)
/// - Priority-based message handling
/// - Proper flush handling and unlock support for responsive state changes
#[derive(Default)]
pub struct ZenohSrc {
    /// Element configuration settings
    settings: Mutex<Settings>,
    /// Current operational state
    state: Mutex<State>,
}

impl GstObjectImpl for ZenohSrc {}

impl ElementImpl for ZenohSrc {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static ELEMENT_METADATA: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
            gst::subclass::ElementMetadata::new(
                "Zenoh Network Source",
                "Source/Network/Protocol",
                "Subscribes to Zenoh networks and delivers data as GStreamer buffers with wildcard key expression support",
                "Marc Pardo <p13marc@gmail.com>",
            )
        });
        Some(&*ELEMENT_METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static PAD_TEMPLATES: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
            let src_pad_template = gst::PadTemplate::new(
                "src",
                gst::PadDirection::Src,
                gst::PadPresence::Always,
                &gst::Caps::new_any(),
            )
            .unwrap();

            vec![src_pad_template]
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

impl ObjectImpl for ZenohSrc {
    fn properties() -> &'static [glib::ParamSpec] {
        static PROPERTIES: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| {
            vec![
                // Key expression property
                glib::ParamSpecString::builder("key-expr")
                    .nick("Zenoh Key Expression")
                    .blurb("Zenoh key expression for data subscription. Supports wildcards: '*' (single level) and '**' (multi-level). Example: 'demo/video/*', 'sensors/**'")
                    .build(),

                // Config file property
                glib::ParamSpecString::builder("config")
                    .nick("Zenoh Configuration")
                    .blurb("Path to Zenoh configuration file for custom network settings (JSON5 format)")
                    .build(),

                // Priority property
                glib::ParamSpecUInt::builder("priority")
                    .nick("Subscriber Priority")
                    .blurb("Message priority level: 1=RealTime(highest), 2=InteractiveHigh, 3=InteractiveLow, 4=DataHigh, 5=Data(default), 6=DataLow, 7=Background(lowest)")
                    .default_value(5)
                    .minimum(1)
                    .maximum(7)
                    .build(),

                // Congestion control property
                glib::ParamSpecString::builder("congestion-control")
                    .nick("Congestion Control")
                    .blurb("Congestion control preference (informational): 'block' or 'drop'. Actual behavior depends on publisher settings.")
                    .default_value(Some("block"))
                    .build(),

                // Reliability property
                glib::ParamSpecString::builder("reliability")
                    .nick("Reliability Mode")
                    .blurb("Expected reliability mode (informational): 'best-effort' or 'reliable'. Actual reliability is determined by publisher.")
                    .default_value(Some("best-effort"))
                    .build(),
                // Statistics properties (read-only)
                glib::ParamSpecUInt64::builder("bytes-received")
                    .nick("Bytes Received")
                    .blurb("Total bytes received since element started")
                    .read_only()
                    .build(),
                glib::ParamSpecUInt64::builder("messages-received")
                    .nick("Messages Received")
                    .blurb("Total messages received since element started")
                    .read_only()
                    .build(),
                glib::ParamSpecUInt64::builder("errors")
                    .nick("Errors")
                    .blurb("Total number of errors encountered")
                    .read_only()
                    .build(),
            ]
        });

        PROPERTIES.as_ref()
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        // Check if we're in a state where property changes are allowed
        let state = self.state.lock().unwrap();
        if state.is_started()
            && matches!(
                pspec.name(),
                "key-expr" | "config" | "reliability" | "congestion-control" | "priority"
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
            name => {
                gst::warning!(CAT, "Unknown property: {}", name);
            }
        }
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        match pspec.name() {
            // Configuration properties - read from settings
            "key-expr" | "config" | "priority" | "congestion-control" | "reliability" => {
                let settings = self.settings.lock().unwrap();
                match pspec.name() {
                    "key-expr" => settings.key_expr.to_value(),
                    "config" => settings.config_file.to_value(),
                    "priority" => (settings.priority as u32).to_value(),
                    "congestion-control" => settings.congestion_control.to_value(),
                    "reliability" => settings.reliability.to_value(),
                    _ => unreachable!(),
                }
            }
            // Statistics properties - read from state
            "bytes-received" => {
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    started.stats.lock().unwrap().bytes_received.to_value()
                } else {
                    0u64.to_value()
                }
            }
            "messages-received" => {
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    started.stats.lock().unwrap().messages_received.to_value()
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
            name => {
                gst::warning!(CAT, "Unknown property: {}", name);
                // Return an empty string value as default
                "".to_value()
            }
        }
    }

    fn constructed(&self) {
        self.parent_constructed();
        self.obj().set_format(gst::Format::Time);
        self.obj().set_do_timestamp(true);
        self.obj().set_live(true);
    }
}

#[glib::object_subclass]
impl ObjectSubclass for ZenohSrc {
    const NAME: &'static str = "GstZenohSrc";
    type Type = super::ZenohSrc;
    type ParentType = gst_base::PushSrc;
    type Interfaces = (gst::URIHandler,);
}

impl BaseSrcImpl for ZenohSrc {
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
                "Cannot start ZenohSrc from state: {}, ignoring start request",
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

        gst::debug!(CAT, "ZenohSrc transitioning from Stopped to Starting");
        *state = State::Starting;
        drop(state); // Release state lock before potentially long operations

        // Get settings
        let settings = self.settings.lock().unwrap();
        let key_expr = settings.key_expr.clone();
        let config_file = settings.config_file.clone();
        let priority = settings.priority;
        let congestion_control = settings.congestion_control.clone();
        let reliability = settings.reliability.clone();
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

        gst::debug!(CAT, "Creating subscriber with key_expr='{}', priority={}, congestion_control='{}', reliability='{}'",
                  key_expr, priority, congestion_control, reliability);

        // Note: Zenoh subscriber reliability is automatically determined by the publisher
        //
        // Unlike publishers, subscribers don't explicitly configure reliability modes.
        // Instead, they automatically adapt to match the reliability mode of the
        // publisher they're receiving from. This ensures consistent delivery guarantees
        // across the pub-sub connection without requiring manual coordination.

        // Create subscriber
        let subscriber = session_wrapper
            .as_session()
            .declare_subscriber(key_expr)
            .wait()
            .map_err(|e| ZenohError::InitError(e).to_error_message())?;

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
            subscriber,
            flushing: Arc::new(AtomicBool::new(false)),
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

        gst::debug!(CAT, "ZenohSrc successfully transitioned to Started state");

        Ok(())
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
            gst::debug!(CAT, "ZenohSrc stop called from state: {}", current_state);
            if state.is_stopped() {
                return Ok(()); // Already stopped is not an error
            }
            // For Starting state, we should wait or error - for now just warn and continue
            gst::warning!(
                CAT,
                "Stopping ZenohSrc from non-started state: {}",
                current_state
            );
        }

        if let State::Started(ref _started) = *state {
            gst::debug!(CAT, "ZenohSrc transitioning from Started to Stopping");
            // Set to Stopping state temporarily
            let _started_data = match std::mem::replace(&mut *state, State::Stopping) {
                State::Started(started) => started,
                _ => unreachable!(),
            };

            // Resources will be cleaned up when _started_data is dropped
            gst::debug!(CAT, "ZenohSrc resources cleaned up");
        }

        *state = State::Stopped;
        gst::debug!(CAT, "ZenohSrc successfully transitioned to Stopped state");

        Ok(())
    }

    fn unlock(&self) -> Result<(), gst::ErrorMessage> {
        gst::debug!(
            CAT,
            imp = self,
            "Unlock called - cancelling blocking operations"
        );
        let state = self.state.lock().unwrap();
        if let State::Started(ref started) = *state {
            started.flushing.store(true, Ordering::SeqCst);
        }
        Ok(())
    }

    fn unlock_stop(&self) -> Result<(), gst::ErrorMessage> {
        gst::debug!(
            CAT,
            imp = self,
            "Unlock stop called - resuming normal operation"
        );
        let state = self.state.lock().unwrap();
        if let State::Started(ref started) = *state {
            started.flushing.store(false, Ordering::SeqCst);
        }
        Ok(())
    }

    fn event(&self, event: &gst::Event) -> bool {
        use gst::EventView;

        match event.view() {
            EventView::FlushStart(_) => {
                gst::debug!(CAT, imp = self, "Flush start - cancelling operations");
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    started.flushing.store(true, Ordering::SeqCst);
                }
                self.parent_event(event)
            }
            EventView::FlushStop(_) => {
                gst::debug!(CAT, imp = self, "Flush stop - resuming operations");
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    started.flushing.store(false, Ordering::SeqCst);
                }
                self.parent_event(event)
            }
            _ => self.parent_event(event),
        }
    }

    fn query(&self, query: &mut gst::QueryRef) -> bool {
        use gst::QueryViewMut;

        match query.view_mut() {
            QueryViewMut::Latency(ref mut q) => {
                // Report as a live source with minimal latency
                // - live: true (we're a network source)
                // - min_latency: ZERO (Zenoh has very low latency)
                // - max_latency: NONE (unbounded, depends on network conditions)
                gst::debug!(CAT, imp = self, "Responding to latency query");
                q.set(true, gst::ClockTime::ZERO, gst::ClockTime::NONE);
                true
            }
            QueryViewMut::Scheduling(ref mut q) => {
                // Report that we support push mode scheduling
                // - SEQUENTIAL flag: we deliver buffers sequentially
                // - minsize: 1 (we can deliver any size)
                // - maxsize: -1 (unlimited)
                // - align: 0 (no alignment requirements)
                gst::debug!(CAT, imp = self, "Responding to scheduling query");
                q.set(gst::SchedulingFlags::SEQUENTIAL, 1, -1, 0);
                q.add_scheduling_modes([gst::PadMode::Push]);
                true
            }
            _ => BaseSrcImplExt::parent_query(self, query),
        }
    }
}

impl PushSrcImpl for ZenohSrc {
    fn create(
        &self,
        _buffer: Option<&mut gst::BufferRef>,
    ) -> Result<CreateSuccess, gst::FlowError> {
        let state_locked = self.state.lock().unwrap();
        let State::Started(ref started) = *state_locked else {
            gst::element_imp_error!(self, gst::CoreError::Failed, ["Not started yet"]);
            return Err(gst::FlowError::Error);
        };

        // Check if we're flushing before attempting to receive
        if started.flushing.load(Ordering::SeqCst) {
            gst::debug!(CAT, imp = self, "Flushing - returning Flushing flow");
            return Err(gst::FlowError::Flushing);
        }

        // CRITICAL: Use recv_timeout() instead of blocking recv()
        // This allows us to check the flushing flag periodically without sleeping
        let sample: zenoh::sample::Sample = loop {
            if started.flushing.load(Ordering::SeqCst) {
                gst::debug!(CAT, imp = self, "Flushing detected during receive");
                return Err(gst::FlowError::Flushing);
            }

            // Use recv_timeout with short timeout to remain responsive to flushing
            // recv_timeout returns Result<Option<Sample>, RecvTimeoutError>
            match started.subscriber.recv_timeout(Duration::from_millis(100)) {
                Ok(Some(sample)) => break sample,
                Ok(None) => {
                    // No sample available, continue loop
                    continue;
                }
                Err(e) => {
                    // Check if it's a timeout or disconnection
                    let err_msg = format!("{:?}", e);
                    if err_msg.contains("Timeout") {
                        // Timeout - check flushing flag and retry
                        continue;
                    } else {
                        // Disconnected or other error
                        started.stats.lock().unwrap().errors += 1;
                        gst::element_imp_error!(
                            self,
                            gst::ResourceError::Read,
                            ["Subscriber error: {}", e]
                        );
                        return Err(gst::FlowError::Error);
                    }
                }
            }
        };

        // Check if the sample has attachment metadata (caps, custom metadata, etc.)
        if let Some(attachment) = sample.attachment() {
            match MetadataParser::parse(attachment) {
                Ok(metadata) => {
                    // If caps are present in metadata, set them on the source pad
                    if let Some(caps) = metadata.caps() {
                        gst::debug!(CAT, imp = self, "Received caps from metadata: {}", caps);

                        // Set caps on the source pad
                        if let Err(e) = self.obj().set_caps(caps) {
                            gst::warning!(CAT, imp = self, "Failed to set caps: {}", e);
                        }
                    }

                    // Log any user metadata
                    if !metadata.user_metadata().is_empty() {
                        gst::trace!(
                            CAT,
                            imp = self,
                            "Received user metadata: {:?}",
                            metadata.user_metadata()
                        );
                    }
                }
                Err(e) => {
                    gst::warning!(CAT, imp = self, "Failed to parse metadata: {}", e);
                }
            }
        }

        let payload = sample.payload();
        let slice = payload.to_bytes();

        let mut buffer = gst::Buffer::with_size(slice.len()).map_err(|_| {
            gst::element_imp_error!(
                self,
                gst::ResourceError::Failed,
                ["Failed to allocate buffer"]
            );
            gst::FlowError::Error
        })?;

        {
            let buffer_mut = buffer.get_mut().ok_or_else(|| {
                gst::element_imp_error!(
                    self,
                    gst::ResourceError::Failed,
                    ["Failed to get mutable buffer reference"]
                );
                gst::FlowError::Error
            })?;

            buffer_mut.copy_from_slice(0, &slice).map_err(|_| {
                gst::element_imp_error!(
                    self,
                    gst::ResourceError::Failed,
                    ["Failed to copy data to buffer"]
                );
                gst::FlowError::Error
            })?;

            // Extract Zenoh timestamp if available and apply it to the buffer
            // This helps with synchronization and proper buffer timestamping
            if let Some(timestamp) = sample.timestamp() {
                // Zenoh timestamps are in NTP64 format (64-bit timestamp)
                // Convert to GStreamer ClockTime (nanoseconds since epoch)
                let ntp_time = timestamp.get_time();

                // NTP64 timestamp is split into:
                // - upper 32 bits: seconds since NTP epoch (Jan 1, 1900)
                // - lower 32 bits: fractional seconds
                // We need to convert this to nanoseconds since Unix epoch (Jan 1, 1970)

                // NTP epoch is 2208988800 seconds before Unix epoch
                const NTP_UNIX_OFFSET: u64 = 2208988800;

                let ntp_secs = ntp_time.as_secs() as u64;
                let ntp_nanos = ntp_time.subsec_nanos() as u64;

                // Convert to Unix epoch
                if ntp_secs >= NTP_UNIX_OFFSET {
                    let unix_secs = ntp_secs - NTP_UNIX_OFFSET;
                    let total_nanos = unix_secs * 1_000_000_000 + ntp_nanos;

                    let pts = gst::ClockTime::from_nseconds(total_nanos);
                    buffer_mut.set_pts(pts);

                    gst::trace!(
                        CAT,
                        imp = self,
                        "Applied Zenoh timestamp to buffer: PTS = {}",
                        pts
                    );
                }
            }
        }

        // Update statistics on success
        let mut stats = started.stats.lock().unwrap();
        stats.bytes_received += slice.len() as u64;
        stats.messages_received += 1;
        drop(stats);

        Ok(CreateSuccess::NewBuffer(buffer))
    }
}

impl URIHandlerImpl for ZenohSrc {
    const URI_TYPE: gst::URIType = gst::URIType::Src;

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
