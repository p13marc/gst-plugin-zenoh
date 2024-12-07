use gst::glib;
use gst::prelude::*;

pub mod imp;

glib::wrapper! {
    pub struct ZenohSrc(ObjectSubclass<imp::ZenohSrc>) @extends gst_base::PushSrc, gst_base::BaseSrc, gst::Element, gst::Object;
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "zenohsrc",
        gst::Rank::MARGINAL,
        ZenohSrc::static_type(),
    )
}
