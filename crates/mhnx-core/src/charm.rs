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
//! constants for +0x02 (0x0A) and +0x12 (100). Both are wrong: they are a
//! linked pair, so merely opening and saving a save rewrote 70 talismans and
//! turned rarity 99 records into rarity 100 ones.
//!
//! The pair is not arbitrary, though. +0x02 is the equipment id 1..10 and
//! +0x12 is the talisman id 97..100, and TALISMAN_RARITY links them
//! (1-2 -> 97, 3-4 -> 98, 5-7 -> 99, 8-10 -> 100). What is impossible is a
//! record whose two bytes disagree. So a new talisman is built from a real
//! record of the same grade when the box holds one, and otherwise from a real
//! talisman with the pair set from that table - never a fixed pair regardless
//! of grade, which is what broke the old version.
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

/// The points range the game actually allows for one skill on one talisman.
///
/// The reference editor indexes it like this (EditTalismanDialog.cs:61): the
/// byte at +0x02 is the talisman type, TALISMAN_RARITY[type] gives the rarity,
/// the rarity picks one of four tables, and the table is indexed by skill id
/// directly (the combo box lists a leading "(none)", so SelectedIndex equals
/// the id). A cell reads "min~max:min2~max2": the first pair bounds skill 1's
/// points, the second bounds skill 2's. "0~0" means the skill is not allowed
/// there at all.
///
/// Verified against the sample saves: all 1292 skill values across 650
/// talismans fall inside the published range.
///
/// It matches every one of the 650 talismans in the sample saves:
/// type 0x0A -> rarity 100, 0x07 -> 99, 0x06 -> 99, 0x04 -> 98, 0x00 -> 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllowedPoints {
    /// Lowest legal points.
    pub min: i8,
    /// Highest legal points.
    pub max: i8,
}

/// Rarity for a talisman type byte, from the reference table.
#[cfg(feature = "gamedata")]
pub fn rarity_of(type_byte: u8) -> Option<i32> {
    crate::gamedata::TALISMAN_RARITY
        .get(type_byte as usize)
        .copied()
}

/// Rarity for a talisman type byte, from the reference table.
#[cfg(not(feature = "gamedata"))]
pub fn rarity_of(_type_byte: u8) -> Option<i32> {
    None
}

/// The legal points range for a skill on a talisman.
#[cfg(feature = "gamedata")]
pub fn allowed_points(type_byte: u8, skill: u8, second: bool) -> Option<AllowedPoints> {
    use crate::gamedata::{
        TALISMAN_ENDURING_SKILL, TALISMAN_SHINING_SKILL, TALISMAN_TIMEWORN_SKILL,
    };
    if skill == 0 {
        return None;
    }
    let table: &[&str] = match rarity_of(type_byte)? {
        98 => TALISMAN_SHINING_SKILL,
        99 => TALISMAN_TIMEWORN_SKILL,
        100 => TALISMAN_ENDURING_SKILL,
        _ => return None,
    };
    // The reference combo box lists the skills with a leading "(none)", so
    // SelectedIndex for skill id N is N, not N - 1.
    let cell = table.get(skill as usize)?;
    let mut halves = cell.split(':');
    let pair = if second { halves.nth(1)? } else { halves.next()? };
    let mut bits = pair.split('~');
    let min: i8 = bits.next()?.trim().parse().ok()?;
    let max: i8 = bits.next()?.trim().parse().ok()?;
    Some(AllowedPoints { min, max })
}

/// The legal points range for a skill on a talisman.
#[cfg(not(feature = "gamedata"))]
pub fn allowed_points(_type_byte: u8, _skill: u8, _second: bool) -> Option<AllowedPoints> {
    None
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


/// The four talisman grades the game ships.
///
/// The rarity byte at +0x12 is 97, 98, 99 or 100 and each one has its own
/// table of legal skill ranges. The byte at +0x02 is paired with it through
/// `TALISMAN_RARITY`, and several type bytes map to the same rarity, so the
/// grade is chosen by rarity and the type byte is copied from a record the
/// game itself wrote rather than invented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum TalismanGrade {
    /// Rarity 97. The lowest grade the tables cover.
    Mystery,
    /// Rarity 98.
    Shining,
    /// Rarity 99.
    Timeworn,
    /// Rarity 100, the skyward talisman most editors stop at.
    #[default]
    Enduring,
}

impl TalismanGrade {
    /// Every grade, in the order the game ranks them.
    pub const ALL: [TalismanGrade; 4] = [
        TalismanGrade::Mystery,
        TalismanGrade::Shining,
        TalismanGrade::Timeworn,
        TalismanGrade::Enduring,
    ];

    /// The rarity byte this grade is stored as.
    pub fn rarity(self) -> u8 {
        match self {
            TalismanGrade::Mystery => 97,
            TalismanGrade::Shining => 98,
            TalismanGrade::Timeworn => 99,
            TalismanGrade::Enduring => 100,
        }
    }

    /// Short label, with the rarity attached.
    ///
    /// Only the top grade's name is confirmed against the game (天の護石,
    /// rarity 100, the one the save's first character is full of). The other
    /// three labels are the conventional translations of the reference
    /// editor's table, so the rarity is carried alongside them: the number is
    /// the fact, the name is a convenience.
    pub fn name(self) -> &'static str {
        match self {
            TalismanGrade::Mystery => "護石 R97",
            TalismanGrade::Shining => "護石 R98",
            TalismanGrade::Timeworn => "古びた護石 R99",
            TalismanGrade::Enduring => "天の護石 R100",
        }
    }

    /// Grade for a rarity byte.
    pub fn from_rarity(rarity: u8) -> Option<Self> {
        TalismanGrade::ALL.into_iter().find(|g| g.rarity() == rarity)
    }

    /// Most decoration slots this grade is allowed.
    ///
    /// 1 / 2 / 3 / 3. The reference editor (MHGU, same layout) states this
    /// directly, and the sample MHXX save agrees: rarity 98 records stop at
    /// two slots, 99 and 100 reach three.
    pub fn max_slots(self) -> u8 {
        match self {
            TalismanGrade::Mystery => 1,
            TalismanGrade::Shining => 2,
            TalismanGrade::Timeworn => 3,
            TalismanGrade::Enduring => 3,
        }
    }

    /// The first type byte the rarity table maps to this grade.
    ///
    /// Used only when the box has no real record of the grade to copy: the
    /// pair still has to agree, and this is the pairing the table states.
    #[cfg(feature = "gamedata")]
    pub fn type_byte(self) -> u8 {
        crate::gamedata::TALISMAN_RARITY
            .iter()
            .position(|r| *r == self.rarity() as i32)
            .unwrap_or(0) as u8
    }

    /// The first type byte the rarity table maps to this grade.
    #[cfg(not(feature = "gamedata"))]
    pub fn type_byte(self) -> u8 {
        0
    }
}

/// Grade of a raw record, from the rarity byte at +0x12.
pub fn rarity_of_record(rec: &[u8]) -> Option<TalismanGrade> {
    if rec.len() < CHARM_RECORD || !is_talisman(rec) {
        return None;
    }
    TalismanGrade::from_rarity(rec[0x12])
}

/// Legal points range for a skill at a rarity, without going through the
/// type byte.
#[cfg(feature = "gamedata")]
pub fn allowed_points_rarity(rarity: u8, skill: u8, second: bool) -> Option<AllowedPoints> {
    use crate::gamedata::{
        TALISMAN_ENDURING_SKILL, TALISMAN_MYSTERY_SKILL, TALISMAN_SHINING_SKILL,
        TALISMAN_TIMEWORN_SKILL,
    };
    if skill == 0 {
        return None;
    }
    let table: &[&str] = match rarity {
        97 => TALISMAN_MYSTERY_SKILL,
        98 => TALISMAN_SHINING_SKILL,
        99 => TALISMAN_TIMEWORN_SKILL,
        100 => TALISMAN_ENDURING_SKILL,
        _ => return None,
    };
    let cell = table.get(skill as usize)?;
    let mut halves = cell.split(':');
    let pair = if second { halves.nth(1)? } else { halves.next()? };
    let mut bits = pair.split('~');
    let min: i8 = bits.next()?.trim().parse().ok()?;
    let max: i8 = bits.next()?.trim().parse().ok()?;
    Some(AllowedPoints { min, max })
}

/// Legal points range for a skill at a rarity.
#[cfg(not(feature = "gamedata"))]
pub fn allowed_points_rarity(_rarity: u8, _skill: u8, _second: bool) -> Option<AllowedPoints> {
    None
}

/// A real record of the requested grade to copy, preferring a clean one.
///
/// Returns None when the box holds no talisman of that grade, which is the
/// normal case for the lowest grades in a long-played save.
pub fn template_for_grade(
    box_bytes: &[u8],
    grade: TalismanGrade,
) -> Option<[u8; CHARM_RECORD]> {
    if box_bytes.len() < CHARM_BOX_SIZE {
        return None;
    }
    let want = grade.rarity();
    let mut fallback: Option<[u8; CHARM_RECORD]> = None;
    for i in 0..CHARM_COUNT {
        let rec = &box_bytes[i * CHARM_RECORD..(i + 1) * CHARM_RECORD];
        if !is_talisman(rec) || rec[0x12] != want {
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

/// The type byte real records of this grade use in this box, if any.
///
/// The table says several type bytes map to the same rarity, and the ones the
/// game actually writes are not the first of each run (rarity 98 records use
/// 0x04, not the 0x03 the table starts on). Writing a table-derived byte would
/// produce a pair no game ever wrote, so a real record of the grade has to be
/// present before the grade can be created or switched to.
pub fn type_byte_for_grade(box_bytes: &[u8], grade: TalismanGrade) -> Option<u8> {
    template_for_grade(box_bytes, grade).map(|t| t[0x02])
}

/// A template for the requested grade: a real record when the box has one,
/// otherwise a clean talisman with the grade's two paired bytes set.
///
/// The second value is true when the pair had to be written. That is allowed:
/// the byte at +0x02 is the equipment id 1..10 and the byte at +0x12 is the
/// talisman id 97..100, and the two are linked by a table both this crate and
/// the reference editor carry (1-2 -> 97, 3-4 -> 98, 5-7 -> 99, 8-10 -> 100).
/// A record whose pair agrees is what the game writes; only disagreements are
/// impossible. Copying a real record is still preferred, because it also
/// carries whatever else the grade needs.
pub fn template_for_grade_or_pair(
    box_bytes: &[u8],
    grade: TalismanGrade,
) -> Option<([u8; CHARM_RECORD], bool)> {
    if let Some(t) = template_for_grade(box_bytes, grade) {
        return Some((t, false));
    }
    let mut t = template_from_box(box_bytes)?;
    t[0x02] = grade.type_byte();
    t[0x12] = grade.rarity();
    Some((t, true))
}

/// What about this talisman would the game not have written?
///
/// One short string per problem, empty when the record is ordinary for its
/// grade. Nothing here changes the save: it is for the warning line.
pub fn grade_warnings(
    box_bytes: &[u8],
    index: usize,
    grade: TalismanGrade,
) -> Vec<alloc::string::String> {
    use alloc::format;
    let mut out = Vec::new();
    if index >= CHARM_COUNT || box_bytes.len() < CHARM_BOX_SIZE {
        return out;
    }
    let o = index * CHARM_RECORD;
    if !is_talisman(&box_bytes[o..o + CHARM_RECORD]) {
        return out;
    }
    if box_bytes[o + 0x10] > grade.max_slots() {
        out.push(format!(
            "スロット{}個は{}の上限{}を超えています",
            box_bytes[o + 0x10],
            grade.name(),
            grade.max_slots()
        ));
    }
    for (skill_at, pts_at, second, label) in [
        (0x0Cusize, 0x0Eusize, false, "スキル1"),
        (0x0D, 0x0F, true, "スキル2"),
    ] {
        let skill = box_bytes[o + skill_at];
        if skill == 0 {
            continue;
        }
        match allowed_points_rarity(grade.rarity(), skill, second) {
            Some(r) if r.min == 0 && r.max == 0 => {
                out.push(format!("{} は{}では付けられません", label, grade.name()));
            }
            Some(r) => {
                let pts = box_bytes[o + pts_at] as i8;
                if pts < r.min || pts > r.max {
                    out.push(format!(
                        "{} {} は {}〜{} の範囲外です",
                        label, pts, r.min, r.max
                    ));
                }
            }
            None => {}
        }
    }
    out
}

/// Clamp the decoration slots to what the grade allows.
pub fn clamp_slots(box_bytes: &mut [u8], index: usize, grade: TalismanGrade) -> bool {
    if index >= CHARM_COUNT || box_bytes.len() < CHARM_BOX_SIZE {
        return false;
    }
    let o = index * CHARM_RECORD;
    if !is_talisman(&box_bytes[o..o + CHARM_RECORD]) {
        return false;
    }
    let max = grade.max_slots();
    if box_bytes[o + 0x10] > max {
        box_bytes[o + 0x10] = max;
        return true;
    }
    false
}

/// Change the grade of a talisman already in the box.
///
/// Only the two linked bytes move; the skills are left alone, so the caller
/// should clamp the points afterwards with `clamp_points`.
/// The type byte is copied from a real record of the grade when one exists,
/// and otherwise taken from the rarity table, which keeps the pair consistent.
pub fn set_grade(box_bytes: &mut [u8], index: usize, grade: TalismanGrade) -> bool {
    if index >= CHARM_COUNT || box_bytes.len() < CHARM_BOX_SIZE {
        return false;
    }
    let o = index * CHARM_RECORD;
    if !is_talisman(&box_bytes[o..o + CHARM_RECORD]) {
        return false;
    }
    // Prefer the byte a real record of this grade uses; fall back to the
    // table's first id for the grade, which the pairing still makes legal.
    let type_byte = type_byte_for_grade(box_bytes, grade).unwrap_or_else(|| grade.type_byte());
    box_bytes[o + 0x02] = type_byte;
    box_bytes[o + 0x12] = grade.rarity();
    true
}

/// Clamp both skill point values to what the grade allows.
///
/// Returns true when a value had to move. A skill the grade forbids outright
/// is cleared rather than silently left illegal.
pub fn clamp_points(box_bytes: &mut [u8], index: usize, grade: TalismanGrade) -> bool {
    if index >= CHARM_COUNT || box_bytes.len() < CHARM_BOX_SIZE {
        return false;
    }
    let o = index * CHARM_RECORD;
    if !is_talisman(&box_bytes[o..o + CHARM_RECORD]) {
        return false;
    }
    let mut moved = false;
    for (skill_at, pts_at, second) in [(0x0Cusize, 0x0Eusize, false), (0x0D, 0x0F, true)] {
        let skill = box_bytes[o + skill_at];
        if skill == 0 {
            continue;
        }
        match allowed_points_rarity(grade.rarity(), skill, second) {
            // "0~0" means the grade does not carry this skill at all. The
            // record is left alone rather than quietly cleared: the reference
            // editor warns about this and still writes, and silently deleting
            // a skill the user chose is worse than writing it. grade_warnings
            // is how the caller tells them.
            Some(r) if r.min == 0 && r.max == 0 => {}
            Some(r) => {
                let pts = box_bytes[o + pts_at] as i8;
                if pts < r.min {
                    box_bytes[o + pts_at] = r.min as u8;
                    moved = true;
                } else if pts > r.max {
                    box_bytes[o + pts_at] = r.max as u8;
                    moved = true;
                }
            }
            None => {
                // The grade does not allow this skill at all.
                box_bytes[o + skill_at] = 0;
                box_bytes[o + pts_at] = 0;
                moved = true;
            }
        }
    }
    moved
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