use gst::prelude::*;
use serial_test::serial;

fn init() {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        gst::init().unwrap();
        gstzenoh::plugin_register_static().expect("Failed to register plugin");
    });
}

#[test]
#[serial]
fn test_zenohdemux_creation() {
    init();

    let demux = gst::ElementFactory::make("zenohdemux")
        .build()
        .expect("Failed to create zenohdemux");

    // Name should start with "zenohdemux" followed by a number
    assert!(
        demux.name().as_str().starts_with("zenohdemux"),
        "Element name should start with 'zenohdemux'"
    );
}

#[test]
#[serial]
fn test_zenohdemux_properties() {
    init();

    let demux = gst::ElementFactory::make("zenohdemux")
        .build()
        .expect("Failed to create zenohdemux");

    // Test key-expr property
    demux.set_property("key-expr", "test/demux/*");
    let key_expr: String = demux.property("key-expr");
    assert_eq!(key_expr, "test/demux/*");

    // Test config property
    demux.set_property("config", Some("/path/to/config.json5"));
    let config: Option<String> = demux.property("config");
    assert_eq!(config, Some("/path/to/config.json5".to_string()));

    // Test receive-timeout-ms property
    demux.set_property("receive-timeout-ms", 250u64);
    let timeout: u64 = demux.property("receive-timeout-ms");
    assert_eq!(timeout, 250);
}

#[test]
#[serial]
fn test_zenohdemux_pad_naming_property() {
    init();

    let demux = gst::ElementFactory::make("zenohdemux")
        .build()
        .expect("Failed to create zenohdemux");

    // Default should be "full-path"
    // We can't easily test enum values without importing the type,
    // but we can verify the property exists and can be set

    // Set to different values using string nick
    demux.set_property_from_str("pad-naming", "last-segment");
    demux.set_property_from_str("pad-naming", "hash");
    demux.set_property_from_str("pad-naming", "full-path");
}

#[test]
#[serial]
fn test_zenohdemux_statistics_initial_values() {
    init();

    let demux = gst::ElementFactory::make("zenohdemux")
        .build()
        .expect("Failed to create zenohdemux");

    // Before starting, statistics should be 0
    let bytes_received: u64 = demux.property("bytes-received");
    let messages_received: u64 = demux.property("messages-received");
    let pads_created: u64 = demux.property("pads-created");

    assert_eq!(bytes_received, 0);
    assert_eq!(messages_received, 0);
    assert_eq!(pads_created, 0);
}

#[test]
#[serial]
fn test_zenohdemux_requires_key_expr() {
    init();

    let demux = gst::ElementFactory::make("zenohdemux")
        .build()
        .expect("Failed to create zenohdemux");

    // Without setting key-expr, state change should fail
    let result = demux.set_state(gst::State::Ready);

    // Should fail because key-expr is required
    assert!(result.is_err(), "State change should fail without key-expr");

    // Clean up
    let _ = demux.set_state(gst::State::Null);
}

#[test]
#[serial]
fn test_zenohdemux_pad_template() {
    init();

    let demux = gst::ElementFactory::make("zenohdemux")
        .build()
        .expect("Failed to create zenohdemux");

    // Check that the element has the expected pad template
    let factory = demux.factory().unwrap();
    let templates = factory.static_pad_templates();

    // Should have one template for dynamic src pads
    assert!(
        !templates.is_empty(),
        "Should have at least one pad template"
    );

    // Find the src template
    let src_template = templates
        .iter()
        .find(|t| t.direction() == gst::PadDirection::Src);

    assert!(src_template.is_some(), "Should have a src pad template");

    let template = src_template.unwrap();
    assert_eq!(template.presence(), gst::PadPresence::Sometimes);
}

#[test]
#[serial]
fn test_zenohdemux_element_metadata() {
    init();

    let factory = gst::ElementFactory::find("zenohdemux").expect("zenohdemux factory not found");

    let long_name = factory.metadata("long-name");
    assert!(long_name.is_some(), "Should have long-name metadata");

    // Check for "Demux" or "Demultiplexer" in the name
    let name = long_name.unwrap();
    assert!(
        name.contains("Demux") || name.contains("demux") || name.contains("Demultiplex"),
        "Long name '{}' should contain 'Demux' or similar",
        name
    );
}
