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

    // Display current properties
    println!("Sink properties:");
    println!("  key-expr: {}", sink.property::<String>("key-expr"));
    println!("  priority: {}", sink.property::<i32>("priority"));
    println!("  congestion-control: {}", sink.property::<String>("congestion-control"));
    println!("  reliability: {}", sink.property::<String>("reliability"));

    println!("Source properties:");
    println!("  key-expr: {}", src.property::<String>("key-expr"));
    println!("  priority: {}", src.property::<i32>("priority"));
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
    // Test different priority levels
    let sink_high_priority = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "config/example/high-priority")
        .property("priority", 5i32)
        .build()?;

    let sink_low_priority = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "config/example/low-priority")
        .property("priority", -5i32)
        .build()?;

    let src_default_priority = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", "config/example/default-priority")
        .property("priority", 0i32)
        .build()?;

    println!("Priority examples:");
    println!("  High priority sink: {}", sink_high_priority.property::<i32>("priority"));
    println!("  Low priority sink: {}", sink_low_priority.property::<i32>("priority"));
    println!("  Default priority src: {}", src_default_priority.property::<i32>("priority"));

    // Test state transitions
    sink_high_priority.set_state(gst::State::Ready)?;
    sink_low_priority.set_state(gst::State::Ready)?;
    src_default_priority.set_state(gst::State::Ready)?;

    thread::sleep(Duration::from_millis(100));

    sink_high_priority.set_state(gst::State::Null)?;
    sink_low_priority.set_state(gst::State::Null)?;
    src_default_priority.set_state(gst::State::Null)?;

    println!("Priority settings test completed successfully");
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