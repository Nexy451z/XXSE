//! Village requests: one bit per request, accepted and completed.
//!
//! The map sits at slot + 0x2C56D and takes 192 bytes, i.e. 1536 bits, read
//! least significant bit first like the quest bit maps. Verified against the
//! stable dump (`C:\anyWorkspace\nx\jksv_ready\system`): all three character
//! slots hold the same 192 bytes, 742 of the 1536 bits are set, and the highest
//! one is bit 1507. (Slots 2 and 3 of that dump are byte copies of slot 1, so
//! the three agreeing is weak evidence; the shape is not - no bit above 1507 is
//! set, and 192 bytes is a whole number of bytes, so the map has no unused
//! tail.)
//!
//! The map is the character's **event flag** space, and the request table
//! indexes into it; it is not a map of requests alone. 444 of those 742 bits
//! are numbers no request record names - other events share the same space.
//! Reading it as "742 requests" would be wrong, and so would writing every bit
//! above the request range.
//!
//! Which number means "accepted" and which means "completed" is **not** in the
//! save. The game's request table (`activityData.atd` in the user's romfs)
//! carries the pair inside each record, and that lookup belongs to
//! `romfstables`. This module deliberately knows only that a flag is a flag, so
//! the table and the map can be checked against each other without either side
//! owning the other.
//!
//! The offsets are MHXX only. MHGU stores a character slot at a different
//! stride, so an offset taken from the wrong layout would read plausible
//! nonsense; reads return None and writes return false for an MHGU save, the
//! same rule `dlc` follows for its own offsets.
//!
//! Every write here changes exactly one bit and touches no other byte, so a
//! cleared flag can be set back and the map returns to the byte it had.

use alloc::vec::Vec;

use crate::save::{Save, SaveKind};

/// Offset of the request flag map, relative to a slot base.
pub const OFF_FLAGS: usize = 0x2C56D;
/// Bytes the map occupies.
pub const FLAG_BYTES: usize = 192;
/// Bits in the map, one per event flag number.
///
/// Kept as the product rather than the literal, so a wrong byte count cannot
/// leave the two disagreeing.
pub const FLAGS: usize = FLAG_BYTES * 8;

/// Do these offsets describe this save?
///
/// The position was read off MHXX saves only. An MHGU character slot is 0x1104
/// bytes longer, so the same slot-relative offset lands somewhere else in it;
/// refusing is honest, while returning whatever happens to be there would look
/// like data.
fn supported(save: &Save) -> bool {
    save.kind() != SaveKind::Mhgu
}

/// Body range of a slot's flag map, or None when it does not fit.
///
/// The slot base comes out of the save file, so it is bounded here rather than
/// trusted: a corrupt base must produce None, not a slice past the end.
fn flag_range(save: &Save, slot: usize) -> Option<core::ops::Range<usize>> {
    if !supported(save) {
        return None;
    }
    let base = save.slot_base(slot)? as usize;
    let start = base.checked_add(OFF_FLAGS)?;
    let end = start.checked_add(FLAG_BYTES)?;
    if end > save.body().len() {
        return None;
    }
    Some(start..end)
}

/// The raw flag map of a slot.
fn flag_bytes(save: &Save, slot: usize) -> Option<&[u8]> {
    let range = flag_range(save, slot)?;
    save.body().get(range)
}

/// One request / event flag of a slot.
///
/// Returns None when the slot has no usable flag map or the flag is outside
/// 0..FLAGS: a flag number that is not in the map does not have a value, so
/// answering false would claim one more "off" bit than the save holds.
pub fn get(save: &Save, slot: usize, flag: usize) -> Option<bool> {
    if flag >= FLAGS {
        return None;
    }
    let bytes = flag_bytes(save, slot)?;
    Some((bytes[flag >> 3] >> (flag & 7)) & 1 == 1)
}

/// Set or clear one request / event flag.
///
/// Returns false - and writes nothing - when the flag or the slot is out of
/// range, when the save holds no flag map there, or when the save is MHGU.
pub fn set(save: &mut Save, slot: usize, flag: usize, on: bool) -> bool {
    if flag >= FLAGS {
        return false;
    }
    let Some(range) = flag_range(save, slot) else {
        return false;
    };
    let Some(bytes) = save.body_mut().get_mut(range) else {
        return false;
    };
    let byte = &mut bytes[flag >> 3];
    let mask = 1u8 << (flag & 7);
    if on {
        *byte |= mask;
    } else {
        *byte &= !mask;
    }
    true
}

/// How many flags are set in a slot.
///
/// Counts the whole map, so this is "bits set", not "requests reported": the
/// event flag space is shared with other unlocks (see the module notes). An
/// unusable slot counts as zero.
pub fn count(save: &Save, slot: usize) -> usize {
    match flag_bytes(save, slot) {
        Some(bytes) => bytes.iter().map(|b| b.count_ones() as usize).sum(),
        None => 0,
    }
}

/// A run of flags, for a list view.
///
/// The range is clipped to the map rather than rejected, so a caller drawing
/// the last page of a list does not have to know the length: asking for 20
/// flags from FLAGS - 3 answers three, and asking from FLAGS answers none. An
/// unusable slot answers no flags at all.
pub fn flags_near(save: &Save, slot: usize, start: usize, len: usize) -> Vec<bool> {
    let mut out = Vec::new();
    if start >= FLAGS || len == 0 {
        return out;
    }
    let Some(bytes) = flag_bytes(save, slot) else {
        return out;
    };
    let end = start.saturating_add(len).min(FLAGS);
    out.reserve(end - start);
    for flag in start..end {
        out.push((bytes[flag >> 3] >> (flag & 7)) & 1 == 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save;

    fn base_of(save: &Save) -> usize {
        save.slot_base(0).unwrap() as usize
    }

    #[test]
    fn the_map_is_a_whole_number_of_bytes() {
        assert_eq!(FLAG_BYTES, 192);
        assert_eq!(FLAGS, 1536);
        assert_eq!(FLAGS % 8, 0);
    }

    #[test]
    fn bits_run_least_significant_first() {
        let mut s = save::empty_switch_save();
        let base = base_of(&s);
        assert!(set(&mut s, 0, 0, true));
        assert!(set(&mut s, 0, 9, true));
        let map = &s.body()[base + OFF_FLAGS..base + OFF_FLAGS + FLAG_BYTES];
        assert_eq!(map[0], 0x01, "flag 0 is the low bit of the first byte");
        assert_eq!(map[1], 0x02, "flag 9 is bit 1 of the second byte");
        assert_eq!(count(&s, 0), 2);
        assert_eq!(flags_near(&s, 0, 0, 3), alloc::vec![true, false, false]);
        assert_eq!(flags_near(&s, 0, 8, 2), alloc::vec![false, true]);
    }

    #[test]
    fn setting_and_clearing_round_trips() {
        let mut s = save::empty_switch_save();
        let before = s.body().to_vec();
        assert!(set(&mut s, 0, 1507, true));
        assert_eq!(get(&s, 0, 1507), Some(true));
        assert_eq!(count(&s, 0), 1);
        assert!(set(&mut s, 0, 1507, false));
        assert_eq!(get(&s, 0, 1507), Some(false));
        assert_eq!(count(&s, 0), 0);
        assert_eq!(s.body(), &before[..], "a set and a clear must restore the map");
    }

    #[test]
    fn out_of_range_is_refused() {
        let mut s = save::empty_switch_save();
        let before = s.body().to_vec();
        assert_eq!(get(&s, 0, FLAGS), None);
        assert_eq!(get(&s, 0, usize::MAX), None);
        assert!(!set(&mut s, 0, FLAGS, true));
        assert!(!set(&mut s, 0, usize::MAX, true));
        assert_eq!(count(&s, 0), 0);
        assert!(flags_near(&s, 0, FLAGS, 4).is_empty());
        assert!(flags_near(&s, 0, usize::MAX, usize::MAX).is_empty());
        // No such slot: the base check covers it.
        assert_eq!(get(&s, 3, 0), None);
        assert!(!set(&mut s, 3, 0, true));
        assert_eq!(count(&s, 3), 0);
        assert!(flags_near(&s, 3, 0, 8).is_empty());
        assert_eq!(s.body(), &before[..], "refused writes must not touch the body");
    }

    #[test]
    fn the_last_flag_and_a_clipped_window_work() {
        let mut s = save::empty_switch_save();
        assert!(set(&mut s, 0, FLAGS - 1, true));
        assert_eq!(get(&s, 0, FLAGS - 1), Some(true));
        assert_eq!(flags_near(&s, 0, FLAGS - 3, 10).len(), 3);
        assert_eq!(flags_near(&s, 0, FLAGS - 3, 10), alloc::vec![false, false, true]);
        assert!(flags_near(&s, 0, 0, 0).is_empty());
    }

    #[test]
    fn mhgu_saves_are_refused() {
        // A well formed MHGU container: body marker, and three slot bases one
        // MHGU stride apart, so only the kind check can reject it.
        let body_len = crate::MHGU_SIZE - crate::SWITCH_HEADER_SIZE;
        let mut data = alloc::vec![0u8; crate::MHGU_SIZE];
        data[..crate::SWITCH_HEADER_SIZE]
            .copy_from_slice(&save::switch_header("system", body_len));
        data[crate::SWITCH_HEADER_SIZE] = 0xC6;
        for (i, off) in [0x10usize, 0x14, 0x18].iter().enumerate() {
            let base = crate::save::FIRST_SLOT_BASE as usize + crate::save::MHGU_SLOT_STRIDE * i;
            let at = crate::SWITCH_HEADER_SIZE + off;
            data[at..at + 4].copy_from_slice(&(base as u32).to_le_bytes());
        }
        let mut s = Save::from_bytes(&data).expect("mhgu save must parse");
        assert_eq!(s.kind(), SaveKind::Mhgu);
        assert_eq!(get(&s, 0, 0), None);
        assert!(!set(&mut s, 0, 0, true));
        assert_eq!(count(&s, 0), 0);
        assert!(flags_near(&s, 0, 0, 8).is_empty());
    }
}
