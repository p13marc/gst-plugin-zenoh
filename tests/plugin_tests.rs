use gst::prelude::*;
use serial_test::serial;

/// Initialize GStreamer and register our plugin for tests
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
fn test_plugin_registration() {
    init();
    
    // Test that our plugin elements are registered
    let zenohsink_factory = gst::ElementFactory::find("zenohsink");
    assert!(zenohsink_factory.is_some(), "zenohsink element not found");
    
    let zenohsrc_factory = gst::ElementFactory::find("zenohsrc");
    assert!(zenohsrc_factory.is_some(), "zenohsrc element not found");
}

#[test] 
#[serial]
fn test_zenohsink_creation() {
    init();
    
    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");
        
    // Element name should start with "zenohsink" followed by a number
    assert!(sink.name().as_str().starts_with("zenohsink"));
    assert_eq!(sink.factory().unwrap().name(), "zenohsink");
}

#[test]
#[serial] 
fn test_zenohsrc_creation() {
    init();
    
    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc");
        
    assert!(src.name().as_str().starts_with("zenohsrc"));
    assert_eq!(src.factory().unwrap().name(), "zenohsrc");
}

#[test]
#[serial]
fn test_zenohsink_properties() {
    init();
    
    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");
    
    // Test default property values
    let key_expr: String = sink.property("key-expr");
    assert_eq!(key_expr, "");
    
    let config: Option<String> = sink.property("config");
    assert_eq!(config, None);
    
    let priority: i32 = sink.property("priority");
    assert_eq!(priority, 0);
    
    let congestion_control: String = sink.property("congestion-control");
    assert_eq!(congestion_control, "block");
    
    let reliability: String = sink.property("reliability");
    assert_eq!(reliability, "best-effort");
    
    // Test setting properties
    sink.set_property("key-expr", "test/key");
    let new_key_expr: String = sink.property("key-expr");
    assert_eq!(new_key_expr, "test/key");
    
    sink.set_property("priority", 5i32);
    let new_priority: i32 = sink.property("priority");
    assert_eq!(new_priority, 5);
    
    sink.set_property("congestion-control", "drop");
    let new_congestion: String = sink.property("congestion-control");
    assert_eq!(new_congestion, "drop");
    
    sink.set_property("reliability", "reliable");
    let new_reliability: String = sink.property("reliability");
    assert_eq!(new_reliability, "reliable");
}

#[test]
#[serial]
fn test_zenohsrc_properties() {
    init();
    
    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc");
    
    // Test default property values
    let key_expr: String = src.property("key-expr");
    assert_eq!(key_expr, "");
    
    let config: Option<String> = src.property("config");
    assert_eq!(config, None);
    
    let priority: i32 = src.property("priority");
    assert_eq!(priority, 0);
    
    let congestion_control: String = src.property("congestion-control");
    assert_eq!(congestion_control, "block");
    
    let reliability: String = src.property("reliability");
    assert_eq!(reliability, "best-effort");
    
    // Test setting properties
    src.set_property("key-expr", "test/key");
    let new_key_expr: String = src.property("key-expr");
    assert_eq!(new_key_expr, "test/key");
    
    src.set_property("priority", -10i32);
    let new_priority: i32 = src.property("priority");
    assert_eq!(new_priority, -10);
}

#[test]
#[serial]
fn test_zenohsink_invalid_properties() {
    init();
    
    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");
    
    // Test invalid congestion control - should keep default
    sink.set_property("congestion-control", "invalid");
    let congestion: String = sink.property("congestion-control");
    assert_eq!(congestion, "block"); // Should remain default
    
    // Test invalid reliability - should keep default  
    sink.set_property("reliability", "invalid");
    let reliability: String = sink.property("reliability");
    assert_eq!(reliability, "best-effort"); // Should remain default
}

#[test]
#[serial]
fn test_pipeline_creation() {
    init();
    
    // Test creating a pipeline with our elements
    let pipeline = gst::Pipeline::builder()
        .name("test-pipeline")
        .build();
    
    let src = gst::ElementFactory::make("zenohsrc")
        .name("test-src")
        .property("key-expr", "test/data")
        .build()
        .expect("Failed to create zenohsrc");
        
    let sink = gst::ElementFactory::make("zenohsink")
        .name("test-sink") 
        .property("key-expr", "test/output")
        .build()
        .expect("Failed to create zenohsink");
    
    pipeline.add_many([&src, &sink]).unwrap();
    
    // Note: We don't link them directly as they would need compatible caps
    // This test just verifies the elements can be added to a pipeline
    
    assert_eq!(pipeline.children().len(), 2);
}

#[test]
#[serial]
fn test_element_state_changes() {
    init();
    
    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/state")
        .build()
        .expect("Failed to create zenohsink");
    
    // Test state changes - these should work without Zenoh running
    // as long as we don't actually start the element
    let initial_state = sink.current_state();
    assert_eq!(initial_state, gst::State::Null);
    
    // Test ready state
    let ready_result = sink.set_state(gst::State::Ready);
    assert!(ready_result.is_ok());
    
    // Return to null
    let null_result = sink.set_state(gst::State::Null);
    assert!(null_result.is_ok());
}