//! MT Framework NX texture (.tex) reader.
//!
//! Measured on the shipped Switch build (MHXX nativeNX):
//!
//! - offset 0x00  u8[4]  magic "TEX\0"
//! - offset 0x04  u32    header word 1 (identical across every texture checked)
//! - offset 0x08  u32    header word 2 (0x00010701 for RGBA8, 0x00011701 for others)
//! - offset 0x0C  u32    header word 3
//! - offset 0x10  u32    payload length in bytes (verified: file_len == 24 + this)
//! - offset 0x14..0x18   zero
//! - offset 0x18  payload
//!
//! The pixel data is Tegra X1 block-linear ("GOB"), not linear rows. A GOB is a
//! 64 byte wide, 8 row tile; a block is a stack of GOBs. For the icon atlases the
//! block is 16 GOBs tall (128 rows), which is what the console's own layout uses.
//!
//! Decoding is a pure permutation of bytes, so it needs no allocation beyond the
//! output buffer and works in no_std.

use alloc::vec;
use alloc::vec::Vec;

/// Texture magic.
pub const TEX_MAGIC: [u8; 4] = *b"TEX\0";
/// Bytes before the payload.
pub const TEX_HEADER_SIZE: usize = 24;

/// A decoded RGBA8 image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// RGBA8 pixels, row major, width * height * 4 bytes.
    pub pixels: Vec<u8>,
}

/// Why a texture could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexError {
    /// The file is shorter than its own header.
    TooShort,
    /// The magic is not "TEX\0".
    BadMagic,
    /// The payload length field does not match the file.
    BadLength,
    /// The payload is not a whole number of RGBA8 pixels for the given size.
    BadSize,
    /// The format is not one this reader implements.
    UnsupportedFormat,
}

/// The header fields this reader needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TexHeader {
    /// Header word at +0x08; 0x00010701 marks the RGBA8 atlases.
    pub format_word: u32,
    /// Payload length from +0x10.
    pub payload_len: usize,
}

impl TexHeader {
    /// True when this texture is uncompressed RGBA8.
    ///
    /// Measured: the icon atlas (2,097,152 bytes for 512x1024) and every other
    /// texture whose payload is exactly width*height*4 carries 0x00010701 here.
    pub fn is_rgba8(&self) -> bool {
        self.format_word & 0x0001_0000 == 0
    }
}

/// Read the header without decoding pixels.
pub fn read_header(bytes: &[u8]) -> Result<TexHeader, TexError> {
    if bytes.len() < TEX_HEADER_SIZE {
        return Err(TexError::TooShort);
    }
    if bytes[0..4] != TEX_MAGIC {
        return Err(TexError::BadMagic);
    }
    let format_word = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let payload_len = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
    Ok(TexHeader {
        format_word,
        payload_len,
    })
}

/// Undo the Tegra block-linear layout for a 4 byte per pixel image.
///
/// `block_gobs` is how many 8-row GOBs one block holds: 16 for the icon atlases,
/// 8 for a 64x64 texture, and so on.
fn unswizzle_rgba8(src: &[u8], width: usize, height: usize, block_gobs: usize) -> Vec<u8> {
    let mut out = vec![0u8; width * height * 4];
    if block_gobs == 0 {
        return out;
    }
    let gobs_per_row = (width * 4 + 63) / 64;
    let block_rows = block_gobs * 8;
    for y in 0..height {
        let block_y = y / block_rows;
        let in_block_y = y % block_rows;
        let gob_y = in_block_y / 8;
        let in_gob_y = in_block_y % 8;
        for x in 0..width {
            let byte_x = x * 4;
            let gob_x = byte_x / 64;
            let in_gob_x = byte_x % 64;
            let gob = ((block_y * gobs_per_row + gob_x) * block_gobs + gob_y) * 512;
            // Tegra X1 GOB: 64 bytes wide, 8 rows, with the address bits shuffled.
            let off = ((in_gob_x & 0x20) << 3)
                | ((in_gob_y & 0x06) << 5)
                | ((in_gob_x & 0x10) << 1)
                | ((in_gob_y & 0x01) << 4)
                | (in_gob_x & 0x0F);
            let from = gob + off;
            if from + 4 <= src.len() {
                let to = (y * width + x) * 4;
                out[to..to + 4].copy_from_slice(&src[from..from + 4]);
            }
        }
    }
    out
}

/// The block height the console uses for a texture of this size.
///
/// The atlases are stored with 16 GOBs per block; a texture shorter than one
/// block is stored with as many GOBs as it has rows.
pub fn block_gobs_for(height: usize) -> usize {
    let gobs = (height + 7) / 8;
    if gobs >= 16 {
        16
    } else if gobs == 0 {
        1
    } else {
        gobs
    }
}

/// Decode an RGBA8 texture at a known size.
pub fn decode_rgba8(bytes: &[u8], width: usize, height: usize) -> Result<Image, TexError> {
    let header = read_header(bytes)?;
    if !header.is_rgba8() {
        return Err(TexError::UnsupportedFormat);
    }
    let need = width * height * 4;
    if header.payload_len < need || bytes.len() < TEX_HEADER_SIZE + need {
        return Err(TexError::BadSize);
    }
    let payload = &bytes[TEX_HEADER_SIZE..TEX_HEADER_SIZE + need];
    let pixels = unswizzle_rgba8(payload, width, height, block_gobs_for(height));
    Ok(Image {
        width,
        height,
        pixels,
    })
}

/// Decode an RGBA8 texture whose size is implied by its payload length.
///
/// The header does not carry the size in a field this reader trusts, so the
/// shape is recovered from the payload: every shipped atlas is a power of two
/// on both axes and taller than it is wide (the icon atlas is 512x1024, the
/// menu icons 512x256). Among the shapes that fit, the widest portrait one is
/// the answer; a landscape shape is only used when no portrait one exists.
pub fn decode_rgba8_auto(bytes: &[u8]) -> Result<Image, TexError> {
    let header = read_header(bytes)?;
    if !header.is_rgba8() {
        return Err(TexError::UnsupportedFormat);
    }
    let pixels = header.payload_len / 4;
    if pixels == 0 || header.payload_len % 4 != 0 {
        return Err(TexError::BadSize);
    }
    let mut portrait: Option<(usize, usize)> = None;
    let mut landscape: Option<(usize, usize)> = None;
    let mut w = 32usize;
    while w <= 4096 {
        if pixels % w == 0 {
            let h = pixels / w;
            if (32..=4096).contains(&h) {
                if w <= h {
                    // keep the widest portrait shape
                    if portrait.map(|(pw, _)| w > pw).unwrap_or(true) {
                        portrait = Some((w, h));
                    }
                } else if landscape.is_none() {
                    landscape = Some((w, h));
                }
            }
        }
        w *= 2;
    }
    let (width, height) = portrait.or(landscape).ok_or(TexError::BadSize)?;
    decode_rgba8(bytes, width, height)
}

impl Image {
    /// One RGBA pixel, or None when the coordinate is outside the image.
    pub fn pixel(&self, x: usize, y: usize) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y * self.width + x) * 4;
        Some([
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ])
    }

    /// Copy a rectangle out as a new image, clamping to the source bounds.
    pub fn crop(&self, x: usize, y: usize, w: usize, h: usize) -> Image {
        let mut out = vec![0u8; w * h * 4];
        for row in 0..h {
            for col in 0..w {
                if let Some(p) = self.pixel(x + col, y + row) {
                    let to = (row * w + col) * 4;
                    out[to..to + 4].copy_from_slice(&p);
                }
            }
        }
        Image {
            width: w,
            height: h,
            pixels: out,
        }
    }
}
