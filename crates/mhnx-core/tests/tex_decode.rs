//! Decoding the Switch build's icon atlas.
//!
//! The expectations below were measured against the shipped texture: the file
//! is 2,097,176 bytes, its header says the payload is 2,097,152, and those are
//! exactly 512 * 1024 * 4 pixels. The sample pixels were read out of a decoder
//! that was checked by eye against the atlas (the weapon and armour icons are
//! visible in it), so they pin the byte order and the unswizzle together.

use mhnx_core::tex::{self, TexError};

/// The atlas the equipment, item and map menus all share.
const ATLAS: &str = "C:\\anyWorkspace\\nx\\mhxx\\extract_resident\\GUI\\99_texture\\HD_cmn_icon_GSM_NOMIP.tex";

fn atlas() -> Option<Vec<u8>> {
    std::fs::read(ATLAS).ok()
}

#[test]
fn the_header_of_the_icon_atlas_is_the_measured_one() {
    let Some(bytes) = atlas() else {
        eprintln!("skipping: {} not present", ATLAS);
        return;
    };
    let h = tex::read_header(&bytes).expect("header");
    assert_eq!(h.payload_len, 2_097_152);
    assert_eq!(bytes.len(), tex::TEX_HEADER_SIZE + h.payload_len);
    assert!(h.is_rgba8(), "the atlas is uncompressed RGBA8");
}

#[test]
fn the_icon_atlas_decodes_to_five_hundred_twelve_by_ten_twenty_four() {
    let Some(bytes) = atlas() else {
        eprintln!("skipping: {} not present", ATLAS);
        return;
    };
    let img = tex::decode_rgba8_auto(&bytes).expect("decode");
    assert_eq!(img.width, 512);
    assert_eq!(img.height, 1024);
    assert_eq!(img.pixels.len(), 512 * 1024 * 4);
}

#[test]
fn the_pixels_land_where_the_verified_decoder_put_them() {
    let Some(bytes) = atlas() else {
        eprintln!("skipping: {} not present", ATLAS);
        return;
    };
    let img = tex::decode_rgba8_auto(&bytes).expect("decode");
    // Sampled from the reference decode of the same file.
    assert_eq!(img.pixel(0, 0), Some([0, 0, 0, 0]));
    assert_eq!(img.pixel(10, 10), Some([0, 0, 0, 0]));
    assert_eq!(img.pixel(100, 600), Some([0, 0, 0, 0]));
    assert_eq!(img.pixel(300, 700), Some([0, 0, 0, 255]));
    assert_eq!(img.pixel(511, 1023), Some([255, 255, 255, 0]));
}

#[test]
fn the_block_height_is_sixteen_for_the_atlas_and_follows_the_height_for_small_ones() {
    assert_eq!(tex::block_gobs_for(1024), 16);
    assert_eq!(tex::block_gobs_for(256), 16);
    assert_eq!(tex::block_gobs_for(128), 16);
    assert_eq!(tex::block_gobs_for(64), 8);
    assert_eq!(tex::block_gobs_for(32), 4);
    assert_eq!(tex::block_gobs_for(8), 1);
}

#[test]
fn a_crop_is_the_rectangle_that_was_asked_for() {
    let Some(bytes) = atlas() else {
        eprintln!("skipping: {} not present", ATLAS);
        return;
    };
    let img = tex::decode_rgba8_auto(&bytes).expect("decode");
    let cell = img.crop(48, 512, 48, 48);
    assert_eq!(cell.width, 48);
    assert_eq!(cell.height, 48);
    assert_eq!(cell.pixels.len(), 48 * 48 * 4);
    assert_eq!(cell.pixel(0, 0), img.pixel(48, 512));
    assert_eq!(cell.pixel(47, 47), img.pixel(95, 559));
}

#[test]
fn a_short_file_is_rejected_rather_than_read_past_its_end() {
    assert_eq!(tex::read_header(&[0u8; 8]).unwrap_err(), TexError::TooShort);
    let mut bad = vec![0u8; 32];
    bad[0..4].copy_from_slice(b"TEX\0");
    assert_eq!(tex::read_header(&bad).map(|h| h.payload_len), Ok(0));
    assert_eq!(tex::decode_rgba8(&bad, 8, 8).unwrap_err(), TexError::BadSize);
}
