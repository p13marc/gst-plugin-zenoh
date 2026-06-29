use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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

/// Default timeout bounding `zenoh::open(...).wait()` during NULL→READY so an
/// unreachable router can't stall the state-change thread forever.
const SESSION_OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// A publish request handed to the background publish worker thread.
///
/// The worker owns the Zenoh publisher and performs the (potentially blocking)
/// `put().wait()` off the GStreamer streaming thread, so `render()` never holds
/// the `state` lock across network I/O and can be bounded/cancelled.
struct PublishJob {
    payload: zenoh::bytes::ZBytes,
    attachment: Option<zenoh::bytes::ZBytes>,
    ack: flume::Sender<Result<(), zenoh::Error>>,
}

/// Outcome of submitting a [`PublishJob`] and awaiting its acknowledgement.
enum PublishOutcome {
    /// Publish completed successfully.
    Ok,
    /// Zenoh reported a publish error.
    Failed(zenoh::Error),
    /// The element was unlocked/flushing while waiting — abandon the publish.
    Flushing,
    /// The configured `publish-timeout-ms` elapsed before completion.
    TimedOut,
    /// The publish worker thread is gone (channel disconnected).
    WorkerGone,
}

/// Zenoh resources created during NULL→READY transition.
///
/// These are lightweight network resources (session + publish worker + matching
/// listener) that allow detecting subscriber presence without consuming pipeline
/// resources. No data flows until the pipeline reaches PLAYING state.
struct ReadyState {
    /// Channel to the background publish worker that owns the Zenoh publisher.
    /// Declared before `_session` so it drops first on teardown: dropping the
    /// sender makes the worker observe a disconnected channel and exit.
    publish_tx: flume::Sender<PublishJob>,
    /// Worker thread handle. Detached on drop (we never join, so teardown can't
    /// hang behind an in-flight block-mode publish); the worker exits on its own
    /// once `publish_tx` is dropped and any in-flight `put()` returns.
    _worker: Option<JoinHandle<()>>,
    /// Connectivity listener handle. Declared before `_session` so it undeclares
    /// while the session is still alive (avoids a no-op undeclare + error log for
    /// owned sessions); keeping the handle — rather than a `.background()`
    /// listener — is what prevents a per-cycle leak on shared sessions.
    _connectivity_listener: Option<crate::connectivity::ConnectivityListener>,
    // Keeping session field to maintain ownership and prevent session from being
    // dropped while the worker's publisher is still in use. Owned or shared.
    _session: SessionWrapper,
    /// Whether there are currently matching Zenoh subscribers.
    /// Updated via Zenoh's background matching listener callback.
    has_subscribers: Arc<AtomicBool>,
    /// Whether the Zenoh session currently has any open transport.
    /// Updated via Zenoh's background transport-events listener callback.
    connected: Arc<AtomicBool>,
}

/// Additional resources created during READY→PAUSED (start()) for data rendering.
struct Started {
    /// Zenoh resources (session, publisher, matching listener)
    ready: ReadyState,
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
    /// Zenoh session + publisher created, matching listener active.
    /// No data flows — pipeline is in GStreamer READY state.
    Ready(ReadyState),
    Starting, // Intermediate state during startup
    Started(Started),
    Stopping, // Intermediate state during shutdown
}

impl State {
    fn is_started(&self) -> bool {
        matches!(self, State::Started(_))
    }

    fn is_ready_or_started(&self) -> bool {
        matches!(self, State::Ready(_) | State::Started(_))
    }

    fn is_stopped(&self) -> bool {
        matches!(self, State::Stopped)
    }

    fn can_start(&self) -> bool {
        matches!(self, State::Ready(_))
    }

    fn can_stop(&self) -> bool {
        matches!(self, State::Started(_))
    }

    /// Returns the has_subscribers atomic, if available (Ready or Started).
    fn has_subscribers(&self) -> Option<&Arc<AtomicBool>> {
        match self {
            State::Ready(ready) => Some(&ready.has_subscribers),
            State::Started(started) => Some(&started.ready.has_subscribers),
            _ => None,
        }
    }

    /// Returns the connectivity atomic, if available (Ready or Started).
    fn connected(&self) -> Option<&Arc<AtomicBool>> {
        match self {
            State::Ready(ready) => Some(&ready.connected),
            State::Started(started) => Some(&started.ready.connected),
            _ => None,
        }
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
    /// Maximum time in milliseconds to wait for a single publish to complete before
    /// giving up (0 = wait indefinitely). Bounds `render()` so shutdown/flush can't hang.
    publish_timeout_ms: u64,
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
            send_caps: true,          // Default to sending caps for ease of use
            caps_interval: 1,         // Send caps every 1 second by default
            send_buffer_meta: true,   // Default to sending buffer timing metadata
            publish_timeout_ms: 5000, // Bound publishes at 5s by default to avoid shutdown hangs
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
    /// Set when the base sink calls `unlock()` (or a flush starts) so an in-flight
    /// `render()` abandons its publish wait promptly without touching `state`.
    unlocked: Arc<AtomicBool>,
}

impl Default for ZenohSink {
    fn default() -> Self {
        Self {
            settings: Mutex::new(Settings::default()),
            state: Mutex::new(State::default()),
            unlocked: Arc::new(AtomicBool::new(false)),
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

    /// Creates the Zenoh session, publisher, and matching listener.
    ///
    /// Called during NULL→READY to set up lightweight network resources
    /// for subscriber matching detection. No data flows at this point.
    fn create_zenoh_resources(&self) -> Result<ReadyState, gst::ErrorMessage> {
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
            gst::debug!(CAT, "Using external shared session (Rust API)");
            SessionWrapper::Shared(shared_session)
        } else if let Some(ref group) = session_group {
            gst::debug!(CAT, "Using session group '{}'", group);
            let session = crate::session::get_or_create_session(group, config_file.as_deref())
                .map_err(|e| ZenohError::Init(e).to_error_message())?;
            SessionWrapper::SharedGroup {
                session,
                group: group.clone(),
            }
        } else {
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
                    gst::ResourceError::OpenWrite,
                    [
                        "Timed out opening Zenoh session after {:?}",
                        SESSION_OPEN_TIMEOUT
                    ]
                )
            })?
            .map_err(|e| ZenohError::Init(e).to_error_message())?;
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

        let owned = OwnedKeyExpr::try_from(key_expr.clone()).map_err(|e| {
            ZenohError::KeyExpr {
                key_expr: key_expr.clone(),
                reason: e.to_string(),
            }
            .to_error_message()
        })?;

        let zenoh_priority = Priority::try_from(priority).unwrap_or(Priority::default());

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

        let zenoh_reliability = match reliability.as_str() {
            "reliable" => Reliability::Reliable,
            "best-effort" => Reliability::BestEffort,
            _ => {
                gst::warning!(CAT, "Unknown reliability '{}', using default", reliability);
                Reliability::BestEffort
            }
        };

        let mut publisher_builder = session_wrapper
            .as_session()
            .declare_publisher(owned)
            .priority(zenoh_priority)
            .congestion_control(zenoh_congestion_control)
            .reliability(zenoh_reliability);

        if express {
            publisher_builder = publisher_builder.express(true);
        }

        let publisher = publisher_builder.wait().map_err(|e| {
            ZenohError::Publish {
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

        // Set up matching status tracking via Zenoh's background callback.
        let has_subscribers = Arc::new(AtomicBool::new(false));
        {
            let has_subscribers = has_subscribers.clone();
            let element_weak = self.obj().downgrade();

            publisher
                .matching_listener()
                .callback(move |status| {
                    let matching = status.matching();
                    has_subscribers.store(matching, Ordering::Relaxed);

                    if let Some(element) = element_weak.upgrade() {
                        element.emit_by_name::<()>("matching-changed", &[&matching]);

                        let element_ref = element.upcast_ref::<gst::Element>();
                        if let Some(bus) = element_ref.bus() {
                            let s = gst::Structure::builder("zenoh-matching-changed")
                                .field("has-subscribers", matching)
                                .build();
                            let _ = bus
                                .post(gst::message::Element::builder(s).src(element_ref).build());
                        }
                    }
                })
                .background()
                .wait()
                .map_err(|e| ZenohError::Init(e).to_error_message())?;
        }

        // Check initial matching status (the callback only fires on *changes*)
        if let Ok(initial_status) = publisher.matching_status().wait() {
            has_subscribers.store(initial_status.matching(), Ordering::Relaxed);
            gst::debug!(
                CAT,
                "Initial matching status: has_subscribers={}",
                initial_status.matching()
            );
        }

        // Observe session connectivity (transport up/down) via Zenoh's
        // transport-events listener. Zenoh reconnects on its own; this only reports.
        // The handle is kept (not `.background()`) and dropped on teardown so it
        // does not leak per start/stop cycle on a shared session.
        let connected = Arc::new(AtomicBool::new(false));
        let connectivity_listener = match crate::connectivity::spawn_listener(
            session_wrapper.as_session(),
            self.obj().upcast_ref::<gst::Element>(),
            connected.clone(),
        ) {
            Ok(listener) => Some(listener),
            Err(e) => {
                // Non-fatal: connectivity reporting is best-effort, data still flows.
                gst::warning!(CAT, "Failed to start connectivity listener: {}", e);
                None
            }
        };

        // Spawn the publish worker that owns the publisher. Performing publishes on a
        // dedicated thread keeps the blocking `put().wait()` off the streaming thread,
        // so `render()` never holds the `state` lock across network I/O.
        let (publish_tx, publish_rx) = flume::unbounded::<PublishJob>();
        let worker = std::thread::Builder::new()
            .name("zenohsink-publish".into())
            .spawn(move || {
                // `publisher` is moved here and lives for the worker's lifetime, which
                // also keeps the background matching listener active.
                while let Ok(job) = publish_rx.recv() {
                    let PublishJob {
                        payload,
                        attachment,
                        ack,
                    } = job;
                    let builder = publisher.put(payload);
                    let res = match attachment {
                        Some(att) => builder.attachment(att).wait(),
                        None => builder.wait(),
                    };
                    // Receiver may be gone (render() bailed on timeout/flush); ignore.
                    let _ = ack.send(res);
                }
            })
            .map_err(|e| {
                gst::error_msg!(
                    gst::ResourceError::Failed,
                    ["Failed to spawn publish worker thread: {}", e]
                )
            })?;

        Ok(ReadyState {
            publish_tx,
            _worker: Some(worker),
            _connectivity_listener: connectivity_listener,
            _session: session_wrapper,
            has_subscribers,
            connected,
        })
    }

    /// Submit a publish job to the worker and wait (bounded) for its result.
    ///
    /// Returns promptly as [`PublishOutcome::Flushing`] if the element is unlocked
    /// while waiting, and as [`PublishOutcome::TimedOut`] once `publish_timeout_ms`
    /// elapses — so the streaming thread can never block indefinitely on a publish.
    fn submit_publish(
        &self,
        publish_tx: &flume::Sender<PublishJob>,
        payload: zenoh::bytes::ZBytes,
        attachment: Option<zenoh::bytes::ZBytes>,
        publish_timeout_ms: u64,
    ) -> PublishOutcome {
        let (ack_tx, ack_rx) = flume::bounded(1);
        if publish_tx
            .send(PublishJob {
                payload,
                attachment,
                ack: ack_tx,
            })
            .is_err()
        {
            return PublishOutcome::WorkerGone;
        }

        let deadline = (publish_timeout_ms > 0)
            .then(|| Instant::now() + Duration::from_millis(publish_timeout_ms));

        loop {
            if self.unlocked.load(Ordering::SeqCst) {
                return PublishOutcome::Flushing;
            }
            match ack_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(())) => return PublishOutcome::Ok,
                Ok(Err(e)) => return PublishOutcome::Failed(e),
                Err(flume::RecvTimeoutError::Timeout) => {
                    if let Some(d) = deadline
                        && Instant::now() >= d
                    {
                        return PublishOutcome::TimedOut;
                    }
                }
                Err(flume::RecvTimeoutError::Disconnected) => return PublishOutcome::WorkerGone,
            }
        }
    }

    /// Decide whether caps should be attached to the current buffer (first buffer,
    /// caps change, or periodic interval), updating the shared caps-tracking state.
    /// Returns the caps to attach, or `None`.
    fn decide_caps_to_send(
        &self,
        current_caps: Option<gst::Caps>,
        send_caps: bool,
        caps_interval: u32,
        caps_sent: &AtomicBool,
        last_caps: &Mutex<Option<gst::Caps>>,
        last_caps_time: &Mutex<Option<Instant>>,
    ) -> Option<gst::Caps> {
        if !send_caps {
            return None;
        }
        let caps = current_caps?;
        let mut should_send = false;

        if !caps_sent.load(Ordering::Acquire) {
            should_send = true;
            caps_sent.store(true, Ordering::Release);
            *last_caps.lock().unwrap_or_else(|e| e.into_inner()) = Some(caps.clone());
            *last_caps_time.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        } else {
            let last = last_caps.lock().unwrap_or_else(|e| e.into_inner());
            if last.as_ref() != Some(&caps) {
                should_send = true;
                drop(last);
                *last_caps.lock().unwrap_or_else(|e| e.into_inner()) = Some(caps.clone());
                *last_caps_time.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
            } else if caps_interval > 0 {
                let last_time = last_caps_time.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(t) = *last_time
                    && t.elapsed().as_secs() >= caps_interval as u64
                {
                    should_send = true;
                    drop(last_time);
                    *last_caps_time.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(Instant::now());
                }
            }
        }

        should_send.then_some(caps)
    }

    /// Encode a single buffer (optional compression + metadata) and publish it via
    /// the worker. Shared by `render()` and `render_list()` so batched buffers get
    /// identical compression and buffer-timing metadata. Updates success stats on
    /// `Ok`; the caller maps failures to a flow return.
    #[allow(clippy::too_many_arguments)]
    fn encode_and_publish(
        &self,
        data: &[u8],
        buffer: &gst::BufferRef,
        caps: Option<&gst::Caps>,
        send_buffer_meta: bool,
        publish_tx: &flume::Sender<PublishJob>,
        stats: &Arc<Mutex<Statistics>>,
        publish_timeout_ms: u64,
    ) -> PublishOutcome {
        #[cfg(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        ))]
        let original_size = data.len();

        // Apply compression if configured.
        #[cfg(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        ))]
        let (payload_bytes, compressed) = {
            let (compression_type, compression_level) = {
                let settings = self.settings.lock().unwrap_or_else(|e| e.into_inner());
                (settings.compression, settings.compression_level)
            };
            if compression_type != crate::compression::CompressionType::None {
                match crate::compression::compress(data, compression_type, compression_level) {
                    Ok(c) => (c, true),
                    Err(e) => {
                        gst::warning!(
                            CAT,
                            imp = self,
                            "Compression failed: {}, sending uncompressed",
                            e
                        );
                        stats.lock().unwrap_or_else(|e| e.into_inner()).errors += 1;
                        (data.to_vec(), false)
                    }
                }
            } else {
                (data.to_vec(), false)
            }
        };

        #[cfg(not(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        )))]
        let (payload_bytes, compressed) = (data.to_vec(), false);

        // Build the attachment if any metadata is needed.
        let needs_attachment = caps.is_some() || send_buffer_meta || compressed;
        let attachment = if needs_attachment {
            let mut mb = MetadataBuilder::new();
            if let Some(caps) = caps {
                mb = mb.caps(caps);
            }
            if send_buffer_meta {
                mb = mb.buffer_timing(buffer);
            }
            #[cfg(any(
                feature = "compression-zstd",
                feature = "compression-lz4",
                feature = "compression-gzip"
            ))]
            if compressed {
                let algo = {
                    let settings = self.settings.lock().unwrap_or_else(|e| e.into_inner());
                    settings.compression.to_metadata_value()
                };
                mb = mb.compression(algo);
            }
            mb.build()
        } else {
            None
        };

        let payload_len = payload_bytes.len() as u64;
        let payload = zenoh::bytes::ZBytes::from(payload_bytes);
        let outcome = self.submit_publish(publish_tx, payload, attachment, publish_timeout_ms);

        if matches!(outcome, PublishOutcome::Ok) {
            let mut s = stats.lock().unwrap_or_else(|e| e.into_inner());
            s.bytes_sent += payload_len;
            s.messages_sent += 1;
            #[cfg(any(
                feature = "compression-zstd",
                feature = "compression-lz4",
                feature = "compression-gzip"
            ))]
            if compressed {
                s.bytes_before_compression += original_size as u64;
                s.bytes_after_compression += payload_len;
            }
        }

        outcome
    }

    /// Map a single-buffer publish outcome to a GStreamer flow return, posting an
    /// element error and updating the error stat where appropriate.
    fn outcome_to_flow(
        &self,
        outcome: PublishOutcome,
        stats: &Arc<Mutex<Statistics>>,
        publish_timeout_ms: u64,
    ) -> Result<gst::FlowSuccess, gst::FlowError> {
        match outcome {
            PublishOutcome::Ok => Ok(gst::FlowSuccess::Ok),
            PublishOutcome::Failed(e) => {
                stats.lock().unwrap_or_else(|e| e.into_inner()).errors += 1;
                let key_expr = self
                    .settings
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .key_expr
                    .clone();
                let err = ZenohError::Publish {
                    key_expr,
                    source: e,
                };
                gst::element_imp_error!(self, gst::ResourceError::Write, ["{}", err]);
                Err(err.to_flow_error())
            }
            PublishOutcome::Flushing => {
                gst::debug!(CAT, imp = self, "Publish abandoned: element is flushing");
                Err(gst::FlowError::Flushing)
            }
            PublishOutcome::TimedOut => {
                stats.lock().unwrap_or_else(|e| e.into_inner()).errors += 1;
                gst::element_imp_error!(
                    self,
                    gst::ResourceError::Write,
                    ["Publish timed out after {} ms", publish_timeout_ms]
                );
                Err(gst::FlowError::Error)
            }
            PublishOutcome::WorkerGone => {
                gst::element_imp_error!(
                    self,
                    gst::CoreError::Failed,
                    ["Publish worker thread is not available"]
                );
                Err(gst::FlowError::Error)
            }
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
        match transition {
            gst::StateChange::NullToReady => {
                // Create Zenoh session, publisher, and matching listener.
                // This is lightweight — no data flows, but subscriber
                // matching detection is available from READY state.
                let ready_state = self.create_zenoh_resources().map_err(|err| {
                    gst::error!(CAT, "Failed to create Zenoh resources: {:?}", err);
                    gst::StateChangeError
                })?;
                let mut state = self.state.lock().unwrap();
                *state = State::Ready(ready_state);
            }
            gst::StateChange::ReadyToNull => {
                // Clean up all Zenoh resources.
                let mut state = self.state.lock().unwrap();
                *state = State::Stopped;
                gst::debug!(CAT, "Zenoh resources cleaned up (READY→NULL)");
            }
            _ => {}
        }

        self.parent_change_state(transition)
    }
}

impl ObjectImpl for ZenohSink {
    fn constructed(&self) {
        self.parent_constructed();
    }

    fn signals() -> &'static [glib::subclass::Signal] {
        static SIGNALS: LazyLock<Vec<glib::subclass::Signal>> = LazyLock::new(|| {
            vec![
                glib::subclass::Signal::builder("matching-changed")
                    .param_types([bool::static_type()])
                    .build(),
            ]
        });
        SIGNALS.as_ref()
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
                // Publish timeout property
                glib::ParamSpecUInt64::builder("publish-timeout-ms")
                    .nick("Publish Timeout")
                    .blurb("Maximum time in milliseconds to wait for a single publish to complete before giving up (0 = wait indefinitely). Bounds render() so shutdown/flush cannot hang.")
                    .default_value(5000)
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
                // Matching status property (read-only)
                glib::ParamSpecBoolean::builder("has-subscribers")
                    .nick("Has Subscribers")
                    .blurb("Whether there are currently matching Zenoh subscribers for this publisher's key expression")
                    .default_value(false)
                    .read_only()
                    .build(),
                // Connectivity property (read-only)
                glib::ParamSpecBoolean::builder("connected")
                    .nick("Connected")
                    .blurb("Whether the Zenoh session currently has any open transport. Zenoh reconnects on its own; this reports transport up/down (see the 'zenoh-connectivity-changed' bus message).")
                    .default_value(false)
                    .read_only()
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
        // Check if we're in a state where property changes are allowed.
        // Zenoh resources (session, publisher) are created during NULL→READY,
        // so these properties are locked once in Ready or Started state.
        let state = self.state.lock().unwrap();
        if state.is_ready_or_started()
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
                "Cannot change property '{}' while element is in Ready or Started state",
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
            "publish-timeout-ms" => {
                settings.publish_timeout_ms = value.get::<u64>().expect("type checked upstream");
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
            | "express" | "send-caps" | "caps-interval" | "send-buffer-meta"
            | "publish-timeout-ms" | "session-group" => {
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
                    "publish-timeout-ms" => settings.publish_timeout_ms.to_value(),
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
            // Matching status - available in Ready or Started state
            "has-subscribers" => {
                let state = self.state.lock().unwrap();
                if let Some(has_subscribers) = state.has_subscribers() {
                    has_subscribers.load(Ordering::Relaxed).to_value()
                } else {
                    false.to_value()
                }
            }
            // Connectivity - available in Ready or Started state
            "connected" => {
                let state = self.state.lock().unwrap();
                if let Some(connected) = state.connected() {
                    connected.load(Ordering::Relaxed).to_value()
                } else {
                    false.to_value()
                }
            }
            // Statistics properties - only available in Started state (data is flowing)
            "bytes-sent" | "messages-sent" | "errors" => {
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    let stats = started.stats.lock().unwrap();
                    match pspec.name() {
                        "bytes-sent" => stats.bytes_sent.to_value(),
                        "messages-sent" => stats.messages_sent.to_value(),
                        "errors" => stats.errors.to_value(),
                        _ => unreachable!(),
                    }
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

        // Check if we can start from current state (must be Ready)
        if !state.can_start() {
            let current_state = match *state {
                State::Stopped => "Stopped",
                State::Ready(_) => "Ready",
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

        gst::debug!(CAT, "ZenohSink transitioning from Ready to Started");

        // Clear any leftover unlock/flush signal from a previous run.
        self.unlocked.store(false, Ordering::SeqCst);

        // Take the ReadyState and promote it to Started with render-time resources
        let ready_state = match std::mem::replace(&mut *state, State::Starting) {
            State::Ready(ready) => ready,
            _ => unreachable!(),
        };

        *state = State::Started(Started {
            ready: ready_state,
            stats: Arc::new(Mutex::new(Statistics::default())),
            caps_sent: Arc::new(AtomicBool::new(false)),
            last_caps_time: Arc::new(Mutex::new(None)),
            last_caps: Arc::new(Mutex::new(None)),
        });
        gst::debug!(CAT, "ZenohSink successfully transitioned to Started state");

        Ok(())
    }

    fn render(&self, buffer: &gst::Buffer) -> Result<gst::FlowSuccess, gst::FlowError> {
        // Clone out the render-time resources under a short-lived lock, then release
        // it before any (blocking) network I/O so stats and state transitions stay
        // observable during active traffic.
        let (publish_tx, stats, caps_sent, last_caps, last_caps_time, publish_timeout_ms) = {
            let state_locked = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let State::Started(ref started) = *state_locked else {
                gst::element_imp_error!(self, gst::CoreError::Failed, ["Not started yet"]);
                return Err(gst::FlowError::Error);
            };
            let publish_timeout_ms = self
                .settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .publish_timeout_ms;
            (
                started.ready.publish_tx.clone(),
                started.stats.clone(),
                started.caps_sent.clone(),
                started.last_caps.clone(),
                started.last_caps_time.clone(),
                publish_timeout_ms,
            )
        };

        let (send_caps, caps_interval, send_buffer_meta) = {
            let settings = self.settings.lock().unwrap_or_else(|e| e.into_inner());
            (
                settings.send_caps,
                settings.caps_interval,
                settings.send_buffer_meta,
            )
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

        let caps_to_send = self.decide_caps_to_send(
            self.obj().sink_pad().current_caps(),
            send_caps,
            caps_interval,
            &caps_sent,
            &last_caps,
            &last_caps_time,
        );

        let outcome = self.encode_and_publish(
            b.as_slice(),
            buffer,
            caps_to_send.as_ref(),
            send_buffer_meta,
            &publish_tx,
            &stats,
            publish_timeout_ms,
        );
        self.outcome_to_flow(outcome, &stats, publish_timeout_ms)
    }

    fn render_list(&self, list: &gst::BufferList) -> Result<gst::FlowSuccess, gst::FlowError> {
        gst::debug!(
            CAT,
            imp = self,
            "Rendering buffer list with {} buffers",
            list.len()
        );

        // Clone out render-time resources under a short-lived lock, then release it
        // before doing any network I/O (mirrors render()).
        let (publish_tx, stats, caps_sent, last_caps, last_caps_time, publish_timeout_ms) = {
            let state_locked = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let State::Started(ref started) = *state_locked else {
                gst::element_imp_error!(self, gst::CoreError::Failed, ["Not started yet"]);
                return Err(gst::FlowError::Error);
            };
            let publish_timeout_ms = self
                .settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .publish_timeout_ms;
            (
                started.ready.publish_tx.clone(),
                started.stats.clone(),
                started.caps_sent.clone(),
                started.last_caps.clone(),
                started.last_caps_time.clone(),
                publish_timeout_ms,
            )
        };

        // Track statistics for the batch
        let mut total_messages = 0u64;
        let mut errors_count = 0u64;

        let (send_caps, caps_interval, send_buffer_meta) = {
            let settings = self.settings.lock().unwrap_or_else(|e| e.into_inner());
            (
                settings.send_caps,
                settings.caps_interval,
                settings.send_buffer_meta,
            )
        };

        // Decide caps once for the batch; attach only to the first buffer that goes out.
        let mut caps_to_send = self.decide_caps_to_send(
            self.obj().sink_pad().current_caps(),
            send_caps,
            caps_interval,
            &caps_sent,
            &last_caps,
            &last_caps_time,
        );

        // Process each buffer in the list through the same encode path as render(),
        // so batched buffers get identical compression and buffer-timing metadata.
        for buffer in list.iter() {
            let b = buffer.map_readable().map_err(|_| {
                gst::element_imp_error!(
                    self,
                    gst::ResourceError::Read,
                    ["Failed to map buffer for reading in buffer list"]
                );
                errors_count += 1;
                gst::FlowError::Error
            })?;

            let caps_for_this = caps_to_send.take();
            match self.encode_and_publish(
                b.as_slice(),
                buffer,
                caps_for_this.as_ref(),
                send_buffer_meta,
                &publish_tx,
                &stats,
                publish_timeout_ms,
            ) {
                PublishOutcome::Ok => {
                    total_messages += 1;
                }
                PublishOutcome::Flushing => {
                    gst::debug!(CAT, imp = self, "Buffer list publish abandoned: flushing");
                    return Err(gst::FlowError::Flushing);
                }
                PublishOutcome::Failed(e) => {
                    errors_count += 1;
                    stats.lock().unwrap_or_else(|e| e.into_inner()).errors += 1;
                    gst::warning!(CAT, imp = self, "Error publishing buffer in list: {}", e);
                    // Continue processing remaining buffers instead of failing immediately.
                }
                PublishOutcome::TimedOut => {
                    errors_count += 1;
                    stats.lock().unwrap_or_else(|e| e.into_inner()).errors += 1;
                    gst::warning!(
                        CAT,
                        imp = self,
                        "Publish in buffer list timed out after {} ms",
                        publish_timeout_ms
                    );
                }
                PublishOutcome::WorkerGone => {
                    gst::element_imp_error!(
                        self,
                        gst::CoreError::Failed,
                        ["Publish worker thread is not available"]
                    );
                    return Err(gst::FlowError::Error);
                }
            }
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
                State::Ready(_) => "Ready",
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

        if let State::Started(_) = *state {
            gst::debug!(
                CAT,
                "ZenohSink transitioning from Started to Ready (PAUSED→READY)"
            );
            // Demote Started back to Ready, keeping Zenoh resources alive
            let started_data = match std::mem::replace(&mut *state, State::Stopping) {
                State::Started(started) => started,
                _ => unreachable!(),
            };

            // Return to Ready state — Zenoh session, publisher, and matching
            // listener remain active for subscriber detection.
            *state = State::Ready(started_data.ready);
            gst::debug!(
                CAT,
                "ZenohSink render resources cleaned up, Zenoh resources retained"
            );
        }

        Ok(())
    }

    fn unlock(&self) -> Result<(), gst::ErrorMessage> {
        gst::debug!(
            CAT,
            imp = self,
            "Unlock called - cancelling any pending publish"
        );
        // Set lock-free so an in-flight render() abandons its publish wait promptly.
        self.unlocked.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn unlock_stop(&self) -> Result<(), gst::ErrorMessage> {
        gst::debug!(
            CAT,
            imp = self,
            "Unlock stop called - resuming normal operation"
        );
        self.unlocked.store(false, Ordering::SeqCst);
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
                gst::debug!(CAT, imp = self, "Flush start - cancelling pending publish");
                // Abort any in-flight publish without touching the state lock.
                self.unlocked.store(true, Ordering::SeqCst);
                self.parent_event(event)
            }
            EventView::FlushStop(_) => {
                gst::debug!(CAT, imp = self, "Flush stop - ready for new data");
                self.unlocked.store(false, Ordering::SeqCst);
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

        // Check if we can modify settings (not in Ready or Started state)
        let state = self.state.lock().unwrap();
        if state.is_ready_or_started() {
            drop(state);
            drop(settings);
            return Err(glib::Error::new(
                gst::URIError::BadState,
                "Cannot change URI while element is in Ready or Started state",
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
