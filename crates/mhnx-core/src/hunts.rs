//! Monster hunt records: slain, captured and size - and the crown rule.
//!
//! Every character slot holds three parallel u16 arrays of 137 entries, all
//! **relative to the slot base** (the offsets live in [`crate::save`], which is
//! where the arithmetic below is documented too):
//!
//! ```text
//!     +0x5EA6   slain      u16 per monster        137 x 2 = 274 B
//!     +0x5FB8   captured   u16 per monster        137 x 2 = 274 B
//!     +0x60CA   size       (min %, max %) u16     137 x 4 = 548 B
//! ```
//!
//! Index 0 is the first monster (リオレイア; names in
//! [`crate::gamedata::MONSTER_HUNT_NAMES`], behind the `gamedata` feature). The
//! Switch save editor this layout was cross-checked against numbers the same
//! entries 1..=137 and puts its array bases one entry *before* the first entry,
//! so its `base + 2 * i` with `i` starting at 1 is this module's index 0. Its
//! own crown-count tool reads the size table at `0x60CA + 4 * (index - 1)`,
//! which is `OFF_MONSTER_SIZE + 4 * monster` here: the two agree exactly.
//!
//! ## Why the three arrays mean slain / captured / size
//!
//! * Arithmetic (`save.rs`, `OFF_MONSTER_HUNT` and the `MONSTER_*` comment):
//!   `0x5EA6 + 137*2 = 0x5FB8`, `+ 137*2 = 0x60CA`, `+ 137*4 = 0x62EE` - and
//!   `0x62EE` is exactly where the equipment box starts. Three arrays that abut
//!   with no gaps and end on a known structure boundary are 137 entries of 2, 2
//!   and 4 bytes; that arithmetic, not the roster, is the evidence for the count.
//! * The verified save `C:\anyWorkspace\nx\jksv_ready\system` (slot 0): 129
//!   entries hold a slain count, 47 a capture count, 78 a size record. Every size
//!   record has `min <= max` and lies in 20..=300, which is what the percentages
//!   the game displays look like (the GUI already labels that column サイズ and
//!   limits it to 0..=300, `app.rs` `tab_hunts`).
//! * The 34 small monsters (0-based 71..=104: Aptonoth ... Moofah) carry a slain
//!   count but **no** size record and **no** capture count in that save, and the
//!   only large monsters with a slain count but no size record are exactly the
//!   seven whose size the game fixes (Furious Rajang, Akantor, Ukanlos, Savage
//!   Deviljho, Alatreon, Amatsu, Nakarkos). Small monsters cannot be captured and
//!   a fixed-size monster never gets a size record, so all three arrays agree with
//!   what the game does.
//!
//! `(0, 0)` in the size table means "no specimen recorded", never "0 %": it is
//! what a never-hunted monster and a fixed-size monster both hold.
//!
//! ## Crowns (大小金)
//!
//! The game derives the crown marks from the size record: the **smallest** size
//! gives the mini crown (小金) and the **largest** the gold crown (金冠). The
//! comparison uses the per-monster thresholds the game keeps in its own size
//! table (`em_size_scale_data`: `mSmallScale`, `mBigScale`, `mKingScale`), and a
//! monster whose size is fixed has all three at 0, so any recorded size shows
//! gold.
//!
//! Those numbers are game data taken from the user's own files, so this crate
//! does not ship them - the same rule as the DLC bundle and `questprogress`,
//! where the index tables stay out and callers pass their own. [`crown`] takes
//! the window as an argument: `(mini_le, gold_ge)` for the 大小金 judgement the
//! save editor's audit asked for. The silver crown (銀冠) needs the third
//! threshold `mBigScale` and is deliberately not modelled here.
//!
//! The *rule* is what was verified, in the reference editor
//! (`.work/finalaudit/refsrc`, `app/core/src/monsters.rs` `crowns`): mini from
//! `min <= mini_le`, gold from `max >= gold_ge`. Its evidence tool
//! (`tools/evidence/goals.py`) raised the gold-crown count of a test save from
//! 13 to 79 this way, and the next game session's award check - which reads the
//! size records through the game's own crown function at `0x67310`, per
//! `docs/09-awards.md` - granted all four crown awards. So the comparison, not
//! the per-monster numbers, is the part that belongs in the library.

use crate::save::Save;

/// Number of monsters the three tables cover.
///
/// Equal to [`crate::save::MONSTER_COUNT`]; kept as its own name because this
/// module's index space (0..137) is all a caller needs.
pub const MONSTERS: usize = 137;

/// Highest value the game's counters ever reach.
///
/// The arrays are u16 and would hold 65535, but the game's increment stops at
/// 9999 (the reference implementation locates that increment at `0x526d94`),
/// and the GUI's editors already limit these fields to 0..=9999. Writing past it
/// would be a number the game itself never produces, so the setters clamp.
pub const MAX_COUNT: u16 = 9999;

/// Slain count of one monster, or `None` for a missing slot or monster.
pub fn killed(save: &Save, slot: usize, monster: usize) -> Option<u16> {
    if monster >= MONSTERS {
        return None;
    }
    save.hunt_counts(slot)?.get(monster).copied()
}

/// Capture count of one monster, or `None` for a missing slot or monster.
pub fn captured(save: &Save, slot: usize, monster: usize) -> Option<u16> {
    if monster >= MONSTERS {
        return None;
    }
    save.capture_counts(slot)?.get(monster).copied()
}

/// Smallest and largest size recorded for one monster, as the percentages the
/// game shows. `(0, 0)` means no specimen.
pub fn size_range(save: &Save, slot: usize, monster: usize) -> Option<(u16, u16)> {
    if monster >= MONSTERS {
        return None;
    }
    save.monster_sizes(slot)?.get(monster).copied()
}

/// Set one monster's slain count. Returns false for a missing slot or monster.
pub fn set_killed(save: &mut Save, slot: usize, monster: usize, value: u16) -> bool {
    if monster >= MONSTERS {
        return false;
    }
    save.set_hunt_count(slot, monster, value.min(MAX_COUNT))
}

/// Set one monster's capture count. Returns false for a missing slot or monster.
pub fn set_captured(save: &mut Save, slot: usize, monster: usize, value: u16) -> bool {
    if monster >= MONSTERS {
        return false;
    }
    save.set_capture_count(slot, monster, value.min(MAX_COUNT))
}

/// Set one monster's size record. Returns false for a missing slot or monster.
///
/// The pair is repaired before it is written, because the game only ever holds a
/// whole, ordered pair: one end at 0 means "the only specimen seen" and becomes
/// both ends, and a pair the wrong way round is swapped. `(0, 0)` stays
/// `(0, 0)` - that is how "no specimen" is stored. The reference editor does
/// the same and its test `size_record_is_whole_and_ordered` pins it; the verified
/// save has no record with `min > max`, so the game does not write one either.
pub fn set_size_range(save: &mut Save, slot: usize, monster: usize, range: (u16, u16)) -> bool {
    if monster >= MONSTERS {
        return false;
    }
    let (mut lo, mut hi) = match range {
        (0, x) | (x, 0) => (x, x),
        pair => pair,
    };
    if lo > hi {
        core::mem::swap(&mut lo, &mut hi);
    }
    save.set_monster_size(slot, monster, lo, hi)
}

/// The two crown marks the game shows for one monster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Crown {
    /// 小金 (miniature crown): the smallest specimen reached the mini threshold.
    pub mini: bool,
    /// 金冠 (gold crown): the largest specimen reached the gold threshold.
    pub gold: bool,
}

/// Judge one size percentage against a monster's crown window
/// `(mini_le, gold_ge)`.
///
/// This is the game's comparison with the numbers supplied by the caller (see
/// the module docs for where the window comes from). `size` is one stored
/// percentage, so use [`record_crown`] for a whole record; `size == 0` is "no
/// specimen" and gives no crown. A window of `(0, 0)` is a monster with a fixed
/// size: every stored size is then gold, and nothing is mini - again the game's
/// own rule, because all its thresholds are 0.
pub fn crown(size: u16, range: (u16, u16)) -> Crown {
    let (mini_le, gold_ge) = range;
    if size == 0 {
        return Crown::default();
    }
    Crown {
        mini: mini_le > 0 && size <= mini_le,
        gold: size >= gold_ge,
    }
}

/// Judge a whole size record `(smallest, largest)`: the smallest can earn the
/// mini crown and the largest the gold crown, in one record.
pub fn record_crown(record: (u16, u16), range: (u16, u16)) -> Crown {
    Crown {
        mini: crown(record.0, range).mini,
        gold: crown(record.1, range).gold,
    }
}
