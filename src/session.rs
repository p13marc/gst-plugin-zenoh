// SPDX-License-Identifier: MPL-2.0

//! Session sharing support for gst-plugin-zenoh
//!
//! This module provides functionality to share Zenoh sessions across multiple
//! GStreamer elements, reducing network overhead and resource usage.
//!
//! ## Usage
//!
//! ### Using session-group property (gst-launch compatible)
//!
//! ```bash
//! # Elements with same session-group share a session
//! gst-launch-1.0 \
//!   videotestsrc ! zenohsink key-expr=demo/video session-group=main \
//!   audiotestsrc ! zenohsink key-expr=demo/audio session-group=main
//! ```
//!
//! ### Using shared session in Rust
//!
//! ```ignore
//! use gstzenoh::ZenohSink;
//! use zenoh::Wait;
//!
//! // Create a Zenoh session (already Clone/Arc-based internally)
//! let session = zenoh::open(zenoh::Config::default()).wait()?;
//!
//! // Use it in multiple elements - just clone the session
//! let sink1 = ZenohSink::builder("demo/video")
//!     .session(session.clone())
//!     .build();
//!
//! let sink2 = ZenohSink::builder("demo/audio")
//!     .session(session)
//!     .build();
//! ```

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use zenoh::Wait;

/// Global registry of shared sessions by group name.
///
/// Sessions are stored directly since `zenoh::Session` is already Arc-based
/// internally and supports Clone.
static SESSION_REGISTRY: LazyLock<Mutex<HashMap<String, zenoh::Session>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Get or create a shared session for a named group.
///
/// This is used internally by elements when the `session-group` property is set.
/// Sessions are cached by group name and reused when multiple elements specify
/// the same group.
///
/// # Arguments
///
/// * `group` - The session group name
/// * `config_path` - Optional path to a Zenoh configuration file
///
/// # Returns
///
/// A `zenoh::Session` that may be shared with other elements in the same group.
///
/// # Note
///
/// If a session already exists for the group, the `config_path` is ignored and
/// the existing session is returned. This means the first element to start with
/// a given group name determines the configuration for that group.
pub(crate) fn get_or_create_session(
    group: &str,
    config_path: Option<&str>,
) -> Result<zenoh::Session, zenoh::Error> {
    let mut registry = SESSION_REGISTRY.lock().unwrap();

    // Check if session already exists for this group
    if let Some(session) = registry.get(group) {
        // Session is Arc-based, clone is cheap
        return Ok(session.clone());
    }

    // Create new session
    let config = match config_path {
        Some(path) => zenoh::Config::from_file(path)?,
        None => zenoh::Config::default(),
    };
    let session = zenoh::open(config).wait()?;

    // Store in registry
    registry.insert(group.to_string(), session.clone());

    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_group_reuse() {
        let session1 =
            get_or_create_session("test-reuse-group", None).expect("Failed to create session");
        let session2 =
            get_or_create_session("test-reuse-group", None).expect("Failed to get session");

        // Should be the same session (same zid)
        assert_eq!(session1.zid(), session2.zid());
    }

    #[test]
    fn test_different_groups_different_sessions() {
        let session1 =
            get_or_create_session("test-group-x", None).expect("Failed to create session");
        let session2 =
            get_or_create_session("test-group-y", None).expect("Failed to create session");

        // Should be different sessions
        assert_ne!(session1.zid(), session2.zid());
    }
}
