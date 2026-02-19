//! Tests for the matching status feature (has-subscribers property and matching-changed signal).
//!
//! The matching listener is now set up during NULL→READY, so subscriber
//! detection works without the pipeline being in PAUSED or PLAYING state.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use gst::prelude::*;
use serial_test::serial;
use zenoh::Wait;

mod common;
#[path = "common/key_expr.rs"]
mod key_expr;
use common::init;
use key_expr::unique_key_expr;

#[test]
#[serial]
fn test_has_subscribers_property_exists_and_defaults_false() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    let has_subscribers: bool = sink.property("has-subscribers");
    assert!(!has_subscribers, "has-subscribers should default to false");
}

#[test]
#[serial]
fn test_has_subscribers_property_is_read_only() {
    init();

    let sink = gst::ElementFactory::make("zenohsink")
        .build()
        .expect("Failed to create zenohsink");

    // Verify the property is read-only by checking its flags
    let pspec = sink
        .find_property("has-subscribers")
        .expect("has-subscribers property should exist");
    assert!(
        !pspec.flags().contains(gst::glib::ParamFlags::WRITABLE),
        "has-subscribers should not be writable"
    );
    assert!(
        pspec.flags().contains(gst::glib::ParamFlags::READABLE),
        "has-subscribers should be readable"
    );
}

#[test]
#[serial]
fn test_has_subscribers_typed_api() {
    init();

    let sink = gstzenoh::ZenohSink::new("test/matching/typed");
    assert!(
        !sink.has_subscribers(),
        "should default to false via typed API"
    );
}

/// Verify that matching detection works from READY state — no need for PLAYING.
#[test]
#[serial]
fn test_matching_works_from_ready_state() {
    init();

    let key_expr = unique_key_expr("matching/ready");

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let sink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .build();

    // Track signal emissions
    let signal_received = Arc::new(AtomicBool::new(false));
    let signal_value = Arc::new(Mutex::new(None::<bool>));

    let sr = signal_received.clone();
    let sv = signal_value.clone();
    sink.connect_matching_changed(move |_sink, matching| {
        sv.lock().unwrap().replace(matching);
        sr.store(true, Ordering::SeqCst);
    });

    // Only go to READY — not PAUSED or PLAYING
    sink.set_state(gst::State::Ready).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Initially no subscribers
    assert!(
        !sink.has_subscribers(),
        "should have no subscribers initially"
    );

    // Create a subscriber
    let _subscriber = zenoh_session
        .declare_subscriber(&key_expr)
        .wait()
        .expect("Failed to create subscriber");

    // Wait for the matching callback to fire
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !signal_received.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }

    assert!(
        signal_received.load(Ordering::SeqCst),
        "matching-changed signal should fire in READY state"
    );
    assert_eq!(
        *signal_value.lock().unwrap(),
        Some(true),
        "signal should have been emitted with true"
    );
    assert!(
        sink.has_subscribers(),
        "has-subscribers should be true in READY state"
    );

    // Cleanup
    let _ = sink.set_state(gst::State::Null);
}

#[test]
#[serial]
fn test_matching_changed_signal_fires_on_subscriber_connect() {
    init();

    let key_expr = unique_key_expr("matching/signal");

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    // Create the sink pipeline with appsrc (no data flow needed)
    let pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder()
        .format(gst::Format::Bytes)
        .build();

    let sink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .build();

    // Track signal emissions
    let signal_received = Arc::new(AtomicBool::new(false));
    let signal_value = Arc::new(Mutex::new(None::<bool>));

    let sr = signal_received.clone();
    let sv = signal_value.clone();
    sink.connect_matching_changed(move |_sink, matching| {
        sv.lock().unwrap().replace(matching);
        sr.store(true, Ordering::SeqCst);
    });

    let src_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = sink.clone().upcast();
    pipeline.add_many([&src_elem, &sink_elem]).unwrap();
    src_elem.link(&sink_elem).unwrap();

    pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Initially no subscribers
    assert!(
        !sink.has_subscribers(),
        "should have no subscribers initially"
    );

    // Create a subscriber on the same key expression
    let _subscriber = zenoh_session
        .declare_subscriber(&key_expr)
        .wait()
        .expect("Failed to create subscriber");

    // Wait for the matching callback to fire
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !signal_received.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }

    assert!(
        signal_received.load(Ordering::SeqCst),
        "matching-changed signal should have fired"
    );
    assert_eq!(
        *signal_value.lock().unwrap(),
        Some(true),
        "signal should have been emitted with true"
    );
    assert!(
        sink.has_subscribers(),
        "has-subscribers should be true after subscriber connects"
    );

    // Cleanup
    let _ = pipeline.set_state(gst::State::Null);
}

#[test]
#[serial]
fn test_matching_changed_signal_fires_on_subscriber_disconnect() {
    init();

    let key_expr = unique_key_expr("matching/disconnect");

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    // Create a subscriber first
    let subscriber = zenoh_session
        .declare_subscriber(&key_expr)
        .wait()
        .expect("Failed to create subscriber");

    // Small delay for subscriber registration to propagate
    thread::sleep(Duration::from_millis(200));

    // Create the sink — only go to READY
    let sink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .build();

    // Track signal emissions — collect all values
    let signal_values = Arc::new(Mutex::new(Vec::<bool>::new()));

    let sv = signal_values.clone();
    sink.connect_matching_changed(move |_sink, matching| {
        sv.lock().unwrap().push(matching);
    });

    sink.set_state(gst::State::Ready).unwrap();

    // Wait for initial status to settle — subscriber already exists
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !sink.has_subscribers() && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert!(
        sink.has_subscribers(),
        "has-subscribers should be true (subscriber pre-existed)"
    );

    // Now drop the subscriber
    drop(subscriber);

    // Wait for the false signal
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while sink.has_subscribers() && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }

    assert!(
        !sink.has_subscribers(),
        "has-subscribers should become false after subscriber disconnects"
    );

    // Cleanup
    let _ = sink.set_state(gst::State::Null);
}

#[test]
#[serial]
fn test_matching_bus_message() {
    init();

    let key_expr = unique_key_expr("matching/bus");

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    // Create the sink pipeline
    let pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder()
        .format(gst::Format::Bytes)
        .build();

    let sink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .build();

    let src_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = sink.clone().upcast();
    pipeline.add_many([&src_elem, &sink_elem]).unwrap();
    src_elem.link(&sink_elem).unwrap();

    pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create a subscriber to trigger the matching change
    let _subscriber = zenoh_session
        .declare_subscriber(&key_expr)
        .wait()
        .expect("Failed to create subscriber");

    // Poll the bus for our element message
    let bus = pipeline.bus().unwrap();
    let mut found_message = false;

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !found_message && std::time::Instant::now() < deadline {
        if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(100)) {
            if let gst::MessageView::Element(element_msg) = msg.view() {
                if let Some(structure) = element_msg.structure() {
                    if structure.name() == "zenoh-matching-changed" {
                        let has_subs = structure.get::<bool>("has-subscribers").unwrap();
                        if has_subs {
                            found_message = true;
                        }
                    }
                }
            }
        }
    }

    assert!(
        found_message,
        "should have received zenoh-matching-changed bus message with has-subscribers=true"
    );

    // Cleanup
    let _ = pipeline.set_state(gst::State::Null);
}
