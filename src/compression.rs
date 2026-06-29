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

/// Self-describing frame for compressed payloads.
///
/// Layout: `MAGIC(3) | algo(1) | version(1) | compressed-bytes...`
///
/// The magic + algo + version header makes a compressed payload recognizable on
/// the wire regardless of the receiver's feature set, so a receiver that lacks
/// the matching decoder produces a clear error instead of forwarding garbage.
/// Uncompressed (`None`) payloads carry **no** frame (zero overhead).
const FRAME_MAGIC: &[u8; 3] = b"GZC";
const FRAME_VERSION: u8 = 1;
const FRAME_HEADER_LEN: usize = 5;

const ALGO_ZSTD: u8 = 1;
const ALGO_LZ4: u8 = 2;
const ALGO_GZIP: u8 = 3;

/// Map a (compressing) algorithm to its on-wire frame byte.
fn algo_to_byte(compression_type: CompressionType) -> u8 {
    match compression_type {
        CompressionType::None => 0,
        #[cfg(feature = "compression-zstd")]
        CompressionType::Zstd => ALGO_ZSTD,
        #[cfg(feature = "compression-lz4")]
        CompressionType::Lz4 => ALGO_LZ4,
        #[cfg(feature = "compression-gzip")]
        CompressionType::Gzip => ALGO_GZIP,
    }
}

/// Compress data using the specified algorithm and level, wrapping the result in
/// a self-describing frame.
///
/// # Arguments
/// * `data` - Input data to compress
/// * `compression_type` - Compression algorithm to use
/// * `level` - Compression level (1-9, higher = better compression)
///
/// # Returns
/// Framed compressed data, or the raw data unchanged for `None`.
pub fn compress(
    data: &[u8],
    compression_type: CompressionType,
    level: i32,
) -> Result<Vec<u8>, CompressionError> {
    if !(1..=9).contains(&level) {
        return Err(CompressionError::InvalidLevel(level));
    }

    // No compression: return the data unframed so the zero-copy path is preserved.
    if compression_type == CompressionType::None {
        return Ok(data.to_vec());
    }

    let payload = match compression_type {
        CompressionType::None => unreachable!(),

        #[cfg(feature = "compression-zstd")]
        CompressionType::Zstd => zstd::encode_all(data, level)
            .map_err(|e| CompressionError::CompressionFailed(e.to_string()))?,

        #[cfg(feature = "compression-lz4")]
        CompressionType::Lz4 => {
            // Use LZ4 high-compression mode so a higher `level` means *better*
            // compression (matching the documented semantics), and `prepend_size`
            // so the decompressor learns the size from the block itself — removing
            // the old hardcoded 16 MB decompression ceiling.
            lz4::block::compress(
                data,
                Some(lz4::block::CompressionMode::HIGHCOMPRESSION(level)),
                true,
            )
            .map_err(|e| CompressionError::CompressionFailed(e.to_string()))?
        }

        #[cfg(feature = "compression-gzip")]
        CompressionType::Gzip => {
            use flate2::Compression;
            use flate2::write::GzEncoder;
            use std::io::Write;

            let mut encoder = GzEncoder::new(Vec::new(), Compression::new(level as u32));
            encoder
                .write_all(data)
                .map_err(|e| CompressionError::CompressionFailed(e.to_string()))?;
            encoder
                .finish()
                .map_err(|e| CompressionError::CompressionFailed(e.to_string()))?
        }
    };

    let mut framed = Vec::with_capacity(FRAME_HEADER_LEN + payload.len());
    framed.extend_from_slice(FRAME_MAGIC);
    framed.push(algo_to_byte(compression_type));
    framed.push(FRAME_VERSION);
    framed.extend_from_slice(&payload);
    Ok(framed)
}

/// Decompress a framed payload produced by [`compress`].
///
/// `declared_type` is the algorithm advertised in the message metadata. For
/// `None` the data is returned unchanged; otherwise the self-describing frame is
/// validated (magic + version) and dispatched by the algorithm byte it carries,
/// so a mismatched/corrupt payload or a missing decoder fails with a clear error
/// rather than forwarding garbage downstream.
pub fn decompress(
    data: &[u8],
    declared_type: CompressionType,
) -> Result<Vec<u8>, CompressionError> {
    if declared_type == CompressionType::None {
        return Ok(data.to_vec());
    }

    if data.len() < FRAME_HEADER_LEN || &data[0..3] != FRAME_MAGIC {
        return Err(CompressionError::DecompressionFailed(
            "compressed payload is missing the expected frame header".to_string(),
        ));
    }
    let algo = data[3];
    let version = data[4];
    if version != FRAME_VERSION {
        return Err(CompressionError::DecompressionFailed(format!(
            "unsupported compression frame version {version}"
        )));
    }
    let payload = &data[FRAME_HEADER_LEN..];

    match algo {
        ALGO_ZSTD => decompress_zstd(payload),
        ALGO_LZ4 => decompress_lz4(payload),
        ALGO_GZIP => decompress_gzip(payload),
        other => Err(CompressionError::UnsupportedType(format!(
            "unknown compression algorithm byte {other}"
        ))),
    }
}

fn decompress_zstd(payload: &[u8]) -> Result<Vec<u8>, CompressionError> {
    #[cfg(feature = "compression-zstd")]
    {
        zstd::decode_all(payload).map_err(|e| CompressionError::DecompressionFailed(e.to_string()))
    }
    #[cfg(not(feature = "compression-zstd"))]
    {
        let _ = payload;
        Err(CompressionError::UnsupportedType(
            "payload is zstd-compressed but the compression-zstd feature is not enabled"
                .to_string(),
        ))
    }
}

fn decompress_lz4(payload: &[u8]) -> Result<Vec<u8>, CompressionError> {
    #[cfg(feature = "compression-lz4")]
    {
        // `None` size hint: the decompressed size is read from the block's own
        // prepended length header (no 16 MB ceiling).
        lz4::block::decompress(payload, None)
            .map_err(|e| CompressionError::DecompressionFailed(e.to_string()))
    }
    #[cfg(not(feature = "compression-lz4"))]
    {
        let _ = payload;
        Err(CompressionError::UnsupportedType(
            "payload is lz4-compressed but the compression-lz4 feature is not enabled".to_string(),
        ))
    }
}

fn decompress_gzip(payload: &[u8]) -> Result<Vec<u8>, CompressionError> {
    #[cfg(feature = "compression-gzip")]
    {
        use std::io::Read;

        use flate2::read::GzDecoder;

        let mut decoder = GzDecoder::new(payload);
        let mut decompressed = Vec::new();
        decoder
            .read_to_end(&mut decompressed)
            .map_err(|e| CompressionError::DecompressionFailed(e.to_string()))?;
        Ok(decompressed)
    }
    #[cfg(not(feature = "compression-gzip"))]
    {
        let _ = payload;
        Err(CompressionError::UnsupportedType(
            "payload is gzip-compressed but the compression-gzip feature is not enabled"
                .to_string(),
        ))
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
        // Repeated payload so compression beats the small frame header overhead.
        let data = b"This is a test string that should compress well with repeated patterns repeated patterns".repeat(8);
        let compressed = compress(&data, CompressionType::Zstd, 5).unwrap();

        // Compressed (framed) data should still be smaller than the input.
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
        // Repeated payload so compression beats the small frame header overhead.
        let data = b"This is a test string that should compress well with repeated patterns repeated patterns".repeat(8);
        let compressed = compress(&data, CompressionType::Gzip, 5).unwrap();

        // Compressed (framed) data should still be smaller than the input.
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

    /// A frame decompressing to >16 MB must round-trip (the old LZ4 path had a
    /// hardcoded 16 MB decompression ceiling).
    #[cfg(feature = "compression-lz4")]
    #[test]
    fn test_lz4_large_frame_over_16mb_roundtrips() {
        // 20 MB of highly-compressible data.
        let size = 20 * 1024 * 1024;
        let mut data = Vec::with_capacity(size);
        let pattern = b"gst-plugin-zenoh-lz4-large-frame-";
        while data.len() < size {
            data.extend_from_slice(pattern);
        }
        data.truncate(size);

        let compressed = compress(&data, CompressionType::Lz4, 5).unwrap();
        let decompressed = decompress(&compressed, CompressionType::Lz4).unwrap();
        assert_eq!(decompressed.len(), data.len());
        assert_eq!(decompressed, data);
    }

    /// Higher LZ4 level must not produce *larger* output (level semantics were
    /// previously inverted).
    #[cfg(feature = "compression-lz4")]
    #[test]
    fn test_lz4_higher_level_not_worse() {
        let mut data = Vec::new();
        for _ in 0..4096 {
            data.extend_from_slice(b"repeating compressible payload ");
        }
        let low = compress(&data, CompressionType::Lz4, 1).unwrap();
        let high = compress(&data, CompressionType::Lz4, 9).unwrap();
        assert!(
            high.len() <= low.len(),
            "level 9 ({}) should be <= level 1 ({})",
            high.len(),
            low.len()
        );
        assert_eq!(decompress(&high, CompressionType::Lz4).unwrap(), data);
    }

    /// Decompressing a payload that is not a valid frame must error clearly
    /// rather than producing garbage.
    #[cfg(feature = "compression-zstd")]
    #[test]
    fn test_decompress_rejects_unframed_payload() {
        let not_a_frame = b"this is plainly not a compression frame";
        let err = decompress(not_a_frame, CompressionType::Zstd);
        assert!(err.is_err(), "unframed payload must be rejected");
    }

    /// A frame carries its own algorithm; `compress` output begins with the magic.
    #[cfg(feature = "compression-zstd")]
    #[test]
    fn test_frame_has_magic_header() {
        let framed = compress(b"some data to compress here", CompressionType::Zstd, 5).unwrap();
        assert_eq!(&framed[0..3], FRAME_MAGIC);
        assert_eq!(framed[3], ALGO_ZSTD);
        assert_eq!(framed[4], FRAME_VERSION);
    }
}
