use gst::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn test_zenoh_sink_reliability_configuration() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    // Create element directly without harness to avoid state transitions
    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink element");

    // Test best-effort reliability (default)
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

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink element");

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

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink element");

    sink.set_property("key-expr", "test/sink/express");

    // Test express mode disabled (default)
    assert!(!sink.property::<bool>("express"));

    // Test express mode enabled
    sink.set_property("express", true);
    assert!(sink.property::<bool>("express"));
}

#[test]
#[serial]
fn test_zenoh_sink_priority_configuration() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink element");

    sink.set_property("key-expr", "test/sink/priority");

    // Test default priority (Data = 5)
    assert_eq!(sink.property::<u32>("priority"), 5);

    // Test high priority (RealTime = 1)
    sink.set_property("priority", 1u32);
    assert_eq!(sink.property::<u32>("priority"), 1);

    // Test low priority (Background = 7)
    sink.set_property("priority", 7u32);
    assert_eq!(sink.property::<u32>("priority"), 7);

    // Test InteractiveHigh priority
    sink.set_property("priority", 2u32);
    assert_eq!(sink.property::<u32>("priority"), 2);

    // Test DataLow priority
    sink.set_property("priority", 6u32);
    assert_eq!(sink.property::<u32>("priority"), 6);
}

#[test]
#[serial]
fn test_zenoh_src_reliability_configuration() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc element");

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

    // Note: Currently we don't have a direct way to pass the Arc<Session> via GStreamer properties
    // This would require a custom property type or a session registry mechanism
    // For now, this test demonstrates the concept and can be expanded when the full API is implemented

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink element");

    sink.set_property("key-expr", "test/sink/shared");
    sink.set_property("reliability", "reliable");
    sink.set_property("express", true);

    // The element should be configurable even without external session
    assert_eq!(sink.property::<String>("reliability"), "reliable");
    assert!(sink.property::<bool>("express"));
}

#[test]
#[serial]
fn test_end_to_end_configuration() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink element");

    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc element");

    // Configure both elements with matching reliability
    let key_expr = "test/e2e/reliable";
    sink.set_property("key-expr", key_expr);
    sink.set_property("reliability", "reliable");
    sink.set_property("congestion-control", "block");
    sink.set_property("express", true);
    sink.set_property("priority", 2u32); // InteractiveHigh priority

    src.set_property("key-expr", key_expr);
    src.set_property("reliability", "reliable");

    // Verify properties are set correctly
    assert_eq!(sink.property::<String>("reliability"), "reliable");
    assert_eq!(sink.property::<String>("congestion-control"), "block");
    assert!(sink.property::<bool>("express"));
    assert_eq!(sink.property::<u32>("priority"), 2);
    assert_eq!(src.property::<String>("reliability"), "reliable");
}
