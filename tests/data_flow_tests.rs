//! End-to-end data flow tests for gst-plugin-zenoh.
//!
//! These tests verify that data sent through zenohsink is correctly
//! received by zenohsrc, proving actual data transmission works.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gst::prelude::*;
use serial_test::serial;
use zenoh::Wait;

mod common;
#[path = "common/key_expr.rs"]
mod key_expr;
#[path = "common/patterns.rs"]
mod patterns;
use common::init;
use key_expr::unique_key_expr;
use patterns::{generate_test_pattern, verify_test_pattern};

/// Helper to stop a pipeline with timeout (zenohsrc can block during state change)
fn stop_pipeline_with_timeout(pipeline: &gst::Pipeline, timeout: Duration) {
    let pipeline_clone = pipeline.clone();
    let cleanup_handle = thread::spawn(move || {
        let _ = pipeline_clone.set_state(gst::State::Null);
    });

    let start = Instant::now();
    while start.elapsed() < timeout {
        if cleanup_handle.is_finished() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    // If timeout, the thread will be orphaned but test continues
}

/// Test basic data round-trip: send data through zenohsink, receive via zenohsrc.
#[test]
#[serial]
fn test_basic_data_roundtrip() {
    init();

    let key_expr = unique_key_expr("roundtrip");
    let test_data: Vec<u8> = b"Hello, Zenoh!".to_vec();

    // Create a shared Zenoh session
    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    // Shared state
    let received_data: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let received_clone = received_data.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // Create receiver pipeline FIRST (subscriber must exist before publisher sends)
    let recv_pipeline = gst::Pipeline::new();

    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(zenoh_session.clone())
        .receive_timeout_ms(50)
        .build();

    let fakesink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .unwrap();

    let src_elem: gst::Element = zenohsrc.clone().upcast();
    recv_pipeline.add_many([&src_elem, &fakesink]).unwrap();
    src_elem.link(&fakesink).unwrap();

    // Add probe to capture data
    let srcpad = zenohsrc.static_pad("src").unwrap();
    srcpad.add_probe(gst::PadProbeType::BUFFER, move |_, probe_info| {
        if let Some(gst::PadProbeData::Buffer(ref buffer)) = probe_info.data {
            if let Ok(map) = buffer.map_readable() {
                *received_clone.lock().unwrap() = Some(map.as_slice().to_vec());
            }
        }
        gst::PadProbeReturn::Remove
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();

    // Give receiver time to establish subscription
    thread::sleep(Duration::from_millis(500));

    // Now create sender pipeline
    let send_pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder()
        .format(gst::Format::Bytes)
        .build();

    let zenohsink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .build();

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.clone().upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send data in a loop
    let test_data_send = test_data.clone();
    let appsrc_sender = appsrc.clone();
    let sender_thread = thread::spawn(move || {
        let mut count = 0;
        while !stop_clone.load(Ordering::SeqCst) && count < 100 {
            let mut buffer = gst::Buffer::with_size(test_data_send.len()).unwrap();
            buffer
                .get_mut()
                .unwrap()
                .copy_from_slice(0, &test_data_send)
                .unwrap();

            if appsrc_sender.push_buffer(buffer).is_err() {
                break;
            }
            count += 1;
            thread::sleep(Duration::from_millis(50));
        }
        appsrc_sender.end_of_stream().ok();
    });

    // Wait for data
    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    while received_data.lock().unwrap().is_none() && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    stop_flag.store(true, Ordering::SeqCst);

    // Clean up
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    // Verify
    let received = received_data.lock().unwrap();
    assert!(received.is_some(), "No data received within timeout");
    assert_eq!(
        received.as_ref().unwrap().as_slice(),
        test_data.as_slice(),
        "Received data doesn't match"
    );
}

/// Test sending multiple buffers in sequence.
#[test]
#[serial]
fn test_multiple_buffers_sequence() {
    init();

    let key_expr = unique_key_expr("sequence");
    let num_buffers = 5usize;
    let buffer_size = 256;

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let received_buffers: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received_buffers.clone();
    let received_count = Arc::new(AtomicU64::new(0));
    let received_count_probe = received_count.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // Create receiver first
    let recv_pipeline = gst::Pipeline::new();

    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(zenoh_session.clone())
        .receive_timeout_ms(50)
        .build();

    let fakesink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .unwrap();

    let src_elem: gst::Element = zenohsrc.clone().upcast();
    recv_pipeline.add_many([&src_elem, &fakesink]).unwrap();
    src_elem.link(&fakesink).unwrap();

    let srcpad = zenohsrc.static_pad("src").unwrap();
    srcpad.add_probe(gst::PadProbeType::BUFFER, move |_, probe_info| {
        if let Some(gst::PadProbeData::Buffer(ref buffer)) = probe_info.data {
            if let Ok(map) = buffer.map_readable() {
                received_clone.lock().unwrap().push(map.as_slice().to_vec());
                received_count_probe.fetch_add(1, Ordering::SeqCst);
            }
        }
        gst::PadProbeReturn::Ok
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create sender
    let send_pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder()
        .format(gst::Format::Bytes)
        .build();

    let zenohsink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .build();

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.clone().upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send data
    let appsrc_sender = appsrc.clone();
    let sender_thread = thread::spawn(move || {
        while !stop_clone.load(Ordering::SeqCst) {
            for i in 0..num_buffers {
                let data = generate_test_pattern(i as u32, buffer_size);
                let mut buffer = gst::Buffer::with_size(data.len()).unwrap();
                buffer.get_mut().unwrap().copy_from_slice(0, &data).unwrap();
                if appsrc_sender.push_buffer(buffer).is_err() {
                    return;
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
        appsrc_sender.end_of_stream().ok();
    });

    // Wait for buffers
    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    while received_count.load(Ordering::SeqCst) < num_buffers as u64 && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    // Verify
    let buffers = received_buffers.lock().unwrap();
    assert!(
        buffers.len() >= num_buffers,
        "Expected {} buffers, got {}",
        num_buffers,
        buffers.len()
    );

    for i in 0..num_buffers {
        verify_test_pattern(&buffers[i], i as u32)
            .unwrap_or_else(|e| panic!("Buffer {} verification failed: {}", i, e));
    }
}

/// Test with various buffer sizes.
#[test]
#[serial]
fn test_various_buffer_sizes() {
    init();

    let key_expr = unique_key_expr("sizes");
    // Note: minimum size is 4 due to test pattern header (4-byte index)
    let sizes: Vec<usize> = vec![4, 16, 100, 1024, 10 * 1024];

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let received_buffers: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received_buffers.clone();
    let received_count = Arc::new(AtomicU64::new(0));
    let received_count_probe = received_count.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    let sizes_send = sizes.clone();

    // Create receiver first
    let recv_pipeline = gst::Pipeline::new();

    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(zenoh_session.clone())
        .receive_timeout_ms(50)
        .build();

    let fakesink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .unwrap();

    let src_elem: gst::Element = zenohsrc.clone().upcast();
    recv_pipeline.add_many([&src_elem, &fakesink]).unwrap();
    src_elem.link(&fakesink).unwrap();

    let srcpad = zenohsrc.static_pad("src").unwrap();
    srcpad.add_probe(gst::PadProbeType::BUFFER, move |_, probe_info| {
        if let Some(gst::PadProbeData::Buffer(ref buffer)) = probe_info.data {
            if let Ok(map) = buffer.map_readable() {
                received_clone.lock().unwrap().push(map.as_slice().to_vec());
                received_count_probe.fetch_add(1, Ordering::SeqCst);
            }
        }
        gst::PadProbeReturn::Ok
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create sender
    let send_pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder()
        .format(gst::Format::Bytes)
        .build();

    let zenohsink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .build();

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.clone().upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send data
    let appsrc_sender = appsrc.clone();
    let sender_thread = thread::spawn(move || {
        while !stop_clone.load(Ordering::SeqCst) {
            for (i, &size) in sizes_send.iter().enumerate() {
                let data = generate_test_pattern(i as u32, size);
                let mut buffer = gst::Buffer::with_size(data.len()).unwrap();
                buffer.get_mut().unwrap().copy_from_slice(0, &data).unwrap();
                if appsrc_sender.push_buffer(buffer).is_err() {
                    return;
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
        appsrc_sender.end_of_stream().ok();
    });

    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    while received_count.load(Ordering::SeqCst) < sizes.len() as u64 && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    let buffers = received_buffers.lock().unwrap();
    assert!(
        buffers.len() >= sizes.len(),
        "Expected {} buffers, got {}",
        sizes.len(),
        buffers.len()
    );

    for (i, &expected_size) in sizes.iter().enumerate() {
        assert_eq!(
            buffers[i].len(),
            expected_size,
            "Buffer {} size mismatch",
            i
        );
        verify_test_pattern(&buffers[i], i as u32)
            .unwrap_or_else(|e| panic!("Buffer {} verification failed: {}", i, e));
    }
}

/// Test statistics tracking.
#[test]
#[serial]
fn test_statistics_during_data_flow() {
    init();

    let key_expr = unique_key_expr("stats");
    let num_buffers = 3usize;
    let buffer_size = 512;

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let received_count = Arc::new(AtomicU64::new(0));
    let received_count_probe = received_count.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // Create receiver first
    let recv_pipeline = gst::Pipeline::new();

    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(zenoh_session.clone())
        .receive_timeout_ms(50)
        .build();

    let fakesink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .unwrap();

    let src_elem: gst::Element = zenohsrc.clone().upcast();
    recv_pipeline.add_many([&src_elem, &fakesink]).unwrap();
    src_elem.link(&fakesink).unwrap();

    let srcpad = zenohsrc.static_pad("src").unwrap();
    srcpad.add_probe(gst::PadProbeType::BUFFER, move |_, probe_info| {
        if let Some(gst::PadProbeData::Buffer(_)) = probe_info.data {
            received_count_probe.fetch_add(1, Ordering::SeqCst);
        }
        gst::PadProbeReturn::Ok
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create sender - keep zenohsink accessible for stats
    let send_pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder()
        .format(gst::Format::Bytes)
        .build();

    let zenohsink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .build();

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.clone().upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send data
    let appsrc_sender = appsrc.clone();
    let sender_thread = thread::spawn(move || {
        while !stop_clone.load(Ordering::SeqCst) {
            for i in 0..num_buffers {
                let data = generate_test_pattern(i as u32, buffer_size);
                let mut buffer = gst::Buffer::with_size(data.len()).unwrap();
                buffer.get_mut().unwrap().copy_from_slice(0, &data).unwrap();
                if appsrc_sender.push_buffer(buffer).is_err() {
                    return;
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
        appsrc_sender.end_of_stream().ok();
    });

    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    while received_count.load(Ordering::SeqCst) < num_buffers as u64 && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    // Check statistics
    let bytes_sent = zenohsink.bytes_sent();
    let messages_sent = zenohsink.messages_sent();

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    assert!(
        messages_sent >= num_buffers as u64,
        "messages_sent ({}) should be >= {}",
        messages_sent,
        num_buffers
    );
    assert!(
        bytes_sent >= (num_buffers * buffer_size) as u64,
        "bytes_sent ({}) should be >= {}",
        bytes_sent,
        num_buffers * buffer_size
    );
}
