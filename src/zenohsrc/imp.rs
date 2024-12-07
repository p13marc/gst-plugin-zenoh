use std::sync::LazyLock;

use gst::{glib, subclass::prelude::*};
use gst_base::subclass::prelude::{BaseSrcImpl, PushSrcImpl};

#[derive(Default)]
pub struct ZenohSrc {}

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
        &[]
    }

    fn signals() -> &'static [glib::subclass::Signal] {
        &[]
    }

    fn set_property(&self, _id: usize, _value: &glib::Value, _pspec: &glib::ParamSpec) {
        std::unimplemented!()
    }

    fn property(&self, _id: usize, _pspec: &glib::ParamSpec) -> glib::Value {
        std::unimplemented!()
    }

    fn constructed(&self) {
        self.parent_constructed();
    }

    fn dispose(&self) {}

    fn notify(&self, pspec: &glib::ParamSpec) {
        self.parent_notify(pspec)
    }

    fn dispatch_properties_changed(&self, pspecs: &[glib::ParamSpec]) {
        self.parent_dispatch_properties_changed(pspecs)
    }
}

#[glib::object_subclass]
impl ObjectSubclass for ZenohSrc {
    const NAME: &'static str = "GstZenohSrc";
    type Type = super::ZenohSrc;
    type ParentType = gst_base::PushSrc;
}

impl BaseSrcImpl for ZenohSrc {}

impl PushSrcImpl for ZenohSrc {
    fn fill(&self, buffer: &mut gst::BufferRef) -> Result<gst::FlowSuccess, gst::FlowError> {
        gst_base::subclass::prelude::PushSrcImplExt::parent_fill(self, buffer)
    }

    fn alloc(&self) -> Result<gst::Buffer, gst::FlowError> {
        gst_base::subclass::prelude::PushSrcImplExt::parent_alloc(self)
    }

    fn create(
        &self,
        buffer: Option<&mut gst::BufferRef>,
    ) -> Result<gst_base::subclass::base_src::CreateSuccess, gst::FlowError> {
        gst_base::subclass::prelude::PushSrcImplExt::parent_create(self, buffer)
    }
}
