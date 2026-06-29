// Shared utilities for the gst-plugin-zenoh elements.
//
// The plugin uses Zenoh's synchronous API via `wait()`. Because `wait()` has no
// built-in timeout, the helper below bounds an otherwise-unbounded blocking call
// (e.g. `zenoh::open(...).wait()`) so a stalled/unreachable router can never hang
// a GStreamer state-change thread indefinitely.

use std::time::Duration;

/// Marker returned by [`call_with_timeout`] when the closure did not finish in time.
pub(crate) struct TimedOut;

/// Run a blocking closure on a detached worker thread and wait at most `timeout`
/// for its result.
///
/// If the closure does not complete in time, `Err(TimedOut)` is returned and the
/// worker thread is left to finish on its own (its result is discarded). This is
/// used to bound `zenoh::open(...).wait()`, which is otherwise unbounded.
pub(crate) fn call_with_timeout<T, F>(timeout: Duration, f: F) -> Result<T, TimedOut>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = flume::bounded(1);
    std::thread::spawn(move || {
        // The receiver may already be gone (timeout elapsed); ignore the send error.
        let _ = tx.send(f());
    });
    rx.recv_timeout(timeout).map_err(|_| TimedOut)
}
