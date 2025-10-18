use anyhow::Error;
use gst::prelude::*;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Error> {
    gst::init()?;
    gstzenoh::plugin_register_static()?;

    println!("Starting configuration example...");
    println!("This example demonstrates different Zenoh configuration options");

    // Create a temporary Zenoh config file
    create_sample_config()?;

    // Example 1: Basic configuration with properties
    println!("\n=== Example 1: Basic configuration with properties ===");
    basic_configuration_example()?;

    // Example 2: Using config file
    println!("\n=== Example 2: Using Zenoh config file ===");
    config_file_example()?;

    // Example 3: Different reliability and congestion control settings
    println!("\n=== Example 3: Different reliability settings ===");
    reliability_example()?;

    // Example 4: Priority settings
    println!("\n=== Example 4: Priority settings ===");
    priority_example()?;

    // Example 5: Express mode settings
    println!("\n=== Example 5: Express mode settings ===");
    express_mode_example()?;

    // Clean up
    cleanup_sample_config()?;

    println!("\nConfiguration examples completed successfully!");
    Ok(())
}

fn basic_configuration_example() -> Result<(), Error> {
    // Create elements with default configuration
    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "config/example/basic")
        .build()?;

    let src = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", "config/example/basic")
        .build()?;

    // Display current properties including the new express property
    println!("Sink properties:");
    println!("  key-expr: {}", sink.property::<String>("key-expr"));
    println!("  priority: {} (Data - default)", sink.property::<u32>("priority"));
    println!("  congestion-control: {}", sink.property::<String>("congestion-control"));
    println!("  reliability: {}", sink.property::<String>("reliability"));
    println!("  express: {}", sink.property::<bool>("express"));

    println!("Source properties:");
    println!("  key-expr: {}", src.property::<String>("key-expr"));
    println!("  priority: {} (Data - default)", src.property::<u32>("priority"));
    println!("  congestion-control: {}", src.property::<String>("congestion-control"));
    println!("  reliability: {}", src.property::<String>("reliability"));

    // Test state transitions
    sink.set_state(gst::State::Ready)?;
    src.set_state(gst::State::Ready)?;

    thread::sleep(Duration::from_millis(100));

    sink.set_state(gst::State::Null)?;
    src.set_state(gst::State::Null)?;

    println!("Basic configuration test completed successfully");
    Ok(())
}

fn config_file_example() -> Result<(), Error> {
    let config_path = "/tmp/zenoh_example_config.json5";

    // Create elements with config file
    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "config/example/file")
        .property("config", config_path)
        .build()?;

    let src = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", "config/example/file")
        .property("config", config_path)
        .build()?;

    println!("Created elements with config file: {}", config_path);
    println!("Config property: {:?}", sink.property::<Option<String>>("config"));

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
    // Test reliable delivery
    let sink_reliable = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "config/example/reliable")
        .property("reliability", "reliable")
        .property("congestion-control", "block")
        .build()?;

    // Test best-effort delivery
    let sink_best_effort = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "config/example/best-effort")
        .property("reliability", "best-effort")
        .property("congestion-control", "drop")
        .build()?;

    println!("Reliable sink:");
    println!("  reliability: {}", sink_reliable.property::<String>("reliability"));
    println!("  congestion-control: {}", sink_reliable.property::<String>("congestion-control"));

    println!("Best-effort sink:");
    println!("  reliability: {}", sink_best_effort.property::<String>("reliability"));
    println!("  congestion-control: {}", sink_best_effort.property::<String>("congestion-control"));

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
    // Test different priority levels using Zenoh Priority enum values
    let sink_realtime_priority = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "config/example/realtime-priority")
        .property("priority", 1u32) // RealTime priority
        .build()?;

    let sink_background_priority = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "config/example/background-priority")
        .property("priority", 7u32) // Background priority
        .build()?;

    let src_default_priority = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", "config/example/default-priority")
        .property("priority", 5u32) // Data priority (default)
        .build()?;

    println!("Priority examples (1=RealTime, 2=InteractiveHigh, 3=InteractiveLow, 4=DataHigh, 5=Data, 6=DataLow, 7=Background):");
    println!("  RealTime priority sink: {}", sink_realtime_priority.property::<u32>("priority"));
    println!("  Background priority sink: {}", sink_background_priority.property::<u32>("priority"));
    println!("  Default priority src: {}", src_default_priority.property::<u32>("priority"));

    // Test state transitions
    sink_realtime_priority.set_state(gst::State::Ready)?;
    sink_background_priority.set_state(gst::State::Ready)?;
    src_default_priority.set_state(gst::State::Ready)?;

    thread::sleep(Duration::from_millis(100));

    sink_realtime_priority.set_state(gst::State::Null)?;
    sink_background_priority.set_state(gst::State::Null)?;
    src_default_priority.set_state(gst::State::Null)?;

    println!("Priority settings test completed successfully");
    Ok(())
}

fn express_mode_example() -> Result<(), Error> {
    // Test express mode enabled vs disabled
    let sink_express = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "config/example/express")
        .property("express", true)
        .property("priority", 2u32) // InteractiveHigh priority
        .property("reliability", "reliable")
        .build()?;

    let sink_normal = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "config/example/normal")
        .property("express", false)
        .property("priority", 4u32) // DataHigh priority
        .property("reliability", "reliable")
        .build()?;

    println!("Express mode examples:");
    println!("  Express enabled sink:");
    println!("    express: {}", sink_express.property::<bool>("express"));
    println!("    priority: {} (InteractiveHigh)", sink_express.property::<u32>("priority"));
    println!("    reliability: {}", sink_express.property::<String>("reliability"));
    
    println!("  Normal mode sink:");
    println!("    express: {}", sink_normal.property::<bool>("express"));
    println!("    priority: {} (DataHigh)", sink_normal.property::<u32>("priority"));
    println!("    reliability: {}", sink_normal.property::<String>("reliability"));

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