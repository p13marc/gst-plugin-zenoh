// Tests for session sharing functionality

mod common;

use gst::prelude::*;
use gstzenoh::{ZenohSink, ZenohSrc};
use serial_test::serial;
use zenoh::Wait;

#[test]
#[serial]
fn test_session_group_property_sink() {
    common::init();

    let sink = ZenohSink::new("demo/video");

    // Initially no session group
    assert!(sink.session_group().is_none());

    // Set session group
    sink.set_session_group("test-group");
    assert_eq!(sink.session_group(), Some("test-group".to_string()));
}

#[test]
#[serial]
fn test_session_group_property_src() {
    common::init();

    let src = ZenohSrc::new("demo/video");

    // Initially no session group
    assert!(src.session_group().is_none());

    // Set session group
    src.set_session_group("test-group");
    assert_eq!(src.session_group(), Some("test-group".to_string()));
}

#[test]
#[serial]
fn test_session_group_builder_sink() {
    common::init();

    let sink = ZenohSink::builder("demo/video")
        .session_group("my-group")
        .build();

    assert_eq!(sink.session_group(), Some("my-group".to_string()));
}

#[test]
#[serial]
fn test_session_group_builder_src() {
    common::init();

    let src = ZenohSrc::builder("demo/video")
        .session_group("my-group")
        .build();

    assert_eq!(src.session_group(), Some("my-group".to_string()));
}

#[test]
#[serial]
fn test_session_sharing_via_rust_api() {
    common::init();

    // Create a shared session
    let session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to create session");

    let session_zid = session.zid();

    // Create multiple sinks sharing the same session
    let sink1 = ZenohSink::builder("demo/video")
        .session(session.clone())
        .build();

    let sink2 = ZenohSink::builder("demo/audio")
        .session(session.clone())
        .build();

    // Create a source sharing the same session
    let src = ZenohSrc::builder("demo/data").session(session).build();

    // All elements should be creatable with the shared session
    assert_eq!(sink1.key_expr(), "demo/video");
    assert_eq!(sink2.key_expr(), "demo/audio");
    assert_eq!(src.key_expr(), "demo/data");

    // Note: We can't directly verify the session ZID from the element,
    // but we've confirmed the session was accepted without errors
    let _ = session_zid; // Use the variable to avoid warning
}

#[test]
#[serial]
fn test_session_sharing_via_session_group() {
    common::init();

    // Create elements with the same session group
    let sink1 = ZenohSink::builder("demo/video")
        .session_group("shared-group-1")
        .build();

    let sink2 = ZenohSink::builder("demo/audio")
        .session_group("shared-group-1")
        .build();

    let src = ZenohSrc::builder("demo/data")
        .session_group("shared-group-1")
        .build();

    // All elements should have the same session group
    assert_eq!(sink1.session_group(), Some("shared-group-1".to_string()));
    assert_eq!(sink2.session_group(), Some("shared-group-1".to_string()));
    assert_eq!(src.session_group(), Some("shared-group-1".to_string()));
}

#[test]
#[serial]
fn test_different_session_groups() {
    common::init();

    let sink1 = ZenohSink::builder("demo/video")
        .session_group("group-a")
        .build();

    let sink2 = ZenohSink::builder("demo/audio")
        .session_group("group-b")
        .build();

    // Different groups
    assert_eq!(sink1.session_group(), Some("group-a".to_string()));
    assert_eq!(sink2.session_group(), Some("group-b".to_string()));
    assert_ne!(sink1.session_group(), sink2.session_group());
}

#[test]
#[serial]
fn test_session_group_with_pipeline() {
    common::init();

    // Create a pipeline with elements using session groups
    let pipeline = gst::Pipeline::new();

    let sink1 = ZenohSink::builder("demo/test/video")
        .session_group("pipeline-group")
        .build();

    let sink2 = ZenohSink::builder("demo/test/audio")
        .session_group("pipeline-group")
        .build();

    let testsrc1 = gst::ElementFactory::make("videotestsrc")
        .property("num-buffers", 1i32)
        .build()
        .unwrap();

    let testsrc2 = gst::ElementFactory::make("audiotestsrc")
        .property("num-buffers", 1i32)
        .build()
        .unwrap();

    pipeline
        .add_many([&testsrc1, sink1.upcast_ref(), &testsrc2, sink2.upcast_ref()])
        .unwrap();
    testsrc1.link(&sink1).unwrap();
    testsrc2.link(&sink2).unwrap();

    // Start the pipeline - both sinks should share the same session
    pipeline.set_state(gst::State::Playing).unwrap();

    // Let it run briefly
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Stop the pipeline
    pipeline.set_state(gst::State::Null).unwrap();
}

#[test]
#[serial]
fn test_session_set_after_creation() {
    common::init();

    // Create elements without session
    let sink = ZenohSink::new("demo/video");

    // Create a session
    let session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to create session");

    // Set session after creation
    sink.set_session(session);

    // Element should still work normally
    assert_eq!(sink.key_expr(), "demo/video");
}

#[test]
#[serial]
fn test_zenohdemux_session_group() {
    common::init();

    use gstzenoh::zenohdemux::ZenohDemux;

    let demux = ZenohDemux::builder("sensor/**")
        .session_group("demux-group")
        .build();

    assert_eq!(demux.session_group(), Some("demux-group".to_string()));
}
