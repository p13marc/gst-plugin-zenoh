//! Connectivity observability tests for gst-plugin-zenoh.
//!
//! Verify that the `connected` property and the `zenoh-connectivity-changed` bus
//! message reflect an open Zenoh transport. Uses explicit TCP endpoints with
//! multicast scouting disabled so a transport forms deterministically, rather
//! than relying on discovery.

use std::time::{Duration, Instant};

use gst::prelude::*;
use serial_test::serial;
use zenoh::Wait;

mod common;
#[path = "common/key_expr.rs"]
mod key_expr;
use common::init;
use key_expr::unique_key_expr;

/// Open a peer that listens on `endpoint`, with multicast scouting disabled.
fn open_listener(endpoint: &str) -> zenoh::Session {
    let mut cfg = zenoh::Config::default();
    cfg.insert_json5("listen/endpoints", &format!("[\"{endpoint}\"]"))
        .unwrap();
    cfg.insert_json5("scouting/multicast/enabled", "false")
        .unwrap();
    zenoh::open(cfg)
        .wait()
        .expect("Failed to open listener session")
}

/// Open a peer that connects to `endpoint`, with multicast scouting disabled.
fn open_connector(endpoint: &str) -> zenoh::Session {
    let mut cfg = zenoh::Config::default();
    cfg.insert_json5("connect/endpoints", &format!("[\"{endpoint}\"]"))
        .unwrap();
    cfg.insert_json5("scouting/multicast/enabled", "false")
        .unwrap();
    zenoh::open(cfg)
        .wait()
        .expect("Failed to open connector session")
}

/// A `zenohsrc` sharing a session that already has an open transport reports
/// `connected = true` and posts a `zenoh-connectivity-changed` (connected=true)
/// bus message once its listener replays the existing transport.
#[test]
#[serial]
fn test_src_reports_connected_over_open_transport() {
    init();

    let endpoint = "tcp/127.0.0.1:17451";
    let _listener = open_listener(endpoint);
    let client = open_connector(endpoint);

    // Give the TCP transport time to establish before the element observes it.
    std::thread::sleep(Duration::from_millis(800));

    let key_expr = unique_key_expr("connectivity");
    let pipeline = gst::Pipeline::new();
    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(client.clone())
        .receive_timeout_ms(50)
        .build();
    let fakesink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .unwrap();
    let src_elem: gst::Element = zenohsrc.clone().upcast();
    pipeline.add_many([&src_elem, &fakesink]).unwrap();
    src_elem.link(&fakesink).unwrap();

    let bus = pipeline.bus().unwrap();
    pipeline.set_state(gst::State::Playing).unwrap();

    // Poll the property and drain the bus for up to ~3s.
    let mut connected = false;
    let mut saw_msg = false;
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        while let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(10)) {
            if let gst::MessageView::Element(e) = msg.view()
                && let Some(s) = e.structure()
                && s.name() == "zenoh-connectivity-changed"
                && s.get::<bool>("connected").unwrap_or(false)
            {
                saw_msg = true;
            }
        }
        if zenohsrc.connected() {
            connected = true;
            if saw_msg {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let _ = pipeline.set_state(gst::State::Null);

    assert!(
        connected,
        "src 'connected' should be true while sharing a session with an open transport"
    );
    assert!(
        saw_msg,
        "expected a 'zenoh-connectivity-changed' bus message with connected=true"
    );
}

/// Regression guard for the connectivity-listener lifecycle on a SHARED session.
///
/// The listener must be undeclared on each stop (it is a kept handle, not a
/// `.background()` listener, which would leak one listener per cycle on a session
/// that outlives the element). This exercises the on-demand pattern
/// (PLAYING↔READY) over a shared session many times and asserts `connected` is
/// correctly re-established on every cycle — i.e. the per-cycle declare/undeclare
/// keeps working and never breaks the on-demand flow.
#[test]
#[serial]
fn test_connectivity_listener_cycles_on_shared_session() {
    init();

    let endpoint = "tcp/127.0.0.1:17452";
    let _listener = open_listener(endpoint);
    let client = open_connector(endpoint);
    std::thread::sleep(Duration::from_millis(800));

    let key_expr = unique_key_expr("connectivity-cycles");
    let pipeline = gst::Pipeline::new();
    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(client.clone())
        .receive_timeout_ms(50)
        .build();
    let fakesink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .unwrap();
    let src_elem: gst::Element = zenohsrc.clone().upcast();
    pipeline.add_many([&src_elem, &fakesink]).unwrap();
    src_elem.link(&fakesink).unwrap();

    // Run several PLAYING<->READY cycles. Each PLAYING re-declares a listener
    // (and each READY drops the previous one); connectivity must report true
    // every time over the shared, already-connected session.
    for cycle in 0..5 {
        pipeline.set_state(gst::State::Playing).unwrap();

        let mut connected = false;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if zenohsrc.connected() {
                connected = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            connected,
            "cycle {cycle}: 'connected' should be true over the shared open transport"
        );

        pipeline.set_state(gst::State::Ready).unwrap();
    }

    let _ = pipeline.set_state(gst::State::Null);
    // The shared session is still usable after all the element's listeners were
    // declared and undeclared — declare a fresh subscriber to prove it.
    let _sub = client
        .declare_subscriber(unique_key_expr("post-cycle"))
        .wait()
        .expect("shared session should remain healthy after listener churn");
}
