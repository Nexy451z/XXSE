//! Guild card awards: the 132 bits the game keeps in three places.
//!
//! The guild card lists 132 awards (勲章). The same 132 bit map is stored three
//! times inside a character slot, always relative to the slot base:
//!
//! ```text
//!     slot + 0xC8115   17 bytes   the card's own copy
//!     slot + 0x3157    20 bytes   the map screen's copy
//!     slot + 0x316B    20 bytes   the notification copy
//! ```
//!
//! In all three slots of the sample save the copies are byte for byte
//! identical, and the 17 byte card equals the first 17 bytes of the other two.
//! Bits are LSB first: award `i` is bit `i & 7` of byte `i >> 3`, so
//! 132 awards fill 17 bytes and the top four bits of the last one are unused.
//!
//! Writing one copy is not enough. The game merges the copies (an OR, as the
//! clearing experiments show: a bit removed from the card alone comes back),
//! so every write here goes to all three. If they are already inconsistent
//! [`copies_agree`] says so, and that check is meant to run before a write so
//! the caller can warn instead of silently picking a side.
//!
//! A slot the save marks as unused holds no character, so every accessor treats
//! it exactly like an out of range slot. These are the MHXX offsets; the MHGU
//! layout differs and is not verified here.

use alloc::vec::Vec;

use crate::save::Save;

/// Number of awards on the guild card.
pub const AWARDS: usize = 132;

/// Offset of the card's own copy, relative to the slot base.
pub const CARD_OFF: usize = 0xC8115;
/// Bytes the card's copy occupies.
pub const CARD_LEN: usize = 17;
/// Offset of the map screen's copy, relative to the slot base.
pub const MAP_OFF: usize = 0x3157;
/// Bytes the map screen's copy occupies.
pub const MAP_LEN: usize = 20;
/// Offset of the notification copy, relative to the slot base.
pub const NOTICE_OFF: usize = 0x316B;
/// Bytes the notification copy occupies.
pub const NOTICE_LEN: usize = 20;

/// Bytes the 132 bits need: 17.
const SPAN: usize = AWARDS.div_ceil(8);

/// The three copies, as (offset, bytes), in the order [`set`] writes them.
///
/// The card copy is the smallest of the three, so it is also the one that
/// bounds the map: `SPAN` fits in every entry.
pub const COPIES: [(usize, usize); 3] = [
    (CARD_OFF, CARD_LEN),
    (MAP_OFF, MAP_LEN),
    (NOTICE_OFF, NOTICE_LEN),
];

/// Base of a slot that can hold awards, or None for no such slot.
///
/// The `used` flag is part of the test on purpose: a slot the save marks as
/// free has no character, and reading its bytes would describe nobody.
fn base_of(save: &Save, slot: usize) -> Option<usize> {
    if !save.slot_used(slot) {
        return None;
    }
    Some(save.slot_base(slot)? as usize)
}

/// Read one of the three copies, whole.
fn copy(save: &Save, slot: usize, off: usize, len: usize) -> Option<&[u8]> {
    let base = base_of(save, slot)?;
    let start = base.checked_add(off)?;
    save.body().get(start..start.checked_add(len)?)
}

/// One bit, LSB first.
fn bit(bytes: &[u8], award: usize) -> bool {
    bytes[award >> 3] & (1 << (award & 7)) != 0
}

/// Set or clear one bit, LSB first. The byte must already be in range.
fn put_bit(bytes: &mut [u8], award: usize, on: bool) {
    let mask = 1u8 << (award & 7);
    if on {
        bytes[award >> 3] |= mask;
    } else {
        bytes[award >> 3] &= !mask;
    }
}

/// The 132 bits of the card's copy, or None when the slot holds no character.
fn card_bits(save: &Save, slot: usize) -> Option<[u8; SPAN]> {
    let src = copy(save, slot, CARD_OFF, CARD_LEN)?;
    let mut out = [0u8; SPAN];
    out.copy_from_slice(&src[..SPAN]);
    Some(out)
}

/// Does this hunter hold the award?
///
/// The card's copy is the one read: it is the smallest and the one the card
/// screen shows. When [`copies_agree`] says the three disagree, this answer
/// can lag behind the game (which merges them), so check that first.
///
/// Returns None for an award index outside `0..AWARDS` and for a slot that
/// holds no character.
pub fn has(save: &Save, slot: usize, award: usize) -> Option<bool> {
    if award >= AWARDS {
        return None;
    }
    Some(bit(&card_bits(save, slot)?, award))
}

/// Grant or revoke one award in **all three copies**.
///
/// One copy alone is undone by the game, so a partial write here would only
/// look correct until the next load. Returns false for an award index outside
/// `0..AWARDS` or a slot that holds no character; nothing is written then.
pub fn set(save: &mut Save, slot: usize, award: usize, on: bool) -> bool {
    if award >= AWARDS || base_of(save, slot).is_none() {
        return false;
    }
    let mut written = true;
    for (off, len) in COPIES {
        written &= with_copy_mut(save, slot, off, len, |b| put_bit(b, award, on));
    }
    // The slot base already proves all three fields fit, so this only fails on
    // a bug; returning it rather than ignoring it keeps the caller honest.
    written
}

/// Run `f` on one copy, mutably, and report whether it ran.
fn with_copy_mut(
    save: &mut Save,
    slot: usize,
    off: usize,
    len: usize,
    f: impl FnOnce(&mut [u8]),
) -> bool {
    let Some(base) = base_of(save, slot) else {
        return false;
    };
    let Some(start) = base.checked_add(off) else {
        return false;
    };
    let Some(end) = start.checked_add(len) else {
        return false;
    };
    match save.body_mut().get_mut(start..end) {
        Some(b) => {
            f(b);
            true
        }
        None => false,
    }
}

/// How many of the 132 awards this hunter holds, or 0 when the slot is empty.
pub fn count(save: &Save, slot: usize) -> usize {
    match card_bits(save, slot) {
        Some(b) => (0..AWARDS).filter(|&a| bit(&b, a)).count(),
        None => 0,
    }
}

/// The awards this hunter does not hold, in ascending order.
///
/// Empty when the slot holds no character: there is no set to be missing from.
pub fn missing(save: &Save, slot: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let Some(b) = card_bits(save, slot) else {
        return out;
    };
    for a in 0..AWARDS {
        if !bit(&b, a) {
            out.push(a);
        }
    }
    out
}

/// Do the three copies hold the same 132 bits?
///
/// Run this before writing: false means the save is already in the state the
/// game repairs by OR, and the card copy [`has`] reads may be the stale one.
/// Only the 132 award bits are compared; the four spare bits of the 17th byte
/// and the three trailing bytes of the 20 byte copies are not awards.
///
/// False when the slot holds no character, because there is nothing to agree.
pub fn copies_agree(save: &Save, slot: usize) -> bool {
    let Some(card) = card_bits(save, slot) else {
        return false;
    };
    for (off, len) in COPIES {
        let Some(other) = copy(save, slot, off, len) else {
            return false;
        };
        for a in 0..AWARDS {
            if bit(other, a) != bit(&card, a) {
                return false;
            }
        }
    }
    true
}
