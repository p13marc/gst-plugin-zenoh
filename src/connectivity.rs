// SPDX-License-Identifier: MPL-2.0

//! Session connectivity observability.
//!
//! Spawns a background Zenoh transport-events listener that tracks whether the
//! session currently has any open transport, mirroring the result into a shared
//! `connected` flag and posting a [`CONNECTIVITY_MESSAGE`] bus message (plus
//! notifying the `connected` property) on each transition.
//!
//! This only *observes* connectivity — Zenoh re-establishes transports on its
//! own, so the plugin does not implement reconnection; it reports it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use gst::prelude::*;
use zenoh::Wait;
use zenoh::sample::SampleKind;

/// Name of the element bus message posted on connectivity transitions. Carries a
/// boolean `connected` field.
pub(crate) const CONNECTIVITY_MESSAGE: &str = "zenoh-connectivity-changed";

/// Spawn a background transport-events listener on `session`.
///
/// `connected` is kept in sync with "any transport currently open"; on each
/// transition the `connected` property is notified and a [`CONNECTIVITY_MESSAGE`]
/// element message is posted on `element`'s bus. The listener runs in the
/// background and lives for the session's lifetime, so it stops when the session
/// is dropped. `history(true)` replays the transports already open when the
/// listener is declared, so the initial `connected` value is correct.
pub(crate) fn spawn_listener(
    session: &zenoh::Session,
    element: &gst::Element,
    connected: Arc<AtomicBool>,
) -> zenoh::Result<()> {
    let element_weak = element.downgrade();
    // Exact count of open transports; `connected` is derived as `count > 0`. The
    // count update and the `connected` swap happen under one lock so transitions
    // stay consistent even if the callback is invoked concurrently.
    let open_count = Arc::new(Mutex::new(0usize));

    session
        .info()
        .transport_events_listener()
        .history(true)
        .callback(move |event| {
            let transition = {
                let mut count = open_count.lock().unwrap_or_else(|e| e.into_inner());
                match event.kind() {
                    SampleKind::Put => *count += 1,
                    SampleKind::Delete => *count = count.saturating_sub(1),
                }
                let now = *count > 0;
                if connected.swap(now, Ordering::SeqCst) == now {
                    None
                } else {
                    Some(now)
                }
            };

            let Some(now_connected) = transition else {
                return;
            };
            let Some(element) = element_weak.upgrade() else {
                return;
            };
            element.notify("connected");
            if let Some(bus) = element.bus() {
                let s = gst::Structure::builder(CONNECTIVITY_MESSAGE)
                    .field("connected", now_connected)
                    .build();
                let _ = bus.post(gst::message::Element::builder(s).src(&element).build());
            }
        })
        .background()
        .wait()?;

    Ok(())
}
