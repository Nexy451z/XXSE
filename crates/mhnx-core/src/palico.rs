//! The player's palico roster.
//!
//! Eighty-four records of 324 bytes at slot + 0x23BB6. The record is also the
//! one the twenty-four palicos for hire use, back to back at slot + 0x2A606
//! (0x23BB6 + 84 * 324 = 0x2A606, and the sample save holds 24 named records
//! there). Each record starts with a UTF-8 name, then the experience at +0x20,
//! a level byte at +0x24 that the game shows as value + 1, the support tendency
//! at +0x25, the enthusiasm byte at +0x26 and the attack tendency at +0x27.
//!
//! From +0x28 the two equipped lists follow - eight support moves and eight
//! skills, 0 = empty, kept packed at the front. +0x38 opens the list of learned
//! support moves (sixteen slots, 57 = empty) and +0x48 the list of learned
//! skills (twelve slots, 96 = empty). Each learned list is followed by the
//! number of slots it uses: +0x55 for moves and +0x57 for skills, with the
//! random point pattern of each list at +0x54 and +0x56.
//!
//! ## Where the layout comes from
//!
//! The same 324 byte record is documented by the reference editor for MHGU
//! (MHGU-Save-editor, app/core/src/palico.rs): learned moves at +0x38 with
//! sixteen slots, learned skills at +0x48 with twelve, 57 and 96 as the empty
//! entries, and the lengths at +0x55 / +0x57. That is the source of the field
//! names here, and the real MHXX save agrees with it record for record:
//!
//! * All 41 used records (17 owned + 24 for hire) satisfy the three properties
//!   a length has to have at once: every non-empty entry sits below the stored
//!   length, every slot between the last entry and the length is 0, and every
//!   slot past the length still carries the 57 / 96 fill. A guessed length
//!   does not fit all three on all 41.
//! * The length counts the slots teaching has not filled yet, so a learned list
//!   reads [skills in use][empty taught slots][57 / 96 fill]: in the sample the
//!   empty tail is two slots on every list that has room, and shorter only when
//!   teaching filled one of them (record 30 has one empty move slot) or the list
//!   is full (records 37 and 41 have no empty skill slot). An empty slot never
//!   precedes a used one in any of the 41 records.
//! * The ids stay inside the name tables: learned skills run 1..=93 and learned
//!   support moves 1..=56, while gamedata::PALICO_SKILL_NAMES ends at 96 and
//!   gamedata::PALICO_SUPPORT_MOVES at 57 - the two values the game uses for
//!   "empty". The equipped lists carry ids that appear in the learned lists,
//!   with one exception the sample really has (record 39, see skill_warnings).
//!
//! The sample character has 17 of the 84 owned records filled.
//!
//! Nothing here writes the lists' lengths: filling a taught slot (the only way
//! the game itself grows a list) leaves the length alone, which is why add_skill
//! and add_move only fill a slot that is already counted. No operation in this
//! module shortens a list - the game has no "forget" and the result would be a
//! record it never writes.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::save::{check_name, NameError, NameFit, NameUnit, Save};

/// Offset of the palico roster, relative to the slot base.
pub const OFF_ROSTER: usize = 0x23BB6;
/// Bytes per record.
pub const RECORD: usize = 324;
/// Records in the roster.
pub const SLOTS: usize = 84;
/// Name field, UTF-8, NUL padded.
pub const NAME: usize = 0x00;
/// Name bytes available.
pub const NAME_LEN: usize = 32;
/// Bytes of the name a palico record can hold: one short of the field, for the
/// NUL the game keeps behind it.
pub const NAME_CAPACITY: usize = NAME_LEN - 1;
/// Experience.
pub const EXP: usize = 0x20;
/// Level, shown as this plus one.
pub const LEVEL: usize = 0x24;
/// Support tendency, eight kinds.
pub const SYSTEM: usize = 0x25;
/// Enthusiasm, which the game calls tension.
pub const ENTHUSIASM: usize = 0x26;
/// Attack tendency.
pub const ATTACK: usize = 0x27;
/// Equipped support moves, eight slots.
pub const MOVES: usize = 0x28;
/// Equipped skills, eight slots.
pub const SKILLS: usize = 0x30;
/// Learned support moves, sixteen slots.
pub const LEARNED_MOVES: usize = 0x38;
/// Learned skills, twelve slots.
pub const LEARNED_SKILLS: usize = 0x48;
/// Slots in the learned support move list.
pub const LEARNED_MOVE_SLOTS: usize = 16;
/// Slots in the learned skill list.
pub const LEARNED_SKILL_SLOTS: usize = 12;
/// Slots in the equipped support move list.
pub const MOVE_SLOTS: usize = 8;
/// Slots in the equipped skill list.
pub const SKILL_SLOTS: usize = 8;
/// Point pattern of the learned support moves.
pub const MOVE_PATTERN: usize = 0x54;
/// Slots the learned support move list uses (0..=16).
pub const MOVE_LEN: usize = 0x55;
/// Point pattern of the learned skills.
pub const SKILL_PATTERN: usize = 0x56;
/// Slots the learned skill list uses (0..=12).
pub const SKILL_LEN: usize = 0x57;
/// Empty entry in a learned move list, one past the move table.
pub const MOVE_EMPTY: u8 = 57;
/// Empty entry in a learned skill list, one past the skill table.
pub const SKILL_EMPTY: u8 = 96;
/// Highest skill id that names a skill; 0 is "no skill".
pub const MAX_SKILL: u8 = SKILL_EMPTY - 1;
/// Highest support move id; 0 is "no move".
pub const MAX_MOVE: u8 = MOVE_EMPTY - 1;
/// Support tendencies the game offers.
pub const SYSTEMS_COUNT: u8 = 8;
/// Highest attack tendency the game offers (0 = none, 1..=5 in the sample).
pub const MAX_ATTACK: u8 = 5;
/// Equipment indices, weapon, head and body.
pub const EQUIP: usize = 0x100;
/// Equipment slots.
pub const EQUIP_SLOTS: usize = 3;

// The two learned lists have to end before the length bytes that follow them,
// and the length bytes have to be inside the record.
const _: () = assert!(LEARNED_MOVES + LEARNED_MOVE_SLOTS == LEARNED_SKILLS);
const _: () = assert!(LEARNED_SKILLS + LEARNED_SKILL_SLOTS == MOVE_PATTERN);
const _: () = assert!(MOVE_PATTERN + 1 == MOVE_LEN);
const _: () = assert!(MOVE_LEN + 1 == SKILL_PATTERN);
const _: () = assert!(SKILL_PATTERN + 1 == SKILL_LEN);
const _: () = assert!(SKILL_LEN < RECORD);

/// The eight support tendencies, in the order the game stores them.
pub const SYSTEMS: [&str; 8] = [
    "カリスマ",
    "ファイト",
    "ガード",
    "アシスト",
    "ヒーリング",
    "ボマー",
    "コレクト",
    "ビースト",
];

/// Name of a support tendency.
pub fn system_name(id: u8) -> &'static str {
    SYSTEMS.get(id as usize).copied().unwrap_or("?")
}

/// Name of an otomon skill, or a placeholder.
pub fn skill_name(id: u8) -> String {
    #[cfg(feature = "gamedata")]
    {
        crate::gamedata::PALICO_SKILL_NAMES
            .get(id as usize)
            .map(|s| alloc::string::ToString::to_string(*s))
            .unwrap_or_else(|| format!("#{}", id))
    }
    #[cfg(not(feature = "gamedata"))]
    {
        format!("#{}", id)
    }
}

/// Name of a support move, or a placeholder.
///
/// The table is the English one the game data bundle carries; a move id past
/// its end is shown as "#id" rather than guessed at.
pub fn move_name(id: u8) -> String {
    #[cfg(feature = "gamedata")]
    {
        crate::gamedata::PALICO_SUPPORT_MOVES
            .get(id as usize)
            .map(|s| alloc::string::ToString::to_string(*s))
            .unwrap_or_else(|| format!("#{}", id))
    }
    #[cfg(not(feature = "gamedata"))]
    {
        format!("#{}", id)
    }
}

/// One palico.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palico {
    /// Record index in the roster.
    pub index: usize,
    /// Name as stored.
    pub name: String,
    /// Level as the game shows it.
    pub level: u16,
    /// Experience points.
    pub exp: u32,
    /// Enthusiasm.
    pub enthusiasm: u8,
    /// Support tendency.
    pub system: u8,
    /// Attack tendency.
    pub attack: u8,
    /// Equipped support moves.
    pub moves: Vec<u8>,
    /// Equipped skills.
    pub skills: Vec<u8>,
    /// Learned support moves, as stored.
    pub learned_moves: Vec<u8>,
    /// Learned skills, as stored.
    pub learned_skills: Vec<u8>,
    /// Slots the learned support move list uses (+0x55).
    pub move_len: u8,
    /// Slots the learned skill list uses (+0x57).
    pub skill_len: u8,
    /// Equipment box indices, 0xFFFF when nothing is equipped.
    pub equip: [u16; EQUIP_SLOTS],
}

impl Palico {
    /// A record with no name is an empty slot.
    pub fn is_empty(&self) -> bool {
        self.name.is_empty()
    }
    /// Support tendency as an id.
    pub fn system(&self) -> u8 {
        self.system
    }

    /// Support tendency as a name.
    pub fn system_name(&self) -> &'static str {
        system_name(self.system)
    }

    /// Equipped skills, empty slots omitted.
    pub fn skills(&self) -> Vec<u8> {
        self.skills
            .iter()
            .copied()
            .filter(|v| *v != 0 && *v != SKILL_EMPTY)
            .collect()
    }

    /// Learned skills, empty slots omitted.
    ///
    /// Only the slots the record says the list uses are read: everything past
    /// +0x57 is the 96 fill the game leaves there.
    pub fn learned_skills(&self) -> Vec<u8> {
        let used = (self.skill_len as usize).min(LEARNED_SKILL_SLOTS);
        self.learned_skills
            .iter()
            .take(used)
            .copied()
            .filter(|v| *v != 0 && *v != SKILL_EMPTY)
            .collect()
    }

    /// Slots the learned skill list uses (+0x57), clamped to the array.
    pub fn learned_skill_slots(&self) -> usize {
        (self.skill_len as usize).min(LEARNED_SKILL_SLOTS)
    }

    /// Equipped support moves, empty slots omitted.
    pub fn moves(&self) -> Vec<u8> {
        self.moves
            .iter()
            .copied()
            .filter(|v| *v != 0 && *v != MOVE_EMPTY)
            .collect()
    }

    /// Learned support moves, empty slots omitted.
    ///
    /// Like learned_skills, only the slots the record says the list uses are
    /// read.
    pub fn learned_moves(&self) -> Vec<u8> {
        let used = (self.move_len as usize).min(LEARNED_MOVE_SLOTS);
        self.learned_moves
            .iter()
            .take(used)
            .copied()
            .filter(|v| *v != 0 && *v != MOVE_EMPTY)
            .collect()
    }

    /// Slots the learned support move list uses (+0x55), clamped to the array.
    pub fn learned_move_slots(&self) -> usize {
        (self.move_len as usize).min(LEARNED_MOVE_SLOTS)
    }

    /// Names of the equipped skills.
    pub fn skill_names(&self) -> Vec<String> {
        self.skills().into_iter().map(skill_name).collect()
    }

    /// The palico name split into the lines the screen draws.
    ///
    /// The game draws U+000A in this field as a second line, so a name holding
    /// one is still a valid name - the roster shows both lines.
    pub fn name_lines(&self) -> Vec<&str> {
        name_lines(&self.name)
    }

    /// How many lines the palico name takes on screen.
    pub fn name_line_count(&self) -> usize {
        name_line_count(&self.name)
    }

    /// What writing this name would do to the record's field.
    pub fn name_fit(&self) -> Result<NameFit, NameError> {
        name_check(&self.name)
    }
}

/// The learned skills of a palico, in list order.
///
/// The ids are one byte in the record and inside the 96 entry skill table
/// (gamedata::PALICO_SKILL_NAMES), so they stay u8.
pub fn learned_skills(p: &Palico) -> Vec<u8> {
    p.learned_skills()
}

/// The learned support moves of a palico, in list order.
pub fn support_moves(p: &Palico) -> Vec<u8> {
    p.learned_moves()
}

/// The equipped skills of a palico, in slot order.
pub fn equipped_skills(p: &Palico) -> Vec<u8> {
    p.skills()
}

/// The equipped support moves of a palico, in slot order.
pub fn equipped_moves(p: &Palico) -> Vec<u8> {
    p.moves()
}

/// The support tendency of a palico, or None when the byte is not one the game
/// offers.
pub fn type_of(p: &Palico) -> Option<u8> {
    (p.system < SYSTEMS_COUNT).then_some(p.system)
}

/// The attack tendency of a palico, or None when the byte is out of range.
pub fn attack_of(p: &Palico) -> Option<u8> {
    (p.attack <= MAX_ATTACK).then_some(p.attack)
}

fn read_name(rec: &[u8]) -> String {
    let raw = &rec[NAME..NAME + NAME_LEN];
    let end = raw.iter().position(|b| *b == 0).unwrap_or(NAME_LEN);
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

fn decode(index: usize, rec: &[u8]) -> Palico {
    Palico {
        index,
        name: read_name(rec),
        level: rec[LEVEL] as u16 + 1,
        exp: u32::from_le_bytes([rec[EXP], rec[EXP + 1], rec[EXP + 2], rec[EXP + 3]]),
        enthusiasm: rec[ENTHUSIASM],
        system: rec[SYSTEM],
        attack: rec[ATTACK],
        moves: rec[MOVES..MOVES + MOVE_SLOTS].to_vec(),
        skills: rec[SKILLS..SKILLS + SKILL_SLOTS].to_vec(),
        learned_moves: rec[LEARNED_MOVES..LEARNED_MOVES + LEARNED_MOVE_SLOTS].to_vec(),
        learned_skills: rec[LEARNED_SKILLS..LEARNED_SKILLS + LEARNED_SKILL_SLOTS].to_vec(),
        move_len: rec[MOVE_LEN],
        skill_len: rec[SKILL_LEN],
        equip: [
            u16::from_le_bytes([rec[EQUIP], rec[EQUIP + 1]]),
            u16::from_le_bytes([rec[EQUIP + 2], rec[EQUIP + 3]]),
            u16::from_le_bytes([rec[EQUIP + 4], rec[EQUIP + 5]]),
        ],
    }
}

/// Read the roster of one character.
pub fn roster(save: &Save, slot: usize) -> Vec<Palico> {
    let mut out = Vec::new();
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return out;
    };
    for index in 0..SLOTS {
        let at = base + OFF_ROSTER + index * RECORD;
        let Some(rec) = save.body().get(at..at + RECORD) else {
            break;
        };
        out.push(decode(index, rec));
    }
    out
}

/// The palicos that actually exist.
pub fn used(save: &Save, slot: usize) -> Vec<Palico> {
    roster(save, slot)
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect()
}

/// Body offset of one roster record, when the slot and the record both exist.
fn record_at(save: &Save, slot: usize, palico: usize) -> Option<usize> {
    if palico >= SLOTS {
        return None;
    }
    let base = save.slot_base(slot)? as usize;
    let at = base + OFF_ROSTER + palico * RECORD;
    (at + RECORD <= save.body().len()).then_some(at)
}

/// Slots the learned list at len_off uses, clamped to cap.
fn list_len(save: &Save, at: usize, len_off: usize, cap: usize) -> usize {
    (save.body()[at + len_off] as usize).min(cap)
}

/// Drop id from the equipped list at list_off, keeping it packed at the front
/// the way the game holds it (0 = empty).
///
/// The reference editor's in-game confirmed behaviour is that clearing a skill
/// or move out of the learned list also unequips it, so the two lists never
/// disagree because of an edit made here.
fn unequip(save: &mut Save, at: usize, list_off: usize, slots: usize, id: u8) {
    let list = &mut save.body_mut()[at + list_off..at + list_off + slots];
    if let Some(k) = list.iter().position(|v| *v == id) {
        list[k..].rotate_left(1);
        if let Some(last) = list.last_mut() {
            *last = 0;
        }
    }
}

/// Does the learned list still hold id somewhere other than skip?
fn still_learned(save: &Save, at: usize, list_off: usize, len: usize, skip: usize, id: u8) -> bool {
    let list = &save.body()[at + list_off..at + list_off + len];
    list.iter().enumerate().any(|(k, v)| k != skip && *v == id)
}

/// Set a learned support move at index (below the list's length).
///
/// A move that is replaced out of the list is unequipped if it was equipped,
/// which is what the game does when it clears a slot. Writing an id that is
/// already in the list is allowed - the game does not deduplicate either - so
/// callers that care can ask move_warnings afterwards.
pub fn set_move(save: &mut Save, slot: usize, palico: usize, index: usize, id: u8) -> bool {
    if id == 0 || id > MAX_MOVE {
        return false;
    }
    let Some(at) = record_at(save, slot, palico) else {
        return false;
    };
    let len = list_len(save, at, MOVE_LEN, LEARNED_MOVE_SLOTS);
    if index >= len {
        return false;
    }
    let old = save.body()[at + LEARNED_MOVES + index];
    if old == id {
        return true;
    }
    save.body_mut()[at + LEARNED_MOVES + index] = id;
    if old != 0 && old != MOVE_EMPTY && !still_learned(save, at, LEARNED_MOVES, len, index, old) {
        unequip(save, at, MOVES, MOVE_SLOTS, old);
    }
    true
}

/// Teach a support move, filling the first empty slot the list already counts.
///
/// Returns the slot written. The length byte is deliberately left alone: the
/// empty slots in a learned list are the taught ones, so filling one is exactly
/// what the game does and nothing outside the list has to change. None means
/// the move is already learned or the list has no room left.
pub fn add_move(save: &mut Save, slot: usize, palico: usize, id: u8) -> Option<usize> {
    if id == 0 || id > MAX_MOVE {
        return None;
    }
    let at = record_at(save, slot, palico)?;
    let len = list_len(save, at, MOVE_LEN, LEARNED_MOVE_SLOTS);
    let list = &save.body()[at + LEARNED_MOVES..at + LEARNED_MOVES + len];
    if list.contains(&id) {
        return None;
    }
    let k = list.iter().position(|v| *v == 0 || *v == MOVE_EMPTY)?;
    save.body_mut()[at + LEARNED_MOVES + k] = id;
    Some(k)
}

/// Set a learned skill at index (below the list's length).
///
/// Same rule as set_move: if the skill being replaced was equipped it is
/// unequipped, and the list's length does not move.
pub fn set_skill(save: &mut Save, slot: usize, palico: usize, index: usize, id: u8) -> bool {
    if id == 0 || id > MAX_SKILL {
        return false;
    }
    let Some(at) = record_at(save, slot, palico) else {
        return false;
    };
    let len = list_len(save, at, SKILL_LEN, LEARNED_SKILL_SLOTS);
    if index >= len {
        return false;
    }
    let old = save.body()[at + LEARNED_SKILLS + index];
    if old == id {
        return true;
    }
    save.body_mut()[at + LEARNED_SKILLS + index] = id;
    if old != 0 && old != SKILL_EMPTY && !still_learned(save, at, LEARNED_SKILLS, len, index, old) {
        unequip(save, at, SKILLS, SKILL_SLOTS, old);
    }
    true
}

/// Teach a skill, filling the first empty slot the list already counts.
///
/// Returns the slot written, or None when it is already learned or the list has
/// no empty taught slot left. The length byte stays where it is, see add_move.
pub fn add_skill(save: &mut Save, slot: usize, palico: usize, id: u8) -> Option<usize> {
    if id == 0 || id > MAX_SKILL {
        return None;
    }
    let at = record_at(save, slot, palico)?;
    let len = list_len(save, at, SKILL_LEN, LEARNED_SKILL_SLOTS);
    let list = &save.body()[at + LEARNED_SKILLS..at + LEARNED_SKILLS + len];
    if list.contains(&id) {
        return None;
    }
    let k = list.iter().position(|v| *v == 0 || *v == SKILL_EMPTY)?;
    save.body_mut()[at + LEARNED_SKILLS + k] = id;
    Some(k)
}

/// Set a palico's support tendency.
pub fn set_type(save: &mut Save, slot: usize, palico: usize, system: u8) -> bool {
    if system >= SYSTEMS_COUNT {
        return false;
    }
    let Some(at) = record_at(save, slot, palico) else {
        return false;
    };
    save.body_mut()[at + SYSTEM] = system;
    true
}

/// Set a palico's attack tendency.
pub fn set_attack(save: &mut Save, slot: usize, palico: usize, attack: u8) -> bool {
    if attack > MAX_ATTACK {
        return false;
    }
    let Some(at) = record_at(save, slot, palico) else {
        return false;
    };
    save.body_mut()[at + ATTACK] = attack;
    true
}

/// Ids a learned list holds, in list order, with the empty tail dropped.
fn learned_ids(rec: &[u8], list_off: usize, len: usize, empty: u8) -> Vec<u8> {
    rec[list_off..list_off + len]
        .iter()
        .copied()
        .filter(|v| *v != 0 && *v != empty)
        .collect()
}

/// The equipped ids of one list, in slot order, 0 dropped.
fn equipped_ids(rec: &[u8], list_off: usize, slots: usize) -> Vec<u8> {
    rec[list_off..list_off + slots]
        .iter()
        .copied()
        .filter(|v| *v != 0)
        .collect()
}

/// Warnings about a palico's skills, one short line per problem.
///
/// Nothing here changes the save and nothing is refused because of it: the
/// rules below are the ones the sample save itself breaks (record 39 equips
/// skills and moves it does not know), and an editor has to be able to show
/// that rather than block on it.
///
/// Checked: a learned list longer than its array, a duplicate learned id, an
/// empty slot in front of a used one, an equipped skill the list does not hold,
/// a duplicate equipped skill, and an equipped list with a hole in it.
///
/// The level based limits the game applies (how many support moves a level
/// allows, how many skill slots it gives) are *not* checked: this project has
/// no MHXX level table, so a warning here would be a guess.
pub fn skill_warnings(save: &Save, slot: usize, palico: usize) -> Vec<String> {
    let mut out = Vec::new();
    let Some(at) = record_at(save, slot, palico) else {
        return out;
    };
    let rec = &save.body()[at..at + RECORD];
    if rec[NAME] == 0 {
        return out;
    }
    let stored = rec[SKILL_LEN] as usize;
    if stored > LEARNED_SKILL_SLOTS {
        out.push(format!(
            "修得スキルの長さ {} が上限 {} を超えています",
            stored, LEARNED_SKILL_SLOTS
        ));
    }
    let len = stored.min(LEARNED_SKILL_SLOTS);
    let learned = learned_ids(rec, LEARNED_SKILLS, len, SKILL_EMPTY);
    for (k, id) in learned.iter().enumerate() {
        if learned[..k].contains(id) {
            out.push(format!("修得スキル {} が重複しています", skill_name(*id)));
        }
    }
    for (k, v) in rec[LEARNED_SKILLS..LEARNED_SKILLS + len].iter().enumerate() {
        if (*v == 0 || *v == SKILL_EMPTY)
            && rec[LEARNED_SKILLS + k..LEARNED_SKILLS + len]
                .iter()
                .any(|x| *x != 0 && *x != SKILL_EMPTY)
        {
            out.push(format!(
                "修得スキルの {} 番目が空ですが、その後ろに修得済みがあります",
                k
            ));
            break;
        }
    }
    let equipped = equipped_ids(rec, SKILLS, SKILL_SLOTS);
    for (k, id) in equipped.iter().enumerate() {
        if !learned.contains(id) {
            out.push(format!("装備スキル {} は修得していません", skill_name(*id)));
        }
        if equipped[..k].contains(id) {
            out.push(format!("装備スキル {} が重複しています", skill_name(*id)));
        }
    }
    let raw = &rec[SKILLS..SKILLS + SKILL_SLOTS];
    if raw
        .iter()
        .skip(1)
        .enumerate()
        .any(|(k, v)| raw[k] == 0 && *v != 0)
    {
        out.push(alloc::string::String::from("装備スキルが前詰めされていません"));
    }
    out
}

/// Warnings about a palico's support moves, the same rules as skill_warnings
/// applied to the move lists and the move table.
pub fn move_warnings(save: &Save, slot: usize, palico: usize) -> Vec<String> {
    let mut out = Vec::new();
    let Some(at) = record_at(save, slot, palico) else {
        return out;
    };
    let rec = &save.body()[at..at + RECORD];
    if rec[NAME] == 0 {
        return out;
    }
    let stored = rec[MOVE_LEN] as usize;
    if stored > LEARNED_MOVE_SLOTS {
        out.push(format!(
            "修得サポート行動の長さ {} が上限 {} を超えています",
            stored, LEARNED_MOVE_SLOTS
        ));
    }
    let len = stored.min(LEARNED_MOVE_SLOTS);
    let learned = learned_ids(rec, LEARNED_MOVES, len, MOVE_EMPTY);
    for (k, id) in learned.iter().enumerate() {
        if learned[..k].contains(id) {
            out.push(format!(
                "修得サポート行動 {} が重複しています",
                move_name(*id)
            ));
        }
    }
    for (k, v) in rec[LEARNED_MOVES..LEARNED_MOVES + len].iter().enumerate() {
        if (*v == 0 || *v == MOVE_EMPTY)
            && rec[LEARNED_MOVES + k..LEARNED_MOVES + len]
                .iter()
                .any(|x| *x != 0 && *x != MOVE_EMPTY)
        {
            out.push(format!(
                "修得サポート行動の {} 番目が空ですが、その後ろに修得済みがあります",
                k
            ));
            break;
        }
    }
    let equipped = equipped_ids(rec, MOVES, MOVE_SLOTS);
    for (k, id) in equipped.iter().enumerate() {
        if !learned.contains(id) {
            out.push(format!(
                "装備サポート行動 {} は修得していません",
                move_name(*id)
            ));
        }
        if equipped[..k].contains(id) {
            out.push(format!(
                "装備サポート行動 {} が重複しています",
                move_name(*id)
            ));
        }
    }
    let raw = &rec[MOVES..MOVES + MOVE_SLOTS];
    if raw
        .iter()
        .skip(1)
        .enumerate()
        .any(|(k, v)| raw[k] == 0 && *v != 0)
    {
        out.push(alloc::string::String::from("装備サポート行動が前詰めされていません"));
    }
    out
}

/// Check a palico name against the record's field, without writing.
///
/// A line break is allowed (the game draws it as a second line); any other
/// control character is refused with [`NameError::Control`].
pub fn name_check(name: &str) -> Result<NameFit, NameError> {
    check_name(name, NAME_CAPACITY, NameUnit::Bytes)
}

/// The lines a palico name holds, as the screen draws them.
pub fn name_lines(name: &str) -> Vec<&str> {
    crate::save::name_lines(name)
}

/// How many lines a palico name holds.
pub fn name_line_count(name: &str) -> usize {
    crate::save::name_line_count(name)
}

/// Rename a palico, refusing control characters.
///
/// The checked form of [`set_name`]: the name is verified before the record is
/// touched, so a tab or a NUL leaves the roster exactly as it was. A line break
/// is written as it is and reported with the warnings
/// [`check_name`] finds (leading, trailing, doubled), never repaired.
pub fn set_name_checked(
    save: &mut Save,
    slot: usize,
    index: usize,
    name: &str,
) -> Result<NameFit, NameError> {
    let fit = name_check(name)?;
    if index >= SLOTS {
        return Err(NameError::NoField);
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return Err(NameError::NoField);
    };
    let at = base + OFF_ROSTER + index * RECORD + NAME;
    let Some(dst) = save.body_mut().get_mut(at..at + NAME_LEN) else {
        return Err(NameError::NoField);
    };
    crate::write_name_field(dst, name);
    Ok(fit)
}

/// Rename a palico. The name field is fixed width and NUL padded.
///
/// A line break is written as it stands; any other control character is
/// refused, so false means the roster is untouched. The name field is 31 bytes
/// wide and one short of its 32 byte record, so a longer name is cut on a
/// character boundary. [`set_name_checked`] reports the same write with the
/// warnings a caller may want to show.
pub fn set_name(save: &mut Save, slot: usize, index: usize, name: &str) -> bool {
    if index >= SLOTS || name_check(name).is_err() {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_ROSTER + index * RECORD + NAME;
    let Some(dst) = save.body_mut().get_mut(at..at + NAME_LEN) else {
        return false;
    };
    crate::write_name_field(dst, name);
    true
}

/// Set a palico's experience.
pub fn set_exp(save: &mut Save, slot: usize, index: usize, exp: u32) -> bool {
    if index >= SLOTS {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_ROSTER + index * RECORD + EXP;
    let Some(dst) = save.body_mut().get_mut(at..at + 4) else {
        return false;
    };
    dst.copy_from_slice(&exp.to_le_bytes());
    true
}

/// Set a palico's level, stored as one less than the number shown.
pub fn set_level(save: &mut Save, slot: usize, index: usize, level: u16) -> bool {
    if index >= SLOTS || !(1..=99).contains(&level) {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_ROSTER + index * RECORD + LEVEL;
    let Some(dst) = save.body_mut().get_mut(at) else {
        return false;
    };
    *dst = (level - 1) as u8;
    true
}

/// Set a palico's enthusiasm.
pub fn set_enthusiasm(save: &mut Save, slot: usize, index: usize, value: u8) -> bool {
    if index >= SLOTS {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let at = base + OFF_ROSTER + index * RECORD + ENTHUSIASM;
    let Some(dst) = save.body_mut().get_mut(at) else {
        return false;
    };
    *dst = value;
    true
}
