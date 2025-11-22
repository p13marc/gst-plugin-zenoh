//! Error handling for the Zenoh GStreamer plugin

use thiserror::Error;

/// Custom error type for Zenoh operations
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum ZenohError {
    /// Error initializing Zenoh session
    #[error("Failed to initialize Zenoh session: {0}")]
    InitError(#[source] zenoh::Error),

    /// Error with configuration file
    #[error("Failed to load Zenoh configuration from '{path}': {source}")]
    ConfigError {
        path: String,
        #[source]
        source: zenoh::Error,
    },

    /// Error with key expression
    #[error("Invalid key expression '{key_expr}': {reason}")]
    KeyExprError { key_expr: String, reason: String },

    /// Error publishing data
    #[error("Failed to publish to '{key_expr}': {source}")]
    PublishError {
        key_expr: String,
        #[source]
        source: zenoh::Error,
    },

    /// Network timeout error
    #[error("Network timeout while publishing to '{key_expr}' (timeout: {timeout_ms}ms)")]
    TimeoutError { key_expr: String, timeout_ms: u64 },

    /// Network connection error
    #[error("Network connection error for '{key_expr}': {details}")]
    ConnectionError { key_expr: String, details: String },

    /// Error receiving data
    #[error("Failed to receive from '{key_expr}': {reason}")]
    ReceiveError { key_expr: String, reason: String },

    /// Buffer mapping error
    #[error("Failed to map GStreamer buffer: {reason}")]
    BufferError { reason: String },

    /// Session closed or invalid
    #[error("Zenoh session is closed or invalid")]
    SessionClosedError,

    /// Resource not available
    #[error("Zenoh resource not available: {resource}")]
    ResourceUnavailable { resource: String },
}

/// Extension trait to convert errors to GStreamer error messages
pub trait ErrorHandling {
    /// Convert to GStreamer error message
    fn to_error_message(&self) -> gst::ErrorMessage;
}

impl ErrorHandling for ZenohError {
    fn to_error_message(&self) -> gst::ErrorMessage {
        // Convert our custom error to a GStreamer error message with appropriate domain/code
        match self {
            ZenohError::InitError(err) => {
                gst::error_msg!(
                    gst::ResourceError::OpenRead,
                    ["Zenoh session initialization failed: {}. Check network connectivity and Zenoh configuration.", err]
                )
            }
            ZenohError::ConfigError { path, source } => {
                gst::error_msg!(
                    gst::ResourceError::Settings,
                    ["Failed to load Zenoh configuration file '{}': {}. Verify file exists and has valid JSON5 syntax.", path, source]
                )
            }
            ZenohError::KeyExprError { key_expr, reason } => {
                gst::error_msg!(
                    gst::ResourceError::Settings,
                    ["Invalid Zenoh key expression '{}': {}. Key expressions must be valid Zenoh paths (e.g., 'demo/video' or 'sensors/**').", key_expr, reason]
                )
            }
            ZenohError::PublishError { key_expr, source } => {
                gst::error_msg!(
                    gst::ResourceError::Write,
                    ["Failed to publish data to '{}': {}. This may indicate network issues or session problems.", key_expr, source]
                )
            }
            ZenohError::TimeoutError {
                key_expr,
                timeout_ms,
            } => {
                gst::error_msg!(
                    gst::ResourceError::Write,
                    ["Network timeout publishing to '{}' ({}ms). Check network connectivity and Zenoh routers.", key_expr, timeout_ms]
                )
            }
            ZenohError::ConnectionError { key_expr, details } => {
                gst::error_msg!(
                    gst::ResourceError::OpenWrite,
                    [
                        "Connection error for '{}': {}. Verify Zenoh network is accessible.",
                        key_expr,
                        details
                    ]
                )
            }
            ZenohError::ReceiveError { key_expr, reason } => {
                gst::error_msg!(
                    gst::ResourceError::Read,
                    ["Failed to receive from '{}': {}. Check if publisher is active and key expression matches.", key_expr, reason]
                )
            }
            ZenohError::BufferError { reason } => {
                gst::error_msg!(
                    gst::ResourceError::Failed,
                    [
                        "GStreamer buffer operation failed: {}. This is an internal error.",
                        reason
                    ]
                )
            }
            ZenohError::SessionClosedError => {
                gst::error_msg!(
                    gst::ResourceError::Close,
                    ["Zenoh session has been closed. Element may need to be restarted."]
                )
            }
            ZenohError::ResourceUnavailable { resource } => {
                gst::error_msg!(
                    gst::ResourceError::NotFound,
                    ["Zenoh resource '{}' is not available. Verify resource exists in the Zenoh network.", resource]
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
        // Convert our custom error to an appropriate FlowError type
        match self {
            ZenohError::InitError(_) => gst::FlowError::NotNegotiated,
            ZenohError::ConfigError { .. } => gst::FlowError::NotNegotiated,
            ZenohError::KeyExprError { .. } => gst::FlowError::NotNegotiated,
            ZenohError::PublishError { .. } => gst::FlowError::Error,
            ZenohError::TimeoutError { .. } => gst::FlowError::Error,
            ZenohError::ConnectionError { .. } => gst::FlowError::Error,
            ZenohError::ReceiveError { .. } => gst::FlowError::Error,
            ZenohError::BufferError { .. } => gst::FlowError::Error,
            ZenohError::SessionClosedError => gst::FlowError::Eos,
            ZenohError::ResourceUnavailable { .. } => gst::FlowError::NotLinked,
        }
    }
}
