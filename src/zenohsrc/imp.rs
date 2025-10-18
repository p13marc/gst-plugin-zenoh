use std::sync::{Arc, LazyLock, Mutex};

use gst::{glib, prelude::*, subclass::prelude::*};
use gst_base::{
    prelude::BaseSrcExt,
    subclass::{base_src::CreateSuccess, prelude::*},
};
use zenoh::Wait;

use crate::error::{ErrorHandling, FlowErrorHandling, ZenohError};

// Define debug category for logging
#[allow(dead_code)]
static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new("zenohsrc", gst::DebugColorFlags::empty(), Some("Zenoh Src"))
});

struct Started {
    // Keeping session field to maintain ownership and prevent session from being dropped
    // while subscriber is still in use. This can be either owned or shared.
    #[allow(dead_code)]
    session: SessionWrapper,
    subscriber:
        zenoh::pubsub::Subscriber<zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>>,
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
    /// Subscriber priority (-100 to 100, higher = more priority)
    priority: i32,
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
            priority: 0,
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
                "Zenoh Source",
                "Source/Network/Zenoh",
                "Receive data over the network via Zenoh",
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
                    .nick("key expression")
                    .blurb("Key expression to where to receive data from")
                    .build(),
                    
                // Config file property
                glib::ParamSpecString::builder("config")
                    .nick("config file")
                    .blurb("Path to Zenoh configuration file")
                    .build(),
                    
                // Priority property
                glib::ParamSpecInt::builder("priority")
                    .nick("priority")
                    .blurb("Priority for the Zenoh subscriber (higher value = higher priority)")
                    .default_value(0)
                    .minimum(-100)
                    .maximum(100)
                    .build(),
                    
                // Congestion control property
                glib::ParamSpecString::builder("congestion-control")
                    .nick("congestion control")
                    .blurb("Congestion control policy (block or drop)")
                    .default_value(Some("block"))
                    .build(),
                    
                // Reliability property
                glib::ParamSpecString::builder("reliability")
                    .nick("reliability")
                    .blurb("Reliability mode (best-effort or reliable)")
                    .default_value(Some("best-effort"))
                    .build(),
            ]
        });

        PROPERTIES.as_ref()
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        // Check if we're in a state where property changes are allowed
        let state = self.state.lock().unwrap();
        if state.is_started() && matches!(pspec.name(), "key-expr" | "config" | "reliability" | "congestion-control" | "priority") {
            gst::warning!(CAT, "Cannot change property '{}' while element is started", pspec.name());
            return;
        }
        drop(state);
        
        let mut settings = self.settings.lock().unwrap();

        match pspec.name() {
            "key-expr" => {
                settings.key_expr = value.get::<String>().expect("type checked upstream");
            }
            "config" => {
                settings.config_file = value.get::<Option<String>>().expect("type checked upstream");
            }
            "priority" => {
                settings.priority = value.get::<i32>().expect("type checked upstream");
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
        let settings = self.settings.lock().unwrap();

        match pspec.name() {
            "key-expr" => settings.key_expr.to_value(),
            "config" => settings.config_file.to_value(),
            "priority" => settings.priority.to_value(),
            "congestion-control" => settings.congestion_control.to_value(),
            "reliability" => settings.reliability.to_value(),
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
            gst::warning!(CAT, "Cannot start ZenohSrc from state: {}, ignoring start request", current_state);
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
            return Err(gst::error_msg!(gst::ResourceError::Settings, ["Key expression is required"]));
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
        let subscriber = session_wrapper.as_session()
            .declare_subscriber(key_expr)
            .wait()
            .map_err(|e| ZenohError::InitError(e).to_error_message())?;

        // Reacquire state lock to complete transition
        let mut state = self.state.lock().unwrap();
        
        // Verify we're still in Starting state (not stopped during initialization)
        if !matches!(*state, State::Starting) {
            gst::warning!(CAT, "State changed during startup, aborting start operation");
            return Err(gst::error_msg!(
                gst::ResourceError::Settings,
                ["State changed during startup"]
            ));
        }
        
        *state = State::Started(Started {
            session: session_wrapper,
            subscriber,
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
            gst::warning!(CAT, "Stopping ZenohSrc from non-started state: {}", current_state);
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

        // Use Zenoh's synchronous API with proper error handling
        let sample = started.subscriber.recv()
            .map_err(|e| {
                let err = ZenohError::ReceiveError(e.to_string());
                // Check if this is a network-related error
                let error_msg = e.to_string();
                if error_msg.contains("timeout") || error_msg.contains("connection") || error_msg.contains("network") {
                    gst::element_imp_error!(self, gst::ResourceError::Read, 
                        ["Network error while receiving: {}", err]);
                } else {
                    gst::element_imp_error!(self, gst::ResourceError::Read, ["{}", err]);
                }
                err.to_flow_error()
            })?;
        
        let payload = sample.payload();
        let slice = payload.to_bytes();

        let mut buffer = gst::Buffer::with_size(slice.len())
            .map_err(|_| {
                gst::element_imp_error!(self, gst::ResourceError::Failed, ["Failed to allocate buffer"]);
                gst::FlowError::Error
            })?;
            
        buffer.get_mut()
            .ok_or_else(|| {
                gst::element_imp_error!(self, gst::ResourceError::Failed, ["Failed to get mutable buffer reference"]);
                gst::FlowError::Error
            })?
            .copy_from_slice(0, &slice)
            .map_err(|_| {
                gst::element_imp_error!(self, gst::ResourceError::Failed, ["Failed to copy data to buffer"]);
                gst::FlowError::Error
            })?;

        Ok(CreateSuccess::NewBuffer(buffer))
    }
}
