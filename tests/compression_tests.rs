//! Compression round-trip tests for gst-plugin-zenoh.
//!
//! These tests verify that data compressed by zenohsink is correctly
//! decompressed by zenohsrc when compression features are enabled.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use gst::prelude::*;
use serial_test::serial;
use zenoh::Wait;

#[cfg(any(
    feature = "compression-zstd",
    feature = "compression-lz4",
    feature = "compression-gzip"
))]
use gstzenoh::compression::CompressionType;

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

/// Generate test data with a recognizable pattern
fn generate_test_data(size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);
    for i in 0..size {
        data.push(((i % 256) ^ ((i / 256) % 256)) as u8);
    }
    data
}

/// Verify test data matches expected pattern
fn verify_test_data(data: &[u8], expected_size: usize) -> bool {
    if data.len() != expected_size {
        return false;
    }
    for i in 0..expected_size {
        let expected = ((i % 256) ^ ((i / 256) % 256)) as u8;
        if data[i] != expected {
            return false;
        }
    }
    true
}

/// Test helper: send and receive data through zenoh with compression
#[cfg(any(
    feature = "compression-zstd",
    feature = "compression-lz4",
    feature = "compression-gzip"
))]
fn compression_roundtrip_test(compression: CompressionType, level: i32, data_size: usize) {
    init();

    let key_expr = unique_key_expr(&format!("comp_{:?}", compression));

    let zenoh_session = zenoh::open(zenoh::Config::default())
        .wait()
        .expect("Failed to open Zenoh session");

    let received_data: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let received_clone = received_data.clone();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

    // Create receiver pipeline
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

    // Capture received data
    let srcpad = zenohsrc.static_pad("src").unwrap();
    srcpad.add_probe(gst::PadProbeType::BUFFER, move |_, probe_info| {
        if let Some(gst::PadProbeData::Buffer(ref buffer)) = probe_info.data {
            let map = buffer.map_readable().unwrap();
            *received_clone.lock().unwrap() = Some(map.to_vec());
        }
        gst::PadProbeReturn::Remove
    });

    recv_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(500));

    // Create sender pipeline with compression
    let send_pipeline = gst::Pipeline::new();

    let appsrc = gst_app::AppSrc::builder().format(gst::Format::Bytes).build();

    let zenohsink = gstzenoh::ZenohSink::builder(&key_expr)
        .session(zenoh_session.clone())
        .build();

    let sink_elem: gst::Element = zenohsink.clone().upcast();
    sink_elem.set_property("compression", compression);
    sink_elem.set_property("compression-level", level);

    let appsrc_elem: gst::Element = appsrc.clone().upcast();
    send_pipeline.add_many([&appsrc_elem, &sink_elem]).unwrap();
    appsrc_elem.link(&sink_elem).unwrap();

    send_pipeline.set_state(gst::State::Playing).unwrap();
    thread::sleep(Duration::from_millis(100));

    // Generate and send test data
    let test_data = generate_test_data(data_size);
    let appsrc_sender = appsrc.clone();
    let test_data_clone = test_data.clone();

    let sender_thread = thread::spawn(move || {
        let mut sent = 0;
        while !stop_clone.load(Ordering::SeqCst) {
            let mut buffer = gst::Buffer::with_size(test_data_clone.len()).unwrap();
            {
                let buffer_mut = buffer.get_mut().unwrap();
                buffer_mut.copy_from_slice(0, &test_data_clone).unwrap();
            }

            if appsrc_sender.push_buffer(buffer).is_err() {
                break;
            }
            sent += 1;
            thread::sleep(Duration::from_millis(30));
        }
        appsrc_sender.end_of_stream().ok();
        sent
    });

    // Wait for data
    let start = Instant::now();
    let timeout = Duration::from_secs(10);
    while received_data.lock().unwrap().is_none() && start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(50));
    }

    stop_flag.store(true, Ordering::SeqCst);
    let _ = send_pipeline.set_state(gst::State::Null);
    sender_thread.join().expect("Sender thread panicked");
    stop_pipeline_with_timeout(&recv_pipeline, Duration::from_secs(1));

    // Verify received data
    let received = received_data.lock().unwrap();
    assert!(
        received.is_some(),
        "No data received for {:?} compression",
        compression
    );
    let received = received.as_ref().unwrap();
    assert!(
        verify_test_data(received, data_size),
        "Data mismatch for {:?} compression: expected {} bytes, got {}",
        compression,
        data_size,
        received.len()
    );
}

/// Test no compression (baseline)
#[cfg(any(
    feature = "compression-zstd",
    feature = "compression-lz4",
    feature = "compression-gzip"
))]
#[test]
#[serial]
fn test_no_compression_roundtrip() {
    compression_roundtrip_test(CompressionType::None, 1, 1024);
}

/// Test zstd compression (requires compression-zstd feature)
#[cfg(feature = "compression-zstd")]
#[test]
#[serial]
fn test_zstd_compression_roundtrip() {
    compression_roundtrip_test(CompressionType::Zstd, 3, 4096);
}

/// Test zstd compression with level 1 (fast)
#[cfg(feature = "compression-zstd")]
#[test]
#[serial]
fn test_zstd_compression_level_1() {
    compression_roundtrip_test(CompressionType::Zstd, 1, 8192);
}

/// Test zstd compression with level 9 (high compression)
#[cfg(feature = "compression-zstd")]
#[test]
#[serial]
fn test_zstd_compression_level_9() {
    compression_roundtrip_test(CompressionType::Zstd, 9, 8192);
}

/// Test lz4 compression (requires compression-lz4 feature)
#[cfg(feature = "compression-lz4")]
#[test]
#[serial]
fn test_lz4_compression_roundtrip() {
    compression_roundtrip_test(CompressionType::Lz4, 1, 4096);
}

/// Test gzip compression (requires compression-gzip feature)
#[cfg(feature = "compression-gzip")]
#[test]
#[serial]
fn test_gzip_compression_roundtrip() {
    compression_roundtrip_test(CompressionType::Gzip, 6, 4096);
}

/// Test compression with large data
#[cfg(feature = "compression-zstd")]
#[test]
#[serial]
fn test_zstd_large_data() {
    compression_roundtrip_test(CompressionType::Zstd, 3, 64 * 1024); // 64KB
}
