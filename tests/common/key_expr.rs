//! Unique key expression generator for test isolation.

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
