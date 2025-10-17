//! Error handling for the Zenoh GStreamer plugin

use thiserror::Error;

/// Custom error type for Zenoh operations
#[derive(Debug, Error)]
pub enum ZenohError {
    /// Error initializing Zenoh
    #[error("Failed to initialize Zenoh: {0}")]
    InitError(#[source] zenoh::Error),

    /// Error with key expression
    #[error("Invalid key expression: {0}")]
    KeyExprError(String),
    
    /// Error publishing data
    #[error("Failed to publish data: {0}")]
    PublishError(#[source] zenoh::Error),
    
    /// Error receiving data
    #[error("Failed to receive data: {0}")]
    ReceiveError(String),
    
    // We don't use this variant directly in the code, but it's kept for future use
    #[allow(dead_code)]
    /// Buffer mapping error
    #[error("Failed to map buffer: {0}")]
    BufferError(String),
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
                gst::error_msg!(gst::ResourceError::OpenRead, ["Zenoh initialization error: {}", err])
            }
            ZenohError::KeyExprError(err) => {
                gst::error_msg!(gst::ResourceError::Settings, ["Key expression error: {}", err])
            }
            ZenohError::PublishError(err) => {
                gst::error_msg!(gst::ResourceError::Write, ["Failed to publish data: {}", err])
            }
            ZenohError::ReceiveError(err) => {
                gst::error_msg!(gst::ResourceError::Read, ["Failed to receive data: {}", err])
            }
            ZenohError::BufferError(err) => {
                gst::error_msg!(gst::ResourceError::Failed, ["Buffer error: {}", err])
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
            ZenohError::KeyExprError(_) => gst::FlowError::NotNegotiated,
            ZenohError::PublishError(_) => gst::FlowError::Error,
            ZenohError::ReceiveError(_) => gst::FlowError::Error,
            ZenohError::BufferError(_) => gst::FlowError::Error,
        }
    }
}