//! The fifth review's leftovers: one fact, one place.
//!
//! The review's finding was structural: four surfaces printed key meanings -
//! the footer, the ASCII build's chip row, the guide strip and the console's
//! deck - and each wrote its own words, so seven screens said two different
//! things about one key. The fix is one table (`hbedit::Editor::key_facts`)
//! that every surface renders, and these tests pin both halves: the table is
//! the only source, and the specific screens the review named now agree.

use mhnx_core::hbedit::{self, caption_width, Editor, Key, Screen};
use mhnx_core::{charm, save};

const UI_SRC: &str = include_str!("../../../switch/src/ui.rs");
const EDITOR_SRC: &str = include_str!("../../../switch/src/editor.rs");
const README_SRC: &str = include_str!("../../../README.md");
const HBEDIT_SRC: &str = include_str!("../src/hbedit.rs");

/// Every screen, in the order the state machine declares them.
const ALL_SCREENS: &[Screen] = &[
    Screen::SaveList,
    Screen::Slots,
    Screen::Char,
    Screen::Items,
    Screen::ItemPick,
    Screen::SkillPick,
    Screen::JewelPick,
    Screen::EquipPick,
    Screen::Charms,
    Screen::CharmEdit,
    Screen::Equip,
    Screen::EquipEdit,
    Screen::QuestEquip,
    Screen::SaveQuests,
    Screen::ArcList,
    Screen::QuestEdit,
    Screen::ArcEntries,
    Screen::EntryView,
    Screen::Mib,
    Screen::Gmd,
    Screen::Check,
    Screen::Hunts,
    Screen::Guild,
    Screen::Progress,
    Screen::Roster,
    Screen::Palico,
    Screen::Sets,
    Screen::Csv,
    Screen::Options,
    Screen::SaveMap,
    Screen::Review,
    Screen::Convert,
];

fn fn_body<'a>(src: &'a str, needle: &str) -> &'a str {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} is gone - the audit must be updated"));
    let open = src[start..].find('{').map(|i| start + i).expect("a body");
    let mut depth = 0i32;
    let mut end = open;
    for (i, ch) in src[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    &src[start..=end]
}

fn blank_save() -> save::Save {
    let mut save = save::empty_switch_save();
    save.set_slot_used(0, true);
    save
}

fn ten_record() -> [u8; charm::CHARM_RECORD] {
    let mut r = [0u8; charm::CHARM_RECORD];
    r[0x00] = 0x06;
    r[0x02] = 10;
    r[0x0C] = 74;
    r[0x0D] = 70;
    r[0x0E] = 6;
    r[0x0F] = 10;
    r[0x10] = 3;
    r[0x12] = 100;
    r[0x13] = 1;
    r
}

fn save_with_charms(n: usize) -> save::Save {
    let mut save = blank_save();
    {
        let b = save.equipment_box_mut(0).expect("the box exists");
        for i in 0..n {
            let o = i * charm::CHARM_RECORD;
            b[o..o + charm::CHARM_RECORD].copy_from_slice(&ten_record());
        }
    }
    save
}

fn editor_with(save: save::Save, screen: Screen) -> Editor {
    let mut ed = Editor::new();
    ed.save = Some(save);
    ed.save_path = Some(String::from("memory:review-21"));
    ed.slot = 0;
    ed.screen = screen;
    ed
}

/// The key tokens a line names, as the bracketed spellings the screens use.
fn key_tokens(text: &str) -> Vec<String> {
    const TOKENS: &[&str] = &[
        "[A]", "[B]", "[X]", "[Y]", "[ZR]", "[＋]", "[-]", "[◀/▶]", "[▲/▼]", "[L/R]", "[R]",
    ];
    TOKENS
        .iter()
        .filter(|t| text.contains(**t))
        .map(|t| (*t).to_string())
        .collect()
}

fn box_record(ed: &Editor, index: usize) -> [u8; charm::CHARM_RECORD] {
    let b = ed
        .save
        .as_ref()
        .and_then(|s| s.equipment_box(0))
        .expect("the box");
    let o = index * charm::CHARM_RECORD;
    let mut out = [0u8; charm::CHARM_RECORD];
    out.copy_from_slice(&b[o..o + charm::CHARM_RECORD]);
    out
}

// ---------------------------------------------------------------------------
// A. One table, four surfaces
// ---------------------------------------------------------------------------

/// The footer is the key table rendered, and nothing else.
///
/// This is the structural half of the review's finding: the line the canvas
/// draws is `key_facts` joined, so a key's meaning cannot be written twice.
#[test]
fn the_footer_is_the_key_table_rendered() {
    for &screen in ALL_SCREENS {
        let mut ed = Editor::new();
        ed.screen = screen;
        let facts = ed.key_facts();
        assert!(!facts.is_empty(), "{screen:?} has no key table");
        let mut want = String::new();
        for fact in &facts {
            if !want.is_empty() {
                want.push_str("  ");
            }
            want.push_str(&fact.render());
        }
        assert_eq!(ed.footer_hint(), want, "{screen:?}");
        // The way out is the last item and it is always named.
        let last = facts.last().expect("a last fact");
        assert!(
            last.key == "＋" || last.label.contains("終了"),
            "{screen:?}: the last item is not the way out: {last:?}"
        );
    }
}

/// The guide prints the same table's first items, in the same words.
///
/// The guide used to be a second prose list of key meanings; it is now the
/// screen's description plus `guide_facts`, so it cannot name a key the footer
/// does not and cannot describe it differently.
#[test]
fn the_guide_prints_the_same_facts_as_the_footer() {
    for &screen in ALL_SCREENS {
        let mut ed = Editor::new();
        ed.screen = screen;
        let guide = ed.screen_guide();
        let footer = ed.footer_hint();
        let facts = ed.guide_facts();
        assert!(!facts.is_empty(), "{screen:?}: the guide names no key");
        for fact in &facts {
            let rendered = fact.render();
            assert!(
                footer.contains(&rendered),
                "{screen:?}: the footer does not print {rendered}: {footer}"
            );
            assert!(
                guide.contains(&rendered),
                "{screen:?}: the guide does not print {rendered}: {guide}"
            );
        }
        // Every key the guide names is a key the footer names: the guide is a
        // prefix of the same list, never a second one.
        for token in key_tokens(&guide) {
            assert!(
                footer.contains(&token),
                "{screen:?}: the guide names {token} but the footer does not: {footer}"
            );
        }
    }
}

/// The text build's chip row renders the state machine's table.
///
/// The old table was a second copy, and the review's seven findings were the
/// copies disagreeing. The row is now `ed.key_facts()` mapped, so the ASCII
/// console prints the same words as the canvas footer.
#[test]
fn the_text_build_renders_the_state_machines_table() {
    let body = fn_body(EDITOR_SRC, "pub(crate) fn actions(");
    assert!(
        body.contains("ed.key_facts()"),
        "the text build must render the one table: {body}"
    );
    assert!(
        !EDITOR_SRC.contains("fn actions(screen: mhnx_core::hbedit::Screen)"),
        "the second table is gone"
    );
    // And the four surfaces the review counted all read it.
    assert!(HBEDIT_SRC.contains("pub fn key_facts"));
    assert!(UI_SRC.contains("ed.footer_hint()"), "the footer draws the table");
    assert!(UI_SRC.contains("ed.screen_guide()"), "the guide draws the table");
    assert!(UI_SRC.contains("ed.guide_facts()") || UI_SRC.contains("screen_guide"));
}

/// The deck's key labels are the state machine's, not a fourth wording.
#[test]
fn the_deck_prints_the_state_machines_labels() {
    assert!(
        UI_SRC.contains("ed.palico_lang_label()"),
        "the language button prints the core's label"
    );
    assert!(
        UI_SRC.contains("ed.equip_a_label()"),
        "the equipment box's [A] word follows the list"
    );
    assert!(
        UI_SRC.contains("ed.step_pill_label()"),
        "the step selector says where no key cycles it"
    );
    assert!(
        UI_SRC.contains("ed.empty_list_line()"),
        "the empty list's line is the core's"
    );
}

// ---------------------------------------------------------------------------
// A1. The seven screens the review named
// ---------------------------------------------------------------------------

/// .mib: [ZR] is named only on a row where it works, in both surfaces.
#[test]
fn the_mib_zr_is_named_only_on_a_monster_row() {
    let mut ed = editor_with(blank_save(), Screen::Mib);
    // Find a plain row and a monster row.
    let plain_row = (0..mhnx_core::questfields::MIB_FIELDS.len())
        .find(|i| {
            ed.mib_cursor = *i;
            !ed.mib_row_is_monster()
        })
        .expect("a plain row");
    ed.mib_cursor = plain_row;
    let footer = ed.footer_hint();
    assert!(!footer.contains("[ZR]"), "a plain row promises ZR: {footer}");
    assert!(!ed.screen_guide().contains("[ZR]"), "{}", ed.screen_guide());
    let monster_row = (0..mhnx_core::questfields::MIB_FIELDS.len())
        .find(|i| {
            ed.mib_cursor = *i;
            ed.mib_row_is_monster()
        })
        .expect("a monster row");
    ed.mib_cursor = monster_row;
    let footer = ed.footer_hint();
    assert!(footer.contains("[ZR] 項目"), "{footer}");
    // The guide is the table's prefix: it never names a key the footer does
    // not, on either row.
    for token in key_tokens(&ed.screen_guide()) {
        assert!(
            footer.contains(&token) || footer.contains("[ZR]"),
            "the guide names {token} but the footer does not: {footer}"
        );
    }
}

/// 支給品: [ZR] is the search, and the line says 検索 - the same word as the
/// pill on the bar.
#[test]
fn the_supply_tab_names_the_search_on_zr() {
    let mut ed = editor_with(blank_save(), Screen::QuestEdit);
    ed.quest_set_tab(hbedit::QUEST_TAB_SUPPLIES);
    let footer = ed.footer_hint();
    assert!(footer.contains("[ZR] 検索"), "{footer}");
    assert!(!footer.contains("[ZR] 項目"), "{footer}");
    // The key really opens the search.
    ed.input(Key::Z);
    assert!(
        ed.take_text_request().is_some(),
        "[ZR] must open the item search"
    );
    // The pill on the bar says the same word.
    assert!(
        UI_SRC.contains("String::from(\"[ZR] 検索\")"),
        "the bar's pill says 検索"
    );
}

/// ギルドカード's 闘技大会 view: [R] is the sub-tab there, so no line names it.
#[test]
fn the_arena_view_names_no_r_key() {
    let mut ed = editor_with(blank_save(), Screen::Guild);
    ed.set_guild_view(hbedit::GUILD_TAB_ARENA);
    let footer = ed.footer_hint();
    assert!(!footer.contains("[R]"), "the arena view promises R: {footer}");
    assert!(!ed.screen_guide().contains("[R]"), "{}", ed.screen_guide());
    // The three numeric views do name it.
    ed.set_guild_view(hbedit::GUILD_TAB_CARD);
    assert!(ed.footer_hint().contains("[R] 数値入力"));
}

/// マイセット's list: [◀/▶] is the tab pair, not a tour of the equipment.
#[test]
fn the_set_list_arrow_is_the_tab_key() {
    let ed = editor_with(blank_save(), Screen::Sets);
    let footer = ed.footer_hint();
    assert!(footer.contains("[◀/▶] タブ"), "{footer}");
    assert!(!footer.contains("装備を巡回"), "{footer}");
    assert!(!ed.screen_guide().contains("巡回"), "{}", ed.screen_guide());
}

/// 報酬: [ZR] walks three cells on a .rem row and two on a .sup row, and the
/// line names the cells the key really walks.
#[test]
fn the_reward_zr_names_the_cells_it_walks() {
    const HASH_REM: u32 = 0x5B3C_302D;
    const HASH_SUP: u32 = 0x5453_9AEE;
    fn entry(name: &str, type_hash: u32) -> mhnx_core::arc::ArcEntry {
        mhnx_core::arc::ArcEntry {
            name: String::from(name),
            type_hash,
            data: vec![0u8; 184],
            original: None,
            original_offset: None,
        }
    }
    let mut ed = editor_with(blank_save(), Screen::EntryView);
    ed.arc = Some(mhnx_core::arc::Arc {
        version: 3,
        header8: 0xFFFF_FFFE,
        entries: vec![
            entry("quest\\rem\\rem_620114", HASH_REM),
            entry("quest\\supp\\supp_1010165", HASH_SUP),
        ],
    });
    ed.entry = 0;
    assert!(ed.entry_has_numbers());
    assert!(ed.entry_has_chance(), "a reward row has a chance column");
    let footer = ed.footer_hint();
    assert!(
        footer.contains("[ZR] アイテム→個数→確率"),
        "the three cells are named: {footer}"
    );
    assert!(
        caption_width(&footer) <= hbedit::FOOTER_HINT_ROOM,
        "the footer does not fit: {}px {footer}",
        caption_width(&footer)
    );
    // A supply row has no chance column, so its cycle is two long and the line
    // says two.
    ed.entry = 1;
    assert!(ed.entry_has_numbers());
    assert!(!ed.entry_has_chance());
    let footer = ed.footer_hint();
    assert!(footer.contains("[ZR] アイテム→個数"), "{footer}");
    assert!(!footer.contains("確率"), "{footer}");
}

/// The talisman form's [B] goes to the list, and no surface says 戻る.
#[test]
fn the_talisman_form_b_names_the_list() {
    let mut ed = editor_with(save_with_charms(3), Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit);
    let footer = ed.footer_hint();
    assert!(footer.contains("[B] 一覧へ"), "{footer}");
    assert!(!footer.contains("[B] 戻る"), "{footer}");
    let guide = ed.screen_guide();
    for fact in ed.guide_facts() {
        assert!(guide.contains(&fact.render()), "{guide}");
    }
    assert!(!guide.contains("戻る"), "{guide}");
}

/// 変更一覧's [Y] rebuilds the list, and the deck's words are the same.
#[test]
fn the_review_y_says_reload() {
    let ed = editor_with(blank_save(), Screen::Review);
    let footer = ed.footer_hint();
    assert!(footer.contains("[Y] 再読込"), "{footer}");
    assert!(!footer.contains("作り直す"), "{footer}");
    assert!(!ed.screen_guide().contains("作り直す"), "{}", ed.screen_guide());
}

// ---------------------------------------------------------------------------
// A2. The palico language has one implementation
// ---------------------------------------------------------------------------

/// The state machine answers ◀ / ▶, and nothing else does.
#[test]
fn the_palico_language_has_one_implementation() {
    let mut ed = editor_with(blank_save(), Screen::Palico);
    assert_eq!(ed.palico_lang, 0);
    ed.input(Key::Left);
    assert_eq!(ed.palico_lang, 1, "◀ switches the language");
    ed.input(Key::Right);
    assert_eq!(ed.palico_lang, 0, "▶ switches it back");
    // The old key is not a second route.
    ed.input(Key::L);
    assert_eq!(ed.palico_lang, 0, "L is the sub-tab, not the language");
    ed.input(Key::R);
    assert_eq!(ed.palico_lang, 0, "R is the sub-tab, not the language");
    // One sentence, from the one method.
    ed.toggle_palico_lang();
    assert_eq!(ed.message, "配信オトモの言語: 英語 (English)");
    ed.toggle_palico_lang();
    assert_eq!(ed.message, "配信オトモの言語: 日本語");
    // The console has no arm of its own for the pair.
    let body = fn_body(UI_SRC, "pub fn handle_pad_key(");
    let palico = body
        .find("Screen::Palico => match key {")
        .expect("the palico arm");
    let tail = &body[palico..];
    let end = tail.find("Screen::Hunts =>").expect("the next arm");
    assert!(
        !tail[..end].contains("Key::Left") && !tail[..end].contains("Key::Right"),
        "the console must not answer the pair itself: {}",
        &tail[..end]
    );
    assert!(
        UI_SRC.contains("ed.toggle_palico_lang()"),
        "the deck's button calls the state machine's method"
    );
}

/// The footer, the guide and the deck say 言語, and the deck prints the core's
/// label.
#[test]
fn the_palico_language_is_one_word_on_every_surface() {
    let mut ed = editor_with(blank_save(), Screen::Palico);
    let footer = ed.footer_hint();
    assert!(footer.contains("[◀/▶] 言語"), "{footer}");
    assert!(ed.screen_guide().contains("[◀/▶] 言語"), "{}", ed.screen_guide());
    assert_eq!(ed.palico_lang_label(), "言語: 日本語");
    ed.toggle_palico_lang();
    assert_eq!(ed.palico_lang_label(), "言語: 英語");
    assert!(
        UI_SRC.contains("format!(\"[◀/▶] {}\", ed.palico_lang_label())"),
        "the deck prints the same label"
    );
    assert!(!UI_SRC.contains("名前の言語"), "the old wording is gone");
}

// ---------------------------------------------------------------------------
// B3. [X] is the save key on the five screens that used to step
// ---------------------------------------------------------------------------

/// [X] never changes the step, and the footer says 保存 on all five.
#[test]
fn the_save_key_saves_on_the_five_screens_that_used_to_step() {
    for screen in [
        Screen::Char,
        Screen::Items,
        Screen::CharmEdit,
        Screen::EquipEdit,
        Screen::Mib,
    ] {
        let mut ed = editor_with(save_with_charms(3), screen);
        if screen == Screen::CharmEdit {
            ed.open_charms();
            ed.charm_a_press();
        }
        if screen == Screen::EquipEdit {
            ed.equip_edit = Some(hbedit::EquipEdit::default());
        }
        let step = ed.step;
        ed.input(Key::X);
        assert_eq!(ed.step, step, "{screen:?}: [X] changed the step");
        assert!(
            ed.footer_hint().contains("[X] 保存"),
            "{screen:?}: the footer does not name the save: {}",
            ed.footer_hint()
        );
    }
    // On the screens with a save open, the press really asks for the write.
    for screen in [Screen::Char, Screen::Items, Screen::CharmEdit] {
        let mut ed = editor_with(save_with_charms(3), screen);
        if screen == Screen::CharmEdit {
            ed.open_charms();
            ed.charm_a_press();
        }
        ed.input(Key::X);
        assert!(
            ed.take_write().is_some(),
            "{screen:?}: [X] must ask for the file write"
        );
    }
    // The equipment form: opened on a real record, [X] commits and writes.
    let mut save = blank_save();
    mhnx_core::equipops::create(&mut save, 0, 1, 40, 1).expect("a weapon record");
    let mut ed = editor_with(save, Screen::Equip);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipEdit, "{}", ed.message);
    ed.input(Key::X);
    assert!(
        ed.take_write().is_some(),
        "EquipEdit: [X] must ask for the file write: {}",
        ed.message
    );
}

// ---------------------------------------------------------------------------
// B4. The reward [ZR] walks three cells
// ---------------------------------------------------------------------------

/// The row's own screen walks アイテム → 個数 → 確率%, and the line says it.
#[test]
fn the_reward_zr_walks_three_cells() {
    let ed = editor_with(blank_save(), Screen::EntryView);
    // A .rem table is what entry_has_numbers answers for; the footer is built
    // from the extension, so a save with no arc has no table and the line is
    // the non-table one.
    assert!(!ed.entry_has_numbers());
    assert!(!ed.footer_hint().contains("[ZR]"));
    // The table's own three cells, from the entry screen's writer.
    assert_eq!(hbedit::QUEST_REWARD_FIELDS, 3);
    assert_eq!(
        [
            hbedit::QUEST_REWARD_ITEM,
            hbedit::QUEST_REWARD_COUNT,
            hbedit::QUEST_REWARD_CHANCE
        ],
        [0, 1, 2]
    );
    // The README says three, not two.
    assert!(README_SRC.contains("3 段"), "the README says three stops");
    assert!(
        README_SRC.contains("アイテム → 個数 → 確率%"),
        "and names the three cells"
    );
}

/// The クエスト改造 報酬 tab's [ZR] walks the same three cells, and the footer
/// names them.
#[test]
fn the_quest_reward_tab_names_three_cells() {
    let mut ed = editor_with(blank_save(), Screen::QuestEdit);
    ed.quest_set_tab(hbedit::QUEST_TAB_REWARDS);
    assert_eq!(ed.quest_reward_field, hbedit::QUEST_REWARD_COUNT);
    ed.input(Key::Z);
    assert_eq!(ed.quest_reward_field, hbedit::QUEST_REWARD_CHANCE);
    ed.input(Key::Z);
    assert_eq!(ed.quest_reward_field, hbedit::QUEST_REWARD_ITEM);
    ed.input(Key::Z);
    assert_eq!(ed.quest_reward_field, hbedit::QUEST_REWARD_COUNT);
    let footer = ed.footer_hint();
    assert!(footer.contains("[ZR] アイテム→個数→確率"), "{footer}");
    // The item cell is a cell, not a number: the tab knows which one it is on.
    ed.quest_reward_field = hbedit::QUEST_REWARD_ITEM;
    assert!(ed.quest_edit_item_cell());
    assert!(!ed.quest_edit_chance());
    ed.quest_reward_field = hbedit::QUEST_REWARD_CHANCE;
    assert!(ed.quest_edit_chance());
    assert!(!ed.quest_edit_item_cell());
}

// ---------------------------------------------------------------------------
// C. The empty list
// ---------------------------------------------------------------------------

/// An empty list still draws the warning strip.
#[test]
fn an_empty_list_still_draws_its_strip() {
    let body = fn_body(UI_SRC, "fn draw_adaptive_rows_warn(");
    let empty = body
        .find("if total == 0 {")
        .expect("the empty branch");
    let tail = &body[empty..];
    let end = tail.find("return;").expect("the early return");
    assert!(
        tail[..end].contains("draw_list_strip"),
        "the empty branch must draw the strip: {}",
        &tail[..end]
    );
    assert!(
        tail[..end].contains("empty_list_line"),
        "and the screen's own empty line"
    );
    // The strip itself is one function, drawn by both branches.
    let strip = fn_body(UI_SRC, "fn draw_list_strip(");
    assert!(strip.contains("accent_warn"));
    assert!(strip.contains("text_secondary"));
}

/// A list with no rows has no bar, so a tap in the bar's strip must not take
/// the previous screen's thumb.
#[test]
fn an_empty_list_has_no_bar_to_take_a_finger() {
    let body = fn_body(UI_SRC, "fn begin_bar_drag(");
    assert!(
        body.contains("bar.scroll.total() == 0"),
        "a 0-row list must not answer the bar: {body}"
    );
    let list = fn_body(UI_SRC, "fn draw_adaptive_rows_warn(");
    let empty = list.find("if total == 0 {").expect("the empty branch");
    let tail = &list[empty..];
    let end = tail.find("return;").expect("the early return");
    assert!(
        tail[..end].contains("self.bar = None"),
        "the empty branch drops the stale bar: {}",
        &tail[..end]
    );
}

/// The empty line distinguishes an empty box from a search with no hits.
#[test]
fn an_empty_list_says_which_empty_it_is() {
    let mut ed = editor_with(save_with_charms(0), Screen::Charms);
    ed.open_charms();
    let empty = ed.empty_list_line();
    assert!(empty.contains("護石"), "{empty}");
    assert!(!empty.contains("検索"), "{empty}");
    ed.charm_filter = String::from("zzzznothing");
    let searched = ed.empty_list_line();
    assert!(searched.contains("zzzznothing"), "{searched}");
    assert!(searched.contains("検索"), "{searched}");
    assert_ne!(empty, searched, "the two states are two sentences");

    let mut ed = editor_with(blank_save(), Screen::Equip);
    let empty = ed.empty_list_line();
    assert!(empty.contains("装備"), "{empty}");
    ed.equip_filter = String::from("そんな装備はない");
    assert!(ed.empty_list_line().contains("そんな装備はない"));
}

// ---------------------------------------------------------------------------
// D9. Overwriting a talisman costs two presses
// ---------------------------------------------------------------------------

/// Moving the slot row onto an existing talisman and pressing [A] asks first.
#[test]
fn overwriting_a_talisman_costs_two_presses() {
    let mut ed = editor_with(save_with_charms(2), Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit);
    let before = box_record(&ed, 1);
    // A field the first press will describe: the slot count, which the writer
    // takes as it stands (the point rows clamp to the skill's table).
    ed.charm_edit.as_mut().expect("the form").field = 2;
    ed.input(Key::Right);
    // The slot row, moved to the second talisman.
    ed.charm_edit.as_mut().expect("the form").field = 0;
    ed.input(Key::Right);
    assert_eq!(ed.charm_edit.expect("the form").index, 1);
    assert!(ed.charm_write_replaces(), "the slot holds a talisman");
    // First press: names the record, writes nothing.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit, "the form stays open");
    assert_eq!(box_record(&ed, 1), before, "nothing was written");
    assert!(ed.message.contains("上書き"), "{}", ed.message);
    // Second press: writes.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Charms, "the write leaves for the list");
    assert_ne!(box_record(&ed, 1), before, "the record was replaced");
}

/// Changing the draft takes the offer back: the second press must write what
/// the first one described.
#[test]
fn changing_the_draft_takes_the_overwrite_offer_back() {
    let mut ed = editor_with(save_with_charms(2), Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    ed.charm_edit.as_mut().expect("the form").field = 0;
    ed.input(Key::Right);
    ed.input(Key::A);
    assert!(ed.message.contains("上書き"), "{}", ed.message);
    // A field the first press did not describe.
    ed.charm_edit.as_mut().expect("the form").field = 4;
    ed.input(Key::Right);
    ed.charm_edit.as_mut().expect("the form").field = 0;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit, "the offer was taken back");
    assert!(ed.message.contains("上書き"), "{}", ed.message);
}

// ---------------------------------------------------------------------------
// E10. The new equipment form opens on the ID row
// ---------------------------------------------------------------------------

/// [+] opens the form on the ID row, whose list writes the kind too.
#[test]
fn the_new_equipment_form_opens_on_the_id_row() {
    let mut ed = editor_with(blank_save(), Screen::Equip);
    ed.input(Key::Plus);
    assert_eq!(ed.screen, Screen::EquipEdit, "{}", ed.message);
    let form = ed.equip_edit.expect("the form");
    assert!(form.new);
    assert_eq!(form.field, form.id_row(), "the form starts on the ID row");
    assert_eq!(form.id_row(), 1);
    // [A] there opens the whole-box picker, not the create.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::EquipPick, "{}", ed.message);
}

// ---------------------------------------------------------------------------
// E11. The step selector says where no key cycles it
// ---------------------------------------------------------------------------

/// The two forms have no pad key for the step, and the pill says so.
#[test]
fn the_step_pill_says_touch_only_where_no_key_cycles() {
    let mut ed = editor_with(save_with_charms(3), Screen::Charms);
    ed.open_charms();
    ed.charm_a_press();
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert_eq!(ed.step_pill_label(), "刻み（タッチのみ）");
    ed.screen = Screen::EquipEdit;
    ed.equip_edit = Some(hbedit::EquipEdit::default());
    assert_eq!(ed.step_pill_label(), "刻み（タッチのみ）");
    // A screen with a key names it.
    ed.screen = Screen::Char;
    assert_eq!(ed.step_pill_label(), "刻み [Y]");
    // And the console prints the core's label.
    assert!(UI_SRC.contains("ed.step_pill_label()"));
}

// ---------------------------------------------------------------------------
// E12. One tab per list in the quest section
// ---------------------------------------------------------------------------

/// The クエスト改造 tab is the editor; the save's list is セーブ内's. The bar
/// lights the tab whose list is showing, so one list never has two tabs.
#[test]
fn the_quest_bar_lights_one_tab_per_list() {
    let src = UI_SRC;
    let jump = fn_body(src, "fn goto_section(");
    let quest = jump.find("MainSection::Quest => {").expect("the quest arm");
    let tail = &jump[quest..];
    let end = tail.find("MainSection::Tools =>").expect("the next arm");
    let arm = &tail[..end];
    assert!(arm.contains("self.quest_bar_tab = 1"), "{arm}");
    assert!(arm.contains("self.quest_bar_tab = 4"), "{arm}");
    assert!(!arm.contains("self.quest_bar_tab = 0"), "{arm}");
    let select = fn_body(src, "fn select_sub_tab(");
    let quest_arm = select
        .find("MainSection::Quest => match idx {")
        .expect("the quest section's tab arm");
    let qtail = &select[quest_arm..];
    let zero = qtail.find("0 => {").expect("the クエスト改造 tab's arm");
    let ztail = &qtail[zero..];
    let zend = ztail.find("1..=3 =>").expect("the next arm");
    assert!(
        ztail[..zend].contains("self.quest_bar_tab = 1"),
        "the editor's tab falls back to the list's tab: {}",
        &ztail[..zend]
    );
    // The two labels name two different things.
    let labels = Editor::quest_section_tab_labels();
    assert_eq!(labels[0], "クエスト改造");
    assert_eq!(labels[1], "セーブ内");
    assert_ne!(labels[0], labels[1]);
}

// ---------------------------------------------------------------------------
// F13. The item set comment names the key that owns ポーチへ
// ---------------------------------------------------------------------------

/// The comment said no key claims ポーチへ while [R] does.
#[test]
fn the_item_set_comment_names_the_key_that_owns_the_pouch() {
    let body = fn_body(HBEDIT_SRC, "fn input_item_set_edit(");
    assert!(body.contains("Key::R =>"), "the form answers [R]");
    assert!(body.contains("item_set_to_pouch"), "and it is the pouch copy");
    assert!(
        !body.contains("no key claims it"),
        "the stale comment is gone: {body}"
    );
}

// ---------------------------------------------------------------------------
// F14. The equipment box's [A] follows the same rule as the talisman list
// ---------------------------------------------------------------------------

/// 編集 / 作成 / 解除 on both lists, from the list's own state.
#[test]
fn the_equipment_box_a_word_follows_the_list() {
    let mut ed = editor_with(blank_save(), Screen::Equip);
    assert_eq!(ed.equip_a_label(), "作成", "an empty box creates");
    assert!(ed.footer_hint().contains("[A] 作成"), "{}", ed.footer_hint());
    assert_eq!(ed.empty_a_label(), Some("作成"));
    ed.equip_filter = String::from("zzzznothing");
    assert_eq!(ed.equip_a_label(), "解除", "a search with no hits drops it");
    // The press really creates on an empty box.
    ed.equip_filter.clear();
    ed.equip_a_press();
    assert_eq!(ed.screen, Screen::EquipEdit, "{}", ed.message);
    assert!(ed.equip_edit.map(|e| e.new).unwrap_or(false));
    // The talisman list answers the same three ways.
    let mut ed = editor_with(save_with_charms(0), Screen::Charms);
    ed.open_charms();
    assert_eq!(ed.charm_a_label(), "作成");
    ed.charm_filter = String::from("zzzznothing");
    assert_eq!(ed.charm_a_label(), "解除");
}

// ---------------------------------------------------------------------------
// F15. The slot screen is キャラ選択 everywhere
// ---------------------------------------------------------------------------

/// The old word is gone from every surface, README included.
#[test]
fn the_slot_word_is_carried_everywhere() {
    for (name, src) in [
        ("ui.rs", UI_SRC),
        ("editor.rs", EDITOR_SRC),
        ("hbedit.rs", HBEDIT_SRC),
        ("README.md", README_SRC),
    ] {
        assert!(!src.contains("枠選択"), "{name} still says 枠選択");
    }
    assert!(UI_SRC.contains("\"キャラ選択\""), "the canvas title");
    assert!(EDITOR_SRC.contains("\"キャラクター選択\""), "the text build's title");
}
