//! Where an equipment or item icon sits on the game's icon sheet, and what
//! colour the game tints it with.
//!
//! Everything here was measured, not guessed:
//!
//! * The sheet is `GUI/99_texture/HD_cmn_icon_GSM_NOMIP.tex` inside
//!   `nativeNX/arc/resident.arc`: 512 x 1024 RGBA8, Tegra block-linear. It is
//!   **grayscale**; the game tints it at draw time, and so do we.
//! * The 48 px grid starts at y = 416 and runs 10 cells wide. Cell `i` (i < 95)
//!   is at `(i % 10) * 48, 416 + (i / 10) * 48`. The last few cells are packed
//!   separately because they sit outside that grid.
//! * Which cell an equipment box entry uses is a table in the game's own code
//!   (`EQUIP_BOX_ICON`, a `u8[25]` indexed by the entry's type byte).
//! * The rarity palette and the item palette are the game's own, stored as
//!   0xAABBGGRR - the same word order this crate's canvas uses.
//!
//! The equipment type byte is the one the save stores: 1 head, 2 chest, 3 arms,
//! 4 waist, 5 legs, 6 talisman, 7..=21 weapon classes. Type 18 is dual blades,
//! which is the anchor the whole table was checked against.

/// One cell of the icon sheet, in pixels from the top left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    /// Left edge.
    pub x: usize,
    /// Top edge.
    pub y: usize,
    /// Cell width and height (square).
    pub size: usize,
}

/// The icon grid's cell size.
pub const CELL: usize = 48;
/// Where the 48 px grid starts on the sheet.
pub const GRID_TOP: usize = 416;
/// How many cells fit across the sheet.
pub const GRID_COLUMNS: usize = 10;

/// Equipment box type byte -> sheet cell, straight from the game's table.
///
/// Index 0 is "empty" (a blank cell), 1..=5 the armour pieces, 6 the talisman,
/// and 7..=21 the weapon classes in the game's own order.
pub const EQUIP_BOX_ICON: [u8; 25] = [
    46, 70, 71, 73, 72, 74, 75, 55, 57, 59, 61, 65, 63, 63, 56, 67, 62, 66, 58, 60, 68, 69, 76,
    78, 79,
];

/// The cells past the 48 px grid, for sheet indices 95 and up.
const EXTRA: [(u16, u16); 12] = [
    (240, 848),
    (288, 848),
    (336, 848),
    (384, 848),
    (0, 896),
    (48, 896),
    (288, 608),
    (288, 608),
    (288, 608),
    (288, 608),
    (432, 848),
    (96, 896),
];

/// Rarity colours, indexed by rarity - 1. 0xAABBGGRR as the game stores them.
pub const RARITY_COLORS: [u32; 11] = [
    0xFFE6_E6E6,
    0xFFF2_92B0,
    0xFF5E_D5DE,
    0xFF9E_8EE8,
    0xFF73_C76F,
    0xFFF8_8E70,
    0xFF5A_56DA,
    0xFFAC_9E4C,
    0xFF5D_8CEE,
    0xFFBD_45FF,
    0xFFFF_007D,
];

/// Item colour palette, indexed by the item's colour byte (taken mod 17).
pub const ITEM_COLORS: [u32; 17] = [
    0xFFF5_F5F5,
    0xFF5C_50E1,
    0xFF83_BF69,
    0xFFFF_BB9F,
    0xFF65_CFF7,
    0xFFD1_A3C3,
    0xFFF0_DF9B,
    0xFF65_9AF7,
    0xFF9D_88F8,
    0xFF6D_D5AD,
    0xFFAA_AAAA,
    0xFF2E_97C1,
    0xFFC7_EB57,
    0xFF3B_8955,
    0xFF00_0099,
    0xFF99_3300,
    0xFF66_0066,
];

/// The sheet cell for a sheet index.
///
/// Indices below 95 are on the plain 10 wide grid; the handful above it are at
/// the coordinates the game lists for them, and anything beyond that falls back
/// to the cell the game's own code uses as its default.
pub fn cell_for(index: u8) -> Rect {
    if (index as usize) < 95 {
        Rect {
            x: (index as usize % GRID_COLUMNS) * CELL,
            y: GRID_TOP + (index as usize / GRID_COLUMNS) * CELL,
            size: CELL,
        }
    } else {
        let at = index as usize - 95;
        let (x, y) = if at < EXTRA.len() {
            EXTRA[at]
        } else {
            (288, 608)
        };
        Rect {
            x: x as usize,
            y: y as usize,
            size: CELL,
        }
    }
}

/// The sheet cell an equipment box entry of this type shows.
///
/// An unknown type gets the empty cell, which is what the game shows for a slot
/// it cannot identify.
pub fn cell_for_equipment(kind: u8) -> Rect {
    let index = *EQUIP_BOX_ICON.get(kind as usize).unwrap_or(&46);
    cell_for(index)
}

/// The tint the game gives an equipment icon: the rarity colour.
///
/// Rarity 0 is not a rarity the game shows, so it reads as the pale default.
pub fn rarity_tint(rarity: u8) -> u32 {
    if rarity == 0 {
        RARITY_COLORS[0]
    } else {
        RARITY_COLORS[(rarity as usize - 1).min(RARITY_COLORS.len() - 1)]
    }
}

/// The tint the game gives an item icon.
pub fn item_tint(color: u8) -> u32 {
    ITEM_COLORS[color as usize % ITEM_COLORS.len()]
}

/// Split a 0xAABBGGRR word into RGBA bytes, which is what the canvas wants.
pub fn to_rgba(color: u32) -> [u8; 4] {
    [
        (color & 0xFF) as u8,
        ((color >> 8) & 0xFF) as u8,
        ((color >> 16) & 0xFF) as u8,
        ((color >> 24) & 0xFF) as u8,
    ]
}

/// Multiply a grayscale pixel by a tint, keeping the alpha.
///
/// The sheet is luminance plus alpha; the game's tint replaces the RGB with the
/// colour scaled by the pixel's brightness, which is why a white icon takes the
/// tint whole and a dark outline stays dark.
pub fn tint_pixel(gray: [u8; 4], tint: u32) -> [u8; 4] {
    let [r, g, b, a] = to_rgba(tint);
    let lum = gray[0] as u32;
    [
        ((r as u32 * lum) / 255) as u8,
        ((g as u32 * lum) / 255) as u8,
        ((b as u32 * lum) / 255) as u8,
        ((a as u32 * gray[3] as u32) / 255) as u8,
    ]
}
