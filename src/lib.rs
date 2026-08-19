mod codec;
mod error;
mod jpeg;
mod tiff;
mod transform;

#[cfg(feature = "wasm")]
mod wasm;

pub use codec::{
    decode, encode, encode_with_level, encode_with_progress, encoded_output_path,
    decoded_output_path, extract_preview, has_encoded_extension, inspect, is_encoded, FileInfo,
    Kind, DEFAULT_LEVEL, ENCODED_EXTENSION, MAGIC, VERSION,
};
pub use error::{Error, Result};
pub use jpeg::jpeg_end;
pub use tiff::{parse_layout, ArwLayout, JpegRef};
