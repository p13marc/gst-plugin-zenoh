//! On-demand pipeline execution using subscriber matching.
//!
//! This example demonstrates how to use the `matching-changed` signal
//! and `has-subscribers` property to start/stop a pipeline based on
//! whether any Zenoh subscribers are currently listening.
//!
//! The pipeline starts in the **Ready** state — Zenoh resources (session,
//! publisher, matching listener) are active but no data flows and no
//! pipeline resources (encoders, capture devices) are consumed.
//!
//! When a subscriber connects, the pipeline transitions to Playing.
//! When all subscribers disconnect, it returns to Ready.
//!
//! ## How to run
//!
//! Terminal 1 (producer — starts in Ready, waiting for subscribers):
//! ```bash
//! GST_PLUGIN_PATH=target/debug cargo run --example on_demand
//! ```
//!
//! Terminal 2 (subscriber — connect and disconnect to control the producer):
//! ```bash
//! GST_PLUGIN_PATH=target/debug gst-launch-1.0 \
//!     zenohsrc key-expr=demo/on-demand ! fakesink
//! ```
//!
//! You'll see the producer start streaming when the subscriber appears,
//! and return to Ready when the subscriber exits (Ctrl+C).

use anyhow::Error;
use gst::prelude::*;
use gstzenoh::ZenohSink;

fn main() -> Result<(), Error> {
    gst::init()?;
    gstzenoh::plugin_register_static()?;

    let key = "demo/on-demand";

    // Build the pipeline: videotestsrc ! zenohsink
    let pipeline = gst::Pipeline::new();

    let src = gst::ElementFactory::make("videotestsrc")
        .property_from_str("pattern", "ball")
        .build()?;

    let sink = ZenohSink::builder(key)
        .reliability("reliable")
        .send_caps(true)
        .send_buffer_meta(true)
        .build();

    pipeline.add_many([&src, sink.upcast_ref()])?;
    src.link(&sink)?;

    // Start the pipeline in Ready — Zenoh resources are active (session,
    // publisher, matching listener) but no data flows.
    println!("[on-demand] Pipeline created on key-expr '{key}'");
    println!("[on-demand] Starting in READY state, waiting for subscribers...");
    pipeline.set_state(gst::State::Ready)?;

    // Connect to the matching-changed signal.
    // When a subscriber connects: READY -> PLAYING
    // When all subscribers leave: PLAYING -> READY
    let pipeline_weak = pipeline.downgrade();
    sink.connect_matching_changed(move |_sink, has_subscribers| {
        let Some(pipeline) = pipeline_weak.upgrade() else {
            return;
        };
        if has_subscribers {
            println!("[on-demand] Subscriber connected — transitioning to PLAYING");
            if let Err(e) = pipeline.set_state(gst::State::Playing) {
                eprintln!("[on-demand] Failed to set PLAYING: {e}");
            }
        } else {
            println!("[on-demand] No more subscribers — transitioning to READY");
            if let Err(e) = pipeline.set_state(gst::State::Ready) {
                eprintln!("[on-demand] Failed to set READY: {e}");
            }
        }
    });

    // Run the GLib main loop, handling bus messages.
    let main_loop = gst::glib::MainLoop::new(None, false);

    let main_loop_quit = main_loop.clone();
    let _bus_watch = pipeline.bus().unwrap().add_watch(move |_, msg| {
        use gst::MessageView;
        match msg.view() {
            MessageView::Error(err) => {
                eprintln!(
                    "[on-demand] Error from {}: {}",
                    msg.src()
                        .map(|s| s.path_string().to_string())
                        .unwrap_or_default(),
                    err.error()
                );
                main_loop_quit.quit();
            }
            MessageView::Eos(..) => {
                println!("[on-demand] End of stream");
                main_loop_quit.quit();
            }
            MessageView::Element(element_msg) => {
                if let Some(s) = element_msg.structure() {
                    if s.name() == "zenoh-matching-changed" {
                        let has_subs: bool = s.get("has-subscribers").unwrap();
                        println!("[on-demand] Bus message: has-subscribers={has_subs}");
                    }
                }
            }
            _ => (),
        }
        gst::glib::ControlFlow::Continue
    })?;

    println!("[on-demand] Running... Press Ctrl+C to stop");
    println!("[on-demand] In another terminal, run:");
    println!("  GST_PLUGIN_PATH=target/debug gst-launch-1.0 zenohsrc key-expr={key} ! fakesink");
    main_loop.run();

    pipeline.set_state(gst::State::Null)?;
    println!("[on-demand] Pipeline stopped");
    Ok(())
}
