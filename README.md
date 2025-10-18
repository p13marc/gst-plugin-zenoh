# gst-plugin-zenoh

This is a [GStreamer](https://gstreamer.freedesktop.org/) plugin for using [Zenoh](https://zenoh.io/) as the transport build using [zenoh-rs](https://github.com/eclipse-zenoh/zenoh).

## Examples

Build the examples by running
```bash
GST_PLUGIN_PATH=target/debug cargo run --example basic
```

## Testing

Run the unit tests using cargo nextest:
```bash
cargo nextest run
```

Or use regular cargo test:
```bash
cargo test
```

The test suite includes:
- Plugin registration and element creation tests
- Property validation and setting tests 
- Error handling and edge case tests
- Pipeline integration tests

All tests are designed to run without requiring a running Zenoh network.
