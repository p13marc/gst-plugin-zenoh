use gst::prelude::*;
use serial_test::serial;

mod common;
use common::init;

#[test]
#[serial]
fn test_sink_state_transitions() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/state/sink")
        .build()
        .expect("Failed to create zenohsink");

    // Test initial state
    assert_eq!(sink.current_state(), gst::State::Null);

    // Test transition to Ready
    assert!(sink.set_state(gst::State::Ready).is_ok());
    assert_eq!(sink.current_state(), gst::State::Ready);

    // Test transition back to Null
    assert!(sink.set_state(gst::State::Null).is_ok());
    assert_eq!(sink.current_state(), gst::State::Null);

    println!("Sink state transitions test passed");
}

#[test]
#[serial]
fn test_src_state_transitions() {
    init();

    let src = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", "test/state/src")
        .build()
        .expect("Failed to create zenohsrc");

    // Test initial state
    assert_eq!(src.current_state(), gst::State::Null);

    // Test transition to Ready
    assert!(src.set_state(gst::State::Ready).is_ok());
    assert_eq!(src.current_state(), gst::State::Ready);

    // Test transition back to Null
    assert!(src.set_state(gst::State::Null).is_ok());
    assert_eq!(src.current_state(), gst::State::Null);

    println!("Source state transitions test passed");
}

#[test]
#[serial]
fn test_property_changes_during_started_state() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/state/property")
        .build()
        .expect("Failed to create zenohsink");

    // Set initial properties
    sink.set_property("priority", 5u32);
    let priority: u32 = sink.property("priority");
    assert_eq!(priority, 5);

    // Start the element
    assert!(sink.set_state(gst::State::Ready).is_ok());

    // Try to change key-expr while started - should be ignored with warning
    sink.set_property("key-expr", "test/state/changed");
    let key_expr: String = sink.property("key-expr");
    assert_eq!(key_expr, "test/state/property"); // Should remain unchanged

    // Priority changes should also be blocked while started
    sink.set_property("priority", 3u32); // InteractiveLow priority
    let priority: u32 = sink.property("priority");
    assert_eq!(priority, 5); // Should remain unchanged

    // Stop the element
    assert!(sink.set_state(gst::State::Null).is_ok());

    // Now key-expr changes should work again
    sink.set_property("key-expr", "test/state/changed");
    let key_expr: String = sink.property("key-expr");
    assert_eq!(key_expr, "test/state/changed");

    // Now priority changes should also work again
    sink.set_property("priority", 3u32); // InteractiveLow priority
    let priority: u32 = sink.property("priority");
    assert_eq!(priority, 3);

    println!("Property change validation test passed");
}

#[test]
#[serial]
fn test_multiple_start_stop_cycles() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/state/cycles")
        .build()
        .expect("Failed to create zenohsink");

    // Test multiple start/stop cycles
    for i in 1..=3 {
        println!("Cycle {}: Starting", i);
        assert!(sink.set_state(gst::State::Ready).is_ok());
        assert_eq!(sink.current_state(), gst::State::Ready);

        println!("Cycle {}: Stopping", i);
        assert!(sink.set_state(gst::State::Null).is_ok());
        assert_eq!(sink.current_state(), gst::State::Null);
    }

    println!("Multiple start/stop cycles test passed");
}

#[test]
#[serial]
fn test_concurrent_elements() {
    init();

    // Test that multiple elements can be created and managed independently
    let sink1 = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/state/concurrent1")
        .build()
        .expect("Failed to create zenohsink1");

    let sink2 = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/state/concurrent2")
        .build()
        .expect("Failed to create zenohsink2");

    let src1 = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", "test/state/concurrent1")
        .build()
        .expect("Failed to create zenohsrc1");

    let src2 = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", "test/state/concurrent2")
        .build()
        .expect("Failed to create zenohsrc2");

    // Start all elements
    assert!(sink1.set_state(gst::State::Ready).is_ok());
    assert!(sink2.set_state(gst::State::Ready).is_ok());
    assert!(src1.set_state(gst::State::Ready).is_ok());
    assert!(src2.set_state(gst::State::Ready).is_ok());

    // Verify all are started
    assert_eq!(sink1.current_state(), gst::State::Ready);
    assert_eq!(sink2.current_state(), gst::State::Ready);
    assert_eq!(src1.current_state(), gst::State::Ready);
    assert_eq!(src2.current_state(), gst::State::Ready);

    // Stop all elements
    assert!(sink1.set_state(gst::State::Null).is_ok());
    assert!(sink2.set_state(gst::State::Null).is_ok());
    assert!(src1.set_state(gst::State::Null).is_ok());
    assert!(src2.set_state(gst::State::Null).is_ok());

    // Verify all are stopped
    assert_eq!(sink1.current_state(), gst::State::Null);
    assert_eq!(sink2.current_state(), gst::State::Null);
    assert_eq!(src1.current_state(), gst::State::Null);
    assert_eq!(src2.current_state(), gst::State::Null);

    println!("Concurrent elements test passed");
}

#[test]
#[serial]
fn test_error_conditions() {
    init();

    // Test element without key-expr
    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    // Should fail to start without key-expr
    let result = sink.set_state(gst::State::Ready);
    assert!(result.is_err(), "Should fail to start without key-expr");

    // Create a new element with proper key-expr
    let sink2 = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/state/error")
        .build()
        .expect("Failed to create zenohsink2");

    // This should work
    assert!(sink2.set_state(gst::State::Ready).is_ok());
    assert!(sink2.set_state(gst::State::Null).is_ok());

    println!("Error conditions test passed");
}
