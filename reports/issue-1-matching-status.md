# Issue #1: Expose Zenoh Subscriber Matching to Enable On-Demand Pipeline Execution

**Issue:** [p13marc/gst-plugin-zenoh#1](https://github.com/p13marc/gst-plugin-zenoh/issues/1)
**Date:** 2026-02-19

## Summary of the Issue

The issue proposes exposing Zenoh's publisher matching API through zenohsink so that
applications can know whether any Zenoh subscribers exist for the published key expression.
Currently, zenohsink begins publishing data as soon as it transitions to PLAYING state,
regardless of whether anyone is listening. The issue explicitly states that zenohsink should
remain passive — it should **not** control the pipeline state itself, only expose the
information.

The proposed mechanisms are:
- A read-only boolean property: `has-subscribers`
- A GStreamer signal: `matching-changed(bool)`

## Analysis: Should We Implement This?

**Yes, we should implement this.** Here's why:

### 1. The API Already Exists in Zenoh

Zenoh's `Publisher` type (which we already hold in the `Started` state) provides exactly the
APIs we need:

- **`publisher.matching_status()`** — returns the current `MatchingStatus` (synchronous one-shot poll)
- **`publisher.matching_listener()`** — returns a `MatchingListenerBuilder` supporting callbacks, channels, and background mode

Both methods are available with the `unstable` feature, which we **already enable** in our
`Cargo.toml` (`zenoh = { version = "1.0", features = ["unstable"] }`). No new dependencies
or features are required.

### 2. It's a Genuine Resource Efficiency Problem

In media pipelines, upstream elements (cameras, encoders, test sources) consume real
resources — CPU, GPU, bandwidth, power. Publishing buffers when no one is receiving them is
wasteful. This is especially important for:

- Battery-powered or embedded devices
- Pipelines with expensive encoding stages
- Multi-stream setups where some streams may have intermittent consumers

### 3. The Design Constraint is Correct

The issue correctly states that zenohsink should remain passive. A GStreamer sink element
should not make autonomous decisions about pipeline state — that's the application's job.
Exposing matching status as a property + signal follows the standard GStreamer pattern (similar
to how `appsink` exposes signals without controlling the pipeline).

### 4. Low Implementation Complexity

The feature touches only zenohsink. No changes to zenohsrc, zenohdemux, metadata,
compression, or session management.

## Alternative Ideas Considered (Non-Architecture)

### Alternative A: Application-Side Zenoh Matching (No Plugin Change)

An application could open its own Zenoh session, declare its own publisher on the same key
expression, and use `matching_listener()` directly from Rust.

**Verdict:** This works but is awkward. It forces users to manage a duplicate publisher and
a separate Zenoh session just for monitoring. It also doesn't work from `gst-launch-1.0`.
The plugin should expose what it already knows.

### Alternative B: `valve` Element Auto-Control

zenohsink could directly control an upstream `valve` element to stop/start data flow.

**Verdict:** This violates the passivity constraint and couples zenohsink to a specific
pipeline topology. Rejected — but worth documenting in examples how users can wire this up
themselves using the signal.

---

## Architecture Options for Matching Status Delivery

This is the core design question: **how does matching status information flow from Zenoh into
the GStreamer world?** Below are the three viable approaches, analyzed in detail.

### Option 1: Dedicated Thread with Channel Receiver

Spawn a `std::thread` that calls `matching_listener.recv()` in a loop.

```rust
// In start():
let matching_listener = publisher.matching_listener().wait()?;
std::thread::Builder::new()
    .name("zenohsink-matching".into())
    .spawn(move || {
        while let Ok(status) = matching_listener.recv() {
            has_subscribers.store(status.matching(), Ordering::Relaxed);
            // emit signal + bus message...
        }
    })?;
```

| Aspect | Assessment |
|--------|-----------|
| Thread cost | One OS thread permanently parked on `recv()` per zenohsink instance |
| Cleanup | Thread exits when `MatchingListener` drops (on `stop()`) |
| Complexity | Moderate — must manage thread handle, WeakRef to element |
| Latency | Excellent — immediate notification |

**Verdict: Overkill.** An entire OS thread to watch a single boolean that changes rarely
(subscriber connect/disconnect events are infrequent). This is the approach from the initial
report and the reason for this re-evaluation.

### Option 2: Zenoh Background Callback (Recommended)

Use Zenoh's `.callback().background()` builder pattern. Zenoh calls our closure on its own
internal thread pool — **no thread spawn on our side**.

```rust
// In start():
let has_subscribers = Arc::new(AtomicBool::new(false));
let has_subscribers_clone = has_subscribers.clone();
let element_weak = self.obj().downgrade();

publisher
    .matching_listener()
    .callback(move |status| {
        let matching = status.matching();
        has_subscribers_clone.store(matching, Ordering::Relaxed);

        if let Some(element) = element_weak.upgrade() {
            element.emit_by_name::<()>("matching-changed", &[&matching]);
            if let Some(bus) = element.bus() {
                let s = gst::Structure::builder("zenoh-matching-changed")
                    .field("has-subscribers", matching)
                    .build();
                let _ = bus.post(
                    gst::message::Element::builder(s).src(&*element).build()
                );
            }
        }
    })
    .background()
    .wait()
    .map_err(|e| ZenohError::Init(e).to_error_message())?;

// Check initial status
let initial = publisher.matching_status().wait()
    .map_err(|e| ZenohError::Init(e).to_error_message())?;
has_subscribers.store(initial.matching(), Ordering::Relaxed);
```

| Aspect | Assessment |
|--------|-----------|
| Thread cost | **Zero** — Zenoh calls the closure on its own existing runtime threads |
| Cleanup | **Automatic** — background listener lives until the publisher is dropped |
| Complexity | **Minimal** — no thread handle, no join, no channel |
| Latency | Excellent — same as Option 1 |
| State to store | Just `Arc<AtomicBool>` in `Started` — no listener handle needed |

**How `.callback().background()` works** (from Zenoh source at
`zenoh-1.7.2/src/api/builders/matching_listener.rs`):

1. `.callback(closure)` wraps the closure into a `Callback<MatchingStatus>` and changes the
   builder's handler type.
2. `.background()` flips the `BACKGROUND` const-generic to `true`, which changes the
   `Resolvable::To` from `ZResult<MatchingListener<Handler>>` to `ZResult<()>`.
3. `.wait()` registers the callback internally with the session and returns `Ok(())` — **no
   `MatchingListener` handle is returned** because none is needed.
4. The callback is invoked by Zenoh whenever matching status changes, until the publisher (or
   session) is dropped.

This is exactly how `zenoh::Subscriber` supports background callbacks too — it's an
established Zenoh pattern, not an obscure API.

**Why this is the best option:**

- **Zero resource overhead** on our side — no thread, no channel, no handle to store
- **Automatic lifecycle** — tied to the publisher's lifetime, which is already managed by our
  `Started` state. When `stop()` drops `Started`, the publisher drops, and the background
  callback is unregistered.
- **Consistent with Zenoh idioms** — this is the recommended pattern in Zenoh docs
- **Minimal state addition** — only `has_subscribers: Arc<AtomicBool>` added to `Started`

### Option 3: Polling in `render()`

Call `publisher.matching_status().wait()` during each `render()` call (or periodically,
e.g., every N buffers or every T seconds).

```rust
// In render(), after getting the Started state:
let now = std::time::Instant::now();
let should_poll = {
    let last = started.last_matching_poll.lock().unwrap();
    last.map_or(true, |t| now.duration_since(t) > Duration::from_secs(1))
};
if should_poll {
    if let Ok(status) = started.publisher.matching_status().wait() {
        let old = started.has_subscribers.swap(status.matching(), Ordering::Relaxed);
        if old != status.matching() {
            // emit signal + bus message...
        }
    }
    *started.last_matching_poll.lock().unwrap() = Some(now);
}
```

| Aspect | Assessment |
|--------|-----------|
| Thread cost | Zero |
| Cleanup | None needed |
| Complexity | Moderate — polling interval logic, extra state for last-poll time |
| Latency | **Poor** — bounded by polling interval AND buffer arrival rate |

**Verdict: Problematic.** The whole point of this feature is to know when subscribers
appear/disappear. If there's no data flowing (pipeline paused, no upstream buffers), `render()`
isn't called, and we never detect status changes. This defeats the primary use case of "don't
produce data when no one is listening." The application needs the notification *before* deciding
whether to start producing data.

Additionally, `matching_status()` is a network round-trip query. Doing it on every `render()`
call adds latency to the hot data path, even if throttled.

---

## Recommended Approach: Option 2 (Background Callback)

### What We Expose

1. **Read-only property `has-subscribers`** (boolean, default: `false`) — polled by applications
2. **GStreamer signal `matching-changed`** (emits `bool`) — push notification for Rust/C apps
3. **Bus message `zenoh-matching-changed`** — push notification for gst-launch / GMainLoop apps

The signal and bus message are complementary:
- **Signal**: Direct, low-latency, for programmatic Rust/C applications
- **Bus message**: Discoverable from `gst-launch-1.0` via `gst_bus_add_watch`, integrates
  naturally with GMainLoop-based applications

## Implementation Plan

### Step 1: Add `has_subscribers` to `Started` struct

In `zenohsink/imp.rs`:

```rust
struct Started {
    _session: SessionWrapper,
    publisher: zenoh::pubsub::Publisher<'static>,
    stats: Arc<Mutex<Statistics>>,
    caps_sent: Arc<AtomicBool>,
    last_caps_time: Arc<Mutex<Option<std::time::Instant>>>,
    last_caps: Arc<Mutex<Option<gst::Caps>>>,
    has_subscribers: Arc<AtomicBool>,  // NEW
}
```

Only one new field. No listener handle needed since `.background()` returns `()`.

### Step 2: Define the GStreamer Signal

In `ObjectImpl::signals()`:

```rust
fn signals() -> &'static [glib::subclass::Signal] {
    static SIGNALS: LazyLock<Vec<glib::subclass::Signal>> = LazyLock::new(|| {
        vec![
            glib::subclass::Signal::builder("matching-changed")
                .param_types([bool::static_type()])
                .build(),
        ]
    });
    SIGNALS.as_ref()
}
```

### Step 3: Add the `has-subscribers` read-only property

In the existing `properties()` and `property()` methods, add:

```rust
// In properties():
glib::ParamSpecBoolean::builder("has-subscribers")
    .nick("Has Subscribers")
    .blurb("Whether there are currently matching Zenoh subscribers")
    .default_value(false)
    .read_only()
    .build(),

// In property():
"has-subscribers" => {
    let state = self.state.lock().unwrap();
    match *state {
        State::Started(ref started) => {
            started.has_subscribers.load(Ordering::Relaxed).to_value()
        }
        _ => false.to_value(),
    }
}
```

### Step 4: Register background matching listener in `start()`

After the publisher is created (line ~818 in current code), before constructing `Started`:

```rust
// Set up matching status tracking
let has_subscribers = Arc::new(AtomicBool::new(false));

// Register background callback — Zenoh calls this on its own threads.
// The callback lives until the publisher is dropped (in stop()).
{
    let has_subscribers = has_subscribers.clone();
    let element_weak = self.obj().downgrade();

    publisher
        .matching_listener()
        .callback(move |status| {
            let matching = status.matching();
            has_subscribers.store(matching, Ordering::Relaxed);

            if let Some(element) = element_weak.upgrade() {
                // Emit GObject signal for Rust/C consumers
                element.emit_by_name::<()>("matching-changed", &[&matching]);

                // Post bus message for gst-launch / GMainLoop consumers
                if let Some(bus) = element.bus() {
                    let s = gst::Structure::builder("zenoh-matching-changed")
                        .field("has-subscribers", matching)
                        .build();
                    let _ = bus.post(
                        gst::message::Element::builder(s).src(&*element).build()
                    );
                }
            }
        })
        .background()
        .wait()
        .map_err(|e| ZenohError::Init(e).to_error_message())?;
}

// Check initial status (the callback only fires on *changes*)
let initial_status = publisher.matching_status().wait()
    .map_err(|e| ZenohError::Init(e).to_error_message())?;
has_subscribers.store(initial_status.matching(), Ordering::Relaxed);
```

### Step 5: No changes to `stop()`

When `stop()` drops the `Started` struct:
1. `publisher` is dropped
2. Zenoh unregisters the background callback automatically
3. `has_subscribers` Arc is dropped

No thread joins, no explicit cleanup, no ordering concerns.

### Step 6: Update Public API (`zenohsink/mod.rs`)

Add typed accessors:

```rust
impl ZenohSink {
    /// Returns whether there are currently matching Zenoh subscribers.
    pub fn has_subscribers(&self) -> bool {
        self.property::<bool>("has-subscribers")
    }

    /// Connect to the `matching-changed` signal.
    ///
    /// The callback receives `true` when at least one subscriber matches,
    /// and `false` when the last matching subscriber disappears.
    pub fn connect_matching_changed<F: Fn(&Self, bool) + Send + Sync + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect("matching-changed", false, move |values| {
            let element = values[0].get::<gst::Element>().unwrap();
            let sink = ZenohSink::try_from(element).unwrap();
            let matching = values[1].get::<bool>().unwrap();
            f(&sink, matching);
            None
        })
    }
}
```

### Step 7: Add Tests

- **Property test:** Verify `has-subscribers` property exists and defaults to `false`
- **Integration test:** Create a zenohsink publisher, then create a zenoh subscriber on the
  same key expression via the Zenoh API directly. Verify that the signal fires with `true`.
  Drop the subscriber. Verify that the signal fires with `false`.
- **Bus message test:** Verify that `zenoh-matching-changed` messages appear on the bus with
  the correct structure.

### Step 8: Update Documentation

- Add the property and signal to `CLAUDE.md` element properties section
- Add usage examples showing valve-based on-demand pipeline
- Update `zenohsink/mod.rs` doc comments

## Scope Summary

| Area | Files Changed |
|------|--------------|
| Core implementation | `src/zenohsink/imp.rs` |
| Public API | `src/zenohsink/mod.rs` |
| Tests | `tests/matching_tests.rs` (new) |
| Documentation | `CLAUDE.md`, module docs |

No changes to zenohsrc, zenohdemux, metadata, compression, or session management.

## Sources

- [Zenoh Publisher API (docs.rs)](https://docs.rs/zenoh/latest/zenoh/pubsub/struct.Publisher.html) — `matching_status()` and `matching_listener()` documentation
- [Zenoh Rust API docs](https://zenoh.io/docs/apis/rust/) — background callback pattern
- [Zenoh 1.1.0 Release Blog](https://zenoh.io/blog/2024-12-12-zenoh-firesong-1.1.0/) — matching API context
- Zenoh source: `zenoh-1.7.2/src/api/builders/matching_listener.rs` — `.callback().background()` implementation
- [GLib ObjectImpl signals](https://docs.rs/glib/latest/glib/subclass/object/trait.ObjectImpl.html) — GStreamer signal definition
