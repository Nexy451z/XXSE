//! Adding, replacing and clearing equipment records.
//!
//! The three operations that are not a plain field edit. Each one has to ask
//! the reference checker first: the seven worn items and forty saved sets hold
//! box indices, and clearing a record that one of them still names leaves a
//! dangling index that this tool cannot repair.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::charm;
use crate::equip;
use crate::equiprefs::{self, EquipRef};
use crate::save::Save;

/// Why an equipment change was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquipError {
    /// The slot number is not 0..3 or the record is past the end of the box.
    OutOfRange,
    /// The record is already blank.
    AlreadyBlank,
    /// The record is still named by something. Carries where.
    Referenced(Vec<EquipRef>),
    /// No blank record is left in the box.
    BoxFull,
    /// A kind change asked for type 0, which is "empty".
    ///
    /// The type field's 0 means the slot holds nothing, so "make this record a
    /// 0" is a delete in disguise; `clear` is the operation for that, and it
    /// asks the reference checker first.
    KindIsBlank,
    /// The kind is past the end of the table (see equip::MAX_KIND).
    UnknownKind(u8),
    /// A talisman was asked for without a kind code the game ships.
    ///
    /// The byte at +0x02 of a talisman is not an item id like a weapon's: it
    /// is the 護石 grade, 1 兵士 .. 10 天 (charm::TALISMAN_KINDS). A record of
    /// type 6 carrying anything else is a record the game never writes.
    BadCharmKind,
}

impl EquipError {
    /// One line for the user.
    pub fn text(&self) -> alloc::string::String {
        match self {
            EquipError::OutOfRange => alloc::string::String::from("枠が範囲外です"),
            EquipError::AlreadyBlank => alloc::string::String::from("すでに空き枠です"),
            EquipError::Referenced(refs) => {
                let mut s = alloc::string::String::from("使用中のため削除できません: ");
                for (i, r) in refs.iter().take(3).enumerate() {
                    if i > 0 {
                        s += "、";
                    }
                    s += &r.text();
                }
                if refs.len() > 3 {
                    s += &format!(" ほか{}件", refs.len() - 3);
                }
                s
            }
            EquipError::BoxFull => alloc::string::String::from("空き枠がありません"),
            EquipError::KindIsBlank => alloc::string::String::from(
                "種別 0（空き）にはできません。枠を空にするときは「この枠を削除」を使ってください",
            ),
            EquipError::UnknownKind(k) => format!(
                "種別 {} は装備の種別表にありません（1〜{}）",
                k,
                equip::MAX_KIND
            ),
            EquipError::BadCharmKind => alloc::string::String::from(
                "お守りにするときは護石の種類（1 兵士 〜 10 天）を選んでください",
            ),
        }
    }
}

/// Clear a record, refusing while anything still points at it.
///
/// `force` is for the caller that has shown the references and been told to go
/// ahead anyway.
pub fn clear(
    save: &mut Save,
    slot: usize,
    index: usize,
    force: bool,
) -> Result<(), EquipError> {
    let Some(box_bytes) = save.equipment_box(slot) else {
        return Err(EquipError::OutOfRange);
    };
    let o = index * equip::EQUIP_RECORD;
    let Some(rec) = box_bytes.get(o..o + equip::EQUIP_RECORD) else {
        return Err(EquipError::OutOfRange);
    };
    if equip::record_is_blank(rec) {
        return Err(EquipError::AlreadyBlank);
    }
    let refs = equiprefs::references(save, slot, index);
    if !refs.is_empty() && !force {
        return Err(EquipError::Referenced(refs));
    }
    let Some(target) = save.equipment_record_mut(slot, index) else {
        return Err(EquipError::OutOfRange);
    };
    equip::clear(target);
    Ok(())
}

/// True when `id` can be the kind code of a talisman record.
///
/// The byte at +0x02 of a type 6 record is not a weapon style id: it is the
/// 護石 grade, 1 兵士 .. 10 天 (charm::TALISMAN_KINDS), which is why the same
/// number is called the id here and the kind in charm.rs. A value past the
/// table would make a record the game never wrote, so it is refused rather
/// than written.
fn is_charm_code(id: u16) -> bool {
    u8::try_from(id).map(charm::is_kind).unwrap_or(false)
}

/// Write the two bytes a talisman record keeps as a linked pair.
///
/// +0x02 is the kind code and +0x12/+0x13 the u16 item id of the 護石 the
/// record came from (353..356: rarity 97..100 in the low byte, 1 in the high
/// one). [charm::set_kind] is the only writer of those three bytes in this
/// crate, and it is used here rather than a new offset: a record of type 6
/// whose pair disagrees is a record the game never writes, which is why a
/// weapon converted to a talisman needs this written on top of the type byte.
fn write_charm_pair(save: &mut Save, slot: usize, index: usize, code: u8) -> bool {
    match save.equipment_box_mut(slot) {
        Some(box_bytes) => charm::set_kind(box_bytes, index, code),
        None => false,
    }
}

/// Put a different item in an existing record.
///
/// The type and the id are what change; the level is kept unless a new one is
/// given, and the three decoration ids are emptied because they belonged to
/// the item that was there. The tail at +0x0C is left alone: it belongs to the
/// type, and a record can carry data its type does not explain.
///
/// A talisman is the one type that needs more than the type byte: `id` is its
/// kind code there, and the two bytes at +0x12/+0x13 have to name the same
/// rarity, so the pair goes through [charm::set_kind]. An `id` the eleven-row
/// kind table does not carry (0, 11 and up) still writes - the box holds two
/// such talismans and the CSV round trip has to be able to put them back - but
/// gets no pair, and [kind_warnings] reports it. [change_kind] is the
/// operation that asks for a kind, so *it* refuses a code the table lacks.
pub fn replace(
    save: &mut Save,
    slot: usize,
    index: usize,
    kind: u8,
    id: u16,
    level: u16,
) -> Result<(), EquipError> {
    check_kind(kind)?;
    // The id of a type 6 record is its 護石 kind code, but it is written as
    // given: the sample save holds two talismans whose code is 0, the CSV
    // export/import has to be able to put those records back, and the id row
    // of the form takes any number. A code the kind table does not carry gets
    // no pair written (there is no rarity to pair it with) and is reported by
    // [kind_warnings] rather than refused.
    let Some(box_bytes) = save.equipment_box(slot) else {
        return Err(EquipError::OutOfRange);
    };
    let o = index * equip::EQUIP_RECORD;
    let Some(rec) = box_bytes.get(o..o + equip::EQUIP_RECORD) else {
        return Err(EquipError::OutOfRange);
    };
    if equip::record_is_blank(rec) {
        // Replacing a blank record is what create() is for.
        return Err(EquipError::AlreadyBlank);
    }
    let Some(target) = save.equipment_record_mut(slot, index) else {
        return Err(EquipError::OutOfRange);
    };
    let mut head = u16::from_le_bytes([target[0], target[1]]);
    head &= !0x1F;
    head |= (kind as u16) & 0x1F;
    target[0..2].copy_from_slice(&head.to_le_bytes());
    target[2..4].copy_from_slice(&id.to_le_bytes());
    for socket in 0..3 {
        equip::set_decoration(target, socket, 0);
    }
    equip::set_level(target, level);
    if kind == charm::CHARM_TYPE && is_charm_code(id) && !write_charm_pair(save, slot, index, id as u8)
    {
        return Err(EquipError::BadCharmKind);
    }
    Ok(())
}

/// Write a new item into the first blank record.
///
/// Returns the index used. The tail is zeroed because the record was blank to
/// begin with, so there is nothing there to preserve. A talisman is written
/// through [charm::set_kind] like [replace] does, so a charm created here is
/// one the game could have written.
pub fn create(
    save: &mut Save,
    slot: usize,
    kind: u8,
    id: u16,
    level: u16,
) -> Result<usize, EquipError> {
    check_kind(kind)?;
    // See replace: the id is written as given, and only a real 護石 code gets
    // the paired bytes.
    let index = {
        let Some(box_bytes) = save.equipment_box(slot) else {
            return Err(EquipError::OutOfRange);
        };
        (0..equip::EQUIP_COUNT).find(|i| {
            let o = i * equip::EQUIP_RECORD;
            box_bytes
                .get(o..o + equip::EQUIP_RECORD)
                .map(equip::record_is_blank)
                .unwrap_or(false)
        })
    };
    let Some(index) = index else {
        return Err(EquipError::BoxFull);
    };
    let Some(target) = save.equipment_record_mut(slot, index) else {
        return Err(EquipError::OutOfRange);
    };
    target.fill(0);
    let head: u16 = (kind as u16) & 0x1F;
    target[0..2].copy_from_slice(&head.to_le_bytes());
    target[2..4].copy_from_slice(&id.to_le_bytes());
    equip::set_level(target, level);
    if kind == charm::CHARM_TYPE && is_charm_code(id) && !write_charm_pair(save, slot, index, id as u8)
    {
        return Err(EquipError::BadCharmKind);
    }
    Ok(index)
}

/// Refuse a type no record can carry before anything is written.
///
/// 0 is "this slot is empty" rather than a kind, and 25..31 name nothing in
/// [equip::type_name]; both are refused instead of written, because a record
/// with one of them is a record no screen can name.
fn check_kind(kind: u8) -> Result<(), EquipError> {
    if kind == 0 {
        return Err(EquipError::KindIsBlank);
    }
    if kind > equip::MAX_KIND {
        return Err(EquipError::UnknownKind(kind));
    }
    Ok(())
}

/// What a [change_kind] did, and what it left behind.
///
/// A kind change is the one equipment edit whose damage cannot be seen on the
/// record afterwards: the bytes are all still there, they simply stop meaning
/// what they meant. So the answer carries three things the two screens print -
/// what was written (`notes`), what still points at the record (`references`),
/// and what does not fit the new type any more (`warnings`, from
/// [kind_warnings]). Nothing in `warnings` was repaired: the record is exactly
/// as the user left it, plus the bytes the new type needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindChange {
    /// Box index of the record.
    pub index: usize,
    /// Type before the change.
    pub from: u8,
    /// Type after it.
    pub to: u8,
    /// The kind code written into +0x02 for a talisman, if one was.
    pub charm_code: Option<u8>,
    /// Everything that still names the record (worn items, saved sets).
    pub references: Vec<EquipRef>,
    /// What was written, in the order the screens print it.
    pub notes: Vec<String>,
    /// Consistency lines measured after the write. Nothing here was repaired.
    pub warnings: Vec<String>,
}

impl KindChange {
    /// One line for a status row: the change, then every note behind it.
    pub fn text(&self) -> String {
        let mut s = if self.from == self.to {
            format!(
                "枠 {} の種別（{}）はそのままで、内容だけを書き換えました",
                self.index,
                equip::type_name(self.to)
            )
        } else {
            format!(
                "枠 {} の種別を {} → {} に変えました",
                self.index,
                equip::type_name(self.from),
                equip::type_name(self.to)
            )
        };
        for n in &self.notes {
            s += " / ";
            s += n;
        }
        if !self.references.is_empty() {
            s += &format!(
                " / この枠は {} か所から参照されています（装備中・マイセット）",
                self.references.len()
            );
        }
        s
    }

    /// Did the type itself move?
    pub fn changed_type(&self) -> bool {
        self.from != self.to
    }
}

/// Change the type of one equipment record, keeping every byte that still
/// has a meaning.
///
/// The type is the low five bits of the record's first word, and this is the
/// operation that writes them on their own - [replace] moves the type together
/// with the id, which is a different question ("put that item here instead")
/// from "this record is a talisman now".
///
/// # What is written
///
/// * The type bits, through [equip::set_kind]. The level and the transmog
///   level share that word and are preserved.
/// * For a talisman, the three bytes the type needs on top: +0x02 (the kind
///   code), +0x12 (the rarity of that kind) and +0x13, all through
///   [charm::set_kind]. That is not optional - a type 6 record without them is
///   invisible to [charm::decode_box] and is refused by every charm editor -
///   and it is the only pair of fields this function writes beyond the type.
/// * `charm_code` is used only when the new type is a talisman, and must be a
///   kind the game ships (1..=10); 0 or 11+ is
///   [EquipError::BadCharmKind].
///
/// # What is deliberately *not* written
///
/// +0x0C..+0x24 stay byte for byte. A weapon's tail is meaningless once the
/// record is a talisman, and so is a talisman's skill list once the record is a
/// sword, but deleting it would take data the user cannot get back, and the
/// only thing wrong with it is that it no longer means anything. The caller is
/// told with [kind_warnings] instead, which [KindChange::warnings] already
/// carries. The level field is left alone for the same reason - a real talisman
/// record has 0 there, i.e. Lv1 on this screen, and a converted one keeps
/// whatever the weapon had, which is reported rather than flattened.
pub fn change_kind(
    save: &mut Save,
    slot: usize,
    index: usize,
    kind: u8,
    charm_code: u8,
) -> Result<KindChange, EquipError> {
    check_kind(kind)?;
    if kind == charm::CHARM_TYPE && !is_charm_code(charm_code as u16) {
        return Err(EquipError::BadCharmKind);
    }
    let from = {
        let Some(box_bytes) = save.equipment_box(slot) else {
            return Err(EquipError::OutOfRange);
        };
        let o = index * equip::EQUIP_RECORD;
        let Some(rec) = box_bytes.get(o..o + equip::EQUIP_RECORD) else {
            return Err(EquipError::OutOfRange);
        };
        if equip::record_is_blank(rec) {
            return Err(EquipError::AlreadyBlank);
        }
        equip::decode_record(index, rec)
            .map(|e| e.kind)
            .ok_or(EquipError::OutOfRange)?
    };

    // One field: the type bits of the first word. equip::set_kind owns them and
    // nothing else, so a level or a transmog level survives.
    {
        let Some(rec) = save.equipment_record_mut(slot, index) else {
            return Err(EquipError::OutOfRange);
        };
        if !equip::set_kind(rec, kind) {
            return Err(EquipError::OutOfRange);
        }
    }

    // The talisman pair, through the one writer that owns those offsets.
    if kind == charm::CHARM_TYPE && !write_charm_pair(save, slot, index, charm_code) {
        return Err(EquipError::BadCharmKind);
    }

    let mut notes: Vec<String> = Vec::new();
    let mut charm_written: Option<u8> = None;
    if kind == charm::CHARM_TYPE {
        let name = charm::kind_name(charm_code).unwrap_or("?");
        let rarity = charm::kind_rarity(charm_code).unwrap_or(0);
        notes.push(format!(
            "護石の種類 {} の種別コード（+0x02 = {}）と護石ID（+0x12 = {} / +0x13 = 1）を charm::set_kind で書きました",
            name, charm_code, rarity
        ));
        charm_written = Some(charm_code);
        if from != charm::CHARM_TYPE {
            notes.push(String::from(
                "前の装備のバイト（+0x0C〜+0x10 のスキル・ポイント・スロット）は消していません。お守りとして整えるには護石タブで編集してください",
            ));
        }
    } else if from == charm::CHARM_TYPE {
        // Read back rather than guessing: the bytes are still there.
        let (code, rarity, high) = {
            let Some(box_bytes) = save.equipment_box(slot) else {
                return Err(EquipError::OutOfRange);
            };
            let o = index * equip::EQUIP_RECORD;
            match box_bytes.get(o..o + equip::EQUIP_RECORD) {
                Some(rec) => (rec[0x02], rec[0x12], rec[0x13]),
                None => (0, 0, 0),
            }
        };
        let name = charm::kind_name(code)
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("ID{}", code));
        notes.push(format!(
            "お守りの種別コード（+0x02 = {} {}）と護石ID（+0x12 = {} / +0x13 = {}）は書き換えずに残しました",
            code, name, rarity, high
        ));
    }
    let level = save
        .equipment_at(slot, index)
        .map(|e| e.level)
        .unwrap_or(1);
    if kind == charm::CHARM_TYPE && level != 1 {
        notes.push(format!(
            "Lv 欄は {} のままです（実機の護石は 0 = この画面の Lv1 です。護石らしくするなら Lv を 1 に）",
            level
        ));
    }

    let references = equiprefs::references(save, slot, index);
    let warnings = kind_warnings(save, slot, index);

    Ok(KindChange {
        index,
        from,
        to: kind,
        charm_code: charm_written,
        references,
        notes,
        warnings,
    })
}

// ---------------------------------------------------------------------------
// Consistency: does the record still agree with its own type?
// ---------------------------------------------------------------------------

/// The record's talisman code, rarity and high byte, for the warning lines.
fn charm_bytes(rec: &[u8]) -> (u8, u8, u8) {
    if rec.len() < equip::EQUIP_RECORD {
        return (0, 0, 0);
    }
    (rec[0x02], rec[0x12], rec[0x13])
}

/// What about this 36 byte record does not fit the type it carries?
///
/// The type is the only byte that decides what the rest means, so changing it
/// can leave a record whose own fields contradict it. The checks here are the
/// ones a save can actually hold:
///
/// * a type past the table (25..31) - no screen can name it;
/// * a talisman (type 6) whose kind code at +0x02 is 0 or past the eleven-row
///   table - the game never writes one, and [charm::decode_box] cannot place
///   it in any grade;
/// * a talisman whose +0x12 does not carry the rarity of its kind, or whose
///   +0x13 is not 1 (the u16 item id 353..356 has 1 in its high byte);
/// * a talisman whose level or transmog bits are set - real records have 0
///   there, which is Lv1 on this screen;
/// * the opposite direction: a weapon or a piece of armour that still carries
///   a talisman's kind code and rarity in +0x02/+0x12 (a conversion away from
///   type 6 leaves them in place on purpose);
/// * a palico kind (22..=24) in a hunter's record.
///
/// Nothing is written and nothing is repaired; the caller decides. Empty means
/// the record's fields agree with its type.
pub fn kind_warnings_record(rec: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let Some(eq) = equip::decode_record(0, rec) else {
        return out;
    };
    if equip::record_is_blank(rec) {
        return out;
    }
    if eq.kind > equip::MAX_KIND {
        out.push(format!(
            "種別 {} は装備の種別表にありません（1〜{}）",
            eq.kind,
            equip::MAX_KIND
        ));
        return out;
    }
    let (code, rarity, high) = charm_bytes(rec);
    if eq.kind == charm::CHARM_TYPE {
        if !charm::is_kind(code) {
            out.push(match charm::kind_name(code) {
                // Row 0 is 装備なし: a name exists but it is not a grade.
                Some(name) => format!(
                    "お守りなのに種別コード（+0x02）が {}（{}）です",
                    code, name
                ),
                None => format!(
                    "お守りなのに種別コード（+0x02）が {} で、護石の種類表（1〜10）にありません",
                    code
                ),
            });
        } else if let Some(want) = charm::kind_rarity(code) {
            if rarity != want {
                out.push(format!(
                    "護石ID（+0x12）が {} ですが、{} のレア度は {} です",
                    rarity,
                    charm::kind_name(code).unwrap_or("?"),
                    want
                ));
            }
        }
        if high != 1 {
            out.push(format!(
                "護石ID の上位バイト（+0x13）が {} です（実機は 1）",
                high
            ));
        }
        if eq.level != 1 {
            out.push(format!(
                "Lv 欄が {} です（実機の護石は Lv 欄 0 = この画面の Lv1）",
                eq.level
            ));
        }
        if eq.transmog != 0 || eq.transmog_level != 1 {
            out.push(format!(
                "着せ替え（+0x04 = {} / 元レベル {}）が残っています（お守りは 0 です）",
                eq.transmog, eq.transmog_level
            ));
        }
    } else {
        if charm::is_kind(code) && (97..=100).contains(&rarity) {
            out.push(format!(
                "お守りの種別コード（+0x02 = {} {}）と護石ID（+0x12 = {} / +0x13 = {}）が残っています。{} はこれらを読みませんが、消していません",
                code,
                charm::kind_name(code).unwrap_or("?"),
                rarity,
                high,
                equip::type_name(eq.kind)
            ));
        }
        if !equip::is_hunter_kind(eq.kind) {
            out.push(format!(
                "{} はオトモの種別です（ハンターの装備BOX には出ません）",
                equip::type_name(eq.kind)
            ));
        }
    }
    out
}

/// The same answer for the record at (slot, index) of an open save.
///
/// A talisman gets the charm module's own two checks on top - the five shape
/// rules ([charm::structure_warnings_record]) and the per-kind ceilings
/// ([charm::grade_warnings]) - because those are the lines the 護石 tab already
/// prints, and a record made by a kind change should read the same in both
/// tabs. An empty vector covers a slot or an index that does not exist.
pub fn kind_warnings(save: &Save, slot: usize, index: usize) -> Vec<String> {
    let Some(box_bytes) = save.equipment_box(slot) else {
        return Vec::new();
    };
    let Some(o) = index.checked_mul(equip::EQUIP_RECORD) else {
        return Vec::new();
    };
    let Some(rec) = box_bytes.get(o..o + equip::EQUIP_RECORD) else {
        return Vec::new();
    };
    let mut out = kind_warnings_record(rec);
    if charm::is_talisman(rec) {
        out.extend(charm::structure_warnings_record(rec));
        if let Some(grade) = charm::rarity_of_record(rec) {
            out.extend(charm::grade_warnings(box_bytes, index, grade));
        }
    }
    out
}