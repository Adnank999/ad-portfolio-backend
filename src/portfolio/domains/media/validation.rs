use super::policy::Purpose;
use crate::common::error::{AppError, AppResult};
use image::{codecs::webp::WebPDecoder, ImageDecoder};
use std::io::Cursor;

pub fn verify_webp(bytes: &[u8], purpose: Purpose, width: u32, height: u32) -> AppResult<()> {
    let invalid = || AppError::Invalid("Invalid, animated, or mismatched WebP image".into());
    if bytes.len() < 20
        || &bytes[..4] != b"RIFF"
        || &bytes[8..12] != b"WEBP"
        || u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| invalid())?) as usize + 8
            != bytes.len()
    {
        return Err(invalid());
    }
    purpose.validate(bytes.len() as i64, width, height)?;
    let decoder = WebPDecoder::new(Cursor::new(bytes)).map_err(|_| invalid())?;
    if decoder.has_animation()
        || decoder.dimensions() != (width, height)
        || decoder.total_bytes() > 16 * 1024 * 1024
    {
        return Err(invalid());
    }
    let mut pixels = vec![0; decoder.total_bytes() as usize];
    decoder.read_image(&mut pixels).map_err(|_| invalid())?;
    Ok(())
}
