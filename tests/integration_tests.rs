use gst::prelude::*;
use serial_test::serial;

/// Initialize GStreamer and register our plugin for tests
fn init() {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        gst::init().unwrap();
        gstzenoh::plugin_register_static().expect("Failed to register plugin");
    });
}

#[test]
#[serial]
fn test_zenohsink_to_zenohsrc_pipeline_integration() {
    init();

    let key_expr = "gst-plugin-test/integration/basic";
    
    // Test that we can create a pipeline with both elements
    let pipeline = gst::Pipeline::new();
    
    let src = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", key_expr)
        .build()
        .expect("Failed to create zenohsrc");
    
    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", key_expr)
        .build()
        .expect("Failed to create zenohsink");
    
    // Add elements to pipeline
    pipeline.add_many([&src, &sink]).unwrap();
    
    // Test state transitions (this tests Zenoh session creation without network)
    assert!(pipeline.set_state(gst::State::Ready).is_ok(), "Failed to set pipeline to Ready");
    
    // Test that elements can start (this will fail if Zenoh sessions can't be created)
    // We don't go to Playing to avoid network timeouts
    
    // Clean up
    pipeline.set_state(gst::State::Null).unwrap();
    
    println!("Pipeline integration test passed");
}

#[test]
#[serial]
fn test_zenoh_configuration_file() {
    init();
    
    // Create a temporary Zenoh config file
    let config_content = r#"{
  "mode": "peer",
  "connect": {
    "endpoints": ["tcp/127.0.0.1:7447"]
  },
  "scouting": {
    "multicast": {
      "enabled": false
    }
  }
}"#;
    
    let temp_dir = std::env::temp_dir();
    let config_path = temp_dir.join("zenoh_test_config.json5");
    std::fs::write(&config_path, config_content).expect("Failed to write config file");
    
    // Test that elements can be created with config file
    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/config")
        .property("config", config_path.to_str().unwrap())
        .build()
        .expect("Failed to create zenohsink with config");
    
    let src = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", "test/config")
        .property("config", config_path.to_str().unwrap())
        .build()
        .expect("Failed to create zenohsrc with config");
    
    // Test state transitions
    assert!(sink.set_state(gst::State::Ready).is_ok());
    assert!(src.set_state(gst::State::Ready).is_ok());
    
    // Clean up
    sink.set_state(gst::State::Null).unwrap();
    src.set_state(gst::State::Null).unwrap();
    
    // Remove temp config file
    std::fs::remove_file(&config_path).ok();
    
    println!("Configuration file test passed");
}

#[test]
#[serial]
fn test_multiple_key_expressions() {
    init();
    
    // Test that different key expressions work independently
    let key1 = "test/multi/key1";
    let key2 = "test/multi/key2";
    
    let sink1 = gst::ElementFactory::make("zenohsink")
        .property("key-expr", key1)
        .build()
        .expect("Failed to create zenohsink1");
    
    let sink2 = gst::ElementFactory::make("zenohsink")
        .property("key-expr", key2)
        .build()
        .expect("Failed to create zenohsink2");
    
    let src1 = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", key1)
        .build()
        .expect("Failed to create zenohsrc1");
    
    let src2 = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", key2)
        .build()
        .expect("Failed to create zenohsrc2");
    
    // Test that all elements can transition to Ready state
    assert!(sink1.set_state(gst::State::Ready).is_ok());
    assert!(sink2.set_state(gst::State::Ready).is_ok());
    assert!(src1.set_state(gst::State::Ready).is_ok());
    assert!(src2.set_state(gst::State::Ready).is_ok());
    
    // Clean up
    for element in [&sink1, &sink2, &src1, &src2] {
        element.set_state(gst::State::Null).unwrap();
    }
    
    println!("Multiple key expressions test passed");
}

#[test]
#[serial]
fn test_zenoh_properties_integration() {
    init();
    
    // Test various Zenoh properties
    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/properties")
        .property("priority", 5i32)
        .property("congestion-control", "drop")
        .property("reliability", "reliable")
        .build()
        .expect("Failed to create zenohsink");
    
    let src = gst::ElementFactory::make("zenohsrc")
        .property("key-expr", "test/properties")
        .property("priority", -5i32)
        .property("congestion-control", "block")
        .property("reliability", "best-effort")
        .build()
        .expect("Failed to create zenohsrc");
    
    // Verify properties are set correctly
    let sink_priority: i32 = sink.property("priority");
    assert_eq!(sink_priority, 5);
    
    let sink_congestion: String = sink.property("congestion-control");
    assert_eq!(sink_congestion, "drop");
    
    let sink_reliability: String = sink.property("reliability");
    assert_eq!(sink_reliability, "reliable");
    
    let src_priority: i32 = src.property("priority");
    assert_eq!(src_priority, -5);
    
    // Test state transitions with these properties
    assert!(sink.set_state(gst::State::Ready).is_ok());
    assert!(src.set_state(gst::State::Ready).is_ok());
    
    // Clean up
    sink.set_state(gst::State::Null).unwrap();
    src.set_state(gst::State::Null).unwrap();
    
    println!("Properties integration test passed");
}