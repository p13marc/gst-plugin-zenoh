use gst::glib;
use gst::prelude::*;

pub mod imp;

glib::wrapper! {
    pub struct ZenohSink(ObjectSubclass<imp::ZenohSink>) @extends gst_base::BaseSink, gst::Element, gst::Object;
}

pub fn register(plugin: &gst::Plugin) -> Result<(), glib::BoolError> {
    gst::Element::register(
        Some(plugin),
        "zenohsink",
        gst::Rank::MARGINAL,
        ZenohSink::static_type(),
    )
}
