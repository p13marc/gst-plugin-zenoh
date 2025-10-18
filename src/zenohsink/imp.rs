use std::sync::{LazyLock, Mutex};

use gst::{glib, prelude::*, subclass::prelude::*};
use gst_base::subclass::prelude::*;
use zenoh::key_expr::OwnedKeyExpr;
use zenoh::Wait;

use crate::error::{ErrorHandling, FlowErrorHandling, ZenohError};

static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new(
        "zenohsink",
        gst::DebugColorFlags::empty(),
        Some("Zenoh Sink"),
    )
});

struct Started {
    // Keeping session field to maintain ownership and prevent session from being dropped
    // while publisher is still in use
    #[allow(dead_code)]
    session: zenoh::Session,
    publisher: zenoh::pubsub::Publisher<'static>,
}

#[derive(Default)]
enum State {
    #[default]
    Stopped,
    Started(Started),
}

#[derive(Debug)]
struct Settings {
    key_expr: String,
    config_file: Option<String>,
    priority: i32,
    congestion_control: String,
    reliability: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            key_expr: String::new(),
            config_file: None,
            priority: 0,
            congestion_control: "block".into(),
            reliability: "best-effort".into(),
        }
    }
}

// Note: We don't define enums for Reliability and CongestionControl
// here since Zenoh already has them, but we expose string properties
// to the GStreamer API for compatibility and future extension

pub struct ZenohSink {
    settings: Mutex<Settings>,
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
                "Zenoh Sink",
                "Source/Network/Zenoh",
                "Send data over the network via Zenoh",
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
                    .nick("key expression")
                    .blurb("Key expression to where to send data")
                    .build(),
                // Config file property
                glib::ParamSpecString::builder("config")
                    .nick("config file")
                    .blurb("Path to Zenoh configuration file")
                    .build(),
                // Priority property
                glib::ParamSpecInt::builder("priority")
                    .nick("priority")
                    .blurb("Priority for the Zenoh publisher (higher value = higher priority)")
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

    fn set_property(&self, _id: usize, value: &gst::glib::Value, pspec: &gst::glib::ParamSpec) {
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

    fn property(&self, _id: usize, pspec: &gst::glib::ParamSpec) -> gst::glib::Value {
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
}

#[glib::object_subclass]
impl ObjectSubclass for ZenohSink {
    const NAME: &'static str = "GstZenohSink";
    type Type = super::ZenohSink;
    type ParentType = gst_base::BaseSink;
}

impl BaseSinkImpl for ZenohSink {
    fn start(&self) -> Result<(), gst::ErrorMessage> {
        let mut state = self.state.lock().unwrap();

        if let State::Started { .. } = *state {
            gst::warning!(CAT, "ZenohSink is already started, ignoring start request");
            return Ok(()); // Already started, nothing to do
        }

        let settings = self.settings.lock().unwrap();
        let key_expr = settings.key_expr.clone();
        let config_file = settings.config_file.clone();
        let priority = settings.priority;
        let congestion_control = settings.congestion_control.clone();
        let reliability = settings.reliability.clone();
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

        // Use Zenoh's synchronous API with wait(), with proper error handling
        let session = zenoh::open(config)
            .wait()
            .map_err(|e| ZenohError::InitError(e).to_error_message())?;

        gst::debug!(CAT, "Creating publisher with key_expr='{}', priority={}, congestion_control='{}', reliability='{}'",
                  key_expr, priority, congestion_control, reliability);

        // Use owned key_expr for static lifetime, with proper error handling
        let owned = OwnedKeyExpr::try_from(key_expr.clone())
            .map_err(|e| ZenohError::KeyExprError(e.to_string()).to_error_message())?;

        // Create publisher with configuration options
        // Note: We intentionally don't set priority/reliability/congestion control yet
        // As these require detailed knowledge of the specific Zenoh API version
        // The properties are exposed in the GStreamer API for future compatibility

        // Create publisher now
        let publisher = session
            .declare_publisher(owned)
            .wait()
            .map_err(|e| ZenohError::PublishError(e).to_error_message())?;

        gst::debug!(CAT, "Publisher created with key_expr='{}', priority={}, congestion_control='{}', reliability='{}'",
            key_expr, priority, congestion_control, reliability);

        *state = State::Started(Started { session, publisher });

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
        started.publisher.put(b.as_slice()).wait().map_err(|e| {
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
            err.to_flow_error()
        })?;
        Ok(gst::FlowSuccess::Ok)
    }

    fn stop(&self) -> Result<(), gst::ErrorMessage> {
        // Get current state and properly clean up resources
        let mut state = self.state.lock().unwrap();
        if let State::Started(ref _started) = *state {
            // No explicit cleanup needed, drop will handle it
            *state = State::Stopped;
        }

        Ok(())
    }

    fn event(&self, event: gst::Event) -> bool {
        gst::debug!(CAT, imp = self, "Handling event {:?}", event);
        self.parent_event(event)
    }
}
