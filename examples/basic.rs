use anyhow::Error;
use gst::prelude::*;

fn main() -> Result<(), Error> {
    gst::init()?;
    gstzenoh::plugin_register_static()?;

    let src_pipeline = gst::parse::launch(
        "videotestsrc ! openh264enc bitrate=500 ! rtph264pay ! zenohsink key-expr=demo/example/gst",
    )?;

    let sink_pipeline =
        gst::parse::launch("zenohsrc key-expr=demo/example/gst ! application/x-rtp, media=(string)video, clock-rate=(int)90000, encoding-name=(string)H264, payload=(int)96 ! rtph264depay ! h264parse ! decodebin ! videoconvert ! autovideosink sync=false
")?;

    src_pipeline.set_state(gst::State::Playing)?;
    sink_pipeline.set_state(gst::State::Playing)?;

    // Create a new main loop
    let main_loop = gst::glib::MainLoop::new(None, false);

    // Add watches for both bus messages
    let main_loop_clone = main_loop.clone();
    let _src_watch = src_pipeline.bus().unwrap().add_watch(move |_, msg| {
        handle_message(&main_loop_clone, msg);
        gst::glib::ControlFlow::Continue
    })?;

    let main_loop_clone = main_loop.clone();
    let _sink_watch = sink_pipeline.bus().unwrap().add_watch(move |_, msg| {
        handle_message(&main_loop_clone, msg);
        gst::glib::ControlFlow::Continue
    })?;

    // Start the main loop
    main_loop.run();

    src_pipeline.set_state(gst::State::Null)?;
    sink_pipeline.set_state(gst::State::Null)?;

    Ok(())
}

fn handle_message(main_loop: &gst::glib::MainLoop, msg: &gst::Message) {
    use gst::MessageView;

    match msg.view() {
        MessageView::Eos(..) => {
            eprintln!("Unexpected EOS");
            main_loop.quit();
        }
        MessageView::Error(err) => {
            eprintln!(
                "Got error from {}: {} ({})",
                msg.src()
                    .map(|s| String::from(s.path_string()))
                    .unwrap_or_else(|| "None".into()),
                err.error(),
                err.debug().unwrap_or_else(|| "".into()),
            );
            main_loop.quit();
        }
        _ => (),
    }
}
