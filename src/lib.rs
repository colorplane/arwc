mod codec;
mod error;
mod jpeg;
mod tiff;
mod transform;

#[cfg(feature = "wasm")]
mod wasm;

pub use codec::{
    decode, decoded_output_path, encode, encode_with_level, encode_with_progress,
    encoded_output_path, extract_preview, has_encoded_extension, inspect, is_encoded, FileInfo,
    Kind, DEFAULT_LEVEL, ENCODED_EXTENSION, FOOTER_LEN, FOOTER_MAGIC, MAGIC, VERSION,
};
pub use error::{Error, Result};
pub use jpeg::{
    attach_view_exif, jpeg_end, jpeg_exif_orientation, jpeg_exif_thumbnail, strip_view_exif,
};
pub use tiff::{parse_layout, ArwLayout, JpegRef};
