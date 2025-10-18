use anyhow::Error;
use gst::prelude::*;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Error> {
    gst::init()?;
    gstzenoh::plugin_register_static()?;

    println!("Starting simple data streaming example...");
    println!("This example sends test data from fakesrc through Zenoh to fakesink");

    // Create sender pipeline: fakesrc -> zenohsink
    let sender = gst::Pipeline::new();
    let fakesrc = gst::ElementFactory::make("fakesrc")
        .property("num-buffers", 10i32)
        .build()?;
    
    let zenohsink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "gst/example/data")
        .build()?;

    sender.add_many([&fakesrc, &zenohsink])?;
    fakesrc.link(&zenohsink)?;

    // Create receiver pipeline: zenohsrc -> fakesink
    let receiver = gst::Pipeline::new();
    let zenohsrc = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", "gst/example/data")
        .build()?;
    
    let fakesink = gst::ElementFactory::make("fakesink")
        .property("signal-handoffs", true)
        .property("silent", false)
        .build()?;

    receiver.add_many([&zenohsrc, &fakesink])?;
    zenohsrc.link(&fakesink)?;

    // Set up message handling for both pipelines
    let main_loop = gst::glib::MainLoop::new(None, false);

    // Start receiver first
    println!("Starting receiver pipeline...");
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
        move |_, msg| {
            handle_message(&main_loop, "SENDER", msg);
            gst::glib::ControlFlow::Continue
        }
    })?;

    let _receiver_watch = receiver_bus.add_watch({
        let main_loop = main_loop.clone();
        move |_, msg| {
            handle_message(&main_loop, "RECEIVER", msg);
            gst::glib::ControlFlow::Continue
        }
    })?;

    // Run for a limited time or until EOS
    println!("Streaming data... Press Ctrl+C to stop");
    main_loop.run();

    // Cleanup
    sender.set_state(gst::State::Null)?;
    receiver.set_state(gst::State::Null)?;

    println!("Example completed successfully!");
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