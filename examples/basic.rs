use anyhow::Error;
use futures::prelude::*;
use futures::stream::select_all;
use gst::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Error> {
    gst::init()?;
    gstzenoh::plugin_register_static()?;

    // TODO: replace udp{sink,src} by zenoh{sink,src}

    let src_pipeline = gst::parse::launch(
        "videotestsrc ! openh264enc bitrate=500 ! rtph264pay ! udpsink host=127.0.0.1 port=5000",
    )?;
    let sink_pipeline =
        gst::parse::launch("udpsrc port=5000 ! application/x-rtp, media=(string)video, clock-rate=(int)90000, encoding-name=(string)H264, payload=(int)96 ! rtph264depay ! h264parse ! decodebin ! videoconvert ! autovideosink sync=false
")?;

    let mut stream = select_all([
        src_pipeline.bus().unwrap().stream(),
        sink_pipeline.bus().unwrap().stream(),
    ]);

    src_pipeline.set_state(gst::State::Playing)?;
    sink_pipeline.set_state(gst::State::Playing)?;

    while let Some(msg) = stream.next().await {
        use gst::MessageView;

        match msg.view() {
            MessageView::Eos(..) => {
                eprintln!("Unexpected EOS");
                break;
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
                break;
            }
            _ => (),
        }
    }

    src_pipeline.set_state(gst::State::Null)?;
    sink_pipeline.set_state(gst::State::Null)?;

    Ok(())
}
