//! Simple data streaming example using the strongly-typed API.
//!
//! This example sends test data from fakesrc through Zenoh to fakesink,
//! demonstrating both the builder pattern and direct element creation.

use anyhow::Error;
use gst::prelude::*;
use gstzenoh::zenohsink::ZenohSink;
use gstzenoh::zenohsrc::ZenohSrc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Error> {
    gst::init()?;
    gstzenoh::plugin_register_static()?;

    println!("Starting simple data streaming example...");
    println!("This example sends test data from fakesrc through Zenoh to fakesink");

    // Create sender pipeline using strongly-typed API
    let sender = gst::Pipeline::new();
    let fakesrc = gst::ElementFactory::make("fakesrc")
        .property("num-buffers", 10i32)
        .build()?;

    // Create ZenohSink using the builder pattern
    let zenohsink = ZenohSink::builder("gst/example/data")
        .reliability("reliable")
        .send_caps(true)
        .send_buffer_meta(true)
        .build();

    // Display sink configuration
    println!("\nZenohSink configuration:");
    println!("  key-expr: {}", zenohsink.key_expr());
    println!("  reliability: {}", zenohsink.reliability());
    println!("  send-caps: {}", zenohsink.send_caps());
    println!("  send-buffer-meta: {}", zenohsink.send_buffer_meta());

    sender.add_many([&fakesrc, zenohsink.upcast_ref()])?;
    fakesrc.link(&zenohsink)?;

    // Create receiver pipeline using strongly-typed API
    let receiver = gst::Pipeline::new();

    // Create ZenohSrc using the new() constructor and setters
    let zenohsrc = ZenohSrc::new("gst/example/data");
    zenohsrc.set_receive_timeout_ms(500);
    zenohsrc.set_apply_buffer_meta(true);

    // Display src configuration
    println!("\nZenohSrc configuration:");
    println!("  key-expr: {}", zenohsrc.key_expr());
    println!("  receive-timeout-ms: {}ms", zenohsrc.receive_timeout_ms());
    println!("  apply-buffer-meta: {}", zenohsrc.apply_buffer_meta());

    let fakesink = gst::ElementFactory::make("fakesink")
        .property("signal-handoffs", true)
        .property("silent", false)
        .build()?;

    receiver.add_many([zenohsrc.upcast_ref(), &fakesink])?;
    zenohsrc.link(&fakesink)?;

    // Set up message handling for both pipelines
    let main_loop = gst::glib::MainLoop::new(None, false);

    // Start receiver first
    println!("\nStarting receiver pipeline...");
    receiver.set_state(gst::State::Playing)?;

    // Give receiver time to start
    thread::sleep(Duration::from_millis(500));

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
            handle_message(&main_loop, "SENDER", msg);

            // Print statistics when EOS is received
            if matches!(msg.view(), gst::MessageView::Eos(..)) {
                println!("\nSender statistics:");
                println!("  bytes-sent: {}", zenohsink.bytes_sent());
                println!("  messages-sent: {}", zenohsink.messages_sent());
                println!("  errors: {}", zenohsink.errors());
                println!("  dropped: {}", zenohsink.dropped());
            }

            gst::glib::ControlFlow::Continue
        }
    })?;

    let _receiver_watch = receiver_bus.add_watch({
        let main_loop = main_loop.clone();
        let zenohsrc = zenohsrc.clone();
        move |_, msg| {
            handle_message(&main_loop, "RECEIVER", msg);

            // Print statistics periodically on state changes
            if matches!(msg.view(), gst::MessageView::StateChanged(..)) {
                println!("\nReceiver statistics:");
                println!("  bytes-received: {}", zenohsrc.bytes_received());
                println!("  messages-received: {}", zenohsrc.messages_received());
                println!("  errors: {}", zenohsrc.errors());
            }

            gst::glib::ControlFlow::Continue
        }
    })?;

    // Run for a limited time or until EOS
    println!("\nStreaming data... Press Ctrl+C to stop");
    main_loop.run();

    // Print final statistics
    println!("\nFinal sender statistics:");
    println!("  bytes-sent: {}", zenohsink.bytes_sent());
    println!("  messages-sent: {}", zenohsink.messages_sent());

    println!("\nFinal receiver statistics:");
    println!("  bytes-received: {}", zenohsrc.bytes_received());
    println!("  messages-received: {}", zenohsrc.messages_received());

    // Cleanup
    sender.set_state(gst::State::Null)?;
    receiver.set_state(gst::State::Null)?;

    println!("\nExample completed successfully!");
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
        MessageView::Info(info) => {
            println!(
                "{}: Info from {}: {}",
                pipeline,
                msg.src()
                    .map(|s| String::from(s.path_string()))
                    .unwrap_or_else(|| "None".into()),
                info.error()
            );
        }
        _ => (),
    }
}
