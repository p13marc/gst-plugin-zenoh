use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use gst::{glib, prelude::*, subclass::prelude::*};
use zenoh::Wait;

use crate::error::{ErrorHandling, ZenohError};
use crate::metadata::MetadataParser;

// Define debug category for logging
static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new(
        "zenohdemux",
        gst::DebugColorFlags::empty(),
        Some("Zenoh Demux"),
    )
});

/// How to derive pad names from key expressions
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, glib::Enum)]
#[enum_type(name = "GstZenohDemuxPadNaming")]
#[repr(u32)]
pub enum PadNaming {
    /// Use full key expression: "camera/front" -> "camera_front"
    #[default]
    #[enum_value(name = "Full Path", nick = "full-path")]
    FullPath = 0,
    /// Use last segment only: "camera/front" -> "front"
    #[enum_value(name = "Last Segment", nick = "last-segment")]
    LastSegment = 1,
    /// Use hash of key expression: "camera/front" -> "pad_a1b2c3"
    #[enum_value(name = "Hash", nick = "hash")]
    Hash = 2,
}

/// Statistics tracking for ZenohDemux
#[derive(Debug, Clone, Default)]
struct Statistics {
    bytes_received: u64,
    messages_received: u64,
    pads_created: u64,
    errors: u64,
}

struct Started {
    /// Session source. When `Some`, this element holds a reference to a shared
    /// session group that must be released on stop; otherwise the owned session
    /// is dropped directly. Kept alive for the element's lifetime.
    _session: zenoh::Session,
    /// Group name to release on stop (only set for shared-group sessions).
    session_group: Option<String>,
    /// Flag to signal that the element is stopping
    stopping: Arc<AtomicBool>,
    /// Statistics tracking
    stats: Arc<Mutex<Statistics>>,
    /// Map of **full key expression** -> source pad. Keyed by the key expression
    /// (not the derived pad name) so two distinct key-exprs can never be merged
    /// onto a single pad even if their pad names would collide.
    pads: Arc<Mutex<HashMap<String, gst::Pad>>>,
    /// Receiver thread handle
    thread_handle: Option<std::thread::JoinHandle<()>>,
}

#[derive(Default)]
enum State {
    #[default]
    Stopped,
    Started(Started),
}

/// Configuration settings for the ZenohDemux element.
#[derive(Debug)]
struct Settings {
    /// Zenoh key expression for subscribing (supports wildcards)
    key_expr: String,
    /// Optional path to Zenoh configuration file
    config_file: Option<String>,
    /// How to name pads from key expressions
    pad_naming: PadNaming,
    /// Receive timeout in milliseconds
    receive_timeout_ms: u64,
    /// Quiescence window (ms) after which `no_more_pads()` is emitted so downstream
    /// auto-pluggers (e.g. decodebin) can finalize. 0 disables emission.
    no_more_pads_timeout_ms: u64,
    /// Session group name for sharing sessions via property (gst-launch compatible)
    session_group: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            key_expr: String::new(),
            config_file: None,
            pad_naming: PadNaming::FullPath,
            receive_timeout_ms: 100,
            no_more_pads_timeout_ms: 500,
            session_group: None,
        }
    }
}

/// Convert a key expression to a valid GStreamer pad name
fn key_expr_to_pad_name(key_expr: &str, naming: PadNaming) -> String {
    match naming {
        PadNaming::FullPath => {
            // Replace invalid characters with underscores
            key_expr
                .replace('/', "_")
                .replace('*', "wildcard")
                .replace(' ', "_")
        }
        PadNaming::LastSegment => {
            // Use only the last segment of the key expression
            key_expr
                .split('/')
                .next_back()
                .unwrap_or("unknown")
                .replace('*', "wildcard")
                .replace(' ', "_")
        }
        PadNaming::Hash => {
            // Use a *stable* hash of the key expression. `DefaultHasher` is not
            // guaranteed stable across Rust versions/platforms, so use FNV-1a with
            // a full 32-bit width (was truncated to 24 bits) to reduce collisions.
            format!("pad_{:08x}", fnv1a_32(key_expr.as_bytes()))
        }
    }
}

/// FNV-1a 32-bit hash — small, fast, and stable across versions/platforms.
fn fnv1a_32(data: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &b in data {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Pick a pad name based on `base` that does not collide with any pad already in
/// `existing`. Pad names must be unique within an element, so when two distinct
/// key expressions derive the same name we disambiguate with a numeric suffix.
fn unique_pad_name(base: &str, existing: &HashMap<String, gst::Pad>) -> String {
    let used: std::collections::HashSet<String> =
        existing.values().map(|p| p.name().to_string()).collect();
    if !used.contains(base) {
        return base.to_string();
    }
    let mut n = 1u32;
    loop {
        let candidate = format!("{base}_{n}");
        if !used.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// GStreamer ZenohDemux element implementation.
pub struct ZenohDemux {
    settings: Mutex<Settings>,
    state: Mutex<State>,
}

impl Default for ZenohDemux {
    fn default() -> Self {
        Self {
            settings: Mutex::new(Settings::default()),
            state: Mutex::new(State::default()),
        }
    }
}

impl GstObjectImpl for ZenohDemux {}

impl ElementImpl for ZenohDemux {
    fn metadata() -> Option<&'static gst::subclass::ElementMetadata> {
        static ELEMENT_METADATA: LazyLock<gst::subclass::ElementMetadata> = LazyLock::new(|| {
            gst::subclass::ElementMetadata::new(
                "Zenoh Stream Demultiplexer",
                "Demuxer/Network/Protocol",
                "Demultiplexes Zenoh streams by key expression, creating dynamic pads for each unique key",
                "Marc Pardo <p13marc@gmail.com>",
            )
        });
        Some(&*ELEMENT_METADATA)
    }

    fn pad_templates() -> &'static [gst::PadTemplate] {
        static PAD_TEMPLATES: LazyLock<Vec<gst::PadTemplate>> = LazyLock::new(|| {
            // Dynamic source pads - created on demand
            let src_template = gst::PadTemplate::new(
                "src_%s",
                gst::PadDirection::Src,
                gst::PadPresence::Sometimes,
                &gst::Caps::new_any(),
            )
            .unwrap();

            vec![src_template]
        });

        PAD_TEMPLATES.as_ref()
    }

    fn change_state(
        &self,
        transition: gst::StateChange,
    ) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        gst::debug!(CAT, imp = self, "State change: {:?}", transition);

        match transition {
            gst::StateChange::NullToReady => {
                // Validate settings before starting
                let settings = self.settings.lock().unwrap();
                if settings.key_expr.is_empty() {
                    gst::element_imp_error!(
                        self,
                        gst::ResourceError::Settings,
                        ["Key expression is required"]
                    );
                    return Err(gst::StateChangeError);
                }
            }
            gst::StateChange::ReadyToPaused => {
                if let Err(e) = self.start() {
                    gst::element_imp_error!(
                        self,
                        gst::ResourceError::OpenRead,
                        ["Failed to start: {}", e]
                    );
                    return Err(gst::StateChangeError);
                }
            }
            gst::StateChange::PausedToReady => {
                self.stop();
            }
            _ => {}
        }

        self.parent_change_state(transition)
    }
}

impl ObjectImpl for ZenohDemux {
    fn properties() -> &'static [glib::ParamSpec] {
        static PROPERTIES: LazyLock<Vec<glib::ParamSpec>> = LazyLock::new(|| {
            vec![
                glib::ParamSpecString::builder("key-expr")
                    .nick("Zenoh Key Expression")
                    .blurb("Zenoh key expression for subscribing. Use wildcards (* or **) to match multiple streams.")
                    .build(),
                glib::ParamSpecString::builder("config")
                    .nick("Zenoh Configuration")
                    .blurb("Path to Zenoh configuration file (JSON5 format)")
                    .build(),
                glib::ParamSpecEnum::builder_with_default("pad-naming", PadNaming::FullPath)
                    .nick("Pad Naming Strategy")
                    .blurb("How to derive pad names from key expressions")
                    .build(),
                glib::ParamSpecUInt64::builder("receive-timeout-ms")
                    .nick("Receive Timeout")
                    .blurb("Timeout in milliseconds for polling Zenoh subscriber")
                    .default_value(100)
                    .minimum(10)
                    .maximum(5000)
                    .build(),
                glib::ParamSpecUInt64::builder("no-more-pads-timeout-ms")
                    .nick("No More Pads Timeout")
                    .blurb("Quiescence window in milliseconds after which no_more_pads() is emitted so downstream auto-pluggers (e.g. decodebin) can finalize (0 = never emit)")
                    .default_value(500)
                    .build(),
                // Session sharing property
                glib::ParamSpecString::builder("session-group")
                    .nick("Session Group")
                    .blurb("Name of the session group for sharing Zenoh sessions across elements. Elements with the same group name share a single session.")
                    .build(),
                // Statistics (read-only)
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
                glib::ParamSpecUInt64::builder("pads-created")
                    .nick("Pads Created")
                    .blurb("Number of dynamic pads created")
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
            "pad-naming" => {
                settings.pad_naming = value.get::<PadNaming>().expect("type checked upstream");
            }
            "receive-timeout-ms" => {
                settings.receive_timeout_ms = value.get::<u64>().expect("type checked upstream");
            }
            "no-more-pads-timeout-ms" => {
                settings.no_more_pads_timeout_ms =
                    value.get::<u64>().expect("type checked upstream");
            }
            "session-group" => {
                settings.session_group = value
                    .get::<Option<String>>()
                    .expect("type checked upstream");
            }
            name => {
                gst::warning!(CAT, imp = self, "Unknown property: {}", name);
            }
        }
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        match pspec.name() {
            "key-expr" => self.settings.lock().unwrap().key_expr.to_value(),
            "config" => self.settings.lock().unwrap().config_file.to_value(),
            "pad-naming" => self.settings.lock().unwrap().pad_naming.to_value(),
            "receive-timeout-ms" => self.settings.lock().unwrap().receive_timeout_ms.to_value(),
            "no-more-pads-timeout-ms" => self
                .settings
                .lock()
                .unwrap()
                .no_more_pads_timeout_ms
                .to_value(),
            "session-group" => self.settings.lock().unwrap().session_group.to_value(),
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
            "pads-created" => {
                let state = self.state.lock().unwrap();
                if let State::Started(ref started) = *state {
                    started.stats.lock().unwrap().pads_created.to_value()
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
                gst::warning!(CAT, imp = self, "Unknown property: {}", name);
                "".to_value()
            }
        }
    }
}

#[glib::object_subclass]
impl ObjectSubclass for ZenohDemux {
    const NAME: &'static str = "GstZenohDemux";
    type Type = super::ZenohDemux;
    type ParentType = gst::Element;
}

impl ZenohDemux {
    fn start(&self) -> Result<(), gst::ErrorMessage> {
        let mut state = self.state.lock().unwrap();
        if matches!(*state, State::Started(_)) {
            return Ok(());
        }

        let settings = self.settings.lock().unwrap();
        let key_expr = settings.key_expr.clone();
        let config_file = settings.config_file.clone();
        let pad_naming = settings.pad_naming;
        let receive_timeout_ms = settings.receive_timeout_ms;
        let no_more_pads_timeout_ms = settings.no_more_pads_timeout_ms;
        let session_group = settings.session_group.clone();
        drop(settings);

        // Determine session source: session-group (property) > new session
        let session = if let Some(ref group) = session_group {
            // Use session group (gst-launch compatible)
            gst::debug!(CAT, imp = self, "Using session group '{}'", group);
            crate::session::get_or_create_session(group, config_file.as_deref())
                .map_err(|e| ZenohError::Init(e).to_error_message())?
        } else {
            // Create a new session
            gst::debug!(CAT, imp = self, "Creating new Zenoh session");
            let config = match config_file {
                Some(path) if !path.is_empty() => {
                    gst::debug!(CAT, imp = self, "Loading Zenoh config from {}", path);
                    zenoh::Config::from_file(&path)
                        .map_err(|e| ZenohError::Init(e).to_error_message())?
                }
                _ => zenoh::Config::default(),
            };
            zenoh::open(config)
                .wait()
                .map_err(|e| ZenohError::Init(e).to_error_message())?
        };

        gst::debug!(
            CAT,
            imp = self,
            "Creating subscriber with key_expr='{}'",
            key_expr
        );

        // Single subscriber for the receiver thread. (A previous second subscriber
        // "for potential future use" doubled inbound traffic and was never read.)
        let subscriber = session
            .declare_subscriber(&key_expr)
            .wait()
            .map_err(|e| ZenohError::Init(e).to_error_message())?;

        let stopping = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(Mutex::new(Statistics::default()));
        let pads: Arc<Mutex<HashMap<String, gst::Pad>>> = Arc::new(Mutex::new(HashMap::new()));

        // Clone for the receiver thread
        let stopping_clone = stopping.clone();
        let stats_clone = stats.clone();
        let pads_clone = pads.clone();
        let element = self.obj().clone();

        // Spawn receiver thread
        let thread_handle = std::thread::spawn(move || {
            Self::receiver_loop(
                element,
                subscriber,
                stopping_clone,
                stats_clone,
                pads_clone,
                pad_naming,
                receive_timeout_ms,
                no_more_pads_timeout_ms,
            );
        });

        *state = State::Started(Started {
            _session: session,
            session_group,
            stopping,
            stats,
            pads,
            thread_handle: Some(thread_handle),
        });

        gst::debug!(CAT, imp = self, "ZenohDemux started successfully");
        Ok(())
    }

    fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        if let State::Started(ref mut started) = *state {
            gst::debug!(CAT, imp = self, "Stopping ZenohDemux");

            // Signal the receiver thread to stop
            started.stopping.store(true, Ordering::SeqCst);

            // Wait for the thread to finish
            if let Some(handle) = started.thread_handle.take() {
                drop(state); // Release lock before joining
                let _ = handle.join();
                state = self.state.lock().unwrap();
            }

            // Push EOS to each dynamic pad, then remove it, so downstream shuts
            // down cleanly instead of hanging waiting for end-of-stream.
            let session_group = if let State::Started(ref started) = *state {
                let pads = started.pads.lock().unwrap();
                for pad in pads.values() {
                    let _ = pad.push_event(gst::event::Eos::new());
                    let _ = self.obj().remove_pad(pad);
                }
                started.session_group.clone()
            } else {
                None
            };

            // Release the shared session reference (closes it on the last release).
            *state = State::Stopped;
            drop(state);
            if let Some(group) = session_group {
                crate::session::release_session(&group);
            }
            gst::debug!(CAT, imp = self, "ZenohDemux stopped");
            return;
        }

        *state = State::Stopped;
        gst::debug!(CAT, imp = self, "ZenohDemux stopped");
    }

    /// Create, activate and add a dynamic source pad, pushing the mandatory
    /// stream-start and segment events. Returns an error instead of panicking so
    /// the receiver thread can surface a GStreamer error and keep running.
    fn create_src_pad(
        element: &super::ZenohDemux,
        pad_name: &str,
        key_expr: &str,
    ) -> Result<gst::Pad, glib::BoolError> {
        let templ = element
            .pad_template("src_%s")
            .ok_or_else(|| glib::bool_error!("missing 'src_%s' pad template"))?;
        let pad = gst::Pad::builder_from_template(&templ)
            .name(pad_name)
            .build();
        pad.set_active(true)?;
        element.add_pad(&pad)?;

        // Send stream-start + segment events (required before any data).
        let stream_id = format!("zenohdemux/{pad_name}/{key_expr}");
        pad.push_event(gst::event::StreamStart::new(&stream_id));
        let segment = gst::FormattedSegment::<gst::ClockTime>::new();
        pad.push_event(gst::event::Segment::new(&segment));
        Ok(pad)
    }

    #[allow(clippy::too_many_arguments)]
    fn receiver_loop(
        element: super::ZenohDemux,
        subscriber: zenoh::pubsub::Subscriber<
            zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>,
        >,
        stopping: Arc<AtomicBool>,
        stats: Arc<Mutex<Statistics>>,
        pads: Arc<Mutex<HashMap<String, gst::Pad>>>,
        pad_naming: PadNaming,
        receive_timeout_ms: u64,
        no_more_pads_timeout_ms: u64,
    ) {
        gst::debug!(CAT, "Receiver loop started");

        // Track quiescence so `no_more_pads()` can be emitted once the set of pads
        // appears stable, letting downstream auto-pluggers (decodebin) finalize.
        let mut last_pad_added: Option<std::time::Instant> = None;
        let mut no_more_pads_emitted = false;

        while !stopping.load(Ordering::SeqCst) {
            // Emit no_more_pads() after the configured quiescence window.
            if !no_more_pads_emitted
                && no_more_pads_timeout_ms > 0
                && let Some(t) = last_pad_added
                && t.elapsed() >= Duration::from_millis(no_more_pads_timeout_ms)
            {
                gst::debug!(CAT, "Quiescence reached; emitting no_more_pads()");
                element.no_more_pads();
                no_more_pads_emitted = true;
            }

            // Use recv_timeout to remain responsive to stopping signal
            match subscriber.recv_timeout(Duration::from_millis(receive_timeout_ms)) {
                Ok(Some(sample)) => {
                    // Get the key expression this sample arrived on
                    let sample_key_expr = sample.key_expr().as_str().to_string();

                    // Get or create the pad for this key expression. The map is keyed
                    // by the *full key expression* so two distinct keys never merge.
                    let pad = {
                        let mut pads_guard = pads.lock().unwrap();
                        if let Some(pad) = pads_guard.get(&sample_key_expr) {
                            pad.clone()
                        } else {
                            // Derive a unique pad name (disambiguating any collision
                            // between two different key-exprs mapping to the same name).
                            let base_name = key_expr_to_pad_name(&sample_key_expr, pad_naming);
                            let pad_name = unique_pad_name(&base_name, &pads_guard);
                            gst::debug!(
                                CAT,
                                "Creating new pad '{}' for key expression '{}'",
                                pad_name,
                                sample_key_expr
                            );

                            // Create + activate + add the pad, surfacing a GStreamer
                            // error instead of panicking the receive thread.
                            match Self::create_src_pad(&element, &pad_name, &sample_key_expr) {
                                Ok(pad) => {
                                    stats.lock().unwrap().pads_created += 1;
                                    pads_guard.insert(sample_key_expr.clone(), pad.clone());
                                    // A new pad resets the quiescence timer.
                                    last_pad_added = Some(std::time::Instant::now());
                                    if no_more_pads_emitted {
                                        gst::warning!(
                                            CAT,
                                            "New key expression '{}' appeared after no_more_pads()",
                                            sample_key_expr
                                        );
                                    }
                                    pad
                                }
                                Err(e) => {
                                    gst::error!(
                                        CAT,
                                        "Failed to create pad for '{}': {}",
                                        sample_key_expr,
                                        e
                                    );
                                    stats.lock().unwrap().errors += 1;
                                    gst::element_error!(
                                        element,
                                        gst::CoreError::Pad,
                                        [
                                            "Failed to create demux pad for '{}': {}",
                                            sample_key_expr,
                                            e
                                        ]
                                    );
                                    continue;
                                }
                            }
                        }
                    };

                    // Process the sample and create a buffer
                    let payload = sample.payload();
                    let data = payload.to_bytes();

                    // Check for metadata (caps, compression, etc.)
                    #[cfg(any(
                        feature = "compression-zstd",
                        feature = "compression-lz4",
                        feature = "compression-gzip"
                    ))]
                    let (final_data, metadata) = if let Some(attachment) = sample.attachment() {
                        match MetadataParser::parse(attachment) {
                            Ok(meta) => {
                                // Check for compression (first-class field).
                                match meta.compression().and_then(
                                    crate::compression::CompressionType::from_metadata_value,
                                ) {
                                    Some(comp_type) => {
                                        match crate::compression::decompress(&data, comp_type) {
                                            Ok(decompressed) => (decompressed, Some(meta)),
                                            Err(e) => {
                                                gst::warning!(CAT, "Decompression failed: {}", e);
                                                stats.lock().unwrap().errors += 1;
                                                continue;
                                            }
                                        }
                                    }
                                    None => (data.to_vec(), Some(meta)),
                                }
                            }
                            Err(e) => {
                                gst::warning!(CAT, "Failed to parse metadata: {}", e);
                                (data.to_vec(), None)
                            }
                        }
                    } else {
                        (data.to_vec(), None)
                    };

                    #[cfg(not(any(
                        feature = "compression-zstd",
                        feature = "compression-lz4",
                        feature = "compression-gzip"
                    )))]
                    let (final_data, metadata) = if let Some(attachment) = sample.attachment() {
                        match MetadataParser::parse(attachment) {
                            Ok(meta) => {
                                // No compression features in this build: if the payload
                                // is compressed, skip it with an error rather than push
                                // compressed bytes downstream as raw data.
                                if let Some(algo) = meta.compression()
                                    && algo != "none"
                                {
                                    gst::warning!(
                                        CAT,
                                        "Received '{}'-compressed payload but this build has no \
                                         compression features enabled; dropping sample",
                                        algo
                                    );
                                    stats.lock().unwrap().errors += 1;
                                    continue;
                                }
                                (data.to_vec(), Some(meta))
                            }
                            Err(e) => {
                                gst::warning!(CAT, "Failed to parse metadata: {}", e);
                                (data.to_vec(), None)
                            }
                        }
                    } else {
                        (data.to_vec(), None)
                    };

                    // Create buffer
                    let mut buffer = match gst::Buffer::with_size(final_data.len()) {
                        Ok(buf) => buf,
                        Err(_) => {
                            gst::warning!(CAT, "Failed to allocate buffer");
                            stats.lock().unwrap().errors += 1;
                            continue;
                        }
                    };

                    {
                        let buffer_ref = match buffer.get_mut() {
                            Some(b) => b,
                            None => {
                                gst::warning!(CAT, "Failed to get mutable buffer");
                                stats.lock().unwrap().errors += 1;
                                continue;
                            }
                        };

                        if buffer_ref.copy_from_slice(0, &final_data).is_err() {
                            gst::warning!(CAT, "Failed to copy data to buffer");
                            stats.lock().unwrap().errors += 1;
                            continue;
                        }

                        // Apply metadata to buffer
                        if let Some(ref meta) = metadata {
                            meta.apply_to_buffer(buffer_ref);

                            // Set caps if present
                            if let Some(caps) = meta.caps() {
                                // We can't set caps on the buffer directly, but we can
                                // set them on the pad if needed
                                if !pad.has_current_caps() {
                                    pad.push_event(gst::event::Caps::new(caps));
                                }
                            }
                        }
                    }

                    // Update statistics
                    {
                        let mut stats_guard = stats.lock().unwrap();
                        stats_guard.bytes_received += final_data.len() as u64;
                        stats_guard.messages_received += 1;
                    }

                    // Push buffer to the pad
                    match pad.push(buffer) {
                        Ok(_) => {}
                        Err(gst::FlowError::Flushing) => {
                            gst::debug!(CAT, "Pad {} is flushing", pad.name());
                        }
                        Err(e) => {
                            gst::warning!(
                                CAT,
                                "Failed to push buffer to pad {}: {:?}",
                                pad.name(),
                                e
                            );
                        }
                    }
                }
                Ok(None) => {
                    // Timeout - continue loop
                    continue;
                }
                Err(e) => {
                    // `recv_timeout` maps an expired timeout to `Ok(None)` (handled
                    // above), so an `Err` here is a genuine disconnect. Never classify
                    // by formatting the error string.
                    gst::warning!(CAT, "Subscriber disconnected: {}", e);
                    stats.lock().unwrap().errors += 1;
                    break;
                }
            }
        }

        gst::debug!(CAT, "Receiver loop finished");
    }
}
