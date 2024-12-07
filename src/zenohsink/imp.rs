use std::sync::{LazyLock, Mutex};

use gst::{glib, prelude::*, subclass::prelude::*};
use gst_base::subclass::prelude::*;
use tokio::sync::mpsc::{self, Sender};

use crate::utils::RUNTIME;

static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new(
        "zenohsink",
        gst::DebugColorFlags::empty(),
        Some("Zenoh Sink"),
    )
});

struct Started {
    sender: Sender<gst::Buffer>,
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
            vec![glib::ParamSpecString::builder("key-expr")
                .nick("key expression")
                .blurb("Key expression to where to send data")
                .build()]
        });

        PROPERTIES.as_ref()
    }

    fn set_property(&self, _id: usize, value: &gst::glib::Value, pspec: &gst::glib::ParamSpec) {
        let mut settings = self.settings.lock().unwrap();

        match pspec.name() {
            "key-expr" => {
                settings.key_expr = value.get::<String>().expect("type checked upstream");
            }
            _ => unimplemented!(),
        }
    }

    fn property(&self, _id: usize, pspec: &gst::glib::ParamSpec) -> gst::glib::Value {
        let settings = self.settings.lock().unwrap();

        match pspec.name() {
            "key-expr" => settings.key_expr.to_value(),
            _ => unimplemented!(),
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
            unreachable!("ZenohSink is already started");
        }

        let settings = self.settings.lock().unwrap();
        let key_expr = settings.key_expr.clone();
        drop(settings);

        let (tx, mut rx) = mpsc::channel(32);

        *state = State::Started(Started { sender: tx });

        RUNTIME.spawn(async move {
            let session = zenoh::open(zenoh::Config::default()).await.unwrap();
            let publisher = session.declare_publisher(&key_expr).await.unwrap();
            while let Some(buffer) = rx.recv().await {
                let b = buffer.into_mapped_buffer_readable().unwrap();
                publisher.put(b.as_slice()).await.unwrap();
            }
        });

        Ok(())
    }

    fn render(&self, buffer: &gst::Buffer) -> Result<gst::FlowSuccess, gst::FlowError> {
        let state_locked = self.state.lock().unwrap();
        let State::Started(ref started) = *state_locked else {
            gst::element_imp_error!(self, gst::CoreError::Failed, ["Not started yet"]);
            return Err(gst::FlowError::Error);
        };

        started.sender.blocking_send(buffer.clone()).unwrap();
        Ok(gst::FlowSuccess::Ok)
    }

    fn stop(&self) -> Result<(), gst::ErrorMessage> {
        *self.state.lock().unwrap() = State::Stopped;

        Ok(())
    }

    fn event(&self, event: gst::Event) -> bool {
        gst::debug!(CAT, imp = self, "Handling event {:?}", event);
        self.parent_event(event)
    }
}
