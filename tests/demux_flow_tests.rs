//! Demux data flow tests for gst-plugin-zenoh.
//!
//! These tests verify that zenohdemux correctly demultiplexes data from
//! multiple Zenoh key expressions into separate GStreamer pads.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gst::prelude::*;
use serial_test::serial;

mod common;
use common::{init, unique_key_expr};

/// Helper to stop a pipeline with timeout
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
}

/// Test that zenohdemux creates a dynamic pad when data arrives
#[test]
#[serial]
fn test_demux_creates_pad_on_data() {
    init();

    let base_key = unique_key_expr("demux_pad");
    let key_expr = format!("{}/*", base_key);
    let specific_key = format!("{}/stream1", base_key);
    let session_group = format!("test_demux_{}", std::process::id());

    let pad_created = Arc::new(AtomicBool::new(false));
    let pad_created_clone = pad_created.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // Create demux pipeline
    let recv_pipeline = gst::Pipeline::new();

    let zenohdemux = gstzenoh::ZenohDemux::builder(&key_expr)
        .session_group(&session_group)
        .receive_timeout_ms(50)
        .build();

    let demux_elem: gst::Element = zenohdemux.clone().upcast();
    recv_pipeline.add(&demux_elem).unwrap();

    // Listen for pad-added signal
    demux_elem.connect_pad_added(move |_, pad: &gst::Pad| {
        pad_created_clone.store(true, Ordering::SeqCst);

        // Create a fakesink and link to the new pad
        if let Some(parent) = pad.parent_element() {
            if let Some(pipeline) = parent.parent() {
                if let Ok(pipeline) = pipeline.downcast::<gst::Pipeline>() {
                    let fakesink = gst::ElementFactory::make("fakesink")
                        .property("sync", false)
                        .property("async", false)
                        .build()
                        .unwrap();

                    pipeline.add(&fakesink).unwrap();
                    fakesink.sync_state_with_parent().unwrap();

                    let sinkpad = fakesink.static_pad("sink").unwrap();
                    let _ = pad.link(&sinkpad);
                }
            }
        }
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create sender with same session group
    let send_pipeline = gst::Pipeline::new();
    let appsrc = gst_app::AppSrc::builder()
        .format(gst::Format::Bytes)
        .build();
    let zenohsink = gstzenoh::ZenohSink::builder(&specific_key)
        .session_group(&session_group)
        .build();

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send data
    let appsrc_sender = appsrc.clone();
    let sender_thread = thread::spawn(move || {
        let mut count = 0;
        while !stop_clone.load(Ordering::SeqCst) {
            let buffer = gst::Buffer::with_size(64).unwrap();
            if appsrc_sender.push_buffer(buffer).is_err() {
                break;
            }
            count += 1;
            thread::sleep(Duration::from_millis(30));
        }
        appsrc_sender.end_of_stream().ok();
        count
    });

    // Wait for pad to be created
    let start = Instant::now();
    let timeout = Duration::from_secs(10);
    while !pad_created.load(Ordering::SeqCst) && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    assert!(
        pad_created.load(Ordering::SeqCst),
        "Pad should have been created"
    );
}

/// Test that demux creates separate pads for different key expressions
#[test]
#[serial]
fn test_demux_multiple_streams() {
    init();

    let base_key = unique_key_expr("demux_multi");
    let key_expr = format!("{}/*", base_key);
    let key1 = format!("{}/video", base_key);
    let key2 = format!("{}/audio", base_key);
    let session_group = format!("test_multi_{}", std::process::id());

    let pads_created = Arc::new(AtomicU64::new(0));
    let pads_clone = pads_created.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone1 = stop_flag.clone();
    let stop_clone2 = stop_flag.clone();

    // Create demux pipeline
    let recv_pipeline = gst::Pipeline::new();

    let zenohdemux = gstzenoh::ZenohDemux::builder(&key_expr)
        .session_group(&session_group)
        .receive_timeout_ms(50)
        .build();

    let demux_elem: gst::Element = zenohdemux.clone().upcast();
    recv_pipeline.add(&demux_elem).unwrap();

    // Listen for pad-added signal
    demux_elem.connect_pad_added(move |_, pad: &gst::Pad| {
        pads_clone.fetch_add(1, Ordering::SeqCst);

        if let Some(parent) = pad.parent_element() {
            if let Some(pipeline) = parent.parent() {
                if let Ok(pipeline) = pipeline.downcast::<gst::Pipeline>() {
                    let fakesink = gst::ElementFactory::make("fakesink")
                        .property("sync", false)
                        .property("async", false)
                        .build()
                        .unwrap();

                    pipeline.add(&fakesink).unwrap();
                    fakesink.sync_state_with_parent().unwrap();

                    let sinkpad = fakesink.static_pad("sink").unwrap();
                    let _ = pad.link(&sinkpad);
                }
            }
        }
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create two senders with same session group
    let send_pipeline1 = gst::Pipeline::new();
    let appsrc1 = gst_app::AppSrc::builder()
        .format(gst::Format::Bytes)
        .build();
    let zenohsink1 = gstzenoh::ZenohSink::builder(&key1)
        .session_group(&session_group)
        .build();

    let appsrc_elem1: gst::Element = appsrc1.clone().upcast();
    let sink_elem1: gst::Element = zenohsink1.upcast();
    send_pipeline1
        .add_many([&appsrc_elem1, &sink_elem1])
        .unwrap();
    appsrc_elem1.link(&sink_elem1).unwrap();

    let send_pipeline2 = gst::Pipeline::new();
    let appsrc2 = gst_app::AppSrc::builder()
        .format(gst::Format::Bytes)
        .build();
    let zenohsink2 = gstzenoh::ZenohSink::builder(&key2)
        .session_group(&session_group)
        .build();

    let appsrc_elem2: gst::Element = appsrc2.clone().upcast();
    let sink_elem2: gst::Element = zenohsink2.upcast();
    send_pipeline2
        .add_many([&appsrc_elem2, &sink_elem2])
        .unwrap();
    appsrc_elem2.link(&sink_elem2).unwrap();

    send_pipeline1.set_state(gst::State::Playing).unwrap();
    send_pipeline2.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send data from both
    let appsrc1_sender = appsrc1.clone();
    let sender1 = thread::spawn(move || {
        while !stop_clone1.load(Ordering::SeqCst) {
            let buffer = gst::Buffer::with_size(64).unwrap();
            if appsrc1_sender.push_buffer(buffer).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        appsrc1_sender.end_of_stream().ok();
    });

    let appsrc2_sender = appsrc2.clone();
    let sender2 = thread::spawn(move || {
        while !stop_clone2.load(Ordering::SeqCst) {
            let buffer = gst::Buffer::with_size(64).unwrap();
            if appsrc2_sender.push_buffer(buffer).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        appsrc2_sender.end_of_stream().ok();
    });

    // Wait for both pads
    let start = Instant::now();
    let timeout = Duration::from_secs(10);
    while pads_created.load(Ordering::SeqCst) < 2 && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline1.set_state(gst::State::Null);
    let _ = send_pipeline2.set_state(gst::State::Null);
    sender1.join().expect("Sender 1 panicked");
    sender2.join().expect("Sender 2 panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    assert!(
        pads_created.load(Ordering::SeqCst) >= 2,
        "Expected at least 2 pads, got {}",
        pads_created.load(Ordering::SeqCst)
    );
}

/// Test that demux delivers data to the correct pad
#[test]
#[serial]
fn test_demux_data_routing() {
    init();

    let base_key = unique_key_expr("demux_route");
    let key_expr = format!("{}/*", base_key);
    let key1 = format!("{}/stream1", base_key);
    let session_group = format!("test_route_{}", std::process::id());

    let received_data: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received_data.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // Create demux pipeline
    let recv_pipeline = gst::Pipeline::new();

    let zenohdemux = gstzenoh::ZenohDemux::builder(&key_expr)
        .session_group(&session_group)
        .receive_timeout_ms(50)
        .build();

    let demux_elem: gst::Element = zenohdemux.clone().upcast();
    recv_pipeline.add(&demux_elem).unwrap();

    // Listen for pad-added and capture data
    demux_elem.connect_pad_added(move |_, pad: &gst::Pad| {
        let received = received_clone.clone();

        if let Some(parent) = pad.parent_element() {
            if let Some(pipeline) = parent.parent() {
                if let Ok(pipeline) = pipeline.downcast::<gst::Pipeline>() {
                    let fakesink = gst::ElementFactory::make("fakesink")
                        .property("sync", false)
                        .property("async", false)
                        .build()
                        .unwrap();

                    pipeline.add(&fakesink).unwrap();

                    // Add probe to capture data
                    let sinkpad = fakesink.static_pad("sink").unwrap();
                    sinkpad.add_probe(gst::PadProbeType::BUFFER, move |_, probe_info| {
                        if let Some(gst::PadProbeData::Buffer(ref buffer)) = probe_info.data {
                            if let Ok(map) = buffer.map_readable() {
                                received.lock().unwrap().push(map.to_vec());
                            }
                        }
                        gst::PadProbeReturn::Ok
                    });

                    fakesink.sync_state_with_parent().unwrap();
                    let _ = pad.link(&sinkpad);
                }
            }
        }
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create sender with specific pattern data
    let send_pipeline = gst::Pipeline::new();
    let appsrc = gst_app::AppSrc::builder()
        .format(gst::Format::Bytes)
        .build();
    let zenohsink = gstzenoh::ZenohSink::builder(&key1)
        .session_group(&session_group)
        .build();

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send unique pattern data
    let test_pattern = vec![0xDE, 0xAD, 0xBE, 0xEF, 0x12, 0x34, 0x56, 0x78];
    let appsrc_sender = appsrc.clone();
    let pattern = test_pattern.clone();

    let sender_thread = thread::spawn(move || {
        let mut count = 0;
        while !stop_clone.load(Ordering::SeqCst) && count < 10 {
            let mut buffer = gst::Buffer::with_size(pattern.len()).unwrap();
            {
                let buffer_mut = buffer.get_mut().unwrap();
                buffer_mut.copy_from_slice(0, &pattern).unwrap();
            }
            if appsrc_sender.push_buffer(buffer).is_err() {
                break;
            }
            count += 1;
            thread::sleep(Duration::from_millis(50));
        }
        appsrc_sender.end_of_stream().ok();
        count
    });

    // Wait for data
    let start = Instant::now();
    let timeout = Duration::from_secs(10);
    while received_data.lock().unwrap().is_empty() && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    // Verify we received the correct pattern
    let received = received_data.lock().unwrap();
    assert!(!received.is_empty(), "Should have received data");
    assert_eq!(
        received[0], test_pattern,
        "Received data should match sent pattern"
    );
}
