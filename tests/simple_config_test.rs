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
    assert_eq!(sink.property::<u32>("priority"), 5); // Default Priority::Data
    assert_eq!(sink.property::<String>("congestion-control"), "block");
    assert_eq!(sink.property::<String>("reliability"), "best-effort");
    assert_eq!(sink.property::<bool>("express"), false);

    // Test setting properties
    sink.set_property("key-expr", "test/config");
    sink.set_property("priority", 1u32); // RealTime priority
    sink.set_property("congestion-control", "drop");
    sink.set_property("reliability", "reliable");
    sink.set_property("express", true);

    // Verify properties were set
    assert_eq!(sink.property::<String>("key-expr"), "test/config");
    assert_eq!(sink.property::<u32>("priority"), 1);
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
    assert_eq!(src.property::<u32>("priority"), 5); // Default Priority::Data
    assert_eq!(src.property::<String>("congestion-control"), "block");
    assert_eq!(src.property::<String>("reliability"), "best-effort");

    // Test setting properties
    src.set_property("key-expr", "test/src/config");
    src.set_property("priority", 3u32); // InteractiveLow
    src.set_property("congestion-control", "drop");
    src.set_property("reliability", "reliable");

    // Verify properties were set
    assert_eq!(src.property::<String>("key-expr"), "test/src/config");
    assert_eq!(src.property::<u32>("priority"), 3);
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

    // Test RealTime priority (highest priority = 1)
    sink.set_property("priority", 1u32);
    assert_eq!(sink.property::<u32>("priority"), 1);

    // Test Background priority (lowest priority = 7)
    sink.set_property("priority", 7u32);
    assert_eq!(sink.property::<u32>("priority"), 7);

    // Test default Data priority
    sink.set_property("priority", 5u32);
    assert_eq!(sink.property::<u32>("priority"), 5);

    // Test InteractiveHigh priority
    sink.set_property("priority", 2u32);
    assert_eq!(sink.property::<u32>("priority"), 2);

    // Test DataLow priority
    sink.set_property("priority", 6u32);
    assert_eq!(sink.property::<u32>("priority"), 6);
}

#[test]
fn test_priority_enum_validation() {
    gst::init().unwrap();
    gstzenoh::plugin_register_static().unwrap();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink element");

    // Test all valid Zenoh Priority enum values
    let priorities = [
        (1u32, "RealTime"),
        (2u32, "InteractiveHigh"),
        (3u32, "InteractiveLow"),
        (4u32, "DataHigh"),
        (5u32, "Data"),
        (6u32, "DataLow"),
        (7u32, "Background"),
    ];

    for (value, name) in priorities {
        sink.set_property("priority", value);
        assert_eq!(sink.property::<u32>("priority"), value, "Failed to set {} priority ({})", name, value);
    }

    // Note: GStreamer's property system rejects invalid values (0, 8) before they reach our validation,
    // which is the correct behavior. Invalid values are simply not accepted.
}
