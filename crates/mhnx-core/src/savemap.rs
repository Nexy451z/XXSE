//! The save map: every named stretch of a save, plus a hex dump of one.
//!
//! The reference editor ships an 858 row `save-map.csv` and a page that lists
//! it and dumps the bytes of the selected row. This is the same idea, built
//! only from the constants the rest of this crate already reads and writes
//! through: a row cannot name a byte no writer knows about, and moving an
//! offset in `save.rs`, `myset.rs`, `guildcard.rs` … moves the row with it.
//!
//! Every row is **read-only**. Nothing here writes: [fields] names the
//! stretches, [value_text] decodes the ones a getter can read, [dump] and
//! [dump_for] print one row as hex. The desktop tab that draws them says so on
//! screen as well.
//!
//! ## Offsets
//!
//! * a body row keeps the absolute body offset of its constant;
//! * a character slot row keeps the constant slot-relative offset, and
//!   [Field::offset] reports it as `FIRST_SLOT_BASE + slot * SLOT_STRIDE + rel`,
//!   which is the layout every MHXX save has;
//! * a download-section row counts from the section pointer the save carries at
//!   body + 0x0C (0xB280 on MHXX).
//!
//! [value_text], [offset_in] and [dump_for] take an open [Save] and resolve all
//! three through it, so a save whose bases moved (an imported slot, MHGU) is
//! still read at the right place. [dump] works on raw body bytes and therefore
//! has to assume the MHXX layout.
//!
//! ## Both builds
//!
//! The table is plain data laid out at compile time, not a once-cell: every
//! row's name is a `&'static str` and nothing is allocated or initialised when
//! it is first read. That is what lets the console build - which has no `std`
//! at all - hand out the same rows as the PC one. The review on hardware
//! therefore names records the same way ("装備ボックス #24"), instead of
//! falling back to "装備ボックス +0x33C".

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::save::{self, Save};
use crate::{
    awards, charm, dlc, equip, guildcard, hunts, itembox, myset, palico, palicogear,
    questprogress, requests,
};

/// How many bytes a [dump] line shows.
pub const DUMP_COLUMNS: usize = 16;

/// How many bytes a dump shows before it says how many are left.
///
/// A whole item box is 5,463 bytes and the equipment box 72,000: the dump is
/// for finding your place in a record, not for reading a box out of a window,
/// so it stops at the same 512 bytes the reference page uses.
pub const DUMP_BYTES: usize = 512;

/// Which records of an array belong to a row.
///
/// Two rows can cover the same bytes. 装備ボックス is 2,000 records of 36 bytes
/// holding weapons, armour and talismans side by side, and 護石 is the very
/// same 2,000 records read for the talismans among them. Which row a change
/// belongs to is a question about the value, not about the table order: the
/// record's type byte ([crate::charm::record_type]) is what says talisman.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// Every record the row covers belongs to it.
    Any,
    /// Only the records [crate::charm::is_talisman] accepts belong to it.
    Talisman,
}

/// What a stretch of bytes holds, as far as this crate can decode it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Raw bytes: one byte, or a colour / id of a few.
    Byte,
    /// Little endian u16.
    U16,
    /// Little endian u32.
    U32,
    /// A bit map: every bit is a value of its own.
    BitMap,
    /// An array of fixed size records.
    Record {
        /// Bytes per record.
        size: usize,
        /// Records the stretch holds.
        count: usize,
        /// Which of those records the row names.
        claim: Claim,
    },
    /// UTF-8 text, NUL padded.
    Text,
}

impl FieldKind {
    /// The word the セーブマップ tab prints for this kind.
    pub fn label(self) -> &'static str {
        match self {
            FieldKind::Byte => "バイト",
            FieldKind::U16 => "u16",
            FieldKind::U32 => "u32",
            FieldKind::BitMap => "ビットマップ",
            FieldKind::Record { .. } => "レコード",
            FieldKind::Text => "テキスト",
        }
    }
}

/// One named stretch of the save.
///
/// Plain data the compiler can lay out ahead of time: the name is a
/// `&'static str`, so a row holds no allocation and the whole table is built
/// by a `const fn`. That is what the console build (no `std`) reads too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// Character slot the row lives in, `None` for the body-wide parts.
    pub slot: Option<usize>,
    /// Body offset in the canonical MHXX layout; see [offset_in] for the offset
    /// inside a particular save.
    pub offset: usize,
    /// How many bytes the row covers.
    pub len: usize,
    /// What the row is called, in the language of the interface.
    pub name: &'static str,
    /// What the bytes hold.
    pub kind: FieldKind,
}

/// Where a row's offset counts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// The body itself.
    Body,
    /// A character slot, resolved through the base the save carries.
    Slot(usize),
    /// The download-quest section, whose pointer the save carries.
    Dlc,
}

impl Origin {
    /// The base the canonical MHXX layout uses for this origin.
    const fn canonical(self) -> usize {
        match self {
            Origin::Body => 0,
            Origin::Slot(n) => save::FIRST_SLOT_BASE as usize + n * save::SLOT_STRIDE,
            Origin::Dlc => DLC_BASE,
        }
    }

    /// The slot index a [Field] reports for this origin.
    const fn slot(self) -> Option<usize> {
        match self {
            Origin::Slot(n) => Some(n),
            _ => None,
        }
    }

    /// What the list calls this origin.
    fn region(self) -> &'static str {
        match self {
            Origin::Body => "本体",
            Origin::Dlc => "DLC セクション",
            Origin::Slot(0) => "スロット1",
            Origin::Slot(1) => "スロット2",
            Origin::Slot(2) => "スロット3",
            Origin::Slot(_) => "スロット",
        }
    }
}

/// Canonical body offset of the download-quest section.
///
/// The save carries the pointer at body + 0x0C and MHXX always writes 0xB280,
/// because the section ends exactly where the first character slot begins. The
/// base is therefore the first slot base minus what the section holds - no new
/// number, just the two constants either side of it.
const DLC_BASE: usize = save::FIRST_SLOT_BASE as usize
    - (dlc::EQUIP_ARRAY_OFFSET + dlc::EQUIP_SLOTS * dlc::EQUIP_SLOT_SIZE);

/// One row of the table, before it is turned into a [Field].
///
/// Everything here is plain data, because the whole table is a `const`.
struct Spec {
    /// Offset from the row's origin.
    offset: usize,
    /// Bytes the row covers.
    len: usize,
    /// What the row is called.
    name: &'static str,
    /// What the bytes hold.
    kind: FieldKind,
}

/// A record row's three numbers, checked while the table is compiled.
///
/// `size * count` has to equal the length, or the row would claim more records
/// than it covers and the dump would stop in the middle of one. A mismatch is a
/// compile error instead of a wrong row.
const fn record(size: usize, count: usize, len: usize) -> FieldKind {
    assert!(
        size * count == len,
        "a record row's size x count must equal its length"
    );
    FieldKind::Record {
        size,
        count,
        claim: Claim::Any,
    }
}

/// The same, for a row that names only the talisman records of an array.
///
/// 護石 is the one row built this way: it covers the equipment box's bytes, but
/// a weapon or a piece of armour in them is not a talisman, so the row must not
/// claim it. [row_at] uses this to settle the tie between the two rows.
const fn talisman_records(size: usize, count: usize, len: usize) -> FieldKind {
    assert!(
        size * count == len,
        "a record row's size x count must equal its length"
    );
    FieldKind::Record {
        size,
        count,
        claim: Claim::Talisman,
    }
}

/// The rows that sit in the body itself.
static BODY_SPECS: &[Spec] = &[
    Spec {
        offset: save::OFF_BRIGHTNESS,
        len: 1,
        name: "画面の明るさ",
        kind: FieldKind::Byte,
    },
    Spec {
        offset: save::OFF_RUMBLE,
        len: 1,
        name: "振動",
        kind: FieldKind::Byte,
    },
    Spec {
        offset: save::OFF_DLC_PALICO,
        len: save::DLC_PALICO_SIZE * save::DLC_PALICO_COUNT,
        name: "DLC オトモ（共通プール）",
        kind: record(
            save::DLC_PALICO_SIZE,
            save::DLC_PALICO_COUNT,
            save::DLC_PALICO_SIZE * save::DLC_PALICO_COUNT,
        ),
    },
];

/// The rows of the download-quest section, relative to its pointer.
static DLC_SPECS: &[Spec] = &[
    Spec {
        offset: dlc::BITMASK_OFFSET,
        len: dlc::BITMASK_LEN,
        name: "DLC 導入済みのビット",
        kind: FieldKind::BitMap,
    },
    Spec {
        offset: dlc::QUEST_ARRAY_OFFSET,
        len: dlc::QUEST_SLOTS * dlc::QUEST_SLOT_SIZE,
        name: "DLC クエスト（205 枠）",
        kind: record(
            dlc::QUEST_SLOT_SIZE,
            dlc::QUEST_SLOTS,
            dlc::QUEST_SLOTS * dlc::QUEST_SLOT_SIZE,
        ),
    },
    Spec {
        offset: dlc::EQUIP_ARRAY_OFFSET,
        len: dlc::EQUIP_SLOTS * dlc::EQUIP_SLOT_SIZE,
        name: "DLC クエスト装備（45 枠）",
        kind: record(
            dlc::EQUIP_SLOT_SIZE,
            dlc::EQUIP_SLOTS,
            dlc::EQUIP_SLOTS * dlc::EQUIP_SLOT_SIZE,
        ),
    },
];

/// One character slot's rows, at the slot-relative offsets the crate uses.
///
/// [fields] repeats this table for all three slots, so a slot row is written
/// once here and is a slot row for every character by construction.
static SLOT_SPECS: &[Spec] = &[
    // ---- character ----
    Spec { offset: save::OFF_NAME, len: 32, name: "名前", kind: FieldKind::Text },
    Spec { offset: save::OFF_NAME_PREVIEW, len: 32, name: "名前（セーブ画面の写し）", kind: FieldKind::Text },
    Spec { offset: save::OFF_PLAY_TIME, len: 4, name: "プレイ時間（秒）", kind: FieldKind::U32 },
    Spec { offset: save::OFF_PLAY_TIME_PREVIEW, len: 4, name: "プレイ時間（セーブ画面の写し）", kind: FieldKind::U32 },
    Spec { offset: save::OFF_FUNDS, len: 4, name: "所持金", kind: FieldKind::U32 },
    Spec { offset: save::OFF_FUNDS_PREVIEW, len: 4, name: "所持金（セーブ画面の写し）", kind: FieldKind::U32 },
    Spec { offset: save::OFF_HUNTER_RANK, len: 2, name: "ハンターランク", kind: FieldKind::U16 },
    Spec { offset: save::OFF_HR_POINTS, len: 4, name: "HR ポイント", kind: FieldKind::U32 },
    Spec { offset: save::OFF_ACADEMY_POINTS, len: 4, name: "龍歴院ポイント", kind: FieldKind::U32 },
    Spec { offset: save::OFF_VILLAGE_POINTS[0], len: 4, name: "村ポイント（ベルナ）", kind: FieldKind::U32 },
    Spec { offset: save::OFF_VILLAGE_POINTS[1], len: 4, name: "村ポイント（ココット）", kind: FieldKind::U32 },
    Spec { offset: save::OFF_VILLAGE_POINTS[2], len: 4, name: "村ポイント（ポッケ）", kind: FieldKind::U32 },
    Spec { offset: save::OFF_VILLAGE_POINTS[3], len: 4, name: "村ポイント（ユクモ）", kind: FieldKind::U32 },
    Spec { offset: save::OFF_VOICE, len: 1, name: "ボイス", kind: FieldKind::Byte },
    Spec { offset: save::OFF_EYE_COLOR, len: 1, name: "瞳の色", kind: FieldKind::Byte },
    Spec { offset: save::OFF_CLOTHING, len: 1, name: "服", kind: FieldKind::Byte },
    Spec { offset: save::OFF_GENDER, len: 1, name: "性別", kind: FieldKind::Byte },
    Spec { offset: save::OFF_GENDER_PREVIEW, len: 1, name: "性別（セーブ画面の写し）", kind: FieldKind::Byte },
    Spec { offset: save::OFF_HAIRSTYLE, len: 1, name: "髪型", kind: FieldKind::Byte },
    Spec { offset: save::OFF_FACE, len: 1, name: "顔", kind: FieldKind::Byte },
    Spec { offset: save::OFF_FEATURES, len: 1, name: "特徴", kind: FieldKind::Byte },
    Spec { offset: save::OFF_SKIN_COLOR, len: 4, name: "肌の色（RGBA）", kind: FieldKind::Byte },
    Spec { offset: save::OFF_HAIR_COLOR, len: 4, name: "髪の色（RGBA）", kind: FieldKind::Byte },
    Spec { offset: save::OFF_FEATURES_COLOR, len: 4, name: "特徴の色（RGBA）", kind: FieldKind::Byte },
    Spec { offset: save::OFF_CLOTHING_COLOR, len: 4, name: "服の色（RGBA）", kind: FieldKind::Byte },
    // ---- item and equipment boxes ----
    Spec { offset: save::OFF_ITEM_BOX, len: itembox::ITEM_BOX_SIZE, name: "アイテムボックス", kind: FieldKind::BitMap },
    Spec {
        offset: save::OFF_EQUIPMENT_BOX,
        len: save::EQUIPMENT_BOX_SIZE,
        name: "装備ボックス",
        kind: record(save::EQUIPMENT_RECORD, save::TOTAL_EQUIP_SLOTS, save::EQUIPMENT_BOX_SIZE),
    },
    // The talismans are the records of the equipment box whose type byte says
    // so; the row is here because the charm module is where that is decoded,
    // and it is the same bytes as 装備ボックス above. The claim is what keeps
    // the two rows from fighting over a change: a talisman record is named by
    // this row, a weapon or a piece of armour by the box's, whichever the table
    // lists first.
    Spec {
        offset: save::OFF_EQUIPMENT_BOX,
        len: charm::CHARM_BOX_SIZE,
        name: "護石",
        kind: talisman_records(charm::CHARM_RECORD, charm::CHARM_COUNT, charm::CHARM_BOX_SIZE),
    },
    Spec {
        offset: palicogear::PALICO_EQUIP_OFFSET,
        len: save::PALICO_EQUIPMENT_SIZE,
        name: "オトモ装備ボックス",
        kind: record(equip::EQUIP_RECORD, palicogear::PALICO_EQUIP_COUNT, save::PALICO_EQUIPMENT_SIZE),
    },
    Spec {
        offset: myset::OFF_ITEM_SETS,
        len: myset::ITEM_SETS * myset::ITEM_SET_SIZE,
        name: "アイテムマイセット",
        kind: record(myset::ITEM_SET_SIZE, myset::ITEM_SETS, myset::ITEM_SETS * myset::ITEM_SET_SIZE),
    },
    Spec {
        offset: myset::OFF_EQUIP_SETS,
        len: myset::EQUIP_SETS * myset::EQUIP_SET_SIZE,
        name: "装備マイセット",
        kind: record(myset::EQUIP_SET_SIZE, myset::EQUIP_SETS, myset::EQUIP_SETS * myset::EQUIP_SET_SIZE),
    },
    Spec { offset: myset::OFF_POUCH, len: myset::POUCH_SIZE, name: "ポーチ", kind: FieldKind::BitMap },
    // ---- otomo ----
    Spec {
        offset: palico::OFF_ROSTER,
        len: palico::SLOTS * palico::RECORD,
        name: "オトモ",
        kind: record(palico::RECORD, palico::SLOTS, palico::SLOTS * palico::RECORD),
    },
    // ---- hunt records ----
    Spec {
        offset: save::OFF_MONSTER_HUNT,
        len: save::MONSTER_TABLE_SIZE,
        name: "狩猟記録（討伐）",
        kind: record(2, hunts::MONSTERS, save::MONSTER_TABLE_SIZE),
    },
    Spec {
        offset: save::OFF_MONSTER_CAPTURE,
        len: save::MONSTER_TABLE_SIZE,
        name: "狩猟記録（捕獲）",
        kind: record(2, hunts::MONSTERS, save::MONSTER_TABLE_SIZE),
    },
    Spec {
        offset: save::OFF_MONSTER_SIZE,
        len: save::MONSTER_SIZE_TABLE_SIZE,
        name: "狩猟記録（サイズ）",
        kind: record(4, hunts::MONSTERS, save::MONSTER_SIZE_TABLE_SIZE),
    },
    // ---- guild card ----
    Spec { offset: save::OFF_GUILD_CARD, len: save::GUILD_CARD_SIZE, name: "ギルドカード", kind: FieldKind::Byte },
    Spec {
        offset: save::OFF_GUILD_CARD + guildcard::GC_HR,
        len: 2,
        name: "ギルドカード（HR）",
        kind: FieldKind::U16,
    },
    Spec {
        offset: save::OFF_GUILD_CARD + guildcard::GC_PLAY_TIME,
        len: 4,
        name: "ギルドカード（プレイ時間）",
        kind: FieldKind::U32,
    },
    // UTF-16, so the value column shows the bytes rather than a wrong string.
    Spec {
        offset: save::OFF_GUILD_CARD + guildcard::GC_COMMENT,
        len: guildcard::GC_COMMENT_CHARS * 2,
        name: "ギルドカード（コメント・UTF-16）",
        kind: FieldKind::Byte,
    },
    Spec {
        offset: save::OFF_GUILD_CARD + guildcard::GC_ID,
        len: 8,
        name: "ギルドカード（カード ID）",
        kind: FieldKind::Byte,
    },
    Spec {
        offset: save::OFF_GUILD_CARD + guildcard::GC_TITLE,
        len: guildcard::TITLE_IDS * 2,
        name: "ギルドカード（称号）",
        kind: record(2, guildcard::TITLE_IDS, guildcard::TITLE_IDS * 2),
    },
    Spec {
        offset: save::OFF_GUILD_CARD + guildcard::GC_QUESTS,
        len: guildcard::QUEST_COUNTS * 2,
        name: "ギルドカード（クエスト回数）",
        kind: record(2, guildcard::QUEST_COUNTS, guildcard::QUEST_COUNTS * 2),
    },
    Spec {
        offset: save::OFF_GUILD_CARD + guildcard::GC_WEAPON,
        len: guildcard::WEAPON_VENUE_BYTES * guildcard::Venue::ALL.len(),
        name: "ギルドカード（武器の使用回数）",
        kind: record(
            guildcard::WEAPON_VENUE_BYTES,
            guildcard::Venue::ALL.len(),
            guildcard::WEAPON_VENUE_BYTES * guildcard::Venue::ALL.len(),
        ),
    },
    Spec {
        offset: save::OFF_GUILD_CARD + guildcard::GC_HISTORY,
        len: guildcard::HISTORY_RECORD * guildcard::HISTORY_RECORDS,
        name: "ギルドカード（履歴）",
        kind: record(
            guildcard::HISTORY_RECORD,
            guildcard::HISTORY_RECORDS,
            guildcard::HISTORY_RECORD * guildcard::HISTORY_RECORDS,
        ),
    },
    Spec {
        offset: guildcard::OFF_ARENA,
        len: guildcard::ARENA_ENTRY * guildcard::ARENA_COUNT * guildcard::ARENA_RANKS,
        name: "闘技大会の記録",
        kind: record(
            guildcard::ARENA_ENTRY,
            guildcard::ARENA_COUNT * guildcard::ARENA_RANKS,
            guildcard::ARENA_ENTRY * guildcard::ARENA_COUNT * guildcard::ARENA_RANKS,
        ),
    },
    Spec {
        offset: guildcard::GC_CARDS,
        len: guildcard::CARD_ELEMENT * guildcard::CARD_LIST_LEN,
        name: "受信カード（保存 100 枚）",
        kind: record(
            guildcard::CARD_ELEMENT,
            guildcard::CARD_LIST_LEN,
            guildcard::CARD_ELEMENT * guildcard::CARD_LIST_LEN,
        ),
    },
    Spec {
        offset: guildcard::GC_CARDS_INFO,
        len: guildcard::CARD_INFO_LEN * guildcard::CARD_LIST_LEN,
        name: "受信カード（保存一覧の情報）",
        kind: record(
            guildcard::CARD_INFO_LEN,
            guildcard::CARD_LIST_LEN,
            guildcard::CARD_INFO_LEN * guildcard::CARD_LIST_LEN,
        ),
    },
    Spec {
        offset: guildcard::GC_INBOX,
        len: guildcard::CARD_ELEMENT * guildcard::INBOX_LIST_LEN,
        name: "受信カード（受信箱 50 枚）",
        kind: record(
            guildcard::CARD_ELEMENT,
            guildcard::INBOX_LIST_LEN,
            guildcard::CARD_ELEMENT * guildcard::INBOX_LIST_LEN,
        ),
    },
    Spec {
        offset: guildcard::GC_INBOX_INFO,
        len: guildcard::INBOX_INFO_LEN * guildcard::INBOX_LIST_LEN,
        name: "受信カード（受信箱の情報）",
        kind: record(
            guildcard::INBOX_INFO_LEN,
            guildcard::INBOX_LIST_LEN,
            guildcard::INBOX_INFO_LEN * guildcard::INBOX_LIST_LEN,
        ),
    },
    // ---- awards, requests, quest progress ----
    Spec { offset: awards::CARD_OFF, len: awards::CARD_LEN, name: "勲章（カード側）", kind: FieldKind::BitMap },
    Spec { offset: awards::MAP_OFF, len: awards::MAP_LEN, name: "勲章（解禁マップ）", kind: FieldKind::BitMap },
    Spec { offset: awards::NOTICE_OFF, len: awards::NOTICE_LEN, name: "勲章（通知）", kind: FieldKind::BitMap },
    Spec { offset: requests::OFF_FLAGS, len: requests::FLAG_BYTES, name: "依頼フラグ", kind: FieldKind::BitMap },
    Spec { offset: questprogress::OFF_CLEARED, len: questprogress::MAP_SIZE, name: "クエスト（クリア）", kind: FieldKind::BitMap },
    Spec { offset: questprogress::OFF_SEEN, len: questprogress::MAP_SIZE, name: "クエスト（発見）", kind: FieldKind::BitMap },
    Spec { offset: questprogress::OFF_FAILED, len: questprogress::MAP_SIZE, name: "クエスト（失敗）", kind: FieldKind::BitMap },
    // ---- shoutouts ----
    Spec {
        offset: save::OFF_MANUAL_SHOUTOUTS,
        len: save::MANUAL_SHOUTOUT_SIZE,
        name: "定型文（手動）",
        kind: record(save::SHOUTOUT_ENTRY, save::TOTAL_MANUAL_SHOUTOUTS, save::MANUAL_SHOUTOUT_SIZE),
    },
    Spec {
        offset: save::OFF_AUTO_SHOUTOUTS,
        len: save::AUTO_SHOUTOUT_SIZE,
        name: "定型文（自動）",
        kind: record(save::SHOUTOUT_ENTRY, save::TOTAL_AUTO_SHOUTOUTS, save::AUTO_SHOUTOUT_SIZE),
    },
];

/// How many rows the map has.
pub const ROWS: usize = BODY_SPECS.len() + DLC_SPECS.len() + SLOT_SPECS.len() * save::MAX_SLOTS;

/// A row with nothing in it, used only to size [TABLE].
const EMPTY_ROW: Field = Field {
    slot: None,
    offset: 0,
    len: 0,
    name: "",
    kind: FieldKind::Byte,
};

/// The row behind an index, with the origin its offset counts from.
const fn spec_at(index: usize) -> (Origin, &'static Spec) {
    let body = BODY_SPECS.len();
    if index < body {
        return (Origin::Body, &BODY_SPECS[index]);
    }
    let index = index - body;
    if index < DLC_SPECS.len() {
        return (Origin::Dlc, &DLC_SPECS[index]);
    }
    let index = index - DLC_SPECS.len();
    (
        Origin::Slot(index / SLOT_SPECS.len()),
        &SLOT_SPECS[index % SLOT_SPECS.len()],
    )
}

/// Build one [Field] from a row.
const fn field_at(index: usize) -> Field {
    let (origin, spec) = spec_at(index);
    Field {
        slot: origin.slot(),
        offset: origin.canonical() + spec.offset,
        len: spec.len,
        name: spec.name,
        kind: spec.kind,
    }
}

/// The table, laid out by the compiler: no allocation, no once-cell.
const fn build_table() -> [Field; ROWS] {
    let mut out = [EMPTY_ROW; ROWS];
    let mut index = 0;
    while index < ROWS {
        out[index] = field_at(index);
        index += 1;
    }
    out
}

/// Every row of the map, built ahead of time.
static TABLE: [Field; ROWS] = build_table();

/// The row behind an index, when the index is in the table.
fn spec(index: usize) -> Option<(Origin, &'static Spec)> {
    if index >= ROWS {
        return None;
    }
    Some(spec_at(index))
}

/// Every named stretch of a save, in the order the tab lists them.
///
/// The table is a `static` built at compile time and handed out by reference:
/// the GUI asks for it every frame, the rows never change, and the console
/// build reads the very same ones.
///
/// The rows are grouped the way the tabs are (character, boxes, records, guild
/// card, progress, shoutouts) rather than sorted by offset, because a reader
/// looks for "the equipment box" before "whatever lives at 0x62EE". Within a
/// group the order follows the offsets.
pub fn fields() -> &'static [Field] {
    &TABLE
}

/// How strongly the row at `index` claims the byte at `body_off`.
///
/// A row that names any record it covers claims weakly (0); one that names
/// only records of one kind claims strongly (1) when the value is that kind
/// and not at all (-1) when it is not. 護石 is the only row of the second sort:
/// the box holds weapons, armour and talismans side by side, so whether a
/// change is a talisman's is a question about the record's type byte
/// ([crate::charm::is_talisman]), not about which row the table lists first.
fn claim_rank(save: &Save, index: usize, start: usize, body_off: usize) -> i8 {
    match TABLE[index].kind {
        FieldKind::Record {
            claim: Claim::Any, ..
        } => 0,
        FieldKind::Record {
            size,
            claim: Claim::Talisman,
            ..
        } => {
            let at = start + ((body_off - start) / size) * size;
            match save.body().get(at..at + size) {
                Some(record) if charm::is_talisman(record) => 1,
                _ => -1,
            }
        }
        _ => 0,
    }
}

/// The row that names the byte at `body_off`, or `None` when no row covers it.
///
/// The narrowest row wins. Two rows of the same width are settled by how
/// strongly they claim the byte: 装備ボックス and 護石 cover the same 72,000
/// bytes, so the record's type byte decides, and a talisman's skill point is
/// named `護石 #225` rather than `装備ボックス #225`. A row whose records are
/// not the kind it names takes no part at all.
///
/// Every build reads this: the table carries `&'static str` names, so the
/// console one has it too and its review names records the same way as the PC
/// one.
pub fn row_at(save: &Save, body_off: usize) -> Option<usize> {
    let mut best: Option<(usize, i8)> = None;
    for index in 0..ROWS {
        let Some(start) = offset_in(save, index) else {
            continue;
        };
        if body_off < start || body_off >= start + TABLE[index].len {
            continue;
        }
        let rank = claim_rank(save, index, start, body_off);
        if rank < 0 {
            continue;
        }
        let keep = match best {
            Some((kept, kept_rank)) => {
                TABLE[kept].len < TABLE[index].len
                    || (TABLE[kept].len == TABLE[index].len && kept_rank >= rank)
            }
            None => false,
        };
        if !keep {
            best = Some((index, rank));
        }
    }
    best.map(|(index, _)| index)
}

/// What the row at `field_index` belongs to: 本体, DLC セクション, or スロットN.
///
/// The [Field] itself only carries `slot`, which cannot tell the body-wide rows
/// from the download section, so the label comes from the table.
pub fn region(field_index: usize) -> &'static str {
    match spec(field_index) {
        Some((origin, _)) => origin.region(),
        None => "—",
    }
}

/// Where the row sits inside `save`.
///
/// The slot bases and the download-section pointer come out of the file, so a
/// save that moved them (an imported character, MHGU) is read where its own
/// pointers say. `None` means the save does not hold this row: no such slot, no
/// download section, or the range runs past the body.
pub fn offset_in(save: &Save, field_index: usize) -> Option<usize> {
    let (origin, spec) = spec(field_index)?;
    let base = match origin {
        Origin::Body => 0,
        Origin::Slot(n) => save.slot_base(n)? as usize,
        Origin::Dlc => {
            let section = save.dlc_section_offset() as usize;
            if section == 0 {
                return None;
            }
            section
        }
    };
    let offset = base.checked_add(spec.offset)?;
    let end = offset.checked_add(spec.len)?;
    if end > save.body().len() {
        return None;
    }
    Some(offset)
}

/// What the row holds, as one line.
///
/// A number the accessors can read is printed as a value (with its hex), a text
/// field as the text, a bit map as how many of its bits are set, and anything
/// that cannot be decoded - a record, an unknown byte, a name that is not UTF-8
/// - as the raw bytes. `None` means the row is not in this save; see
/// [offset_in].
pub fn value_text(save: &Save, field_index: usize) -> Option<String> {
    let offset = offset_in(save, field_index)?;
    let (_, spec) = spec(field_index)?;
    let bytes = save.body().get(offset..offset + spec.len)?;
    Some(describe(bytes, spec.kind))
}

/// The hex dump of one row, read from raw body bytes.
///
/// `bytes` is the save **body** (without the Switch header) and the row is
/// taken at its canonical MHXX offset; use [dump_for] when a [Save] is at hand,
/// which reads the bases out of the file instead of assuming them.
pub fn dump(bytes: &[u8], field_index: usize) -> Option<Vec<String>> {
    let (origin, spec) = spec(field_index)?;
    let offset = origin.canonical().checked_add(spec.offset)?;
    if offset.checked_add(spec.len)? > bytes.len() {
        return None;
    }
    Some(hex_lines(bytes, offset, spec.len))
}

/// The hex dump of one row of an open save.
///
/// The same output as [dump], with the row resolved through the save's own slot
/// and download-section pointers.
pub fn dump_for(save: &Save, field_index: usize) -> Option<Vec<String>> {
    let offset = offset_in(save, field_index)?;
    let (_, spec) = spec(field_index)?;
    Some(hex_lines(save.body(), offset, spec.len))
}

/// The lines one row's dump is made of.
///
/// Sixteen bytes a line with the body offset in front, and a last line saying
/// how much was left out once the row is longer than [DUMP_BYTES].
fn hex_lines(body: &[u8], offset: usize, len: usize) -> Vec<String> {
    let shown = len.min(DUMP_BYTES);
    let mut out = Vec::with_capacity(shown / DUMP_COLUMNS + 2);
    for row in (0..shown).step_by(DUMP_COLUMNS) {
        let n = DUMP_COLUMNS.min(shown - row);
        let mut line = format!("{:06X}  ", offset + row);
        for i in 0..n {
            if i == 8 && n > 8 {
                line.push(' ');
            }
            line.push_str(&format!("{:02x} ", body[offset + row + i]));
        }
        while line.ends_with(' ') {
            line.pop();
        }
        out.push(line);
    }
    if len > shown {
        out.push(format!("… ほか {} バイト", len - shown));
    }
    out
}

/// One row's value, from its bytes.
fn describe(bytes: &[u8], kind: FieldKind) -> String {
    match kind {
        FieldKind::Byte if bytes.len() == 1 => format!("{} (0x{:02X})", bytes[0], bytes[0]),
        FieldKind::Byte => hex_bytes(bytes, 8),
        FieldKind::U16 if bytes.len() >= 2 => {
            let v = u16::from_le_bytes([bytes[0], bytes[1]]);
            format!("{} (0x{:04X})", v, v)
        }
        FieldKind::U32 if bytes.len() >= 4 => {
            let v = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            format!("{} (0x{:08X})", v, v)
        }
        // A length the kind does not expect is not decoded into a wrong
        // number: the bytes are shown instead.
        FieldKind::U16 | FieldKind::U32 => hex_bytes(bytes, 8),
        FieldKind::Text => text_of(bytes),
        FieldKind::BitMap => {
            let ones: usize = bytes.iter().map(|b| b.count_ones() as usize).sum();
            format!(
                "{} バイト（{} ビット中 {} ビット）",
                bytes.len(),
                bytes.len() * 8,
                ones
            )
        }
        FieldKind::Record { size, count, .. } => format!("{} バイト × {} 件", size, count),
    }
}

/// A NUL padded UTF-8 field, or its bytes when it is not readable text.
fn text_of(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    let raw = &bytes[..end];
    if raw.is_empty() {
        return String::from("（空）");
    }
    match core::str::from_utf8(raw) {
        Ok(s) if !s.chars().any(|c| c.is_control()) => String::from(s),
        _ => hex_bytes(bytes, 8),
    }
}

/// Up to `cap` bytes as hex, with a marker when more follow.
fn hex_bytes(bytes: &[u8], cap: usize) -> String {
    let n = bytes.len().min(cap);
    let mut out = String::with_capacity(n * 3 + 2);
    for (i, b) in bytes[..n].iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{:02X}", b));
    }
    if bytes.len() > n {
        out.push_str(" …");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row has to land inside the save, and the three numbers of a
    /// record row have to agree, or the table would name bytes that are not
    /// there.
    #[test]
    fn every_row_lands_inside_the_save() {
        let rows = fields();
        assert!(rows.len() >= 80, "the map names only {} rows", rows.len());
        for field in rows {
            assert!(!field.name.is_empty());
            assert!(field.len > 0, "{} has no length", field.name);
            assert!(
                field.offset + field.len <= save::BODY_SIZE,
                "{} runs past the end of the body: 0x{:X} + {}",
                field.name,
                field.offset,
                field.len
            );
            if let Some(slot) = field.slot {
                assert!(slot < save::MAX_SLOTS);
                let base = save::FIRST_SLOT_BASE as usize + slot * save::SLOT_STRIDE;
                assert!(field.offset >= base, "{} is before its slot", field.name);
                assert!(
                    field.offset - base + field.len <= save::SLOT_STRIDE,
                    "{} crosses the end of its slot",
                    field.name
                );
            }
            match field.kind {
                FieldKind::U16 => assert_eq!(field.len, 2, "{}", field.name),
                FieldKind::U32 => assert_eq!(field.len, 4, "{}", field.name),
                FieldKind::Record { size, count, .. } => {
                    assert!(size > 0 && count > 0, "{}", field.name);
                    assert_eq!(size * count, field.len, "{}", field.name);
                }
                _ => {}
            }
        }
        // The table is one `static`: the same rows every time, built by the
        // compiler rather than by a once-cell the console build does not have.
        assert!(core::ptr::eq(fields().as_ptr(), fields().as_ptr()));
        assert_eq!(fields().len(), ROWS);
    }

    /// The names are `&'static str` and the table is a `const`, which is what
    /// lets the review name records on the console build too. A `String` here
    /// would put a once-cell (and `std`) back into the module.
    #[test]
    fn the_rows_need_no_cell_and_no_allocation() {
        let name: &'static str = fields()[0].name;
        assert_eq!(name, "画面の明るさ");
        // Rows can be held in a const: nothing in Field is an allocation.
        const FIRST: Field = TABLE[0];
        assert_eq!(FIRST.name, "画面の明るさ");
        // The one proviso: the charm row names only talisman records.
        let charm_row = fields()
            .iter()
            .position(|f| f.name == "護石")
            .expect("the talisman row");
        assert!(matches!(
            fields()[charm_row].kind,
            FieldKind::Record {
                claim: Claim::Talisman,
                ..
            }
        ));
        assert!(fields()
            .iter()
            .filter(|f| f.name != "護石")
            .all(|f| !matches!(
                f.kind,
                FieldKind::Record {
                    claim: Claim::Talisman,
                    ..
                }
            )));
    }

    /// The rows have to be the constants' own numbers: a row that recomputed
    /// its offset could drift away from the writer.
    #[test]
    fn the_rows_come_from_the_crate_constants() {
        let rows = fields();
        let index_of = |name: &str, slot: Option<usize>| {
            rows
                .iter()
                .position(|f| f.name == name && f.slot == slot)
                .unwrap_or_else(|| panic!("no row named {} for slot {:?}", name, slot))
        };
        let first = save::FIRST_SLOT_BASE as usize;
        assert_eq!(
            rows[index_of("画面の明るさ", None)].offset,
            save::OFF_BRIGHTNESS
        );
        assert_eq!(
            rows[index_of("アイテムボックス", Some(0))].offset,
            first + save::OFF_ITEM_BOX
        );
        assert_eq!(
            rows[index_of("装備ボックス", Some(0))].len,
            save::EQUIPMENT_BOX_SIZE
        );
        // The same row one slot later is one stride later, nothing else.
        assert_eq!(
            rows[index_of("アイテムボックス", Some(1))].offset
                - rows[index_of("アイテムボックス", Some(0))].offset,
            save::SLOT_STRIDE
        );
        assert_eq!(
            rows[index_of("アイテムボックス", Some(2))].offset
                - rows[index_of("アイテムボックス", Some(1))].offset,
            save::SLOT_STRIDE
        );
        // The download section is where the reference save puts it.
        assert_eq!(
            rows[index_of("DLC クエスト（205 枠）", None)].offset,
            0xB280 + dlc::QUEST_ARRAY_OFFSET
        );
    }

    /// Sixteen bytes a line, the body offset in front.
    #[test]
    fn the_dump_is_sixteen_bytes_a_line() {
        let mut body = alloc::vec![0u8; save::BODY_SIZE];
        body.fill(0xAB);
        let rows = fields();
        let brightness = rows
            .iter()
            .position(|f| f.name == "画面の明るさ")
            .expect("the row");
        assert_eq!(
            dump(&body, brightness).expect("a dump"),
            alloc::vec![format!("{:06X}  ab", save::OFF_BRIGHTNESS)]
        );
        let name = rows
            .iter()
            .position(|f| f.name == "名前" && f.slot == Some(0))
            .expect("the row");
        let lines = dump(&body, name).expect("a dump");
        assert_eq!(lines.len(), 2);
        // Eight bytes, a wider gap, then the next eight.
        let sixteen = format!("{}  {}", ["ab"; 8].join(" "), ["ab"; 8].join(" "));
        assert_eq!(
            lines[0],
            format!(
                "{:06X}  {}",
                save::FIRST_SLOT_BASE as usize + save::OFF_NAME,
                sixteen
            )
        );
        assert_eq!(
            lines[1],
            format!(
                "{:06X}  {}",
                save::FIRST_SLOT_BASE as usize + save::OFF_NAME + 16,
                sixteen
            )
        );
        // Long rows stop at the cap and say how much is left.
        let boxrow = rows
            .iter()
            .position(|f| f.name == "装備ボックス" && f.slot == Some(0))
            .expect("the row");
        let lines = dump(&body, boxrow).expect("a dump");
        assert_eq!(lines.len(), DUMP_BYTES / DUMP_COLUMNS + 1);
        assert!(lines.last().expect("a line").contains("ほか"));
        // An index past the table, and a body too short for the row, are None
        // rather than a panic.
        assert!(dump(&body, rows.len()).is_none());
        assert!(dump(&body[..2], brightness).is_none());
    }
}
