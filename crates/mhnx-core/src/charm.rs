//! Talismans (お守り) inside the equipment box.
//!
//! slot + 0x62EE is NOT a charm-only box: it is the character's whole
//! equipment box, 2000 records of 36 bytes, holding weapons, armour and
//! talismans side by side. In the sample save it holds 188 talismans,
//! 601 weapons and pieces of armour, and 1211 blank records.
//!
//! Every record starts with a type and a level packed into the first bytes:
//!
//! - type = byte0 & 0x1F
//! - level = (byte1 << 3) | (byte0 >> 5)
//!
//! Type 6 is a talisman; 1..5 are the five armour pieces and 7..20 are the
//! weapon classes. A record of all zeros is empty.
//!
//! A talisman record, measured across all 650 talismans of the sample saves:
//!
//! - +0x00 u8 : 0x06, i.e. type 6 at level 0
//! - +0x01 u8 : 0x00
//! - +0x02 u8 : NOT constant. Takes 0x00, 0x04, 0x06, 0x07 and 0x0A, paired
//!   with the rarity at +0x12 the way the reference editor's rarity table is.
//! - +0x06..+0x09 : zero in most records but non-zero in 15 of the 650
//! - +0x0C u8 : first skill id (1-based into skills::SKILL_NAMES, 0 = none)
//! - +0x0D u8 : second skill id
//! - +0x0E i8 : first skill points (signed: real records carry 0xFF/0xFE/0xFD)
//! - +0x0F i8 : second skill points
//! - +0x10 u8 : decoration slots
//! - +0x11 u8 : 0x00 in all 650
//! - +0x12 u8 : rarity, 98, 99 or 100
//! - +0x13 u8 : 0x01 in all 650
//!
//! # Writing rules
//!
//! Only the five fields an editor owns are ever written: +0x0C, +0x0D, +0x0E,
//! +0x0F and +0x10. Everything else is preserved byte for byte.
//!
//! An earlier version of this module zeroed the record and wrote fixed
//! constants for +0x02 (0x0A) and +0x12 (100). Both are wrong - they are a
//! linked pair that varies - so merely opening and saving a save rewrote 70
//! talismans and turned rarity 99 records into rarity 100 ones. Never
//! synthesise those bytes: copy them from a record the game wrote.
//!
//! A brand new talisman therefore needs a template (see create_charm), and
//! writing one over a weapon or a piece of armour is refused outright.

use alloc::vec::Vec;

use crate::skills::skill_name;

/// Number of records in the equipment box.
pub const CHARM_COUNT: usize = 2000;
/// Size of one equipment record.
pub const CHARM_RECORD: usize = 36;
/// Size of the whole equipment box.
pub const CHARM_BOX_SIZE: usize = CHARM_COUNT * CHARM_RECORD;
/// Record type that marks a talisman.
pub const CHARM_TYPE: u8 = 6;
/// Type field mask.
pub const TYPE_MASK: u8 = 0x1F;

/// Type field of a record (low 5 bits of the first byte).
pub fn record_type(rec: &[u8]) -> u8 {
    rec.first().copied().unwrap_or(0) & TYPE_MASK
}

/// Is this record completely blank?
pub fn is_empty_record(rec: &[u8]) -> bool {
    rec.iter().all(|b| *b == 0)
}

/// Does this record hold a talisman?
pub fn is_talisman(rec: &[u8]) -> bool {
    !is_empty_record(rec) && record_type(rec) == CHARM_TYPE
}

/// May a talisman be written here without destroying something else?
pub fn is_writable(rec: &[u8]) -> bool {
    is_empty_record(rec) || is_talisman(rec)
}

/// One talisman.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Charm {
    /// Record index inside the box (0-based).
    pub index: usize,
    /// Decoration slots.
    pub slots: u8,
    /// First skill id (1-based, 0 = none).
    pub skill1: u8,
    /// First skill points (signed).
    pub points1: i8,
    /// Second skill id (1-based, 0 = none).
    pub skill2: u8,
    /// Second skill points (signed).
    pub points2: i8,
}

impl Charm {
    /// Name of the first skill.
    pub fn skill1_name(&self) -> Option<&'static str> {
        skill_name(self.skill1)
    }
    /// Name of the second skill.
    pub fn skill2_name(&self) -> Option<&'static str> {
        skill_name(self.skill2)
    }
    /// One line summary, used by the CLI and the GUI.
    pub fn summary(&self) -> alloc::string::String {
        use alloc::format;
        let mut s = if self.slots == 0 {
            alloc::string::String::from("[---]")
        } else {
            format!("[{} slot{}]", self.slots, if self.slots > 1 { "s" } else { "" })
        };
        if let Some(n) = self.skill1_name() {
            s += &format!(" {} {:+}", n, self.points1);
        }
        if let Some(n) = self.skill2_name() {
            s += &format!(" / {} {:+}", n, self.points2);
        }
        s
    }
}

/// The points range actually seen for one skill at one rarity.
///
/// The game data ships four tables of allowed ranges (TALISMAN_*_SKILL), but
/// how they are indexed could not be established with confidence, so this
/// derives the answer from the box itself instead of guessing: every talisman
/// in it contributes one sample. A value outside the observed range is not
/// proof of an illegal record, but it is worth flagging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointsRange {
    /// Lowest points seen.
    pub low: i8,
    /// Highest points seen.
    pub high: i8,
    /// How many talismans back this up.
    pub samples: usize,
}

/// Observed points range for (rarity at +0x12, skill id).
pub fn observed_points_range(
    box_bytes: &[u8],
    rarity: u8,
    skill: u8,
) -> Option<PointsRange> {
    if box_bytes.len() < CHARM_BOX_SIZE || skill == 0 {
        return None;
    }
    let mut out: Option<PointsRange> = None;
    for i in 0..CHARM_COUNT {
        let o = i * CHARM_RECORD;
        let rec = &box_bytes[o..o + CHARM_RECORD];
        if !is_talisman(rec) || rec[0x12] != rarity {
            continue;
        }
        for (sid, pts) in [
            (rec[0x0C], rec[0x0E] as i8),
            (rec[0x0D], rec[0x0F] as i8),
        ] {
            if sid != skill {
                continue;
            }
            out = Some(match out {
                None => PointsRange {
                    low: pts,
                    high: pts,
                    samples: 1,
                },
                Some(r) => PointsRange {
                    low: r.low.min(pts),
                    high: r.high.max(pts),
                    samples: r.samples + 1,
                },
            });
        }
    }
    out
}

/// Decode every talisman in an equipment box.
///
/// box_bytes must be the 72000 bytes starting at slot + 0x62EE.
pub fn decode_box(box_bytes: &[u8]) -> Vec<Charm> {
    let mut out = Vec::new();
    if box_bytes.len() < CHARM_BOX_SIZE {
        return out;
    }
    for i in 0..CHARM_COUNT {
        let o = i * CHARM_RECORD;
        let rec = &box_bytes[o..o + CHARM_RECORD];
        if !is_talisman(rec) {
            continue;
        }
        out.push(Charm {
            index: i,
            slots: rec[0x10],
            skill1: rec[0x0C],
            points1: rec[0x0E] as i8,
            skill2: rec[0x0D],
            points2: rec[0x0F] as i8,
        });
    }
    out
}
/// Modify a talisman, touching only the five fields an editor owns.
///
/// Everything else stays exactly as the game wrote it. Returns false if the
/// record is not already a talisman.
pub fn update_charm(box_bytes: &mut [u8], charm: &Charm) -> bool {
    if charm.index >= CHARM_COUNT || box_bytes.len() < CHARM_BOX_SIZE {
        return false;
    }
    let o = charm.index * CHARM_RECORD;
    if !is_talisman(&box_bytes[o..o + CHARM_RECORD]) {
        return false;
    }
    box_bytes[o + 0x0C] = charm.skill1;
    box_bytes[o + 0x0D] = charm.skill2;
    box_bytes[o + 0x0E] = charm.points1 as u8;
    box_bytes[o + 0x0F] = charm.points2 as u8;
    box_bytes[o + 0x10] = charm.slots;
    true
}

/// Create a talisman in a blank record by copying a real one.
///
/// The rarity at +0x12 and its paired byte at +0x02 are not understood well
/// enough to invent, so a template the game itself wrote is required rather
/// than optional. Use template_from_box to get one.
pub fn create_charm(box_bytes: &mut [u8], index: usize, template: &[u8], charm: &Charm) -> bool {
    if index >= CHARM_COUNT || box_bytes.len() < CHARM_BOX_SIZE {
        return false;
    }
    if template.len() != CHARM_RECORD || !is_talisman(template) {
        return false;
    }
    let o = index * CHARM_RECORD;
    if !is_empty_record(&box_bytes[o..o + CHARM_RECORD]) {
        return false;
    }
    box_bytes[o..o + CHARM_RECORD].copy_from_slice(template);
    box_bytes[o + 0x0C] = charm.skill1;
    box_bytes[o + 0x0D] = charm.skill2;
    box_bytes[o + 0x0E] = charm.points1 as u8;
    box_bytes[o + 0x0F] = charm.points2 as u8;
    box_bytes[o + 0x10] = charm.slots;
    true
}

/// The most common talisman in this box, used as a template for new ones.
///
/// Picks the modal (rarity, +0x02) pair. The sample saves are dominated by
/// rarity 100 with 0x0A at +0x02, so that is what a new talisman looks like.
/// Is this a "clean" talisman: the modal shape with nothing in the four bytes
/// at +0x06..+0x09?
///
/// 15 of the 650 talismans in the sample saves carry data there, and they all
/// sit in the modal (rarity 100, +0x02 = 0x0A) class. Cloning one of those
/// copies junk into every new talisman, so a clean record is preferred.
fn is_clean(rec: &[u8]) -> bool {
    rec[0x06..0x0A].iter().all(|b| *b == 0)
}

pub fn template_from_box(box_bytes: &[u8]) -> Option<[u8; CHARM_RECORD]> {
    if box_bytes.len() < CHARM_BOX_SIZE {
        return None;
    }
    // Two passes: find the most common (rarity, +0x02) pair, then prefer the
    // first record of that pair whose tail is clean. Falling back to the first
    // record of the pair keeps the function total.
    let mut counts: Vec<((u8, u8), usize)> = Vec::new();
    for i in 0..CHARM_COUNT {
        let rec = &box_bytes[i * CHARM_RECORD..(i + 1) * CHARM_RECORD];
        if !is_talisman(rec) {
            continue;
        }
        let key = (rec[0x12], rec[0x02]);
        match counts.iter_mut().find(|(k, _)| *k == key) {
            Some((_, c)) => *c += 1,
            None => counts.push((key, 1)),
        }
    }
    let modal = counts.into_iter().max_by_key(|(_, n)| *n).map(|(k, _)| k)?;

    let mut fallback: Option<[u8; CHARM_RECORD]> = None;
    for i in 0..CHARM_COUNT {
        let rec = &box_bytes[i * CHARM_RECORD..(i + 1) * CHARM_RECORD];
        if !is_talisman(rec) || (rec[0x12], rec[0x02]) != modal {
            continue;
        }
        let mut out = [0u8; CHARM_RECORD];
        out.copy_from_slice(rec);
        if is_clean(rec) {
            return Some(out);
        }
        if fallback.is_none() {
            fallback = Some(out);
        }
    }
    fallback
}

/// Remove a talisman.
///
/// Refuses to blank anything that is not a talisman.
pub fn clear_charm(box_bytes: &mut [u8], index: usize) -> bool {
    if index >= CHARM_COUNT || box_bytes.len() < CHARM_BOX_SIZE {
        return false;
    }
    let o = index * CHARM_RECORD;
    if !is_talisman(&box_bytes[o..o + CHARM_RECORD]) {
        return false;
    }
    for b in &mut box_bytes[o..o + CHARM_RECORD] {
        *b = 0;
    }
    true
}

/// First record that holds no equipment at all.
pub fn first_free(box_bytes: &[u8]) -> Option<usize> {
    if box_bytes.len() < CHARM_BOX_SIZE {
        return None;
    }
    (0..CHARM_COUNT).find(|i| is_empty_record(&box_bytes[i * CHARM_RECORD..(i + 1) * CHARM_RECORD]))
}

/// A record a talisman may be written to: a blank one, or the first talisman
/// so an edit has somewhere to point.
///
/// Returns None when the box holds neither, rather than handing back a weapon.
/// An earlier version fell back to "first index with no decoded talisman",
/// which on a box with no blanks returned 0 - a Great Sword.
pub fn first_writable(box_bytes: &[u8]) -> Option<usize> {
    if let Some(i) = first_free(box_bytes) {
        return Some(i);
    }
    decode_box(box_bytes).first().map(|c| c.index)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn weapon_at_zero() -> Vec<u8> {
        let mut b = alloc::vec![0u8; CHARM_BOX_SIZE];
        // The first record of the sample save: Great Sword, level 10, id 26.
        b[0] = 0x47;
        b[1] = 0x01;
        b[2] = 0x1A;
        b
    }

    /// A rarity 99 talisman, the shape the sample saves hold 68 of.
    fn rarity_99_record() -> [u8; CHARM_RECORD] {
        let mut r = [0u8; CHARM_RECORD];
        r[0x00] = 0x06;
        r[0x02] = 0x06;
        r[0x06] = 0x28;
        r[0x07] = 0x0B;
        r[0x0C] = 0x4A;
        r[0x0D] = 0x46;
        r[0x0E] = 0x06;
        r[0x0F] = 0x0A;
        r[0x10] = 0x03;
        r[0x12] = 99;
        r[0x13] = 0x01;
        r
    }

    #[test]
    fn a_weapon_is_not_a_talisman() {
        let b = weapon_at_zero();
        assert_eq!(record_type(&b[..CHARM_RECORD]), 7);
        assert!(!is_talisman(&b[..CHARM_RECORD]));
        assert!(!is_writable(&b[..CHARM_RECORD]));
        assert!(decode_box(&b).is_empty());
    }

    #[test]
    fn a_talisman_with_level_bits_is_still_found() {
        let mut b = alloc::vec![0u8; CHARM_BOX_SIZE];
        b[0] = 0xE6; // type 6, level 7
        b[0x0C] = 1;
        b[0x10] = 3;
        assert_eq!(decode_box(&b).len(), 1);
    }

    #[test]
    fn updating_refuses_a_weapon() {
        let mut b = weapon_at_zero();
        let before = b[..CHARM_RECORD].to_vec();
        let c = Charm { index: 0, slots: 3, skill1: 1, points1: 6, skill2: 200, points2: -2 };
        assert!(!update_charm(&mut b, &c));
        assert_eq!(&b[..CHARM_RECORD], &before[..]);
    }

    #[test]
    fn updating_preserves_every_byte_it_does_not_own() {
        let mut b = alloc::vec![0u8; CHARM_BOX_SIZE];
        let template = rarity_99_record();
        b[..CHARM_RECORD].copy_from_slice(&template);

        let c = Charm { index: 0, slots: 2, skill1: 7, points1: -3, skill2: 0, points2: 4 };
        assert!(update_charm(&mut b, &c));
        let rec = &b[..CHARM_RECORD];
        assert_eq!(rec[0x0C], 7);
        assert_eq!(rec[0x0D], 0);
        assert_eq!(rec[0x0E], 0xFD, "points are signed");
        assert_eq!(rec[0x0F], 4);
        assert_eq!(rec[0x10], 2);
        // The rarity pair and the bytes nobody understands survive.
        for off in [0x00usize, 0x01, 0x02, 0x03, 0x06, 0x07, 0x11, 0x12, 0x13] {
            assert_eq!(rec[off], template[off], "byte {:#04x} was rewritten", off);
        }
        assert_eq!(rec[0x12], 99, "rarity 99 must survive an edit");
        assert_eq!(rec[0x02], 0x06);
    }

    #[test]
    fn creating_copies_a_template_and_keeps_its_unknown_bytes() {
        let mut b = alloc::vec![0u8; CHARM_BOX_SIZE];
        let template = rarity_99_record();
        b[..CHARM_RECORD].copy_from_slice(&template);
        let c = Charm { index: 5, slots: 3, skill1: 2, points1: 1, skill2: 0, points2: 0 };
        assert!(create_charm(&mut b, 5, &template, &c));
        let rec = &b[5 * CHARM_RECORD..6 * CHARM_RECORD];
        assert_eq!(rec[0x0C], 2);
        assert_eq!(rec[0x12], template[0x12], "the template rarity carries over");
        assert_eq!(rec[0x02], template[0x02]);
        assert_eq!(rec[0x06], template[0x06], "unknown bytes carry over");
        assert_eq!(decode_box(&b).len(), 2);
    }

    #[test]
    fn creating_refuses_a_non_blank_record_or_a_bad_template() {
        let mut b = weapon_at_zero();
        let template = rarity_99_record();
        let c = Charm { index: 0, slots: 3, skill1: 2, points1: 1, skill2: 0, points2: 0 };
        assert!(!create_charm(&mut b, 0, &template, &c), "record 0 is a weapon");
        let mut b2 = alloc::vec![0u8; CHARM_BOX_SIZE];
        assert!(!create_charm(&mut b2, 1, &b[..CHARM_RECORD], &c), "a weapon is not a template");
    }

    #[test]
    fn template_from_box_prefers_the_common_shape() {
        let mut b = alloc::vec![0u8; CHARM_BOX_SIZE];
        b[..CHARM_RECORD].copy_from_slice(&rarity_99_record());
        let mut t100 = rarity_99_record();
        t100[0x12] = 100;
        t100[0x02] = 0x0A;
        b[CHARM_RECORD..2 * CHARM_RECORD].copy_from_slice(&t100);
        b[2 * CHARM_RECORD..3 * CHARM_RECORD].copy_from_slice(&t100);
        let got = template_from_box(&b).unwrap();
        assert_eq!(got[0x12], 100, "the modal pair wins");
        assert_eq!(got[0x02], 0x0A);
    }

    #[test]
    fn clearing_refuses_a_weapon() {
        let mut b = weapon_at_zero();
        assert!(!clear_charm(&mut b, 0));
        assert_eq!(b[0], 0x47);
    }

    /// 15 of the 650 real talismans carry data at +0x06..+0x09. Cloning such a
    /// record would spread those bytes into every new talisman, so the picker
    /// must prefer a clean one even when the dirty shape is more common.
    #[test]
    fn template_from_box_skips_a_record_with_a_dirty_tail() {
        let mut b = alloc::vec![0u8; CHARM_BOX_SIZE];
        let mut dirty = rarity_99_record();
        dirty[0x12] = 100;
        dirty[0x02] = 0x0A;
        dirty[0x06] = 0x8C;
        dirty[0x07] = 0x0A;
        // Two dirty ones, so that shape is "modal" by count.
        b[..CHARM_RECORD].copy_from_slice(&dirty);
        b[CHARM_RECORD..2 * CHARM_RECORD].copy_from_slice(&dirty);
        // One clean record of the same (rarity, +0x02) pair. The sample
        // record carries data at +0x06..+0x09, so clear it here: that is what
        // "clean" means for this test.
        let mut clean = rarity_99_record();
        clean[0x12] = 100;
        clean[0x02] = 0x0A;
        clean[0x06] = 0;
        clean[0x07] = 0;
        clean[0x08] = 0;
        clean[0x09] = 0;
        let last = (CHARM_COUNT - 1) * CHARM_RECORD;
        b[last..last + CHARM_RECORD].copy_from_slice(&clean);

        let got = template_from_box(&b).unwrap();
        assert_eq!(got[0x12], 100);
        assert!(
            got[0x06..0x0A].iter().all(|x| *x == 0),
            "a clean record must win: {:02x?}",
            &got[0x06..0x0A]
        );
    }

    #[test]
    fn first_writable_never_returns_a_weapon() {
        let mut b = alloc::vec![0u8; CHARM_BOX_SIZE];
        b[0] = 0x47;
        b[CHARM_RECORD] = 0x06;
        b[CHARM_RECORD + 0x0C] = 3;
        assert_eq!(first_free(&b), Some(2));
        assert_eq!(first_writable(&b), Some(2));

        // A box with no blank record and no talisman must give None, not 0.
        let mut full = alloc::vec![0u8; CHARM_BOX_SIZE];
        for i in 0..CHARM_COUNT {
            full[i * CHARM_RECORD] = 0x07;
        }
        assert_eq!(first_free(&full), None);
        assert_eq!(first_writable(&full), None);
    }

    #[test]
    fn box_size_matches_the_layout() {
        assert_eq!(
            crate::save::OFF_EQUIPMENT_BOX + CHARM_BOX_SIZE,
            crate::save::OFF_PALICO_EQUIPMENT
        );
    }
}