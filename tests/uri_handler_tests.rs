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
fn test_zenohsink_uri_handler_interface() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    // Verify element implements URIHandler
    assert!(
        sink.is::<gst::URIHandler>(),
        "zenohsink should implement URIHandler"
    );
}

#[test]
#[serial]
fn test_zenohsrc_uri_handler_interface() {
    init();

    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc");

    // Verify element implements URIHandler
    assert!(
        src.is::<gst::URIHandler>(),
        "zenohsrc should implement URIHandler"
    );
}

#[test]
#[serial]
fn test_zenohsink_simple_uri() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Set a simple URI
    uri_handler.set_uri("zenoh:demo/video/stream").unwrap();

    // Verify the key-expr was set
    let key_expr: String = sink.property("key-expr");
    assert_eq!(key_expr, "demo/video/stream");

    // Get the URI back
    let uri = uri_handler.uri().unwrap();
    assert_eq!(uri, "zenoh:demo/video/stream");
}

#[test]
#[serial]
fn test_zenohsrc_simple_uri() {
    init();

    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc");

    let uri_handler = src.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Set a simple URI
    uri_handler.set_uri("zenoh:demo/video/stream").unwrap();

    // Verify the key-expr was set
    let key_expr: String = src.property("key-expr");
    assert_eq!(key_expr, "demo/video/stream");

    // Get the URI back
    let uri = uri_handler.uri().unwrap();
    assert_eq!(uri, "zenoh:demo/video/stream");
}

#[test]
#[serial]
fn test_uri_with_parameters() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Set URI with parameters
    uri_handler
        .set_uri("zenoh:demo/video?priority=2&reliability=reliable&congestion-control=drop")
        .unwrap();

    // Verify all properties were set correctly
    let key_expr: String = sink.property("key-expr");
    let priority: u32 = sink.property("priority");
    let reliability: String = sink.property("reliability");
    let congestion_control: String = sink.property("congestion-control");

    assert_eq!(key_expr, "demo/video");
    assert_eq!(priority, 2);
    assert_eq!(reliability, "reliable");
    assert_eq!(congestion_control, "drop");
}

#[test]
#[serial]
fn test_uri_with_express_mode() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Set URI with express mode enabled
    uri_handler
        .set_uri("zenoh:demo/video?express=true&priority=1")
        .unwrap();

    let key_expr: String = sink.property("key-expr");
    let express: bool = sink.property("express");
    let priority: u32 = sink.property("priority");

    assert_eq!(key_expr, "demo/video");
    assert!(express);
    assert_eq!(priority, 1);
}

#[test]
#[serial]
fn test_uri_with_config_file() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Set URI with config file path
    uri_handler
        .set_uri("zenoh:demo/video?config=/path/to/config.json5")
        .unwrap();

    let key_expr: String = sink.property("key-expr");
    let config: Option<String> = sink.property("config");

    assert_eq!(key_expr, "demo/video");
    assert_eq!(config, Some("/path/to/config.json5".to_string()));
}

#[test]
#[serial]
fn test_uri_url_encoding() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Set URI with URL-encoded characters (spaces, special chars)
    uri_handler
        .set_uri("zenoh:demo/video/stream%20with%20spaces")
        .unwrap();

    let key_expr: String = sink.property("key-expr");
    assert_eq!(key_expr, "demo/video/stream with spaces");
}

#[test]
#[serial]
fn test_uri_get_after_set_properties() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    // Set properties individually
    sink.set_property("key-expr", "demo/video");
    sink.set_property("priority", 3u32);
    sink.set_property("reliability", "reliable");
    sink.set_property("express", true);

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Get URI - should reflect all non-default properties
    let uri = uri_handler.uri().unwrap();

    assert!(uri.contains("demo/video"));
    assert!(uri.contains("priority=3"));
    assert!(uri.contains("reliability=reliable"));
    assert!(uri.contains("express=true"));
}

#[test]
#[serial]
fn test_invalid_uri_scheme() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Try to set invalid URI scheme
    let result = uri_handler.set_uri("http://example.com");

    assert!(result.is_err(), "Should reject invalid URI scheme");
}

#[test]
#[serial]
fn test_empty_key_expression() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Try to set URI with empty key expression
    let result = uri_handler.set_uri("zenoh:");

    assert!(result.is_err(), "Should reject empty key expression");
}

#[test]
#[serial]
fn test_uri_cannot_change_while_playing() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Set initial URI
    uri_handler.set_uri("zenoh:demo/video").unwrap();

    // Start the element
    sink.set_state(gst::State::Ready).unwrap();

    // Try to change URI while not in NULL state
    let result = uri_handler.set_uri("zenoh:demo/audio");

    // Should fail because element is not in NULL/Stopped state
    assert!(
        result.is_err(),
        "Should not allow URI change while not in NULL state"
    );

    // Clean up
    sink.set_state(gst::State::Null).unwrap();
}

#[test]
#[serial]
fn test_invalid_priority_in_uri() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Try to set URI with invalid priority value
    let result = uri_handler.set_uri("zenoh:demo/video?priority=invalid");

    assert!(result.is_err(), "Should reject invalid priority value");
}

#[test]
#[serial]
fn test_invalid_reliability_in_uri() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Try to set URI with invalid reliability value
    let result = uri_handler.set_uri("zenoh:demo/video?reliability=invalid");

    assert!(result.is_err(), "Should reject invalid reliability value");
}

#[test]
#[serial]
fn test_invalid_congestion_control_in_uri() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Try to set URI with invalid congestion-control value
    let result = uri_handler.set_uri("zenoh:demo/video?congestion-control=invalid");

    assert!(
        result.is_err(),
        "Should reject invalid congestion-control value"
    );
}

#[test]
#[serial]
fn test_uri_protocols() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Get supported protocols
    let protocols = uri_handler.protocols();

    assert_eq!(protocols.len(), 1);
    assert_eq!(protocols[0], "zenoh");
}

#[test]
#[serial]
fn test_uri_type() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc");

    let sink_uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();
    let src_uri_handler = src.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Verify URI types
    assert_eq!(sink_uri_handler.uri_type(), gst::URIType::Sink);
    assert_eq!(src_uri_handler.uri_type(), gst::URIType::Src);
}

#[test]
#[serial]
fn test_uri_with_wildcards() {
    init();

    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc");

    let uri_handler = src.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Set URI with Zenoh wildcards
    uri_handler.set_uri("zenoh:demo/*/video").unwrap();

    let key_expr: String = src.property("key-expr");
    assert_eq!(key_expr, "demo/*/video");

    // Try multi-level wildcard
    uri_handler.set_uri("zenoh:sensors/**").unwrap();
    let key_expr: String = src.property("key-expr");
    assert_eq!(key_expr, "sensors/**");
}

#[test]
#[serial]
fn test_uri_unknown_parameters_warning() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let uri_handler = sink.dynamic_cast_ref::<gst::URIHandler>().unwrap();

    // Set URI with unknown parameter (should warn but not fail)
    let result = uri_handler.set_uri("zenoh:demo/video?unknown-param=value&priority=2");

    assert!(
        result.is_ok(),
        "Should accept URI with unknown parameters (with warning)"
    );

    // Known parameters should still be applied
    let priority: u32 = sink.property("priority");
    assert_eq!(priority, 2);
}
