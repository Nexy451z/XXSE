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
//! The pair is not arbitrary, though. +0x02 is the talisman *kind* - the row of
//! `amuletData_jpn.gmd`, 1 兵士 … 10 天 with 7 龍, see [TALISMAN_KINDS] - and
//! +0x12/+0x13 is the u16 item id of the charm the talisman came from, 353..356
//! (0x161..0x164): rarity 97..100 in the low byte and 1 in the high byte.
//! TALISMAN_RARITY links the kind to that rarity (1-2 -> 97, 3-4 -> 98,
//! 5-7 -> 99, 8-10 -> 100). What is impossible is a record whose two bytes
//! disagree, so every writer here goes through [set_kind] and writes both.
//!
//! Two earlier versions got this wrong and both showed up on hardware as "the
//! talisman I made is not the one I asked for":
//!
//! * zeroing the record and writing a fixed 0x0A/100 pair turned every 99 into
//!   a 天の護石 the moment the save was opened and written;
//! * deriving the kind from the rarity table's first row wrote 0x08 (英雄の
//!   護石) for rarity 100, so a "天の護石" on screen was an 英雄 in game, and no
//!   other kind - 龍の護石 included - could be chosen at all.
//!
//! A new talisman is still copied from a real record when the box holds one of
//! the same kind, so the shape the game wrote is preserved; only the pair is
//! ever written on top, and only from the table above. Writing one over a
//! weapon or a piece of armour is refused outright.
//!
//! # Each kind has its own ceiling
//!
//! How many points one skill may carry, and how many decoration slots a
//! talisman may have, is not one number for all of them: the game ships four
//! point tables and four slot tables and a kind selects one through its rarity
//! (1-2 -> 97, 3-4 -> 98, 5-7 -> 99, 8-10 -> 100). [KIND_LIMITS] is that
//! measurement unrolled per kind, and [KindLimits] documents where each number
//! comes from. The short version:
//!
//! | kind | rarity | slot 1 | slot 2 | slots |
//! |---|---|---|---|---|
//! | 1 兵士 / 2 闘士 | 97 | +10 | 不可 | 1 |
//! | 3 騎士 / 4 城塞 | 98 | +10 | +13 | 2 |
//! | 5 女王 / 6 王 / 7 龍 | 99 | +8 | +13 | 3 |
//! | 8 英雄 / 9 伝説 / 10 天 | 100 | +10 | +13 | 3 |
//!
//! Two consequences the editor has to respect: a 兵士 / 闘士 cannot carry a
//! second skill at all, and a 女王 / 王 / 龍 tops out at +8 on the first skill
//! while the 天 tops out at +10. Nothing here clamps a record on its own -
//! [clamp_points] and [clamp_points_to_kind] only move a value when a caller
//! asks, and [grade_warnings] reports instead of writing.

use alloc::vec::Vec;

use crate::save::Save;
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
    let kind = template
        .get(0x02)
        .copied()
        .filter(|k| is_kind(*k))
        .unwrap_or(0);
    create_charm_kind(box_bytes, index, template, charm, kind)
}

/// Create a talisman of a chosen kind in a blank record.
///
/// Same as [create_charm], except that the kind byte at +0x02 and the rarity
/// item id at +0x12 are written from [set_kind] instead of being inherited
/// from the template. That is what makes a 龍の護石 possible at all: the sample
/// save holds nothing but 天の護石 records, so an editor that only copies a
/// template can never produce any other kind.
///
/// `kind` 0 means "keep whatever the template carries".
pub fn create_charm_kind(
    box_bytes: &mut [u8],
    index: usize,
    template: &[u8],
    charm: &Charm,
    kind: u8,
) -> bool {
    if index >= CHARM_COUNT || box_bytes.len() < CHARM_BOX_SIZE {
        return false;
    }
    if template.len() != CHARM_RECORD || !is_talisman(template) {
        return false;
    }
    if kind != 0 && !is_kind(kind) {
        return false;
    }
    let o = index * CHARM_RECORD;
    if !is_empty_record(&box_bytes[o..o + CHARM_RECORD]) {
        return false;
    }
    box_bytes[o..o + CHARM_RECORD].copy_from_slice(template);
    // The template supplies the grade pair, not its decorations: a talisman
    // with more socketed gems than it has sockets is a record the game never
    // writes. The three decoration ids are cleared here, and slots decides
    // how many the caller wants.
    box_bytes[o + 0x06..o + 0x0C].fill(0);
    box_bytes[o + 0x0C] = charm.skill1;
    box_bytes[o + 0x0D] = charm.skill2;
    box_bytes[o + 0x0E] = charm.points1 as u8;
    box_bytes[o + 0x0F] = charm.points2 as u8;
    box_bytes[o + 0x10] = charm.slots;
    if kind != 0 {
        // The template's pair is replaced, not left in place: a 龍の護石 made
        // from a 天の護石 template has to say 龍 in both linked bytes.
        set_kind(box_bytes, index, kind);
    }
    true
}

/// The most common talisman in this box, used as a template for new ones.
///
/// Picks the modal (rarity, +0x02) pair. The sample saves are dominated by
/// rarity 100 with 0x0A at +0x02, so that is what a new talisman looks like.
/// Is this a "clean" talisman: the modal shape with no decorations in it?
///
/// The three decoration sockets are u16 at +0x06, +0x08 and +0x0A, so the
/// check has to reach +0x0B; stopping at +0x0A left the third socket out and a
/// template with something in it handed that decoration to every new talisman.
/// 15 of the 650 talismans in the sample saves carry data here, and they all
/// sit in the modal (rarity 100, +0x02 = 0x0A) class.
fn is_clean(rec: &[u8]) -> bool {
    rec[0x06..0x0C].iter().all(|b| *b == 0)
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

// ---------------------------------------------------------------------------
// The eleven talisman kinds (the byte at +0x02)
// ---------------------------------------------------------------------------

/// The eleven rows of `amuletData_jpn.gmd`, in file order: the talisman kind
/// the record carries at +0x02.
///
/// The table is not guesswork. `romfs/nativeNX/table/amuletData_jpn.gmd`
/// (1448 bytes) holds eleven name strings and eleven descriptions, and its
/// companion `amuletData.amlt` states the count as 11 (9-byte entries, ids
/// 0..10). The names, read out of the file, are exactly these, in this order:
///
/// ```text
/// 0 装備なし    1 兵士の護石   2 闘士の護石   3 騎士の護石
/// 4 城塞の護石  5 女王の護石   6 王の護石     7 龍の護石
/// 8 英雄の護石  9 伝説の護石  10 天の護石
/// ```
///
/// Row 0 is "装備なし" (nothing), so a talisman record with 0x00 at +0x02 is
/// not one the game wrote. The database tab lists the same eleven strings
/// (database.rs, "護石名"), so the two must not drift.
pub const TALISMAN_KINDS: [&str; 11] = [
    "装備なし",
    "兵士の護石",
    "闘士の護石",
    "騎士の護石",
    "城塞の護石",
    "女王の護石",
    "王の護石",
    "龍の護石",
    "英雄の護石",
    "伝説の護石",
    "天の護石",
];

/// Rarity (97..100) of each kind, indexed by the byte at +0x02.
///
/// Same table as `gamedata::TALISMAN_RARITY`; kept here so the pairing works
/// without the `gamedata` feature (the console build has it, a bare
/// `cargo test -p mhnx-core` does not). Index 0 has no rarity: it is
/// "装備なし".
const KIND_RARITY: [u8; 11] = [0, 97, 97, 98, 98, 99, 99, 99, 100, 100, 100];

/// How many kinds the game actually ships: 1..=10.
pub const TALISMAN_KIND_COUNT: u8 = 10;

/// Name of a kind byte, or None when it is not one of the eleven rows.
pub fn kind_name(kind: u8) -> Option<&'static str> {
    TALISMAN_KINDS.get(kind as usize).copied()
}

/// Is this byte one of the ten talisman kinds the game ships (1..=10)?
///
/// 0 is the "装備なし" row and is reported as not a kind: a record with it at
/// +0x02 is not a record the game wrote.
pub fn is_kind(kind: u8) -> bool {
    kind >= 1 && kind <= TALISMAN_KIND_COUNT
}

/// Rarity (97..100) that belongs to a kind byte.
pub fn kind_rarity(kind: u8) -> Option<u8> {
    if !is_kind(kind) {
        return None;
    }
    Some(KIND_RARITY[kind as usize])
}

// ---------------------------------------------------------------------------
// The ceiling of each kind, measured from the game's tables
// ---------------------------------------------------------------------------

/// What one talisman kind can hold, measured from the game's own tables.
///
/// The game ships no table *per kind*: it ships four point tables and four
/// slot-probability tables, and a kind picks one of the four through its
/// rarity (1-2 -> 97, 3-4 -> 98, 5-7 -> 99, 8-10 -> 100, see [KIND_RARITY]).
/// [KIND_LIMITS] unrolls that into the eleven rows a save can carry, so a
/// screen can print the ceiling of a kind without walking a table.
///
/// # How the numbers were measured
///
/// `romfs/nativeNX/table`:
///
/// ```text
/// amuletSkillData00..07.amskl   8 bytes of header, then rows of
///                               u16 skill id, s8 min, s8 max
///                               (pair 2N / 2N+1 = rarity N: skill 1 / skill 2)
/// amuletSlotData00..03.amslt    8 bytes of header, then 21 rows of
///                               u8 prob[4] = how often 0/1/2/3 slots roll
/// ```
///
/// * `00/01` = rarity 97, `02/03` = 98, `04/05` = 99, `06/07` = 100. The
///   pairing is not a guess: those eight files reproduce the four
///   `gamedata::TALISMAN_*_SKILL` tables cell for cell (824 cells, 0
///   mismatches), and the reference editor indexes them by rarity.
/// * `max_first` / `max_second` are the largest `max` in each half. Rarity 97
///   has *no rows at all* in its second half, so 兵士 / 闘士 cannot carry a
///   second skill - the editor's table says `0~0` for every second cell there.
/// * `first_skills` / `second_skills` count the rows, i.e. how many skill ids
///   the kind can roll at all. 女王 / 王 / 龍 offer 56 in slot 1 and 105 in
///   slot 2; 英雄 / 伝説 / 天 offer 62 and 94.
/// * `max_slots` is the highest slot index whose probability is non-zero
///   anywhere in the rarity's slot table: 1 / 2 / 3 / 3.
///
/// Cross-checks, all on real data:
///
/// * The sample save (`C:\anyWorkspace\nx\jksv_ready\system`, 650 talismans,
///   1292 skill values on the 648 records that carry a kind): every value sits
///   inside the row of the table its kind selects, and 609 of those records fit
///   *only* that table - the other three reject them - so kind -> rarity ->
///   table is the road the game really writes. (The two remaining records carry
///   the 装備なし row with rarity 99; their single skill value also fits that
///   table, which is why the total checked is 1294.)
/// * The observed maxima agree: 天 (rarity 100) reaches +10 in slot 1 (skills
///   42/43, exactly the rows `amuletSkillData06` caps at 10) and +13 in slot 2
///   (skills 27..31, `amuletSkillData07`). 龍 (99) reaches +12 of its +13.
/// * `../extract/god_talisman_report.md` records the binary's own fallbacks:
///   聴覚保護 caps at +7 and 攻撃 at +10, which are the
///   `amuletSkillData06/07` numbers for skill ids 5 and 18.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindLimits {
    /// The kind byte at +0x02, 1..=10.
    pub kind: u8,
    /// The rarity that kind is stored as.
    pub rarity: u8,
    /// Largest points value slot 1 can carry.
    pub max_first: i8,
    /// Largest points value slot 2 can carry. Only meaningful when
    /// [KindLimits::second_skills] is not 0.
    pub max_second: i8,
    /// Most decoration slots this kind may carry.
    pub max_slots: u8,
    /// How many skill ids this kind can roll into slot 1.
    pub first_skills: u16,
    /// How many skill ids this kind can roll into slot 2 (0 = none).
    pub second_skills: u16,
}

impl KindLimits {
    /// The game's name of the kind.
    pub fn name(self) -> &'static str {
        kind_name(self.kind).unwrap_or("?")
    }

    /// Can this kind carry a second skill at all?
    pub fn has_second_skill(self) -> bool {
        self.second_skills > 0
    }

    /// The ceiling of one skill slot, or None when the kind has no such slot.
    pub fn max_points(self, second: bool) -> Option<i8> {
        if second {
            if self.second_skills == 0 {
                return None;
            }
            Some(self.max_second)
        } else {
            Some(self.max_first)
        }
    }

    /// One line the two screens print, e.g.
    /// 「天の護石: スキル1 最大 +10 / スキル2 最大 +13 / 装飾スロット 最大 3」.
    pub fn summary(self) -> alloc::string::String {
        use alloc::format;
        match self.max_points(true) {
            Some(second) => format!(
                "{}: スキル1 最大 {:+} / スキル2 最大 {:+} / 装飾スロット 最大 {}",
                self.name(),
                self.max_first,
                second,
                self.max_slots
            ),
            None => format!(
                "{}: スキル1 最大 {:+} / 第2スキルなし / 装飾スロット 最大 {}",
                self.name(),
                self.max_first,
                self.max_slots
            ),
        }
    }

    /// The same thing squeezed for a narrow column: 「上限 +10・+13・S3」.
    pub fn short(self) -> alloc::string::String {
        use alloc::format;
        match self.max_points(true) {
            Some(second) => format!("上限 +{}・+{}・S{}", self.max_first, second, self.max_slots),
            None => format!("上限 +{}・第2なし・S{}", self.max_first, self.max_slots),
        }
    }

    /// The ceiling without a label, for a row that already names the kind:
    /// 「スキル1+10 / スキル2+13 / S3」.
    pub fn compact(self) -> alloc::string::String {
        use alloc::format;
        match self.max_points(true) {
            Some(second) => format!(
                "スキル1{:+} / スキル2{:+} / S{}",
                self.max_first, second, self.max_slots
            ),
            None => format!(
                "スキル1{:+} / 第2スキルなし / S{}",
                self.max_first, self.max_slots
            ),
        }
    }
}

/// The ceiling of each kind byte, measured as described on [KindLimits].
///
/// Row 0 is 装備なし and has no ceiling: a record that carries it is not one the
/// game wrote, and [kind_limits] answers None for it.
///
/// The four groups are identical inside themselves on purpose - the game has
/// one table per rarity, not one per kind - so 兵士 and 闘士 share a ceiling,
/// as do 女王 / 王 / 龍 and 英雄 / 伝説 / 天.
pub const KIND_LIMITS: [Option<KindLimits>; 11] = [
    None,
    Some(KindLimits { kind: 1, rarity: 97, max_first: 10, max_second: 0, max_slots: 1, first_skills: 59, second_skills: 0 }),
    Some(KindLimits { kind: 2, rarity: 97, max_first: 10, max_second: 0, max_slots: 1, first_skills: 59, second_skills: 0 }),
    Some(KindLimits { kind: 3, rarity: 98, max_first: 10, max_second: 13, max_slots: 2, first_skills: 71, second_skills: 74 }),
    Some(KindLimits { kind: 4, rarity: 98, max_first: 10, max_second: 13, max_slots: 2, first_skills: 71, second_skills: 74 }),
    Some(KindLimits { kind: 5, rarity: 99, max_first: 8, max_second: 13, max_slots: 3, first_skills: 56, second_skills: 105 }),
    Some(KindLimits { kind: 6, rarity: 99, max_first: 8, max_second: 13, max_slots: 3, first_skills: 56, second_skills: 105 }),
    Some(KindLimits { kind: 7, rarity: 99, max_first: 8, max_second: 13, max_slots: 3, first_skills: 56, second_skills: 105 }),
    Some(KindLimits { kind: 8, rarity: 100, max_first: 10, max_second: 13, max_slots: 3, first_skills: 62, second_skills: 94 }),
    Some(KindLimits { kind: 9, rarity: 100, max_first: 10, max_second: 13, max_slots: 3, first_skills: 62, second_skills: 94 }),
    Some(KindLimits { kind: 10, rarity: 100, max_first: 10, max_second: 13, max_slots: 3, first_skills: 62, second_skills: 94 }),
];

/// The measured ceiling of a kind byte, or None for 装備なし / out of range.
pub fn kind_limits(kind: u8) -> Option<KindLimits> {
    KIND_LIMITS.get(kind as usize).copied().flatten()
}

/// The measured ceiling of a raw record's kind, or None when the record is not
/// a talisman, or carries a kind byte the game does not ship.
pub fn kind_limits_of_record(rec: &[u8]) -> Option<KindLimits> {
    kind_limits(kind_of_record(rec)?)
}

/// The one-line ceiling of a kind, for the two screens that print it.
pub fn kind_limit_line(kind: u8) -> Option<alloc::string::String> {
    kind_limits(kind).map(|l| l.summary())
}

/// The kind the game uses as the face of a rarity, when a caller asks for a
/// grade instead of a kind.
///
/// The rarity groups hold two or three kinds each (97: 兵士/闘士, 98: 騎士/
/// 城塞, 99: 女王/王/龍, 100: 英雄/伝説/天), and the game writes several of
/// them. The one picked here is the highest kind of the group - 闘士 / 城塞 /
/// 龍 / 天 - because a grade on its own cannot say which of the group the
/// record is, and the pick has to be a record the game could have written.
pub fn canonical_kind(rarity: u8) -> Option<u8> {
    (1..=TALISMAN_KIND_COUNT)
        .rev()
        .find(|k| KIND_RARITY[*k as usize] == rarity)
}

/// The kind byte of a raw record, if the record is a talisman with a kind the
/// game ships.
pub fn kind_of_record(rec: &[u8]) -> Option<u8> {
    if rec.len() < CHARM_RECORD || !is_talisman(rec) {
        return None;
    }
    let kind = rec[0x02];
    if is_kind(kind) {
        Some(kind)
    } else {
        None
    }
}

/// The game's name of the kind of the talisman at `index` of a box.
pub fn kind_name_at(box_bytes: &[u8], index: usize) -> Option<&'static str> {
    let o = index.checked_mul(CHARM_RECORD)?;
    kind_name_of_record(box_bytes.get(o..o + CHARM_RECORD)?)
}

/// Every talisman of a box together with the game's name of its kind.
///
/// The list screens print the name in front of the skills, which is the one
/// label the game's box screen and this editor now share.
pub fn decode_box_named(box_bytes: &[u8]) -> Vec<(Charm, &'static str)> {
    decode_box(box_bytes)
        .into_iter()
        .map(|c| {
            let name = kind_name_at(box_bytes, c.index).unwrap_or("—");
            (c, name)
        })
        .collect()
}

/// The game's name of a raw record's kind.
///
/// Every row of the table is named here, row 0 ("装備なし") included: two of
/// the 650 talismans in the sample save carry 0x00 at +0x02 with a rarity of
/// 99, which the table does not explain, and a screen that prints nothing for
/// them would look broken. [kind_of_record] still answers None for them,
/// because they are not a kind the game ships.
pub fn kind_name_of_record(rec: &[u8]) -> Option<&'static str> {
    if rec.len() < CHARM_RECORD || !is_talisman(rec) {
        return None;
    }
    // Row 0 is "装備なし", exactly as the table says; a byte past the table
    // has no name at all.
    TALISMAN_KINDS.get(rec[0x02] as usize).copied()
}

/// Write the kind (and the rarity that belongs to it) into a talisman.
///
/// The two bytes are a linked pair: +0x02 is the kind and +0x12 is the u16
/// item id of the charm the talisman came from, 353..356 (0x161..0x164), whose
/// low byte is the rarity 97..100 and whose high byte is 1. Writing one
/// without the other leaves a record the game never wrote - which is exactly
/// what the old `set_grade` did when it copied the first table row of a
/// rarity (0x08 for rarity 100, i.e. 英雄の護石) instead of the kind the user
/// asked for (0x0A, 天の護石).
pub fn set_kind(box_bytes: &mut [u8], index: usize, kind: u8) -> bool {
    if index >= CHARM_COUNT || box_bytes.len() < CHARM_BOX_SIZE {
        return false;
    }
    let Some(rarity) = kind_rarity(kind) else {
        return false;
    };
    let o = index * CHARM_RECORD;
    if !is_talisman(&box_bytes[o..o + CHARM_RECORD]) {
        return false;
    }
    box_bytes[o + 0x02] = kind;
    box_bytes[o + 0x12] = rarity;
    box_bytes[o + 0x13] = 1;
    true
}

/// Kind of the talisman at (slot, index) of a save.
pub fn charm_kind(save: &Save, slot: usize, index: usize) -> Option<u8> {
    let box_bytes = save.equipment_box(slot)?;
    let o = index.checked_mul(CHARM_RECORD)?;
    kind_of_record(box_bytes.get(o..o + CHARM_RECORD)?)
}

/// A real record of the requested kind to copy, preferring a clean one.
pub fn template_for_kind(box_bytes: &[u8], kind: u8) -> Option<[u8; CHARM_RECORD]> {
    if box_bytes.len() < CHARM_BOX_SIZE || !is_kind(kind) {
        return None;
    }
    let mut fallback: Option<[u8; CHARM_RECORD]> = None;
    for i in 0..CHARM_COUNT {
        let rec = &box_bytes[i * CHARM_RECORD..(i + 1) * CHARM_RECORD];
        if !is_talisman(rec) || rec[0x02] != kind {
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

/// A template for a kind: a real record of that kind when the box holds one,
/// otherwise a clean talisman with the kind's two paired bytes written.
///
/// The second value is true when the pair had to be written. That is allowed
/// now that the pairing is known from the game's own table: the sample save
/// only holds 天の護石 records (kind 10), and without writing the pair no
/// other kind - 龍の護石 included - could be created at all.
pub fn template_for_kind_or_pair(
    box_bytes: &[u8],
    kind: u8,
) -> Option<([u8; CHARM_RECORD], bool)> {
    if let Some(t) = template_for_kind(box_bytes, kind) {
        return Some((t, false));
    }
    let rarity = kind_rarity(kind)?;
    let mut t = template_from_box(box_bytes)?;
    t[0x02] = kind;
    t[0x12] = rarity;
    t[0x13] = 1;
    Some((t, true))
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

    /// The kind this grade falls back to when the box holds no record of it.
    ///
    /// This used to be the *first* row of the rarity table for the grade, which
    /// gave rarity 100 -> 0x08 (英雄の護石) and so renamed every talisman whose
    /// rarity was switched to 天 to 英雄. The canonical kind is the highest of
    /// the group instead: 闘士 (97), 城塞 (98), 龍 (99), 天 (100) - every one of
    /// them a kind the game itself ships.
    pub fn type_byte(self) -> u8 {
        canonical_kind(self.rarity()).unwrap_or(0)
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
    // +0x13 is the high byte of the u16 charm item id at +0x12 (353..356);
    // leaving a stale high byte would name an item that does not exist.
    t[0x13] = 1;
    Some((t, true))
}

/// What about this talisman would the game not have written?
///
/// One short string per problem, empty when the record is ordinary for its
/// grade. Nothing here changes the save: it is for the warning line.
///
/// This answers "is this legal for its grade"; the five shape rules that hold
/// whatever the grade is - no skill, the same skill twice, points without a
/// skill and so on - live in `structure_warnings`, so a caller that wants both
/// asks both.
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
    // The kind named at +0x02 is the finer answer and the one the game's box
    // screen shows, so its measured ceiling is used when the byte is a kind
    // the game ships (the four-rarity table is what lies behind it anyway).
    // A record that carries 装備なし or an unknown byte falls back to the grade
    // it was passed, which is how this function behaved before.
    let limits = kind_limits_of_record(&box_bytes[o..o + CHARM_RECORD]);
    let who = match limits {
        Some(l) => l.name(),
        None => grade.name(),
    };
    let slot_cap = limits
        .map(|l| l.max_slots.min(grade.max_slots()))
        .unwrap_or_else(|| grade.max_slots());
    if box_bytes[o + 0x10] > slot_cap {
        out.push(format!(
            "スロット{}個は{}の上限{}を超えています",
            box_bytes[o + 0x10],
            who,
            slot_cap
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
        let pts = box_bytes[o + pts_at] as i8;
        // The per-skill table is the narrowest answer, so it speaks first; the
        // kind's ceiling only speaks when the table had nothing to say (no
        // gamedata feature, or a skill id past the end of it). Both lines for
        // one value would be noise.
        let mut flagged = false;
        match allowed_points_rarity(grade.rarity(), skill, second) {
            Some(r) if r.min == 0 && r.max == 0 => {
                out.push(format!("{} は{}では付けられません", label, who));
                flagged = true;
            }
            Some(r) => {
                if pts < r.min || pts > r.max {
                    out.push(format!(
                        "{} {} は {}〜{} の範囲外です",
                        label, pts, r.min, r.max
                    ));
                    flagged = true;
                }
            }
            None => {
                // The table has nothing for this skill id. With the gamedata
                // tables compiled in that means the id is past the end of the
                // talisman table (the four tables stop at id 205, while the
                // skill list runs to 306): a skill the game can never roll on a
                // charm. Without the tables nothing is known, so the build
                // stays quiet and the kind's ceiling speaks instead.
                #[cfg(feature = "gamedata")]
                if TalismanGrade::from_rarity(grade.rarity()).is_some() {
                    out.push(format!(
                        "{} のスキルID {} は護石の表に無いため付けられません",
                        label, skill
                    ));
                    flagged = true;
                }
            }
        }
        if flagged {
            continue;
        }
        let Some(l) = limits else { continue };
        match l.max_points(second) {
            Some(cap) if pts > cap => out.push(format!(
                "{} {} は {:+} ですが、{} の上限は {:+} です",
                label,
                skill_label(skill),
                pts,
                l.name(),
                cap
            )),
            None => out.push(format!(
                "{} は{}に設定できません（この種類は第2スキルを持てません）",
                label,
                l.name()
            )),
            _ => {}
        }
    }
    out
}

/// The name of a skill for a warning line, with the raw id as the fallback.
///
/// The table is 1-based and finite: a record can carry an id past its end,
/// which is itself worth naming rather than silently printing nothing.
fn skill_label(id: u8) -> alloc::string::String {
    use alloc::format;
    match skill_name(id) {
        Some(n) => alloc::string::String::from(n),
        None => format!("ID{}", id),
    }
}

/// Structural warnings for one raw record (36 bytes).
///
/// These are the five shapes the game never writes, independent of grade and
/// of the points tables, taken from the reference editor's own checks:
///
/// 1. no skill at all - both skill ids 0
/// 2. the same skill in both slots
/// 3. only the second slot filled (skill 1 empty, skill 2 not)
/// 4. a skill whose points are 0, so it would contribute nothing
/// 5. points left behind with no skill in that slot
///
/// Warning only: nothing here reads past the record or writes to it, and the
/// caller decides what to do with the lines. An empty vector means the record
/// has an ordinary shape.
pub fn structure_warnings_record(rec: &[u8]) -> Vec<alloc::string::String> {
    use alloc::format;
    let mut out = Vec::new();
    if rec.len() < CHARM_RECORD || !is_talisman(rec) {
        return out;
    }
    let skill1 = rec[0x0C];
    let skill2 = rec[0x0D];
    let points1 = rec[0x0E] as i8;
    let points2 = rec[0x0F] as i8;

    // 1. A talisman with nothing on it is not a record the game writes: it
    // would be an empty charm occupying a box slot.
    if skill1 == 0 && skill2 == 0 {
        out.push(alloc::string::String::from(
            "スキルが1つも設定されていません（スキル1もスキル2も空です）",
        ));
    }
    // 2. The two slots are meant to carry different skills.
    if skill1 != 0 && skill1 == skill2 {
        out.push(format!(
            "スキル1とスキル2が同じです（{}）",
            skill_label(skill1)
        ));
    }
    // 3. The second slot filled while the first is empty.
    if skill1 == 0 && skill2 != 0 {
        out.push(format!(
            "スキル1が空なのにスキル2だけ設定されています（{}）",
            skill_label(skill2)
        ));
    }
    for (label, skill, points) in [
        ("スキル1", skill1, points1),
        ("スキル2", skill2, points2),
    ] {
        // 4. A skill with no points does nothing: the game never stores one.
        if skill != 0 && points == 0 {
            out.push(format!(
                "{}（{}）のポイントが0です",
                label,
                skill_label(skill)
            ));
        }
        // 5. Points with no skill are the leftover of a skill that was
        // cleared: the value is kept but means nothing.
        if skill == 0 && points != 0 {
            out.push(format!(
                "{}が空なのにポイントだけ{}入っています",
                label, points
            ));
        }
    }
    out
}

/// Structural warnings for the talisman at (slot, index) of a save.
///
/// The box accessor owns the bounds check, so a slot or index that does not
/// exist (or a record that is not a talisman) answers with no warnings. See
/// structure_warnings_record for the five rules. Nothing is written, and the
/// save is only read.
pub fn structure_warnings(
    save: &Save,
    slot: usize,
    index: usize,
) -> Vec<alloc::string::String> {
    let Some(box_bytes) = save.equipment_box(slot) else {
        return Vec::new();
    };
    let Some(o) = index.checked_mul(CHARM_RECORD) else {
        return Vec::new();
    };
    let Some(end) = o.checked_add(CHARM_RECORD) else {
        return Vec::new();
    };
    let Some(rec) = box_bytes.get(o..end) else {
        return Vec::new();
    };
    structure_warnings_record(rec)
}

/// Clamp the decoration slots to what the record's kind allows.
///
/// The ceiling is the kind's own, read from [KIND_LIMITS] (1 / 2 / 3 / 3), and
/// the grade passed in is kept as a second opinion: the two always agree
/// because the kind decides the rarity, but a caller that hands in a lower
/// grade must not be silently overruled.
pub fn clamp_slots(box_bytes: &mut [u8], index: usize, grade: TalismanGrade) -> bool {
    if index >= CHARM_COUNT || box_bytes.len() < CHARM_BOX_SIZE {
        return false;
    }
    let o = index * CHARM_RECORD;
    if !is_talisman(&box_bytes[o..o + CHARM_RECORD]) {
        return false;
    }
    let max = kind_limits_of_record(&box_bytes[o..o + CHARM_RECORD])
        .map(|l| l.max_slots.min(grade.max_slots()))
        .unwrap_or_else(|| grade.max_slots());
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
    // A kind that already belongs to the target rarity stays: switching a
    // 龍の護石 to the "rarity 99" of its own group must not rename it to the
    // 王の護石 that happens to sit earlier in the box. Only when the kind and
    // the target disagree is another kind of the grade used - a real record's
    // when the box holds one, the group's top kind otherwise.
    let keep = kind_of_record(&box_bytes[o..o + CHARM_RECORD])
        .filter(|k| KIND_RARITY[*k as usize] == grade.rarity());
    let type_byte = keep
        .or_else(|| type_byte_for_grade(box_bytes, grade))
        .unwrap_or_else(|| grade.type_byte());
    box_bytes[o + 0x02] = type_byte;
    box_bytes[o + 0x12] = grade.rarity();
    box_bytes[o + 0x13] = 1;
    true
}

/// Clamp both skill point values to what the grade and the kind allow.
///
/// Returns true when a value had to move. The ceiling of one slot is the
/// smallest of the per-skill table and the kind's own maximum
/// ([KindLimits::max_points]), so a record typed as a 兵士 can never end up
/// holding a second skill's points even where the per-skill table has a row.
///
/// A skill the grade forbids outright is kept and left for [grade_warnings]:
/// silently deleting a skill the user chose is worse than writing it.
pub fn clamp_points(box_bytes: &mut [u8], index: usize, grade: TalismanGrade) -> bool {
    if index >= CHARM_COUNT || box_bytes.len() < CHARM_BOX_SIZE {
        return false;
    }
    let o = index * CHARM_RECORD;
    if !is_talisman(&box_bytes[o..o + CHARM_RECORD]) {
        return false;
    }
    let limits = kind_limits_of_record(&box_bytes[o..o + CHARM_RECORD]);
    let mut moved = false;
    for (skill_at, pts_at, second) in [(0x0Cusize, 0x0Eusize, false), (0x0D, 0x0F, true)] {
        let skill = box_bytes[o + skill_at];
        if skill == 0 {
            continue;
        }
        let cap = limits.and_then(|l| l.max_points(second));
        match allowed_points_rarity(grade.rarity(), skill, second) {
            // "0~0" means the grade does not carry this skill at all. The
            // record is left alone rather than quietly cleared: the reference
            // editor warns about this and still writes, and grade_warnings is
            // how the caller tells them.
            Some(r) if r.min == 0 && r.max == 0 => {}
            Some(r) => {
                let high = match cap {
                    Some(c) => r.max.min(c),
                    None => r.max,
                };
                let pts = box_bytes[o + pts_at] as i8;
                if pts < r.min {
                    box_bytes[o + pts_at] = r.min as u8;
                    moved = true;
                } else if pts > high {
                    box_bytes[o + pts_at] = high as u8;
                    moved = true;
                }
            }
            None => {
                // The table knows nothing about this skill id (it is past the
                // end of its 206 rows), so the grade does not allow it at all.
                // The kind's ceiling is a ceiling, not a permission: a value
                // under it is still a skill the game cannot roll, so this case
                // keeps the old answer and clears the slot. [clamp_points_to_kind]
                // is the non-destructive, table-free clamp when that is wanted.
                box_bytes[o + skill_at] = 0;
                box_bytes[o + pts_at] = 0;
                moved = true;
            }
        }
    }
    moved
}

/// Clamp both point values to the ceiling of the record's own kind alone.
///
/// This is the half of [clamp_points] that needs no per-skill table at all, so
/// it also answers on a build without the `gamedata` feature, and it is what a
/// caller should use when only the kind is trusted. A second skill on a kind
/// that has no second slot (兵士 / 闘士) is left where it is: the kind says it
/// cannot be rolled, but removing it is the user's call, and
/// [grade_warnings] is the line that says so.
pub fn clamp_points_to_kind(box_bytes: &mut [u8], index: usize) -> bool {
    if index >= CHARM_COUNT || box_bytes.len() < CHARM_BOX_SIZE {
        return false;
    }
    let o = index * CHARM_RECORD;
    let Some(limits) = kind_limits_of_record(&box_bytes[o..o + CHARM_RECORD]) else {
        return false;
    };
    let mut moved = false;
    for (skill_at, pts_at, second) in [(0x0Cusize, 0x0Eusize, false), (0x0D, 0x0F, true)] {
        if box_bytes[o + skill_at] == 0 {
            continue;
        }
        let Some(cap) = limits.max_points(second) else {
            continue;
        };
        if (box_bytes[o + pts_at] as i8) > cap {
            box_bytes[o + pts_at] = cap as u8;
            moved = true;
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
        // The template supplies the grade pair, not its gems: this template
        // carries decoration 2856 at +0x06, and a new talisman must not
        // inherit it (it would have more gems than sockets).
        assert_eq!(rec[0x06..0x0C], [0u8; 6], "the decorations are not copied");
        // Everything else the template carried still comes along.
        assert_eq!(rec[0x13], template[0x13], "unknown bytes carry over");
        assert_eq!(rec[0x11], template[0x11]);
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

    // --- the per-kind ceilings (charm::KIND_LIMITS) -----------------------

    /// A talisman of a chosen kind: the record shape the sample save holds,
    /// retyped. The pair at +0x02 / +0x12 is written from the kind table.
    fn record_of_kind(kind: u8) -> [u8; CHARM_RECORD] {
        let mut r = rarity_99_record();
        r[0x02] = kind;
        r[0x12] = kind_rarity(kind).unwrap();
        r[0x13] = 1;
        r
    }

    #[test]
    fn each_kind_carries_the_ceiling_measured_from_the_tables() {
        // 1-2 -> rarity 97: one slot, no second skill at all.
        for kind in [1u8, 2] {
            let l = kind_limits(kind).expect("a shipped kind");
            assert_eq!(l.rarity, 97);
            assert_eq!(l.max_first, 10);
            assert_eq!(l.max_slots, 1);
            assert_eq!(l.max_points(true), None, "rarity 97 has no slot-2 table");
            assert_eq!(l.second_skills, 0);
            assert_eq!(l.first_skills, 59);
        }
        // 3-4 -> 98: two slots, +13 on the second skill.
        for kind in [3u8, 4] {
            let l = kind_limits(kind).unwrap();
            assert_eq!(l.rarity, 98);
            assert_eq!((l.max_first, l.max_second, l.max_slots), (10, 13, 2));
            assert_eq!((l.first_skills, l.second_skills), (71, 74));
        }
        // 5-7 -> 99: the first skill stops at +8, the second reaches +13.
        for kind in [5u8, 6, 7] {
            let l = kind_limits(kind).unwrap();
            assert_eq!(l.rarity, 99);
            assert_eq!((l.max_first, l.max_second, l.max_slots), (8, 13, 3));
            assert_eq!((l.first_skills, l.second_skills), (56, 105));
        }
        // 8-10 -> 100: the skyward talisman, +10 / +13 / three slots.
        for kind in [8u8, 9, 10] {
            let l = kind_limits(kind).unwrap();
            assert_eq!(l.rarity, 100);
            assert_eq!((l.max_first, l.max_second, l.max_slots), (10, 13, 3));
            assert_eq!((l.first_skills, l.second_skills), (62, 94));
        }
        // Row 0 is 装備なし and past the table is not a kind.
        assert!(kind_limits(0).is_none());
        assert!(kind_limits(11).is_none());
        // Every kind agrees with the rarity table it is stored as.
        for kind in 1..=TALISMAN_KIND_COUNT {
            assert_eq!(kind_limits(kind).unwrap().rarity, kind_rarity(kind).unwrap());
        }
    }

    #[test]
    fn the_kind_ceiling_line_names_the_kind_and_its_numbers() {
        let line = kind_limit_line(10).expect("a shipped kind");
        assert!(line.contains("天の護石"), "{}", line);
        assert!(
            line.contains("+10") && line.contains("+13") && line.contains('3'),
            "{}",
            line
        );
        let soldier = kind_limit_line(1).unwrap();
        assert!(soldier.contains("兵士の護石"), "{}", soldier);
        assert!(soldier.contains("第2スキルなし"), "{}", soldier);
        assert!(kind_limit_line(0).is_none());
    }

    #[test]
    fn clamping_to_the_kind_stops_a_sky_talisman_at_ten_and_thirteen() {
        let mut b = alloc::vec![0u8; CHARM_BOX_SIZE];
        let mut r = record_of_kind(10);
        r[0x0C] = 42; // a slot-1 skill the rarity-100 table caps at +10
        r[0x0E] = 12;
        r[0x0D] = 27; // a slot-2 skill it caps at +13
        r[0x0F] = 20;
        r[0x10] = 3;
        b[..CHARM_RECORD].copy_from_slice(&r);

        assert!(clamp_points_to_kind(&mut b, 0));
        assert_eq!(b[0x0E] as i8, 10);
        assert_eq!(b[0x0F] as i8, 13);
        // A value inside the ceiling is left alone, and the record is then
        // ordinary for the kind it carries.
        assert!(!clamp_points_to_kind(&mut b, 0));
        assert!(grade_warnings(&b, 0, TalismanGrade::Enduring).is_empty());
    }

    #[test]
    fn a_soldier_kind_has_one_slot_and_cannot_carry_a_second_skill() {
        let mut b = alloc::vec![0u8; CHARM_BOX_SIZE];
        let mut r = record_of_kind(1);
        r[0x0C] = 1;
        r[0x0E] = 5;
        r[0x0D] = 2;
        r[0x0F] = 5;
        r[0x10] = 3;
        b[..CHARM_RECORD].copy_from_slice(&r);

        // The kind's ceiling never raises anything, and the second skill is
        // kept: removing it is the user's call.
        assert!(!clamp_points_to_kind(&mut b, 0));
        assert_eq!(b[0x0D], 2);
        assert_eq!(b[0x0F] as i8, 5);

        assert!(clamp_slots(&mut b, 0, TalismanGrade::Mystery));
        assert_eq!(b[0x10], 1, "a 兵士 carries one slot");

        let w = grade_warnings(&b, 0, TalismanGrade::Mystery);
        let joined = w.join(" / ");
        assert!(joined.contains("兵士の護石"), "{}", joined);
        assert!(joined.contains("スキル2"), "{}", joined);
    }

    #[test]
    fn a_skill_id_past_the_talisman_table_is_reported() {
        // The four point tables have 206 cells (0..=205) while the skill list
        // runs to 306, so ids above 205 are real skills no talisman can carry.
        let mut b = alloc::vec![0u8; CHARM_BOX_SIZE];
        let mut r = record_of_kind(10);
        r[0x0C] = 210;
        r[0x0E] = 3;
        r[0x0D] = 0;
        r[0x0F] = 0;
        r[0x10] = 1;
        b[..CHARM_RECORD].copy_from_slice(&r);
        let w = grade_warnings(&b, 0, TalismanGrade::Enduring);
        assert!(
            w.iter().any(|line| line.contains("スキルID 210")),
            "{:?}",
            w
        );
    }

    #[test]
    fn a_queen_talisman_stops_its_first_skill_at_eight() {
        let mut b = alloc::vec![0u8; CHARM_BOX_SIZE];
        let mut r = record_of_kind(5);
        r[0x0C] = 16; // the only slot-1 skill amuletSkillData04 caps at 8
        r[0x0E] = 11;
        r[0x0D] = 27;
        r[0x0F] = 13;
        r[0x10] = 3;
        b[..CHARM_RECORD].copy_from_slice(&r);

        assert!(clamp_points_to_kind(&mut b, 0));
        assert_eq!(b[0x0E] as i8, 8, "a 女王 stops at +8 on the first skill");
        assert_eq!(b[0x0F] as i8, 13, "the second skill still reaches +13");
    }
}