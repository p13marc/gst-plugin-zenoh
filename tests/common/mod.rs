// Common test utilities for gst-plugin-zenoh tests

use std::sync::Once;

pub mod helpers;

pub use helpers::*;

static INIT: Once = Once::new();

/// Initialize GStreamer and register the plugin for tests.
///
/// This function is idempotent and can be called multiple times safely.
/// It ensures GStreamer is initialized and the zenoh plugin is registered
/// exactly once per test process.
pub fn init() {
    INIT.call_once(|| {
        gst::init().unwrap();
        gstzenoh::plugin_register_static().expect("Failed to register plugin");
    });
}
