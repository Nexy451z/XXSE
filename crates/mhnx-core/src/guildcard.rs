//! The guild card: title, weapon usage, quest history, arena log and the cards
//! received from other hunters.
//!
//! The card is a 6,328 byte (0x18B8) block at slot + 0xC71BD that mirrors part of
//! the character: its appearance bytes at +0x19 are identical to the character's
//! own copy at slot + 0x23B48 in every slot of the sample save, and its hunter
//! rank matches. It also keeps its own play time, which lags the character by
//! a few seconds because it was written earlier.
//!
//! save::GUILD_CARD_SIZE (4,986) is **not** the whole card: it ends at the end of
//! the arena log (card + 0x1224 + 342). The card goes on to card + 0x18B8, and
//! that end is not a guess: slot + 0xC71BD + 0x18B8 = slot + 0xC8A75 is exactly
//! where the 100 info records of the received-card list begin, and that list in
//! turn ends (slot + 0xC9BA5) exactly where the 50 element inbox begins. The
//! sample save walks both lists element by element without a gap. GC_SIZE is the
//! real card size.
//!
//! ## Where the new fields come from
//!
//! Every offset below is either from the C# MHXX save editor this project already
//! used (Offsets.cs, EditGuildCardDialog.cs, read out in
//! save_editor/analysis/3ds_save_format.md section 4.6/4.7) or from the
//! MHGU-Save-editor reference tree in .work/finalaudit/refsrc (core/src/
//! guildcard.rs, core/src/cards.rs, docs/11-save-map.md "Card layout"), and every
//! one of them was then read back from the real save
//! (C:/anyWorkspace/nx/jksv_ready/system) with the values recorded in
//! crates/mhnx-core/tests/guildcard_fields.rs. Where the two references
//! disagreed, the offsets here are the ones the real save agrees with.
//!
//! The arena log sits at slot + 0xC83E1, which is card + 0x1224: seventeen
//! arenas of five ranked entries, four bytes each. The first two bytes are the
//! best time in hundredths of a second, so 57,544 is 9:35.44.
//!
//! Open question, deliberately not changed here: the reference tree
//! (core/src/arena.rs, docs/11-save-map.md "Arena log", confirmed in game) reads
//! a whole entry as one u32 instead - an 18 bit frame counter at 1/60 s, the
//! hunter and partner weapon in bits 18..25, the equipment set in 26..28 and the
//! grade in 29..30. Under that reading 57,544 would be frames, i.e. 15:59.07,
//! not hundredths. app.rs and tests/hbedit_flow.rs display and assert the
//! hundredths value today and are outside this change, so ArenaTime keeps the
//! existing interpretation; the two views are recorded in
//! .work/guild/DEPTH.md.
//!
//! These are the MHXX offsets. The MHGU layout differs in places and is not
//! verified here.

use alloc::string::String;
use alloc::vec::Vec;

use crate::romfstables::gmd_strings;
use crate::save::{check_name, NameError, NameFit, NameUnit, Save};

/// Offset of the guild card, relative to the slot base.
pub const OFF_GUILD_CARD: usize = 0xC71BD;
/// Size of the guild card block, up to the end of the arena log.
pub const GUILD_CARD_SIZE: usize = 4986;
/// Size of the whole card as the game stores it (0x18B8 = 6,328 bytes).
///
/// The extra 1,342 bytes behind GUILD_CARD_SIZE hold the arena partners
/// (card + 0x16D4) and end where the received-card info records start.
pub const GC_SIZE: usize = 0x18B8;
/// Hunter rank as the card records it.
pub const GC_HR: usize = 0x16;
/// The hunter's name on the card: UTF-16LE units, no length field.
///
/// The field is the eleven units in front of GC_HR, because 0x16 / 2 = 11. The
/// sample save (C:\anyWorkspace\nx\jksv_ready\system) agrees: card + 0 holds
/// CA 30 CA 30 00 00 CA C1 00 00 00 00 60 97 0D 30 A8 96 36 08 00 00 - the
/// units CA 30 CA 30 are "ナナ", 00 00 is its terminator, the eight units from
/// byte 6 on (CA C1 00 00 00 00 60 97 ...) are leftovers from a longer earlier
/// name, and byte 0x16 (C7 03) is where GC_HR starts. So the name is written up
/// to and including the terminator only; the leftovers stay as they are.
/// Eleven units is also the longest the game's name entry accepts.
pub const GC_NAME: usize = 0x00;
/// Units of the name field: MHXX stores eleven (GC_HR / 2).
pub const GC_NAME_UNITS: usize = GC_HR / 2;
/// The card's copy of the appearance, eight bytes.
pub const GC_APPEARANCE: usize = 0x19;
/// The card's copy of the five armour pigments, 20 bytes.
///
/// It is the same nine-slot colour block the character keeps: card + 0x24 reads
/// 3D 36 3E FF four times and 38 62 CD FF, and 0x24 + 20 = 0x38 is GC_COLOURS,
/// i.e. the card's skin colour - the same relationship
/// save::OFF_ARMOR_COLORS has to save::OFF_SKIN_COLOR.
pub const GC_PIGMENTS: usize = 0x24;
/// The card's copy of the four colours.
pub const GC_COLOURS: usize = 0x38;
// The card blocks line up with the character's: pigments take the twenty bytes
// in front of the colours, wherever the colours are.
const _: () = assert!(GC_PIGMENTS + crate::save::ARMOR_COLORS_SIZE == GC_COLOURS);
/// Card play time, in seconds.
pub const GC_PLAY_TIME: usize = 0x914;
/// Card ID, eight bytes.
pub const GC_ID: usize = 0x8B0;
/// Profile comment, UTF-16, 28 characters.
pub const GC_COMMENT: usize = 0x878;
/// Characters in the comment.
pub const GC_COMMENT_CHARS: usize = 28;
/// Bytes the card name field can hold: eleven UTF-16 units.
pub const NAME_CAPACITY: usize = GC_NAME_UNITS * 2;
/// Bytes the comment field can hold: twenty-eight UTF-16 units.
pub const COMMENT_CAPACITY: usize = GC_COMMENT_CHARS * 2;

/// Offset of the arena log, relative to the slot base.
pub const OFF_ARENA: usize = 0xC83E1;
/// Arenas recorded.
pub const ARENA_COUNT: usize = 17;
/// Ranked entries kept per arena.
pub const ARENA_RANKS: usize = 5;
/// Bytes per entry.
pub const ARENA_ENTRY: usize = 4;

// ---- weapon usage ----------------------------------------------------------
//
// Three arrays of 15 u16, one per venue, back to back:
//
//     card + 0x8BA   village
//     card + 0x8D8   hub
//     card + 0x8F6   arena
//
// 0x8D8 - 0x8BA = 0x8F6 - 0x8D8 = 0x1E = exactly 15 u16, and the last array ends
// at 0x8F6 + 0x1E = 0x914, which is the card's play time. That arithmetic is the
// evidence for the count and the layout, on top of the references: Offsets.cs's
// commented GUILDCARD_VILLAGE_WEAPON_OFFSET (0xC7A77 = card + 0x8BA, "size 30")
// and EditGuildCardDialog.cs:88-101, which indexes 0x8BA/0x8D8/0x8F6 with
// index * 2 over its 15 entry weapon list. The in-game "All" graph is the sum of
// the three arrays and is not stored (docs/04 of the reference tree).
//
// The storage order is the classic internal order, not the on-screen bar order:
// 0 great sword, 1 sword and shield, 2 hammer, 3 lance, 4 heavy bowgun,
// 5 light bowgun, 6 long sword, 7 switch axe, 8 gunlance, 9 bow, 10 dual blades,
// 11 hunting horn, 12 insect glaive, 13 charge blade, 14 prowler.

/// Weapon types the usage graph has: 14 weapons plus the Prowler.
pub const WEAPON_TYPES: usize = 15;
/// First venue array, relative to the card.
pub const GC_WEAPON: usize = 0x8BA;
/// Bytes one venue array occupies (15 u16).
pub const WEAPON_VENUE_BYTES: usize = WEAPON_TYPES * 2;

/// Japanese name of one of the fifteen weapon kinds the usage graph counts.
///
/// The storage order (see [WEAPON_TYPES]), not the on-screen bar order. The
/// card's own gamedata table (HUNT_WEAPONS) is English, and both editors show
/// these names beside Japanese headings, so the words come from the same
/// equipment vocabulary the 装備 screen uses (equip::type_name) in the card's
/// order - with the Prowler last, which the equipment box does not hold.
pub const WEAPON_TYPE_NAMES: [&str; WEAPON_TYPES] = [
    "大剣",
    "片手剣",
    "ハンマー",
    "ランス",
    "ヘビィボウガン",
    "ライトボウガン",
    "太刀",
    "スラッシュアックス",
    "ガンランス",
    "弓",
    "双剣",
    "狩猟笛",
    "操虫棍",
    "チャージアックス",
    "ニャンター",
];

/// Japanese name of one weapon kind, or "?" past the end.
pub fn weapon_type_name(index: usize) -> &'static str {
    WEAPON_TYPE_NAMES.get(index).copied().unwrap_or("?")
}

/// Japanese name of one venue, in the order the card stores them.
pub fn venue_name(venue: Venue) -> &'static str {
    match venue {
        Venue::Village => "村",
        Venue::Hub => "集会所",
        Venue::Arena => "闘技場",
    }
}

/// Which quest venue a usage array counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Venue {
    /// Village quests.
    Village,
    /// Hub quests.
    Hub,
    /// Arena.
    Arena,
}

impl Venue {
    /// All three, in the order they are stored.
    pub const ALL: [Venue; 3] = [Venue::Village, Venue::Hub, Venue::Arena];

    /// Position in the block, 0..=2.
    pub fn index(self) -> usize {
        match self {
            Venue::Village => 0,
            Venue::Hub => 1,
            Venue::Arena => 2,
        }
    }

    /// Offset of this venue's array, relative to the card.
    pub fn offset(self) -> usize {
        GC_WEAPON + self.index() * WEAPON_VENUE_BYTES
    }
}

// ---- title -----------------------------------------------------------------
//
// The card keeps three ids: a word of GC_Title_1, a linking word of GC_Title_2
// (0 = none) and another word of GC_Title_1, so [4, 0, 6] is "かけだし" + "" +
// "ハンター" = "かけだしハンター", which is what the sample save holds. The same
// save has bits 4, 6 and 502 of the word unlock map set and bit 0 of the link
// map, i.e. the selected ids really are unlocked ones. Reference:
// core/src/guildcard.rs:13 (TITLE = CARD + 0x854) and docs/11-save-map.md "Card
// layout" (+0x854, 3 x u16, confirmed in game), plus the C# editor's fields 0-2
// of cUIOGuildCardEdit.
//
// The unlock maps live in the character slot, not on the card, each with a NEW
// copy the game sets at the same time:
//
//     0x2FC7  164 B   words, 1309 bits     0x306B  the NEW copy
//     0x310F   16 B   links,  121 bits     0x311F  the NEW copy
//
// 1309 bits need 164 bytes (0x2FC7 + 0xA4 = 0x306B) and 121 bits need 16 bytes
// (0x310F + 0x10 = 0x311F); both stay clear of the award maps at 0x3157/0x316B
// that awards already uses.

/// The card's title: three u16 ids, relative to the card.
pub const GC_TITLE: usize = 0x854;
/// How many ids a title has.
pub const TITLE_IDS: usize = 3;
/// Words in GC_Title_1 (the entries offered for the first and last field).
pub const TITLE_WORDS: usize = 1309;
/// Linking words in GC_Title_2; entry 0 is "(なし)".
pub const TITLE_LINKS: usize = 121;
/// Card scene (background), one byte.
pub const GC_SCENE: usize = 0x85A;
/// Card pose, one byte.
pub const GC_POSE: usize = 0x85B;
/// Word unlock map, relative to the slot base.
pub const GC_TITLE_WORD_MAP: usize = 0x2FC7;
/// The word map's NEW copy.
pub const GC_TITLE_WORD_MAP_NEW: usize = 0x306B;
/// Link unlock map, relative to the slot base.
pub const GC_TITLE_LINK_MAP: usize = 0x310F;
/// The link map's NEW copy.
pub const GC_TITLE_LINK_MAP_NEW: usize = 0x311F;

// ---- quest counters --------------------------------------------------------
//
// Seven u16 right behind the title: village low / high, hub low / high / G,
// special permit, arena. The card shows their sum capped at 99,999. Reference:
// docs/11-save-map.md +0x85E (7 x u16) and EditGuildCardDialog.cs:74-80, which
// reads 0x85E, 0x860, 0x862, 0x864, 0x866, 0x868 and 0x86A. The sample save
// holds 273, 106, 125, 230, 382, 233, 24 for all three slots.

/// Quest counters, relative to the card.
pub const GC_QUESTS: usize = 0x85E;
/// How many quest counters there are.
pub const QUEST_COUNTS: usize = 7;

/// What each of the seven counters counts, in the order they are stored.
///
/// The card draws one bar per counter and the sum above them; the two editors
/// print these words beside the numbers, so the two screens and the game say
/// the same thing.
pub const QUEST_COUNT_LABELS: [&str; QUEST_COUNTS] = [
    "村（下位）",
    "村（上位）",
    "集会所（下位）",
    "集会所（上位）",
    "集会所（G）",
    "特別許可",
    "闘技大会",
];

/// The name of one of the seven counters, or "?" past the end.
pub fn quest_count_label(index: usize) -> &'static str {
    QUEST_COUNT_LABELS.get(index).copied().unwrap_or("?")
}
/// The cap the game applies to the total it shows.
pub const QUEST_TOTAL_CAP: u32 = 99_999;

// ---- quest history ---------------------------------------------------------
//
// Ten records of 160 bytes, newest first, from card + 0x918 to card + 0xF58.
// Each record: day u8, month u8, year u16, kind u16, quest id u16, the quest
// name as UTF-16 (16 characters at +0x08), three highlight codes at +0x28, the
// party names as UTF-16 (22 bytes each from +0x44) and the party weapon bytes at
// +0x9C. Reference: docs/05-quests.md "Quest history log" and
// core/src/guildcard.rs:86-133. The sample save decodes to dates 2022-08-19..22
// with real quest names ("金と銀がもたらす悲哀", "千夜一夜の太古の閣", ...), kind
// 7/8/9, and the owner "ナナ" in party slot 0.

/// First history record, relative to the card.
pub const GC_HISTORY: usize = 0x918;
/// Records kept.
pub const HISTORY_RECORDS: usize = 10;
/// Bytes per record.
pub const HISTORY_RECORD: usize = 0xA0;
/// Kind written into a record that was never used; such a record is skipped.
pub const HISTORY_UNSET: u16 = 11;
/// Party members a record can name.
pub const PARTY_SLOTS: usize = 4;
/// Party weapon byte of an empty slot.
pub const PARTY_NONE: u8 = 0xFF;
/// Party weapon byte of a Palico (Prowler) instead of a hunter.
pub const PARTY_PALICO: u8 = 15;

// ---- received cards --------------------------------------------------------
//
// The 100 cards kept are a run of elements at slot + 0x2C6BD: u32 zlib length,
// the zlib stream of one 6,328 byte card, u32 state (3 = a card, 1 = empty) and
// a 36 byte trailer (u16 HR, 11 UTF-16 name, 8 byte owner id, u32 play time).
// The elements are packed and the list as a whole is sized for
// 100 x (0x18B8 + 8) = 633,600 bytes, i.e. exactly up to slot + 0xC71BD where
// the own card starts; the 100 info records (44 bytes each) follow the own card
// at slot + 0xC8A75 and end at slot + 0xC9BA5 where the 50 element inbox starts.
// Those four boundaries hit exactly in the sample save, and the walk decodes 24
// named cards (HR 193..999) with dates from 2021-11-14 on.
//
// The element size is variable (compressed data), so a reader has to walk from
// the front; the padding after the last element is what the game leaves there
// and must not be read as an element. Reference: core/src/cards.rs:20-60.
//
// Info record of a kept card (44 bytes): date (day u8, month u8, year u16),
// comment UTF-16 12 characters, unity u32 at +0x1C, card type at +0x20. The
// sample save's first record is 2021-11-14 with unity 76 and a zero comment.

/// First kept-card element, relative to the slot base.
pub const GC_CARDS: usize = 0x2C6BD;
/// Cards the kept list holds.
pub const CARD_LIST_LEN: usize = 100;
/// Info records of the kept list, relative to the slot base.
pub const GC_CARDS_INFO: usize = 0xC8A75;
/// Bytes per kept-list info record.
pub const CARD_INFO_LEN: usize = 44;
/// First inbox element, relative to the slot base.
pub const GC_INBOX: usize = 0xC9BA5;
/// Cards the inbox holds.
pub const INBOX_LIST_LEN: usize = 50;
/// Info records of the inbox, relative to the slot base.
pub const GC_INBOX_INFO: usize = 0x117125;
/// Bytes per inbox info record.
pub const INBOX_INFO_LEN: usize = 36;
/// Bytes of one inflated card (the same layout as the own card).
pub const CARD_DATA_LEN: usize = GC_SIZE;
/// Bytes the list accounting gives each element (data + 8).
pub const CARD_ELEMENT: usize = CARD_DATA_LEN + 8;
/// Bytes of the trailer behind an element.
pub const CARD_TRAILER: usize = 36;
/// State of an element that holds a card.
pub const CARD_FILLED: u32 = 3;
/// Characters of a card's comment in its info record.
pub const CARD_INFO_COMMENT_CHARS: usize = 12;

/// One arena result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArenaTime {
    /// Arena index, 0..16.
    pub arena: usize,
    /// Rank slot, 0 is the best.
    pub rank: usize,
    /// Best time in hundredths of a second, 0 when not recorded.
    pub centis: u16,
    /// The two trailing bytes, not yet understood.
    pub extra: [u8; 2],
}

impl ArenaTime {
    /// The time as minutes, seconds and hundredths.
    pub fn as_clock(&self) -> (u16, u8, u8) {
        let total = self.centis as u32;
        (
            (total / 6000) as u16,
            ((total / 100) % 60) as u8,
            (total % 100) as u8,
        )
    }

    /// Is this slot empty?
    pub fn is_empty(&self) -> bool {
        self.centis == 0
    }
}

/// One member of a history record's party.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartyMember {
    /// Name as stored (UTF-16, up to 11 characters).
    ///
    /// The game reuses one stack buffer for names, so the bytes behind a short
    /// name's terminator can hold the tail of an older name. They are ignored
    /// here, but a name can still be stale if the record's own bytes are.
    pub name: String,
    /// Weapon type in the weapon-usage order, PARTY_PALICO for a Palico.
    pub weapon: u8,
}

/// One entry of the card's quest history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRecord {
    /// Day of the clear.
    pub day: u8,
    /// Month of the clear, 1 based.
    pub month: u8,
    /// Year of the clear (the console clock).
    pub year: u16,
    /// What the record is about: 0 HR up, 1 MHGen transfer, 2 Welcome, 3 forged,
    /// 4/5 upgrade or forge counts, 6 all awards, 7/8/9 quest completed, failed or
    /// abandoned, 10 hired. HISTORY_UNSET never appears here.
    pub kind: u16,
    /// Quest id, or the value the kind uses (the HR of an HR-up record).
    pub quest_id: u16,
    /// Quest name as stored, up to 16 characters.
    pub name: String,
    /// The three highlight codes, 36 = empty.
    pub highlights: [u8; 3],
    /// Party members in slot order, empty slots left out.
    pub party: Vec<PartyMember>,
}

/// A title, as three table indices.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Title {
    /// Word of GC_Title_1; 0 is "(なし)".
    pub first: u16,
    /// Linking word of GC_Title_2; 0 is "(なし)", i.e. no connector.
    pub link: u16,
    /// Second word of GC_Title_1; 0 is "(なし)".
    pub last: u16,
}

/// The title text tables, built from the two romfs .gmd files.
///
/// The names are not in this repository: GC_Title_1 holds 1309 words followed by
/// their 1309 descriptions, GC_Title_2 holds 121 linking words. A table built
/// from anything else simply has no name for the higher ids.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TitleTable {
    words: Vec<String>,
    links: Vec<String>,
}

impl TitleTable {
    /// Build the table from the raw bytes of GC_Title_1 and GC_Title_2.
    pub fn from_gmd(words: &[u8], links: &[u8]) -> Self {
        let mut words = gmd_strings(words);
        words.truncate(TITLE_WORDS);
        let mut links = gmd_strings(links);
        links.truncate(TITLE_LINKS);
        Self { words, links }
    }

    /// A word of GC_Title_1.
    pub fn word(&self, index: usize) -> Option<&str> {
        self.words.get(index).map(|s| s.as_str())
    }

    /// A linking word of GC_Title_2.
    pub fn link(&self, index: usize) -> Option<&str> {
        self.links.get(index).map(|s| s.as_str())
    }

    /// Is the table unusable (no file, or not a .gmd)?
    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.links.is_empty()
    }

    /// The title written out: word, connector, word. Id 0 is "(なし)" in both
    /// tables and is left out, so [4, 0, 6] is "かけだしハンター".
    pub fn text(&self, title: Title) -> String {
        let mut out = String::new();
        if title.first != 0 {
            if let Some(w) = self.word(title.first as usize) {
                out.push_str(w);
            }
        }
        if title.link != 0 {
            if let Some(l) = self.link(title.link as usize) {
                out.push_str(l);
            }
        }
        if title.last != 0 {
            if let Some(w) = self.word(title.last as usize) {
                out.push_str(w);
            }
        }
        out
    }
}

/// Which list of received cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardList {
    /// The cards kept (card list), 100.
    Stored,
    /// The courier's inbox, 50.
    Inbox,
}

impl CardList {
    /// (first element, elements, info records, info record size), relative to
    /// the slot base.
    pub fn at(self) -> (usize, usize, usize, usize) {
        match self {
            CardList::Stored => (GC_CARDS, CARD_LIST_LEN, GC_CARDS_INFO, CARD_INFO_LEN),
            CardList::Inbox => (GC_INBOX, INBOX_LIST_LEN, GC_INBOX_INFO, INBOX_INFO_LEN),
        }
    }

    /// How many elements the list holds.
    pub fn count(self) -> usize {
        self.at().1
    }

    /// Bytes per info record.
    pub fn info_len(self) -> usize {
        self.at().3
    }

    /// Bytes the whole list is sized for, padding included.
    pub fn size(self) -> usize {
        self.count() * CARD_ELEMENT
    }
}

/// One card another hunter sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunterCard {
    /// Which list it sits in.
    pub list: CardList,
    /// Its index in that list.
    pub slot: usize,
    /// Hunter rank from the trailer.
    pub hunter_rank: u16,
    /// Name from the trailer (UTF-16, up to 11 characters).
    pub name: String,
    /// The owner's 8 byte card id.
    pub id: [u8; 8],
    /// Play time of the owner, in seconds. A card received again replaces the
    /// kept one only with more.
    pub play_time: u32,
    /// Date the card was received (kept list) or updated (inbox).
    pub date: (u8, u8, u16),
    /// Unity with the owner, 76 per quest together; 0 in the inbox.
    pub unity: u32,
    /// Comment the receiver added; empty in the inbox.
    pub comment: String,
    /// Card type (0..=23 for the game's messages, 25 street pass); None in the
    /// inbox.
    pub kind: Option<u8>,
    /// Compressed length of the element, for diagnostics.
    pub compressed: usize,
}

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

/// A little endian u16, or None when the slice is too short.
fn u16_le(b: &[u8], off: usize) -> Option<u16> {
    let end = off.checked_add(2)?;
    let s = b.get(off..end)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

/// A little endian u32, or None when the slice is too short.
fn u32_le(b: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    let s = b.get(off..end)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// A length prefixed UTF-16LE string: up to units code units, up to the NUL.
fn utf16(b: &[u8], units: usize) -> String {
    let mut v: Vec<u16> = Vec::new();
    for i in 0..units {
        let Some(c) = u16_le(b, i * 2) else { break };
        if c == 0 {
            break;
        }
        v.push(c);
    }
    // Lone surrogates (stale bytes behind a short name, a damaged record) turn
    // into U+FFFD instead of panicking.
    String::from_utf16_lossy(&v)
}

/// One bit, LSB first.
fn bit_on(bytes: &[u8], index: usize) -> bool {
    match bytes.get(index >> 3) {
        Some(b) => b & (1 << (index & 7)) != 0,
        None => false,
    }
}

/// Read the arena log of one character.
pub fn arena(save: &Save, slot: usize) -> Vec<ArenaTime> {
    let mut out = Vec::new();
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return out;
    };
    let at = base + OFF_ARENA;
    for arena in 0..ARENA_COUNT {
        for rank in 0..ARENA_RANKS {
            let o = at + (arena * ARENA_RANKS + rank) * ARENA_ENTRY;
            let Some(e) = save.body().get(o..o + ARENA_ENTRY) else {
                return out;
            };
            out.push(ArenaTime {
                arena,
                rank,
                centis: u16_at(e, 0),
                extra: [e[2], e[3]],
            });
        }
    }
    out
}

/// The whole 6,328 byte card of one character.
///
/// Unlike Save::guild_card this does not stop at the arena log, so the fields
/// behind it (arena partners, and the received lists that follow) line up. The
/// MHGU layout is not verified.
pub fn full_card(save: &Save, slot: usize) -> Option<&[u8]> {
    let base = save.slot_base(slot)? as usize;
    let start = base.checked_add(OFF_GUILD_CARD)?;
    save.body().get(start..start.checked_add(GC_SIZE)?)
}

/// The hunter name a card block carries, decoded from UTF-16.
pub fn name_in(card: &[u8]) -> String {
    match card.get(GC_NAME..GC_NAME + GC_NAME_UNITS * 2) {
        Some(b) => utf16(b, GC_NAME_UNITS),
        None => String::new(),
    }
}

/// The hunter name the card carries.
pub fn card_name(save: &Save, slot: usize) -> Option<String> {
    Some(name_in(full_card(save, slot)?))
}

/// Check a card name against the eleven unit field, without writing.
///
/// A line break is allowed - the game draws it as a second line and the field
/// stores it as the unit 0x000A. Any other control character is refused with
/// [`NameError::Control`]; a name longer than eleven units is passed but
/// flagged with [`crate::save::NameWarning::TooLong`].
pub fn name_check(text: &str) -> Result<NameFit, NameError> {
    check_name(text, NAME_CAPACITY, NameUnit::Utf16)
}

/// The lines of a card block's name, as the screen draws them.
///
/// The card stores the name as UTF-16, so this decodes first and then splits;
/// every line is owned because the decode allocates.
pub fn name_lines_in(card: &[u8]) -> Vec<String> {
    let name = name_in(card);
    name.split('\n').map(String::from).collect()
}

/// How many lines a card block's name takes on screen.
pub fn name_line_count_in(card: &[u8]) -> usize {
    crate::save::name_line_count(&name_in(card))
}

/// The card name of one character, split into the lines the screen draws.
pub fn card_name_lines(save: &Save, slot: usize) -> Option<Vec<String>> {
    Some(name_lines_in(full_card(save, slot)?))
}

/// How many lines the card name of one character takes on screen.
pub fn card_name_line_count(save: &Save, slot: usize) -> Option<usize> {
    Some(name_line_count_in(full_card(save, slot)?))
}

/// The hunter rank stored on the card.
pub fn hunter_rank(save: &Save, slot: usize) -> Option<u16> {
    let base = save.slot_base(slot)? as usize;
    let b = save.body().get(base + OFF_GUILD_CARD + GC_HR..base + OFF_GUILD_CARD + GC_HR + 2)?;
    Some(u16_at(b, 0))
}

/// The card's own play time in seconds.
pub fn play_time(save: &Save, slot: usize) -> Option<u32> {
    let base = save.slot_base(slot)? as usize;
    let o = base + OFF_GUILD_CARD + GC_PLAY_TIME;
    let b = save.body().get(o..o + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// The card ID, eight raw bytes.
pub fn card_id(save: &Save, slot: usize) -> Option<[u8; 8]> {
    let base = save.slot_base(slot)? as usize;
    let o = base + OFF_GUILD_CARD + GC_ID;
    let b = save.body().get(o..o + 8)?;
    let mut out = [0u8; 8];
    out.copy_from_slice(b);
    Some(out)
}

/// The profile comment of a card block, decoded from UTF-16.
pub fn comment_in(card: &[u8]) -> String {
    let Some(b) = card.get(GC_COMMENT..GC_COMMENT + GC_COMMENT_CHARS * 2) else {
        return String::new();
    };
    utf16(b, GC_COMMENT_CHARS)
}

/// The profile comment, decoded from UTF-16.
pub fn comment(save: &Save, slot: usize) -> Option<String> {
    Some(comment_in(full_card(save, slot)?))
}

/// Check a comment against the twenty-eight unit field, without writing.
///
/// Same rule as [`name_check`]: a line break goes in, every other control
/// character is refused, and a comment that does not fit is flagged rather than
/// silently dropped.
pub fn comment_check(text: &str) -> Result<NameFit, NameError> {
    check_name(text, COMMENT_CAPACITY, NameUnit::Utf16)
}

/// The lines of a card block's comment, as the screen draws them.
pub fn comment_lines_in(card: &[u8]) -> Vec<String> {
    let text = comment_in(card);
    text.split('\n').map(String::from).collect()
}

/// How many lines a card block's comment takes on screen.
pub fn comment_line_count_in(card: &[u8]) -> usize {
    crate::save::name_line_count(&comment_in(card))
}

/// The comment of one character, split into the lines the screen draws.
pub fn comment_lines(save: &Save, slot: usize) -> Option<Vec<String>> {
    Some(comment_lines_in(full_card(save, slot)?))
}

/// How many lines the comment of one character takes on screen.
pub fn comment_line_count(save: &Save, slot: usize) -> Option<usize> {
    Some(comment_line_count_in(full_card(save, slot)?))
}

// ---- weapon usage ----------------------------------------------------------

/// Usage of every weapon type in one venue, 15 entries.
pub fn venue_usage_in(card: &[u8], venue: Venue) -> Vec<u32> {
    let at = venue.offset();
    (0..WEAPON_TYPES)
        .map(|w| u16_le(card, at + w * 2).unwrap_or(0) as u32)
        .collect()
}

/// Usage of every weapon type in one venue, 15 entries.
pub fn venue_usage(save: &Save, slot: usize, venue: Venue) -> Vec<u32> {
    match full_card(save, slot) {
        Some(card) => venue_usage_in(card, venue),
        None => Vec::new(),
    }
}

/// Usage of every weapon type over all three venues, 15 entries.
///
/// This is the number the card's "All" graph shows: the game adds the three
/// arrays up at display time and stores no total.
pub fn weapon_usage_in(card: &[u8]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    out.resize(WEAPON_TYPES, 0);
    for venue in Venue::ALL {
        let at = venue.offset();
        for (w, total) in out.iter_mut().enumerate() {
            *total += u16_le(card, at + w * 2).unwrap_or(0) as u32;
        }
    }
    out
}

/// Usage of every weapon type over all three venues, 15 entries.
pub fn weapon_usage(save: &Save, slot: usize) -> Vec<u32> {
    match full_card(save, slot) {
        Some(card) => weapon_usage_in(card),
        None => Vec::new(),
    }
}

/// Rewrite one weapon type's count in one venue.
///
/// There is no single "total" setter on purpose: the game stores three
/// independent arrays and the card only adds them up when it draws the graph,
/// so writing a total would mean inventing a split between the venues.
pub fn set_weapon_usage(
    save: &mut Save,
    slot: usize,
    venue: Venue,
    weapon: usize,
    count: u16,
) -> bool {
    if weapon >= WEAPON_TYPES {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let o = base + OFF_GUILD_CARD + venue.offset() + weapon * 2;
    let Some(dst) = save.body_mut().get_mut(o..o + 2) else {
        return false;
    };
    dst.copy_from_slice(&count.to_le_bytes());
    true
}

// ---- title -----------------------------------------------------------------

/// The title stored on a card block.
pub fn title_in(card: &[u8]) -> Option<Title> {
    Some(Title {
        first: u16_le(card, GC_TITLE)?,
        link: u16_le(card, GC_TITLE + 2)?,
        last: u16_le(card, GC_TITLE + 4)?,
    })
}

/// The title stored on the card.
pub fn title(save: &Save, slot: usize) -> Option<Title> {
    title_in(full_card(save, slot)?)
}

/// Rewrite the title. The ids are written as given; the caller decides which
/// unlocked entry to use (see set_title_word_unlocked).
pub fn set_title(save: &mut Save, slot: usize, title: Title) -> bool {
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let o = base + OFF_GUILD_CARD + GC_TITLE;
    let Some(dst) = save.body_mut().get_mut(o..o + TITLE_IDS * 2) else {
        return false;
    };
    dst[0..2].copy_from_slice(&title.first.to_le_bytes());
    dst[2..4].copy_from_slice(&title.link.to_le_bytes());
    dst[4..6].copy_from_slice(&title.last.to_le_bytes());
    true
}

/// The card scene (background) and pose.
pub fn scene_and_pose(save: &Save, slot: usize) -> Option<(u8, u8)> {
    let card = full_card(save, slot)?;
    Some((*card.get(GC_SCENE)?, *card.get(GC_POSE)?))
}

/// Bytes one unlock map occupies for the given number of entries.
pub const fn map_bytes(entries: usize) -> usize {
    entries.div_ceil(8)
}

fn map(save: &Save, slot: usize, off: usize, entries: usize) -> Option<&[u8]> {
    let base = save.slot_base(slot)? as usize;
    let start = base.checked_add(off)?;
    save.body().get(start..start.checked_add(map_bytes(entries))?)
}

/// Is title word word unlocked for this character?
pub fn title_word_unlocked(save: &Save, slot: usize, word: usize) -> bool {
    if word >= TITLE_WORDS {
        return false;
    }
    map(save, slot, GC_TITLE_WORD_MAP, TITLE_WORDS).is_some_and(|b| bit_on(b, word))
}

/// Is linking word link unlocked for this character?
pub fn title_link_unlocked(save: &Save, slot: usize, link: usize) -> bool {
    if link >= TITLE_LINKS {
        return false;
    }
    map(save, slot, GC_TITLE_LINK_MAP, TITLE_LINKS).is_some_and(|b| bit_on(b, link))
}

/// Every unlocked word, smallest id first.
pub fn unlocked_title_words(save: &Save, slot: usize) -> Vec<u16> {
    let Some(b) = map(save, slot, GC_TITLE_WORD_MAP, TITLE_WORDS) else {
        return Vec::new();
    };
    (0..TITLE_WORDS)
        .filter(|&w| bit_on(b, w))
        .map(|w| w as u16)
        .collect()
}

/// Every unlocked linking word, smallest id first.
pub fn unlocked_title_links(save: &Save, slot: usize) -> Vec<u16> {
    let Some(b) = map(save, slot, GC_TITLE_LINK_MAP, TITLE_LINKS) else {
        return Vec::new();
    };
    (0..TITLE_LINKS)
        .filter(|&w| bit_on(b, w))
        .map(|w| w as u16)
        .collect()
}

/// Set or clear one word's unlock bit, and its NEW copy, as the game does.
pub fn set_title_word_unlocked(save: &mut Save, slot: usize, word: usize, value: bool) -> bool {
    if word >= TITLE_WORDS {
        return false;
    }
    set_unlock(
        save,
        slot,
        GC_TITLE_WORD_MAP,
        GC_TITLE_WORD_MAP_NEW,
        word,
        value,
    )
}

/// Set or clear one linking word's unlock bit, and its NEW copy.
pub fn set_title_link_unlocked(save: &mut Save, slot: usize, link: usize, value: bool) -> bool {
    if link >= TITLE_LINKS {
        return false;
    }
    set_unlock(
        save,
        slot,
        GC_TITLE_LINK_MAP,
        GC_TITLE_LINK_MAP_NEW,
        link,
        value,
    )
}

/// Set or clear one bit in an unlock map and in its NEW copy.
fn set_unlock(
    save: &mut Save,
    slot: usize,
    on: usize,
    new: usize,
    index: usize,
    value: bool,
) -> bool {
    let Some(base) = save.slot_base(slot) else {
        return false;
    };
    let mut ok = true;
    for off in [on, new] {
        let target = (base as usize).checked_add(off).and_then(|o| o.checked_add(index >> 3));
        let Some(byte) = target.and_then(|o| save.body_mut().get_mut(o)) else {
            ok = false;
            continue;
        };
        let mask = 1u8 << (index & 7);
        if value {
            *byte |= mask;
        } else {
            *byte &= !mask;
        }
    }
    ok
}

// ---- quest counters --------------------------------------------------------

/// The card's seven quest counters.
pub fn quest_counts_in(card: &[u8]) -> Vec<u32> {
    (0..QUEST_COUNTS)
        .map(|k| u16_le(card, GC_QUESTS + k * 2).unwrap_or(0) as u32)
        .collect()
}

/// The card's seven quest counters: village low / high, hub low / high / G,
/// special permit, arena.
pub fn quest_counts(save: &Save, slot: usize) -> Vec<u32> {
    match full_card(save, slot) {
        Some(card) => quest_counts_in(card),
        None => Vec::new(),
    }
}

/// The total the card shows, capped like the game caps it.
pub fn quest_total(save: &Save, slot: usize) -> u32 {
    let total: u32 = quest_counts(save, slot).iter().sum();
    total.min(QUEST_TOTAL_CAP)
}

/// Rewrite one of the card's seven quest counters.
///
/// The field is the u16 [quest_counts_in] reads, so a value above 65,535 is
/// clamped rather than wrapped and a kind past the seven counters is refused.
/// The card's own total is not stored: the game adds the seven up when it
/// draws the card and caps that sum, which [quest_total] repeats.
pub fn set_quest_count(save: &mut Save, slot: usize, kind: usize, value: u32) -> bool {
    if kind >= QUEST_COUNTS {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let o = base + OFF_GUILD_CARD + GC_QUESTS + kind * 2;
    let Some(dst) = save.body_mut().get_mut(o..o + 2) else {
        return false;
    };
    dst.copy_from_slice(&(value.min(u16::MAX as u32) as u16).to_le_bytes());
    true
}

// ---- quest history ---------------------------------------------------------

fn history_records(card: &[u8]) -> Vec<HistoryRecord> {
    let mut out = Vec::new();
    for k in 0..HISTORY_RECORDS {
        let o = GC_HISTORY + k * HISTORY_RECORD;
        let Some(r) = card.get(o..o + HISTORY_RECORD) else {
            break;
        };
        let kind = u16_at(r, 4);
        // The constructor leaves 11 in a record nobody wrote and the date zero
        // in one that was cleared; both are skipped instead of shown as
        // "1 January year 0".
        if kind == HISTORY_UNSET || r[..4] == [0; 4] {
            continue;
        }
        let mut party = Vec::new();
        for p in 0..PARTY_SLOTS {
            let Some(weapon) = r.get(0x9C + p).copied() else {
                break;
            };
            if weapon == PARTY_NONE {
                continue;
            }
            let start = 0x44 + 22 * p;
            let name = match r.get(start..start + 22) {
                Some(b) => utf16(b, 11),
                None => String::new(),
            };
            party.push(PartyMember { name, weapon });
        }
        out.push(HistoryRecord {
            day: r[0],
            month: r[1],
            year: u16_at(r, 2),
            kind,
            quest_id: u16_at(r, 6),
            name: utf16(&r[8..0x28], 16),
            highlights: [r[0x28], r[0x29], r[0x2A]],
            party,
        });
    }
    out
}

/// The card's quest history, newest first, with unused records left out.
pub fn history_in(card: &[u8]) -> Vec<HistoryRecord> {
    history_records(card)
}

/// The card's quest history, newest first, with unused records left out.
pub fn history(save: &Save, slot: usize) -> Vec<HistoryRecord> {
    match full_card(save, slot) {
        Some(card) => history_records(card),
        None => Vec::new(),
    }
}

// ---- received cards --------------------------------------------------------

/// One element of a received-card list, as walked from the front.
#[derive(Debug, Clone, Copy)]
struct Element {
    /// Offset of the element in the body.
    at: usize,
    /// Length of the compressed card.
    len: usize,
    /// The u32 state behind the stream.
    state: u32,
}

/// Walk a list from its first element.
///
/// The element length varies, so the only way in is from the front. A length
/// that cannot fit (beyond one card, or past the end of the list) means the
/// bytes are not a list the game wrote, and the walk stops there. None when the
/// list does not fit in the body at all.
fn walk(save: &Save, slot: usize, list: CardList) -> Option<Vec<Element>> {
    let base = save.slot_base(slot)? as usize;
    let (at, n, _, _) = list.at();
    let start = base.checked_add(at)?;
    let end = start.checked_add(list.size())?;
    if end > save.body().len() {
        return None;
    }
    let body = save.body();
    let mut out = Vec::new();
    let mut p = start;
    for _ in 0..n {
        let len = u32_le(body, p)? as usize;
        let tail = p
            .checked_add(8)?
            .checked_add(len)?
            .checked_add(CARD_TRAILER)?;
        if len > CARD_DATA_LEN || tail > end {
            return Some(out);
        }
        out.push(Element {
            at: p,
            len,
            state: u32_le(body, p + 4 + len)?,
        });
        p = tail;
    }
    Some(out)
}

/// The name, rank, id and play time a trailer holds.
fn trailer(t: &[u8]) -> (u16, String, [u8; 8], u32) {
    let hr = u16_le(t, 0).unwrap_or(0);
    let name = utf16(t.get(2..24).unwrap_or(&[]), 11);
    let mut id = [0u8; 8];
    if let Some(b) = t.get(24..32) {
        id.copy_from_slice(b);
    }
    let play = u32_le(t, 32).unwrap_or(0);
    (hr, name, id, play)
}

/// Every card in one list, in slot order.
///
/// Only the trailer and the info record are read; the compressed card itself
/// stays compressed. Use card_data to inflate one.
pub fn cards(save: &Save, slot: usize, list: CardList) -> Vec<HunterCard> {
    let Some(elements) = walk(save, slot, list) else {
        return Vec::new();
    };
    let Some(base) = save.slot_base(slot) else {
        return Vec::new();
    };
    let base = base as usize;
    let (_, _, info, info_len) = list.at();
    let body = save.body();
    let mut out = Vec::new();
    for (index, e) in elements.iter().enumerate() {
        if e.state != CARD_FILLED {
            continue;
        }
        let Some(t) = body.get(e.at + 8 + e.len..e.at + 8 + e.len + CARD_TRAILER) else {
            continue;
        };
        let (hunter_rank, name, id, play_time) = trailer(t);
        let start = base + info + info_len * index;
        let inf = body.get(start..start + info_len).unwrap_or(&[]);
        let date = (
            inf.first().copied().unwrap_or(0),
            inf.get(1).copied().unwrap_or(0),
            u16_le(inf, 2).unwrap_or(0),
        );
        let (unity, kind) = if list == CardList::Stored {
            (u32_le(inf, 0x1C).unwrap_or(0), inf.get(0x20).copied())
        } else {
            (0, None)
        };
        let comment = if list == CardList::Stored {
            match inf.get(4..4 + CARD_INFO_COMMENT_CHARS * 2) {
                Some(b) => utf16(b, CARD_INFO_COMMENT_CHARS),
                None => String::new(),
            }
        } else {
            String::new()
        };
        out.push(HunterCard {
            list,
            slot: index,
            hunter_rank,
            name,
            id,
            play_time,
            date,
            unity,
            comment,
            kind,
            compressed: e.len,
        });
    }
    out
}

/// The cards kept (card list).
pub fn hunter_cards(save: &Save, slot: usize) -> Vec<HunterCard> {
    cards(save, slot, CardList::Stored)
}

/// The cards in the courier's inbox.
pub fn card_inbox(save: &Save, slot: usize) -> Vec<HunterCard> {
    cards(save, slot, CardList::Inbox)
}

/// Inflate one card, 6,328 bytes with the same layout as the own card.
///
/// slot is the element index; use cards to find which indices hold a card. The
/// returned bytes can be fed to title_in, comment_in, weapon_usage_in,
/// quest_counts_in and history_in.
pub fn card_data(save: &Save, slot: usize, list: CardList, index: usize) -> Option<Vec<u8>> {
    let elements = walk(save, slot, list)?;
    let e = elements.get(index)?;
    if e.state != CARD_FILLED {
        return None;
    }
    let z = save.body().get(e.at + 4..e.at + 4 + e.len)?;
    let mut data = miniz_oxide::inflate::decompress_to_vec_zlib(z).unwrap_or_default();
    if data.is_empty() {
        return None;
    }
    data.resize(CARD_DATA_LEN, 0);
    Some(data)
}

/// Rewrite the card's hunter rank.
pub fn set_hunter_rank(save: &mut Save, slot: usize, rank: u16) -> bool {
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let o = base + OFF_GUILD_CARD + GC_HR;
    let Some(b) = save.body_mut().get_mut(o..o + 2) else {
        return false;
    };
    b.copy_from_slice(&rank.to_le_bytes());
    true
}

/// Rewrite the card's own play time, in seconds.
pub fn set_play_time(save: &mut Save, slot: usize, seconds: u32) -> bool {
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let o = base + OFF_GUILD_CARD + GC_PLAY_TIME;
    let Some(b) = save.body_mut().get_mut(o..o + 4) else {
        return false;
    };
    b.copy_from_slice(&seconds.to_le_bytes());
    true
}

/// Rewrite the profile comment. Longer text is truncated to the field.
///
/// The field is UTF-16, so a line break is stored as the unit 0x000A and the
/// game draws it as a second line. Any other control character is refused, so
/// false means the card is untouched.
pub fn set_comment(save: &mut Save, slot: usize, text: &str) -> bool {
    if comment_check(text).is_err() {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let o = base + OFF_GUILD_CARD + GC_COMMENT;
    let Some(dst) = save.body_mut().get_mut(o..o + GC_COMMENT_CHARS * 2) else {
        return false;
    };
    dst.fill(0);
    for (i, ch) in text.chars().take(GC_COMMENT_CHARS).enumerate() {
        let v = ch as u32;
        // Anything outside the basic plane cannot fit in a single unit.
        let unit = if v <= 0xFFFF { v as u16 } else { 0x30FB };
        dst[i * 2..i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    true
}

/// Rewrite the profile comment, refusing control characters.
///
/// The checked form of [`set_comment`]. Both fields are UTF-16, so a line
/// break is stored as the unit 0x000A and the decoder hands it back as '\n';
/// every other control character makes the call fail without touching the
/// card. A comment that does not fit comes back flagged, cut to twenty-eight
/// units on a character boundary.
pub fn set_comment_checked(save: &mut Save, slot: usize, text: &str) -> Result<NameFit, NameError> {
    let fit = comment_check(text)?;
    if !set_comment(save, slot, text) {
        return Err(NameError::NoField);
    }
    Ok(fit)
}

/// Rewrite the hunter name on the card.
///
/// Only the characters and the terminator are written. The field is eleven
/// units wide and the sample save really does keep leftover bytes behind the
/// terminator (GC_NAME lists them), so zeroing the rest would rewrite bytes
/// nobody asked to change. Text shorter than eleven units gets a NUL behind it;
/// text that fills the field gets none, and longer text is truncated.
///
/// A line break is written as the unit 0x000A, which is the second line the
/// game draws. Any other control character is refused, so false means the card
/// is untouched; [`set_card_name_checked`] reports the same write with the
/// warnings a caller may want to show.
pub fn set_card_name(save: &mut Save, slot: usize, text: &str) -> bool {
    if name_check(text).is_err() {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let o = base + OFF_GUILD_CARD + GC_NAME;
    let Some(dst) = save.body_mut().get_mut(o..o + GC_NAME_UNITS * 2) else {
        return false;
    };
    let mut written = 0usize;
    for ch in text.chars() {
        if written >= GC_NAME_UNITS {
            break;
        }
        let v = ch as u32;
        // The game's name entry is BMP only; anything outside it cannot fit in
        // one unit, so it becomes the same placeholder set_comment uses.
        let unit = if v <= 0xFFFF { v as u16 } else { 0x30FB };
        dst[written * 2..written * 2 + 2].copy_from_slice(&unit.to_le_bytes());
        written += 1;
    }
    // The terminator, and nothing behind it.
    if written < GC_NAME_UNITS {
        dst[written * 2..written * 2 + 2].copy_from_slice(&0u16.to_le_bytes());
    }
    true
}

/// Rewrite the hunter name on the card, refusing control characters.
///
/// The checked form of [`set_card_name`]. A line break is stored as the unit
/// 0x000A and survives the UTF-16 decode as '\n', which is the second line the
/// game draws; any other control character fails the call and leaves the card
/// alone.
pub fn set_card_name_checked(save: &mut Save, slot: usize, text: &str) -> Result<NameFit, NameError> {
    let fit = name_check(text)?;
    if !set_card_name(save, slot, text) {
        return Err(NameError::NoField);
    }
    Ok(fit)
}

/// Rewrite one arena entry's time, in hundredths of a second.
pub fn set_arena_time(
    save: &mut Save,
    slot: usize,
    arena: usize,
    rank: usize,
    centis: u16,
) -> bool {
    if arena >= ARENA_COUNT || rank >= ARENA_RANKS {
        return false;
    }
    let Some(base) = save.slot_base(slot).map(|b| b as usize) else {
        return false;
    };
    let o = base + OFF_ARENA + (arena * ARENA_RANKS + rank) * ARENA_ENTRY;
    let Some(dst) = save.body_mut().get_mut(o..o + 2) else {
        return false;
    };
    dst.copy_from_slice(&centis.to_le_bytes());
    true
}
