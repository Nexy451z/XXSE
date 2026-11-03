//! Quest progress: cleared, seen and failed bit maps.
//!
//! Every character slot carries three bit maps, all **relative to the slot
//! base** and each 189 bytes (1512 bits, of which 1509 are meaningful):
//!
//! ```text
//!     +0x2C77   cleared   755 bits set in the verified save
//!     +0x2D77   seen     1220
//!     +0x2E77   failed     72
//! ```
//!
//! The three maps sit 0x100 apart and the values are the same in all three
//! character slots, which is how they were found: a structure scan over the
//! slot produced exactly one window whose `+0x100` neighbour is a superset of
//! it (cleared is a subset of seen) and whose `+0x200` neighbour is a sparse
//! third map.
//!
//! ## Bit order and index range
//!
//! **LSB first**: bit `i` is byte `i >> 3`, mask `1 << (i & 7)`. Index 0 is
//! unused (it lines up with the dummy `q0000000.arc` and is clear in all three
//! maps), so only **1..=1508** are accepted. Bits 1509..1511 exist inside the
//! 189th byte but are clear in the verified save and out of range here.
//!
//! ## What an index means
//!
//! `index = band start + rank of the quest id inside the band` (quest ids in
//! ascending order). The index-to-quest-id table is **not** in this crate: it
//! would be game data taken from the user's own files, so callers work in
//! indices and resolve names through [`crate::romfstables`] at run time.
//!
//! The one band whose membership is confirmed is the special permit band:
//! 228 entries starting at index 947, laid out as the 18 deviants in id order
//! (twelve with 16 quests each, six with 6 each). The other bands are described
//! in `QUEST_BITS.md`; nothing here depends on their sizes.
//!
//! ## Invariants this module maintains
//!
//! * Setting a cleared bit also sets the matching seen bit.
//! * Clearing a seen bit while its cleared bit is still set is refused, because
//!   that would leave a quest cleared but never seen.
//! * `failed` is read only. 58 of the 72 failed bits in the verified save are
//!   also cleared, so its exact meaning is not established: the failed bit
//!   survives a clear rather than meaning "failed and not cleared".
//!
//! The real save already violates the cleared-in-seen invariant in three
//! places (indices 821, 1309 and 1392). Those are reported by
//! [`clears_without_seen`] and deliberately left alone: guessing which side is
//! wrong would corrupt data the game wrote.

use alloc::vec::Vec;

use crate::save::{Save, SaveKind};

/// Number of quest progress bits, including the unused index 0.
///
/// 1509 matches the total the reference editor publishes for MHGU, and the
/// highest bit ever set in the verified save is 1508.
pub const BITS: usize = 1509;

/// Length of one bit map in bytes (1512 bits; the last three are unused).
pub const MAP_SIZE: usize = 189;

/// Cleared map offset, relative to a character slot base.
pub const OFF_CLEARED: usize = 0x2C77;
/// Seen map offset, relative to a character slot base.
pub const OFF_SEEN: usize = 0x2D77;
/// Failed map offset, relative to a character slot base.
pub const OFF_FAILED: usize = 0x2E77;

/// Which of the three maps a call refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// The quest has been cleared.
    Cleared,
    /// The quest has been seen (listed / taken at least once).
    Seen,
    /// The quest has been failed. Read only.
    Failed,
}

impl Progress {
    /// Byte offset of this map inside a character slot.
    pub fn offset(self) -> usize {
        match self {
            Progress::Cleared => OFF_CLEARED,
            Progress::Seen => OFF_SEEN,
            Progress::Failed => OFF_FAILED,
        }
    }

    /// Can this map be written at all?
    pub fn is_writable(self) -> bool {
        !matches!(self, Progress::Failed)
    }
}

/// The maps only have verified offsets for MHXX, so an MHGU save is refused
/// rather than read at the wrong place.
fn supported(save: &Save) -> bool {
    save.kind() != SaveKind::Mhgu
}

/// Is this an index the game actually uses? 0 and 1509 and up are not.
fn valid(index: usize) -> bool {
    index > 0 && index < BITS
}

/// Body offset where one map starts, if the save, slot and map are all usable.
fn map_start(save: &Save, slot: usize, kind: Progress) -> Option<usize> {
    if !supported(save) {
        return None;
    }
    let base = save.slot_base(slot)? as usize;
    let start = base.checked_add(kind.offset())?;
    if start.checked_add(MAP_SIZE)? > save.body().len() {
        return None;
    }
    Some(start)
}

/// Read one bit. `None` for an out of range index or an unsupported save.
pub fn get(save: &Save, slot: usize, kind: Progress, index: usize) -> Option<bool> {
    if !valid(index) {
        return None;
    }
    let start = map_start(save, slot, kind)?;
    let byte = *save.body().get(start + (index >> 3))?;
    Some((byte >> (index & 7)) & 1 == 1)
}

/// Write one bit and report whether the write was accepted.
///
/// Rules, in the order they are applied:
///
/// * `Failed` is read only, so every call for it returns `false`.
/// * Index 0 and 1509 and up are refused.
/// * Setting `Cleared` also sets `Seen` (the cleared-in-seen invariant).
/// * Clearing `Seen` is refused while `Cleared` is still set.
///
/// Everything is checked before the first byte changes, so a refused call
/// leaves the save exactly as it was.
pub fn set(save: &mut Save, slot: usize, kind: Progress, index: usize, on: bool) -> bool {
    if !kind.is_writable() || !valid(index) {
        return false;
    }
    let Some(start) = map_start(save, slot, kind) else {
        return false;
    };
    match (kind, on) {
        (Progress::Seen, false) => {
            // A quest that is still cleared must stay seen.
            if get(save, slot, Progress::Cleared, index) != Some(false) {
                return false;
            }
            write_bit(save, start, index, false)
        }
        (Progress::Cleared, true) => {
            let Some(seen) = map_start(save, slot, Progress::Seen) else {
                return false;
            };
            // Seen first: if that write failed, cleared must not be left on its
            // own. Seen without cleared is a normal state, so the reverse
            // order has no safe failure case.
            if !write_bit(save, seen, index, true) {
                return false;
            }
            write_bit(save, start, index, true)
        }
        _ => write_bit(save, start, index, on),
    }
}

fn write_bit(save: &mut Save, start: usize, index: usize, on: bool) -> bool {
    let at = start + (index >> 3);
    let Some(byte) = save.body_mut().get_mut(at) else {
        return false;
    };
    let mask = 1u8 << (index & 7);
    if on {
        *byte |= mask;
    } else {
        *byte &= !mask;
    }
    true
}

/// Number of set bits in one map, counting only the valid indices.
pub fn count(save: &Save, slot: usize, kind: Progress) -> usize {
    let Some(start) = map_start(save, slot, kind) else {
        return 0;
    };
    let bytes = &save.body()[start..start + MAP_SIZE];
    (1..BITS)
        .filter(|i| (bytes[i >> 3] >> (i & 7)) & 1 == 1)
        .count()
}

/// Indices that are cleared but not seen: the invariant violation, in order.
///
/// The verified save has three (821, 1309, 1392). They are a fact about the
/// data, not a bug in this module, so nothing is repaired automatically.
pub fn clears_without_seen(save: &Save, slot: usize) -> Vec<usize> {
    let (Some(cleared), Some(seen)) = (
        map_start(save, slot, Progress::Cleared),
        map_start(save, slot, Progress::Seen),
    ) else {
        return Vec::new();
    };
    let body = save.body();
    let mut out = Vec::new();
    for index in 1..BITS {
        let c = (body[cleared + (index >> 3)] >> (index & 7)) & 1 == 1;
        let s = (body[seen + (index >> 3)] >> (index & 7)) & 1 == 1;
        if c && !s {
            out.push(index);
        }
    }
    out
}
