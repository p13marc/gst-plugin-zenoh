//! Error handling for the Zenoh GStreamer plugin

use thiserror::Error;

/// Custom error type for Zenoh operations
#[derive(Debug, Error)]
pub enum ZenohError {
    /// Error initializing Zenoh session
    #[error("Failed to initialize Zenoh session: {0}")]
    Init(#[source] zenoh::Error),

    /// Error with key expression
    #[error("Invalid key expression '{key_expr}': {reason}")]
    KeyExpr { key_expr: String, reason: String },

    /// Error publishing data
    #[error("Failed to publish to '{key_expr}': {source}")]
    Publish {
        key_expr: String,
        #[source]
        source: zenoh::Error,
    },
}

/// Extension trait to convert errors to GStreamer error messages
pub trait ErrorHandling {
    /// Convert to GStreamer error message
    fn to_error_message(&self) -> gst::ErrorMessage;
}

impl ErrorHandling for ZenohError {
    fn to_error_message(&self) -> gst::ErrorMessage {
        match self {
            ZenohError::Init(err) => {
                gst::error_msg!(
                    gst::ResourceError::OpenRead,
                    [
                        "Zenoh session initialization failed: {}. Check network connectivity and Zenoh configuration.",
                        err
                    ]
                )
            }
            ZenohError::KeyExpr { key_expr, reason } => {
                gst::error_msg!(
                    gst::ResourceError::Settings,
                    [
                        "Invalid Zenoh key expression '{}': {}. Key expressions must be valid Zenoh paths (e.g., 'demo/video' or 'sensors/**').",
                        key_expr,
                        reason
                    ]
                )
            }
            ZenohError::Publish { key_expr, source } => {
                gst::error_msg!(
                    gst::ResourceError::Write,
                    [
                        "Failed to publish data to '{}': {}. This may indicate network issues or session problems.",
                        key_expr,
                        source
                    ]
                )
            }
        }
    }
}

/// Extension trait to convert errors to FlowError
pub trait FlowErrorHandling {
    /// Convert to GStreamer FlowError
    fn to_flow_error(&self) -> gst::FlowError;
}

impl FlowErrorHandling for ZenohError {
    fn to_flow_error(&self) -> gst::FlowError {
        match self {
            ZenohError::Init(_) => gst::FlowError::NotNegotiated,
            ZenohError::KeyExpr { .. } => gst::FlowError::NotNegotiated,
            ZenohError::Publish { .. } => gst::FlowError::Error,
        }
    }
}
