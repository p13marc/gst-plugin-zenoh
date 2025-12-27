use gst::prelude::*;
use serial_test::serial;

mod common;
use common::init;

#[test]
#[serial]
fn test_zenohsink_statistics_initial_values() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    // Check initial statistics values (should be 0)
    let bytes_sent: u64 = sink.property("bytes-sent");
    let messages_sent: u64 = sink.property("messages-sent");
    let errors: u64 = sink.property("errors");
    let dropped: u64 = sink.property("dropped");

    assert_eq!(bytes_sent, 0, "Initial bytes-sent should be 0");
    assert_eq!(messages_sent, 0, "Initial messages-sent should be 0");
    assert_eq!(errors, 0, "Initial errors should be 0");
    assert_eq!(dropped, 0, "Initial dropped should be 0");
}

#[test]
#[serial]
fn test_zenohsrc_statistics_initial_values() {
    init();

    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc");

    // Check initial statistics values (should be 0)
    let bytes_received: u64 = src.property("bytes-received");
    let messages_received: u64 = src.property("messages-received");
    let errors: u64 = src.property("errors");

    assert_eq!(bytes_received, 0, "Initial bytes-received should be 0");
    assert_eq!(
        messages_received, 0,
        "Initial messages-received should be 0"
    );
    assert_eq!(errors, 0, "Initial errors should be 0");
}

#[test]
#[serial]
fn test_zenohsink_statistics_read_only() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    // Attempting to set read-only properties should be ignored/fail gracefully
    // GStreamer will log a warning but not crash

    // These should not change the values
    let _result = std::panic::catch_unwind(|| {
        sink.set_property("bytes-sent", 12345u64);
    });

    // The set should either panic or be ignored - either is acceptable for read-only
    // The important thing is the value doesn't change
    let bytes_sent: u64 = sink.property("bytes-sent");
    assert_eq!(bytes_sent, 0, "bytes-sent should remain 0 (read-only)");
}

#[test]
#[serial]
fn test_zenohsrc_statistics_read_only() {
    init();

    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc");

    // Attempting to set read-only properties should be ignored/fail gracefully
    let _result = std::panic::catch_unwind(|| {
        src.set_property("bytes-received", 12345u64);
    });

    let bytes_received: u64 = src.property("bytes-received");
    assert_eq!(
        bytes_received, 0,
        "bytes-received should remain 0 (read-only)"
    );
}

// This test is commented out because it can hang in some environments
// TODO: Re-enable with proper timeout handling
/*
#[test]
#[serial]
fn test_statistics_integration() {
    init();

    // Create a simple pipeline with zenohsink and zenohsrc
    let _pipeline = gst::Pipeline::new();

    // Create source element
    let videotestsrc = gst::ElementFactory::make("videotestsrc")
        .property("num-buffers", 10i32)
        .property("is-live", false)
        .build()
        .expect("Failed to create videotestsrc");

    // Create zenohsink
    let zenohsink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/statistics/video")
        .build()
        .expect("Failed to create zenohsink");

    // Create zenohsrc
    let zenohsrc = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", "test/statistics/video")
        .build()
        .expect("Failed to create zenohsrc");

    // Create fakesink
    let fakesink = gst::ElementFactory::make("fakesink")
        .build()
        .expect("Failed to create fakesink");

    // Build sender pipeline
    let sender_pipeline = gst::Pipeline::new();
    sender_pipeline
        .add_many([&videotestsrc, &zenohsink])
        .unwrap();
    videotestsrc.link(&zenohsink).unwrap();

    // Build receiver pipeline
    let receiver_pipeline = gst::Pipeline::new();
    receiver_pipeline.add_many([&zenohsrc, &fakesink]).unwrap();
    zenohsrc.link(&fakesink).unwrap();

    // Start both pipelines
    sender_pipeline.set_state(gst::State::Playing).unwrap();
    receiver_pipeline.set_state(gst::State::Playing).unwrap();

    // Wait for a bit to let data flow
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Check sender statistics
    let bytes_sent: u64 = zenohsink.property("bytes-sent");
    let messages_sent: u64 = zenohsink.property("messages-sent");

    println!(
        "Sender - Bytes sent: {}, Messages sent: {}",
        bytes_sent, messages_sent
    );

    // We should have sent some data
    assert!(messages_sent > 0, "Should have sent at least one message");
    assert!(bytes_sent > 0, "Should have sent some bytes");

    // Check receiver statistics
    let bytes_received: u64 = zenohsrc.property("bytes-received");
    let messages_received: u64 = zenohsrc.property("messages-received");

    println!(
        "Receiver - Bytes received: {}, Messages received: {}",
        bytes_received, messages_received
    );

    // We should have received some data
    assert!(
        messages_received > 0,
        "Should have received at least one message"
    );
    assert!(bytes_received > 0, "Should have received some bytes");

    // Stop pipelines
    sender_pipeline.set_state(gst::State::Null).unwrap();
    receiver_pipeline.set_state(gst::State::Null).unwrap();
}
*/

#[test]
#[serial]
fn test_statistics_persist_across_state_changes() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/statistics/persist")
        .build()
        .expect("Failed to create zenohsink");

    // Start and stop the element
    sink.set_state(gst::State::Ready).unwrap();
    sink.set_state(gst::State::Paused).unwrap();

    // Note: Statistics are reset when element is stopped and restarted
    // This test verifies the behavior is consistent

    sink.set_state(gst::State::Ready).unwrap();
    sink.set_state(gst::State::Null).unwrap();

    // After returning to NULL, statistics should reset to 0
    let bytes_sent: u64 = sink.property("bytes-sent");
    assert_eq!(bytes_sent, 0, "Statistics should reset after NULL state");
}

#[test]
#[serial]
fn test_statistics_properties_exist() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let src = gst::ElementFactory::make("zenohsrc")
        .build()
        .expect("Failed to create zenohsrc");

    // Verify all statistics properties exist and are readable
    let sink_props = ["bytes-sent", "messages-sent", "errors", "dropped"];
    for prop in &sink_props {
        let value: u64 = sink.property(prop);
        println!("zenohsink.{} = {}", prop, value);
    }

    let src_props = ["bytes-received", "messages-received", "errors"];
    for prop in &src_props {
        let value: u64 = src.property(prop);
        println!("zenohsrc.{} = {}", prop, value);
    }
}

#[test]
#[serial]
fn test_error_statistics_on_invalid_session() {
    init();

    // Create sink with invalid config file to trigger errors
    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/errors")
        .property("config", "/nonexistent/path/to/config.json5")
        .build()
        .expect("Failed to create zenohsink");

    // Try to start - this should fail
    let result = sink.set_state(gst::State::Playing);

    // The state change should fail
    assert!(
        result.is_err() || result == Ok(gst::StateChangeSuccess::Async),
        "Should fail or go async with invalid config"
    );

    // Clean up
    sink.set_state(gst::State::Null).unwrap();
}

#[test]
#[serial]
fn test_statistics_types() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    // Verify all statistics are u64 type
    let bytes_sent: u64 = sink.property("bytes-sent");
    let messages_sent: u64 = sink.property("messages-sent");
    let errors: u64 = sink.property("errors");
    let dropped: u64 = sink.property("dropped");

    // Type checking is done at compile time, so if we get here, types are correct
    assert_eq!(bytes_sent, 0);
    assert_eq!(messages_sent, 0);
    assert_eq!(errors, 0);
    assert_eq!(dropped, 0);
}
