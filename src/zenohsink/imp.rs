use std::sync::LazyLock;

use gst::glib;
use gst::subclass::prelude::*;
use gst_base::subclass::prelude::BaseSinkImpl;

#[derive(Default)]
pub struct ZenohSink {}

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
    fn properties() -> &'static [gst::glib::ParamSpec] {
        &[]
    }

    fn signals() -> &'static [gst::glib::subclass::Signal] {
        &[]
    }

    fn set_property(&self, _id: usize, _value: &gst::glib::Value, _pspec: &gst::glib::ParamSpec) {
        std::unimplemented!()
    }

    fn property(&self, _id: usize, _pspec: &gst::glib::ParamSpec) -> gst::glib::Value {
        std::unimplemented!()
    }

    fn constructed(&self) {
        self.parent_constructed();
    }

    fn dispose(&self) {}

    fn notify(&self, pspec: &gst::glib::ParamSpec) {
        self.parent_notify(pspec)
    }

    fn dispatch_properties_changed(&self, pspecs: &[gst::glib::ParamSpec]) {
        self.parent_dispatch_properties_changed(pspecs)
    }
}

#[glib::object_subclass]
impl ObjectSubclass for ZenohSink {
    const NAME: &'static str = "GstZenohSink";
    type Type = super::ZenohSink;
    type ParentType = gst_base::BaseSink;
}

impl BaseSinkImpl for ZenohSink {}
