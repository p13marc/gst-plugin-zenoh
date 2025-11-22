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
}

/// Current metadata format version
pub const METADATA_VERSION: &str = "1.0";

/// Builder for creating Zenoh attachments with GStreamer metadata
#[derive(Debug, Default)]
pub struct MetadataBuilder {
    caps: Option<gst::Caps>,
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

/// Parse metadata from a Zenoh attachment
#[derive(Debug, Default)]
pub struct MetadataParser {
    caps: Option<gst::Caps>,
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
                            return Err(format!("Failed to parse caps '{}'", value_unescaped))
                        }
                    }
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
}
