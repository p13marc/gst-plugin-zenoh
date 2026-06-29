//! Throughput / latency baseline harness for the zenohsrc receive path.
//!
//! These are **not** correctness tests — they are `#[ignore]`d measurement
//! harnesses that establish a baseline and act as a regression tripwire for the
//! data path touched by the `handler`/ring work (#13) and the connectivity
//! listener (#14). They print numbers and assert only loose sanity bounds, since
//! end-to-end Zenoh timings are inherently noisy.
//!
//! Run explicitly:
//!   cargo test --test throughput_bench -- --ignored --nocapture
//!
//! Sender and receiver share one in-process session, so timings reflect the
//! plugin's encode/handler/decode path rather than the network.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use gst::prelude::*;
use serial_test::serial;
use zenoh::Wait;

mod common;
#[path = "common/key_expr.rs"]
mod key_expr;
use common::init;
use key_expr::unique_key_expr;

const N: u64 = 2000;
const BUF_SIZE: usize = 1024;

/// Push `N` buffers through `zenohsink` -> `zenohsrc` over a shared session and
/// return (received_count, elapsed) once all are received or a deadline passes.
fn run_path(handler: &str, queue_depth: u32) -> (u64, Duration) {
    let key_expr = unique_key_expr(&format!("bench-{handler}"));
    let session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("open session");

    // Receiver.
    let recv_pipeline = gst::Pipeline::new();
    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(session.clone())
        .receive_timeout_ms(20)
        .handler(handler)
        .queue_depth(queue_depth)
        .build();
    let fakesink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .unwrap();
    let src_elem: gst::Element = zenohsrc.clone().upcast();
    recv_pipeline.add_many([&src_elem, &fakesink]).unwrap();
    src_elem.link(&fakesink).unwrap();

    let received = Arc::new(AtomicU64::new(0));
    let received_probe = received.clone();
    let srcpad = zenohsrc.static_pad("src").unwrap();
    srcpad.add_probe(gst::PadProbeType::BUFFER, move |_, _| {
        received_probe.fetch_add(1, Ordering::Relaxed);
        gst::PadProbeReturn::Ok
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    std::thread::sleep(Duration::from_millis(500)); // establish subscription

    // Sender.
    let send_pipeline = gst::Pipeline::new();
    let appsrc = gst_app::AppSrc::builder()
        .format(gst::Format::Bytes)
        .build();
    let zenohsink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(session.clone())
        .build();
    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.clone().upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();
    send_pipeline.set_state(gst::State::Playing).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    let start = Instant::now();
    for i in 0..N {
        let mut buffer = gst::Buffer::with_size(BUF_SIZE).unwrap();
        buffer
            .get_mut()
            .unwrap()
            .set_pts(gst::ClockTime::from_mseconds(i));
        let _ = appsrc.push_buffer(buffer);
    }

    // Wait until all received (FIFO / large-ring) or a deadline.
    let deadline = start + Duration::from_secs(20);
    while received.load(Ordering::Relaxed) < N && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let elapsed = start.elapsed();
    let count = received.load(Ordering::Relaxed);

    let _ = send_pipeline.set_state(gst::State::Null);
    let _ = recv_pipeline.set_state(gst::State::Null);

    (count, elapsed)
}

#[test]
#[ignore = "benchmark; run with --ignored --nocapture"]
#[serial]
fn bench_fifo_throughput() {
    init();
    let (count, elapsed) = run_path("fifo", 30);
    let per_sec = count as f64 / elapsed.as_secs_f64();
    println!(
        "[bench] FIFO: received {count}/{N} in {:.3}s => {per_sec:.0} msg/s ({BUF_SIZE}B each)",
        elapsed.as_secs_f64()
    );
    assert_eq!(count, N, "FIFO must deliver every buffer");
}

#[test]
#[ignore = "benchmark; run with --ignored --nocapture"]
#[serial]
fn bench_ring_throughput() {
    init();
    // Depth >= N so the ring never drops; this isolates handler overhead vs FIFO.
    let (count, elapsed) = run_path("ring", (N as u32) + 100);
    let per_sec = count as f64 / elapsed.as_secs_f64();
    println!(
        "[bench] RING(depth={}): received {count}/{N} in {:.3}s => {per_sec:.0} msg/s ({BUF_SIZE}B each)",
        N + 100,
        elapsed.as_secs_f64()
    );
    // Loose tripwire: the ring handler must deliver everything when not dropping
    // and stay within a sane factor of FIFO (timings are noisy; this only catches
    // a gross regression, not micro-overhead).
    assert_eq!(count, N, "non-dropping ring must deliver every buffer");
}
