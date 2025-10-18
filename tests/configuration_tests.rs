use gst::prelude::*;
use gst_check::Harness;
use serial_test::serial;

#[test]
#[serial]
fn test_zenoh_sink_reliability_configuration() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let mut sink_harness = Harness::new("zenohsink");
    sink_harness.set_src_caps_str("application/octet-stream");

    // Test best-effort reliability (default)
    let sink = sink_harness.element().unwrap();
    sink.set_property("key-expr", "test/sink/best-effort");
    sink.set_property("reliability", "best-effort");
    assert_eq!(sink.property::<String>("reliability"), "best-effort");

    // Test reliable reliability
    sink.set_property("reliability", "reliable");
    assert_eq!(sink.property::<String>("reliability"), "reliable");

    // Test invalid reliability (should keep previous valid value)
    sink.set_property("reliability", "invalid");
    assert_eq!(sink.property::<String>("reliability"), "reliable");
}

#[test]
#[serial]
fn test_zenoh_sink_congestion_control_configuration() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let mut sink_harness = Harness::new("zenohsink");
    sink_harness.set_src_caps_str("application/octet-stream");

    let sink = sink_harness.element().unwrap();
    sink.set_property("key-expr", "test/sink/congestion");

    // Test block congestion control (default)
    sink.set_property("congestion-control", "block");
    assert_eq!(sink.property::<String>("congestion-control"), "block");

    // Test drop congestion control
    sink.set_property("congestion-control", "drop");
    assert_eq!(sink.property::<String>("congestion-control"), "drop");

    // Test invalid congestion control (should keep previous valid value)
    sink.set_property("congestion-control", "invalid");
    assert_eq!(sink.property::<String>("congestion-control"), "drop");
}

#[test]
#[serial]
fn test_zenoh_sink_express_mode() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let mut sink_harness = Harness::new("zenohsink");
    sink_harness.set_src_caps_str("application/octet-stream");

    let sink = sink_harness.element().unwrap();
    sink.set_property("key-expr", "test/sink/express");

    // Test express mode disabled (default)
    assert_eq!(sink.property::<bool>("express"), false);

    // Test express mode enabled
    sink.set_property("express", true);
    assert_eq!(sink.property::<bool>("express"), true);
}

#[test]
#[serial]
fn test_zenoh_sink_priority_configuration() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let mut sink_harness = Harness::new("zenohsink");
    sink_harness.set_src_caps_str("application/octet-stream");

    let sink = sink_harness.element().unwrap();
    sink.set_property("key-expr", "test/sink/priority");

    // Test default priority
    assert_eq!(sink.property::<i32>("priority"), 0);

    // Test high priority
    sink.set_property("priority", 50);
    assert_eq!(sink.property::<i32>("priority"), 50);

    // Test low priority
    sink.set_property("priority", -50);
    assert_eq!(sink.property::<i32>("priority"), -50);

    // Test boundary values
    sink.set_property("priority", 100);
    assert_eq!(sink.property::<i32>("priority"), 100);

    sink.set_property("priority", -100);
    assert_eq!(sink.property::<i32>("priority"), -100);
}

#[test]
#[serial]
fn test_zenoh_src_reliability_configuration() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let src_harness = Harness::new("zenohsrc");

    let src = src_harness.element().unwrap();
    src.set_property("key-expr", "test/src/best-effort");

    // Test best-effort reliability (default)
    src.set_property("reliability", "best-effort");
    assert_eq!(src.property::<String>("reliability"), "best-effort");

    // Test reliable reliability
    src.set_property("reliability", "reliable");
    assert_eq!(src.property::<String>("reliability"), "reliable");

    // Test invalid reliability (should keep previous valid value)
    src.set_property("reliability", "invalid");
    assert_eq!(src.property::<String>("reliability"), "reliable");
}

// Note: Property locking test requires actual Zenoh session which is complex to set up in unit tests

#[test]
#[serial]
fn test_shared_session_functionality() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let mut sink_harness = Harness::new("zenohsink");
    sink_harness.set_src_caps_str("application/octet-stream");

    // Note: Currently we don't have a direct way to pass the Arc<Session> via GStreamer properties
    // This would require a custom property type or a session registry mechanism
    // For now, this test demonstrates the concept and can be expanded when the full API is implemented

    let sink = sink_harness.element().unwrap();
    sink.set_property("key-expr", "test/sink/shared");
    sink.set_property("reliability", "reliable");
    sink.set_property("express", true);
    
    // The element should be configurable even without external session
    assert_eq!(sink.property::<String>("reliability"), "reliable");
    assert_eq!(sink.property::<bool>("express"), true);
}

#[test]
#[serial]
fn test_end_to_end_configuration() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let mut sink_harness = Harness::new("zenohsink");
    sink_harness.set_src_caps_str("application/octet-stream");

    let src_harness = Harness::new("zenohsrc");

    let sink = sink_harness.element().unwrap();
    let src = src_harness.element().unwrap();

    // Configure both elements with matching reliability
    let key_expr = "test/e2e/reliable";
    sink.set_property("key-expr", key_expr);
    sink.set_property("reliability", "reliable");
    sink.set_property("congestion-control", "block");
    sink.set_property("express", true);
    sink.set_property("priority", 10);

    src.set_property("key-expr", key_expr);
    src.set_property("reliability", "reliable");

    // Verify properties are set correctly
    assert_eq!(sink.property::<String>("reliability"), "reliable");
    assert_eq!(sink.property::<String>("congestion-control"), "block");
    assert_eq!(sink.property::<bool>("express"), true);
    assert_eq!(sink.property::<i32>("priority"), 10);
    assert_eq!(src.property::<String>("reliability"), "reliable");
}