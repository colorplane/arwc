use wasm_bindgen::prelude::*;

fn map_err(e: crate::Error) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Compress an uncompressed Sony ARW. Result is a `.ARWC.JPG` JPEG
/// (stripable view EXIF + camera JPEG) with an ARWZ trailer after EOI.
#[wasm_bindgen]
pub fn encode(arw: &[u8], level: i32) -> Result<Vec<u8>, JsValue> {
    crate::encode_with_level(arw, level).map_err(map_err)
}

/// Reconstruct the original ARW from a JPEG+ARWZ container.
#[wasm_bindgen]
pub fn decode(file: &[u8]) -> Result<Vec<u8>, JsValue> {
    crate::decode(file).map_err(map_err)
}

/// Camera JPEG only. Stops at `FF D9` — does not decode the raw trailer.
#[wasm_bindgen]
pub fn extract_preview(file: &[u8]) -> Result<Vec<u8>, JsValue> {
    crate::extract_preview(file)
        .map(|s| s.to_vec())
        .map_err(map_err)
}

/// How many leading bytes are the viewable JPEG (for Range requests).
#[wasm_bindgen]
pub fn preview_bytes(file: &[u8]) -> Result<u32, JsValue> {
    crate::inspect(file)
        .map(|i| i.preview_bytes)
        .map_err(map_err)
}
