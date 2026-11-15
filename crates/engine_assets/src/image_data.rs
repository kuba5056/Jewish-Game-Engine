use std::path::Path;

use crate::AssetError;

/// A decoded image: `width * height` RGBA pixels, 8 bits per channel, sRGB,
/// rows top to bottom.
#[derive(Debug, Clone)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decodes an image file held in memory (PNG or JPEG).
pub fn decode_image(file_bytes: &[u8]) -> Result<ImageData, String> {
    let image = image::load_from_memory(file_bytes)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    Ok(ImageData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

/// Reads and decodes an image file (PNG or JPEG).
pub fn load_image(path: &Path) -> Result<ImageData, AssetError> {
    let bytes = std::fs::read(path).map_err(|e| AssetError::io(path, e))?;
    decode_image(&bytes).map_err(|e| AssetError::invalid(path, e))
}
