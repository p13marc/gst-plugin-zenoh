//! Shutdown / liveness tests for Epic #2 (concurrency & shutdown safety).
//!
//! These verify that tearing a pipeline down never hangs the calling thread, even
//! when there is no subscriber present and the default `congestion-control=block`
//! is in effect. A watchdog thread fails the test if the NULL transition does not
//! complete promptly instead of deadlocking the whole test binary.

use std::sync::mpsc;
use std::time::Duration;

use gst::prelude::*;
use serial_test::serial;

mod common;
use common::init;

/// Run `f` on a worker thread and panic if it does not finish within `timeout`.
fn assert_completes_within<F>(timeout: Duration, what: &str, f: F)
where
    F: FnOnce() + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        f();
        let _ = tx.send(());
    });
    match rx.recv_timeout(timeout) {
        Ok(()) => {
            let _ = handle.join();
        }
        Err(_) => panic!("{what} did not complete within {timeout:?} (likely hung)"),
    }
}

/// A sender with no subscriber under default `congestion-control=block` must reach
/// NULL without hanging the state-change thread.
#[test]
#[serial]
fn test_sink_shutdown_without_subscriber_does_not_hang() {
    init();

    assert_completes_within(Duration::from_secs(15), "sink play+null cycle", || {
        let videotestsrc = gst::ElementFactory::make("videotestsrc")
            .property("is-live", true)
            .build()
            .expect("Failed to create videotestsrc");
        let zenohsink = gst::ElementFactory::make("zenohsink")
            .property("key-expr", "test/shutdown/no-subscriber")
            // Defaults: congestion-control=block. Keep a small publish timeout so a
            // stalled publish can never wedge the streaming thread.
            .property("publish-timeout-ms", 1000u64)
            .build()
            .expect("Failed to create zenohsink");

        let pipeline = gst::Pipeline::new();
        pipeline.add_many([&videotestsrc, &zenohsink]).unwrap();
        videotestsrc.link(&zenohsink).unwrap();

        pipeline.set_state(gst::State::Playing).unwrap();
        std::thread::sleep(Duration::from_millis(300));

        // The critical operation: this must not block forever.
        pipeline.set_state(gst::State::Null).unwrap();
    });
}

/// Cycling a sink through READY→PLAYING→NULL repeatedly must remain responsive
/// (no accumulating deadlock from the publish worker lifecycle).
#[test]
#[serial]
fn test_sink_repeated_play_null_cycles() {
    init();

    assert_completes_within(Duration::from_secs(20), "repeated play/null cycles", || {
        let videotestsrc = gst::ElementFactory::make("videotestsrc")
            .property("is-live", true)
            .build()
            .expect("Failed to create videotestsrc");
        let zenohsink = gst::ElementFactory::make("zenohsink")
            .property("key-expr", "test/shutdown/cycles")
            .property("publish-timeout-ms", 1000u64)
            .build()
            .expect("Failed to create zenohsink");

        let pipeline = gst::Pipeline::new();
        pipeline.add_many([&videotestsrc, &zenohsink]).unwrap();
        videotestsrc.link(&zenohsink).unwrap();

        for _ in 0..3 {
            pipeline.set_state(gst::State::Playing).unwrap();
            std::thread::sleep(Duration::from_millis(150));
            pipeline.set_state(gst::State::Null).unwrap();
        }
    });
}
