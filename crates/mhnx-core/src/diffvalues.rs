//! Value level differences: what a write changes, field by field.
//!
//! The byte ranges in [crate::hbedit] answer *where* a write moves bytes; this
//! module answers *what* they became. Every value is read back through an
//! accessor mhnx-core already had for the matching screen, so nothing is decoded
//! twice and an area with no getter (定型文, 受信カード一覧, アンロック, ...)
//! simply keeps its byte range and contributes no line here.
//!
//! The walk is gated on the ranges: a getter only runs for the part whose bytes
//! the ranges say moved. Without that gate a change in one tab would pick up a
//! line from every other getter that happens to read past its own records.
//!
//! The console build shares this module: [crate::hbedit::SaveReview] carries
//! these values, so the two-press review on the Switch says "何から何に" the
//! same way the PC dialog does. The PC dialog had its own copy of this walk
//! before it lived here; moving it into the library is what lets both builds
//! print the same lines.
//!
//! ## What is covered
//!
//! The body wide parts (options, slot flags, download palicoes, download
//! quests), every character slot (character sheet, guild card, arena log,
//! equipment box, item box, the pouch, item and equipment sets, palico roster,
//! hunt records, palico equipment box, quest progress, awards) and the 45
//! quest-equipment loadouts the DLC section carries. The pouch was the last
//! area an audit found "editable, but the review only shows bytes"; its getter
//! ([`myset::pouch`]) was already there, so it is read the same way as
//! everything else. 狩猟記録, オトモ装備ボックス and クエスト装備 had the same
//! fault and got their walks the same way.

use alloc::borrow::ToOwned;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::awards;
use crate::charm;
use crate::dlc::{self, QuestEquipItem, QuestEquipList};
use crate::equip;
use crate::guildcard::{self, Title, TitleTable};
use crate::hbedit::DiffRange;
use crate::hunts;
use crate::itembox::ItemSlot;
use crate::myset;
use crate::palico::{self, Palico};
use crate::palicogear;
use crate::questprogress::{self, Progress};
use crate::requests;
use crate::save::{Save, MAX_SLOTS};

/// How many value lines a review prints before it counts the rest.
///
/// The reference editor lists every target; a save-wide edit here can move
/// thousands of bits (a bulk quest unlock), and a dialog taller than the window
/// helps nobody. Thirty lines is about a screen of values on the PC; the console
/// review draws as many as its panel holds and says how many it left out.
pub const VALUE_SHOWN: usize = 30;

/// One value that differs between the two sides of a review or a comparison.
///
/// The line reads "スロット1 名前: アイルー → アイルー2".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueChange {
    /// Character slot it belongs to, None for the body-wide parts.
    pub slot: Option<usize>,
    /// What the value is called, e.g. "装備ボックス #12 レベル".
    pub field: String,
    /// What it was.
    pub old: String,
    /// What it became.
    pub new: String,
}

impl ValueChange {
    /// The line the review and the comparison print for this value.
    pub fn line(&self) -> String {
        let head = match self.slot {
            Some(i) => format!("スロット{} {}", i + 1, self.field),
            None => self.field.clone(),
        };
        let word = |value: &str| {
            if value.is_empty() {
                String::from("（空）")
            } else {
                value.to_owned()
            }
        };
        format!("{}: {} → {}", head, word(&self.old), word(&self.new))
    }
}

/// Push a value line, but only when the two sides really differ.
///
/// A field that did not change would otherwise bury the ones that did: only the
/// write target's differences are listed, the way the byte ranges do it.
fn differing(
    out: &mut Vec<ValueChange>,
    slot: Option<usize>,
    field: impl Into<String>,
    old: String,
    new: String,
) {
    if old != new {
        out.push(ValueChange {
            slot,
            field: field.into(),
            old,
            new,
        });
    }
}

/// Every value the ranges say a write would move, in file order.
///
/// titles spells the 称号 out when the romfs tables are loaded; without them the
/// three ids are shown, which is still the value that gets written.
pub fn diff_values(
    ranges: &[DiffRange],
    old: &Save,
    new: &Save,
    titles: Option<&TitleTable>,
) -> Vec<ValueChange> {
    let mut out: Vec<ValueChange> = Vec::new();
    if ranges.iter().any(|range| range.slot.is_none()) {
        body_values(ranges, old, new, &mut out);
    }
    let mut slots: Vec<usize> = ranges.iter().filter_map(|range| range.slot).collect();
    slots.sort_unstable();
    slots.dedup();
    for slot in slots {
        slot_values(ranges, old, new, slot, titles, &mut out);
    }
    // The request flags are the one slot value with no layout entry of their
    // own: they sit between the named parts and are reported as その他, a range
    // that does not name the slot it came from. So when such a range moved,
    // every slot is asked, and the getters decide which one really changed.
    if touched(ranges, None, &["その他"]) {
        for slot in 0..MAX_SLOTS {
            request_values(old, new, slot, &mut out);
        }
    }
    out
}

/// Does a range name this part of this slot?
fn touched(ranges: &[DiffRange], slot: Option<usize>, names: &[&str]) -> bool {
    ranges
        .iter()
        .any(|range| range.slot == slot && names.contains(&range.name))
}

/// The body wide values: options, slot flags, download palicoes, download
/// quests and the quest-equipment loadouts that live in the same section.
fn body_values(ranges: &[DiffRange], old: &Save, new: &Save, out: &mut Vec<ValueChange>) {
    let (a, b) = (old.options(), new.options());
    differing(out, None, "オプション 明るさ", a.brightness.to_string(), b.brightness.to_string());
    differing(out, None, "オプション 振動", a.rumble.to_string(), b.rumble.to_string());
    differing(out, None, "オプション 3D 無効", a.disable_3d.to_string(), b.disable_3d.to_string());
    differing(
        out,
        None,
        "オプション 拡張スライドパッド",
        a.circle_pad_pro.to_string(),
        b.circle_pad_pro.to_string(),
    );
    for slot in 0..MAX_SLOTS {
        differing(
            out,
            None,
            format!("スロット使用フラグ スロット{}", slot + 1),
            on_word(old.slot_used(slot)),
            on_word(new.slot_used(slot)),
        );
    }
    let (a, b) = (old.dlc_palicoes(), new.dlc_palicoes());
    for (x, y) in a.iter().zip(b.iter()) {
        differing(
            out,
            None,
            format!("DLC オトモ #{}", x.index + 1),
            dlc_palico_line(x.id, &x.name),
            dlc_palico_line(y.id, &y.name),
        );
    }
    // The download section is the one expensive body area to walk, so it is only
    // read when a range says the section really moved: the quest payloads are
    // compared byte for byte (a quest that changed at the same size is still a
    // change) and the 45 quest-equipment loadouts are read entry by entry.
    if !touched(ranges, None, &["DLC セクション"]) {
        return;
    }
    dlc_quest_values(old, new, out);
    quest_equipment_values(old, new, out);
}

/// The download quests, named by their id: the value a replacement keeps.
fn dlc_quest_values(old: &Save, new: &Save, out: &mut Vec<ValueChange>) {
    let before = dlc::list_quests(old);
    let after = dlc::list_quests(new);
    for quest in &after {
        let field = format!("DLC クエスト {}", quest.quest_id);
        match before.iter().find(|b| b.quest_id == quest.quest_id) {
            Some(b) if b.data == quest.data => {}
            Some(b) => differing(
                out,
                None,
                field,
                format!("内容 {} バイト", b.data.len()),
                format!("書き換え {} バイト", quest.data.len()),
            ),
            None => differing(
                out,
                None,
                field,
                String::from("なし"),
                format!("追加 {} バイト", quest.data.len()),
            ),
        }
    }
    for b in &before {
        if after.iter().any(|q| q.quest_id == b.quest_id) {
            continue;
        }
        differing(
            out,
            None,
            format!("DLC クエスト {}", b.quest_id),
            format!("{} バイト", b.data.len()),
            String::from("なし"),
        );
    }
}

/// One download Palico as a value: id and name, or なし.
fn dlc_palico_line(id: u8, name: &str) -> String {
    let name = name.trim();
    if id == 0 && name.is_empty() {
        String::from("なし")
    } else if name.is_empty() {
        format!("名前なし（ID {}）", id)
    } else {
        format!("{}（ID {}）", name, id)
    }
}

/// The 45 quest-equipment loadouts: the supply box and the ammo list of each.
///
/// Every loadout carries five records of 24 + 8 entries, so this is the largest
/// value walk in the module; it only runs when the download section moved.
fn quest_equipment_values(old: &Save, new: &Save, out: &mut Vec<ValueChange>) {
    for slot in 0..dlc::EQUIP_SLOTS {
        let before = dlc::quest_equipment_items(old, slot);
        let after = dlc::quest_equipment_items(new, slot);
        for (a, b) in before.iter().zip(after.iter()) {
            if a.item == b.item && a.count == b.count {
                continue;
            }
            differing(
                out,
                None,
                quest_equipment_field(slot, a),
                quest_item_line(a),
                quest_item_line(b),
            );
        }
    }
}

/// "クエスト装備 #3 支給品 レコード1 位置2".
fn quest_equipment_field(slot: usize, item: &QuestEquipItem) -> String {
    let list = match item.list {
        QuestEquipList::Supply => "支給品",
        QuestEquipList::Ammo => "弾",
    };
    format!(
        "クエスト装備 #{} {} レコード{} 位置{}",
        slot + 1,
        list,
        item.record + 1,
        item.index + 1
    )
}

/// One quest-equipment entry as the supply panel words it.
fn quest_item_line(item: &QuestEquipItem) -> String {
    if item.item == 0 {
        String::from("なし")
    } else {
        format!("{} ×{}", crate::hbedit::item_name_of(item.item), item.count)
    }
}

/// Everything inside one character slot that has a readable value.
///
/// Each getter runs only for the parts its own bytes cover, so a change in one
/// tab cannot pick up a line from another.
fn slot_values(
    ranges: &[DiffRange],
    old: &Save,
    new: &Save,
    slot: usize,
    titles: Option<&TitleTable>,
    out: &mut Vec<ValueChange>,
) {
    let moved = |names: &[&str]| touched(ranges, Some(slot), names);
    // 所持金 and the rank points sit in their own layout entry, not in
    // キャラクター情報, so the money and rank rows are gated on it too: without
    // that a write that only moves the money showed no value at all.
    if moved(&["キャラクター情報", "名前", "所持金・HR ポイント"]) {
        character_values(old, new, slot, out);
    }
    if moved(&["ギルドカード"]) {
        card_values(old, new, slot, titles, out);
    }
    if moved(&["闘技大会の記録"]) {
        arena_values(old, new, slot, out);
    }
    if moved(&["装備ボックス"]) {
        equipment_values(old, new, slot, out);
    }
    if moved(&["アイテムボックス"]) {
        item_values(old, new, slot, out);
    }
    // The pouch sits between the item sets and the money. It is written from
    // two screens (the item box editor and ポーチへ適用 in the set editor), and
    // before this gate it had a byte range and no value at all.
    if moved(&["ポーチ"]) {
        pouch_values(old, new, slot, out);
    }
    if moved(&["アイテムマイセット", "装備マイセット"]) {
        set_values(old, new, slot, out);
    }
    if moved(&["オトモ"]) {
        palico_values(old, new, slot, out);
    }
    // The areas that used to keep their byte range and nothing else. The
    // pouch above is the newest of them.
    if moved(&["狩猟記録"]) {
        hunt_values(old, new, slot, out);
    }
    if moved(&["オトモ装備ボックス"]) {
        palico_gear_values(old, new, slot, out);
    }
    if moved(&["クエスト進行状況"]) {
        quest_values(old, new, slot, out);
    }
    if moved(&["勲章（解禁マップ）", "勲章（通知）", "勲章（カード側）"]) {
        award_values(old, new, slot, out);
    }
}

/// The character sheet: name, rank, money, play time and the look.
fn character_values(old: &Save, new: &Save, slot: usize, out: &mut Vec<ValueChange>) {
    let (Some(a), Some(b)) = (old.read_character(slot), new.read_character(slot)) else {
        return;
    };
    let at = Some(slot);
    differing(out, at, "名前", a.name.clone(), b.name.clone());
    differing(out, at, "画面の名前", a.name_preview.clone(), b.name_preview.clone());
    differing(out, at, "プレイ時間", play_clock(a.play_time), play_clock(b.play_time));
    differing(out, at, "所持金", format!("{} z", a.funds), format!("{} z", b.funds));
    differing(
        out,
        at,
        "ハンターランク",
        a.hunter_rank.to_string(),
        b.hunter_rank.to_string(),
    );
    differing(out, at, "HR ポイント", a.hr_points.to_string(), b.hr_points.to_string());
    differing(
        out,
        at,
        "学園ポイント",
        a.academy_points.to_string(),
        b.academy_points.to_string(),
    );
    for (i, name) in ["ベルナ", "ココット", "ポッケ", "ユクモ"].iter().enumerate() {
        differing(
            out,
            at,
            format!("{}ポイント", name),
            a.village_points[i].to_string(),
            b.village_points[i].to_string(),
        );
    }
    for (field, x, y) in [
        ("声", a.voice, b.voice),
        ("目の色", a.eye_color, b.eye_color),
        ("服装", a.clothing, b.clothing),
        ("性別", a.gender, b.gender),
        ("髪型", a.hairstyle, b.hairstyle),
        ("顔", a.face, b.face),
        ("特徴", a.features, b.features),
    ] {
        differing(out, at, field, x.to_string(), y.to_string());
    }
    for (field, x, y) in [
        ("肌の色", a.skin_color, b.skin_color),
        ("髪の色", a.hair_color, b.hair_color),
        ("特徴の色", a.features_color, b.features_color),
        ("服の色", a.clothing_color, b.clothing_color),
    ] {
        differing(out, at, field, rgba(x), rgba(y));
    }
}

/// The guild card itself: comment, title, rank, play time and quest count.
fn card_values(
    old: &Save,
    new: &Save,
    slot: usize,
    titles: Option<&TitleTable>,
    out: &mut Vec<ValueChange>,
) {
    let at = Some(slot);
    differing(
        out,
        at,
        "コメント",
        guildcard::comment(old, slot).unwrap_or_default(),
        guildcard::comment(new, slot).unwrap_or_default(),
    );
    if let (Some(x), Some(y)) = (guildcard::title(old, slot), guildcard::title(new, slot)) {
        differing(out, at, "称号", title_text(titles, x), title_text(titles, y));
    }
    differing(
        out,
        at,
        "ギルドカードの HR",
        guildcard::hunter_rank(old, slot).unwrap_or(0).to_string(),
        guildcard::hunter_rank(new, slot).unwrap_or(0).to_string(),
    );
    differing(
        out,
        at,
        "ギルドカードのプレイ時間",
        play_clock(guildcard::play_time(old, slot).unwrap_or(0).min(i32::MAX as u32) as i32),
        play_clock(guildcard::play_time(new, slot).unwrap_or(0).min(i32::MAX as u32) as i32),
    );
    differing(
        out,
        at,
        "クエスト達成数",
        guildcard::quest_total(old, slot).to_string(),
        guildcard::quest_total(new, slot).to_string(),
    );
}

/// The arena log and the weapon usage grid of one card.
fn arena_values(old: &Save, new: &Save, slot: usize, out: &mut Vec<ValueChange>) {
    let at = Some(slot);
    let (a, b) = (guildcard::arena(old, slot), guildcard::arena(new, slot));
    for (x, y) in a.iter().zip(b.iter()) {
        if x.centis == y.centis {
            continue;
        }
        differing(
            out,
            at,
            format!("闘技大会 {} の {} 位", x.arena + 1, x.rank + 1),
            arena_clock(x.centis),
            arena_clock(y.centis),
        );
    }
    let (a, b) = (
        guildcard::weapon_usage(old, slot),
        guildcard::weapon_usage(new, slot),
    );
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        differing(
            out,
            at,
            format!("武器使用数 {} 番", i + 1),
            x.to_string(),
            y.to_string(),
        );
    }
}

/// The equipment box: every changed record, as a piece or as a talisman.
fn equipment_values(old: &Save, new: &Save, slot: usize, out: &mut Vec<ValueChange>) {
    let (Some(a), Some(b)) = (old.equipment_box(slot), new.equipment_box(slot)) else {
        return;
    };
    let charms_a = charm_map(a);
    let charms_b = charm_map(b);
    let records = (a.len() / charm::CHARM_RECORD)
        .min(b.len() / charm::CHARM_RECORD)
        .min(charm::CHARM_COUNT);
    for i in 0..records {
        let at = i * charm::CHARM_RECORD;
        let ra = &a[at..at + charm::CHARM_RECORD];
        let rb = &b[at..at + charm::CHARM_RECORD];
        if ra == rb {
            continue;
        }
        let ca = charms_a.get(&i).copied();
        let cb = charms_b.get(&i).copied();
        if ca.is_some() || cb.is_some() || charm::is_talisman(ra) || charm::is_talisman(rb) {
            charm_values(out, slot, i, ra, rb, ca, cb);
        } else {
            equip_values(out, slot, i, ra, rb);
        }
    }
}

/// The talismans of one equipment box, by record index.
fn charm_map(box_bytes: &[u8]) -> alloc::collections::BTreeMap<usize, charm::Charm> {
    let mut out: alloc::collections::BTreeMap<usize, charm::Charm> =
        alloc::collections::BTreeMap::new();
    for talisman in charm::decode_box(box_bytes) {
        out.insert(talisman.index, talisman);
    }
    out
}

/// One talisman's owned fields, old against new.
fn charm_values(
    out: &mut Vec<ValueChange>,
    slot: usize,
    index: usize,
    old_rec: &[u8],
    new_rec: &[u8],
    old_charm: Option<charm::Charm>,
    new_charm: Option<charm::Charm>,
) {
    let at = Some(slot);
    differing(
        out,
        at,
        format!("護石 #{} スキル1", index + 1),
        charm_skill(old_charm, true),
        charm_skill(new_charm, true),
    );
    differing(
        out,
        at,
        format!("護石 #{} スキル2", index + 1),
        charm_skill(old_charm, false),
        charm_skill(new_charm, false),
    );
    differing(
        out,
        at,
        format!("護石 #{} スロット", index + 1),
        charm_slots(old_charm),
        charm_slots(new_charm),
    );
    differing(
        out,
        at,
        format!("護石 #{} レア度", index + 1),
        charm_rarity(old_rec),
        charm_rarity(new_rec),
    );
}

/// One talisman skill as "攻撃 +5", or なし.
fn charm_skill(charm: Option<charm::Charm>, first: bool) -> String {
    let Some(charm) = charm else {
        return String::from("なし");
    };
    let (name, points) = if first {
        (charm.skill1_name(), charm.points1)
    } else {
        (charm.skill2_name(), charm.points2)
    };
    match name {
        Some(name) => format!("{} {:+}", name, points),
        None => String::from("なし"),
    }
}

/// The decoration slot count of one talisman, or なし.
fn charm_slots(charm: Option<charm::Charm>) -> String {
    match charm {
        Some(charm) => format!("{} 個", charm.slots),
        None => String::from("なし"),
    }
}

/// The rarity of one raw record, as the charm module words it.
fn charm_rarity(rec: &[u8]) -> String {
    if charm::is_talisman(rec) {
        match charm::rarity_of_record(rec) {
            Some(grade) => format!("{}（R{}）", grade.name(), grade.rarity()),
            None => String::from("—"),
        }
    } else if charm::is_empty_record(rec) {
        String::from("なし")
    } else {
        String::from("—")
    }
}

/// One record of the equipment box that is not a talisman.
fn equip_values(out: &mut Vec<ValueChange>, slot: usize, index: usize, old_rec: &[u8], new_rec: &[u8]) {
    let (Some(a), Some(b)) = (
        equip::decode_record(index, old_rec),
        equip::decode_record(index, new_rec),
    ) else {
        return;
    };
    let at = Some(slot);
    let field = |what: &str| format!("装備ボックス #{} {}", index + 1, what);
    differing(
        out,
        at,
        field("種別"),
        equip::type_name(a.kind).to_owned(),
        equip::type_name(b.kind).to_owned(),
    );
    differing(out, at, field("ID"), a.id.to_string(), b.id.to_string());
    differing(
        out,
        at,
        field("名前"),
        equip::name(&a).to_owned(),
        equip::name(&b).to_owned(),
    );
    differing(out, at, field("レベル"), a.level.to_string(), b.level.to_string());
    differing(out, at, field("強化先"), transmog_label(&a), transmog_label(&b));
    differing(out, at, field("装飾品"), deco_list(old_rec), deco_list(new_rec));
}

/// The three decoration sockets of one record, named.
fn deco_list(rec: &[u8]) -> String {
    equip::Equipment::decorations(0, rec)
        .iter()
        .map(|id| deco_label(*id))
        .collect::<Vec<String>>()
        .join(" / ")
}

/// A decoration socket: a dash when it is empty.
fn deco_label(id: u16) -> String {
    if id == 0 {
        String::from("—")
    } else {
        crate::hbedit::decoration_name_of(id)
    }
}

/// Readable "name Lv n" for the piece a record is transmogged from.
fn transmog_label(e: &equip::Equipment) -> String {
    if e.transmog == 0 {
        return String::from("—");
    }
    let mut look = *e;
    look.id = e.transmog;
    format!("{} Lv{}", equip::name(&look), e.transmog_level)
}

/// The item box: every changed record as "name ×count".
fn item_values(old: &Save, new: &Save, slot: usize, out: &mut Vec<ValueChange>) {
    let (Some(a), Some(b)) = (old.read_item_box(slot), new.read_item_box(slot)) else {
        return;
    };
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x == y {
            continue;
        }
        differing(
            out,
            Some(slot),
            format!("アイテムボックス #{}", i + 1),
            item_line(x),
            item_line(y),
        );
    }
}

/// One item box record as the item box screen words it.
fn item_line(slot: &ItemSlot) -> String {
    item_count_line(slot.id, slot.count as u16)
}

/// One (id, count) pair as the item box, the sets and the pouch word it.
fn item_count_line(id: u16, count: u16) -> String {
    if id == 0 {
        String::from("なし")
    } else {
        format!("{} ×{}", crate::hbedit::item_name_of(id), count)
    }
}

/// The pouch: the thirty-two entries the hunter carries into a quest.
///
/// Two screens write it - the item box editor's ポーチ column and ポーチへ適用
/// in the set editor - and until this walk existed a change there showed its
/// byte range and no value. [myset::pouch] is the getter the set screen already
/// reads, so both sides are read through it and nothing is decoded twice.
fn pouch_values(old: &Save, new: &Save, slot: usize, out: &mut Vec<ValueChange>) {
    let (a, b) = (myset::pouch(old, slot), myset::pouch(new, slot));
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x == y {
            continue;
        }
        differing(
            out,
            Some(slot),
            format!("ポーチ #{}", i + 1),
            item_count_line(x.id, x.count),
            // The name again only when the item itself changed: a count
            // edit reads as "回復薬 ×10 → ×3", not the name twice.
            if x.id == y.id && y.id != 0 {
                format!("×{}", y.count)
            } else {
                item_count_line(y.id, y.count)
            },
        );
    }
}

/// The item and equipment sets: the names and the seven parts.
fn set_values(old: &Save, new: &Save, slot: usize, out: &mut Vec<ValueChange>) {
    let at = Some(slot);
    let (a, b) = (myset::item_sets(old, slot), myset::item_sets(new, slot));
    for (x, y) in a.iter().zip(b.iter()) {
        differing(
            out,
            at,
            format!("アイテムマイセット #{} 名前", x.index + 1),
            set_name(&x.name),
            set_name(&y.name),
        );
        let (ua, ub) = (x.used_items(), y.used_items());
        if ua != ub {
            differing(
                out,
                at,
                format!("アイテムマイセット #{} 中身", x.index + 1),
                format!("{} 件", ua.len()),
                format!("{} 件", ub.len()),
            );
        }
    }
    let (a, b) = (myset::equip_sets(old, slot), myset::equip_sets(new, slot));
    for (x, y) in a.iter().zip(b.iter()) {
        differing(
            out,
            at,
            format!("装備マイセット #{} 名前", x.index + 1),
            set_name(&x.name),
            set_name(&y.name),
        );
        for part in 0..myset::EQUIP_SET_PARTS {
            if x.parts[part] == y.parts[part] {
                continue;
            }
            differing(
                out,
                at,
                format!("装備マイセット #{} {}", x.index + 1, myset::PART_NAMES[part]),
                part_label(old, slot, x.parts[part]),
                part_label(new, slot, y.parts[part]),
            );
        }
    }
}

/// A saved set's name, with the game's "unused" marker kept.
fn set_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        String::from("（名前なし）")
    } else {
        trimmed.to_owned()
    }
}

/// What a set's part index points at in that save's equipment box.
fn part_label(save: &Save, slot: usize, index: u16) -> String {
    if index == myset::NONE {
        return String::from("なし");
    }
    match save.equipment_at(slot, index as usize) {
        Some(eq) if !eq.is_empty() => format!("{} Lv{}", equip::name(&eq), eq.level),
        _ => format!("#{}", index),
    }
}

/// The otomon roster: name, level, tendency, skills and the worn gear.
fn palico_values(old: &Save, new: &Save, slot: usize, out: &mut Vec<ValueChange>) {
    let (a, b) = (palico::roster(old, slot), palico::roster(new, slot));
    for (x, y) in a.iter().zip(b.iter()) {
        // Only records that really differ: an untouched empty slot is skipped
        // by the equality test, but a record the game never named still has a
        // level and a tendency, and those are values like any other.
        if x == y {
            continue;
        }
        let at = Some(slot);
        let label = format!("オトモ #{}", x.index + 1);
        differing(out, at, format!("{} 名前", label), palico_name(&x.name), palico_name(&y.name));
        differing(out, at, format!("{} レベル", label), x.level.to_string(), y.level.to_string());
        differing(
            out,
            at,
            format!("{} 傾向", label),
            x.system_name().to_owned(),
            y.system_name().to_owned(),
        );
        differing(out, at, format!("{} スキル", label), palico_skills(x), palico_skills(y));
        // The worn list holds equipment box indices, so this says which record
        // moved, not which piece: the box spelling is [palico_gear_values].
        for part in 0..palico::EQUIP_SLOTS {
            let (xw, yw) = (x.equip[part], y.equip[part]);
            if xw == yw {
                continue;
            }
            differing(
                out,
                at,
                format!("{} {}", label, crate::hbedit::palico_part_name(part)),
                worn_text(old, slot, xw),
                worn_text(new, slot, yw),
            );
        }
    }
}

/// What a palico wears in one part: the box index and the piece, or なし.
fn worn_text(save: &Save, slot: usize, index: u16) -> String {
    if index == crate::hbedit::PALICO_EQUIP_NONE {
        return String::from("なし");
    }
    match palicogear::decode_one(save, slot, index as usize) {
        Some(eq) => format!("枠 {} / {} ID {}", index, equip::type_name(eq.kind), eq.id),
        None => format!("枠 {}", index),
    }
}

/// A palico name, with the empty name called なし.
fn palico_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        String::from("なし")
    } else {
        trimmed.to_owned()
    }
}

/// The equipped skills of one palico, named.
fn palico_skills(palico: &Palico) -> String {
    let names: Vec<String> = palico
        .skills()
        .into_iter()
        .map(palico::skill_name)
        .collect();
    if names.is_empty() {
        String::from("なし")
    } else {
        names.join("・")
    }
}

/// The hunt records: slain, captured and the size pair of one monster.
///
/// The three arrays are per monster, so a row names the monster and then the
/// three values the hunt screen edits. A size of (0, 0) is "no specimen".
fn hunt_values(old: &Save, new: &Save, slot: usize, out: &mut Vec<ValueChange>) {
    let at = Some(slot);
    for monster in 0..hunts::MONSTERS {
        let (Some(sa), Some(sb)) = (
            hunts::killed(old, slot, monster),
            hunts::killed(new, slot, monster),
        ) else {
            continue;
        };
        let name = hunt_monster_name(monster);
        differing(
            out,
            at,
            format!("狩猟記録 {} 討伐数", name),
            sa.to_string(),
            sb.to_string(),
        );
        let (Some(ca), Some(cb)) = (
            hunts::captured(old, slot, monster),
            hunts::captured(new, slot, monster),
        ) else {
            continue;
        };
        differing(
            out,
            at,
            format!("狩猟記録 {} 捕獲数", name),
            ca.to_string(),
            cb.to_string(),
        );
        let (Some(za), Some(zb)) = (
            hunts::size_range(old, slot, monster),
            hunts::size_range(new, slot, monster),
        ) else {
            continue;
        };
        differing(
            out,
            at,
            format!("狩猟記録 {} サイズ", name),
            size_text(za),
            size_text(zb),
        );
    }
}

/// A monster name for a hunt-record index, or its number when no table is here.
fn hunt_monster_name(index: usize) -> String {
    let name = crate::hbedit::monster_name(index);
    if name == "?" {
        format!("#{}", index + 1)
    } else {
        String::from(name)
    }
}

/// A size record as the percentages the game shows.
fn size_text(range: (u16, u16)) -> String {
    if range.0 == 0 && range.1 == 0 {
        String::from("記録なし")
    } else {
        format!("{}% 〜 {}%", range.0, range.1)
    }
}

/// The palico equipment box: every changed record, by kind, id and level.
///
/// The names are not decoded: [equip::name] answers "-" for the three palico
/// kinds on purpose (their id mapping is not established), so the record is
/// named by its type and id the way the palico equipment panel already does.
fn palico_gear_values(old: &Save, new: &Save, slot: usize, out: &mut Vec<ValueChange>) {
    for index in 0..palicogear::PALICO_EQUIP_COUNT {
        let (Some(ra), Some(rb)) = (
            palicogear::record(old, slot, index),
            palicogear::record(new, slot, index),
        ) else {
            continue;
        };
        if ra == rb {
            continue;
        }
        let at = Some(slot);
        let field = |what: &str| format!("オトモ装備ボックス #{} {}", index + 1, what);
        match (
            equip::decode_record(index, ra),
            equip::decode_record(index, rb),
        ) {
            (Some(a), Some(b)) => {
                differing(
                    out,
                    at,
                    field("種別"),
                    equip::type_name(a.kind).to_owned(),
                    equip::type_name(b.kind).to_owned(),
                );
                differing(out, at, field("ID"), a.id.to_string(), b.id.to_string());
                differing(out, at, field("レベル"), a.level.to_string(), b.level.to_string());
            }
            // A record too short to decode still moved; the bytes are the value.
            _ => differing(
                out,
                at,
                field("内容"),
                format!("{} バイト", ra.len()),
                format!("{} バイト", rb.len()),
            ),
        }
        differing(
            out,
            at,
            field("装飾品"),
            deco_list(ra),
            deco_list(rb),
        );
    }
}

/// The three quest progress maps, one changed bit at a time.
fn quest_values(old: &Save, new: &Save, slot: usize, out: &mut Vec<ValueChange>) {
    let at = Some(slot);
    for (kind, word) in [
        (Progress::Cleared, "クリア"),
        (Progress::Seen, "既読"),
        (Progress::Failed, "失敗"),
    ] {
        for index in 0..questprogress::BITS {
            match (
                questprogress::get(old, slot, kind, index),
                questprogress::get(new, slot, kind, index),
            ) {
                (Some(x), Some(y)) if x != y => differing(
                    out,
                    at,
                    format!("クエスト #{} {}", index, word),
                    on_word(x),
                    on_word(y),
                ),
                _ => {}
            }
        }
    }
}

/// The request flags, one changed bit at a time.
fn request_values(old: &Save, new: &Save, slot: usize, out: &mut Vec<ValueChange>) {
    let at = Some(slot);
    for index in 0..requests::FLAGS {
        match (requests::get(old, slot, index), requests::get(new, slot, index)) {
            (Some(x), Some(y)) if x != y => differing(
                out,
                at,
                format!("依頼フラグ #{}", index),
                on_word(x),
                on_word(y),
            ),
            _ => {}
        }
    }
}

/// The 132 award bits of the card copy.
///
/// An award is unreadable (None) on a side whose slot holds no character, which
/// is exactly the first save of a new character: reading that side as "not
/// held" is what lets the review say なし → 取得 instead of dropping the line.
fn award_values(old: &Save, new: &Save, slot: usize, out: &mut Vec<ValueChange>) {
    let at = Some(slot);
    for index in 0..awards::AWARDS {
        let before = awards::has(old, slot, index);
        let after = awards::has(new, slot, index);
        if before.is_none() && after.is_none() {
            continue;
        }
        let (before, after) = (before.unwrap_or(false), after.unwrap_or(false));
        if before != after {
            differing(
                out,
                at,
                format!("勲章ビット #{}", index),
                award_word(before),
                award_word(after),
            );
        }
    }
}

/// A progress or request bit as a word.
fn on_word(on: bool) -> String {
    String::from(if on { "済" } else { "未" })
}

/// An award bit as a word.
fn award_word(on: bool) -> String {
    String::from(if on { "取得" } else { "なし" })
}

/// Seconds as h:mm:ss, for a play time the save stores in seconds.
fn play_clock(seconds: i32) -> String {
    let seconds = seconds.max(0) as u32;
    format!(
        "{}:{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

/// A stored arena time as the "m:ss.cc" the game shows.
fn arena_clock(centis: u16) -> String {
    let total = centis as u32;
    format!("{}:{:02}.{:02}", total / 6000, (total / 100) % 60, total % 100)
}

/// An RGBA colour as four numbers.
fn rgba(c: [u8; 4]) -> String {
    format!("{}/{}/{}/{}", c[0], c[1], c[2], c[3])
}

/// The 称号 spelled out from the romfs tables, or its three ids.
fn title_text(titles: Option<&TitleTable>, title: Title) -> String {
    let ids = format!("{} / {} / {}", title.first, title.link, title.last);
    match titles {
        Some(table) if !table.is_empty() => {
            let text = table.text(title);
            if text.is_empty() {
                ids
            } else {
                text
            }
        }
        _ => ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hbedit::{diff_save, SaveDiff, REVIEW_RANGES};

    /// Every value line of a diff, one per line, for a substring assertion.
    fn joined(diff: &SaveDiff) -> String {
        diff.values
            .iter()
            .map(|value| value.line())
            .collect::<Vec<String>>()
            .join("\n")
    }

    #[test]
    fn the_values_read_old_and_new_out_of_the_getters() {
        let old = crate::save::empty_switch_save();
        let mut new = old.clone();
        let mut character = new.read_character(0).expect("a character record");
        character.name = String::from("差分テスト");
        character.funds = 123_456;
        new.write_character(&character).expect("the character writes");
        // A weapon in the hunter's box: the level is stored minus one, so the
        // decoded pair is 1 -> 12 and the type 空き -> 大剣.
        crate::equipops::create(&mut new, 0, 7, 40, 12).expect("a blank weapon record");

        let diff = diff_save(&old.to_bytes(), &new.to_bytes());
        let text = joined(&diff);
        assert!(text.contains("スロット1 名前: （空） → 差分テスト"), "{}", text);
        assert!(text.contains("スロット1 所持金: 0 z → 123456 z"), "{}", text);
        assert!(
            text.contains("スロット1 装備ボックス #1 種別: 空き → 大剣"),
            "{}",
            text
        );
        assert!(
            text.contains("スロット1 装備ボックス #1 レベル: 1 → 12"),
            "{}",
            text
        );
    }

    #[test]
    fn the_hunt_records_report_slain_captured_and_size() {
        let old = crate::save::empty_switch_save();
        let mut new = old.clone();
        assert!(hunts::set_killed(&mut new, 0, 5, 7));
        assert!(hunts::set_captured(&mut new, 0, 5, 3));
        assert!(hunts::set_size_range(&mut new, 0, 5, (100, 130)));

        let diff = diff_save(&old.to_bytes(), &new.to_bytes());
        let text = joined(&diff);
        assert!(text.contains("討伐数: 0 → 7"), "{}", text);
        assert!(text.contains("捕獲数: 0 → 3"), "{}", text);
        assert!(text.contains("サイズ: 記録なし → 100% 〜 130%"), "{}", text);
        // The monster is named, or numbered when no name table is loaded: the
        // row has to say which monster moved either way.
        assert!(
            text.contains("狩猟記録 ") && (text.contains("Rathalos") || text.contains("#6")),
            "{}",
            text
        );
    }

    #[test]
    fn the_palico_gear_box_reports_the_piece_that_changed() {
        let old = crate::save::empty_switch_save();
        let mut new = old.clone();
        let index = palicogear::create(&mut new, 0, palicogear::WEAPON, 431, 5)
            .expect("a blank palico record");
        assert_eq!(index, 0);

        let diff = diff_save(&old.to_bytes(), &new.to_bytes());
        let text = joined(&diff);
        assert!(
            text.contains("オトモ装備ボックス #1 種別: 空き → オトモ武器"),
            "{}",
            text
        );
        assert!(text.contains("オトモ装備ボックス #1 ID: 0 → 431"), "{}", text);
        assert!(
            text.contains("オトモ装備ボックス #1 レベル: 1 → 5"),
            "{}",
            text
        );
    }

    #[test]
    fn the_quest_equipment_items_report_the_supply_and_ammo_entries() {
        let old = crate::save::empty_switch_save();
        let mut new = old.clone();
        dlc::set_quest_equipment_item(&mut new, 2, 1, QuestEquipList::Supply, 3, 1, 5)
            .expect("a supply entry");
        dlc::set_quest_equipment_item(&mut new, 2, 4, QuestEquipList::Ammo, 7, 2, 9)
            .expect("an ammo entry");

        let diff = diff_save(&old.to_bytes(), &new.to_bytes());
        let text = joined(&diff);
        assert!(
            text.contains("クエスト装備 #3 支給品 レコード2 位置4: なし → "),
            "{}",
            text
        );
        assert!(text.contains("×5"), "{}", text);
        assert!(
            text.contains("クエスト装備 #3 弾 レコード5 位置8: なし → "),
            "{}",
            text
        );
        assert!(text.contains("×9"), "{}", text);
    }

    #[test]
    fn a_long_value_list_is_capped_and_counts_the_rest() {
        let old = crate::save::empty_switch_save();
        let mut new = old.clone();
        // Index 0 is the game's "no quest" slot and is not writable, so the
        // forty bits are 1..=40.
        for index in 1..=40 {
            assert!(questprogress::set(&mut new, 0, Progress::Cleared, index, true));
        }

        let diff = diff_save(&old.to_bytes(), &new.to_bytes());
        // Each cleared bit also sets its seen bit (the invariant the progress
        // screen keeps), so forty bits are eighty value lines.
        assert_eq!(diff.values.len(), 80);
        let lines = diff.lines(REVIEW_RANGES);
        let values_drawn = lines.iter().filter(|l| l.contains("クエスト #")).count();
        assert_eq!(values_drawn, VALUE_SHOWN);
        assert!(
            lines
                .iter()
                .any(|l| l.contains("ほか 50 件（値は先頭 30 件まで")),
            "{}",
            lines.join("\n")
        );
    }

    #[test]
    fn the_review_prints_the_values_between_the_summary_and_the_ranges() {
        let old = crate::save::empty_switch_save();
        let mut new = old.clone();
        assert!(hunts::set_killed(&mut new, 0, 0, 2));

        let diff = diff_save(&old.to_bytes(), &new.to_bytes());
        let lines = diff.lines(REVIEW_RANGES);
        let at = |needle: &str| {
            lines
                .iter()
                .position(|l| l == needle)
                .unwrap_or_else(|| panic!("no {:?} in\n{}", needle, lines.join("\n")))
        };
        let summary = at("■ 変更される内容");
        let values = at("■ 変更される値（旧 → 新）");
        let ranges = at("■ 変更される範囲");
        assert!(summary < values, "{}", lines.join("\n"));
        assert!(values < ranges, "{}", lines.join("\n"));
        // Every line of the section is a value, and the value really is there.
        let section = &lines[values + 1..ranges];
        assert!(
            section
                .iter()
                .any(|l| l.contains("狩猟記録") && l.contains("討伐数: 0 → 2")),
            "{}",
            lines.join("\n")
        );
        for line in section.iter().filter(|l| !l.is_empty()) {
            assert!(line.starts_with('・'), "{}", line);
        }
    }

    /// The pouch is editable from two screens, and until this walk existed a
    /// change there showed the byte range and no value: the audit found it as
    /// the last hole of that kind. A count edit names the item once and the
    /// count twice; clearing says なし.
    #[test]
    fn the_pouch_reports_the_item_and_the_count() {
        let mut old = crate::save::empty_switch_save();
        // 9 is 回復薬 in the item table.
        assert!(myset::set_pouch_item(&mut old, 0, 0, 9, 10));
        assert!(myset::set_pouch_item(&mut old, 0, 3, 9, 2));
        let mut new = old.clone();
        assert!(myset::set_pouch_item(&mut new, 0, 0, 9, 3));
        assert!(myset::set_pouch_item(&mut new, 0, 3, 0, 0));

        let diff = diff_save(&old.to_bytes(), &new.to_bytes());
        let text = joined(&diff);
        assert!(
            text.contains("スロット1 ポーチ #1: 回復薬 ×10 → ×3"),
            "{}",
            text
        );
        assert!(
            text.contains("スロット1 ポーチ #4: 回復薬 ×2 → なし"),
            "{}",
            text
        );
        // The gate opened on the ポーチ range and nothing else moved.
        assert!(
            diff.ranges.iter().all(|range| range.name == "ポーチ"),
            "{:?}",
            diff.ranges
        );
    }
}
