//! Integration tests for on-demand pipeline execution using matching status.
//!
//! These tests verify that a pipeline can be started/stopped in response
//! to Zenoh subscriber presence using the matching-changed signal.
//!
//! The on-demand pattern:
//! - Pipeline starts in READY state (Zenoh resources active, no data flowing)
//! - When a subscriber connects: transition to PLAYING
//! - When all subscribers disconnect: transition back to READY
//!
//! This conserves pipeline resources (encoders, capture devices, etc.)
//! while maintaining subscriber detection at zero cost.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use gst::prelude::*;
use serial_test::serial;
use zenoh::Wait;

mod common;
#[path = "common/key_expr.rs"]
mod key_expr;
use common::init;
use key_expr::unique_key_expr;

/// Helper: wait until a predicate returns true, or time out.
fn wait_for(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Build a pipeline: videotestsrc is-live=true ! zenohsink
/// is-live=true makes the source respect clock timing and allows proper state transitions.
fn build_pipeline(key_expr: &str, session: zenoh::Session) -> (gst::Pipeline, gstzenoh::ZenohSink) {
    let pipeline = gst::Pipeline::new();

    let src = gst::ElementFactory::make("videotestsrc")
        .property("is-live", true)
        .build()
        .unwrap();

    let sink = gstzenoh::ZenohSink::builder(key_expr)
        .session(session)
        .send_caps(false) // reduce overhead for tests
        .send_buffer_meta(false)
        .build();

    let src_elem: gst::Element = src.upcast();
    let sink_elem: gst::Element = sink.clone().upcast();
    pipeline.add_many([&src_elem, &sink_elem]).unwrap();
    src_elem.link(&sink_elem).unwrap();

    (pipeline, sink)
}

/// Helper: get current pipeline state with a short timeout.
fn current_state(pipeline: &gst::Pipeline) -> gst::State {
    let (_, current, _) = pipeline.state(gst::ClockTime::from_mseconds(500));
    current
}

/// On-demand lifecycle: pipeline starts in READY, transitions to PLAYING
/// when subscribers appear, and goes back to READY when they leave.
#[test]
#[serial]
fn test_on_demand_ready_to_playing_cycle() {
    init();

    let key_expr = unique_key_expr("on_demand/ready_playing");

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let (pipeline, sink) = build_pipeline(&key_expr, zenoh_session.clone());

    // Wire up on-demand: subscriber -> Playing, no subscriber -> Ready
    let pipeline_weak = pipeline.downgrade();
    sink.connect_matching_changed(move |_sink, has_subscribers| {
        let Some(pipeline) = pipeline_weak.upgrade() else {
            return;
        };
        if has_subscribers {
            let _ = pipeline.set_state(gst::State::Playing);
        } else {
            let _ = pipeline.set_state(gst::State::Ready);
        }
    });

    // Start pipeline in READY — Zenoh resources are active, matching works
    pipeline.set_state(gst::State::Ready).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Verify we're in READY and no subscribers yet
    assert_eq!(current_state(&pipeline), gst::State::Ready);
    assert!(!sink.has_subscribers());

    // Connect a subscriber — pipeline should transition to PLAYING
    let subscriber = zenoh_session
        .declare_subscriber(&key_expr)
        .wait()
        .expect("Failed to create subscriber");

    let playing = wait_for(Duration::from_secs(5), || {
        current_state(&pipeline) == gst::State::Playing
    });
    assert!(
        playing,
        "pipeline should transition to PLAYING when subscriber connects"
    );
    assert!(sink.has_subscribers());

    // Disconnect the subscriber — pipeline should go back to READY
    drop(subscriber);

    let ready = wait_for(Duration::from_secs(5), || {
        current_state(&pipeline) == gst::State::Ready
    });
    assert!(
        ready,
        "pipeline should transition to READY when subscriber disconnects"
    );
    assert!(!sink.has_subscribers());

    // Reconnect — pipeline should go back to PLAYING
    let _subscriber2 = zenoh_session
        .declare_subscriber(&key_expr)
        .wait()
        .expect("Failed to create subscriber");

    let playing_again = wait_for(Duration::from_secs(5), || {
        current_state(&pipeline) == gst::State::Playing
    });
    assert!(
        playing_again,
        "pipeline should return to PLAYING when subscriber reconnects"
    );

    // Cleanup
    let _ = pipeline.set_state(gst::State::Null);
}

/// Verify that multiple subscribe/unsubscribe cycles work correctly.
#[test]
#[serial]
fn test_on_demand_multiple_cycles() {
    init();

    let key_expr = unique_key_expr("on_demand/cycles");

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let (pipeline, sink) = build_pipeline(&key_expr, zenoh_session.clone());

    // Count state transitions triggered by the signal
    let transition_count = Arc::new(AtomicU32::new(0));
    let tc = transition_count.clone();
    let pipeline_weak = pipeline.downgrade();
    sink.connect_matching_changed(move |_sink, has_subscribers| {
        tc.fetch_add(1, Ordering::SeqCst);
        let Some(pipeline) = pipeline_weak.upgrade() else {
            return;
        };
        if has_subscribers {
            let _ = pipeline.set_state(gst::State::Playing);
        } else {
            let _ = pipeline.set_state(gst::State::Ready);
        }
    });

    // Start in READY
    pipeline.set_state(gst::State::Ready).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Run 3 connect/disconnect cycles
    for i in 0..3 {
        // Connect
        let subscriber = zenoh_session
            .declare_subscriber(&key_expr)
            .wait()
            .expect("Failed to create subscriber");

        let playing = wait_for(Duration::from_secs(5), || {
            current_state(&pipeline) == gst::State::Playing
        });
        assert!(playing, "cycle {i}: should reach PLAYING");

        // Disconnect
        drop(subscriber);

        let ready = wait_for(Duration::from_secs(5), || {
            current_state(&pipeline) == gst::State::Ready
        });
        assert!(ready, "cycle {i}: should reach READY");
    }

    // We should have seen at least 6 transitions (3 true + 3 false)
    assert!(
        transition_count.load(Ordering::SeqCst) >= 6,
        "expected at least 6 signal emissions, got {}",
        transition_count.load(Ordering::SeqCst)
    );

    let _ = pipeline.set_state(gst::State::Null);
}

/// Verify that matching detection works even when pipeline never leaves READY.
#[test]
#[serial]
fn test_matching_detection_in_ready_only() {
    init();

    let key_expr = unique_key_expr("on_demand/ready_only");

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let (pipeline, sink) = build_pipeline(&key_expr, zenoh_session.clone());

    // Start in READY — never go to PLAYING
    pipeline.set_state(gst::State::Ready).unwrap();
    thread::sleep(Duration::from_millis(500));

    assert!(!sink.has_subscribers());

    // Connect a subscriber
    let subscriber = zenoh_session
        .declare_subscriber(&key_expr)
        .wait()
        .expect("Failed to create subscriber");

    let detected = wait_for(Duration::from_secs(5), || sink.has_subscribers());
    assert!(
        detected,
        "should detect subscriber in READY state (without PLAYING)"
    );

    // Disconnect
    drop(subscriber);

    let gone = wait_for(Duration::from_secs(5), || !sink.has_subscribers());
    assert!(gone, "should detect subscriber departure in READY state");

    let _ = pipeline.set_state(gst::State::Null);
}
