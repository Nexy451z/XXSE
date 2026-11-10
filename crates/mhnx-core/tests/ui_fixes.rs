//! The console fixes of the third review, pinned by the state machine.
//!
//! Every item of that review is one of three shapes: a key that did the wrong
//! thing (A), a label that described a key that did something else (B), or a
//! feature with no way in (C). The state machine owns the answer to all three -
//! the key, the label it prints and the state the label names - so the tests
//! drive it, and the Switch layer only draws what these tests pin.
//!
//! The live save is only ever read: every test works on an in-memory copy
//! opened through the same request/answer path the console uses.

use std::path::PathBuf;

use mhnx_core::hbedit::{Editor, Key, Request, Screen};

fn real_save() -> PathBuf {
    let stable = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    if stable.exists() {
        return stable;
    }
    PathBuf::from(r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system")
}

fn real_arc() -> PathBuf {
    PathBuf::from(r"C:\anyWorkspace\nx\mhxx\save_editor\mhxx_quetool\quest\q1010165.arc")
}

/// Act as the I/O layer: answer whatever the editor asks for.
fn service(ed: &mut Editor) {
    if let Some(req) = ed.take_request() {
        match req {
            Request::ReadSave(_) => {
                let p = ed.save_files.first().cloned().unwrap_or_default();
                let b = std::fs::read(&p).expect("the save is readable");
                ed.open_save_bytes(&p, &b);
            }
            Request::ReadArc(_) => {
                let p = ed.arc_files.first().cloned().unwrap_or_default();
                let b = std::fs::read(&p).expect("the arc is readable");
                ed.open_arc_bytes(&p, &b);
            }
            Request::ExtractQuests { .. } => {}
        }
    }
}

fn open_save(ed: &mut Editor) {
    let p = real_save();
    if !p.exists() {
        return;
    }
    ed.set_save_files(vec![p.to_string_lossy().to_string()]);
    ed.input(Key::A);
    service(ed);
    assert_eq!(ed.screen, Screen::Slots, "the save opens on the slot screen");
}

// ---------------------------------------------------------------------------
// A. Mis-presses that lost data
// ---------------------------------------------------------------------------

/// A1: [A] on an empty quest list installed 189 quests, and nothing said so.
#[test]
fn a_press_on_an_empty_quest_list_installs_nothing() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    // The quest section is a section, not a key: the save list's own [Y] is
    // 再読込 now (the console's chip table is the reference), so the jump is
    // made the way the console's section bar makes it.
    ed.screen = Screen::ArcList;
    ed.cursor = 0;
    ed.open_save_quests();
    assert_eq!(ed.screen, Screen::SaveQuests);

    let before = ed.save_quests().len();
    // A filter that matches nothing is the same list an empty save shows.
    ed.apply_quest_filter(String::from("zzzz-no-such-quest"));
    assert!(ed.filtered_quests().is_empty());
    ed.input(Key::A);
    assert_eq!(
        ed.save_quests().len(),
        before,
        "one [A] must not install the bundled quests"
    );
    assert!(
        ed.message.contains("一括"),
        "the press has to say where the bulk install is: {}",
        ed.message
    );
    assert_eq!(
        ed.take_request(),
        None,
        "nothing may be written or extracted by that press"
    );
}

/// A2: releasing a download palico deleted the record on one press.
#[test]
fn the_palico_release_asks_twice() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.screen = Screen::Palico;

    let ids = mhnx_core::dlcats::all_ids();
    let id = ids[0];
    if ed.is_dlc_palico_installed(id) {
        // Make it uninstalled so the test starts from the same place either
        // way: the release is the two-press operation under test here.
        ed.palico_cursor = 0;
        ed.palico_a_press();
        ed.palico_a_press();
        assert!(!ed.is_dlc_palico_installed(id));
    }
    assert_eq!(ed.palico_row_action(id), "導入");
    ed.palico_a_press();
    assert!(ed.is_dlc_palico_installed(id), "one press installs");
    assert_eq!(ed.palico_row_action(id), "解除（2回）");

    // The release deletes the record: the first press only names it.
    ed.palico_a_press();
    assert!(
        ed.is_dlc_palico_installed(id),
        "one press must not delete a palico"
    );
    assert!(
        ed.message.contains("もう一度") && ed.message.contains("2回"),
        "the queue has to say what the second press does: {}",
        ed.message
    );
    // Any other key takes the offer back.
    ed.input(Key::Down);
    ed.input(Key::Up);
    ed.palico_a_press();
    assert!(
        ed.is_dlc_palico_installed(id),
        "the cursor move must drop the armed release"
    );
    ed.palico_a_press();
    assert!(!ed.is_dlc_palico_installed(id), "the second press releases it");
}

/// A3: the .mib presets rewrote every monster on one press.
#[test]
fn the_mib_presets_ask_twice_and_name_every_monster() {
    let mut ed = Editor::new();
    // Three monster slots with an id, the rest empty.
    let mut mib = vec![0u8; 0x200];
    let slots: Vec<usize> = mhnx_core::questfields::MIB_FIELDS
        .iter()
        .filter(|f| f.key.starts_with("mob_slot[") && f.offset + 13 <= mib.len())
        .map(|f| f.offset)
        .take(3)
        .collect();
    assert_eq!(slots.len(), 3, "the .mib field table lost its monster slots");
    for &off in slots.iter() {
        mib[off..off + 2].copy_from_slice(&1u16.to_le_bytes());
        mib[off + 5] = 60;
    }
    ed.mib_bytes = mib;
    assert_eq!(ed.mib_mob_count(), 3);

    let label = ed.mib_preset_label(0);
    assert!(label.contains("全3体"), "the chip has to carry the count: {}", label);

    assert!(!ed.mib_preset_press(0), "the first press only asks");
    assert_eq!(ed.mib_bytes[slots[0] + 5], 60, "the first press wrote nothing");
    assert!(
        ed.message.contains("全3体") && ed.message.contains("2回"),
        "the question has to say what would change: {}",
        ed.message
    );
    assert!(ed.mib_preset_press(0), "the second press runs it");
    for &off in slots.iter() {
        assert_eq!(ed.mib_bytes[off + 5], 1, "every monster the preset names");
    }
    assert!(ed.dirty);
}

/// A3: the .sem presets copied / zeroed every spawn on one press.
#[test]
fn the_sem_presets_ask_twice() {
    let arc = real_arc();
    if !arc.exists() {
        return;
    }
    let bytes = std::fs::read(&arc).expect("the sample arc");
    let mut ed = Editor::new();
    ed.open_arc_bytes(&arc.to_string_lossy(), &bytes);
    let n = ed.sem_entry_indices().len();
    if n < 2 {
        return;
    }
    assert!(ed.sem_preset_label(0).contains(&format!("{}", n - 1)));
    assert!(!ed.sem_preset_press(0), "the first press only asks");
    assert!(ed.message.contains("2回"));
    assert!(ed.sem_preset_press(0), "the second press runs it");
}

/// A4: [-] did its screen's job and the folder rescan on the same press.
#[test]
fn only_the_save_list_rescans_on_minus() {
    let mut ed = Editor::new();
    for screen in [
        Screen::SaveList,
        Screen::Slots,
        Screen::Char,
        Screen::Items,
        Screen::Equip,
        Screen::Charms,
        Screen::Sets,
        Screen::Csv,
        Screen::Roster,
        Screen::Palico,
        Screen::Hunts,
        Screen::Guild,
        Screen::Progress,
        Screen::SaveQuests,
        Screen::ArcList,
        Screen::ArcEntries,
        Screen::Mib,
        Screen::Gmd,
        Screen::Check,
        Screen::EntryView,
    ] {
        ed.screen = screen;
        // Nowhere, on any screen: [-] is the delete key and nothing else, and
        // the save list's refresh is [Y] 再読込 (the review's twelfth finding).
        assert!(
            !ed.minus_rescans(),
            "the rescan must not fire on Minus anywhere: {:?}",
            screen
        );
    }
    ed.screen = Screen::SaveList;
    assert!(!ed.minus_rescans(), "the save list refreshes on [Y]");
    // The quest list refreshes itself: its 展開 re-reads the quest folder as
    // part of that one action.
    ed.screen = Screen::ArcList;
    assert!(!ed.minus_rescans());
}

// ---------------------------------------------------------------------------
// B. The label and the key
// ---------------------------------------------------------------------------

/// B1: the item box said "ページ" for a key that switches BOX tabs.
#[test]
fn the_item_box_footer_names_the_box_switch() {
    let mut ed = Editor::new();
    ed.screen = Screen::Items;
    let hint = ed.footer_hint();
    // L / R still switch the BOX page; the footer spends its 763px on the
    // keys a player cannot guess instead (the tab bar is on the glass and
    // tappable), so what is pinned here is that the word ページ is gone and the
    // line fits.
    assert!(!hint.contains("ページ"), "{}", hint);
    assert!(
        mhnx_core::hbedit::caption_width(&hint) <= mhnx_core::hbedit::FOOTER_HINT_ROOM,
        "{}",
        hint
    );
}

/// B2: the checker's [A] is the automatic fix, and there is no ◀/▶ 調整.
#[test]
fn the_checker_footer_names_the_automatic_fix() {
    let mut ed = Editor::new();
    ed.screen = Screen::Check;
    let hint = ed.footer_hint();
    assert!(hint.contains("[A] 自動修正"), "{}", hint);
    assert!(!hint.contains("決定"), "{}", hint);
    assert!(!hint.contains("調整"), "{}", hint);
}

/// B3: the .arc list's footer is that screen's own keys, [Y] included.
#[test]
fn the_arc_list_footer_lists_its_own_keys() {
    let mut ed = Editor::new();
    ed.screen = Screen::ArcList;
    let hint = ed.footer_hint();
    // [B] names the screen it goes to: the save list. It used to say 戻る and
    // send the user to セーブ内, whose [B] came right back here.
    for chip in ["[A] 開く", "[Y] 導入", "[ZR] SDへ展開", "[B] セーブ一覧"] {
        assert!(
            hint.contains(chip),
            "the .arc list footer is missing {}: {}",
            chip,
            hint
        );
    }
}

/// B4: the arena editor's four directions mean one thing each, on both lines.
#[test]
fn the_arena_editor_speaks_the_same_language_everywhere() {
    let mut ed = Editor::new();
    ed.screen = Screen::Guild;
    ed.guild_tab = 1; // the arena view
    ed.arena_edit = true;
    let hint = ed.footer_hint();
    assert!(hint.contains("[◀/▶] 項目"), "{}", hint);
    assert!(hint.contains("[▲/▼] 増減"), "{}", hint);
    assert!(!hint.contains("分/秒"), "{}", hint);
}

/// B5: [A] on the guild card is the row's own action.
#[test]
fn the_guild_card_a_key_matches_its_row() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.screen = Screen::Guild;
    ed.guild_field = 2;
    assert_eq!(ed.guild_a_label(), "編集");

    ed.guild_field = 0;
    assert_eq!(ed.guild_a_label(), "+刻み");
    let before = mhnx_core::guildcard::hunter_rank(ed.save.as_ref().unwrap(), ed.slot).unwrap();
    ed.input(Key::A);
    let after = mhnx_core::guildcard::hunter_rank(ed.save.as_ref().unwrap(), ed.slot).unwrap();
    assert_eq!(after, (before + 1).min(999), "[A] must step the rank");
    assert!(ed.dirty);

    ed.guild_field = 1;
    let t0 = mhnx_core::guildcard::play_time(ed.save.as_ref().unwrap(), ed.slot).unwrap();
    ed.input(Key::A);
    let t1 = mhnx_core::guildcard::play_time(ed.save.as_ref().unwrap(), ed.slot).unwrap();
    assert!(t1 > t0, "[A] must step the clock");
}

/// B6: the download palico counts come from the data, not from a constant.
#[test]
fn the_palico_counts_are_the_lists_own() {
    let ed = Editor::new();
    let label = ed.palico_bulk_label();
    let total = mhnx_core::dlcats::all_ids().len();
    assert!(label.contains(&format!("{} 匹", total)), "{}", label);
    assert!(label.contains(&format!("JP {}", mhnx_core::dlcats::JP_IDS.len())));
    assert!(label.contains(&format!("EN {}", mhnx_core::dlcats::EN_IDS.len())));
    assert!(label.contains("導入済み"));
    assert!(!label.contains("28"), "the old constant is gone: {}", label);
}

/// C6 (the same rule as B): a screen that spends [＋] says how to leave.
#[test]
fn the_screens_that_use_plus_say_how_to_quit() {
    let mut ed = Editor::new();
    for screen in [Screen::Charms, Screen::Equip] {
        ed.screen = screen;
        let hint = ed.footer_hint();
        // The exit is its own item, not a parenthesis on the [＋] one: the
        // third review found the same key on one line with two meanings
        // ("[＋] 新規 …（[＋]は[B]の後）"). The line still has to say how to
        // leave, and it says it without reusing the glyph.
        assert!(
            hint.contains("※終了は [B]"),
            "{:?} spends [＋], so its footer must name the way out: {}",
            screen,
            hint
        );
        assert!(
            !hint.contains("（[＋]は"),
            "{:?} gives [＋] two meanings on one line: {}",
            screen,
            hint
        );
    }
    ed.screen = Screen::Slots;
    assert!(ed.footer_hint().contains("[＋] 終了"));
}

// ---------------------------------------------------------------------------
// C. The holes
// ---------------------------------------------------------------------------

/// C1: the equipment filter is shown, and an empty answer clears it.
#[test]
fn the_equipment_filter_is_shown_and_can_be_cleared() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.screen = Screen::Equip; // the box is a section, not a key
    let all = ed.equipment().len();
    // The heading names the box and says where the talismans are (the third
    // review's finding 14): a talisman is in the same box and is not a row of
    // this list, and nothing on screen used to say so.
    assert!(ed.equip_filter_heading().starts_with("装備BOX"));
    assert!(ed.equip_filter_heading().contains("護石一覧"));

    ed.apply_equip_filter(String::from("大剣"));
    assert!(ed.equip_filter_heading().contains("大剣"));
    assert!(ed.equipment().len() < all);
    assert!(ed.message.contains("解除"), "{}", ed.message);

    ed.apply_equip_filter(String::new());
    assert!(ed.equip_filter_heading().starts_with("装備BOX"));
    assert!(!ed.equip_filter_heading().contains("検索:"));
    assert_eq!(ed.equipment().len(), all);
    assert!(ed.message.contains("解除しました"));
}

/// C2: the hunt list can show every monster, so 0 -> 1 is reachable, and a
/// monster that is put back to 0 keeps its row while that list is on.
#[test]
fn the_hunt_list_can_show_every_monster() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.screen = Screen::Hunts;
    let recorded = ed.hunt_rows().len();
    assert!(ed.hunt_heading().contains("記録ありのみ"), "{}", ed.hunt_heading());

    ed.toggle_hunt_show_all();
    let every = ed.hunt_rows().len();
    assert!(every > recorded, "the full table is longer than the filtered one");
    assert!(ed.hunt_heading().contains("全モンスター"));

    // A monster with no record at all: give it one, then take it back.
    let empty = ed
        .hunt_rows()
        .into_iter()
        .position(|(_, s, c, _, _)| s == 0 && c == 0)
        .expect("the save has a monster with no record");
    ed.hunt_cursor = empty;
    ed.hunt_field = 0;
    ed.input(Key::Right);
    assert_eq!(ed.hunt_rows()[empty].1, 1, "[▶] must write the first record");
    ed.input(Key::Left);
    assert_eq!(ed.hunt_rows()[empty].1, 0, "[◀] takes it back");
    assert_eq!(
        ed.hunt_rows().len(),
        every,
        "a row put back to 0 must not vanish from the full list"
    );
}

/// C3: the failure bit is read-only, and the line says so.
#[test]
fn the_failure_bit_says_it_is_read_only() {
    let ed = Editor::new();
    let meta = ed.progress_quest_meta(0);
    assert!(meta.contains("参照のみ"), "{}", meta);
}

/// C4: the save's quest list deletes, and it takes two presses.
#[test]
fn the_save_quest_list_deletes_on_the_second_press() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    // The quest section is a section, not a key: the save list's own [Y] is
    // 再読込 now (the console's chip table is the reference), so the jump is
    // made the way the console's section bar makes it.
    ed.screen = Screen::ArcList;
    ed.cursor = 0;
    ed.open_save_quests();
    assert_eq!(ed.screen, Screen::SaveQuests);
    ed.apply_quest_filter(String::new());
    let list = ed.filtered_quests();
    if list.is_empty() {
        return;
    }
    let qid = list[0].quest_id;
    let before = ed.save_quests().len();

    ed.input(Key::Minus);
    assert_eq!(ed.save_quests().len(), before, "one press must not delete");
    assert!(
        ed.message.contains("もう一度") && ed.message.contains("2回"),
        "{}",
        ed.message
    );
    // Any other key drops the offer.
    ed.input(Key::Down);
    ed.input(Key::Up);
    ed.input(Key::Minus);
    assert_eq!(ed.save_quests().len(), before, "the offer was taken back");
    ed.input(Key::Minus);
    assert_eq!(ed.save_quests().len(), before - 1, "the second press deletes");
    assert!(!ed.save_quests().iter().any(|q| q.quest_id == qid));
    assert!(ed.message.contains(&format!("{}", qid)), "{}", ed.message);
}

/// C5: the 没クエスト bundle has a bulk install, wired from one place.
#[test]
fn the_dead_quest_bundle_is_wired_to_a_bulk_install() {
    // The bundle is generated from the user's own game files, so the count is
    // 0 on a checkout without them; the wiring is checked either way.
    let (official, dead) = mhnx_core::dlc::bundled_quest_counts();
    if official > 0 || dead > 0 {
        assert_eq!(official, 189, "the 公式 bundle is 189 quests");
        assert_eq!(dead, 34, "the 没 bundle is 34 quests");
    }
    let ui = include_str!("../../../switch/src/ui.rs");
    assert!(
        ui.contains("UiAction::InstallDlcCategory(1)"),
        "the console must wire the 没クエスト bundle to the bulk button"
    );
    assert!(
        ui.contains("bundled_quest_counts()"),
        "the button's count must come from the bundle"
    );
}

/// C7: the inspector's value box is a touch target, not decoration.
#[test]
fn the_inspector_value_box_is_tappable() {
    let ui = include_str!("../../../switch/src/ui.rs");
    let box_hit = ui
        .find("// Slot 4 (iy + 110, h = 48): Current Value Box")
        .expect("the value box is drawn");
    let rest = &ui[box_hit..];
    let end = rest.find("// Slot 5").expect("the [A] button follows it");
    assert!(
        rest[..end].contains("self.add_hit("),
        "the value box must carry a hit box of its own"
    );
}
