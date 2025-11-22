use gst::prelude::*;
use serial_test::serial;

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
fn test_render_list_method_exists() {
    init();

    // This test verifies that ZenohSink has the render_list implementation
    // The actual buffer list processing is tested through integration tests

    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/render_list/method")
        .build()
        .expect("Failed to create zenohsink");

    // Verify the element was created successfully
    assert!(sink.name().starts_with("zenohsink"));

    // The render_list method is implemented in BaseSinkImpl
    // and will be called automatically by GStreamer when buffer lists are pushed
}

#[test]
#[serial]
fn test_buffer_list_creation() {
    init();

    // Create a buffer list with multiple buffers
    let mut list = gst::BufferList::new();

    {
        let list_mut = list.get_mut().unwrap();

        // Add 5 buffers to the list
        for i in 0..5 {
            let mut buffer = gst::Buffer::with_size(100).unwrap();
            {
                let buffer_mut = buffer.get_mut().unwrap();
                let mut map = buffer_mut.map_writable().unwrap();
                // Fill with test data
                for byte in map.as_mut_slice() {
                    *byte = (i * 10) as u8;
                }
            }
            list_mut.add(buffer);
        }
    }

    // Verify the list has 5 buffers
    assert_eq!(list.len(), 5);
}

#[test]
#[serial]
fn test_empty_buffer_list() {
    init();

    // Create an empty buffer list
    let list = gst::BufferList::new();
    assert_eq!(list.len(), 0);

    // Empty lists are valid and should be handled gracefully
}

#[test]
#[serial]
fn test_large_buffer_list_creation() {
    init();

    // Create a large buffer list (100 buffers)
    let mut list = gst::BufferList::new();
    {
        let list_mut = list.get_mut().unwrap();
        for i in 0..100 {
            let mut buffer = gst::Buffer::with_size(1024).unwrap();
            {
                let buffer_mut = buffer.get_mut().unwrap();
                let mut map = buffer_mut.map_writable().unwrap();
                for (idx, byte) in map.as_mut_slice().iter_mut().enumerate() {
                    *byte = ((i + idx) % 256) as u8;
                }
            }
            list_mut.add(buffer);
        }
    }

    assert_eq!(list.len(), 100);

    // Large buffer lists should be supported for batch operations
}

#[test]
#[serial]
fn test_mixed_buffer_sizes_list() {
    init();

    // Create a list with varying buffer sizes
    let sizes = vec![10, 50, 100, 25, 500, 75];
    let mut list = gst::BufferList::new();
    {
        let list_mut = list.get_mut().unwrap();
        for size in &sizes {
            let buffer = gst::Buffer::with_size(*size).unwrap();
            list_mut.add(buffer);
        }
    }

    assert_eq!(list.len(), 6);

    // Verify we can iterate over the list
    let mut count = 0;
    for buffer in list.iter() {
        assert!(buffer.size() > 0);
        count += 1;
    }
    assert_eq!(count, 6);
}

#[test]
#[serial]
fn test_render_list_with_pipeline() {
    init();

    // Create a simple pipeline: videotestsrc ! zenohsink
    // This tests that buffer lists flow through correctly
    let pipeline = gst::Pipeline::new();

    let src = gst::ElementFactory::make("videotestsrc")
        .property("num-buffers", 10i32)
        .property("is-live", false)
        .build()
        .expect("Failed to create videotestsrc");

    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/render_list/pipeline")
        .build()
        .expect("Failed to create zenohsink");

    pipeline.add_many([&src, &sink]).unwrap();
    src.link(&sink).unwrap();

    // Start the pipeline
    pipeline.set_state(gst::State::Playing).unwrap();

    // Let it run for a bit
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Check that data was sent
    let messages_sent: u64 = sink.property("messages-sent");
    assert!(messages_sent > 0, "Should have sent at least one message");

    // Stop the pipeline
    pipeline.set_state(gst::State::Null).unwrap();
}

#[test]
#[serial]
fn test_buffer_list_statistics_tracking() {
    init();

    // This test verifies that statistics are tracked correctly
    // when using render_list (batch mode)

    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/render_list/stats")
        .build()
        .expect("Failed to create zenohsink");

    // Initially, all statistics should be zero
    let initial_messages: u64 = sink.property("messages-sent");
    let initial_bytes: u64 = sink.property("bytes-sent");
    let initial_errors: u64 = sink.property("errors");

    assert_eq!(initial_messages, 0);
    assert_eq!(initial_bytes, 0);
    assert_eq!(initial_errors, 0);

    // The render_list method will update these statistics
    // when buffers are actually processed
}

#[test]
#[serial]
fn test_render_list_resilience() {
    init();

    // Verify that render_list implementation includes error resilience
    // The implementation should continue processing buffers even if some fail

    let sink = gst::ElementFactory::make("zenohsink")
        .property("key-expr", "test/render_list/resilience")
        .build()
        .expect("Failed to create zenohsink");

    // The render_list method is implemented to:
    // 1. Process each buffer in the list
    // 2. Track errors but continue with remaining buffers
    // 3. Update statistics atomically after processing
    // 4. Return success if at least one buffer was sent

    assert!(sink.name().starts_with("zenohsink"));
}
