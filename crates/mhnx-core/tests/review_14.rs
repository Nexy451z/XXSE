//! The thirteen findings of the fourth review, pinned one by one.
//!
//! Each test names the function the finding was about, so a regression points
//! at the code rather than at the screen. Four of them read switch/src/ui.rs and
//! switch/src/editor.rs as text: the console's dispatch cannot be driven from a
//! host test, so what is pinned there is the wiring (the console uses the state
//! machine's answer) rather than the pixels.
//!
//! Nothing here writes a card: the saves are built in memory, or opened from the
//! machine's sample save through the same request/answer path the console uses.

use std::path::PathBuf;

use mhnx_core::arc::{Arc, ArcEntry, ARC_VERSION};
use mhnx_core::hbedit::{
    Editor, EquipScope, Key, MibPickTarget, Request, Screen, Write, GUILD_ROW_COMMENT,
    QUEST_TAB_MONSTERS, QUEST_TAB_REWARDS, STEPS, TEXT_ITEM_SET_NAME,
};
use mhnx_core::quest::{Gmd, GMD_TAG, GMD_VERSION, MIB_SIZE};
use mhnx_core::save::{Save, BODY_SIZE, SLOT_STRIDE};

/// Body offset of the first character slot (also stored at body[0x10]).
const FIRST_SLOT_BASE: u32 = 0x126474;

const HASH_SEM: u32 = 0x2553_701D;
const HASH_REM: u32 = 0x5B3C_302D;
const HASH_MIB: u32 = 0x1BBF_D18E;
const HASH_GMD: u32 = 0x242B_B29A;

// ---------------------------------------------------------------------------
// The two layers, as text
// ---------------------------------------------------------------------------

fn ui_src() -> &'static str {
    include_str!("../../../switch/src/ui.rs")
}

fn editor_src() -> &'static str {
    include_str!("../../../switch/src/editor.rs")
}

/// The body of one function, from its declaration to the next item.
fn fn_body<'a>(src: &'a str, decl: &str) -> &'a str {
    let start = src
        .find(decl)
        .unwrap_or_else(|| panic!("{decl} is not in the console sources"));
    let rest = &src[start + decl.len()..];
    let mut end = rest.len();
    for pat in ["\n    fn ", "\n    pub fn ", "\n    #[allow("] {
        if let Some(i) = rest.find(pat) {
            end = end.min(i);
        }
    }
    &rest[..end]
}

// ---------------------------------------------------------------------------
// Saves and quests, built in memory
// ---------------------------------------------------------------------------

fn blank_body() -> Vec<u8> {
    let mut body = vec![0u8; BODY_SIZE];
    body[0] = 0xC6;
    for slot in 0..3 {
        let base = FIRST_SLOT_BASE as usize + slot * SLOT_STRIDE;
        body[0x10 + 4 * slot..0x14 + 4 * slot].copy_from_slice(&(base as u32).to_le_bytes());
        body[0x04 + slot] = 1;
    }
    body
}

/// An editor with the synthetic save open, on the slot list.
fn open_blank() -> Editor {
    let save = Save::from_bytes(&blank_body()).expect("the synthetic save must parse");
    let mut ed = Editor::new();
    ed.save = Some(save);
    ed.save_path = Some(String::from("memory:review-14"));
    ed.slot = 0;
    ed.screen = Screen::Slots;
    ed
}

/// The machine's sample save, or None when this checkout has no card copy.
fn real_save() -> Option<PathBuf> {
    let stable = PathBuf::from(r"C:\anyWorkspace\nx\jksv_ready\system");
    if stable.exists() {
        return Some(stable);
    }
    let ryujinx = PathBuf::from(
        r"C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system",
    );
    ryujinx.exists().then_some(ryujinx)
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

/// Open the sample save through the same path the console uses.
fn open_real_save() -> Option<Editor> {
    let p = real_save()?;
    let mut ed = Editor::new();
    ed.set_save_files(vec![p.to_string_lossy().to_string()]);
    ed.input(Key::A);
    service(&mut ed);
    assert_eq!(ed.screen, Screen::Slots, "the save opens on the slot screen");
    Some(ed)
}

fn mib_bytes(id: u32, minutes: u8) -> Vec<u8> {
    let mut mib = vec![0u8; MIB_SIZE];
    mib[0x0C..0x10].copy_from_slice(&id.to_le_bytes());
    mib[0x16] = minutes;
    for (k, (mid, count)) in [(85u16, 1u8), (32, 2)].iter().enumerate() {
        let o = 100 + 13 * k;
        mib[o..o + 2].copy_from_slice(&mid.to_le_bytes());
        mib[o + 3] = 1;
        mib[o + 4] = *count;
        mib[o + 5] = 50;
        mib[o + 6] = 50;
        mib[o + 7] = 50;
        mib[o + 9..o + 11].copy_from_slice(&100u16.to_le_bytes());
        mib[o + 12] = 1;
    }
    mib
}

fn gmd_bytes() -> Vec<u8> {
    Gmd {
        version: GMD_VERSION,
        tag: GMD_TAG,
        key: String::from("questData_1010165"),
        strings: vec![
            String::from("テストクエスト"),
            String::from("受付嬢"),
            String::from("依頼の内容"),
            String::from("リオレウス"),
            String::from("リオレウス1頭の狩猟"),
            String::from("3回力尽きる"),
            String::from("サブ依頼"),
        ],
    }
    .to_bytes()
}

/// 184 byte reward table: one group of three rows whose chances add up to 100.
fn rem_bytes() -> Vec<u8> {
    let mut v = vec![0u8; 184];
    v[0..4].copy_from_slice(&1.0f32.to_le_bytes());
    v[4..8].copy_from_slice(&1u32.to_le_bytes());
    v[8] = 1;
    v[9] = 3;
    let rows: [[u8; 4]; 4] = [
        [0x63, 0x01, 1, 50],
        [0x55, 0x03, 2, 30],
        [0xB4, 0x06, 1, 20],
        [0, 0, 0, 0xFF],
    ];
    for (i, r) in rows.iter().enumerate() {
        v[24 + i * 4..24 + i * 4 + 4].copy_from_slice(r);
    }
    v
}

/// One 32 byte .sem: wave +0x08, area +0x0C, x/y/z/w +0x10..+0x20.
fn sem_bytes(area: u32, x: f32, y: f32, z: f32, w: f32, wave: u32) -> Vec<u8> {
    let mut v = vec![0u8; 32];
    v[0..4].copy_from_slice(&1.0f32.to_le_bytes());
    v[4..8].copy_from_slice(&1u32.to_le_bytes());
    v[8..12].copy_from_slice(&wave.to_le_bytes());
    v[0x0C..0x10].copy_from_slice(&area.to_le_bytes());
    v[0x10..0x14].copy_from_slice(&x.to_le_bytes());
    v[0x14..0x18].copy_from_slice(&y.to_le_bytes());
    v[0x18..0x1C].copy_from_slice(&z.to_le_bytes());
    v[0x1C..0x20].copy_from_slice(&w.to_le_bytes());
    v
}

fn entry(name: &str, type_hash: u32, data: Vec<u8>) -> ArcEntry {
    ArcEntry {
        name: String::from(name),
        type_hash,
        data,
        original: None,
        original_offset: None,
    }
}

/// A whole quest, in the shape q1010165 has.
fn quest_arc(id: u32, minutes: u8) -> Arc {
    Arc {
        version: ARC_VERSION,
        header8: 0xFFFF_FFFE,
        entries: vec![
            entry(
                "quest\\boss\\setEmMain\\b_m00em000_00",
                HASH_SEM,
                sem_bytes(0, 0.0, 0.0, 0.0, 0.0, 0),
            ),
            entry(
                "quest\\boss\\setEmMain\\b_m15em010_00",
                HASH_SEM,
                sem_bytes(1, 180.0, 23.0, -150.0, 90.0, 0),
            ),
            entry(
                "quest\\boss\\setEmMain\\b_m15em046_00",
                HASH_SEM,
                sem_bytes(2, 147.0, 0.0, 0.0, 45.0, 1),
            ),
            entry("quest\\rem\\rem_000000", HASH_REM, vec![0u8; 184]),
            entry("quest\\rem\\rem_620114", HASH_REM, rem_bytes()),
            entry(
                "quest\\questData\\questData_1010165_jpn",
                HASH_GMD,
                gmd_bytes(),
            ),
            entry(
                "quest\\questData\\questData_1010165",
                HASH_MIB,
                mib_bytes(id, minutes),
            ),
        ],
    }
}

/// Open a quest through the byte path the console uses.
fn open_quest(id: u32, minutes: u8) -> Editor {
    let mut ed = Editor::new();
    let bytes = quest_arc(id, minutes).to_bytes().expect("the archive rebuilds");
    ed.open_arc_bytes("sdmc:/quest/q1010165.arc", &bytes);
    ed.enter_quest_edit();
    assert_eq!(ed.screen, Screen::QuestEdit, "{}", ed.message);
    ed
}

/// The bytes of one archive entry, by name suffix.
fn entry_bytes(ed: &Editor, suffix: &str) -> Vec<u8> {
    ed.arc
        .as_ref()
        .expect("an archive")
        .entries
        .iter()
        .find(|e| e.name.ends_with(suffix))
        .map(|e| e.data.clone())
        .unwrap_or_else(|| panic!("no entry ending in {}", suffix))
}

// ---------------------------------------------------------------------------
// 1: [X] on クエスト改造 writes the file, exactly as it does on the file screens
// ---------------------------------------------------------------------------

/// The クエスト改造 branch of Editor::quick_save called install_into_save and
/// stopped there: the status line said 上書きしました and the card was
/// untouched. The four file screens beside it call both.
#[test]
fn the_quest_screens_save_key_writes_the_file() {
    let mut ed = open_blank();
    let bytes = quest_arc(1_010_165, 50).to_bytes().expect("the archive rebuilds");
    ed.open_arc_bytes("sdmc:/quest/q1010165.arc", &bytes);
    // A quest that came out of the save has no path of its own: that is the
    // branch the finding is about.
    ed.arc_path = None;
    ed.enter_quest_edit_from(Screen::SaveQuests);
    assert_eq!(ed.screen, Screen::QuestEdit);

    ed.quick_save();

    let write = ed
        .take_write()
        .expect("[X] on クエスト改造 must ask for the file write");
    let Write::File { path, bytes } = write;
    assert_eq!(path, "memory:review-14", "the open save is the file to write");
    assert!(!bytes.is_empty());
    assert!(
        ed.save_quests().iter().any(|q| q.quest_id == 1_010_165),
        "the quest has to be in the save the write carries: {}",
        ed.message
    );
    assert!(
        ed.message.contains("書き込み中"),
        "the line has to say the write is going out: {}",
        ed.message
    );
    // The install's own sentence is not lost on the way: the user asked what
    // happened to the slot, and 書き込み中 alone does not answer it.
    assert!(
        ed.message.contains("枠"),
        "the line still names the slot the quest landed in: {}",
        ed.message
    );
}

// ---------------------------------------------------------------------------
// 2: [X] on the two forms writes the draft before it saves
// ---------------------------------------------------------------------------

/// The talisman form's [X] asked for the write and left the form where it was:
/// the line said 保存しました about a save that did not hold the record on
/// screen, and the [B] after it threw the draft away.
#[test]
fn the_save_key_on_the_talisman_form_writes_the_draft() {
    let Some(mut ed) = open_real_save() else {
        return;
    };
    ed.open_charms();
    assert_eq!(ed.screen, Screen::Charms);
    if ed.charms().is_empty() {
        return;
    }
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit, "{}", ed.message);
    let before = ed.charm_edit.expect("the form");
    // The form lands on スキル1, and one press on it changes the draft.
    ed.input(Key::Right);
    let after = ed.charm_edit.expect("the form");
    assert!(
        after.skill1 != before.skill1,
        "the press has to change the draft for the test to mean anything"
    );

    ed.quick_save();

    let form = ed.charm_edit.expect("the form is still readable");
    let recorded = ed
        .charms()
        .into_iter()
        .find(|c| c.index == form.index)
        .expect("the record the form was on");
    assert_eq!(
        recorded.slots, form.slots,
        "the save must hold the draft the form was showing"
    );
    assert_eq!(recorded.skill1, form.skill1);
    assert_eq!(recorded.points1, form.points1);
    assert!(
        ed.take_write().is_some(),
        "and the file write is asked for after the commit"
    );
    assert!(!ed.charm_draft_dirty(), "nothing is left in the form");
}

/// The equipment form's [X] had the same shape: the draft stayed in the form
/// and the save went out without it.
#[test]
fn the_save_key_on_the_equipment_form_writes_the_draft() {
    let mut ed = open_blank();
    ed.screen = Screen::Equip;
    // [+] opens the new-record form, which is the branch that creates a record.
    ed.input(Key::Plus);
    assert_eq!(ed.screen, Screen::EquipEdit, "{}", ed.message);
    let form = ed.equip_edit.expect("the new form");
    assert!(form.new);
    // 種別 / ID / レベル: the level row is the third.
    ed.input(Key::Down);
    ed.input(Key::Down);
    ed.input(Key::Right);
    let form = ed.equip_edit.expect("the form");
    assert_eq!(form.level, 2, "the press has to change the draft");

    ed.quick_save();

    assert!(
        !ed.equip_draft_dirty(),
        "the draft has to be committed by the save key: {}",
        ed.message
    );
    assert!(
        ed.equipment()
            .iter()
            .any(|q| q.kind == form.kind && q.id == form.id && q.level == 2),
        "the record the form described is in the box: {}",
        ed.message
    );
    assert!(ed.take_write().is_some(), "and the file write is asked for");
}

/// A draft the form cannot commit stops the save instead of writing a file that
/// does not hold it.
#[test]
fn a_draft_that_cannot_be_committed_is_said_out_loud() {
    let mut ed = open_blank();
    ed.open_charms();
    // A blank box has no talisman to copy the kind and rarity from, so the
    // write is refused - and the save key must not go on to write the file.
    ed.input(Key::L);
    assert_eq!(ed.screen, Screen::CharmEdit, "{}", ed.message);
    ed.input(Key::Right);
    assert!(ed.charm_draft_dirty(), "the form holds a draft");

    ed.quick_save();

    assert!(
        ed.take_write().is_none(),
        "a save that lacks the draft must not be written"
    );
    assert!(
        ed.message.contains("下書きが残っています"),
        "and the line has to say so: {}",
        ed.message
    );
}

// ---------------------------------------------------------------------------
// 3: [X] in the name keyboard asks before it empties the field
// ---------------------------------------------------------------------------

/// handle_osk_key answered [X] with buffer.clear(): one press, next to the [B]
/// that removes one character, and a name typed over a minute was gone.
#[test]
fn the_name_keyboards_clear_key_asks_twice() {
    let body = fn_body(ui_src(), "pub fn handle_osk_key");
    assert!(
        body.contains("osk.clear.press()"),
        "the clear has to be the two-press rule: {body}"
    );
    assert!(
        !body.contains("osk.buffer.clear();\n                self.osk = Some(osk);"),
        "the clear must not run on the first press: {body}"
    );
    // The legend names the key and the second press, so the rule is not a
    // secret the user has to discover.
    let draw = fn_body(ui_src(), "fn draw_osk");
    assert!(draw.contains("[X] 全消去（2回）"), "{draw}");
    // The touch button takes the same two presses: a finger and a key must not
    // cost a different number of presses for the same delete.
    let touch = fn_body(ui_src(), "UiAction::OskClear =>");
    assert!(touch.contains("osk.clear.press()"), "{touch}");
}

// ---------------------------------------------------------------------------
// 4: a tapped area writes the .sem, and the slot is always disarmed
// ---------------------------------------------------------------------------

/// The エリア chooser is the .mib picker modal with a different writer, and the
/// tap path called the .mib writer with a field index that names nothing - so a
/// tapped area wrote nothing at all while the pad's [A] wrote the .sem. The
/// slot stayed armed, so the next monster pick wrote the area instead.
#[test]
fn a_tapped_area_writes_the_sem_and_disarms_the_slot() {
    let mut ed = open_quest(1_010_165, 50);
    let before = entry_bytes(&ed, "b_m15em010_00");
    assert_eq!(
        u32::from_le_bytes([before[0x0C], before[0x0D], before[0x0E], before[0x0F]]),
        1,
        "the sample placement starts on area 1"
    );

    // The old tap path: the .mib writer with the "no field" index the chooser
    // used, which writes nothing.
    assert!(
        !ed.apply_mib_pick(MibPickTarget::Field(usize::MAX), 5, "エリア 5"),
        "a pick that names no row writes nothing, and has to say so"
    );

    // The tap path now: the area writer, the same one the pad's [A] uses.
    assert!(ed.apply_mib_pick(MibPickTarget::Area(0), 5, "エリア 5"));
    let after = entry_bytes(&ed, "b_m15em010_00");
    assert_eq!(
        u32::from_le_bytes([after[0x0C], after[0x0D], after[0x0E], after[0x0F]]),
        5,
        "the tapped area is in the .sem"
    );
    assert!(ed.dirty);

    // And the console disarms the slot on every way out of the chooser.
    let select = fn_body(ui_src(), "UiAction::MibPickerSelect(idx) =>");
    assert!(select.contains("quest_area_slot.take()"), "{select}");
    assert!(select.contains("MibPickTarget::Area"), "{select}");
    let close = fn_body(ui_src(), "UiAction::MibPickerClose =>");
    assert!(close.contains("quest_area_slot = None"), "{close}");
    let open = fn_body(ui_src(), "pub fn open_mib_picker");
    assert!(open.contains("quest_area_slot = None"), "{open}");
}

// ---------------------------------------------------------------------------
// 5: one [＋] press is one quit question
// ---------------------------------------------------------------------------

/// The frame answered [＋] with press_quit, and then handed the same press to
/// the state machine, whose every screen calls press_quit again - so the arm
/// and the confirmation were the same press, and one [＋] on a dirty save ended
/// the session with the edit still in memory.
#[test]
fn one_plus_press_is_one_quit_question() {
    // The mechanism: the second call is the one that quits.
    let mut ed = open_blank();
    ed.dirty = true;
    assert!(!ed.press_quit(), "the first press only offers the quit");
    assert!(ed.quit_armed());
    assert!(ed.press_quit(), "the second press is the one that quits");

    // The wiring: the console spends the press it answered.
    let body = fn_body(editor_src(), "if down & BTN_PLUS != 0 && !plus_is_a_tool {");
    assert!(body.contains("ed.press_quit()"), "{body}");
    assert!(
        body.contains("key = None"),
        "the same frame must not hand [＋] to the screen as well: {body}"
    );
}

// ---------------------------------------------------------------------------
// 6: a press that changes nothing leaves no undo step
// ---------------------------------------------------------------------------

/// input_guild pushed an undo step before it looked at the row, and
/// input_options pushed one before it looked at the value: a press on the
/// comment row, or on a counter already at its end, left a step that took the
/// save back to where it already was.
#[test]
fn a_press_that_changes_nothing_leaves_no_undo_step() {
    let mut ed = open_blank();
    ed.screen = Screen::Guild;
    ed.set_guild_view(mhnx_core::hbedit::GUILD_TAB_CARD);

    // The comment row has no number to step.
    ed.guild_field = GUILD_ROW_COMMENT;
    let depth = ed.undo_depth();
    let dirty = ed.dirty;
    ed.input(Key::Right);
    assert_eq!(
        ed.undo_depth(),
        depth,
        "a press that changed nothing left a step"
    );
    assert_eq!(ed.dirty, dirty, "and it must not mark the save dirty");
    assert!(
        ed.message.contains("ランク") || ed.message.contains("[A]"),
        "the press has to say where it does work: {}",
        ed.message
    );

    // A counter already at its end.
    let mut save = ed.save.take().expect("the save");
    assert!(mhnx_core::guildcard::set_hunter_rank(&mut save, 0, 999));
    ed.save = Some(save);
    ed.guild_field = 0;
    let depth = ed.undo_depth();
    ed.input(Key::Right);
    assert_eq!(ed.undo_depth(), depth, "999 cannot go up");
    assert_eq!(
        mhnx_core::guildcard::hunter_rank(ed.save.as_ref().unwrap(), 0),
        Some(999)
    );

    // And the one row that clamps: brightness at its ceiling.
    let mut save = ed.save.take().expect("the save");
    let mut o = save.options();
    o.brightness = 255;
    save.set_options(&o);
    ed.save = Some(save);
    ed.screen = Screen::Options;
    ed.option_cursor = 0;
    let depth = ed.undo_depth();
    ed.input(Key::Right);
    assert_eq!(ed.undo_depth(), depth, "255 cannot go up");
    assert_eq!(ed.save.as_ref().unwrap().options().brightness, 255);

    // A press that does change something still leaves its step.
    ed.input(Key::Left);
    assert_eq!(ed.undo_depth(), depth + 1);
    assert_eq!(ed.save.as_ref().unwrap().options().brightness, 254);
}

// ---------------------------------------------------------------------------
// 7: a sub-tab or a section jump asks before it drops a form's draft
// ---------------------------------------------------------------------------

/// select_sub_tab and goto_section called open_charms / close_set_form
/// unconditionally, so a shoulder press or a ZL / ZR jump threw away a draft
/// that [B] asks about.
#[test]
fn a_jump_that_leaves_a_form_asks_twice() {
    let Some(mut ed) = open_real_save() else {
        return;
    };
    ed.open_charms();
    if ed.charms().is_empty() {
        return;
    }
    ed.input(Key::A);
    assert_eq!(ed.screen, Screen::CharmEdit);
    ed.input(Key::Right);
    assert!(ed.charm_draft_dirty(), "the form holds a draft");
    let draft = ed.charm_edit.expect("the form");

    assert_eq!(ed.draft_form_name(), Some("護石編集"));
    assert!(ed.form_leave_press(), "the first press only asks");
    assert!(
        ed.message.contains("下書き"),
        "the question has to name the draft: {}",
        ed.message
    );
    assert_eq!(
        ed.charm_edit.map(|c| c.slots),
        Some(draft.slots),
        "the draft is still there after the question"
    );
    assert!(!ed.form_leave_press(), "the second press goes through");

    // The console asks before both jumps.
    for decl in ["pub fn select_sub_tab", "fn goto_section"] {
        let body = fn_body(ui_src(), decl);
        assert!(body.contains("ed.form_leave_press()"), "{decl}: {body}");
    }
}

// ---------------------------------------------------------------------------
// 8: the item set's name is a key the pad can press
// ---------------------------------------------------------------------------

/// request_item_set_name was only ever reached from Key::Z, and the console
/// used to spend ZR on the section bar before the state machine saw it. The
/// state machine answers ZrJob::Screen on the item set form now; this pins that
/// the key arrives, and that a finger has the same route.
#[test]
fn the_item_set_name_is_a_key_the_pad_can_press() {
    let mut ed = open_blank();
    ed.screen = Screen::Sets;
    assert!(ed.open_item_set_editor(0), "{}", ed.message);
    assert_eq!(
        ed.zr_job(),
        mhnx_core::hbedit::ZrJob::Screen,
        "the console has to hand ZR to the state machine on this form"
    );

    ed.input(Key::Z);

    assert_eq!(
        ed.take_text_request().map(|(id, _)| id),
        Some(TEXT_ITEM_SET_NAME),
        "ZR has to open the set-name keyboard"
    );
    // The touch route: the deck's pill sends the same key.
    assert!(
        ui_src().contains("[ZR] セット名"),
        "the item set form needs a pill a finger can press"
    );
}

// ---------------------------------------------------------------------------
// 9: the talisman list's [B] says where it goes
// ---------------------------------------------------------------------------

/// [B] on 護石一覧 goes to the character select, not to the screen before it,
/// and the footer said 戻る - a word that promised the other one.
#[test]
fn the_talisman_list_says_where_b_goes() {
    let mut ed = open_blank();
    ed.open_charms();
    let hint = ed.footer_hint();
    assert!(hint.contains("[B] 枠選択"), "{hint}");
    assert!(!hint.contains("[B] 戻る"), "{hint}");
    // And the guide is telling the truth.
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::Slots);
}

// ---------------------------------------------------------------------------
// 10: the update banner names no key it cannot keep
// ---------------------------------------------------------------------------

/// The banner ended with "[A] 詳細", and [A] only opens the dialog on the start
/// screen: on the other twenty the same [A] is the screen's own key.
#[test]
fn the_update_banner_names_no_key_it_cannot_keep() {
    let v = mhnx_core::update::Version::parse("v0.4.1").expect("a version");
    let text = mhnx_core::update::banner_text(&v);
    assert!(!text.contains("[A]"), "the line must not promise [A]: {text}");
    assert!(text.contains("v0.4.1"), "{text}");

    // The console adds the pad's route where the pad really has one.
    let guide = fn_body(ui_src(), "fn draw_guide");
    assert!(
        guide.contains("ed.screen == Screen::SaveList"),
        "the banner has to know where [A] works: {guide}"
    );
    let banner = fn_body(ui_src(), "fn draw_update_banner");
    assert!(banner.contains("a_opens_it"), "{banner}");
    assert!(banner.contains("タップで詳細"), "{banner}");
}

// ---------------------------------------------------------------------------
// 11: the new-equipment form offers the whole box
// ---------------------------------------------------------------------------

/// The new-record form starts on kind 1 (a piece of armour), and
/// open_equip_picker scoped the list by the form's own kind - so creating a
/// weapon meant walking the 種別 row to a weapon kind first. A pick carries both
/// halves already, so the new form opens the whole box.
#[test]
fn the_new_equipment_form_offers_the_whole_box() {
    let mut ed = open_blank();
    ed.screen = Screen::Equip;
    ed.input(Key::Plus);
    assert_eq!(ed.screen, Screen::EquipEdit);
    assert!(ed.equip_edit.map(|e| e.new).unwrap_or(false));

    assert!(ed.open_equip_picker(), "{}", ed.message);
    assert_eq!(ed.screen, Screen::EquipPick);
    assert_eq!(ed.equip_pick_scope, EquipScope::All);
    assert_eq!(ed.equip_pick_kind, 0, "the whole box starts unfiltered");
    let choices = ed.equip_pick_choices();
    assert!(
        choices.iter().any(|c| c.kind == 7),
        "a weapon has to be in the list the new form opens"
    );
    assert!(
        choices.iter().any(|c| (1..=5).contains(&c.kind)),
        "and so does armour"
    );

    // One search and one pick: the type and the id arrive together, so the
    // 種別 row never has to be walked to reach a weapon.
    let hit = ed
        .equip_pick_choices()
        .into_iter()
        .find(|c| c.kind == 7 && c.id > 0 && c.name() != "-" && c.name() != "(未使用)")
        .expect("the weapon table names at least one 大剣");
    ed.apply_equip_pick_filter(String::from(hit.name()));
    let pos = ed
        .equip_pick_choices()
        .iter()
        .position(|c| *c == hit)
        .expect("the row the search found is in the list");
    ed.equip_pick_cursor = pos;
    ed.input(Key::A);

    let form = ed.equip_edit.expect("the form");
    assert_eq!(form.kind, hit.kind, "the pick carries the type");
    assert_eq!(form.id, hit.id, "and the id");
    assert!(form.kind >= 7, "a weapon kind, without walking the type row");
    // And the pick left the cursor on the row that writes.
    assert_eq!(form.field, form.write_row());
}

// ---------------------------------------------------------------------------
// 12: クエスト改造 has its step key back
// ---------------------------------------------------------------------------

/// [Y] on クエスト改造 opened the ▸詳細 rows, so the step size could not be
/// changed and the reward chance 10% → 50% was forty presses of [▶].
#[test]
fn the_quest_screens_step_key_is_y_again() {
    let mut ed = open_quest(1_010_165, 50);
    let (table, key) = ed.step_table();
    assert_eq!(key, "Y", "the step pills have to name the key again");
    assert_eq!(table, &STEPS[..]);

    ed.quest_edit_tab = QUEST_TAB_REWARDS;
    ed.quest_edit_cursor = 0;
    ed.input(Key::Z);
    assert!(ed.quest_edit_chance, "the chance is the value [◀/▶] steps");
    assert_eq!(ed.quest_rewards()[0].chance, 50);

    // One press for the step, four for the value: 50% → 10%.
    ed.input(Key::Y);
    assert_eq!(ed.step, 1, "[Y] is the step key");
    assert_eq!(STEPS[ed.step], 10);
    for _ in 0..4 {
        ed.input(Key::Left);
    }
    assert_eq!(
        ed.quest_rewards()[0].chance,
        10,
        "four presses instead of forty: {}",
        ed.message
    );

    // The .sem numbers the ▸詳細 rows used to hold are values of the row now:
    // the monster tab is five slots, and [ZR] walks the eleven values - the
    // five .mib ones, the area and the five .sem ones. The rows themselves are
    // gone, so opening a detail can no longer push slots 4 and 5 off the list
    // (the second review's finding 19), and the file screens have one door
    // (the [ファイル] pill / [R] held, finding 13).
    ed.quest_edit_tab = QUEST_TAB_MONSTERS;
    ed.quest_edit_cursor = 0;
    assert_eq!(
        ed.quest_row_count(),
        mhnx_core::hbedit::QUEST_MON_SLOTS,
        "the monster tab is the five slots and nothing else"
    );
    ed.quest_edit_field = 0;
    for _ in 0..mhnx_core::hbedit::QUEST_MON_FIELDS {
        ed.input(Key::Z);
    }
    assert_eq!(ed.quest_edit_field, 0, "[ZR] wraps after all eleven values");
    for _ in 0..mhnx_core::hbedit::QUEST_MON_SEM_X {
        ed.input(Key::Z);
    }
    assert_eq!(
        ed.quest_field_label(),
        "X座標",
        "the .sem placement is one of the row's values: {}",
        ed.message
    );
}

// ---------------------------------------------------------------------------
// 13: クエスト改造's [B] goes back to the list the quest came from
// ---------------------------------------------------------------------------

/// [B] went to the .arc list every time, so a quest opened from the save's own
/// list came back to a list that did not hold it.
#[test]
fn the_quest_screen_goes_back_to_the_list_it_came_from() {
    // From the save's own list.
    let mut ed = open_quest(1_010_165, 50);
    ed.enter_quest_edit_from(Screen::SaveQuests);
    assert_eq!(ed.quest_return, Screen::SaveQuests);
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::SaveQuests);

    // From the .arc list on the card.
    let mut ed = open_quest(1_010_165, 50);
    ed.enter_quest_edit_from(Screen::ArcList);
    assert_eq!(ed.quest_return, Screen::ArcList);
    ed.input(Key::B);
    assert_eq!(ed.screen, Screen::ArcList);

    // The item picker behind a reward row comes back to the same quest screen
    // without forgetting where the quest came from.
    let mut ed = open_quest(1_010_165, 50);
    ed.enter_quest_edit_from(Screen::SaveQuests);
    ed.enter_quest_edit();
    assert_eq!(ed.quest_return, Screen::SaveQuests);

    // And the console passes the list it opened the quest from.
    assert!(
        ui_src().contains("enter_quest_edit_from(Screen::SaveQuests)"),
        "the save's own list has to be the return target"
    );
}
