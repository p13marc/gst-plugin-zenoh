//! Test helper utilities for gst-plugin-zenoh integration tests.
//!
//! Provides utilities for generating unique key expressions and test patterns.

use std::sync::atomic::{AtomicU64, Ordering};

/// Counter for generating unique key expressions
static KEY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a unique key expression for test isolation.
///
/// Each test should use a unique key expression to avoid interference
/// from other tests running in parallel or residual data from previous runs.
pub fn unique_key_expr(prefix: &str) -> String {
    let count = KEY_COUNTER.fetch_add(1, Ordering::SeqCst);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("test/{}/{}/{}", prefix, timestamp, count)
}

/// Generate test data with a recognizable pattern.
///
/// Creates a buffer filled with a pattern based on the index,
/// making it easy to verify data integrity after transmission.
///
/// Note: The pattern starts with a 4-byte header containing the index,
/// so the minimum effective size is 4 bytes.
pub fn generate_test_pattern(index: u32, size: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size);

    // First 4 bytes: index as big-endian
    data.extend_from_slice(&index.to_be_bytes());

    // Remaining bytes: repeating pattern based on index
    let pattern_byte = (index % 256) as u8;
    while data.len() < size {
        data.push(pattern_byte);
    }

    data
}

/// Verify that received data matches the expected test pattern.
pub fn verify_test_pattern(data: &[u8], expected_index: u32) -> Result<(), String> {
    if data.len() < 4 {
        return Err(format!("Buffer too small: {} bytes", data.len()));
    }

    // Check index in first 4 bytes
    let received_index = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    if received_index != expected_index {
        return Err(format!(
            "Index mismatch: expected {}, got {}",
            expected_index, received_index
        ));
    }

    // Check pattern in remaining bytes
    let expected_byte = (expected_index % 256) as u8;
    for (i, &byte) in data[4..].iter().enumerate() {
        if byte != expected_byte {
            return Err(format!(
                "Pattern mismatch at offset {}: expected {}, got {}",
                i + 4,
                expected_byte,
                byte
            ));
        }
    }

    Ok(())
}
