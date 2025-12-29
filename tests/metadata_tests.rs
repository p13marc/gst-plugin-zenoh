//! Metadata preservation tests for gst-plugin-zenoh.
//!
//! These tests verify that GStreamer buffer metadata (PTS, DTS, duration, flags)
//! is correctly transmitted via Zenoh attachments and restored on the receiving side.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gst::prelude::*;
use serial_test::serial;
use zenoh::Wait;

mod common;
use common::{init, unique_key_expr};

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
}

/// Captured buffer metadata for verification
#[derive(Debug, Clone)]
struct CapturedMeta {
    pts: Option<gst::ClockTime>,
    dts: Option<gst::ClockTime>,
    duration: Option<gst::ClockTime>,
    offset: u64,
    offset_end: u64,
}

/// Test that PTS (Presentation Timestamp) is preserved through transmission.
#[test]
#[serial]
fn test_pts_preservation() {
    init();

    let key_expr = unique_key_expr("pts");

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let received_meta: Arc<Mutex<Option<CapturedMeta>>> = Arc::new(Mutex::new(None));
    let received_clone = received_meta.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // Create receiver first with apply-buffer-meta enabled
    let recv_pipeline = gst::Pipeline::new();

    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(zenoh_session.clone())
        .receive_timeout_ms(50)
        .apply_buffer_meta(true)
        .build();

    let fakesink = gst::ElementFactory::make("fakesink")
        .property("sync", false)
        .build()
        .unwrap();

    let src_elem: gst::Element = zenohsrc.clone().upcast();
    recv_pipeline.add_many([&src_elem, &fakesink]).unwrap();
    src_elem.link(&fakesink).unwrap();

    // Capture metadata from received buffer
    let srcpad = zenohsrc.static_pad("src").unwrap();
    srcpad.add_probe(gst::PadProbeType::BUFFER, move |_, probe_info| {
        if let Some(gst::PadProbeData::Buffer(ref buffer)) = probe_info.data {
            *received_clone.lock().unwrap() = Some(CapturedMeta {
                pts: buffer.pts(),
                dts: buffer.dts(),
                duration: buffer.duration(),
                offset: buffer.offset(),
                offset_end: buffer.offset_end(),
                
            });
        }
        gst::PadProbeReturn::Remove
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create sender with send-buffer-meta enabled
    let send_pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder().format(gst::Format::Time).build();

    let zenohsink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .send_buffer_meta(true)
        .build();

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.clone().upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send buffer with specific PTS
    let expected_pts = gst::ClockTime::from_mseconds(1234);
    let appsrc_sender = appsrc.clone();
    let sender_thread = thread::spawn(move || {
        while !stop_clone.load(Ordering::SeqCst) {
            let mut buffer = gst::Buffer::with_size(64).unwrap();
            {
                let buffer_ref = buffer.get_mut().unwrap();
                buffer_ref.set_pts(expected_pts);
            }

            if appsrc_sender.push_buffer(buffer).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        appsrc_sender.end_of_stream().ok();
    });

    // Wait for data
    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    while received_meta.lock().unwrap().is_none() && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    // Verify PTS was preserved
    let meta = received_meta.lock().unwrap();
    assert!(meta.is_some(), "No buffer received");
    let meta = meta.as_ref().unwrap();
    assert_eq!(
        meta.pts,
        Some(expected_pts),
        "PTS mismatch: expected {:?}, got {:?}",
        Some(expected_pts),
        meta.pts
    );
}

/// Test that DTS (Decode Timestamp) is preserved through transmission.
#[test]
#[serial]
fn test_dts_preservation() {
    init();

    let key_expr = unique_key_expr("dts");

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let received_meta: Arc<Mutex<Option<CapturedMeta>>> = Arc::new(Mutex::new(None));
    let received_clone = received_meta.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // Create receiver
    let recv_pipeline = gst::Pipeline::new();

    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(zenoh_session.clone())
        .receive_timeout_ms(50)
        .apply_buffer_meta(true)
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
            *received_clone.lock().unwrap() = Some(CapturedMeta {
                pts: buffer.pts(),
                dts: buffer.dts(),
                duration: buffer.duration(),
                offset: buffer.offset(),
                offset_end: buffer.offset_end(),
                
            });
        }
        gst::PadProbeReturn::Remove
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create sender
    let send_pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder().format(gst::Format::Time).build();

    let zenohsink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .send_buffer_meta(true)
        .build();

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.clone().upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send buffer with specific DTS
    let expected_pts = gst::ClockTime::from_mseconds(1000);
    let expected_dts = gst::ClockTime::from_mseconds(900); // DTS before PTS for B-frames
    let appsrc_sender = appsrc.clone();
    let sender_thread = thread::spawn(move || {
        while !stop_clone.load(Ordering::SeqCst) {
            let mut buffer = gst::Buffer::with_size(64).unwrap();
            {
                let buffer_ref = buffer.get_mut().unwrap();
                buffer_ref.set_pts(expected_pts);
                buffer_ref.set_dts(expected_dts);
            }

            if appsrc_sender.push_buffer(buffer).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        appsrc_sender.end_of_stream().ok();
    });

    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    while received_meta.lock().unwrap().is_none() && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    let meta = received_meta.lock().unwrap();
    assert!(meta.is_some(), "No buffer received");
    let meta = meta.as_ref().unwrap();
    assert_eq!(
        meta.dts,
        Some(expected_dts),
        "DTS mismatch: expected {:?}, got {:?}",
        Some(expected_dts),
        meta.dts
    );
}

/// Test that duration is preserved through transmission.
#[test]
#[serial]
fn test_duration_preservation() {
    init();

    let key_expr = unique_key_expr("duration");

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let received_meta: Arc<Mutex<Option<CapturedMeta>>> = Arc::new(Mutex::new(None));
    let received_clone = received_meta.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // Create receiver
    let recv_pipeline = gst::Pipeline::new();

    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(zenoh_session.clone())
        .receive_timeout_ms(50)
        .apply_buffer_meta(true)
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
            *received_clone.lock().unwrap() = Some(CapturedMeta {
                pts: buffer.pts(),
                dts: buffer.dts(),
                duration: buffer.duration(),
                offset: buffer.offset(),
                offset_end: buffer.offset_end(),
                
            });
        }
        gst::PadProbeReturn::Remove
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create sender
    let send_pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder().format(gst::Format::Time).build();

    let zenohsink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .send_buffer_meta(true)
        .build();

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.clone().upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send buffer with specific duration (e.g., 1 frame at 30fps = ~33.33ms)
    let expected_duration = gst::ClockTime::from_nseconds(33_333_333);
    let appsrc_sender = appsrc.clone();
    let sender_thread = thread::spawn(move || {
        while !stop_clone.load(Ordering::SeqCst) {
            let mut buffer = gst::Buffer::with_size(64).unwrap();
            {
                let buffer_ref = buffer.get_mut().unwrap();
                buffer_ref.set_pts(gst::ClockTime::ZERO);
                buffer_ref.set_duration(expected_duration);
            }

            if appsrc_sender.push_buffer(buffer).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        appsrc_sender.end_of_stream().ok();
    });

    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    while received_meta.lock().unwrap().is_none() && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    let meta = received_meta.lock().unwrap();
    assert!(meta.is_some(), "No buffer received");
    let meta = meta.as_ref().unwrap();
    assert_eq!(
        meta.duration,
        Some(expected_duration),
        "Duration mismatch: expected {:?}, got {:?}",
        Some(expected_duration),
        meta.duration
    );
}

/// Test that all metadata fields are preserved in a single transmission.
#[test]
#[serial]
fn test_full_metadata_preservation() {
    init();

    let key_expr = unique_key_expr("fullmeta");

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let received_meta: Arc<Mutex<Option<CapturedMeta>>> = Arc::new(Mutex::new(None));
    let received_clone = received_meta.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // Create receiver
    let recv_pipeline = gst::Pipeline::new();

    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(zenoh_session.clone())
        .receive_timeout_ms(50)
        .apply_buffer_meta(true)
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
            *received_clone.lock().unwrap() = Some(CapturedMeta {
                pts: buffer.pts(),
                dts: buffer.dts(),
                duration: buffer.duration(),
                offset: buffer.offset(),
                offset_end: buffer.offset_end(),
                
            });
        }
        gst::PadProbeReturn::Remove
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create sender
    let send_pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder().format(gst::Format::Time).build();

    let zenohsink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .send_buffer_meta(true)
        .build();

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.clone().upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send buffer with all metadata set
    let expected_pts = gst::ClockTime::from_mseconds(5000);
    let expected_dts = gst::ClockTime::from_mseconds(4900);
    let expected_duration = gst::ClockTime::from_mseconds(40);
    let expected_offset = 42u64;
    let expected_offset_end = 43u64;

    let appsrc_sender = appsrc.clone();
    let sender_thread = thread::spawn(move || {
        while !stop_clone.load(Ordering::SeqCst) {
            let mut buffer = gst::Buffer::with_size(128).unwrap();
            {
                let buffer_ref = buffer.get_mut().unwrap();
                buffer_ref.set_pts(expected_pts);
                buffer_ref.set_dts(expected_dts);
                buffer_ref.set_duration(expected_duration);
                buffer_ref.set_offset(expected_offset);
                buffer_ref.set_offset_end(expected_offset_end);
            }

            if appsrc_sender.push_buffer(buffer).is_err() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        appsrc_sender.end_of_stream().ok();
    });

    let start = Instant::now();
    let timeout = Duration::from_secs(5);
    while received_meta.lock().unwrap().is_none() && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    let meta = received_meta.lock().unwrap();
    assert!(meta.is_some(), "No buffer received");
    let meta = meta.as_ref().unwrap();

    assert_eq!(meta.pts, Some(expected_pts), "PTS mismatch");
    assert_eq!(meta.dts, Some(expected_dts), "DTS mismatch");
    assert_eq!(meta.duration, Some(expected_duration), "Duration mismatch");
    assert_eq!(meta.offset, expected_offset, "Offset mismatch");
    assert_eq!(meta.offset_end, expected_offset_end, "Offset-end mismatch");
}

/// Test that metadata is NOT applied when apply-buffer-meta is disabled.
#[test]
#[serial]
fn test_metadata_disabled_receiver() {
    init();

    let key_expr = unique_key_expr("nometa");

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let received_meta: Arc<Mutex<Option<CapturedMeta>>> = Arc::new(Mutex::new(None));
    let received_clone = received_meta.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // Create receiver with apply-buffer-meta DISABLED
    let recv_pipeline = gst::Pipeline::new();

    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(zenoh_session.clone())
        .receive_timeout_ms(50)
        .apply_buffer_meta(false) // Explicitly disabled
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
            *received_clone.lock().unwrap() = Some(CapturedMeta {
                pts: buffer.pts(),
                dts: buffer.dts(),
                duration: buffer.duration(),
                offset: buffer.offset(),
                offset_end: buffer.offset_end(),
                
            });
        }
        gst::PadProbeReturn::Remove
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create sender with send-buffer-meta enabled
    let send_pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder().format(gst::Format::Time).build();

    let zenohsink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .send_buffer_meta(true)
        .build();

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.clone().upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send buffer with metadata - continue until received or timeout
    let sent_pts = gst::ClockTime::from_mseconds(9999);
    let appsrc_sender = appsrc.clone();
    let sender_thread = thread::spawn(move || {
        let mut count = 0;
        while !stop_clone.load(Ordering::SeqCst) {
            let mut buffer = gst::Buffer::with_size(64).unwrap();
            {
                let buffer_ref = buffer.get_mut().unwrap();
                buffer_ref.set_pts(sent_pts);
                buffer_ref.set_duration(gst::ClockTime::from_mseconds(33));
            }

            if appsrc_sender.push_buffer(buffer).is_err() {
                break;
            }
            count += 1;
            thread::sleep(Duration::from_millis(30));
        }
        appsrc_sender.end_of_stream().ok();
        count
    });

    // Wait longer for data to be received (Zenoh session establishment can take time)
    let start = Instant::now();
    let timeout = Duration::from_secs(10);
    while received_meta.lock().unwrap().is_none() && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    // Verify that sent PTS was NOT applied (should be None or different)
    let meta = received_meta.lock().unwrap();
    assert!(meta.is_some(), "No buffer received");
    let meta = meta.as_ref().unwrap();

    // When apply-buffer-meta is false, the PTS should NOT be the one we sent
    // It will either be None or set by zenohsrc from Zenoh timestamp
    assert_ne!(
        meta.pts,
        Some(sent_pts),
        "PTS should NOT match sent value when apply-buffer-meta is disabled"
    );
}

/// Test sequential buffers with increasing timestamps.
#[test]
#[serial]
fn test_sequential_timestamps() {
    init();

    let key_expr = unique_key_expr("seqts");
    let num_buffers = 5usize;

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let received_pts_list: Arc<Mutex<Vec<Option<gst::ClockTime>>>> =
        Arc::new(Mutex::new(Vec::new()));
    let received_clone = received_pts_list.clone();
    let received_count = Arc::new(AtomicU64::new(0));
    let received_count_probe = received_count.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // Create receiver
    let recv_pipeline = gst::Pipeline::new();

    let zenohsrc = gstzenoh::ZenohSrc::builder(&key_expr)
        .session(zenoh_session.clone())
        .receive_timeout_ms(50)
        .apply_buffer_meta(true)
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
            received_clone.lock().unwrap().push(buffer.pts());
            received_count_probe.fetch_add(1, Ordering::SeqCst);
        }
        gst::PadProbeReturn::Ok
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create sender
    let send_pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder().format(gst::Format::Time).build();

    let zenohsink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .send_buffer_meta(true)
        .build();

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    let sink_elem: gst::Element = zenohsink.clone().upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Send buffers with sequential timestamps
    let frame_duration_ns = 33_333_333u64; // ~30fps
    let appsrc_sender = appsrc.clone();
    let sender_thread = thread::spawn(move || {
        while !stop_clone.load(Ordering::SeqCst) {
            for i in 0..num_buffers {
                let pts = gst::ClockTime::from_nseconds(i as u64 * frame_duration_ns);
                let mut buffer = gst::Buffer::with_size(64).unwrap();
                {
                    let buffer_ref = buffer.get_mut().unwrap();
                    buffer_ref.set_pts(pts);
                }

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

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    // Verify sequential timestamps
    let pts_list = received_pts_list.lock().unwrap();
    assert!(
        pts_list.len() >= num_buffers,
        "Expected {} buffers, got {}",
        num_buffers,
        pts_list.len()
    );

    for i in 0..num_buffers {
        let expected_pts = gst::ClockTime::from_nseconds(i as u64 * frame_duration_ns);
        assert_eq!(
            pts_list[i],
            Some(expected_pts),
            "Buffer {} PTS mismatch: expected {:?}, got {:?}",
            i,
            Some(expected_pts),
            pts_list[i]
        );
    }
}
