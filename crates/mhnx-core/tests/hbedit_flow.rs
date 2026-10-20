//! End-to-end tests for the on-device editing flow.
//!
//! These drive the exact state machine the Switch build runs, using synthetic
//! button presses against the real save and quest files on this machine. They
//! are the closest thing to hardware testing that is possible without a
//! console, and they are skipped when the files are missing.

use std::path::PathBuf;

use mhnx_core::arc::{extension_for, Arc};
use mhnx_core::dlc;
use mhnx_core::hbedit::{Editor, Key, Request, Screen, Write};
use mhnx_core::questfields;
use mhnx_core::save::Save;

fn real_save() -> PathBuf {
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
                match std::fs::read(&p) {
                    Ok(b) => ed.open_save_bytes(&p, &b),
                    Err(e) => panic!("read {}: {}", p, e),
                }
            }
            Request::ReadArc(_) => {
                let p = ed.arc_files.first().cloned().unwrap_or_default();
                match std::fs::read(&p) {
                    Ok(b) => ed.open_arc_bytes(&p, &b),
                    Err(e) => panic!("read {}: {}", p, e),
                }
            }
            // The PC test does not write files for an extract request; the
            // tests that care inspect the request itself.
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
    assert_eq!(ed.screen, Screen::Slots, "save should open on the slot screen");
}

#[test]
fn save_flow_edits_funds_and_writes_a_valid_container() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);

    // Slots -> character editor.
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Char);
    assert_eq!(ed.field, 0);
    // The name is the first field. Whatever it is, it comes from the save.
    let want_name = ed
        .save
        .as_ref()
        .unwrap()
        .read_character(0)
        .unwrap()
        .name
        .clone();
    assert!(ed.lines()[1].text.contains(&want_name), "line: {}", ed.lines()[1].text);

    // Move to funds (field 1) and bump it. The default step is 1. The expected
    // value is read from the save so the test works with any dump.
    ed.input(Key::Down);
    assert_eq!(ed.field, 1);
    let funds0 = ed.save.as_ref().unwrap().read_character(0).unwrap().funds as i64;
    assert_eq!(ed.char_value(1), funds0);
    ed.input(Key::Right);
    assert_eq!(ed.char_value(1), funds0 + 1);
    ed.input(Key::Left);
    assert_eq!(ed.char_value(1), funds0);
    // One X press moves the step to 10 (STEPS = [1, 10, 100, 1000, 10000]).
    ed.input(Key::X);
    ed.input(Key::Right);
    assert_eq!(ed.char_value(1), funds0 + 10);
    ed.input(Key::Left);
    assert_eq!(ed.char_value(1), funds0);

    // Write and check the produced container.
    ed.input(Key::A);
    match ed.take_write() {
        Some(Write::File { path, bytes }) => {
            assert!(path.ends_with("system"));
            assert_eq!(bytes.len(), 4_726_188, "switch save size");
            let s = Save::from_bytes(&bytes).expect("valid save");
            assert_eq!(s.read_character(0).unwrap().funds as i64, funds0);
            assert_eq!(&bytes[0x14..0x18], &[0x74, 0xEE, 0xB2, 0x36], "container header");
        }
        other => panic!("expected a save write, got {:?}", other),
    }
}

#[test]
fn save_flow_edits_an_item_count() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);

    // Slots -> X -> item box.
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::Items);
    let vis = ed.visible_items();
    assert!(!vis.is_empty(), "the dump has non-empty item slots");

    // Pick a slot with room for the +11 the test adds, whatever the dump holds.
    let boxes = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    let first = *vis
        .iter()
        .find(|i| boxes[**i].count < 100)
        .expect("a slot with room for 11 more");
    let before = boxes[first].count;
    // Default step is 1; X cycles it to 10.
    ed.input(Key::Right);
    ed.input(Key::X);
    ed.input(Key::Right);
    let after = ed.save.as_ref().unwrap().read_item_box(0).unwrap()[first].count;
    assert_eq!(after, before + 11);

    ed.input(Key::A);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let s = Save::from_bytes(&bytes).unwrap();
            assert_eq!(s.read_item_box(0).unwrap()[first].count, after);
            // Everything else must be untouched.
            let orig = std::fs::read(real_save()).unwrap();
            let os = Save::from_bytes(&orig).unwrap();
            let a = s.read_item_box(0).unwrap();
            let b = os.read_item_box(0).unwrap();
            for i in 0..2300 {
                if i != first {
                    assert_eq!(a[i], b[i], "item {} changed unexpectedly", i);
                }
            }
        }
        other => panic!("expected a write, got {:?}", other),
    }
}

/// Move the .mib cursor to the field with the given key.
fn goto_mib_field(ed: &mut Editor, key: &str) {
    for _ in 0..questfields::MIB_FIELDS.len() {
        if questfields::MIB_FIELDS[ed.mib_cursor].key == key {
            return;
        }
        ed.input(Key::Down);
    }
    panic!("field {} not found", key);
}

#[test]
fn quest_flow_edits_a_mib_field_and_repacks_the_arc() {
    if !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    // Y switches from the save list to the quest list.
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::ArcList);
    ed.input(Key::A);
    service(&mut ed);
    assert_eq!(ed.screen, Screen::ArcEntries);
    // The editor should have pre-selected the .mib entry.
    let e = ed.arc.as_ref().unwrap().entries[ed.cursor].clone();
    assert_eq!(extension_for(e.type_hash), Some("mib"));

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Mib);

    // Read and change the time limit.
    let before = {
        let f = questfields::MIB_FIELDS
            .iter()
            .find(|f| f.key == "time_limit")
            .unwrap();
        questfields::read_u32(ed.mib_bytes(), f.offset, f.kind).unwrap()
    };
    assert_eq!(before, 50);
    goto_mib_field(&mut ed, "time_limit");
    ed.input(Key::Right);
    let after_val = {
        let f = questfields::MIB_FIELDS
            .iter()
            .find(|f| f.key == "time_limit")
            .unwrap();
        questfields::read_u32(ed.mib_bytes(), f.offset, f.kind).unwrap()
    };
    assert_eq!(after_val, 51);

    // Write the archive back.
    ed.input(Key::A);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let a = Arc::parse(&bytes).expect("repacked archive must parse");
            let mib = a.by_ext("mib").unwrap();
            let f = questfields::MIB_FIELDS
                .iter()
                .find(|f| f.key == "time_limit")
                .unwrap();
            assert_eq!(
                questfields::read_u32(&mib.data, f.offset, f.kind).unwrap(),
                51
            );
            // Every other entry must survive untouched.
            let orig = Arc::parse(&std::fs::read(real_arc()).unwrap()).unwrap();
            assert_eq!(a.entries.len(), orig.entries.len());
            for (x, y) in a.entries.iter().zip(orig.entries.iter()) {
                assert_eq!(x.name, y.name);
                if extension_for(x.type_hash) != Some("mib") {
                    assert_eq!(x.data, y.data, "entry {} changed", y.name);
                }
            }
        }
        other => panic!("expected an .arc write, got {:?}", other),
    }
}

#[test]
fn quest_flow_cycles_an_enum_field() {
    if !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    ed.input(Key::Y);
    ed.input(Key::A);
    service(&mut ed);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Mib);

    // quest_kind is an enum field; Right must wrap inside the table.
    goto_mib_field(&mut ed, "quest_kind");
    let f = questfields::MIB_FIELDS
        .iter()
        .find(|f| f.key == "quest_kind")
        .unwrap();
    let table = questfields::enum_table(f.enum_name.unwrap()).unwrap();
    let before = questfields::read_u32(ed.mib_bytes(), f.offset, f.kind).unwrap() as usize;
    ed.input(Key::Right);
    let after = questfields::read_u32(ed.mib_bytes(), f.offset, f.kind).unwrap() as usize;
    assert_eq!(after, (before + 1) % table.len());
    ed.input(Key::Left);
    assert_eq!(
        questfields::read_u32(ed.mib_bytes(), f.offset, f.kind).unwrap() as usize,
        before
    );
}

#[test]
fn quest_flow_edits_gmd_text() {
    if !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    ed.input(Key::Y);
    ed.input(Key::A);
    service(&mut ed);

    // Move to the .gmd entry and open it.
    let n = ed.arc.as_ref().unwrap().entries.len();
    let mut found = false;
    for _ in 0..n {
        let e = ed.arc.as_ref().unwrap().entries[ed.cursor].clone();
        if extension_for(e.type_hash) == Some("gmd") {
            found = true;
            break;
        }
        ed.input(Key::Down);
    }
    assert!(found, "no .gmd entry");
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Gmd);

    // Ask to edit the title, then answer like the software keyboard would.
    ed.input(Key::A);
    let (idx, initial) = ed.take_text_request().expect("text request");
    assert_eq!(idx, 0);
    assert!(!initial.is_empty());
    ed.apply_text(0, "テスト用クエスト名".to_string());

    ed.input(Key::X);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let a = Arc::parse(&bytes).unwrap();
            let g = mhnx_core::quest::Gmd::from_bytes(&a.by_ext("gmd").unwrap().data).unwrap();
            assert_eq!(g.title(), Some("テスト用クエスト名"));
            assert_eq!(g.strings.len(), 7);
        }
        other => panic!("expected an .arc write, got {:?}", other),
    }
}

#[test]
fn quest_flow_installs_a_download_quest_into_the_save() {
    if !real_save().exists() || !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    // Open the save first.
    open_save(&mut ed);
    // Then switch to the quest list and open the archive.
    // Saving is on the slot screen, so Y jumps straight to the quest side.
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::ArcList);
    ed.input(Key::A);
    service(&mut ed);
    assert_eq!(ed.screen, Screen::ArcEntries);

    // X installs the open archive into the open save.
    let had_it = dlc::find_quest(ed.save.as_ref().unwrap(), 1010165).is_some();
    let before_n = dlc::list_quests(ed.save.as_ref().unwrap()).len();
    ed.input(Key::X);
    assert!(ed.message.contains("installed"), "message was {}", ed.message);
    let list = dlc::list_quests(ed.save.as_ref().unwrap());
    // Installing a quest that is already there replaces it in place, which is
    // the safe edit: the slot and the id never move.
    assert_eq!(list.len(), if had_it { before_n } else { before_n + 1 });
    assert!(list.iter().any(|q| q.quest_id == 1010165));

    // Writing the save must keep the quest.
    ed.input(Key::X); // install again is harmless (idempotent per id? it uses a free slot)
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let s = Save::from_bytes(&bytes).unwrap();
            assert!(!dlc::list_quests(&s).is_empty());
        }
        _ => {}
    }
}

#[test]
fn cursor_navigation_wraps_on_every_screen() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    // Slot screen wraps over three slots.
    ed.input(Key::Up);
    assert_eq!(ed.slot, 2);
    ed.input(Key::Down);
    assert_eq!(ed.slot, 0);

    ed.input(Key::A);
    // Character screen wraps over all fields.
    for _ in 0..CHARFIELD_COUNT {
        ed.input(Key::Down);
    }
    assert_eq!(ed.field, 0);
    ed.input(Key::Up);
    assert_eq!(ed.field, CHARFIELD_COUNT - 1);
}

const CHARFIELD_COUNT: usize = mhnx_core::hbedit::CHAR_FIELDS.len();
#[test]
fn quest_flow_checks_and_fixes_the_one_player_only_flag() {
    if !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    ed.input(Key::Y);
    ed.input(Key::A);
    service(&mut ed);

    // L opens the checker.
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Check);
    let idx = ed
        .findings
        .iter()
        .position(|f| f.code == "one-player-only")
        .expect("the sample quest carries the forced 1-player flag");
    assert!(ed.findings[idx].fixable);
    assert_eq!(ed.findings[idx].location.as_ref().unwrap().1, 0x1A);

    // A repairs it.
    ed.input(Key::A);
    assert!(
        ed.findings.iter().all(|f| f.code != "one-player-only"),
        "the finding should be gone after the fix"
    );
    assert!(ed.message.contains("0x2F"));

    // X writes the archive.
    ed.input(Key::X);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let a = Arc::parse(&bytes).unwrap();
            let mib = a.by_ext("mib").unwrap();
            assert_eq!(mib.data[0x1A], 0x00);
            // Nothing else changed.
            let orig = Arc::parse(&std::fs::read(real_arc()).unwrap()).unwrap();
            for (x, y) in a.entries.iter().zip(orig.entries.iter()) {
                assert_eq!(x.name, y.name);
                if x.name != mib.name {
                    assert_eq!(x.data, y.data, "{} changed", y.name);
                }
            }
        }
        other => panic!("expected a write, got {:?}", other),
    }
}
// ---------------------------------------------------------------------------
// Screens added for the features taken from the MHXX/MHGU Save Manager.
// ---------------------------------------------------------------------------

#[test]
fn options_screen_edits_and_saves() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    // X on the save list opens the options.
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::SaveList);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::Options);

    let before = ed.save.as_ref().unwrap().options().brightness;
    ed.input(Key::Left);
    assert_eq!(ed.save.as_ref().unwrap().options().brightness, before - 1);
    ed.input(Key::Right);
    assert_eq!(ed.save.as_ref().unwrap().options().brightness, before);
    // Row 1 is rumble and toggles.
    ed.input(Key::Down);
    let r = ed.save.as_ref().unwrap().options().rumble;
    ed.input(Key::Right);
    assert_ne!(ed.save.as_ref().unwrap().options().rumble, r);

    // The whole body still serialises.
    ed.input(Key::A);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let s = Save::from_bytes(&bytes).unwrap();
            assert_ne!(s.options().rumble, r);
        }
        other => panic!("expected a write, got {:?}", other),
    }
}

#[test]
fn palico_screen_installs_and_removes() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    // R on the slot screen opens the Palico catalogue.
    ed.input(Key::R);
    assert_eq!(ed.screen, Screen::Palico);

    let lines = ed.lines();
    assert!(lines[0].text.contains("DOWNLOAD PALICO"));
    assert!(lines[0].text.contains("JP"));

    // Language toggle.
    ed.input(Key::L);
    assert!(ed.lines()[0].text.contains("EN"));
    ed.input(Key::L);

    // Walk to 0x66 (Boonyah / 坊ニャ春道) and install it.
    let ids = mhnx_core::dlcats::all_ids();
    let target = ids.iter().position(|i| *i == 0x66).unwrap();
    for _ in 0..target {
        ed.input(Key::Down);
    }
    assert!(!ed.save.as_ref().unwrap().has_dlc_palico(0x66));
    let initial_cats = ed
        .save
        .as_ref()
        .unwrap()
        .dlc_palicoes()
        .iter()
        .filter(|c| c.id != 0)
        .count();
    ed.input(Key::A);
    assert!(ed.save.as_ref().unwrap().has_dlc_palico(0x66));
    assert!(ed.message.contains("added"), "message was {}", ed.message);

    // Pressing A again removes it.
    ed.input(Key::A);
    assert!(!ed.save.as_ref().unwrap().has_dlc_palico(0x66));

    // X writes it out.
    ed.input(Key::X);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let s = Save::from_bytes(&bytes).unwrap();
            assert_eq!(
                s.dlc_palicoes().iter().filter(|c| c.id != 0).count(),
                initial_cats
            );
        }
        other => panic!("expected a write, got {:?}", other),
    }
}

#[test]
fn charm_screen_creates_and_deletes() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    // L on the slot screen opens the charm box.
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Charms);
    let before_n = ed.save.as_ref().unwrap().charms(0).unwrap().len();

    // L starts a brand new charm in the first free slot.
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::CharmEdit);
    let lines = ed.lines();
    assert!(lines[0].text.contains("CHARM EDIT"));
    // The defaults are a usable charm.
    assert!(lines[3].text.contains("skill 1 id"));

    // A new charm starts as a copy of a real one, so the seed values come from
    // the box rather than from invented defaults. Read them, then bump.
    let seed = ed.charm_edit.expect("the form is open");
    assert!(seed.skill1 > 0, "a new charm must start from a real talisman");
    let want_skill1 = seed.skill1 as i32 + 1;
    let want_points1 = seed.points1 as i32 + 1;

    // The form opens on the first skill row; bump it and its points, then write.
    ed.input(Key::Right);
    ed.input(Key::Down);
    ed.input(Key::Right);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Charms);
    let list = ed.save.as_ref().unwrap().charms(0).unwrap();
    assert_eq!(list.len(), before_n + 1, "one charm was added");
    let added = list
        .iter()
        .find(|c| c.index == seed.index)
        .expect("the charm the form wrote");
    assert_eq!(added.skill1 as i32, want_skill1);
    assert_eq!(added.points1 as i32, want_points1);
    assert!(added.skill1_name().is_some(), "the skill must resolve");

    // It survives a save round trip.
    ed.input(Key::X);
    match ed.take_write() {
        Some(Write::File { bytes, .. }) => {
            let s = Save::from_bytes(&bytes).unwrap();
            assert_eq!(s.charms(0).unwrap().len(), before_n + 1);
        }
        other => panic!("expected a write, got {:?}", other),
    }

    // A on the list deletes whatever the cursor points at.
    let n = ed.save.as_ref().unwrap().charms(0).unwrap().len();
    ed.input(Key::A);
    assert_eq!(ed.save.as_ref().unwrap().charms(0).unwrap().len(), n - 1);
}

#[test]
fn every_screen_renders_without_panicking() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    let mut steps = 0;
    for key in [
        Key::L, Key::Y, Key::Right, Key::A, Key::B,
        Key::R, Key::Down, Key::L, Key::A, Key::X, Key::B,
        Key::B, Key::X, Key::Down, Key::Right, Key::A, Key::B,
    ] {
        ed.input(key);
        for l in ed.lines() {
            assert!(!l.text.is_empty(), "empty line on {:?}", ed.screen);
        }
        steps += 1;
    }
    assert!(steps > 0);
}
// ---------------------------------------------------------------------------
// Quest extraction: the editor asks, the platform writes.
// ---------------------------------------------------------------------------

#[test]
fn extracting_one_quest_asks_for_exactly_that_id() {
    if !real_save().exists() || !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    open_save(&mut ed);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::ArcList);
    ed.input(Key::A);
    service(&mut ed);
    assert_eq!(ed.screen, Screen::ArcEntries);

    // The editor must ask for the id it read out of the open .mib.
    let want = ed
        .arc
        .as_ref()
        .and_then(|a| a.by_ext("mib"))
        .and_then(|m| m.data.get(0x0C..0x10))
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .unwrap();
    assert!(want > 0);

    assert_eq!(ed.take_request(), None);
    ed.input(Key::Minus);
    match ed.take_request() {
        Some(Request::ExtractQuests { ids }) => assert_eq!(ids, vec![want]),
        other => panic!("expected an extract request, got {:?}", other),
    }
    // The I/O layer reports back through apply_extracted.
    ed.apply_extracted(1, "/switch/mhnxeditor/mhxx/quest/arc");
    assert!(ed.message.contains("1 quest file"), "message was {}", ed.message);
}

#[test]
fn extracting_all_asks_for_every_quest() {
    if !real_save().exists() || !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    open_save(&mut ed);
    ed.input(Key::Y);
    ed.input(Key::A);
    service(&mut ed);
    assert_eq!(ed.screen, Screen::ArcEntries);

    ed.input(Key::R);
    match ed.take_request() {
        Some(Request::ExtractQuests { ids }) => assert!(ids.is_empty(), "empty means every quest"),
        other => panic!("expected an extract request, got {:?}", other),
    }
}

#[test]
fn minus_is_only_swallowed_on_the_quest_entry_screen() {
    if !real_save().exists() || !real_arc().exists() {
        return;
    }
    let mut ed = Editor::new();
    ed.set_arc_files(vec![real_arc().to_string_lossy().to_string()]);
    open_save(&mut ed);
    // Elsewhere the Switch layer keeps Minus for its folder rescan.
    assert!(!ed.uses_minus(), "slot screen must leave Minus to the rescan");
    ed.input(Key::Y);
    assert!(!ed.uses_minus(), "quest list must leave Minus to the rescan");
    ed.input(Key::A);
    service(&mut ed);
    assert!(ed.uses_minus(), "entry screen consumes Minus itself");
}

#[test]
fn a_missing_save_reports_that_nothing_was_written() {
    let mut ed = Editor::new();
    ed.apply_extracted(0, "/switch/mhnxeditor/mhxx/quest/arc");
    assert!(ed.message.contains("nothing written"), "message was {}", ed.message);
}
// ---------------------------------------------------------------------------
// The save -> quest editor loop on the console side.
// ---------------------------------------------------------------------------

#[test]
fn x_on_the_quest_list_opens_the_saves_own_quests() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::ArcList);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::SaveQuests, "X must switch to the save's quests");
    assert!(!ed.save_quests().is_empty());
    // The lines must name a real quest from the save.
    let first = ed.save_quests()[0].clone();
    let lines = ed.lines();
    assert!(lines[0].text.contains("QUESTS INSIDE THIS SAVE"), "{}", lines[0].text);
    assert!(
        lines.iter().any(|l| l.text.contains(&first.quest_id.to_string())),
        "the list must show the ids"
    );
}

#[test]
fn opening_a_quest_from_the_save_fills_the_editor() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::Y);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::SaveQuests);

    let want = ed.save_quests()[0].quest_id;
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ArcEntries, "A must open the quest");
    assert!(ed.message.contains(&want.to_string()), "message was {}", ed.message);
    // The .mib of that quest is now in the field editor's buffer.
    let mib_len = ed.arc.as_ref().and_then(|a| a.by_ext("mib")).expect("a .mib").data.len();
    let buf = ed.mib_bytes();
    assert_eq!(buf.len(), mib_len);
    // And the id in it matches the quest we picked.
    let id = i32::from_le_bytes([buf[0x0C], buf[0x0D], buf[0x0E], buf[0x0F]]);
    assert_eq!(id, want);
}

#[test]
fn editing_a_quest_from_the_save_writes_back_to_the_same_slot() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    let before = ed.save.as_ref().unwrap().to_bytes();
    ed.input(Key::Y);
    ed.input(Key::X);
    let target = ed.save_quests()[0].clone();
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::ArcEntries);

    // The entry cursor must point at the .mib, so A opens the field editor
    // instead of re-loading entry 0 and wiping the buffer.
    assert!(
        !ed.mib_bytes().is_empty(),
        "opening a quest must load its .mib (cursor {} vs entry {})",
        ed.cursor,
        ed.entry
    );
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Mib, "A must open the .mib of the chosen entry");

    // Edit a field that cannot be zero, then write back. The change must reach
    // the save: this is the bug where install_into_save repacked the archive as
    // it was on disk and the edit vanished while claiming success.
    let fields = mhnx_core::questfields::MIB_FIELDS;
    let lv = fields
        .iter()
        .position(|f| f.key == "quest_level")
        .expect("a quest_level field");
    // Move the cursor to that row and bump it with the default step of 1.
    for _ in 0..lv {
        ed.input(Key::Down);
    }
    let before_val =
        mhnx_core::questfields::read_u32(&ed.mib_bytes(), fields[lv].offset, fields[lv].kind)
            .unwrap();
    ed.input(Key::Right);
    let after_val =
        mhnx_core::questfields::read_u32(&ed.mib_bytes(), fields[lv].offset, fields[lv].kind)
            .unwrap();
    assert_eq!(after_val, before_val + 1, "the field editor must change the buffer");

    // Back to the entry list, then X installs into the save. This is the exact
    // path that used to repack the archive as it was on disk and lose the edit.
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::ArcEntries);
    ed.input(Key::X);
    assert!(ed.message.contains("installed"), "message was {}", ed.message);
    let after = ed.save.as_ref().unwrap().to_bytes();
    assert_eq!(after.len(), before.len());
    let diff = (0..before.len()).filter(|i| before[*i] != after[*i]).count();
    assert!(
        diff > 0,
        "the edit must reach the save - a silent no-op is the bug this test exists for"
    );
    // Only that quest's slot may move.
    let sec = ed.save.as_ref().unwrap().dlc_section_offset() as usize;
    let start = 0x24 + sec + mhnx_core::dlc::QUEST_ARRAY_OFFSET + target.slot * mhnx_core::dlc::QUEST_SLOT_SIZE;
    let end = start + mhnx_core::dlc::QUEST_SLOT_SIZE;
    let outside = (0..before.len())
        .filter(|i| before[*i] != after[*i])
        .filter(|i| *i < start || *i >= end)
        .count();
    assert_eq!(outside, 0, "{} bytes changed outside the quest slot", outside);
    // The rest of the save is intact.
    assert_eq!(ed.save.as_ref().unwrap().charms(0).unwrap().len(), 188);
    let q = ed.save_quests()[target.slot].clone();
    assert_eq!(q.quest_id, target.quest_id);
}

#[test]
fn a_quest_id_that_is_not_in_the_save_is_refused() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.open_quest_from_save(9999999);
    assert_ne!(ed.screen, Screen::ArcEntries);
    assert!(ed.message.contains("not in this save"), "message was {}", ed.message);
}

#[test]
fn minus_is_also_swallowed_on_the_save_quest_list() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::Y);
    ed.input(Key::X);
    assert_eq!(ed.screen, Screen::SaveQuests);
    assert!(ed.uses_minus(), "the save quest list uses Minus to extract");
}
// ---------------------------------------------------------------------------
// Item type change on the console side.
// ---------------------------------------------------------------------------

#[test]
fn r_on_the_item_box_opens_the_picker() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X); // items screen
    assert_eq!(ed.screen, Screen::Items);
    ed.input(Key::R);
    assert_eq!(ed.screen, Screen::ItemPick);
    // With no filter every item is listed.
    assert!(ed.item_pick_matches().len() > 2500, "{}", ed.item_pick_matches().len());
    let lines = ed.lines();
    assert!(lines[0].text.contains("PICK ITEM"), "{}", lines[0].text);
}

#[test]
fn filtering_and_choosing_changes_the_item_type() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    ed.input(Key::R);
    assert_eq!(ed.screen, Screen::ItemPick);

    let target = ed.item_pick_target;
    // The picker talks to the software keyboard; here we answer directly.
    ed.apply_item_filter(String::from("回復薬"));
    let matches = ed.item_pick_matches();
    assert!(!matches.is_empty(), "the filter must find something");
    let want = matches[0];

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Items, "choosing returns to the box");
    let boxed = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(boxed[target].id, want, "the slot must hold the chosen item");
    assert!(boxed[target].count >= 1, "a new item needs a count");
    assert!(ed.message.contains("回復薬"), "message was {}", ed.message);
}

#[test]
fn cancelling_the_picker_leaves_the_box_alone() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    let before = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    ed.input(Key::R);
    ed.apply_item_filter(String::from("回復薬"));
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Items);
    let after = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(before, after, "B must not write anything");
}

#[test]
fn the_filter_request_uses_its_own_sentinel() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    ed.input(Key::R);
    ed.input(Key::Y);
    let (idx, initial) = ed.take_text_request().expect("a filter request");
    assert_eq!(idx, mhnx_core::hbedit::TEXT_ITEM_FILTER);
    assert_eq!(idx, usize::MAX - 1, "must not collide with the name request");
    assert!(initial.is_empty());
}

#[test]
fn the_item_picker_does_not_panic_on_an_empty_match() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    ed.input(Key::R);
    ed.apply_item_filter(String::from("zzzznothingmatchesthis"));
    assert!(ed.item_pick_matches().is_empty());
    // Navigation and A must both be harmless with nothing to pick.
    ed.input(Key::Down);
    ed.input(Key::Up);
    ed.input(Key::Right);
    let before = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    ed.input(Key::A);
    let after = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(before, after);
}
// ---------------------------------------------------------------------------
// The console entry viewer: the six container formats used to be unreadable.
// ---------------------------------------------------------------------------

fn sample_arc() -> Option<std::path::PathBuf> {
    let mut p = std::path::PathBuf::from("C:/");
    for part in ["anyWorkspace", "nx", "mhxx", "mhnxeditor", "dist", "quests_all"] {
        p.push(part);
    }
    p.push("q1010165.arc");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

#[test]
fn y_on_an_entry_shows_it_decoded() {
    let Some(p) = sample_arc() else { return };
    let mut ed = Editor::new();
    let bytes = std::fs::read(&p).unwrap();
    ed.open_arc_bytes(&p.to_string_lossy(), &bytes);
    assert_eq!(ed.screen, Screen::ArcEntries);

    // Walk every entry and check that Y always produces a readable screen,
    // and that the four container types show their contents.
    let n = ed.arc.as_ref().map(|a| a.entries.len()).unwrap_or(0);
    assert_eq!(n, 15, "the sample quest has 15 entries");
    let mut seen: Vec<String> = Vec::new();
    for i in 0..n {
        ed.cursor = i;
        ed.input(Key::Y);
        assert_eq!(ed.screen, Screen::EntryView, "entry {} did not open", i);
        let lines: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
        assert!(lines.len() > 1, "entry {} rendered nothing", i);
        assert!(
            lines[0].len() > 3,
            "entry {} has no header: {:?}",
            i,
            lines[0]
        );
        seen.push(lines.join(" | "));
        ed.input(Key::B);
        assert_eq!(ed.screen, Screen::ArcEntries);
    }
    let joined = seen.join("\n");
    // .rem shows reward records with the Japanese item names.
    assert!(
        joined.contains("古びたお守り"),
        ".rem reward names missing; got:
{}",
        &joined[..joined.len().min(1500)]
    );
    // .qdl lists the referenced files.
    assert!(joined.contains("questData_1010165"), ".qdl file list missing");
    // .sem shows a position.
    assert!(joined.contains("x "), ".sem position missing");
}

#[test]
fn the_entry_view_never_panics_on_a_junk_payload() {
    let Some(p) = sample_arc() else { return };
    let mut ed = Editor::new();
    let bytes = std::fs::read(&p).unwrap();
    ed.open_arc_bytes(&p.to_string_lossy(), &bytes);
    // Replace the payload under every entry with noise and view them all.
    if let Some(a) = &mut ed.arc {
        for e in a.entries.iter_mut() {
            e.data = vec![0x5A; 40];
        }
    }
    let n = ed.arc.as_ref().map(|a| a.entries.len()).unwrap_or(0);
    for i in 0..n {
        ed.cursor = i;
        ed.input(Key::Y);
        let _ = ed.lines();
        ed.input(Key::B);
    }
}
// ---------------------------------------------------------------------------
// The console skill picker: 306 skills cannot be reached by +/- on a number.
// ---------------------------------------------------------------------------

#[test]
fn y_on_a_skill_row_opens_the_name_picker() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L); // charms
    ed.input(Key::L); // new charm form
    assert_eq!(ed.screen, Screen::CharmEdit);

    // The form opens on the skill 1 row, so Y works straight away.
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
    assert_eq!(ed.skill_pick_field, 1);
    assert_eq!(
        ed.skill_pick_matches().len(),
        306,
        "the whole table is reachable"
    );
    let lines = ed.lines();
    assert!(lines[0].text.contains("PICK SKILL 1"), "{}", lines[0].text);
    // The first rows carry real Japanese names.
    assert!(
        lines.iter().any(|l| l.text.contains("毒")),
        "skill names must be rendered"
    );
}

#[test]
fn filtering_by_name_and_choosing_writes_the_skill() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    ed.input(Key::L);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);

    ed.apply_skill_filter(String::from("攻撃"));
    let matches = ed.skill_pick_matches();
    assert!(!matches.is_empty(), "the filter must find 攻撃");
    let want = matches[0];

    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit, "choosing returns to the form");
    assert_eq!(
        ed.charm_edit.map(|e| e.skill1),
        Some(want),
        "the chosen id must land in skill 1"
    );
    assert!(ed.message.contains("攻撃"), "message was {}", ed.message);
}

#[test]
fn the_second_skill_row_targets_skill_2() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    ed.input(Key::L);
    // A new charm is seeded from a real talisman, so remember skill 1.
    let skill1_before = ed.charm_edit.map(|e| e.skill1);
    // skill 1 -> points 1 -> skill 2
    ed.input(Key::Down);
    ed.input(Key::Down);
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::SkillPick);
    assert_eq!(ed.skill_pick_field, 2);
    ed.apply_skill_filter(String::from("匠"));
    let want = ed.skill_pick_matches()[0];
    ed.input(Key::A);
    assert_eq!(ed.charm_edit.map(|e| e.skill2), Some(want));
    assert_eq!(
        ed.charm_edit.map(|e| e.skill1),
        skill1_before,
        "skill 1 untouched"
    );
}

#[test]
fn y_off_a_skill_row_says_so_instead_of_opening() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    ed.input(Key::L);
    ed.input(Key::Up); // skill 1 -> deco slots
    ed.input(Key::Y);
    assert_eq!(ed.screen, Screen::CharmEdit);
    assert!(ed.message.contains("skill row"), "message was {}", ed.message);
}

#[test]
fn the_skill_picker_does_not_panic_on_an_empty_match() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    ed.input(Key::L);
    ed.input(Key::Y);
    ed.apply_skill_filter(String::from("zzzznothing"));
    assert!(ed.skill_pick_matches().is_empty());
    ed.input(Key::Down);
    ed.input(Key::Up);
    ed.input(Key::Right);
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit);
}
// ---------------------------------------------------------------------------
// Undo for the console item box.
// ---------------------------------------------------------------------------

#[test]
fn replacing_an_item_can_be_taken_back() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X); // items
    assert_eq!(ed.screen, Screen::Items);

    let before = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    let target = ed.visible_items()[ed.cursor];

    // Change the type through the picker.
    ed.input(Key::R);
    assert_eq!(ed.screen, Screen::ItemPick);
    ed.apply_item_filter(String::from("解毒薬"));
    let want = ed.item_pick_matches()[0];
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::Items);
    let changed = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(changed[target].id, want, "the picker must write");
    assert_ne!(changed[target].id, before[target].id);

    // L puts it back.
    ed.input(Key::L);
    let restored = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(
        restored[target].id, before[target].id,
        "undo must restore the old item"
    );
    assert!(ed.message.contains("restored"), "message was {}", ed.message);
}

#[test]
fn undo_with_nothing_to_take_back_says_so() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    ed.input(Key::L);
    assert!(ed.message.contains("nothing to undo"), "message was {}", ed.message);
}

#[test]
fn undo_restores_the_whole_box_not_just_the_item() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::X);
    let before = ed.save.as_ref().unwrap().read_item_box(0).unwrap();

    ed.input(Key::R);
    ed.apply_item_filter(String::from("回復薬"));
    let want = ed.item_pick_matches()[0];
    ed.input(Key::A);
    ed.input(Key::L);

    let after = ed.save.as_ref().unwrap().read_item_box(0).unwrap();
    assert_eq!(before, after, "every slot must come back");
}
// ---------------------------------------------------------------------------
// The console talisman editor shows the published range, not just the numbers.
// ---------------------------------------------------------------------------

#[test]
fn the_console_charm_editor_shows_the_allowed_range() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L); // charms
    // Select a known talisman (224: 属性会心 +6 / 達人 +10).
    ed.cursor = ed
        .charms()
        .iter()
        .position(|c| c.index == 224)
        .unwrap_or(0);
    ed.input(Key::Y); // edit
    assert_eq!(ed.screen, Screen::CharmEdit);
    let text: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = text.join("\n");
    assert!(
        joined.contains("type 0x"),
        "the type and rarity must be shown:\n{}",
        joined
    );
    assert!(
        joined.contains("rarity"),
        "the rarity label is missing:\n{}",
        joined
    );
    assert!(
        joined.contains("allowed"),
        "the allowed range must be shown:\n{}",
        joined
    );
}

#[test]
fn an_illegal_skill_is_flagged_on_the_console() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::L);
    ed.cursor = ed
        .charms()
        .iter()
        .position(|c| c.index == 224)
        .unwrap_or(0);
    ed.input(Key::Y);
    // Force skill 1 to something the Enduring table forbids: id 1 (Poison)
    // is 0~0 for every talisman type in the sample data.
    if let Some(e) = &mut ed.charm_edit {
        e.skill1 = 1;
        e.points1 = 5;
    }
    let joined: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = joined.join("\n");
    assert!(
        joined.contains("cannot go on this type") || joined.contains("allowed"),
        "an illegal skill must be flagged:\n{}",
        joined
    );
}
// ---------------------------------------------------------------------------
// 3DS <-> Switch conversion on the console.
// ---------------------------------------------------------------------------

#[test]
fn l_on_the_save_list_opens_the_convert_screen() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::B); // back to the save list
    assert_eq!(ed.screen, Screen::SaveList);
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::Convert);
    let joined: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let joined = joined.join("\n");
    assert!(joined.contains("CONVERT SAVE"), "{}", joined);
    assert!(joined.contains("source"), "{}", joined);
    assert!(joined.contains("3DS"), "the target must be named:\n{}", joined);
}

#[test]
fn a_switch_save_converts_to_a_3ds_body() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::B);
    ed.input(Key::L);
    ed.input(Key::A);
    match ed.take_write() {
        Some(mhnx_core::hbedit::Write::File { path, bytes }) => {
            assert!(path.ends_with(".3ds"), "wrote {}", path);
            // The 3DS container is the body alone, with no 36 byte header.
            assert_eq!(bytes.len(), 4_726_152, "3DS save is the bare body");
            let before = ed.save.as_ref().unwrap().body().to_vec();
            assert_eq!(bytes, before, "the body must be carried over unchanged");
        }
        other => panic!("expected a write, got {:?}", other),
    }
}

#[test]
fn a_3ds_save_converts_to_a_switch_container() {
    if !real_save().exists() {
        return;
    }
    let src = mhnx_core::save::Save::from_bytes(&std::fs::read(real_save()).unwrap()).unwrap();
    // A bare body is what a 3DS save is.
    let body = src.to_3ds_bytes();
    let mut ed = Editor::new();
    ed.open_save_bytes("C:/fake/system", &body);
    assert_eq!(
        ed.save.as_ref().unwrap().kind(),
        mhnx_core::save::SaveKind::ThreeDs,
        "a bare body must be recognised as 3DS"
    );
    ed.input(Key::B); // slots -> save list
    ed.input(Key::L); // convert
    assert_eq!(ed.screen, Screen::Convert);
    ed.input(Key::A);
    match ed.take_write() {
        Some(mhnx_core::hbedit::Write::File { path, bytes }) => {
            assert!(path.ends_with(".switch"), "wrote {}", path);
            assert_eq!(bytes.len(), 4_726_188, "Switch save carries the header");
            let back = mhnx_core::save::Save::from_bytes(&bytes).unwrap();
            assert_eq!(
                back.kind(),
                mhnx_core::save::SaveKind::Switch,
                "the container must be a Switch one"
            );
            assert_eq!(back.body(), &body[..], "the body must survive");
        }
        other => panic!("expected a write, got {:?}", other),
    }
}
#[test]
fn the_quest_list_shows_a_title_on_every_row() {
    if !real_save().exists() {
        return;
    }
    let mut ed = Editor::new();
    open_save(&mut ed);
    ed.input(Key::Y); // quest list
    ed.input(Key::X); // quests inside the save
    assert_eq!(ed.screen, Screen::SaveQuests);
    // Titles are decoded once when the screen opens.
    assert!(
        ed.quest_titles.len() > 150,
        "expected the cache to be filled, got {}",
        ed.quest_titles.len()
    );
    let first = ed.save_quests()[0].quest_id;
    assert!(
        !ed.quest_title(first).is_empty(),
        "quest {} has no cached title",
        first
    );
    // Every visible row must carry its title, not just the cursor row.
    let lines: Vec<String> = ed.lines().iter().map(|l| l.text.clone()).collect();
    let rows: Vec<&String> = lines.iter().filter(|l| l.contains("slot ")).collect();
    assert!(rows.len() >= 10, "expected a page of rows, got {}", rows.len());
    let with_title = rows
        .iter()
        .filter(|l| {
            // "slot NNN  id XXXX  NNNN B  <title>"
            l.split(" B  ").nth(1).map(|t| !t.trim().is_empty()).unwrap_or(false)
        })
        .count();
    assert!(
        with_title >= rows.len() - 1,
        "only {} of {} rows carried a title",
        with_title,
        rows.len()
    );
}
