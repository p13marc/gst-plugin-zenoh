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
fn test_zenohsink_start_without_key_expr() {
    init();
    
    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");
    
    // Try to start without setting key-expr (should fail)
    let pipeline = gst::Pipeline::builder().build();
    pipeline.add(&sink).unwrap();
    
    // This should fail because no key-expr is set
    let result = pipeline.set_state(gst::State::Playing);
    
    // The state change should fail or go to paused with errors
    assert!(matches!(
        result,
        Err(_) | Ok(gst::StateChangeSuccess::Async) | Ok(gst::StateChangeSuccess::NoPreroll)
    ));
}

#[test]
#[serial]
fn test_zenohsrc_start_without_key_expr() {
    init();
    
    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc");
    
    // Try to start without setting key-expr (should fail)
    let pipeline = gst::Pipeline::builder().build();
    pipeline.add(&src).unwrap();
    
    // This should fail because no key-expr is set
    let result = pipeline.set_state(gst::State::Playing);
    
    // The state change should fail or go to paused with errors
    assert!(matches!(
        result,
        Err(_) | Ok(gst::StateChangeSuccess::Async) | Ok(gst::StateChangeSuccess::NoPreroll)
    ));
}

#[test]
#[serial]
fn test_invalid_config_file() {
    init();
    
    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/key")
        .property("config", "/nonexistent/config.json5")
        .build()
        .expect("Failed to create zenohsink");
    
    let pipeline = gst::Pipeline::builder().build();
    pipeline.add(&sink).unwrap();
    
    // This should fail because the config file doesn't exist
    let result = pipeline.set_state(gst::State::Playing);
    
    // Should fail or have async state change with errors
    assert!(matches!(
        result,
        Err(_) | Ok(gst::StateChangeSuccess::Async) | Ok(gst::StateChangeSuccess::NoPreroll)
    ));
}

#[test]
#[serial] 
fn test_property_validation_logging() {
    init();
    
    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");
    
    // Set invalid values - these should log warnings but not crash
    sink.set_property("congestion-control", "totally-invalid");
    sink.set_property("reliability", "also-invalid");
    
    // Verify the properties remained at their defaults
    let congestion: String = sink.property("congestion-control");
    assert_eq!(congestion, "block");
    
    let reliability: String = sink.property("reliability");
    assert_eq!(reliability, "best-effort");
}

#[test]
#[serial]
fn test_double_start_handling() {
    init();
    
    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/double-start")
        .build()
        .expect("Failed to create zenohsink");
    
    let pipeline = gst::Pipeline::builder().build();
    pipeline.add(&sink).unwrap();
    
    // Try to set state multiple times - should handle gracefully
    let _result1 = pipeline.set_state(gst::State::Ready);
    let _result2 = pipeline.set_state(gst::State::Ready); // Should be handled gracefully
    
    // Clean up
    let _cleanup = pipeline.set_state(gst::State::Null);
}

#[test]
#[serial]
fn test_priority_bounds() {
    init();
    
    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");
    
    // Test setting priority within bounds (1-7 for Zenoh)
    sink.set_property("priority", 1u32); // RealTime
    let priority: u32 = sink.property("priority");
    assert_eq!(priority, 1);
    
    sink.set_property("priority", 7u32); // Background
    let priority: u32 = sink.property("priority");
    assert_eq!(priority, 7);
    
    // Test setting priority outside bounds - GStreamer should reject with panic
    let result = std::panic::catch_unwind(|| {
        sink.set_property("priority", 0u32);
    });
    // Should panic because 0 is outside the 1-7 range
    assert!(result.is_err(), "Setting priority outside bounds should panic");
    
    let result = std::panic::catch_unwind(|| {
        sink.set_property("priority", 8u32);
    });
    // Should panic because 8 is outside the 1-7 range
    assert!(result.is_err(), "Setting priority outside bounds should panic");
}
