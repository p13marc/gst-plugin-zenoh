use std::sync::{LazyLock, Mutex};

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
    // while subscriber is still in use
    #[allow(dead_code)]
    session: zenoh::Session,
    subscriber:
        zenoh::pubsub::Subscriber<zenoh::handlers::FifoChannelHandler<zenoh::sample::Sample>>,
}

#[derive(Default)]
enum State {
    #[default]
    Stopped,
    Started(Started),
}

#[derive(Debug, Default)]
struct Settings {
    key_expr: String,
}

#[derive(Default)]
pub struct ZenohSrc {
    settings: Mutex<Settings>,
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
            vec![glib::ParamSpecString::builder("key-expr")
                .nick("key expression")
                .blurb("Key expression to where to received data")
                .build()]
        });

        PROPERTIES.as_ref()
    }

    fn set_property(&self, _id: usize, value: &glib::Value, pspec: &glib::ParamSpec) {
        let mut settings = self.settings.lock().unwrap();

        match pspec.name() {
            "key-expr" => {
                settings.key_expr = value.get::<String>().expect("type checked upstream");
            }
            _ => unimplemented!(),
        }
    }

    fn property(&self, _id: usize, pspec: &glib::ParamSpec) -> glib::Value {
        let settings = self.settings.lock().unwrap();

        match pspec.name() {
            "key-expr" => settings.key_expr.to_value(),
            _ => unimplemented!(),
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

        if let State::Started { .. } = *state {
            unreachable!("ZenohSrc is already started");
        }

        let settings = self.settings.lock().unwrap();
        let key_expr = settings.key_expr.clone();
        drop(settings);

        // Use Zenoh's synchronous API with wait(), with proper error handling
        let session = zenoh::open(zenoh::Config::default())
            .wait()
            .map_err(|e| ZenohError::InitError(e).to_error_message())?;
            
        let subscriber = session.declare_subscriber(key_expr)
            .wait()
            .map_err(|e| ZenohError::InitError(e).to_error_message())?;

        *state = State::Started(Started {
            session,
            subscriber,
        });

        // Commented code removed - using wait() instead of async

        Ok(())
    }
    fn stop(&self) -> Result<(), gst::ErrorMessage> {
        *self.state.lock().unwrap() = State::Stopped;

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
                gst::element_imp_error!(self, gst::ResourceError::Read, ["{}", err]);
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
