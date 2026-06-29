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
use std::time::Duration;

use zenoh::Wait;

static CAT: LazyLock<gst::DebugCategory> = LazyLock::new(|| {
    gst::DebugCategory::new(
        "zenohsession",
        gst::DebugColorFlags::empty(),
        Some("Zenoh Session Registry"),
    )
});

/// Bound on closing the last reference to a shared session so a stalled close
/// can't hang element teardown.
const SESSION_CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

/// A reference-counted shared session.
struct SessionEntry {
    session: zenoh::Session,
    /// Config path the session was first opened with, used to warn on mismatch.
    config_path: Option<String>,
    /// Number of live element references; the session is closed when it reaches 0.
    refcount: usize,
}

/// Global registry of shared sessions by group name.
static SESSION_REGISTRY: LazyLock<Mutex<HashMap<String, SessionEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Get or create a shared session for a named group, bumping its reference count.
///
/// This is used internally by elements when the `session-group` property is set.
/// Sessions are cached by group name and reused when multiple elements specify
/// the same group. Each successful call must be balanced by a [`release_session`]
/// call when the element tears down.
///
/// The session is opened *outside* the registry lock (double-checked insert) so
/// concurrent resolution of different groups does not serialize on a blocking
/// `zenoh::open`. If a group is reused with a different `config`, a warning is
/// logged and the existing session is returned.
pub(crate) fn get_or_create_session(
    group: &str,
    config_path: Option<&str>,
) -> Result<zenoh::Session, zenoh::Error> {
    // Fast path: an entry already exists — bump its refcount and return a clone.
    {
        let mut registry = SESSION_REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = registry.get_mut(group) {
            if entry.config_path.as_deref() != config_path {
                gst::warning!(
                    CAT,
                    "Session group '{}' reused with a different config ({:?} vs existing {:?}); \
                     the existing session is kept and the new config is ignored",
                    group,
                    config_path,
                    entry.config_path
                );
            }
            entry.refcount += 1;
            return Ok(entry.session.clone());
        }
    }

    // Open the session outside the lock so other groups don't serialize behind it.
    let config = match config_path {
        Some(path) => zenoh::Config::from_file(path)?,
        None => zenoh::Config::default(),
    };
    let session = zenoh::open(config).wait()?;

    // Re-lock and double-check: another thread may have created the group meanwhile.
    let mut registry = SESSION_REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = registry.get_mut(group) {
        entry.refcount += 1;
        let winner = entry.session.clone();
        drop(registry);
        // Discard the redundant session we just opened.
        let _ = session.close().wait();
        return Ok(winner);
    }
    registry.insert(
        group.to_string(),
        SessionEntry {
            session: session.clone(),
            config_path: config_path.map(String::from),
            refcount: 1,
        },
    );
    Ok(session)
}

/// Release one reference to a shared session group, closing the session when the
/// last reference is dropped. Balances [`get_or_create_session`].
pub(crate) fn release_session(group: &str) {
    // Decrement under the lock; if it hit zero, take ownership of the session and
    // close it *outside* the lock so a slow close can't block other resolvers.
    let to_close = {
        let mut registry = SESSION_REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        match registry.get_mut(group) {
            Some(entry) => {
                entry.refcount = entry.refcount.saturating_sub(1);
                if entry.refcount == 0 {
                    registry.remove(group).map(|e| e.session)
                } else {
                    None
                }
            }
            None => None,
        }
    };

    if let Some(session) = to_close {
        gst::debug!(CAT, "Closing last reference to session group '{}'", group);
        // Bound the close so teardown can't hang on a stalled session.
        match crate::utils::call_with_timeout(SESSION_CLOSE_TIMEOUT, move || session.close().wait())
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => gst::warning!(CAT, "Error closing shared session '{}': {}", group, e),
            Err(_) => gst::warning!(
                CAT,
                "Timed out closing shared session '{}'; detaching",
                group
            ),
        }
    }
}

/// Whether a session group is currently present in the registry (for tests).
#[cfg(test)]
pub(crate) fn registry_contains(group: &str) -> bool {
    SESSION_REGISTRY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(group)
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

        // Balance the two references.
        release_session("test-reuse-group");
        release_session("test-reuse-group");
    }

    #[test]
    fn test_different_groups_different_sessions() {
        let session1 =
            get_or_create_session("test-group-x", None).expect("Failed to create session");
        let session2 =
            get_or_create_session("test-group-y", None).expect("Failed to create session");

        // Should be different sessions
        assert_ne!(session1.zid(), session2.zid());

        release_session("test-group-x");
        release_session("test-group-y");
    }

    #[test]
    fn test_session_refcount_release_closes_on_last() {
        let group = "test-refcount-lifecycle";
        assert!(!registry_contains(group));

        // Two references to the same group.
        let s1 = get_or_create_session(group, None).expect("open 1");
        let s2 = get_or_create_session(group, None).expect("open 2");
        assert_eq!(s1.zid(), s2.zid());
        assert!(
            registry_contains(group),
            "entry should exist while referenced"
        );

        // First release keeps it alive (refcount 2 -> 1).
        release_session(group);
        assert!(
            registry_contains(group),
            "entry must survive while a reference remains"
        );

        // Last release removes it (refcount -> 0) — no leak.
        release_session(group);
        assert!(
            !registry_contains(group),
            "entry must be removed once the last reference is released"
        );
    }
}
