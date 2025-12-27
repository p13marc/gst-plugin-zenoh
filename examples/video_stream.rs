//! Video streaming example using the strongly-typed API.
//!
//! This example streams test video through Zenoh, demonstrating
//! the use of ZenohSink and ZenohSrc with H.264 encoding.

use anyhow::Error;
use gst::prelude::*;
use gstzenoh::zenohsink::ZenohSink;
use gstzenoh::zenohsrc::ZenohSrc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Error> {
    gst::init()?;
    gstzenoh::plugin_register_static()?;

    println!("Starting video streaming example...");
    println!("This example streams test video through Zenoh");
    println!("Make sure you have GStreamer video plugins installed!");
    println!(
        "Required: gstreamer1-plugins-ugly (x264enc/avdec_h264) or gstreamer1-plugins-bad (openh264enc)"
    );

    // Create sender pipeline
    let sender = gst::Pipeline::new();

    let videosrc = gst::ElementFactory::make("videotestsrc").build()?;

    let videoconvert = gst::ElementFactory::make("videoconvert").build()?;

    // Use x264enc for better compatibility, fall back to other encoders
    let encoder = if let Ok(x264) = gst::ElementFactory::make("x264enc")
        .property("bitrate", 1000u32) // 1 Mbps
        .build()
    {
        println!("Using x264enc for encoding");
        x264
    } else if let Ok(openh264) = gst::ElementFactory::make("openh264enc")
        .property("bitrate", 1000000u32) // 1 Mbps
        .build()
    {
        println!("Using openh264enc for encoding");
        openh264
    } else {
        return Err(anyhow::anyhow!(
            "No H.264 encoder available. Please install gst-plugins-ugly (x264enc) or gst-plugins-bad (openh264enc)"
        ));
    };

    // Create ZenohSink using the builder pattern with optimal video settings
    let zenohsink = ZenohSink::builder("gst/example/video")
        .reliability("reliable")
        .priority(2) // InteractiveHigh for video
        .express(true) // Low latency
        .send_caps(true) // Important for video format negotiation
        .caps_interval(5) // Resend caps every 5 seconds
        .send_buffer_meta(true) // Preserve timing
        .build();

    println!("\nZenohSink configuration:");
    println!("  key-expr: {}", zenohsink.key_expr());
    println!("  reliability: {}", zenohsink.reliability());
    println!("  priority: {} (InteractiveHigh)", zenohsink.priority());
    println!("  express: {}", zenohsink.express());
    println!("  send-caps: {}", zenohsink.send_caps());

    sender.add_many([&videosrc, &videoconvert, &encoder, zenohsink.upcast_ref()])?;
    gst::Element::link_many([&videosrc, &videoconvert, &encoder, zenohsink.upcast_ref()])?;

    // Create receiver pipeline
    let receiver = gst::Pipeline::new();

    // Create ZenohSrc with settings optimized for video
    let zenohsrc = ZenohSrc::builder("gst/example/video")
        .receive_timeout_ms(100) // Fast response for video
        .apply_buffer_meta(true) // Apply timing metadata
        .build();

    println!("\nZenohSrc configuration:");
    println!("  key-expr: {}", zenohsrc.key_expr());
    println!("  receive-timeout-ms: {}ms", zenohsrc.receive_timeout_ms());
    println!("  apply-buffer-meta: {}", zenohsrc.apply_buffer_meta());

    let decoder = gst::ElementFactory::make("decodebin").build()?;
    let videoconvert_sink = gst::ElementFactory::make("videoconvert").build()?;
    let videosink = gst::ElementFactory::make("autovideosink")
        .property("sync", false)
        .build()?;

    receiver.add_many([
        zenohsrc.upcast_ref(),
        &decoder,
        &videoconvert_sink,
        &videosink,
    ])?;
    zenohsrc.link(&decoder)?;

    // Handle dynamic pad linking for decodebin
    let videoconvert_sink_weak = videoconvert_sink.downgrade();
    let videosink_weak = videosink.downgrade();
    decoder.connect_pad_added(move |_, pad| {
        let Some(videoconvert_sink) = videoconvert_sink_weak.upgrade() else {
            return;
        };
        let Some(videosink) = videosink_weak.upgrade() else {
            return;
        };

        if let Some(caps) = pad.current_caps() {
            let structure = caps.structure(0).unwrap();
            if structure.name().starts_with("video/") {
                let sink_pad = videoconvert_sink.static_pad("sink").unwrap();
                if sink_pad.link(pad).is_ok() {
                    println!("Successfully linked decoder to videoconvert");
                    if videoconvert_sink.link(&videosink).is_ok() {
                        println!("Successfully linked videoconvert to videosink");
                    }
                }
            }
        }
    });

    // Set up message handling
    let main_loop = gst::glib::MainLoop::new(None, false);

    // Start receiver first
    println!("\nStarting receiver pipeline...");
    receiver.set_state(gst::State::Playing)?;

    // Give receiver time to start
    thread::sleep(Duration::from_millis(1000));

    // Start sender
    println!("Starting sender pipeline...");
    sender.set_state(gst::State::Playing)?;

    // Handle bus messages
    let sender_bus = sender.bus().unwrap();
    let receiver_bus = receiver.bus().unwrap();

    let _sender_watch = sender_bus.add_watch({
        let main_loop = main_loop.clone();
        let zenohsink = zenohsink.clone();
        move |_, msg| {
            handle_message(&main_loop, "VIDEO SENDER", msg);

            // Periodically print statistics
            if matches!(msg.view(), gst::MessageView::StateChanged(..)) {
                if zenohsink.messages_sent() > 0 && zenohsink.messages_sent() % 100 == 0 {
                    println!(
                        "Sender stats: {} messages, {} bytes",
                        zenohsink.messages_sent(),
                        zenohsink.bytes_sent()
                    );
                }
            }

            gst::glib::ControlFlow::Continue
        }
    })?;

    let _receiver_watch = receiver_bus.add_watch({
        let main_loop = main_loop.clone();
        let zenohsrc = zenohsrc.clone();
        move |_, msg| {
            handle_message(&main_loop, "VIDEO RECEIVER", msg);

            // Periodically print statistics
            if matches!(msg.view(), gst::MessageView::StateChanged(..)) {
                if zenohsrc.messages_received() > 0 && zenohsrc.messages_received() % 100 == 0 {
                    println!(
                        "Receiver stats: {} messages, {} bytes",
                        zenohsrc.messages_received(),
                        zenohsrc.bytes_received()
                    );
                }
            }

            gst::glib::ControlFlow::Continue
        }
    })?;

    println!("\nStreaming video... A video window should appear. Press Ctrl+C to stop");
    main_loop.run();

    // Print final statistics
    println!("\nFinal sender statistics:");
    println!("  bytes-sent: {}", zenohsink.bytes_sent());
    println!("  messages-sent: {}", zenohsink.messages_sent());
    println!("  errors: {}", zenohsink.errors());
    println!("  dropped: {}", zenohsink.dropped());

    println!("\nFinal receiver statistics:");
    println!("  bytes-received: {}", zenohsrc.bytes_received());
    println!("  messages-received: {}", zenohsrc.messages_received());
    println!("  errors: {}", zenohsrc.errors());

    // Cleanup
    sender.set_state(gst::State::Null)?;
    receiver.set_state(gst::State::Null)?;

    println!("\nVideo streaming example completed!");
    Ok(())
}

fn handle_message(main_loop: &gst::glib::MainLoop, pipeline: &str, msg: &gst::Message) {
    use gst::MessageView;

    match msg.view() {
        MessageView::Eos(..) => {
            println!("{}: End of stream reached", pipeline);
            main_loop.quit();
        }
        MessageView::Error(err) => {
            eprintln!(
                "{}: Error from {}: {} ({})",
                pipeline,
                msg.src()
                    .map(|s| String::from(s.path_string()))
                    .unwrap_or_else(|| "None".into()),
                err.error(),
                err.debug().unwrap_or_else(|| "".into()),
            );
            main_loop.quit();
        }
        MessageView::Warning(warn) => {
            println!(
                "{}: Warning from {}: {}",
                pipeline,
                msg.src()
                    .map(|s| String::from(s.path_string()))
                    .unwrap_or_else(|| "None".into()),
                warn.error()
            );
        }
        MessageView::StateChanged(state_changed) => {
            // Only log pipeline-level state changes
            if let Some(src) = msg.src() {
                if src.type_().name() == "GstPipeline" {
                    println!(
                        "{}: Pipeline state changed from {:?} to {:?}",
                        pipeline,
                        state_changed.old(),
                        state_changed.current()
                    );
                }
            }
        }
        _ => (),
    }
}
