use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;

use zenoh::handlers::{Callback, FifoChannelHandler, IntoHandler};
use zenoh::pubsub::Subscriber;
use zenoh::sample::Sample;

use gst::subclass::prelude::URIHandlerImpl;
use gst::{glib, prelude::*, subclass::prelude::*};
use gst_base::{
    prelude::BaseSrcExt,
    subclass::{base_src::CreateSuccess, prelude::*},
};
use zenoh::Wait;

use crate::error::{ErrorHandling, ZenohError};
use crate::metadata::MetadataParser;

// Define debug category for logging
static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new("zenohsrc", gst::DebugColorFlags::empty(), Some("Zenoh Src"))
});

/// Default timeout bounding `zenoh::open(...).wait()` during start so an unreachable
/// router can't stall the state-change thread forever.
const SESSION_OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// Statistics tracking for ZenohSrc
#[derive(Debug, Clone, Default)]
struct Statistics {
    bytes_received: u64,
    messages_received: u64,
    errors: u64,
}

/// Receive-handler strategy for the Zenoh subscriber.
///
/// `Fifo` (default) is the lossless, unbounded handler — every sample is queued
/// in order. `Ring` keeps only the most recent `queue-depth` samples, dropping
/// the oldest when full; this bounds latency build-up for live sources (e.g. RTP
/// video) where staleness is worse than loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum HandlerMode {
    #[default]
    Fifo,
    Ring,
}

impl HandlerMode {
    /// Parse the property nick (`"fifo"` / `"ring"`); `None` if unrecognized.
    fn from_nick(s: &str) -> Option<Self> {
        match s {
            "fifo" => Some(Self::Fifo),
            "ring" => Some(Self::Ring),
            _ => None,
        }
    }

    fn as_nick(self) -> &'static str {
        match self {
            Self::Fifo => "fifo",
            Self::Ring => "ring",
        }
    }
}

/// Generic bounded "keep latest N" buffer that drops — and counts — the oldest
/// element when full.
///
/// Extracted from the handler so the drop-oldest accounting is unit-tested
/// directly: the handler is `Sample`-typed and `Sample` has no public
/// constructor, so the logic is exercised here with a testable element type.
/// Mirrors Zenoh's own `RingChannel`, but the built-in ring drops silently
/// whereas this one records every eviction for the `dropped` stat.
struct CountingRing<T> {
    ring: Mutex<VecDeque<T>>,
    capacity: usize,
    dropped: Arc<AtomicU64>,
}

impl<T> CountingRing<T> {
    fn new(capacity: usize, dropped: Arc<AtomicU64>) -> Self {
        Self {
            ring: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
            dropped,
        }
    }

    /// Push the newest element, evicting (and counting) the oldest when full.
    fn push(&self, item: T) {
        let mut ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
        if ring.len() >= self.capacity {
            ring.pop_front();
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        ring.push_back(item);
    }

    /// Pop the oldest retained element, if any.
    fn pop(&self) -> Option<T> {
        self.ring
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
    }
}

/// Shared inner state of the drop-oldest ring channel: the counting ring plus a
/// `flume(1)` "not empty" signal so the receiver can block with a timeout and
/// observe disconnection (all senders dropped).
struct CountingRingInner {
    ring: CountingRing<Sample>,
    not_empty: flume::Receiver<()>,
}

/// Keep-latest-N, drop-oldest subscriber handler factory with eviction counting.
struct CountingRingChannel {
    capacity: usize,
    dropped: Arc<AtomicU64>,
}

/// Receiver half of [`CountingRingChannel`], stored inside the `Subscriber`.
struct CountingRingHandler {
    inner: Weak<CountingRingInner>,
}

impl CountingRingHandler {
    /// Receive the next sample, waiting up to `timeout`.
    ///
    /// `Ok(Some(_))` when a sample is available, `Ok(None)` on timeout, `Err(_)`
    /// once disconnected (subscriber/callback dropped). The `ZResult<Option<_>>`
    /// shape matches Zenoh's FIFO/ring handlers so the same receive loop drives
    /// either handler.
    fn recv_timeout(&self, timeout: Duration) -> zenoh::Result<Option<Sample>> {
        let Some(inner) = self.inner.upgrade() else {
            return Err("ring channel has been closed".into());
        };
        loop {
            if let Some(sample) = inner.ring.pop() {
                return Ok(Some(sample));
            }
            match inner.not_empty.recv_timeout(timeout) {
                Ok(()) => {}
                Err(flume::RecvTimeoutError::Timeout) => return Ok(None),
                Err(flume::RecvTimeoutError::Disconnected) => {
                    // Drain once more: a sample may have landed between the pull
                    // above and the senders being dropped.
                    if let Some(sample) = inner.ring.pop() {
                        return Ok(Some(sample));
                    }
                    return Err("ring channel disconnected".into());
                }
            }
        }
    }
}

impl IntoHandler<Sample> for CountingRingChannel {
    type Handler = CountingRingHandler;

    fn into_handler(self) -> (Callback<Sample>, Self::Handler) {
        let (sender, receiver) = flume::bounded::<()>(1);
        let inner = Arc::new(CountingRingInner {
            ring: CountingRing::new(self.capacity, self.dropped),
            not_empty: receiver,
        });
        let handler = CountingRingHandler {
            inner: Arc::downgrade(&inner),
        };
        // The callback owns the only strong reference to `inner`, so dropping the
        // subscriber drops the callback, releases `inner` and the `flume` sender,
        // and makes the handler's `recv_timeout` observe disconnection.
        let callback = Callback::from(move |sample: Sample| {
            inner.ring.push(sample);
            // Wake a blocked receiver; capacity-1, so a full channel already
            // carries a pending wake-up.
            let _ = sender.try_send(());
        });
        (callback, handler)
    }
}

/// A subscriber with either the default FIFO handler or the drop-oldest ring
/// handler, unified behind a single `recv_timeout` so `create()` is handler-agnostic.
enum SubscriberHandle {
    Fifo(Subscriber<FifoChannelHandler<Sample>>),
    Ring(Subscriber<CountingRingHandler>),
}

impl SubscriberHandle {
    fn recv_timeout(&self, timeout: Duration) -> zenoh::Result<Option<Sample>> {
        match self {
            SubscriberHandle::Fifo(s) => s.recv_timeout(timeout),
            SubscriberHandle::Ring(s) => s.recv_timeout(timeout),
        }
    }
}

struct Started {
    // Keeping session field to maintain ownership and prevent session from being dropped
    // while subscriber is still in use. This can be either owned or shared.
    _session: SessionWrapper,
    /// Subscriber wrapped in `Arc` so `create()` can clone it out and release the
    /// `state` lock before the blocking `recv_timeout` poll loop.
    subscriber: Arc<SubscriberHandle>,
    /// Statistics tracking (shared for thread-safe updates)
    stats: Arc<Mutex<Statistics>>,
    /// Samples dropped by the ring handler (stays 0 for FIFO). Updated from the
    /// Zenoh callback thread, read by the `dropped` property — hence an atomic.
    dropped: Arc<AtomicU64>,
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
    /// Element is using an externally-provided shared session (Rust API); the
    /// caller owns its lifetime, so this element does not release it.
    Shared(zenoh::Session),
    /// Element is using a session from the named group registry; dropping this
    /// releases one reference (closing the session on the last release).
    SharedGroup {
        session: zenoh::Session,
        group: String,
    },
}

impl SessionWrapper {
    /// Get a reference to the underlying Zenoh session
    fn as_session(&self) -> &zenoh::Session {
        match self {
            SessionWrapper::Owned(session) => session,
            SessionWrapper::Shared(session) => session,
            SessionWrapper::SharedGroup { session, .. } => session,
        }
    }
}

impl Drop for SessionWrapper {
    fn drop(&mut self) {
        if let SessionWrapper::SharedGroup { group, .. } = self {
            crate::session::release_session(group);
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
    /// Receive timeout in milliseconds for polling Zenoh subscriber
    /// Affects CPU usage vs responsiveness tradeoff (lower = more responsive but higher CPU)
    receive_timeout_ms: u64,
    /// Apply buffer timing metadata (PTS, DTS, duration, flags) from received messages (default: true)
    apply_buffer_meta: bool,
    /// Subscriber receive-handler strategy (FIFO vs. drop-oldest ring).
    handler: HandlerMode,
    /// Ring-handler capacity: max samples retained before the oldest is dropped.
    /// Ignored for the (unbounded) FIFO handler.
    queue_depth: u32,
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
            receive_timeout_ms: 100, // 100ms default for good responsiveness
            apply_buffer_meta: true, // Default to applying buffer timing metadata
            handler: HandlerMode::Fifo, // Lossless default preserves prior behavior
            queue_depth: 30,         // ~1s at 30fps; only used by the ring handler
            external_session: None,
            session_group: None,
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
    /// Flag to signal that the element is flushing and should cancel blocking
    /// operations. Stored on `self` (not behind `state`) so `unlock()`, `unlock_stop()`
    /// and the flush `event()` handlers can set it lock-free without queueing behind
    /// a `create()` that holds the `state` lock across its poll loop.
    flushing: Arc<AtomicBool>,
}

impl ZenohSrc {
    /// Sets the external Zenoh session to use for this element.
    ///
    /// This is called from the public API to enable session sharing.
    pub(crate) fn set_external_session(&self, session: zenoh::Session) {
        let mut settings = self.settings.lock().unwrap();
        settings.external_session = Some(session);
    }
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

                // Receive timeout property
                glib::ParamSpecUInt64::builder("receive-timeout-ms")
                    .nick("Receive Timeout")
                    .blurb("Timeout in milliseconds for polling Zenoh subscriber. Lower values increase responsiveness but use more CPU. Higher values reduce CPU but slow down state changes.")
                    .default_value(100)
                    .minimum(10)
                    .maximum(5000)
                    .build(),

                // Buffer metadata property
                glib::ParamSpecBoolean::builder("apply-buffer-meta")
                    .nick("Apply Buffer Metadata")
                    .blurb("Apply buffer timing metadata (PTS, DTS, duration, offset, flags) from received messages for proper A/V sync")
                    .default_value(true)
                    .build(),

                // Receive-handler strategy
                glib::ParamSpecString::builder("handler")
                    .nick("Receive Handler")
                    .blurb("Subscriber receive handler: 'fifo' (lossless, unbounded, default) or 'ring' (keep only the most recent 'queue-depth' samples, dropping the oldest). Use 'ring' for live sources to bound latency build-up.")
                    .default_value(Some("fifo"))
                    .build(),

                // Ring-handler depth
                glib::ParamSpecUInt::builder("queue-depth")
                    .nick("Queue Depth")
                    .blurb("Maximum samples retained by the 'ring' handler before the oldest is dropped. Ignored for the 'fifo' handler.")
                    .default_value(30)
                    .minimum(1)
                    .maximum(100_000)
                    .build(),

                // Session sharing property
                glib::ParamSpecString::builder("session-group")
                    .nick("Session Group")
                    .blurb("Name of the session group for sharing Zenoh sessions across elements. Elements with the same group name share a single session.")
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
                glib::ParamSpecUInt64::builder("dropped")
                    .nick("Dropped")
                    .blurb("Total samples dropped by the 'ring' handler since the element started (always 0 for the 'fifo' handler)")
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
                "key-expr" | "config" | "session-group" | "handler" | "queue-depth"
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
            "receive-timeout-ms" => {
                let timeout = value.get::<u64>().expect("type checked upstream");
                // Clamp to valid range (10-5000ms)
                settings.receive_timeout_ms = timeout.clamp(10, 5000);
                if timeout != settings.receive_timeout_ms {
                    gst::warning!(
                        CAT,
                        "Receive timeout clamped to {}ms (was {}ms)",
                        settings.receive_timeout_ms,
                        timeout
                    );
                }
            }
            "apply-buffer-meta" => {
                settings.apply_buffer_meta = value.get::<bool>().expect("type checked upstream");
            }
            "handler" => {
                let nick = value.get::<String>().expect("type checked upstream");
                match HandlerMode::from_nick(&nick) {
                    Some(mode) => settings.handler = mode,
                    None => gst::warning!(
                        CAT,
                        "Invalid handler '{}' (expected 'fifo' or 'ring'); keeping '{}'",
                        nick,
                        settings.handler.as_nick()
                    ),
                }
            }
            "queue-depth" => {
                settings.queue_depth = value.get::<u32>().expect("type checked upstream");
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

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        match pspec.name() {
            // Configuration properties - read from settings
            "key-expr" | "config" | "receive-timeout-ms" | "apply-buffer-meta" | "handler"
            | "queue-depth" | "session-group" => {
                let settings = self.settings.lock().unwrap();
                match pspec.name() {
                    "key-expr" => settings.key_expr.to_value(),
                    "config" => settings.config_file.to_value(),
                    "receive-timeout-ms" => settings.receive_timeout_ms.to_value(),
                    "apply-buffer-meta" => settings.apply_buffer_meta.to_value(),
                    "handler" => settings.handler.as_nick().to_value(),
                    "queue-depth" => settings.queue_depth.to_value(),
                    "session-group" => settings.session_group.to_value(),
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
            "dropped" => {
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    started.dropped.load(Ordering::Relaxed).to_value()
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
        let external_session = settings.external_session.clone();
        let session_group = settings.session_group.clone();
        let handler = settings.handler;
        let queue_depth = settings.queue_depth.max(1) as usize;
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
                .map_err(|e| ZenohError::Init(e).to_error_message())?;
            SessionWrapper::SharedGroup {
                session,
                group: group.clone(),
            }
        } else {
            // Priority 3: Create a new owned session
            gst::debug!(CAT, "Creating new Zenoh session");
            let config = match config_file {
                Some(path) if !path.is_empty() => {
                    gst::debug!(CAT, "Loading Zenoh config from {}", path);
                    zenoh::Config::from_file(&path)
                        .map_err(|e| ZenohError::Init(e).to_error_message())?
                }
                _ => zenoh::Config::default(),
            };
            // Bound the open so an unreachable router can't stall the state change.
            let session = crate::utils::call_with_timeout(SESSION_OPEN_TIMEOUT, move || {
                zenoh::open(config).wait()
            })
            .map_err(|_| {
                gst::error_msg!(
                    gst::ResourceError::OpenRead,
                    [
                        "Timed out opening Zenoh session after {:?}",
                        SESSION_OPEN_TIMEOUT
                    ]
                )
            })?
            .map_err(|e| ZenohError::Init(e).to_error_message())?;
            SessionWrapper::Owned(session)
        };

        gst::debug!(CAT, "Creating subscriber with key_expr='{}'", key_expr);

        // Note: Zenoh subscriber reliability is automatically determined by the publisher
        //
        // Unlike publishers, subscribers don't explicitly configure reliability modes.
        // Instead, they automatically adapt to match the reliability mode of the
        // publisher they're receiving from. This ensures consistent delivery guarantees
        // across the pub-sub connection without requiring manual coordination.

        // Create the subscriber with the configured receive handler. FIFO is the
        // lossless default; ring keeps only the most recent `queue_depth` samples
        // (drop-oldest) to bound latency for live sources, counting evictions.
        let dropped = Arc::new(AtomicU64::new(0));
        let subscriber = match handler {
            HandlerMode::Fifo => {
                gst::debug!(CAT, "Using FIFO receive handler (unbounded)");
                let sub = session_wrapper
                    .as_session()
                    .declare_subscriber(key_expr)
                    .wait()
                    .map_err(|e| ZenohError::Init(e).to_error_message())?;
                SubscriberHandle::Fifo(sub)
            }
            HandlerMode::Ring => {
                gst::debug!(
                    CAT,
                    "Using ring receive handler (drop-oldest, depth={})",
                    queue_depth
                );
                let sub = session_wrapper
                    .as_session()
                    .declare_subscriber(key_expr)
                    .with(CountingRingChannel {
                        capacity: queue_depth,
                        dropped: dropped.clone(),
                    })
                    .wait()
                    .map_err(|e| ZenohError::Init(e).to_error_message())?;
                SubscriberHandle::Ring(sub)
            }
        };
        let subscriber = Arc::new(subscriber);

        // Clear any leftover flush signal from a previous run.
        self.flushing.store(false, Ordering::SeqCst);

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
            _session: session_wrapper,
            subscriber,
            stats: Arc::new(Mutex::new(Statistics::default())),
            dropped,
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
        // Lock-free: never touch the `state` lock that create() holds across its poll loop.
        self.flushing.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn unlock_stop(&self) -> Result<(), gst::ErrorMessage> {
        gst::debug!(
            CAT,
            imp = self,
            "Unlock stop called - resuming normal operation"
        );
        self.flushing.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn event(&self, event: &gst::Event) -> bool {
        use gst::EventView;

        match event.view() {
            EventView::FlushStart(_) => {
                gst::debug!(CAT, imp = self, "Flush start - cancelling operations");
                self.flushing.store(true, Ordering::SeqCst);
                self.parent_event(event)
            }
            EventView::FlushStop(_) => {
                gst::debug!(CAT, imp = self, "Flush stop - resuming operations");
                self.flushing.store(false, Ordering::SeqCst);
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
        // Clone out the subscriber + stats under a short-lived lock, then release the
        // `state` lock before the blocking recv loop so flush/unlock (which set
        // `self.flushing`) and state changes are never queued behind create().
        let (subscriber, stats) = {
            let state_locked = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let State::Started(ref started) = *state_locked else {
                gst::element_imp_error!(self, gst::CoreError::Failed, ["Not started yet"]);
                return Err(gst::FlowError::Error);
            };
            (started.subscriber.clone(), started.stats.clone())
        };

        // Check if we're flushing before attempting to receive
        if self.flushing.load(Ordering::SeqCst) {
            gst::debug!(CAT, imp = self, "Flushing - returning Flushing flow");
            return Err(gst::FlowError::Flushing);
        }

        // Get the configured settings
        let (receive_timeout_ms, apply_buffer_meta) = {
            let settings = self.settings.lock().unwrap_or_else(|e| e.into_inner());
            (settings.receive_timeout_ms, settings.apply_buffer_meta)
        };

        // CRITICAL: Use recv_timeout() instead of blocking recv()
        // This allows us to check the flushing flag periodically without sleeping
        let sample: zenoh::sample::Sample = loop {
            if self.flushing.load(Ordering::SeqCst) {
                gst::debug!(CAT, imp = self, "Flushing detected during receive");
                return Err(gst::FlowError::Flushing);
            }

            // Use recv_timeout with configurable timeout to remain responsive to flushing
            // recv_timeout returns Result<Option<Sample>, RecvTimeoutError>
            match subscriber.recv_timeout(Duration::from_millis(receive_timeout_ms)) {
                Ok(Some(sample)) => break sample,
                Ok(None) => {
                    // No sample available, continue loop
                    continue;
                }
                Err(e) => {
                    // `recv_timeout` maps an expired timeout to `Ok(None)` (handled
                    // above), so an `Err` here is a genuine disconnect — the channel's
                    // senders were dropped (subscriber undeclared / session closed).
                    // Never classify by formatting the error string.
                    if self.flushing.load(Ordering::SeqCst) {
                        gst::debug!(CAT, imp = self, "Subscriber closed while flushing");
                        return Err(gst::FlowError::Flushing);
                    }
                    stats.lock().unwrap_or_else(|e| e.into_inner()).errors += 1;
                    gst::element_imp_error!(
                        self,
                        gst::ResourceError::Read,
                        ["Subscriber disconnected: {}", e]
                    );
                    return Err(gst::FlowError::Error);
                }
            }
        };

        // Check if the sample has attachment metadata (caps, buffer timing, compression, etc.)
        // Parse metadata once and extract all relevant information
        #[cfg(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        ))]
        let (parsed_metadata, compression_type) = if let Some(attachment) = sample.attachment() {
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

                    // Check for compression metadata (first-class field).
                    let compression = metadata
                        .compression()
                        .and_then(crate::compression::CompressionType::from_metadata_value);

                    // Log any user metadata
                    if !metadata.user_metadata().is_empty() {
                        gst::trace!(
                            CAT,
                            imp = self,
                            "Received user metadata: {:?}",
                            metadata.user_metadata()
                        );
                    }

                    (Some(metadata), compression)
                }
                Err(e) => {
                    gst::warning!(CAT, imp = self, "Failed to parse metadata: {}", e);
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        #[cfg(not(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        )))]
        let parsed_metadata = if let Some(attachment) = sample.attachment() {
            match MetadataParser::parse(attachment) {
                Ok(metadata) => {
                    // This build has no compression features. If the sender compressed
                    // the payload, we cannot decode it — fail clearly instead of pushing
                    // compressed bytes downstream as if they were raw.
                    if let Some(algo) = metadata.compression()
                        && algo != "none"
                    {
                        stats.lock().unwrap_or_else(|e| e.into_inner()).errors += 1;
                        gst::element_imp_error!(
                            self,
                            gst::StreamError::Decode,
                            [
                                "Received '{}'-compressed payload but this build has no \
                                 compression features enabled",
                                algo
                            ]
                        );
                        return Err(gst::FlowError::Error);
                    }

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

                    Some(metadata)
                }
                Err(e) => {
                    gst::warning!(CAT, imp = self, "Failed to parse metadata: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let payload = sample.payload();
        let compressed_data = payload.to_bytes();

        // Decompress if needed
        #[cfg(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        ))]
        let slice = if let Some(comp_type) = compression_type {
            match crate::compression::decompress(&compressed_data, comp_type) {
                Ok(decompressed) => {
                    gst::trace!(
                        CAT,
                        imp = self,
                        "Decompressed {} bytes to {} bytes using {:?}",
                        compressed_data.len(),
                        decompressed.len(),
                        comp_type
                    );
                    decompressed
                }
                Err(e) => {
                    stats.lock().unwrap_or_else(|e| e.into_inner()).errors += 1;
                    gst::element_imp_error!(
                        self,
                        gst::StreamError::Decode,
                        ["Decompression failed: {}", e]
                    );
                    return Err(gst::FlowError::Error);
                }
            }
        } else {
            compressed_data.to_vec()
        };

        #[cfg(not(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        )))]
        let slice = compressed_data.to_vec();

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

            // Apply buffer timing metadata if enabled and available
            // This preserves PTS, DTS, duration, offset, and flags from the sender
            if apply_buffer_meta && let Some(ref metadata) = parsed_metadata {
                // Check if we have buffer timing metadata
                let has_timing = metadata.pts().is_some()
                    || metadata.dts().is_some()
                    || metadata.duration().is_some()
                    || metadata.offset().is_some()
                    || metadata.offset_end().is_some()
                    || metadata.flags().is_some();

                if has_timing {
                    metadata.apply_to_buffer(buffer_mut);
                    gst::trace!(
                        CAT,
                        imp = self,
                        "Applied buffer timing metadata: PTS={:?}, DTS={:?}, duration={:?}, flags={:?}",
                        metadata.pts(),
                        metadata.dts(),
                        metadata.duration(),
                        metadata.flags()
                    );
                }
            }

            // If no buffer timing metadata was applied, try Zenoh timestamp as fallback
            // This is useful when receiving from a sender that doesn't use buffer metadata
            if buffer_mut.pts().is_none()
                && let Some(timestamp) = sample.timestamp()
            {
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
        {
            let mut stats = stats.lock().unwrap_or_else(|e| e.into_inner());
            stats.bytes_received += slice.len() as u64;
            stats.messages_received += 1;
        }

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
        if settings.receive_timeout_ms != 100 {
            params.push(format!(
                "receive-timeout-ms={}",
                settings.receive_timeout_ms
            ));
        }
        if !settings.apply_buffer_meta {
            params.push("apply-buffer-meta=false".to_string());
        }
        if settings.handler != HandlerMode::Fifo {
            params.push(format!("handler={}", settings.handler.as_nick()));
        }
        if settings.queue_depth != 30 {
            params.push(format!("queue-depth={}", settings.queue_depth));
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
                        "receive-timeout-ms" => {
                            let timeout: u64 = value.parse().map_err(|_| {
                                glib::Error::new(
                                    gst::URIError::BadUri,
                                    &format!("Invalid receive-timeout-ms value: {}", value),
                                )
                            })?;
                            // Clamp to valid range
                            settings.receive_timeout_ms = timeout.clamp(10, 5000);
                        }
                        "apply-buffer-meta" => {
                            settings.apply_buffer_meta = match value.as_str() {
                                "true" | "1" | "yes" => true,
                                "false" | "0" | "no" => false,
                                _ => {
                                    return Err(glib::Error::new(
                                        gst::URIError::BadUri,
                                        &format!("Invalid apply-buffer-meta value: {}", value),
                                    ));
                                }
                            };
                        }
                        "handler" => {
                            settings.handler = HandlerMode::from_nick(&value).ok_or_else(|| {
                                glib::Error::new(
                                    gst::URIError::BadUri,
                                    &format!(
                                        "Invalid handler value: {} (expected 'fifo' or 'ring')",
                                        value
                                    ),
                                )
                            })?;
                        }
                        "queue-depth" => {
                            let depth: u32 = value.parse().map_err(|_| {
                                glib::Error::new(
                                    gst::URIError::BadUri,
                                    &format!("Invalid queue-depth value: {}", value),
                                )
                            })?;
                            settings.queue_depth = depth.clamp(1, 100_000);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_mode_nick_roundtrip() {
        assert_eq!(HandlerMode::from_nick("fifo"), Some(HandlerMode::Fifo));
        assert_eq!(HandlerMode::from_nick("ring"), Some(HandlerMode::Ring));
        assert_eq!(HandlerMode::from_nick("bogus"), None);
        assert_eq!(HandlerMode::Fifo.as_nick(), "fifo");
        assert_eq!(HandlerMode::Ring.as_nick(), "ring");
        // Default preserves prior (lossless) behavior.
        assert_eq!(HandlerMode::default(), HandlerMode::Fifo);
    }

    #[test]
    fn counting_ring_drops_oldest_and_counts() {
        let dropped = Arc::new(AtomicU64::new(0));
        let ring = CountingRing::new(2, dropped.clone());

        // Push five into a depth-2 ring: the three oldest are evicted and counted.
        for i in 0..5 {
            ring.push(i);
        }
        assert_eq!(dropped.load(Ordering::Relaxed), 3);

        // Only the two most recent survive, in arrival order (drop-oldest).
        assert_eq!(ring.pop(), Some(3));
        assert_eq!(ring.pop(), Some(4));
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn counting_ring_under_capacity_never_drops() {
        let dropped = Arc::new(AtomicU64::new(0));
        let ring = CountingRing::new(5, dropped.clone());

        ring.push("a");
        ring.push("b");
        assert_eq!(dropped.load(Ordering::Relaxed), 0);

        // FIFO order preserved when within capacity.
        assert_eq!(ring.pop(), Some("a"));
        assert_eq!(ring.pop(), Some("b"));
        assert_eq!(ring.pop(), None);
    }
}
