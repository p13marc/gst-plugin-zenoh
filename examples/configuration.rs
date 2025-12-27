//! Configuration example demonstrating different Zenoh settings.
//!
//! This example shows how to configure ZenohSink and ZenohSrc elements
//! using both the strongly-typed API and the property-based API.

use anyhow::Error;
use gst::prelude::*;
use gstzenoh::zenohsink::ZenohSink;
use gstzenoh::zenohsrc::ZenohSrc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Error> {
    gst::init()?;
    gstzenoh::plugin_register_static()?;

    println!("Starting configuration example...");
    println!("This example demonstrates different Zenoh configuration options");

    // Create a temporary Zenoh config file
    create_sample_config()?;

    // Example 1: Using the strongly-typed builder API
    println!("\n=== Example 1: Strongly-typed builder API ===");
    builder_api_example()?;

    // Example 2: Using the strongly-typed setters
    println!("\n=== Example 2: Strongly-typed setters ===");
    setter_api_example()?;

    // Example 3: Using config file
    println!("\n=== Example 3: Using Zenoh config file ===");
    config_file_example()?;

    // Example 4: Different reliability and congestion control settings
    println!("\n=== Example 4: Different reliability settings ===");
    reliability_example()?;

    // Example 5: Priority settings
    println!("\n=== Example 5: Priority settings ===");
    priority_example()?;

    // Example 6: Express mode settings
    println!("\n=== Example 6: Express mode settings ===");
    express_mode_example()?;

    // Clean up
    cleanup_sample_config()?;

    println!("\nConfiguration examples completed successfully!");
    Ok(())
}

fn builder_api_example() -> Result<(), Error> {
    // Create elements using the builder API with full type safety
    let sink = ZenohSink::builder("config/example/builder")
        .reliability("reliable")
        .priority(2) // InteractiveHigh
        .express(true)
        .congestion_control("block")
        .send_caps(true)
        .caps_interval(5)
        .build();

    let src = ZenohSrc::builder("config/example/builder")
        .receive_timeout_ms(500)
        .apply_buffer_meta(true)
        .build();

    // Access properties using typed getters
    println!("Sink properties (via typed getters):");
    println!("  key-expr: {}", sink.key_expr());
    println!("  priority: {} (InteractiveHigh)", sink.priority());
    println!("  congestion-control: {}", sink.congestion_control());
    println!("  reliability: {}", sink.reliability());
    println!("  express: {}", sink.express());
    println!("  send-caps: {}", sink.send_caps());
    println!("  caps-interval: {}s", sink.caps_interval());

    println!("Source properties (via typed getters):");
    println!("  key-expr: {}", src.key_expr());
    println!("  receive-timeout-ms: {}ms", src.receive_timeout_ms());
    println!("  apply-buffer-meta: {}", src.apply_buffer_meta());

    // Test state transitions
    sink.set_state(gst::State::Ready)?;
    src.set_state(gst::State::Ready)?;

    thread::sleep(Duration::from_millis(100));

    sink.set_state(gst::State::Null)?;
    src.set_state(gst::State::Null)?;

    println!("Builder API test completed successfully");
    Ok(())
}

fn setter_api_example() -> Result<(), Error> {
    // Create elements using new() and then configure with setters
    let sink = ZenohSink::new("config/example/setters");
    sink.set_reliability("reliable");
    sink.set_priority(3); // InteractiveLow
    sink.set_express(false);
    sink.set_congestion_control("drop");

    let src = ZenohSrc::new("config/example/setters");
    src.set_receive_timeout_ms(200);
    src.set_apply_buffer_meta(false);

    println!("Sink properties (configured via setters):");
    println!("  key-expr: {}", sink.key_expr());
    println!("  priority: {} (InteractiveLow)", sink.priority());
    println!("  congestion-control: {}", sink.congestion_control());
    println!("  reliability: {}", sink.reliability());
    println!("  express: {}", sink.express());

    println!("Source properties (configured via setters):");
    println!("  key-expr: {}", src.key_expr());
    println!("  receive-timeout-ms: {}ms", src.receive_timeout_ms());
    println!("  apply-buffer-meta: {}", src.apply_buffer_meta());

    // Test state transitions
    sink.set_state(gst::State::Ready)?;
    src.set_state(gst::State::Ready)?;

    thread::sleep(Duration::from_millis(100));

    sink.set_state(gst::State::Null)?;
    src.set_state(gst::State::Null)?;

    println!("Setter API test completed successfully");
    Ok(())
}

fn config_file_example() -> Result<(), Error> {
    let config_path = "/tmp/zenoh_example_config.json5";

    // Create elements with config file using builder
    let sink = ZenohSink::builder("config/example/file")
        .config(config_path)
        .build();

    let src = ZenohSrc::builder("config/example/file")
        .config(config_path)
        .build();

    println!("Created elements with config file: {}", config_path);
    println!("Config property: {:?}", sink.config());

    // Test state transitions
    sink.set_state(gst::State::Ready)?;
    src.set_state(gst::State::Ready)?;

    thread::sleep(Duration::from_millis(100));

    sink.set_state(gst::State::Null)?;
    src.set_state(gst::State::Null)?;

    println!("Config file test completed successfully");
    Ok(())
}

fn reliability_example() -> Result<(), Error> {
    // Test reliable delivery using builder
    let sink_reliable = ZenohSink::builder("config/example/reliable")
        .reliability("reliable")
        .congestion_control("block")
        .build();

    // Test best-effort delivery
    let sink_best_effort = ZenohSink::builder("config/example/best-effort")
        .reliability("best-effort")
        .congestion_control("drop")
        .build();

    println!("Reliable sink:");
    println!("  reliability: {}", sink_reliable.reliability());
    println!(
        "  congestion-control: {}",
        sink_reliable.congestion_control()
    );

    println!("Best-effort sink:");
    println!("  reliability: {}", sink_best_effort.reliability());
    println!(
        "  congestion-control: {}",
        sink_best_effort.congestion_control()
    );

    // Test state transitions
    sink_reliable.set_state(gst::State::Ready)?;
    sink_best_effort.set_state(gst::State::Ready)?;

    thread::sleep(Duration::from_millis(100));

    sink_reliable.set_state(gst::State::Null)?;
    sink_best_effort.set_state(gst::State::Null)?;

    println!("Reliability settings test completed successfully");
    Ok(())
}

fn priority_example() -> Result<(), Error> {
    // Test different priority levels using the strongly-typed API
    let sink_realtime = ZenohSink::builder("config/example/realtime-priority")
        .priority(1) // RealTime priority
        .build();

    let sink_background = ZenohSink::builder("config/example/background-priority")
        .priority(7) // Background priority
        .build();

    let src_default = ZenohSrc::builder("config/example/default-priority")
        .priority(5) // Data priority (default)
        .build();

    println!(
        "Priority examples (1=RealTime, 2=InteractiveHigh, 3=InteractiveLow, 4=DataHigh, 5=Data, 6=DataLow, 7=Background):"
    );
    println!("  RealTime priority sink: {}", sink_realtime.priority());
    println!("  Background priority sink: {}", sink_background.priority());
    println!("  Default priority src: {}", src_default.priority());

    // Test state transitions
    sink_realtime.set_state(gst::State::Ready)?;
    sink_background.set_state(gst::State::Ready)?;
    src_default.set_state(gst::State::Ready)?;

    thread::sleep(Duration::from_millis(100));

    sink_realtime.set_state(gst::State::Null)?;
    sink_background.set_state(gst::State::Null)?;
    src_default.set_state(gst::State::Null)?;

    println!("Priority settings test completed successfully");
    Ok(())
}

fn express_mode_example() -> Result<(), Error> {
    // Test express mode using the strongly-typed API
    let sink_express = ZenohSink::builder("config/example/express")
        .express(true)
        .priority(2) // InteractiveHigh priority
        .reliability("reliable")
        .build();

    let sink_normal = ZenohSink::builder("config/example/normal")
        .express(false)
        .priority(4) // DataHigh priority
        .reliability("reliable")
        .build();

    println!("Express mode examples:");
    println!("  Express enabled sink:");
    println!("    express: {}", sink_express.express());
    println!(
        "    priority: {} (InteractiveHigh)",
        sink_express.priority()
    );
    println!("    reliability: {}", sink_express.reliability());

    println!("  Normal mode sink:");
    println!("    express: {}", sink_normal.express());
    println!("    priority: {} (DataHigh)", sink_normal.priority());
    println!("    reliability: {}", sink_normal.reliability());

    // Test state transitions
    sink_express.set_state(gst::State::Ready)?;
    sink_normal.set_state(gst::State::Ready)?;

    thread::sleep(Duration::from_millis(100));

    sink_express.set_state(gst::State::Null)?;
    sink_normal.set_state(gst::State::Null)?;

    println!("Express mode test completed successfully");
    Ok(())
}

fn create_sample_config() -> Result<(), Error> {
    let config_content = r#"{
  "mode": "peer",
  "connect": {
    "endpoints": ["tcp/127.0.0.1:7447"]
  },
  "scouting": {
    "multicast": {
      "enabled": false
    }
  },
  "timestamping": {
    "enabled": true
  }
}"#;

    std::fs::write("/tmp/zenoh_example_config.json5", config_content)?;
    println!("Created sample Zenoh config file at /tmp/zenoh_example_config.json5");
    Ok(())
}

fn cleanup_sample_config() -> Result<(), Error> {
    std::fs::remove_file("/tmp/zenoh_example_config.json5").ok();
    println!("Cleaned up sample config file");
    Ok(())
}
