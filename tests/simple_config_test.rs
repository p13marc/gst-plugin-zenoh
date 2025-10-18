use gst::prelude::*;

#[test]
fn test_zenoh_sink_properties() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    // Create a zenohsink element
    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink element");

    // Test default values
    assert_eq!(sink.property::<String>("key-expr"), "");
    assert_eq!(sink.property::<i32>("priority"), 0);
    assert_eq!(sink.property::<String>("congestion-control"), "block");
    assert_eq!(sink.property::<String>("reliability"), "best-effort");
    assert_eq!(sink.property::<bool>("express"), false);

    // Test setting properties
    sink.set_property("key-expr", "test/config");
    sink.set_property("priority", 50i32);
    sink.set_property("congestion-control", "drop");
    sink.set_property("reliability", "reliable");
    sink.set_property("express", true);

    // Verify properties were set
    assert_eq!(sink.property::<String>("key-expr"), "test/config");
    assert_eq!(sink.property::<i32>("priority"), 50);
    assert_eq!(sink.property::<String>("congestion-control"), "drop");
    assert_eq!(sink.property::<String>("reliability"), "reliable");
    assert_eq!(sink.property::<bool>("express"), true);
}

#[test]
fn test_zenoh_sink_invalid_properties() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink element");

    // Set a valid reliability first
    sink.set_property("reliability", "reliable");
    assert_eq!(sink.property::<String>("reliability"), "reliable");

    // Try to set invalid reliability (should be ignored and keep previous value)
    sink.set_property("reliability", "invalid");
    assert_eq!(sink.property::<String>("reliability"), "reliable");

    // Set a valid congestion control first
    sink.set_property("congestion-control", "drop");
    assert_eq!(sink.property::<String>("congestion-control"), "drop");

    // Try to set invalid congestion control (should be ignored)
    sink.set_property("congestion-control", "invalid");
    assert_eq!(sink.property::<String>("congestion-control"), "drop");
}

#[test]
fn test_zenoh_src_properties() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    // Create a zenohsrc element
    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc element");

    // Test default values
    assert_eq!(src.property::<String>("key-expr"), "");
    assert_eq!(src.property::<i32>("priority"), 0);
    assert_eq!(src.property::<String>("congestion-control"), "block");
    assert_eq!(src.property::<String>("reliability"), "best-effort");

    // Test setting properties
    src.set_property("key-expr", "test/src/config");
    src.set_property("priority", -20i32);
    src.set_property("congestion-control", "drop");
    src.set_property("reliability", "reliable");

    // Verify properties were set
    assert_eq!(src.property::<String>("key-expr"), "test/src/config");
    assert_eq!(src.property::<i32>("priority"), -20);
    assert_eq!(src.property::<String>("congestion-control"), "drop");
    assert_eq!(src.property::<String>("reliability"), "reliable");
}

#[test]
fn test_priority_boundaries() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink element");

    // Test minimum priority
    sink.set_property("priority", -100i32);
    assert_eq!(sink.property::<i32>("priority"), -100);

    // Test maximum priority
    sink.set_property("priority", 100i32);
    assert_eq!(sink.property::<i32>("priority"), 100);

    // Test normal priority
    sink.set_property("priority", 0i32);
    assert_eq!(sink.property::<i32>("priority"), 0);
}