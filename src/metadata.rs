// SPDX-License-Identifier: MPL-2.0

//! Metadata transmission support for gst-plugin-zenoh
//!
//! This module provides functionality to transmit GStreamer capabilities and custom
//! metadata across Zenoh networks using Zenoh's attachment feature.

use std::collections::HashMap;
use std::str::FromStr;
use zenoh::bytes::ZBytes;

/// Metadata keys used in Zenoh attachments
pub mod keys {
    /// GStreamer caps serialized as a string
    pub const CAPS: &str = "gst.caps";
    /// Custom user-defined metadata prefix
    pub const USER_PREFIX: &str = "user.";
    /// Metadata format version
    pub const VERSION: &str = "gst.version";
    /// Compression algorithm used (if any)
    pub const COMPRESSION: &str = "gst.compression";
    /// Buffer presentation timestamp in nanoseconds
    pub const PTS: &str = "gst.pts";
    /// Buffer decoding timestamp in nanoseconds
    pub const DTS: &str = "gst.dts";
    /// Buffer duration in nanoseconds
    pub const DURATION: &str = "gst.duration";
    /// Buffer byte offset
    pub const OFFSET: &str = "gst.offset";
    /// Buffer byte offset end
    pub const OFFSET_END: &str = "gst.offset-end";
    /// Buffer flags (comma-separated)
    pub const FLAGS: &str = "gst.flags";
    /// Zenoh key expression the sample was received on
    pub const KEY_EXPR: &str = "zenoh.key-expr";
}

/// Current metadata format version (1.1 adds buffer timing support)
pub const METADATA_VERSION: &str = "1.1";

/// Builder for creating Zenoh attachments with GStreamer metadata
#[derive(Debug, Default)]
pub struct MetadataBuilder {
    caps: Option<gst::Caps>,
    pts: Option<gst::ClockTime>,
    dts: Option<gst::ClockTime>,
    duration: Option<gst::ClockTime>,
    offset: Option<u64>,
    offset_end: Option<u64>,
    flags: Option<gst::BufferFlags>,
    key_expr: Option<String>,
    user_metadata: HashMap<String, String>,
}

impl MetadataBuilder {
    /// Create a new metadata builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the GStreamer caps to transmit
    pub fn caps(mut self, caps: &gst::Caps) -> Self {
        self.caps = Some(caps.clone());
        self
    }

    /// Set buffer timing information from a GStreamer buffer
    ///
    /// This extracts PTS, DTS, duration, offset, offset_end, and flags from the buffer.
    pub fn buffer_timing(mut self, buffer: &gst::Buffer) -> Self {
        self.pts = buffer.pts();
        self.dts = buffer.dts();
        self.duration = buffer.duration();

        // Only include offset if it's valid (not u64::MAX which means "none")
        let offset = buffer.offset();
        if offset != u64::MAX {
            self.offset = Some(offset);
        }

        let offset_end = buffer.offset_end();
        if offset_end != u64::MAX {
            self.offset_end = Some(offset_end);
        }

        self.flags = Some(buffer.flags());
        self
    }

    /// Set the presentation timestamp
    pub fn pts(mut self, pts: Option<gst::ClockTime>) -> Self {
        self.pts = pts;
        self
    }

    /// Set the decoding timestamp
    pub fn dts(mut self, dts: Option<gst::ClockTime>) -> Self {
        self.dts = dts;
        self
    }

    /// Set the buffer duration
    pub fn duration(mut self, duration: Option<gst::ClockTime>) -> Self {
        self.duration = duration;
        self
    }

    /// Set the buffer flags
    pub fn flags(mut self, flags: gst::BufferFlags) -> Self {
        self.flags = Some(flags);
        self
    }

    /// Set the Zenoh key expression
    pub fn key_expr(mut self, key_expr: impl Into<String>) -> Self {
        self.key_expr = Some(key_expr.into());
        self
    }

    /// Add custom user metadata
    pub fn user_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.user_metadata.insert(key.into(), value.into());
        self
    }

    /// Build the attachment as ZBytes
    ///
    /// The attachment is encoded as a simple key-value format:
    /// - Format: "key1=value1\nkey2=value2\n..."
    /// - Caps are serialized using their string representation
    /// - Timestamps are serialized as nanoseconds
    /// - Flags are serialized as comma-separated names
    pub fn build(self) -> Option<ZBytes> {
        let mut parts = Vec::new();

        // Add version
        parts.push(format!("{}={}", keys::VERSION, METADATA_VERSION));

        // Add caps if present
        if let Some(caps) = self.caps {
            let caps_str = caps.to_string();
            // Escape newlines in caps
            let caps_escaped = caps_str.replace('\n', "\\n");
            parts.push(format!("{}={}", keys::CAPS, caps_escaped));
        }

        // Add buffer timing metadata
        if let Some(pts) = self.pts {
            parts.push(format!("{}={}", keys::PTS, pts.nseconds()));
        }
        if let Some(dts) = self.dts {
            parts.push(format!("{}={}", keys::DTS, dts.nseconds()));
        }
        if let Some(duration) = self.duration {
            parts.push(format!("{}={}", keys::DURATION, duration.nseconds()));
        }
        if let Some(offset) = self.offset {
            parts.push(format!("{}={}", keys::OFFSET, offset));
        }
        if let Some(offset_end) = self.offset_end {
            parts.push(format!("{}={}", keys::OFFSET_END, offset_end));
        }
        if let Some(flags) = self.flags {
            let flags_str = flags_to_string(flags);
            if !flags_str.is_empty() {
                parts.push(format!("{}={}", keys::FLAGS, flags_str));
            }
        }

        // Add key expression if present
        if let Some(key_expr) = self.key_expr {
            // Escape newlines in key expression (unlikely but safe)
            let key_expr_escaped = key_expr.replace('\n', "\\n");
            parts.push(format!("{}={}", keys::KEY_EXPR, key_expr_escaped));
        }

        // Add user metadata
        for (key, value) in self.user_metadata {
            let full_key = if key.starts_with(keys::USER_PREFIX) {
                key
            } else {
                format!("{}{}", keys::USER_PREFIX, key)
            };
            // Escape newlines in values
            let value_escaped = value.replace('\n', "\\n");
            parts.push(format!("{}={}", full_key, value_escaped));
        }

        if parts.is_empty() {
            None
        } else {
            let attachment_str = parts.join("\n");
            Some(ZBytes::from(attachment_str.into_bytes()))
        }
    }
}

/// Convert GStreamer buffer flags to a comma-separated string
fn flags_to_string(flags: gst::BufferFlags) -> String {
    let mut parts = Vec::new();

    if flags.contains(gst::BufferFlags::LIVE) {
        parts.push("live");
    }
    if flags.contains(gst::BufferFlags::DISCONT) {
        parts.push("discont");
    }
    if flags.contains(gst::BufferFlags::DELTA_UNIT) {
        parts.push("delta");
    }
    if flags.contains(gst::BufferFlags::HEADER) {
        parts.push("header");
    }
    if flags.contains(gst::BufferFlags::GAP) {
        parts.push("gap");
    }
    if flags.contains(gst::BufferFlags::DROPPABLE) {
        parts.push("droppable");
    }
    if flags.contains(gst::BufferFlags::MARKER) {
        parts.push("marker");
    }
    if flags.contains(gst::BufferFlags::CORRUPTED) {
        parts.push("corrupted");
    }
    if flags.contains(gst::BufferFlags::NON_DROPPABLE) {
        parts.push("non-droppable");
    }

    parts.join(",")
}

/// Parse a comma-separated string back to GStreamer buffer flags
fn string_to_flags(s: &str) -> gst::BufferFlags {
    let mut flags = gst::BufferFlags::empty();

    for part in s.split(',') {
        match part.trim() {
            "live" => flags |= gst::BufferFlags::LIVE,
            "discont" => flags |= gst::BufferFlags::DISCONT,
            "delta" => flags |= gst::BufferFlags::DELTA_UNIT,
            "header" => flags |= gst::BufferFlags::HEADER,
            "gap" => flags |= gst::BufferFlags::GAP,
            "droppable" => flags |= gst::BufferFlags::DROPPABLE,
            "marker" => flags |= gst::BufferFlags::MARKER,
            "corrupted" => flags |= gst::BufferFlags::CORRUPTED,
            "non-droppable" => flags |= gst::BufferFlags::NON_DROPPABLE,
            _ => {} // Ignore unknown flags for forward compatibility
        }
    }

    flags
}

/// Parse metadata from a Zenoh attachment
#[derive(Debug, Default)]
pub struct MetadataParser {
    caps: Option<gst::Caps>,
    pts: Option<gst::ClockTime>,
    dts: Option<gst::ClockTime>,
    duration: Option<gst::ClockTime>,
    offset: Option<u64>,
    offset_end: Option<u64>,
    flags: Option<gst::BufferFlags>,
    key_expr: Option<String>,
    user_metadata: HashMap<String, String>,
    version: Option<String>,
}

impl MetadataParser {
    /// Parse metadata from ZBytes attachment
    pub fn parse(zbytes: &ZBytes) -> Result<Self, String> {
        let bytes = zbytes.to_bytes();
        let attachment_str =
            std::str::from_utf8(&bytes).map_err(|e| format!("Invalid UTF-8: {}", e))?;

        let mut parser = MetadataParser::default();

        for line in attachment_str.lines() {
            if line.is_empty() {
                continue;
            }

            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("Invalid metadata line: {}", line))?;

            // Unescape newlines
            let value_unescaped = value.replace("\\n", "\n");

            match key {
                keys::VERSION => {
                    parser.version = Some(value_unescaped);
                }
                keys::CAPS => {
                    // Parse caps from string
                    match gst::Caps::from_str(&value_unescaped) {
                        Ok(caps) => parser.caps = Some(caps),
                        Err(_) => {
                            return Err(format!("Failed to parse caps '{}'", value_unescaped));
                        }
                    }
                }
                keys::PTS => {
                    if let Ok(ns) = value_unescaped.parse::<u64>() {
                        parser.pts = Some(gst::ClockTime::from_nseconds(ns));
                    }
                }
                keys::DTS => {
                    if let Ok(ns) = value_unescaped.parse::<u64>() {
                        parser.dts = Some(gst::ClockTime::from_nseconds(ns));
                    }
                }
                keys::DURATION => {
                    if let Ok(ns) = value_unescaped.parse::<u64>() {
                        parser.duration = Some(gst::ClockTime::from_nseconds(ns));
                    }
                }
                keys::OFFSET => {
                    if let Ok(offset) = value_unescaped.parse::<u64>() {
                        parser.offset = Some(offset);
                    }
                }
                keys::OFFSET_END => {
                    if let Ok(offset_end) = value_unescaped.parse::<u64>() {
                        parser.offset_end = Some(offset_end);
                    }
                }
                keys::FLAGS => {
                    parser.flags = Some(string_to_flags(&value_unescaped));
                }
                keys::KEY_EXPR => {
                    parser.key_expr = Some(value_unescaped);
                }
                k if k.starts_with(keys::USER_PREFIX) => {
                    let user_key = k.trim_start_matches(keys::USER_PREFIX);
                    parser
                        .user_metadata
                        .insert(user_key.to_string(), value_unescaped);
                }
                _ => {
                    // Unknown key - ignore for forward compatibility
                }
            }
        }

        Ok(parser)
    }

    /// Get the parsed caps, if any
    pub fn caps(&self) -> Option<&gst::Caps> {
        self.caps.as_ref()
    }

    /// Get the presentation timestamp
    pub fn pts(&self) -> Option<gst::ClockTime> {
        self.pts
    }

    /// Get the decoding timestamp
    pub fn dts(&self) -> Option<gst::ClockTime> {
        self.dts
    }

    /// Get the buffer duration
    pub fn duration(&self) -> Option<gst::ClockTime> {
        self.duration
    }

    /// Get the buffer offset
    pub fn offset(&self) -> Option<u64> {
        self.offset
    }

    /// Get the buffer offset end
    pub fn offset_end(&self) -> Option<u64> {
        self.offset_end
    }

    /// Get the buffer flags
    pub fn flags(&self) -> Option<gst::BufferFlags> {
        self.flags
    }

    /// Get the Zenoh key expression
    pub fn key_expr(&self) -> Option<&str> {
        self.key_expr.as_deref()
    }

    /// Get the metadata format version
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    /// Get all user metadata
    pub fn user_metadata(&self) -> &HashMap<String, String> {
        &self.user_metadata
    }

    /// Get a specific user metadata value
    pub fn get_user_metadata(&self, key: &str) -> Option<&str> {
        self.user_metadata.get(key).map(|s| s.as_str())
    }

    /// Apply parsed buffer timing to a mutable buffer
    ///
    /// This sets PTS, DTS, duration, offset, offset_end, and flags on the buffer
    /// from the parsed metadata.
    pub fn apply_to_buffer(&self, buffer: &mut gst::BufferRef) {
        if let Some(pts) = self.pts {
            buffer.set_pts(pts);
        }
        if let Some(dts) = self.dts {
            buffer.set_dts(dts);
        }
        if let Some(duration) = self.duration {
            buffer.set_duration(duration);
        }
        if let Some(offset) = self.offset {
            buffer.set_offset(offset);
        }
        if let Some(offset_end) = self.offset_end {
            buffer.set_offset_end(offset_end);
        }
        if let Some(flags) = self.flags {
            buffer.set_flags(flags);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metadata_round_trip() {
        gst::init().unwrap();

        let caps = gst::Caps::builder("video/x-raw")
            .field("width", 1920)
            .field("height", 1080)
            .field("framerate", gst::Fraction::new(30, 1))
            .build();

        let zbytes = MetadataBuilder::new()
            .caps(&caps)
            .user_metadata("custom_key", "custom_value")
            .build()
            .expect("Failed to build metadata");

        let parser = MetadataParser::parse(&zbytes).expect("Failed to parse metadata");

        assert_eq!(parser.version(), Some(METADATA_VERSION));
        assert!(parser.caps().is_some());
        assert_eq!(parser.get_user_metadata("custom_key"), Some("custom_value"));

        let parsed_caps = parser.caps().unwrap();
        let structure = parsed_caps.structure(0).unwrap();
        assert_eq!(structure.name(), "video/x-raw");
        assert_eq!(structure.get::<i32>("width").unwrap(), 1920);
    }

    #[test]
    fn test_metadata_builder_empty() {
        let zbytes = MetadataBuilder::new().build();
        // Empty builder should still create attachment with version
        assert!(zbytes.is_some());
    }

    #[test]
    fn test_metadata_caps_only() {
        gst::init().unwrap();

        let caps = gst::Caps::builder("audio/x-raw")
            .field("rate", 48000)
            .field("channels", 2)
            .build();

        let zbytes = MetadataBuilder::new()
            .caps(&caps)
            .build()
            .expect("Failed to build");

        let parser = MetadataParser::parse(&zbytes).expect("Failed to parse");
        assert!(parser.caps().is_some());
        assert_eq!(parser.user_metadata().len(), 0);
    }

    #[test]
    fn test_metadata_user_only() {
        let zbytes = MetadataBuilder::new()
            .user_metadata("key1", "value1")
            .user_metadata("key2", "value2")
            .build()
            .expect("Failed to build");

        let parser = MetadataParser::parse(&zbytes).expect("Failed to parse");
        assert!(parser.caps().is_none());
        assert_eq!(parser.user_metadata().len(), 2);
        assert_eq!(parser.get_user_metadata("key1"), Some("value1"));
        assert_eq!(parser.get_user_metadata("key2"), Some("value2"));
    }

    #[test]
    fn test_metadata_newline_escaping() {
        gst::init().unwrap();

        let value_with_newline = "line1\nline2\nline3";

        let zbytes = MetadataBuilder::new()
            .user_metadata("multiline", value_with_newline)
            .build()
            .expect("Failed to build");

        let parser = MetadataParser::parse(&zbytes).expect("Failed to parse");
        assert_eq!(
            parser.get_user_metadata("multiline"),
            Some(value_with_newline)
        );
    }

    #[test]
    fn test_buffer_timing_round_trip() {
        gst::init().unwrap();

        let pts = gst::ClockTime::from_nseconds(1_000_000_000); // 1 second
        let dts = gst::ClockTime::from_nseconds(900_000_000); // 0.9 seconds
        let duration = gst::ClockTime::from_nseconds(33_333_333); // ~30fps frame

        let zbytes = MetadataBuilder::new()
            .pts(Some(pts))
            .dts(Some(dts))
            .duration(Some(duration))
            .flags(gst::BufferFlags::DELTA_UNIT | gst::BufferFlags::DISCONT)
            .build()
            .expect("Failed to build");

        let parser = MetadataParser::parse(&zbytes).expect("Failed to parse");

        assert_eq!(parser.pts(), Some(pts));
        assert_eq!(parser.dts(), Some(dts));
        assert_eq!(parser.duration(), Some(duration));

        let flags = parser.flags().expect("Should have flags");
        assert!(flags.contains(gst::BufferFlags::DELTA_UNIT));
        assert!(flags.contains(gst::BufferFlags::DISCONT));
        assert!(!flags.contains(gst::BufferFlags::HEADER));
    }

    #[test]
    fn test_buffer_timing_from_buffer() {
        gst::init().unwrap();

        // Create a buffer with timing info
        let mut buffer = gst::Buffer::with_size(100).unwrap();
        {
            let buffer_ref = buffer.get_mut().unwrap();
            buffer_ref.set_pts(gst::ClockTime::from_nseconds(2_000_000_000));
            buffer_ref.set_dts(gst::ClockTime::from_nseconds(1_900_000_000));
            buffer_ref.set_duration(gst::ClockTime::from_nseconds(40_000_000));
            buffer_ref.set_offset(1024);
            buffer_ref.set_offset_end(2048);
            buffer_ref.set_flags(gst::BufferFlags::HEADER | gst::BufferFlags::MARKER);
        }

        // Build metadata from buffer
        let zbytes = MetadataBuilder::new()
            .buffer_timing(&buffer)
            .build()
            .expect("Failed to build");

        // Parse it back
        let parser = MetadataParser::parse(&zbytes).expect("Failed to parse");

        assert_eq!(
            parser.pts(),
            Some(gst::ClockTime::from_nseconds(2_000_000_000))
        );
        assert_eq!(
            parser.dts(),
            Some(gst::ClockTime::from_nseconds(1_900_000_000))
        );
        assert_eq!(
            parser.duration(),
            Some(gst::ClockTime::from_nseconds(40_000_000))
        );
        assert_eq!(parser.offset(), Some(1024));
        assert_eq!(parser.offset_end(), Some(2048));

        let flags = parser.flags().expect("Should have flags");
        assert!(flags.contains(gst::BufferFlags::HEADER));
        assert!(flags.contains(gst::BufferFlags::MARKER));
    }

    #[test]
    fn test_apply_to_buffer() {
        gst::init().unwrap();

        let pts = gst::ClockTime::from_nseconds(3_000_000_000);
        let dts = gst::ClockTime::from_nseconds(2_900_000_000);
        let duration = gst::ClockTime::from_nseconds(16_666_667); // ~60fps

        let zbytes = MetadataBuilder::new()
            .pts(Some(pts))
            .dts(Some(dts))
            .duration(Some(duration))
            .flags(gst::BufferFlags::LIVE | gst::BufferFlags::DISCONT)
            .build()
            .expect("Failed to build");

        let parser = MetadataParser::parse(&zbytes).expect("Failed to parse");

        // Create a new buffer and apply the parsed timing
        let mut buffer = gst::Buffer::with_size(100).unwrap();
        {
            let buffer_ref = buffer.get_mut().unwrap();
            parser.apply_to_buffer(buffer_ref);
        }

        // Verify the buffer has the correct timing
        assert_eq!(buffer.pts(), Some(pts));
        assert_eq!(buffer.dts(), Some(dts));
        assert_eq!(buffer.duration(), Some(duration));
        assert!(buffer.flags().contains(gst::BufferFlags::LIVE));
        assert!(buffer.flags().contains(gst::BufferFlags::DISCONT));
    }

    #[test]
    fn test_flags_serialization() {
        // Test all supported flags
        let all_flags = gst::BufferFlags::LIVE
            | gst::BufferFlags::DISCONT
            | gst::BufferFlags::DELTA_UNIT
            | gst::BufferFlags::HEADER
            | gst::BufferFlags::GAP
            | gst::BufferFlags::DROPPABLE
            | gst::BufferFlags::MARKER
            | gst::BufferFlags::CORRUPTED
            | gst::BufferFlags::NON_DROPPABLE;

        let flags_str = flags_to_string(all_flags);
        let parsed_flags = string_to_flags(&flags_str);

        assert!(parsed_flags.contains(gst::BufferFlags::LIVE));
        assert!(parsed_flags.contains(gst::BufferFlags::DISCONT));
        assert!(parsed_flags.contains(gst::BufferFlags::DELTA_UNIT));
        assert!(parsed_flags.contains(gst::BufferFlags::HEADER));
        assert!(parsed_flags.contains(gst::BufferFlags::GAP));
        assert!(parsed_flags.contains(gst::BufferFlags::DROPPABLE));
        assert!(parsed_flags.contains(gst::BufferFlags::MARKER));
        assert!(parsed_flags.contains(gst::BufferFlags::CORRUPTED));
        assert!(parsed_flags.contains(gst::BufferFlags::NON_DROPPABLE));
    }

    #[test]
    fn test_empty_flags() {
        let empty_flags = gst::BufferFlags::empty();
        let flags_str = flags_to_string(empty_flags);
        assert!(flags_str.is_empty());

        let parsed = string_to_flags("");
        assert!(parsed.is_empty());
    }

    #[test]
    fn test_combined_caps_and_timing() {
        gst::init().unwrap();

        let caps = gst::Caps::builder("video/x-raw")
            .field("width", 1280)
            .field("height", 720)
            .build();

        let pts = gst::ClockTime::from_nseconds(5_000_000_000);

        let zbytes = MetadataBuilder::new()
            .caps(&caps)
            .pts(Some(pts))
            .duration(Some(gst::ClockTime::from_nseconds(33_333_333)))
            .flags(gst::BufferFlags::DELTA_UNIT)
            .user_metadata("source", "camera1")
            .build()
            .expect("Failed to build");

        let parser = MetadataParser::parse(&zbytes).expect("Failed to parse");

        // Verify all fields
        assert!(parser.caps().is_some());
        assert_eq!(parser.pts(), Some(pts));
        assert_eq!(
            parser.duration(),
            Some(gst::ClockTime::from_nseconds(33_333_333))
        );
        assert!(
            parser
                .flags()
                .unwrap()
                .contains(gst::BufferFlags::DELTA_UNIT)
        );
        assert_eq!(parser.get_user_metadata("source"), Some("camera1"));

        // Verify caps content
        let parsed_caps = parser.caps().unwrap();
        let structure = parsed_caps.structure(0).unwrap();
        assert_eq!(structure.get::<i32>("width").unwrap(), 1280);
    }

    #[test]
    fn test_backward_compatibility() {
        gst::init().unwrap();

        // Simulate a v1.0 message without timing fields
        let old_format = "gst.version=1.0\ngst.caps=video/x-raw";
        let zbytes = ZBytes::from(old_format.as_bytes().to_vec());

        let parser = MetadataParser::parse(&zbytes).expect("Failed to parse");

        // Should parse successfully with None for timing fields
        assert_eq!(parser.version(), Some("1.0"));
        assert!(parser.caps().is_some());
        assert!(parser.pts().is_none());
        assert!(parser.dts().is_none());
        assert!(parser.duration().is_none());
        assert!(parser.flags().is_none());
    }

    #[test]
    fn test_malformed_metadata_missing_equals() {
        // Line without '=' should fail
        let malformed = "gst.version=1.0\ninvalid_line_without_equals";
        let zbytes = ZBytes::from(malformed.as_bytes().to_vec());

        let result = MetadataParser::parse(&zbytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid metadata line"));
    }

    #[test]
    fn test_malformed_metadata_invalid_utf8() {
        // Invalid UTF-8 bytes
        let invalid_utf8: Vec<u8> = vec![0xff, 0xfe, 0x00, 0x01];
        let zbytes = ZBytes::from(invalid_utf8);

        let result = MetadataParser::parse(&zbytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid UTF-8"));
    }

    #[test]
    fn test_malformed_metadata_invalid_caps() {
        gst::init().unwrap();

        // Invalid caps string
        let invalid_caps = "gst.version=1.0\ngst.caps=not_a_valid_caps!!!";
        let zbytes = ZBytes::from(invalid_caps.as_bytes().to_vec());

        let result = MetadataParser::parse(&zbytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse caps"));
    }

    #[test]
    fn test_malformed_metadata_invalid_timestamp() {
        // Invalid timestamp (not a number) - should be silently ignored
        let invalid_ts = "gst.version=1.0\ngst.pts=not_a_number";
        let zbytes = ZBytes::from(invalid_ts.as_bytes().to_vec());

        // Should parse successfully, just skip the invalid field
        let parser =
            MetadataParser::parse(&zbytes).expect("Should parse despite invalid timestamp");
        assert!(parser.pts().is_none()); // Invalid value is skipped
    }

    #[test]
    fn test_unknown_keys_forward_compatibility() {
        // Unknown keys should be ignored for forward compatibility
        let future_format = "gst.version=2.0\ngst.future_field=some_value\ngst.pts=1000000000";
        let zbytes = ZBytes::from(future_format.as_bytes().to_vec());

        let parser = MetadataParser::parse(&zbytes).expect("Should parse with unknown keys");
        assert_eq!(parser.version(), Some("2.0"));
        assert_eq!(
            parser.pts(),
            Some(gst::ClockTime::from_nseconds(1_000_000_000))
        );
    }

    #[test]
    fn test_empty_metadata() {
        let empty = "";
        let zbytes = ZBytes::from(empty.as_bytes().to_vec());

        let parser = MetadataParser::parse(&zbytes).expect("Should parse empty metadata");
        assert!(parser.version().is_none());
        assert!(parser.caps().is_none());
        assert!(parser.pts().is_none());
    }

    #[test]
    fn test_unknown_flags_ignored() {
        // Unknown flags should be ignored
        let flags_str = "live,discont,future_flag,another_unknown";
        let parsed = string_to_flags(flags_str);

        // Known flags should be parsed
        assert!(parsed.contains(gst::BufferFlags::LIVE));
        assert!(parsed.contains(gst::BufferFlags::DISCONT));
        // Unknown flags are silently ignored (no panic)
    }
}
