// SPDX-License-Identifier: MPL-2.0

//! Compression support for gst-plugin-zenoh
//!
//! This module provides optional compression/decompression functionality for buffer data.
//! Compression algorithms are enabled via Cargo features:
//! - `compression-zstd`: Zstandard compression (recommended for general use)
//! - `compression-lz4`: LZ4 compression (fastest, lower compression ratio)
//! - `compression-gzip`: Gzip compression (widely compatible)
//!
//! Each compression algorithm can be individually enabled or all can be enabled with the
//! `compression` feature.

use gst::glib;
use thiserror::Error;

/// Compression algorithm selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, glib::Enum, Default)]
#[enum_type(name = "GstZenohCompression")]
pub enum CompressionType {
    /// No compression
    #[default]
    #[enum_value(name = "None", nick = "none")]
    None,

    /// Zstandard compression (requires compression-zstd feature)
    #[cfg(feature = "compression-zstd")]
    #[enum_value(name = "Zstd", nick = "zstd")]
    Zstd,

    /// LZ4 compression (requires compression-lz4 feature)
    #[cfg(feature = "compression-lz4")]
    #[enum_value(name = "Lz4", nick = "lz4")]
    Lz4,

    /// Gzip compression (requires compression-gzip feature)
    #[cfg(feature = "compression-gzip")]
    #[enum_value(name = "Gzip", nick = "gzip")]
    Gzip,
}

impl CompressionType {
    /// Convert to metadata key value
    pub fn to_metadata_value(&self) -> &'static str {
        match self {
            Self::None => "none",
            #[cfg(feature = "compression-zstd")]
            Self::Zstd => "zstd",
            #[cfg(feature = "compression-lz4")]
            Self::Lz4 => "lz4",
            #[cfg(feature = "compression-gzip")]
            Self::Gzip => "gzip",
        }
    }

    /// Parse from metadata key value
    pub fn from_metadata_value(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            #[cfg(feature = "compression-zstd")]
            "zstd" => Some(Self::Zstd),
            #[cfg(feature = "compression-lz4")]
            "lz4" => Some(Self::Lz4),
            #[cfg(feature = "compression-gzip")]
            "gzip" => Some(Self::Gzip),
            _ => None,
        }
    }
}

/// Compression errors
#[derive(Error, Debug)]
pub enum CompressionError {
    #[error("Compression failed: {0}")]
    CompressionFailed(String),

    #[error("Decompression failed: {0}")]
    DecompressionFailed(String),

    #[error("Unsupported compression type: {0}")]
    UnsupportedType(String),

    #[error("Invalid compression level: {0}")]
    InvalidLevel(i32),
}

/// Compress data using the specified algorithm and level
///
/// # Arguments
/// * `data` - Input data to compress
/// * `compression_type` - Compression algorithm to use
/// * `level` - Compression level (1-9, algorithm-specific interpretation)
///
/// # Returns
/// Compressed data or error
pub fn compress(
    data: &[u8],
    compression_type: CompressionType,
    level: i32,
) -> Result<Vec<u8>, CompressionError> {
    if !(1..=9).contains(&level) {
        return Err(CompressionError::InvalidLevel(level));
    }

    match compression_type {
        CompressionType::None => Ok(data.to_vec()),

        #[cfg(feature = "compression-zstd")]
        CompressionType::Zstd => zstd::encode_all(data, level)
            .map_err(|e| CompressionError::CompressionFailed(e.to_string())),

        #[cfg(feature = "compression-lz4")]
        CompressionType::Lz4 => {
            // LZ4 doesn't have traditional levels 1-9, but we map them to acceleration
            // Higher acceleration = faster but less compression
            // Level 1 = high compression (acceleration 1)
            // Level 9 = fast compression (acceleration 9)
            let acceleration = level;
            lz4::block::compress(
                data,
                Some(lz4::block::CompressionMode::FAST(acceleration)),
                false,
            )
            .map_err(|e| CompressionError::CompressionFailed(e.to_string()))
        }

        #[cfg(feature = "compression-gzip")]
        CompressionType::Gzip => {
            use flate2::write::GzEncoder;
            use flate2::Compression;
            use std::io::Write;

            let mut encoder = GzEncoder::new(Vec::new(), Compression::new(level as u32));
            encoder
                .write_all(data)
                .map_err(|e| CompressionError::CompressionFailed(e.to_string()))?;
            encoder
                .finish()
                .map_err(|e| CompressionError::CompressionFailed(e.to_string()))
        }

        #[cfg(not(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        )))]
        _ => Err(CompressionError::UnsupportedType(format!(
            "{:?}",
            compression_type
        ))),
    }
}

/// Decompress data using the specified algorithm
///
/// # Arguments
/// * `data` - Compressed input data
/// * `compression_type` - Compression algorithm used
///
/// # Returns
/// Decompressed data or error
pub fn decompress(
    data: &[u8],
    compression_type: CompressionType,
) -> Result<Vec<u8>, CompressionError> {
    match compression_type {
        CompressionType::None => Ok(data.to_vec()),

        #[cfg(feature = "compression-zstd")]
        CompressionType::Zstd => {
            zstd::decode_all(data).map_err(|e| CompressionError::DecompressionFailed(e.to_string()))
        }

        #[cfg(feature = "compression-lz4")]
        CompressionType::Lz4 => {
            // LZ4 needs to know the decompressed size, but we don't store it
            // We'll use a reasonable max size (16MB) for decompression
            const MAX_DECOMPRESSED_SIZE: i32 = 16 * 1024 * 1024;
            lz4::block::decompress(data, Some(MAX_DECOMPRESSED_SIZE))
                .map_err(|e| CompressionError::DecompressionFailed(e.to_string()))
        }

        #[cfg(feature = "compression-gzip")]
        CompressionType::Gzip => {
            use flate2::read::GzDecoder;
            use std::io::Read;

            let mut decoder = GzDecoder::new(data);
            let mut decompressed = Vec::new();
            decoder
                .read_to_end(&mut decompressed)
                .map_err(|e| CompressionError::DecompressionFailed(e.to_string()))?;
            Ok(decompressed)
        }

        #[cfg(not(any(
            feature = "compression-zstd",
            feature = "compression-lz4",
            feature = "compression-gzip"
        )))]
        _ => Err(CompressionError::UnsupportedType(format!(
            "{:?}",
            compression_type
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_type_metadata_conversion() {
        assert_eq!(CompressionType::None.to_metadata_value(), "none");
        assert_eq!(
            CompressionType::from_metadata_value("none"),
            Some(CompressionType::None)
        );

        #[cfg(feature = "compression-zstd")]
        {
            assert_eq!(CompressionType::Zstd.to_metadata_value(), "zstd");
            assert_eq!(
                CompressionType::from_metadata_value("zstd"),
                Some(CompressionType::Zstd)
            );
        }

        #[cfg(feature = "compression-lz4")]
        {
            assert_eq!(CompressionType::Lz4.to_metadata_value(), "lz4");
            assert_eq!(
                CompressionType::from_metadata_value("lz4"),
                Some(CompressionType::Lz4)
            );
        }

        #[cfg(feature = "compression-gzip")]
        {
            assert_eq!(CompressionType::Gzip.to_metadata_value(), "gzip");
            assert_eq!(
                CompressionType::from_metadata_value("gzip"),
                Some(CompressionType::Gzip)
            );
        }

        assert_eq!(CompressionType::from_metadata_value("invalid"), None);
    }

    #[test]
    fn test_no_compression() {
        let data = b"Hello, World!";
        let compressed = compress(data, CompressionType::None, 5).unwrap();
        assert_eq!(compressed, data);

        let decompressed = decompress(&compressed, CompressionType::None).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn test_invalid_level() {
        let data = b"test";
        assert!(compress(data, CompressionType::None, 0).is_err());
        assert!(compress(data, CompressionType::None, 10).is_err());
    }

    #[cfg(feature = "compression-zstd")]
    #[test]
    fn test_zstd_compression() {
        let data = b"This is a test string that should compress well with repeated patterns repeated patterns";
        let compressed = compress(data, CompressionType::Zstd, 5).unwrap();

        // Compressed data should be smaller (for this specific test data)
        assert!(compressed.len() < data.len());

        let decompressed = decompress(&compressed, CompressionType::Zstd).unwrap();
        assert_eq!(decompressed, data);
    }

    #[cfg(feature = "compression-lz4")]
    #[test]
    fn test_lz4_compression() {
        let data = b"This is a test string that should compress well with repeated patterns repeated patterns";
        let compressed = compress(data, CompressionType::Lz4, 5).unwrap();

        let decompressed = decompress(&compressed, CompressionType::Lz4).unwrap();
        assert_eq!(decompressed, data);
    }

    #[cfg(feature = "compression-gzip")]
    #[test]
    fn test_gzip_compression() {
        let data = b"This is a test string that should compress well with repeated patterns repeated patterns";
        let compressed = compress(data, CompressionType::Gzip, 5).unwrap();

        // Compressed data should be smaller
        assert!(compressed.len() < data.len());

        let decompressed = decompress(&compressed, CompressionType::Gzip).unwrap();
        assert_eq!(decompressed, data);
    }

    #[cfg(feature = "compression-zstd")]
    #[test]
    fn test_compression_levels() {
        let data = b"Test data for compression levels";

        let level1 = compress(data, CompressionType::Zstd, 1).unwrap();
        let level9 = compress(data, CompressionType::Zstd, 9).unwrap();

        // Both should decompress correctly
        assert_eq!(decompress(&level1, CompressionType::Zstd).unwrap(), data);
        assert_eq!(decompress(&level9, CompressionType::Zstd).unwrap(), data);

        // Higher compression level should generally produce smaller output
        // (not guaranteed for small data, but likely for this test)
    }
}
