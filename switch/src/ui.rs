//! Redesigned Switch UI/UX Engine for XXSE (`XXSE-1.nro` & `XXSE-2.nro`).
//!
//! Architecture & Ergonomics:
//!   - `ZL / ZR`: Cycle the 7 top-level Sections. Six of them are the work
//!     (`セーブ`, `キャラ`, `アイテム`, `護石・装備`, `クエスト`, `オトモ・記録`);
//!     the seventh is `詳細・ツール` and holds the screens that read the file
//!     rather than the play (`セーブマップ` / `CSV入出力` / `環境設定`).
//!     Nothing was removed: every screen that used to sit beside the everyday
//!     ones still exists, one press further to the right.
//!   - `L / R`: Cycle the Sub-Tabs of the current Section (NEVER switches
//!     character slot unexpectedly!), answered on the press rather than on the
//!     release so two quick presses do not feel one behind. Holding L offers
//!     the whole-save undo (two holds run it); holding R opens 詳細. On the four
//!     pick lists the shoulders are the ten row step and repeat while held.
//!   - The strip under the sub-tabs is the one-line guide for the screen
//!     (`Editor::screen_guide`), so a first-time player is told what `[A]` does.
//!   - `R 長押し` (or the `[詳細]` pill on the sub-tab bar): the `詳細` view of
//!     the screen that is showing - the save map row that screen edits, with
//!     its hex dump (`Screen::SaveMap`). `[B]` comes back; inside the screen,
//!     `[A]` opens the byte editor on the selected row (one byte at a time,
//!     asked first, one undo step per confirmed change).
//!   - `A`: Always Open / Edit / Choose from list (unified across every screen, never deletes!).
//!   - `◀ / ▶`: Always adjust numeric values (`±step`).
//!   - `右スティック`: `▲ / ▼` turn a page (as many rows as the screen shows),
//!     `◀ / ▶` jump to the first / last row. It is the one input the rest of the
//!     map left free: ZL / ZR are the sections, L / R the sub-tabs, and the
//!     shoulders are already scrolling on the pick lists.
//!   - Dragging a finger over the rows scrolls them (one row per row of travel,
//!     with the flick coasting on after the finger lifts).
//!   - The right hand scrollbar of every list can be dragged as well: the thumb
//!     moves the window and the cursor keeps its place inside it, and a tap on
//!     the bar - above or below the thumb - turns one page. The bar is drawn
//!     6px wide (14px while a finger holds it) and answers a finger within 40px
//!     (`hbedit::SCROLL_HIT_W`), and the row's tap area and its value both stop
//!     8px short of that zone (`hbedit::SCROLL_TEXT_GAP`), so no text and no tap
//!     can land on the other.
//!   - 装備フォームの `[A]` は行ごとの操作（装備名/装飾品を選ぶ・値・書き込み）で、
//!     行そのものに「▸ 選ぶ / ▸ 書き込み」と出る（`hbedit::EquipFormAction`）。
//!   - `X`: Always Save / Apply changes (`セーブ保存` / `.arc保存` / `護石反映`).
//!   - `Y`: The list key. On every screen whose list can be narrowed it is
//!     that screen's 検索 (equipment box, talisman list, talisman form, the
//!     quest list and the three pickers) or `空きも表示` on the item box; on a
//!     screen with no list it is that screen's own second action and the footer
//!     names it (刻み on the byte screens, 列切替 on 狩猟記録, 項目切替 on オトモ,
//!     タブ on マイセット and on 進行状況). No screen deletes on `Y`: the
//!     two-press delete is `[-]` everywhere.
//!   - `R`: the number key where the footer promises `[R] 数値入力`, and the
//!     second bit of 進行状況's quest view (既読). The footer names it on both.
//!
//! The words are one vocabulary, not one screen's habit: 検索 opens a name
//! search everywhere, 刻み is the step size, 削除 removes a record and 解除
//! empties a slot. 絞る / 絞り込み / 外す / 刻み幅 are the old synonyms and are
//! gone from the drawn labels (the key table lives in editor::actions).
//!   - 護石の `[A]` は行ごとの操作（種類/スロット/スキル選択/数値/書き込み）で、
//!     フッターと `[A]` ボタンの文言はその行の動作と同じ値から作られる
//!     （`mhnx_core::hbedit::CharmAction`）。
//!   - `B`: Back / Cancel.
//!
//! Two Distinct Visual Architectures (controlled by `XXSE_UI_VARIANT`):
//!   - Variant 1 (`XXSE-1.nro`): **Horizon Split-Studio**
//!     2-tier flat top navigation + borderless clean left list (`780px`, zero inline button spam)
//!     + permanent right-hand Context Inspector & Cheat Deck (`436px`).
//!   - Variant 2 (`XXSE-2.nro`): **Guild Wide Bento Grid**
//!     Floating top guild ribbon + sub-tab/cheat bar + full-width `1240px` 2-Column Bento Card Grid
//!     where unselected cards are ultra-clean and only the focused card reveals inline controls.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use mhnx_core::arc::extension_for;
use mhnx_core::charm;
use mhnx_core::check::Severity;
use mhnx_core::dlcats::{self, Lang};
use mhnx_core::entries::{Esl, ItemTable, Qdp, QuestLink};
use mhnx_core::hbedit::{
    arena_clock, arena_name, arena_part_name, arena_rank_name, charm_form_rows, deck_stack,
    decoration_name_of, footer_guide_right, footer_quit_x, item_name_of, monster_name,
    palico_part_name, palico_skill_tab_name, Editor, ItemSetOp, Key, MibPickTarget,
    PalicoEquipRow,
    PalicoSkillRow, Screen, Shoulder, ARENA_COLUMNS, ARENA_PARTS, ARENA_ROWS, CHAR_FIELDS,
    CHARM_EDIT_ROWS, CONSOLE_SAVE, CSV_DIR, GUILD_TAB_ARENA, GUILD_TAB_CARD, GUILD_TAB_COUNTS,
    GUILD_TAB_WEAPONS, GUILD_VENUES, GUILD_WEAPON_ROWS, HUNT_STEPS, ITEM_OP_ADD, ITEM_OP_CLAMP,
    ITEM_OP_COMPACT, ITEM_OP_DEDUPE, ITEM_OP_UNDO, ITEM_OPS, ITEM_SET_EDIT_ROWS,
    PALICO_EQUIP_NAME_NOTE, PALICO_EQUIP_ROWS, PROGRESS_AWARDS, PROGRESS_QUESTS,
    PROGRESS_REQUESTS, SET_EDIT_ROWS, SET_ROW_NAME, STEPS,
    // The scroll model: see mhnx_core::hbedit for the arithmetic and its tests.
    list_end, page_move, scroll_track, ScrollBar,
    // クエスト改造: the aggregate screen, its tabs and its row model.
    quest_row_badge, QuestPick, QUEST_MON_AREA, QUEST_TAB_BASIC, QUEST_TAB_LABELS,
    QUEST_TAB_MONSTERS, QUEST_TAB_REWARDS, QUEST_TAB_SUPPLIES, QUEST_TABS,
};
use mhnx_core::questfields;
use mhnx_core::save::{Options, SaveKind};

use crate::text::{rgb, rgba, Canvas, Color, FontSize};

/// Returns `true` when building `XXSE-2.nro` (Wide 2-Column Bento Grid UI).
/// Defaults to `false` (`XXSE-1.nro`: Horizon Split-Studio UI).
#[inline]
pub const fn is_bento_variant() -> bool {
    match option_env!("XXSE_UI_VARIANT") {
        Some(s) => s.as_bytes().len() == 1 && s.as_bytes()[0] == b'2',
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Theme Palette (Variant 1: Horizon Split-Studio vs Variant 2: Guild Bento)
// ---------------------------------------------------------------------------

struct Theme {
    bg_canvas: Color,
    bg_header: Color,
    bg_subbar: Color,
    bg_panel: Color,
    bg_row_even: Color,
    bg_row_odd: Color,
    bg_selected: Color,
    bg_val_box: Color,
    bg_btn: Color,
    bg_btn_primary: Color,
    bg_btn_cheat: Color,
    bg_danger: Color,
    bg_footer: Color,
    border_subtle: Color,
    border_focus: Color,
    accent_primary: Color,
    accent_secondary: Color,
    accent_ok: Color,
    accent_warn: Color,
    accent_danger: Color,
    text_primary: Color,
    text_secondary: Color,
    text_muted: Color,
    corner_r: i32,
}

const THEME_V1_HORIZON: Theme = Theme {
    bg_canvas: rgb(14, 17, 23),
    bg_header: rgb(20, 25, 34),
    bg_subbar: rgb(17, 21, 29),
    bg_panel: rgb(21, 26, 35),
    bg_row_even: rgb(22, 27, 36),
    bg_row_odd: rgb(18, 23, 31),
    bg_selected: rgb(24, 54, 78),
    bg_val_box: rgb(12, 15, 20),
    bg_btn: rgb(36, 44, 58),
    bg_btn_primary: rgb(22, 96, 136),
    bg_btn_cheat: rgb(68, 52, 24),
    bg_danger: rgb(156, 46, 58),
    bg_footer: rgb(18, 22, 30),
    border_subtle: rgb(44, 54, 70),
    border_focus: rgb(0, 212, 245),
    accent_primary: rgb(0, 212, 245),
    accent_secondary: rgb(246, 194, 78),
    accent_ok: rgb(62, 214, 146),
    accent_warn: rgb(242, 154, 68),
    accent_danger: rgb(238, 108, 116),
    text_primary: rgb(240, 244, 250),
    text_secondary: rgb(156, 170, 188),
    // The caption ink. At (106,118,136) it measured 3.8-4.0:1 against the guide
    // and footer bands, under the 4.5:1 AA floor, and it is the colour the one
    // line a first-time player reads is drawn in. (122,134,154) is 4.97:1 on
    // this theme's subbar and 5.05:1 on the bento one, so both variants clear
    // it without the strip growing (a 22px line needs a 26px box, and taking
    // those 6px off CONTENT_H drops every list from nine rows to eight).
    text_muted: rgb(122, 134, 154),
    corner_r: 7,
};

const THEME_V2_BENTO: Theme = Theme {
    bg_canvas: rgb(17, 15, 14),
    bg_header: rgb(28, 24, 22),
    bg_subbar: rgb(24, 20, 18),
    bg_panel: rgb(26, 22, 20),
    bg_row_even: rgb(31, 27, 24),
    bg_row_odd: rgb(26, 22, 20),
    bg_selected: rgb(56, 42, 28),
    bg_val_box: rgb(16, 14, 13),
    bg_btn: rgb(48, 41, 36),
    bg_btn_primary: rgb(138, 56, 38),
    bg_btn_cheat: rgb(78, 58, 24),
    bg_danger: rgb(164, 48, 48),
    bg_footer: rgb(25, 21, 19),
    border_subtle: rgb(62, 53, 46),
    border_focus: rgb(232, 188, 92),
    accent_primary: rgb(232, 188, 92),
    accent_secondary: rgb(224, 92, 64),
    accent_ok: rgb(104, 202, 136),
    accent_warn: rgb(236, 158, 76),
    accent_danger: rgb(236, 112, 104),
    text_primary: rgb(245, 239, 230),
    text_secondary: rgb(182, 170, 154),
    // Same reason as the horizon theme: 5.0:1 rather than 4.0:1.
    text_muted: rgb(146, 132, 120),
    corner_r: 10,
};

#[inline]
const fn theme() -> &'static Theme {
    if is_bento_variant() {
        &THEME_V2_BENTO
    } else {
        &THEME_V1_HORIZON
    }
}

#[inline]
const fn white_or_dark(c: Color) -> Color {
    let r = (c & 0xFF) as u32;
    let g = ((c >> 8) & 0xFF) as u32;
    let b = ((c >> 16) & 0xFF) as u32;
    let luma = (r * 299 + g * 587 + b * 114) / 1000;
    if luma >= 125 {
        rgb(14, 16, 20)
    } else {
        rgb(255, 255, 255)
    }
}

// Layout geometry (1280 x 720) — Full-width 2-tier header + guide + body + footer
const TOP_SEC_H: i32 = 48;
const SUB_TAB_Y: i32 = TOP_SEC_H;
const SUB_TAB_H: i32 = 44;
/// The one-line guide strip under the sub-tab bar (Editor::screen_guide).
///
/// 28px, holding the 22px row font: the strip was 22px of 17px text, so the one
/// sentence a first-time player reads was the smallest type on the screen -
/// smaller than the row values it explains. The review asked for the row size
/// and this is it.
///
/// The cost was the six pixels CONTENT_Y moved down, which took a row from every
/// list (nine rows to eight). The row is back: the list panel's own top padding
/// is 4px now instead of 10, so nine rows fit again when no warning strip is
/// drawn and eight when one is (hbedit::list_window_rows).
const GUIDE_Y: i32 = SUB_TAB_Y + SUB_TAB_H; // 92
const GUIDE_H: i32 = 28;
const CONTENT_Y: i32 = GUIDE_Y + GUIDE_H; // 120
const FOOTER_Y: i32 = 668;
const FOOTER_H: i32 = 52;
const CONTENT_H: i32 = FOOTER_Y - CONTENT_Y - 8; // 546

/// The [生データ] / [ファイル] pill on the sub-tab bar: the one button every
/// screen carries. It is labelled by `Editor::detail_pill_label`, so it says
/// 生データ where it opens the hex view and ファイル on クエスト改造.
///
/// It sits between the [R] button (which ends at 850 at the widest) and the
/// screen's own tool pills (which start at 964), so it is in the same band as
/// the keys and never moves from screen to screen.
const DETAIL_X: i32 = 856;
const DETAIL_W: i32 = 100;
const MAIN_X: i32 = 20;
const MAIN_W: i32 = 1240;

// Split-Studio geometry for Variant 1 (`XXSE-1.nro`)
const V1_LIST_W: i32 = 796;
const V1_INSP_X: i32 = MAIN_X + V1_LIST_W + 16; // 832
const V1_INSP_W: i32 = 1280 - V1_INSP_X - 20; // 428

// Scrolling. The arithmetic lives in `mhnx_core::hbedit` (ScrollBar, page_move,
// scroll_track, KeyRepeat) because it is the one part of this that a host test
// can pin; these are only the pixel numbers this screen draws with.
/// Rows the MIB picker modal shows at once, and the step its paging keys take.
const MIB_PICKER_VISIBLE: usize = 10;
/// Distance between two rows of the MIB picker, in pixels (46 + 4).
const MIB_PICKER_PITCH: i32 = 50;
/// One row of a list, in pixels.
const LIST_ROW_H: i32 = 56;
/// Pixels a finger has to travel to scroll one row when it drags the list.
/// Equal to the row height on purpose: the rows move with the finger.
const DRAG_ROW_PX: i32 = LIST_ROW_H;
/// Flick velocity is counted in this many units per row per frame.
const FLING_UNIT: i32 = 64;
/// How far a finger may wander and still count as a tap, in pixels.
const TAP_SLOP_PX: i32 = 18;

// ---------------------------------------------------------------------------
// 6 Top-Level Sections (ZL / ZR) & In-Section Sub-Tabs (L / R)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MainSection {
    /// Opening a save and converting it: the first thing a session does.
    Save = 0,
    Hunter = 1,
    Items = 2,
    Gear = 3,
    Quest = 4,
    PalicoRecord = 5,
    /// 詳細・ツール: the screens that read the file rather than the play.
    ///
    /// It is a section, not a hidden mode, because the two-tier navigation has
    /// to be able to say where the technical screens went: one press of ZR past
    /// オトモ・記録 and they are all there. The everyday work never needs it.
    Tools = 6,
}

/// The top row, in the order the pad walks it.
///
/// The everyday sections come first and the technical one last, which is the
/// whole shape of the design: a player who never opens 詳細・ツール can still
/// do every edit, and a player who wants the bytes knows where they are.
const MAIN_SECTIONS: [(MainSection, &str); 7] = [
    (MainSection::Save, "セーブ"),
    (MainSection::Hunter, "キャラ"),
    (MainSection::Items, "アイテム"),
    (MainSection::Gear, "護石・装備"),
    (MainSection::Quest, "クエスト"),
    (MainSection::PalicoRecord, "オトモ・記録"),
    (MainSection::Tools, "詳細・ツール"),
];

pub fn section_of(screen: Screen) -> MainSection {
    match screen {
        // Opening a save and converting it are the two file-level jobs a
        // session starts with; 環境設定 and CSV are tools now.
        Screen::SaveList | Screen::Convert => MainSection::Save,
        Screen::Slots | Screen::Char => MainSection::Hunter,
        Screen::Items | Screen::ItemPick => MainSection::Items,
        Screen::Charms
        | Screen::CharmEdit
        | Screen::SkillPick
        | Screen::Equip
        | Screen::EquipEdit
        | Screen::JewelPick
        | Screen::EquipPick
        | Screen::Sets => MainSection::Gear,
        // クエスト改造 is the section's first tab and the screen an opened
        // quest lands on; the file screens behind 詳細 stay in the same
        // section, one press away.
        Screen::QuestEdit
        | Screen::SaveQuests
        | Screen::QuestEquip
        | Screen::ArcList
        | Screen::ArcEntries
        | Screen::Mib
        | Screen::Gmd
        | Screen::EntryView
        | Screen::Check => MainSection::Quest,
        // The progress screen is the fifth tab of the records section: awards,
        // requests and quest bits are what the character has done, the same
        // kind of thing the hunt record and the card are.
        Screen::Roster | Screen::Palico | Screen::Hunts | Screen::Guild | Screen::Progress => {
            MainSection::PalicoRecord
        }
        // 詳細・ツール: the three screens that read the file rather than the
        // play. 生データ is the one every record screen's pill lands on.
        // 詳細・ツール: the screens that read the file rather than the play.
        // 変更一覧 belongs here for the same reason 生データ does - it names
        // bytes, not a record - and it is the one screen on this shelf that
        // edits (one value at a time) instead of only showing.
        Screen::SaveMap | Screen::Review | Screen::Csv | Screen::Options => MainSection::Tools,
    }
}

fn is_quest_inner(screen: Screen) -> bool {
    matches!(
        screen,
        Screen::ArcEntries | Screen::Mib | Screen::Gmd | Screen::EntryView | Screen::Check
    )
}

/// Is there a 詳細 view behind this screen?
///
/// The pill is drawn on every screen so the bar never changes shape, but it is
/// only lit where pressing it does something. That answer is the state
/// machine's (hbedit::Editor::detail_pill_live) because the reviewed bug was
/// the console's own copy: it kept 生データ lit on the 生データ screen, where
/// the pill could only ever answer "already showing".
fn detail_available(ed: &Editor) -> bool {
    ed.detail_pill_live()
}

// ---------------------------------------------------------------------------
// Interactive HitBoxes & Actions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub enum UiAction {
    Key(Key),
    /// A tap that lands outside a modal panel. It covers the whole frame while
    /// the keyboard or the MIB picker is up, so a tap that misses the panel
    /// stops here instead of reaching the list - or the footer's undo pill -
    /// underneath it.
    ModalBlank,
    SelectSlot(usize),
    /// One slot card's "このキャラを編集" button: the slot is the card's own.
    ///
    /// It used to send a bare `Key::A`, and the pad's [A] opens `ed.slot` - so
    /// the button on card 2 opened card 1's character. The slot travels with the
    /// action now, and an empty slot pushes no button at all.
    OpenSlot(usize),
    /// ポーチへ on the item set form: copy the set into the pouch.
    ///
    /// It was [-]'s job, and [-] is the delete key now. The form's [ZR] is
    /// already the set-name keyboard, so the copy is a pill.
    ItemSetToPouch,
    /// The slot badge in the header: one tap switches the hunter who is being
    /// edited, so it asks first. A card on the スロット screen selects a slot
    /// and changes nothing, so that one is still [UiAction::SelectSlot].
    SlotBadge(usize),
    GotoSection(MainSection),
    SelectSubTab(usize),
    CycleSection(bool),
    CycleSubTab(bool),
    SetStep(usize),
    SelectRow(usize),
    ActivateRow(usize),
    /// One cell of the arena grid: (arena row, rank column).
    ArenaCell(usize, usize),
    /// One cell of the guild card's weapon-usage grid: (weapon row, venue).
    ///
    /// A tap moves the cursor and nothing else, the same rule ArenaCell
    /// follows: the write still starts with [A], so a tap cannot change a
    /// count.
    GuildWeaponCell(usize, usize),
    AdjustRow(usize, Key),
    SetRowMax(usize),
    SelectSubField(usize, usize),
    Rescan,
    QuickSave,
    /// The whole-save undo (L held down, or the footer pill).
    Undo,
    /// The save-review dialog's body, one press of ▲ / ▼ (or a page).
    ReviewScroll(i32),
    /// The footer's 終了 pill: the touch route for [＋].
    Quit,
    /// The save review: A on the dialog.
    CommitReview,
    /// The save review: B on the dialog.
    CancelReview,
    // The item box bulk operation menu
    OpenItemMenu,
    ItemOp(usize),
    // One-tap Bulk Cheats & Presets
    CharMaxAll,
    // The talisman screen has no bulk action and no preset: its rows are
    // edited one field at a time, with [A] acting on the selected row.
    NewCharm,
    DeleteCharmConfirm,
    RosterMaxAll,
    InstallDlcCategory(u8),
    MibPickerSelect(usize),
    MibPickerClose,
    MibPreset(u8),
    SemPreset(u8),
    // クエスト改造's file-by-file screens have one door: the [ファイル] pill
    // (UiAction::OpenDetail, which open_detail routes to open_quest_detail).
    // The screen used to have three - the pill, a chip on the bar and a row the
    // list ended with - and they were named two different things.
    /// The `[生データ]` / `[ファイル]` pill / `R 長押し`: the view behind this screen.
    OpenDetail,
    /// The talisman list's name search. It has to be an action of its own: that
    /// screen's keys are all spoken for ([-] deletes, [＋] creates, L/R are the
    /// sub-tabs), so unlike the equipment box it has no free key to give the
    /// filter, and the button says [検索] rather than naming one.
    FilterCharms,
    /// The item box's name search, for the same reason: [Y] is the 空きも表示
    /// toggle and every other key on that screen is taken, so the search is a
    /// pill in the inspector. One word for it, the same one the talisman and
    /// equipment screens use: 検索.
    FilterItems,
    /// 進行状況's 既読 write: the second bit of the quest view.
    ///
    /// [Y] used to run it there and move the tab on the other two views. The
    /// key is the tab key on all three now, so the write is this pill.
    ProgressSeen,
    /// 報酬 / 支給品's item-name search.
    FilterQuestItems,
    /// 進行状況's name search.
    ///
    /// The quest view lists 1,508 rows and the request view 380, and the screen
    /// has no key left to give it: [A] toggles the bit, [Y] walks the views, [R]
    /// is the quest view's second bit, the four directions move and [L] is the
    /// tab. So it is [ZR] and a pill, the same answer the item box's search got.
    FilterProgress,
    InstallAllPalicoes,
    TogglePalicoLang,
    ExtractOfficialArcs,
    /// Write the quest under the cursor of the save's quest list out to the
    /// card. [-] on that screen is the two-press delete now, so the single
    /// extract it used to be moved to a button of its own.
    ExtractSelectedQuest,
    /// Write every quest of the save out to the card.
    ///
    /// This was [R], which the console spends on the sub-tab: the footer
    /// promised "SDへ全展開" and the press changed tab. The button calls the
    /// same method the key does in the state machine.
    ExtractAllQuests,
    /// Switch the hunt list between 記録ありのみ and every monster.
    ToggleHuntShowAll,
    /// 狩猟記録 from the guild card's deck.
    ///
    /// The card's bar is its own four views while that screen is showing, so
    /// the section's 狩猟記録 tab is not on the glass: this is the touch route
    /// to the screen the tab would have opened (hbedit::Editor::open_hunts).
    OpenHunts,
    /// Drop the equipment box's name filter and show every record again.
    ClearEquipFilter,
    /// The item box's 所持品のみ <-> 空きも表示 toggle.
    ///
    /// [Y] used to be the step on this screen, which is why the state machine's
    /// own toggle could not be reached from either the pad or a finger. The key
    /// and this button now run the same method (hbedit::Editor::
    /// toggle_item_show_empty), so the list, the count and the heading cannot
    /// disagree about which state they are in.
    ToggleItemEmpty,
    InstallArcRow(usize),
    // Built-in OSK
    OskTab(usize),
    OskCell(usize, usize),
    OskBackspace,
    OskClear,
    OskModify,
    OskSpace,
    /// Insert the line break a multiline field holds (U+000A).
    OskNewline,
    OskConfirm,
    OskCancel,
    // The update banner and its dialog: four answers, none of which is a key
    // on the screen under it.
    /// A tap on the banner (or [A] on the start screen): open the dialog.
    UpdateOpen,
    /// [A] in the dialog: 確認 / ダウンロード / 中止 / もう一度.
    UpdatePrimary,
    /// [B] in the dialog: put the banner away until the next start.
    UpdateLater,
    /// [Y] in the dialog: the start-up check on or off.
    UpdateToggle,
}

#[derive(Clone)]
pub struct MibPickerState {
    pub field_idx: usize,
    pub title: String,
    pub items: Vec<(u32, String)>,
    pub cursor: usize,
}

#[derive(Clone)]
pub struct OskState {
    pub req_idx: usize,
    pub header: String,
    pub buffer: String,
    /// What the field accepts: the bytes it counts and whether it holds a
    /// line break. A name does; every other text request does not.
    pub spec: mhnx_core::hbedit::NameSpec,
    pub tab: usize,
    pub row: usize,
    pub col: usize,
    /// The last character the field refused, so the modal can name it.
    pub refused: Option<char>,
    /// Why this keyboard was used when the system one was the first choice.
    ///
    /// Empty on the normal path. It is set when swkbd cannot be launched (the
    /// process is a library applet) and a one-line field falls back to this
    /// built-in keyboard: the modal then says so instead of leaving the user to
    /// wonder why the system keyboard never appeared.
    pub note: String,
    /// The two-press rule for [X] 全消去.
    ///
    /// [X] used to empty the field on one press, next to [B] which removes one
    /// character: the two keys sit side by side on the pad and a name typed
    /// over a minute was gone with no way back. The first press only offers it
    /// (mhnx_core::confirm::ConfirmPress); any other key takes the offer back.
    pub clear: mhnx_core::confirm::ConfirmPress,
}

#[derive(Clone, Copy)]
struct HitBox {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    action: UiAction,
    is_row_body: bool,
}

impl HitBox {
    #[inline]
    fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// What the last frame put on screen for one list's scrollbar.
///
/// The row ids are kept here on purpose. The list itself is rebuilt from the
/// save on every frame, so by the time a finger has dragged the thumb there is
/// no way back from a pixel to a row id except this: the same ids, in the same
/// order, that the rows were drawn from. They are only kept while the list can
/// actually scroll, which is the only time the bar is drawn or dragged.
#[derive(Clone)]
struct BarState {
    /// The screen the bar belongs to: a bar left over from another screen is
    /// never allowed to move this one's cursor.
    screen: Screen,
    track_y: i32,
    track_h: i32,
    /// The area that answers a finger, wider than the bar.
    hit_x: i32,
    hit_w: i32,
    scroll: ScrollBar,
    /// Every row id of the list, in the order the list shows them.
    ids: Vec<usize>,
    /// The MIB picker's bar rather than a list's: a drag writes its cursor.
    picker: bool,
}

/// The thumb drag in progress.
#[derive(Clone, Copy)]
struct BarDrag {
    /// Where inside the thumb the finger landed, in pixels.
    grab: i32,
    /// The row the cursor held inside the window when the drag started, so
    /// dragging the bar moves the view and not the selection to the top.
    inside: usize,
    /// Where the finger went down, so a tap that never moves is not a drag.
    start_y: i32,
    /// The finger has moved far enough to be dragging.
    moved: bool,
    /// The finger went down on the thumb itself rather than beside it.
    on_thumb: bool,
    /// The finger went down above the thumb (the rest of the bar is below it).
    above: bool,
    picker: bool,
}

/// Do L / R mean "move by ten" on this screen instead of "change the sub-tab"?
///
/// The four pick lists spend the shoulders on their ten row step: the console
/// turns L / R into [◀] / [▶] there (see `GuiState::handle_pad_key`). They are
/// therefore the screens where a held shoulder has to auto-repeat, and the only
/// ones: everywhere else the shoulder is the sub-tab key, and repeating that
/// would walk the tab strip while the finger rests.
///
/// The answer itself is the state machine's (hbedit::Editor::shoulder_job), so
/// the key the pad answers and the key the repeat answers cannot disagree.
pub fn shoulder_moves(ed: &Editor) -> bool {
    matches!(ed.shoulder_job(Key::L), Shoulder::Move)
}

pub struct GuiState {
    hits: Vec<HitBox>,
    was_touching: bool,
    drag_start_y: i32,
    last_drag_y: i32,
    dragged: bool,
    pending_activate_row: Option<usize>,
    pub wants_rescan: bool,
    pub wants_extract_official_arcs: bool,
    pub wants_install_arc_idx: Option<usize>,
    /// The snapshot button was pressed; the I/O layer owns the folder.
    pub wants_restore_snapshot: bool,
    /// Which tab of the クエスト section bar is lit.
    ///
    /// The bar used to derive it from the screen (SaveQuests -> 1 + quest_tab),
    /// so pressing クエスト改造 with no .arc open - which lands on the save's own
    /// list - lit セーブ内 instead of the tab that was pressed. The tab the user
    /// chose is the tab that stays lit.
    ///
    /// The *list* tab (セーブ内 / 公式 / 没クエ) and its three cursors live in the
    /// state machine now (hbedit::Editor::quest_tab / quest_cat_cursor): the
    /// guide under the bar and the extract button both depend on them, and both
    /// answered for the save's list whatever tab was showing while this layer
    /// kept them.
    pub quest_bar_tab: usize,

    pub mib_picker: Option<MibPickerState>,
    /// The monster slot whose エリア番号 chooser is open, if it is the one
    /// the shared picker modal is showing. The picker is the same modal the
    /// .mib screen uses; this says which writer its [A] lands in.
    pub quest_area_slot: Option<usize>,
    /// The shared picker modal is showing the equipment form's 種別 list.
    ///
    /// The form's [A] on 種別 asks for a list the state machine cannot open
    /// (hbedit::Editor::equip_kind_pick_pending); this says which writer the
    /// modal's pick lands in.
    pub equip_kind_picker: bool,
    pub mib_mob_sub: usize,
    pub osk: Option<OskState>,
    pub osk_result: Option<(usize, String)>,
    pub osk_cancelled: bool,
    pub sets_cursor: usize,
    pub sets_tab: usize,
    /// The snapshot a review is waiting on. Set by the [Y] press on the options
    /// screen through the same dialog a save write uses, and taken by the
    /// second press: restoring is a write like any other and must not happen
    /// on the press that asked for it.
    pub pending_snapshot: Option<crate::editor::SnapshotRestore>,
    /// The whole-save undo asked for once, waiting for the second answer.
    undo_press: mhnx_core::confirm::ConfirmPress,
    /// The header slot badge's tap: the first one only names the slot it would
    /// switch to. Changing the slot changes whose save every screen is editing,
    /// which is not something one stray tap may do.
    slot_press: mhnx_core::confirm::ConfirmPress,
    /// The roster's bulk MAX asked for once, waiting for the second answer.
    roster_max: mhnx_core::confirm::ConfirmPress,
    /// First line of the save-review dialog that is on screen.
    ///
    /// The dialog's body is longer than the panel: the values alone can be
    /// thirty lines. It used to stop at the panel's edge and say "ほか N 行"
    /// with no way to see them, so the one thing the user was asked to read
    /// before a write was the one thing they could not. Reset whenever no
    /// review is open, so a new dialog always starts at its first line.
    review_scroll: usize,
    /// The scrollbar the last frame drew, with the row ids behind it.
    bar: Option<BarState>,
    /// The thumb being dragged, if a finger is on one right now.
    bar_drag: Option<BarDrag>,
    /// Rows a flick still owes the list, in FLING_UNIT units per row.
    fling: i32,
    /// Rows the finger moved on the last drag frame: the speed a flick starts at.
    drag_rows: i32,
    /// The optional update check, its banner and its dialog.
    ///
    /// It lives here because the banner is part of the guide strip and the
    /// dialog is a modal layer of the same frame; the network and the card
    /// behind it are in crate::update.
    pub update: crate::update::UpdateState,
}

impl GuiState {
    pub fn new() -> Self {
        Self {
            hits: Vec::with_capacity(96),
            was_touching: false,
            drag_start_y: 0,
            dragged: false,
            last_drag_y: 0,
            pending_activate_row: None,
            wants_rescan: false,
            wants_extract_official_arcs: false,
            wants_install_arc_idx: None,
            wants_restore_snapshot: false,
            quest_bar_tab: 0,
            mib_picker: None,
            quest_area_slot: None,
            equip_kind_picker: false,
            mib_mob_sub: 0,
            osk: None,
            osk_result: None,
            osk_cancelled: false,
            sets_cursor: 0,
            sets_tab: 0,
            pending_snapshot: None,
            review_scroll: 0,
            undo_press: mhnx_core::confirm::ConfirmPress::new(),
            slot_press: mhnx_core::confirm::ConfirmPress::new(),
            roster_max: mhnx_core::confirm::ConfirmPress::new(),
            bar: None,
            bar_drag: None,
            fling: 0,
            drag_rows: 0,
            update: crate::update::UpdateState::new(),
        }
    }

    pub fn open_osk(
        &mut self,
        req_idx: usize,
        header: &str,
        initial: &str,
        spec: mhnx_core::hbedit::NameSpec,
    ) {
        self.open_osk_with_note(req_idx, header, initial, spec, "");
    }

    /// The same keyboard, with the reason it was used instead of swkbd.
    ///
    /// hbedit::KeyboardRoute::BuiltinFallback is the one route that passes a
    /// note: a one-line field would have used the system keyboard, but the
    /// applet cannot be launched (started from the album), so the built-in one
    /// opens and says why.
    pub fn open_osk_with_note(
        &mut self,
        req_idx: usize,
        header: &str,
        initial: &str,
        spec: mhnx_core::hbedit::NameSpec,
        note: &str,
    ) {
        // Whatever an old keyboard or an old save left in the text goes
        // through the field filter before it can be edited: a tab has no glyph
        // in a name and the writers refuse it, so it never reaches the box.
        let (initial, refused) = spec.filter(initial);
        self.osk = Some(OskState {
            req_idx,
            header: String::from(header),
            buffer: initial,
            spec,
            tab: 0,
            row: 0,
            col: 0,
            refused,
            note: String::from(note),
            clear: mhnx_core::confirm::ConfirmPress::new(),
        });
    }

    pub fn handle_osk_key(&mut self, key: Key) -> bool {
        let Some(mut osk) = self.osk.take() else {
            return false;
        };
        // The 全消去 offer belongs to the press that made it, the same rule the
        // two-press deletes follow: anything but [X] takes it back.
        if key != Key::X {
            osk.clear.clear();
        }
        match key {
            Key::Up => {
                osk.row = (osk.row + 4) % 5;
                self.osk = Some(osk);
            }
            Key::Down => {
                osk.row = (osk.row + 1) % 5;
                self.osk = Some(osk);
            }
            Key::Left => {
                osk.col = (osk.col + 9) % 10;
                self.osk = Some(osk);
            }
            Key::Right => {
                osk.col = (osk.col + 1) % 10;
                self.osk = Some(osk);
            }
            Key::L => {
                osk.tab = (osk.tab + 3) % 4;
                self.osk = Some(osk);
            }
            Key::R => {
                osk.tab = (osk.tab + 1) % 4;
                self.osk = Some(osk);
            }
            Key::A => {
                let r = osk.row;
                let c = osk.col;
                osk_press_cell(&mut osk, r, c);
                self.osk = Some(osk);
            }
            Key::B => {
                let _ = osk.buffer.pop();
                self.osk = Some(osk);
            }
            // [X] is 全消去, and it asks: the first press names what it would
            // take, the second one takes it. It used to empty the field on one
            // press, one key away from [B] (which removes a character).
            Key::X => {
                if osk.buffer.is_empty() {
                    osk.clear.clear();
                    self.osk = Some(osk);
                    return true;
                }
                if osk.clear.press() {
                    osk.buffer.clear();
                }
                self.osk = Some(osk);
            }
            Key::Y => {
                osk_cycle_last_char(&mut osk.buffer);
                self.osk = Some(osk);
            }
            // ZR is the line-break key. It is the one button this modal had
            // left: ZL still walks the tabs, and [+] confirms. A field that
            // does not hold a line break says so instead of ignoring it.
            Key::Z => {
                osk_newline(&mut osk);
                self.osk = Some(osk);
            }
            Key::Plus => {
                self.osk_result = Some((osk.req_idx, osk.buffer));
            }
            Key::Minus => {
                self.osk_cancelled = true;
            }
        }
        true
    }

    #[inline]
    fn add_hit(&mut self, x: i32, y: i32, w: i32, h: i32, action: UiAction) {
        self.hits.push(HitBox {
            x,
            y,
            w,
            h,
            action,
            is_row_body: false,
        });
    }

    #[inline]
    fn add_row_hit(&mut self, x: i32, y: i32, w: i32, h: i32, row_idx: usize) {
        self.hits.push(HitBox {
            x,
            y,
            w,
            h,
            action: UiAction::SelectRow(row_idx),
            is_row_body: true,
        });
    }

    // -----------------------------------------------------------------------
    // ZL / ZR: Main Section Cycling (6 Top-Level Categories)
    // -----------------------------------------------------------------------
    pub fn cycle_sidebar_tab(&mut self, ed: &mut Editor, forward: bool) {
        // With no save open only the three sections that work on a card are
        // offered: the save list, the .arc list, and the tools (the CSV folder
        // is readable without a save). The four that edit a character slot are
        // left out rather than opening on "open a save first".
        let allowed: &[MainSection] = if ed.save.is_some() {
            &[
                MainSection::Save,
                MainSection::Hunter,
                MainSection::Items,
                MainSection::Gear,
                MainSection::Quest,
                MainSection::PalicoRecord,
                MainSection::Tools,
            ]
        } else {
            &[MainSection::Save, MainSection::Quest, MainSection::Tools]
        };
        let cur = section_of(ed.screen);
        let pos = allowed.iter().position(|&s| s == cur).unwrap_or(0);
        let next = if forward {
            (pos + 1) % allowed.len()
        } else {
            (pos + allowed.len() - 1) % allowed.len()
        };
        self.goto_section(ed, allowed[next]);
    }

    /// Put the set editor away when a section or sub-tab jump leaves the
    /// my-sets screen.
    ///
    /// The key path closes it through B, but a touch or a ZL / ZR jump changes
    /// `ed.screen` without going through the state machine, and a form left
    /// open on a record no screen shows is a trap.
    fn close_set_form(&mut self, ed: &mut Editor) {
        if ed.set_edit.is_some() {
            ed.set_edit = None;
            ed.set_clear_armed = None;
            ed.set_warnings.clear();
        }
        // The item set form is the other mode of the same screen and is put
        // away by the same jumps.
        if ed.item_set_edit.is_some() {
            ed.item_set_edit = None;
            ed.item_set_armed = None;
            ed.item_set_warnings.clear();
        }
    }

    /// Close the shared picker modal and every flag that says which writer its
    /// [A] lands in.
    ///
    /// The panel is one modal with three writers - the .mib field, the .sem
    /// area and the equipment form's 種別 - and each writer is a flag beside it.
    /// Closing the panel used to clear two of the three: a 種別 list closed with
    /// [B] left `equip_kind_picker` set, and the next .mib / area list's [A]
    /// then ran `apply_equip_kind` instead of the row the list was showing - a
    /// pick silently dropped, and a 種別 change the user never asked for.
    /// Opening a list and closing one go through here, so the three answers
    /// cannot drift apart.
    fn close_picker_modal(&mut self) {
        self.mib_picker = None;
        self.quest_area_slot = None;
        self.equip_kind_picker = false;
    }

    /// Put the palico equipment panel away when a jump leaves its screens.
    ///
    /// The key path closes it through B, but a touch or a ZL / ZR jump changes
    /// `ed.screen` without going through the state machine, and a panel left
    /// open on a screen that no longer draws it is a trap. Silent on purpose:
    /// the jump already sets a message of its own.
    fn close_palico_panel(&mut self, ed: &mut Editor) {
        ed.palico_equip = None;
        // The skill panel is the second mode of the same screen and is put
        // away by the same jumps: a form left open on a screen that no longer
        // draws it is the trap this exists for.
        ed.palico_skill = None;
    }

    fn goto_section(&mut self, ed: &mut Editor, sec: MainSection) {
        // The same question the sub-tab asks: ZL / ZR changes the screen
        // without going through the form's own keys, and the calls below throw
        // the draft away.
        if ed.form_leave_press() {
            return;
        }
        // The press that goes through drops the form. Without this the talisman
        // and equipment drafts stayed in memory (only the my-set forms were
        // closed here), so the next jump asked the same question again and [＋]
        // named a form that was not on screen.
        ed.drop_form_drafts();
        self.close_set_form(ed);
        self.close_palico_panel(ed);
        // The item bulk menu is a mode of the item box. A section jump changes
        // the screen without going through that screen's keys, so without this
        // the menu stayed open over the next screen and came back with it.
        ed.dismiss_item_menu();
        // The three sections a card can answer without a save are the file
        // side of the tool (save list, .arc list, CSV folder).
        let needs_save = !matches!(
            sec,
            MainSection::Save | MainSection::Quest | MainSection::Tools
        );
        if needs_save && ed.save.is_none() {
            ed.message = String::from("先にセーブデータを開いてください");
            return;
        }
        match sec {
            MainSection::Save => {
                ed.screen = Screen::SaveList;
                ed.cursor = 0;
            }
            MainSection::Hunter => {
                ed.screen = Screen::Char;
                if ed.field > 9 {
                    ed.field = 0;
                }
                ed.cursor = ed.field;
            }
            MainSection::Items => {
                ed.screen = Screen::Items;
                ed.cursor = 0;
            }
            MainSection::Gear => {
                ed.open_charms();
            }
            MainSection::PalicoRecord => {
                ed.screen = Screen::Roster;
                ed.roster_cursor = 0;
            }
            MainSection::Quest => {
                // The bar lights the tab whose list is really on screen. The
                // クエスト改造 tab is the editor: with no .arc loaded it falls
                // back to the save's list, and that list is セーブ内's - lighting
                // クエスト改造 over it gave one list two tabs, which is the fifth
                // review's finding. The list has to move with the bar too:
                // leaving the state machine's own tab where the last visit put
                // it (公式 / 没クエ) lit one tab and showed another tab's rows.
                ed.set_quest_tab(0);
                if ed.save.is_some() {
                    self.quest_bar_tab = 1;
                    ed.refresh_quest_titles();
                    ed.screen = Screen::SaveQuests;
                    let n = ed.filtered_quests().len();
                    ed.save_quest_cursor = ed.save_quest_cursor.min(n.saturating_sub(1));
                    ed.cursor = ed.save_quest_cursor;
                } else {
                    // SD(.arc) is the tab that lists files, and that is what
                    // this lands on.
                    self.quest_bar_tab = 4;
                    ed.screen = Screen::ArcList;
                    ed.cursor = 0;
                }
            }
            // 詳細・ツール opens on 生データ when there is a save to read: the
            // section's first tab. Without one it opens the CSV folder, which
            // is the one tool that lists something on a bare card.
            MainSection::Tools => {
                if ed.save.is_some() {
                    let from = ed.screen;
                    ed.open_save_map(from);
                } else {
                    ed.open_csv();
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // L / R: Sub-Tab System Inside the Current Section!
    // -----------------------------------------------------------------------
    /// The bar labels as owned strings.
    ///
    /// Most sections name their tabs with literals; the クエスト bar counts what
    /// its two bundles really carry, so the labels are built at run time and the
    /// whole row is one type.
    fn statics(labels: &'static [&'static str]) -> Vec<String> {
        labels.iter().map(|s| String::from(*s)).collect()
    }

    fn sub_tabs_for(&self, ed: &Editor) -> (Vec<String>, usize) {
        // The progress screen carries the three views itself: one screen with
        // three tabs, the same shape the quest entries have, so the bar names
        // them and L / R walk them. B goes back to the card.
        if ed.screen == Screen::Progress {
            let labels: &'static [&'static str] = &["勲章", "依頼", "クエスト"];
            return (Self::statics(labels), ed.progress_view().min(2));
        }
        // クエスト改造 carries the four questions the game asks about a quest,
        // so the bar names them and L / R walks them - the same shape the
        // progress screen uses for its three views.
        if ed.screen == Screen::QuestEdit {
            let labels: &'static [&'static str] = &["基本", "モンスター", "報酬", "支給品"];
            return (Self::statics(labels), ed.quest_edit_tab.min(3));
        }
        // ギルドカード carries four views of its own, and the bar named two of
        // them: 回数 and 武器 were reachable only with [Y], and while either was
        // on screen the bar lit ギルドカード - a lit tab that is not the view
        // showing, and a press on it did nothing at all (select_sub_tab returns
        // early on the lit tab). The four are this screen's tabs now, the same
        // shape 進行状況 and クエスト改造 already use.
        if ed.screen == Screen::Guild {
            let labels: &'static [&'static str] = &["カード", "回数", "武器", "闘技大会"];
            let active = match ed.guild_view() {
                GUILD_TAB_COUNTS => 1,
                GUILD_TAB_WEAPONS => 2,
                GUILD_TAB_ARENA => 3,
                _ => 0,
            };
            return (Self::statics(labels), active);
        }
        if is_quest_inner(ed.screen) {
            let labels: &'static [&'static str] = &[
                "条件(.mib)",
                "初期配置(.sem)",
                "文章(.gmd)",
                "構成(.arc)",
                "検査",
            ];
            let active = match ed.screen {
                Screen::Mib => 0,
                Screen::EntryView
                    if ed
                        .arc
                        .as_ref()
                        .and_then(|a| a.entries.get(ed.entry))
                        .and_then(|e| extension_for(e.type_hash))
                        == Some("sem") =>
                {
                    1
                }
                Screen::Gmd => 2,
                Screen::ArcEntries | Screen::EntryView => 3,
                Screen::Check => 4,
                _ => 0,
            };
            return (Self::statics(labels), active);
        }

        match section_of(ed.screen) {
            // セーブ: the two file-level jobs. 環境設定 and CSV moved to
            // 詳細・ツール, which is what made this row two tabs instead of
            // four - the everyday row no longer mixes them in.
            MainSection::Save => {
                let labels: &'static [&'static str] = &["セーブ一覧", "形式変換"];
                let active = match ed.screen {
                    Screen::SaveList => 0,
                    Screen::Convert => 1,
                    _ => 0,
                };
                (Self::statics(labels), active)
            }
            MainSection::Hunter => {
                let labels: &'static [&'static str] = &[
                    "基本ステータス",
                    "外見・性別",
                    "スロット",
                ];
                let active = match ed.screen {
                    Screen::Char if ed.field <= 9 => 0,
                    Screen::Char => 1,
                    Screen::Slots => 2,
                    _ => 0,
                };
                (Self::statics(labels), active)
            }
            MainSection::Items => {
                let labels: &'static [&'static str] = &[
                    "所持品",
                    "BOX 1-5",
                    "BOX 6-10",
                    "BOX 11-15",
                    "BOX 16-23",
                ];
                let active = if !ed.show_empty {
                    0
                } else if ed.cursor < 500 {
                    1
                } else if ed.cursor < 1000 {
                    2
                } else if ed.cursor < 1500 {
                    3
                } else {
                    4
                };
                (Self::statics(labels), active)
            }
            MainSection::Gear => {
                let labels: &'static [&'static str] = &[
                    "護石一覧",
                    "護石編集",
                    "装備BOX",
                    "マイセット",
                ];
                let active = match ed.screen {
                    Screen::CharmEdit | Screen::SkillPick => 1,
                    Screen::Equip | Screen::EquipEdit | Screen::JewelPick
                    | Screen::EquipPick => 2,
                    Screen::Sets => 3,
                    // 護石一覧. The tab the user pressed stays lit even when the
                    // form could not open (an empty box, or a search that found
                    // nothing): the list on screen is the one the form would
                    // have opened on, and the bar used to light 護石一覧 - a
                    // press that moved the highlight to a tab nobody pressed
                    // (the same rule the quest bar follows with quest_bar_tab).
                    // The answer is the state machine's (Editor::charm_tab),
                    // which is also the only place that knows the form could
                    // not open.
                    _ => ed.charm_tab.min(1),
                };
                (Self::statics(labels), active)
            }
            MainSection::PalicoRecord => {
                // The guild card carries two views of its own - the card and
                // the arena log it ends with - so the bar names both and the
                // cursor stays on one Screen. Six tabs still fit between the
                // L button (60px) and the right-hand tool pills (880px): the
                // widest label is ギルドカード at 6 CJK glyphs, 102px + 24 of
                // padding = 126, and 109+109+92+126+92+92 + 5*6 = 650.
                let labels: &'static [&'static str] = &[
                    "所持オトモ",
                    "配信オトモ",
                    "狩猟記録",
                    "ギルドカード",
                    "闘技大会",
                    "進行状況",
                ];
                let active = match ed.screen {
                    Screen::Roster => 0,
                    Screen::Palico => 1,
                    Screen::Hunts => 2,
                    Screen::Guild if ed.guild_view() == GUILD_TAB_ARENA => 4,
                    Screen::Guild => 3,
                    // The progress screen returns above with its own three
                    // tabs, so this arm is only reached when it is not showing.
                    _ => 5,
                };
                (Self::statics(labels), active)
            }
            MainSection::Quest => {
                // クエスト改造 is the section's first tab, because it is the
                // screen the work is done on: the four list tabs beside it are
                // where a quest is found, and [A] on one lands on this tab.
                //
                // The two bundle tabs count what the bundle really carries
                // (hbedit::Editor::quest_section_tab_labels): they printed 189
                // and 34 as literals, and a checkout without the generated
                // bundle has zero of each - a tab promising 189 quests over an
                // empty list.
                let labels = mhnx_core::hbedit::Editor::quest_section_tab_labels();
                let active = match ed.screen {
                    // The tab the user pressed is the tab that stays lit. The
                    // クエスト改造 tab opens the save's own list when no .arc is
                    // loaded, and this used to light セーブ内 instead - a press
                    // that moved the highlight to a tab nobody pressed.
                    Screen::SaveQuests => self.quest_bar_tab.min(3),
                    Screen::ArcList => 4,
                    Screen::QuestEquip => 5,
                    // QuestEdit has its own four tabs (基本/モンスター/…), so
                    // this arm is only reached when it is not showing.
                    _ => 0,
                };
                (labels, active)
            }
            // 詳細・ツール: the technical screens, kept out of the everyday
            // row. The screen is called 生データ everywhere else in this build
            // (the pill, the footer, the guide, the title), so the tab says the
            // same word: it used to say セーブマップ, and the two names for one
            // screen were printed side by side in the title.
            MainSection::Tools => {
                let labels: &'static [&'static str] = &[
                    "生データ",
                    "変更一覧",
                    "CSV入出力",
                    "環境設定",
                ];
                let active = match ed.screen {
                    Screen::SaveMap => 0,
                    Screen::Review => 1,
                    Screen::Csv => 2,
                    Screen::Options => 3,
                    _ => 0,
                };
                (Self::statics(labels), active)
            }
        }
    }

    pub fn cycle_sub_tab(&mut self, ed: &mut Editor, forward: bool) {
        let (labels, cur) = self.sub_tabs_for(ed);
        let len = labels.len();
        if len == 0 {
            return;
        }
        let next = if forward {
            (cur + 1) % len
        } else {
            (cur + len - 1) % len
        };
        self.select_sub_tab(ed, next);
    }

    pub fn select_sub_tab(&mut self, ed: &mut Editor, idx: usize) {
        // A press on the tab that is already lit is not a move: it used to run
        // the whole jump below, which put the list's cursor back at the top
        // (護石 / 装備 / マイセット / オトモ / 狩猟記録) - so a tap on the tab the
        // user was already on threw away the row they had reached. Nothing
        // happens now, on every screen. 進行状況 and クエスト改造 used to be
        // exempt because their bar is the same state machine; the exemption is
        // what made a tap on the lit クエスト改造 tab reset its cursor
        // (hbedit::Editor::quest_set_tab is a no-op on the tab that is showing
        // now, and this returns before any of the jump's work).
        if idx == self.sub_tabs_for(ed).1 {
            return;
        }
        // A tab press that would leave a form asks first, exactly as [B] does:
        // the jump below closes the form, and a draft dropped by a shoulder
        // press is the same loss [B] asks about through a key nobody expects to
        // be destructive. The second press goes through.
        if ed.form_leave_press() {
            return;
        }
        // The press that goes through drops the form it asked about - the same
        // half the section jump runs, and for the same reason.
        ed.drop_form_drafts();
        // The マイセット tab means the list, so a tab press also closes the
        // set editor; leaving the screen any other way does too.
        self.close_set_form(ed);
        // A pending CSV apply belongs to the screen it was armed on.
        ed.csv_armed = None;
        // Same rule for the palico equipment panel.
        self.close_palico_panel(ed);
        // And for the item bulk menu: a page / tab move leaves the screen the
        // menu belongs to, so the menu must not still be open on the way back.
        ed.dismiss_item_menu();

        // On the progress screen the sub-tabs are its three views, so a tab
        // press (or a tap on the bar) switches the view, not the section.
        if ed.screen == Screen::Progress {
            ed.progress_tab = idx.min(2);
            return;
        }
        // Same on クエスト改造: 基本 / モンスター / 報酬 / 支給品 are one screen.
        if ed.screen == Screen::QuestEdit {
            ed.quest_set_tab(idx);
            return;
        }

        if is_quest_inner(ed.screen) {
            safe_flush_quest_entry(ed);
            match idx {
                0 => {
                    if let Some(pos) = ed
                        .arc
                        .as_ref()
                        .and_then(|a| a.entries.iter().position(|e| extension_for(e.type_hash) == Some("mib")))
                    {
                        ed.gmd = None;
                        ed.screen = Screen::ArcEntries;
                        ed.cursor = pos;
                        ed.input(Key::A);
                    } else {
                        ed.message = String::from("この .arc には .mib が含まれていません");
                    }
                }
                1 => {
                    let sems = ed.sem_entry_indices();
                    if let Some(&pos) = sems.first() {
                        ed.gmd = None;
                        ed.entry = pos;
                        ed.cursor = pos;
                        ed.entry_row_cursor = 0;
                        ed.screen = Screen::EntryView;
                        ed.message = String::from("初期配置座標 (.sem): [◀/▶]でエリア・X/Y/Z座標を変更 ([Y]で刻み)");
                    } else {
                        ed.message = String::from("この .arc には .sem (初期配置) が含まれていません");
                    }
                }
                2 => {
                    if let Some(pos) = ed
                        .arc
                        .as_ref()
                        .and_then(|a| a.entries.iter().position(|e| extension_for(e.type_hash) == Some("gmd")))
                    {
                        ed.screen = Screen::ArcEntries;
                        ed.cursor = pos;
                        ed.input(Key::A);
                    } else {
                        ed.message = String::from("この .arc には .gmd が含まれていません");
                    }
                }
                3 => {
                    ed.gmd = None;
                    ed.screen = Screen::ArcEntries;
                    ed.cursor = ed.entry;
                }
                _ => {
                    ed.gmd = None;
                    ed.run_check();
                }
            }
            return;
        }

        match section_of(ed.screen) {
            MainSection::Save => match idx {
                0 => {
                    ed.screen = Screen::SaveList;
                    ed.cursor = 0;
                }
                _ => {
                    if ed.save.is_none() {
                        ed.message = String::from("変換するには先にセーブデータを開いてください");
                    } else {
                        ed.screen = Screen::Convert;
                    }
                }
            },
            // 詳細・ツール. The CSV tab lists a folder, so it opens even with
            // no save loaded: the user may want to see what is on the card
            // first. The other two need a save and say so.
            MainSection::Tools => match idx {
                0 => {
                    if ed.save.is_none() {
                        ed.message = String::from("生データを見るには先にセーブデータを開いてください");
                    } else {
                        let from = if ed.screen == Screen::SaveMap {
                            ed.map_return
                        } else {
                            ed.screen
                        };
                        ed.open_save_map(from);
                    }
                }
                // 変更一覧 builds its list on the way in; the screen's own [Y]
                // rebuilds it, and a save with no reference says so instead of
                // showing an empty list.
                1 => {
                    if ed.save.is_none() {
                        ed.message = String::from(
                            "変更一覧を見るには先にセーブデータを開いてください",
                        );
                    } else {
                        ed.open_review();
                    }
                }
                2 => ed.open_csv(),
                _ => {
                    if ed.save.is_none() {
                        ed.message = String::from("設定を開くには先にセーブデータを開いてください");
                    } else {
                        ed.screen = Screen::Options;
                        ed.option_cursor = 0;
                    }
                }
            },
            MainSection::Hunter => match idx {
                0 => {
                    ed.screen = Screen::Char;
                    if ed.field > 9 {
                        ed.field = 0;
                    }
                    ed.cursor = ed.field;
                }
                1 => {
                    ed.screen = Screen::Char;
                    if ed.field < 10 {
                        ed.field = 10;
                    }
                    ed.cursor = ed.field;
                }
                _ => {
                    ed.screen = Screen::Slots;
                    ed.cursor = ed.slot;
                }
            },
            MainSection::Items => {
                ed.screen = Screen::Items;
                match idx {
                    0 => {
                        ed.show_empty = false;
                        ed.cursor = 0;
                        ed.message = String::from("所持アイテムのみ表示中 ([A]でアイテム変更 / [◀▶]で個数)");
                    }
                    1 => {
                        ed.show_empty = true;
                        ed.cursor = 0;
                        ed.message = String::from("全枠表示: BOX 1〜5 (#1〜#500) — 空き枠にも[A]で追加可能");
                    }
                    2 => {
                        ed.show_empty = true;
                        ed.cursor = 500.min(ed.visible_items().len().saturating_sub(1));
                        ed.message = String::from("全枠表示: BOX 6〜10 (#501〜#1000)");
                    }
                    3 => {
                        ed.show_empty = true;
                        ed.cursor = 1000.min(ed.visible_items().len().saturating_sub(1));
                        ed.message = String::from("全枠表示: BOX 11〜15 (#1001〜#1500)");
                    }
                    _ => {
                        ed.show_empty = true;
                        ed.cursor = 1500.min(ed.visible_items().len().saturating_sub(1));
                        ed.message = String::from("全枠表示: BOX 16〜23 (#1501〜#2300)");
                    }
                }
            }
            MainSection::Gear => match idx {
                0 => ed.open_charms(),
                // 護石編集: the tab opens the *form*, not the list. It used to
                // call open_charms() - the same call the tab beside it makes -
                // so pressing 護石編集 put the list back on screen, lit 護石一覧
                // and left the form one more [A] away. The form opens on the
                // record the list was on, and says so when there is none - and
                // the tab stays lit in that case, because the press was still
                // the press (hbedit::Editor::charm_tab).
                // 護石編集: the tab lights and the list stays. It used to call
                // the form opener, so [R] - the shoulder that walks
                // the tab strip - opened an edit form on whatever row the
                // cursor was on. The form opens from [A] on the row it edits
                // (hbedit::Editor::charm_a_press); a tab press is navigation.
                1 => ed.charm_tab_press(),
                // The row the box was on is kept: a tab step used to put the
                // list back at the top, so crossing to 装備BOX and back cost
                // the row the user had reached (hbedit::Editor::open_equip_box).
                2 => ed.open_equip_box(),
                _ => {
                    ed.screen = Screen::Sets;
                    self.sets_cursor = 0;
                }
            },
            // The guild card's own four views are the bar's tabs while that
            // screen is showing (sub_tabs_for): the card, the quest counters,
            // the weapon grid and the arena log. A press on one of them is a
            // view switch, and the lit tab is the view that is really on
            // screen - it used to light ギルドカード over 回数 and 武器, two of
            // the four views the bar did not name at all.
            MainSection::PalicoRecord if ed.screen == Screen::Guild => {
                if ed.save.is_none() {
                    ed.message = String::from("先にセーブデータを開いてください");
                    return;
                }
                let view = match idx {
                    1 => GUILD_TAB_COUNTS,
                    2 => GUILD_TAB_WEAPONS,
                    3 => GUILD_TAB_ARENA,
                    _ => mhnx_core::hbedit::GUILD_TAB_CARD,
                };
                ed.set_guild_view(view);
            }
            MainSection::PalicoRecord => match idx {
                // The three lists keep their rows across a tab step: a tab is
                // navigation, and putting the cursor back at the top made
                // crossing to 狩猟記録 and back cost the row the user had
                // reached. Each screen clamps its own cursor when it draws.
                0 => ed.open_roster(),
                1 => {
                    ed.screen = Screen::Palico;
                    ed.cursor = ed.palico_cursor;
                }
                2 => ed.open_hunts(),
                3 => {
                    ed.screen = Screen::Guild;
                    ed.set_guild_view(mhnx_core::hbedit::GUILD_TAB_CARD);
                }
                4 => {
                    if ed.save.is_none() {
                        ed.message = String::from("先にセーブデータを開いてください");
                        return;
                    }
                    ed.screen = Screen::Guild;
                    ed.set_guild_view(GUILD_TAB_ARENA);
                }
                _ => {
                    if ed.save.is_none() {
                        ed.message = String::from("先にセーブデータを開いてください");
                        return;
                    }
                    ed.open_progress();
                }
            },
            MainSection::Quest => match idx {
                // クエスト改造: the screen an opened quest lands on. With no
                // .arc loaded the tab goes to the list a quest is opened from
                // and says so, rather than offering a screen with no quest.
                0 => {
                    if ed.arc.is_some() {
                        // The editor is showing, so the editor's tab is the
                        // lit one.
                        self.quest_bar_tab = 0;
                        // The tab is pressed from the list the quest came from,
                        // so that list is where [B] goes back to.
                        let from = ed.screen;
                        ed.enter_quest_edit_from(from);
                    } else if ed.save.is_some() {
                        // No .arc loaded: the tab lands on the save's list, and
                        // that list is セーブ内's. Lighting クエスト改造 over it
                        // gave one list two tabs; the bar now names the tab the
                        // rows belong to.
                        self.quest_bar_tab = 1;
                        ed.set_quest_tab(0);
                        ed.refresh_quest_titles();
                        ed.screen = Screen::SaveQuests;
                        ed.message =
                            String::from("改造するクエストを選んで [A] で開いてください");
                    } else {
                        // SD(.arc) is the tab that lists files.
                        self.quest_bar_tab = 4;
                        ed.screen = Screen::ArcList;
                        ed.cursor = 0;
                        ed.message = String::from(".arc を選んで [A] で開くと改造画面になります");
                    }
                }
                1..=3 => {
                    if ed.save.is_none() {
                        ed.message = String::from("先にセーブデータを開いてください");
                        return;
                    }
                    ed.set_quest_tab(idx - 1);
                    self.quest_bar_tab = idx;
                    ed.refresh_quest_titles();
                    ed.screen = Screen::SaveQuests;
                }
                4 => {
                    self.quest_bar_tab = 4;
                    ed.screen = Screen::ArcList;
                    ed.cursor = 0;
                }
                _ => {
                    if ed.save.is_none() {
                        ed.message = String::from("先にセーブデータを開いてください");
                        return;
                    }
                    self.quest_bar_tab = 5;
                    ed.screen = Screen::QuestEquip;
                    ed.qe_open = false;
                    ed.cursor = 0;
                }
            },
        }
    }

    // -----------------------------------------------------------------------
    // Page Scroll & MIB Picker Modal
    // -----------------------------------------------------------------------
    pub fn page_scroll(&mut self, ed: &mut Editor, delta: i32) {
        if ed.screen == Screen::SaveQuests && ed.quest_tab > 0 {
            // The tab on screen owns its cursor (hbedit::Editor::
            // quest_list_cursor), so a page turn cannot move a list that is not
            // showing.
            let len = ed.quest_list_len();
            if len > 0 {
                let cur = ed.quest_list_cursor() as i32;
                let next = (cur + delta).clamp(0, len as i32 - 1) as usize;
                ed.set_quest_list_cursor(next);
            }
            return;
        }
        if ed.screen == Screen::Sets {
            // The equipment form has eight rows on one panel, so there is
            // nothing to page there; the item form has thirty-two and pages
            // itself through the cursor. Neither may move the list underneath.
            if ed.set_editor_open() {
                return;
            }
            let len = sets_len(ed, self.sets_tab);
            if len > 0 {
                let next = (self.sets_cursor as i32 + delta).clamp(0, len as i32 - 1) as usize;
                self.sets_cursor = next;
            }
            return;
        }
        scroll_active_cursor(ed, delta);
    }

    /// How many rows the list on screen shows at once.
    ///
    /// Read off the bar the last frame drew, because it is the one place that
    /// knows both the row count and the row a warning strip took away. A page
    /// key and the scrollbar therefore cannot disagree about the size of a page.
    fn page_rows(&self) -> usize {
        match &self.bar {
            Some(bar) if bar.scroll.active() => bar.scroll.visible(),
            _ => 9,
        }
    }

    /// One page up ([dir] < 0) or down on whatever list is showing.
    ///
    /// A page is what the screen shows, not a fixed eight rows: the equipment
    /// box shows nine and a warning strip costs one, and paging by a number the
    /// user cannot see is how a list skips entries it was asked to show.
    pub fn page_by(&mut self, ed: &mut Editor, dir: i32) {
        if dir == 0 {
            return;
        }
        if self.mib_picker.is_some() {
            self.scroll_mib_picker(dir * MIB_PICKER_VISIBLE as i32);
            return;
        }
        // The my-sets form has no page to turn, and the list under it must not
        // move while the form is open.
        if ed.screen == Screen::Sets && ed.set_editor_open() {
            return;
        }
        // 生データ with the byte editor open: the stick pages the *bytes*, not
        // the list. The equipment box is 72,000 bytes and the list is one row.
        if ed.screen == Screen::SaveMap && ed.map_edit_open() {
            ed.map_edit_page(dir);
            return;
        }
        let len = list_len_of(ed, self);
        if len == 0 {
            return;
        }
        let cur = active_cursor_of(ed, self);
        let next = page_move(cur, len, self.page_rows(), dir);
        if next != cur {
            set_active_cursor(ed, self, next);
        }
    }

    /// Jump to the first row, or to the last one.
    ///
    /// The end of a 340 row list was reachable only by holding a direction key
    /// down, which is the same as not being reachable.
    pub fn jump_end(&mut self, ed: &mut Editor, forward: bool) {
        if let Some(picker) = self.mib_picker.as_mut() {
            picker.cursor = list_end(picker.items.len(), forward);
            return;
        }
        if ed.screen == Screen::Sets && ed.set_editor_open() {
            return;
        }
        // The ends of the byte region while the 生データ editor is open, for the
        // same reason the pages move bytes there.
        if ed.screen == Screen::SaveMap && ed.map_edit_open() {
            ed.map_edit_jump(forward);
            return;
        }
        let len = list_len_of(ed, self);
        if len == 0 {
            return;
        }
        let next = list_end(len, forward);
        if next != active_cursor_of(ed, self) {
            set_active_cursor(ed, self, next);
        }
    }

    /// Draw one list's scrollbar and remember it for the touch handler.
    ///
    /// A list that fits draws nothing: a thumb that cannot move would claim the
    /// table is longer than it is. The bar is drawn 6px (14px while a finger
    /// holds it), the area that answers a finger is 40px
    /// (hbedit::SCROLL_HIT_W), and the row - its tap area and its value - stops
    /// 8px short of that area, so the two never meet on screen.
    #[allow(clippy::too_many_arguments)]
    fn draw_list_bar(
        &mut self,
        c: &mut Canvas,
        ed: &Editor,
        panel_x: i32,
        panel_w: i32,
        track_y: i32,
        track_h: i32,
        scroll: ScrollBar,
        ids: &[usize],
        picker: bool,
    ) {
        let t = theme();
        let track = scroll_track(panel_x, panel_w);
        let active = scroll.active();
        self.bar = Some(BarState {
            screen: ed.screen,
            track_y,
            track_h,
            hit_x: track.hit_x,
            hit_w: track.hit_w,
            scroll,
            ids: if active { ids.to_vec() } else { Vec::new() },
            picker,
        });
        if !active {
            return;
        }
        // The groove, then the thumb. The thumb turns amber while it is being
        // dragged, so a finger knows it is holding the bar and not a row; the
        // bar itself also grows for as long as the finger holds it, because a
        // 6px line is something the eye reads but a thumb cannot hold. The
        // growth is even, so the bar widens around its own centre line and the
        // strip it covers never moves.
        let dragging = matches!(&self.bar_drag, Some(d) if d.picker == picker);
        let bar_w = if dragging {
            mhnx_core::hbedit::SCROLL_BAR_ACTIVE_W
        } else {
            track.bar_w
        };
        let bar_x = track.bar_x - (bar_w - track.bar_w) / 2;
        c.round_rect(bar_x, track_y, bar_w, track_h, bar_w / 2, t.bg_btn);
        let thumb_col = if dragging {
            t.accent_secondary
        } else {
            t.accent_primary
        };
        let thumb = scroll.thumb_len(track_h);
        let off = scroll.thumb_offset(track_h);
        c.round_rect(
            bar_x,
            track_y + off,
            bar_w,
            thumb,
            bar_w / 2,
            thumb_col,
        );
    }

    /// Does a finger at this point take hold of the scrollbar? Start a drag if
    /// so, and answer true: nothing under the bar may also answer this tap.
    fn begin_bar_drag(&mut self, ed: &Editor, tx: i32, ty: i32) -> bool {
        let Some(bar) = self.bar.as_ref() else {
            return false;
        };
        // A bar with nothing to scroll is not on the glass, so it cannot take a
        // finger either. The state is written by the last frame that drew a
        // list, and an empty list returns before it draws one - so without the
        // row count here a 0-row list kept the previous screen's bar and the
        // 40px strip beside it answered a tap with no thumb under it.
        if bar.scroll.total() == 0 {
            return false;
        }
        if bar.screen != ed.screen || !bar.scroll.active() {
            return false;
        }
        if tx < bar.hit_x || tx >= bar.hit_x + bar.hit_w {
            return false;
        }
        if ty < bar.track_y || ty >= bar.track_y + bar.track_h {
            return false;
        }
        let thumb = bar.scroll.thumb_len(bar.track_h);
        let off = bar.scroll.thumb_offset(bar.track_h);
        // Where the finger landed inside the thumb. A finger that landed beside
        // it takes hold of the nearest end instead of teleporting the list, and
        // a tap that never moves is a page rather than a drag (the thumb itself
        // is not a page button, so that case does nothing).
        let landed = ty - bar.track_y - off;
        let grab = landed.clamp(0, (thumb - 1).max(0));
        let cur = active_cursor_of(ed, self);
        let inside = bar
            .ids
            .iter()
            .position(|&id| id == cur)
            .unwrap_or(0)
            .saturating_sub(bar.scroll.start());
        let picker = bar.picker;
        self.bar_drag = Some(BarDrag {
            grab,
            inside,
            start_y: ty,
            moved: false,
            on_thumb: landed >= 0 && landed < thumb,
            above: landed < 0,
            picker,
        });
        true
    }

    /// The row a thumb at this height points at, and the row the cursor is on.
    fn bar_target(&self, ed: &Editor, ty: i32, drag: BarDrag) -> Option<(usize, usize)> {
        let bar = self.bar.as_ref()?;
        if !bar.scroll.active() {
            return None;
        }
        let offset = ty - drag.grab - bar.track_y;
        let start = bar.scroll.start_for_offset(bar.track_h, offset);
        let pos = (start + drag.inside).min(bar.scroll.total().saturating_sub(1));
        let target = bar.ids.get(pos).copied().unwrap_or(pos);
        let cur = if drag.picker {
            self.mib_picker.as_ref().map(|p| p.cursor).unwrap_or(0)
        } else {
            active_cursor_of(ed, self)
        };
        Some((target, cur))
    }

    /// Follow a dragged thumb: the cursor lands on the row the bar now shows,
    /// keeping its place inside the window so the list scrolls rather than
    /// dragging the selection to the top.
    fn drag_bar(&mut self, ed: &mut Editor, ty: i32) -> bool {
        let Some(mut drag) = self.bar_drag else {
            return false;
        };
        if !drag.moved {
            // A finger that has not moved yet is a tap, not a drag: a tap on
            // the bar scrolls a page (see handle_touch) and must not throw the
            // list to wherever the finger landed.
            if (ty - drag.start_y).abs() < 4 {
                return false;
            }
            drag.moved = true;
            self.bar_drag = Some(drag);
        }
        let Some((target, cur)) = self.bar_target(ed, ty, drag) else {
            return false;
        };
        if target == cur {
            return false;
        }
        if drag.picker {
            let Some(mut picker) = self.mib_picker.take() else {
                return false;
            };
            picker.cursor = target.min(picker.items.len().saturating_sub(1));
            self.mib_picker = Some(picker);
            return true;
        }
        set_active_cursor(ed, self, target);
        true
    }

    /// The coast after a flick: keep moving at the speed the finger left with,
    /// losing a quarter of it a frame until there is nothing left to give.
    fn tick_fling(&mut self, ed: &mut Editor) -> bool {
        if self.fling == 0 {
            return false;
        }
        if self.mib_picker.is_some() || self.osk.is_some() || ed.review_open() {
            self.fling = 0;
            return false;
        }
        let rows = self.fling / FLING_UNIT;
        self.fling = self.fling * 3 / 4;
        if self.fling.abs() < FLING_UNIT / 2 {
            self.fling = 0;
        }
        if rows == 0 {
            return false;
        }
        self.page_scroll(ed, rows);
        true
    }

    /// Scroll the save-review dialog by `delta` lines.
    ///
    /// The dialog is modal, so the four directions are free while it is up:
    /// [▲] / [▼] move one line and [◀] / [▶] ten, and the two buttons in its
    /// footer send the same two steps. Nothing else about the dialog moves -
    /// [A] still writes and [B] still cancels.
    pub fn scroll_review(&mut self, ed: &Editor, delta: i32) {
        let total = ed.review.as_ref().map(|r| r.lines.len()).unwrap_or(0);
        let max = total.saturating_sub(1) as i64;
        self.review_scroll = (self.review_scroll as i64 + delta as i64).clamp(0, max) as usize;
    }

    /// Stop a coasting list: a pad press has taken over from the flick.
    pub fn stop_fling(&mut self) {
        self.fling = 0;
    }

    /// Drop every gesture in progress.
    ///
    /// Used when a modal takes the screen: a flick that started before the
    /// dialog opened must not keep scrolling the list under it.
    fn cancel_gestures(&mut self) {
        self.bar_drag = None;
        self.fling = 0;
        self.drag_rows = 0;
        self.dragged = false;
        self.pending_activate_row = None;
    }

    pub fn scroll_mib_picker(&mut self, delta: i32) {
        if let Some(picker) = &mut self.mib_picker {
            let len = picker.items.len();
            if len > 0 {
                let next = (picker.cursor as i32 + delta).clamp(0, len as i32 - 1) as usize;
                picker.cursor = next;
            }
        }
    }

    pub fn open_mib_picker(&mut self, ed: &mut Editor, field_idx: usize) -> bool {
        // Whatever the last chooser was for, this one is a .mib field: every
        // other writer is disarmed before the list is built, so a pick can only
        // ever write the field the list is showing.
        self.close_picker_modal();
        let Some((title, items, cur_val)) = mib_field_choices(ed.mib_bytes(), field_idx) else {
            return false;
        };
        if items.is_empty() {
            return false;
        }
        ed.mib_cursor = field_idx;
        ed.cursor = field_idx;
        let cursor = items.iter().position(|(v, _)| *v == cur_val).unwrap_or(0);
        self.mib_picker = Some(MibPickerState {
            field_idx,
            title,
            items,
            cursor,
        });
        true
    }

    /// Open the equipment form's 種別 list: the kinds a box holds.
    ///
    /// The row used to step one kind per press, so reaching 弓 from 頭 cost
    /// fourteen presses; the list is one press and the same modal the .mib and
    /// エリア choosers use. The count in the title is the table's own length
    /// (equip::BOX_KINDS): it said 20 over a list of 19, and "Twenty-one kinds"
    /// in the core's comment, so three numbers described one table.
    fn open_equip_kind_picker(&mut self, ed: &mut Editor) -> bool {
        let Some(e) = ed.equip_edit else {
            return false;
        };
        let items = mhnx_core::hbedit::Editor::equip_kind_choices();
        if items.is_empty() {
            return false;
        }
        let cursor = items
            .iter()
            .position(|(v, _)| *v == e.kind as u32)
            .unwrap_or(0);
        // The two other writers are disarmed first: a 種別 list opened over a
        // .mib field's list is still one modal, and the flag says which writer
        // the [A] lands in.
        self.close_picker_modal();
        self.equip_kind_picker = true;
        self.mib_picker = Some(MibPickerState {
            field_idx: usize::MAX,
            title: alloc::format!(
                "種別を選ぶ（ハンターの箱に入る {} 種）",
                mhnx_core::equip::BOX_KINDS.len()
            ),
            items,
            cursor,
        });
        true
    }

    /// Open the エリア番号 chooser for one monster slot.
    ///
    /// The area is the number a custom quest most often gets wrong, so the
    /// row it belongs to is the row's main value and this is a list, not a
    /// number to step: 0..=20 is the range the .sem screen clamps to, and the
    /// same writer (hbedit::Editor::set_sem_area) writes the pick.
    pub fn open_quest_area_picker(&mut self, ed: &mut Editor, slot: usize) -> bool {
        let Some(sem) = ed.sem_of_slot(slot) else {
            ed.message = format!("スロット{} の .sem がないのでエリアを選べません", slot + 1);
            return false;
        };
        let _ = sem;
        let current = ed
            .quest_monsters()
            .get(slot)
            .and_then(|m| m.area)
            .unwrap_or(0);
        let mut items: Vec<(u32, String)> = Vec::with_capacity(21);
        for area in 0..=20u32 {
            let label = if area == 0 {
                String::from("0: 未使用（出現しない）")
            } else {
                format!("エリア {}", area)
            };
            items.push((area, label));
        }
        let cursor = items.iter().position(|(v, _)| *v == current).unwrap_or(0);
        // One modal, one writer: the 種別 flag and the previous list go first.
        self.close_picker_modal();
        self.quest_area_slot = Some(slot);
        self.mib_picker = Some(MibPickerState {
            field_idx: usize::MAX,
            title: format!("スロット{} のエリア番号（.sem +0x0C）", slot + 1),
            items,
            cursor,
        });
        true
    }

    /// Open whatever [A] opens on one .mib row, and say what happened.
    ///
    /// One row of the .mib screen is not "a monster" by itself: the 種類 byte
    /// beside the id decides whether the list is the monster table (狩猟/討伐,
    /// 部位破壊, ...) or the item picker (納品, 採取), and a kind that carries no
    /// id has no list at all. Both the pad and the touch chip come through here,
    /// so a tap cannot open a different table from [A].
    fn open_mib_row_picker(&mut self, ed: &mut Editor, field_idx: usize) -> bool {
        match mhnx_core::hbedit::mib_pick_kind(ed.mib_bytes(), field_idx) {
            mhnx_core::hbedit::MibPickKind::Item => {
                if ed.open_mib_item_pick(field_idx) {
                    true
                } else {
                    ed.message = String::from("対象アイテムの一覧を開けません");
                    false
                }
            }
            mhnx_core::hbedit::MibPickKind::Monster => {
                if self.open_mib_picker(ed, field_idx) {
                    true
                } else {
                    ed.message = String::from("モンスターの一覧を作れません");
                    false
                }
            }
            // 部位破壊: the list is the parts the shipped quests measured for
            // the monster of this row, so the part is chosen by name.
            mhnx_core::hbedit::MibPickKind::Part => {
                if self.open_mib_picker(ed, field_idx) {
                    true
                } else {
                    ed.message = String::from("部位の一覧を作れません");
                    false
                }
            }
            mhnx_core::hbedit::MibPickKind::None => {
                ed.message = match ed.mib_target_info(field_idx) {
                    Some(info) => format!(
                        "{} は {} です（一覧で選ぶ対象IDがありません。[◀/▶] で数値を変更）",
                        info.kind_label,
                        mhnx_core::hbedit::target_id_word(info.id_kind)
                    ),
                    None => String::from("この項目は [◀ / ▶] キーで数値を直接変更できます"),
                };
                false
            }
        }
    }

    pub fn handle_mib_picker_key(&mut self, ed: &mut Editor, key: Key) -> bool {
        let Some(mut picker) = self.mib_picker.take() else {
            return false;
        };
        let len = picker.items.len();
        if len == 0 {
            return true;
        }
        match key {
            Key::Up => {
                picker.cursor = (picker.cursor + len - 1) % len;
                self.mib_picker = Some(picker);
            }
            Key::Down => {
                picker.cursor = (picker.cursor + 1) % len;
                self.mib_picker = Some(picker);
            }
            Key::Left | Key::L => {
                picker.cursor = picker.cursor.saturating_sub(10);
                self.mib_picker = Some(picker);
            }
            Key::Right | Key::R => {
                picker.cursor = (picker.cursor + 10).min(len - 1);
                self.mib_picker = Some(picker);
            }
            Key::A => {
                if let Some((raw_val, label)) = picker.items.get(picker.cursor).cloned() {
                    // Three lists share this modal and each has its own writer:
                    // the equipment form's 種別 (the kind list), the エリア番号
                    // chooser (the .sem byte) and the .mib field (the monster /
                    // part / item table). Both routes - this press and a tap on
                    // a row - come through here, so they cannot write to
                    // different places.
                    if self.equip_kind_picker {
                        self.equip_kind_picker = false;
                        ed.apply_equip_kind(raw_val as u8);
                        return true;
                    }
                    let target = match self.quest_area_slot.take() {
                        Some(slot) => MibPickTarget::Area(slot),
                        None => MibPickTarget::Field(picker.field_idx),
                    };
                    ed.apply_mib_pick(target, raw_val, &label);
                }
            }
            Key::B | Key::Minus => {
                // Every writer, not just the two: a 種別 flag left set made the
                // next list's [A] write a kind instead of the row.
                self.close_picker_modal();
            }
            _ => {
                self.mib_picker = Some(picker);
            }
        }
        true
    }

    // -----------------------------------------------------------------------
    // Unified Controller Key Handler (A=Open/Edit, L/R=SubTab, X=Save, Y=Tool)
    // -----------------------------------------------------------------------
    /// 詳細: from the aggregate screen to the file-by-file screens.
    ///
    /// Nothing is thrown away: the working copies are flushed first, so the
    /// .mib and .gmd screens open on the bytes this screen was showing. [B]
    /// on any of them comes back here (see handle_pad_key).
    fn open_quest_detail(&mut self, ed: &mut Editor) {
        ed.flush_quest_edit();
        ed.screen = Screen::ArcEntries;
        ed.cursor = ed.entry;
        ed.message = String::from("ファイル: ファイル単位の画面です（[B] でクエスト改造へ戻ります）");
    }

    /// 詳細: the raw bytes behind the screen that is showing.
    ///
    /// Every record screen answers this the same way - the save map row for
    /// the region it edits - which is why the button can sit on all of them
    /// without a destination per screen. The screens that already *are* a
    /// detail view answer with a message rather than leaving a dead button.
    pub fn open_detail(&mut self, ed: &mut Editor) {
        let target = ed.detail_target(self.sets_tab);
        self.open_detail_at(ed, target);
    }

    /// The same, on the screen state captured when [R] went down.
    ///
    /// [R]'s tap waits for the release (hbedit::ShoulderTap), so the hold that
    /// opens this can arrive a second after the press. The review's requirement
    /// was that the detail view opens on the screen the user was looking at
    /// when they pressed - and the row is resolved from the snapshot
    /// (hbedit::Editor::detail_row_of), not from whatever the release frame
    /// shows.
    pub fn open_detail_at(
        &mut self,
        ed: &mut Editor,
        target: mhnx_core::hbedit::DetailTarget,
    ) {
        let from = target.screen;
        // クエスト改造's 詳細 is its own set of file screens, not the save map.
        if from == Screen::QuestEdit {
            self.open_quest_detail(ed);
            return;
        }
        if from == Screen::SaveMap {
            ed.message = String::from("生データを表示中です（[B] で戻ります）");
            return;
        }
        if matches!(
            from,
            Screen::ArcList
                | Screen::ArcEntries
                | Screen::EntryView
                | Screen::Mib
                | Screen::Gmd
                | Screen::Check
        ) {
            ed.message = String::from("この画面はクエストの詳細表示です（[B] で戻ります）");
            return;
        }
        // A pick list is a mode of the screen under it: jumping away from it
        // would leave a pick half answered.
        if matches!(
            from,
            Screen::ItemPick | Screen::SkillPick | Screen::JewelPick | Screen::EquipPick
        ) {
            ed.message = String::from("選択中は生データを開けません（[B] で戻ってから）");
            return;
        }
        if ed.save.is_none() {
            ed.message = String::from("生データを見るには先にセーブデータを開いてください");
            return;
        }
        // A form left open on a screen that is no longer drawn is the trap the
        // section jumps already close; the same three rules apply here.
        self.close_set_form(ed);
        self.close_palico_panel(ed);
        ed.dismiss_item_menu();
        // The row comes from the snapshot: three screens resolve theirs from a
        // view number, and the state machine owns that rule now
        // (hbedit::Editor::detail_row_of).
        let row = ed.detail_row_of(target);
        if row.is_none() {
            ed.message = String::from("この画面には対応する生データがありません");
            return;
        }
        ed.open_save_map_at(from, row);
    }

    /// Feed one pad key. The console calls this on the frame the key goes down
    /// (a held direction repeats through the same entry point), so every answer
    /// here is a key-down answer.
    ///
    /// The key map, by route, and why nothing collides:
    ///
    /// | key            | pick lists (Item/Skill/Jewel/EquipPick) | item box / roster / palico panels | every other screen |
    /// |----------------|--------------------------------------|-----------------------------------|--------------------|
    /// | [◀] / [▶]      | 10 rows (hbedit::pick_move)           | value / page by the row's meaning | value ±step; 10 on an id row |
    /// | [▲] / [▼]      | 1 row                                 | 1 row                             | 1 row              |
    /// | held [◀]/[▶]   | repeats at 8 / 4 / 3 frames           | repeats at 8 / 4 / 3 frames       | repeats at 8 / 4 / 3 frames |
    /// | [L] / [R] tap  | ten rows, same as [◀] / [▶], repeated while held | the mode's own key (swallowed by the set / palico form) | previous / next sub-tab; [R]'s tap runs on the *release* |
    /// | held [L]       | (repeats the ten-row step)            | —                                 | first hold offers 元に戻す, second hold runs it |
    /// | held [R]       | (repeats the ten-row step)            | —                                 | opens 詳細 (from the screen the key went down on) |
    /// | [A]            | 決定                                  | the row's own action              | the row's own action (equipment / talisman forms: 選ぶ・書き込み・作成) |
    /// | [Y]            | 検索 (the keyboard)                    | 一覧の表示切替 (item box: 空きも表示) / the mode's own key | 一覧の 検索・表示切替, otherwise the screen's own second action |
    /// | [X]            | クリア／保存 (per screen)              | 保存                              | 保存 (global QuickSave) |
    /// | [ZR]           | 種別 filter (the name picker; the other three send it to the section bar) | the screen's own second action | the screen's own second action, else the section bar |
    /// | [－] / [＋]     | 2-press delete / 終了                  | 2-press delete / quit             | 2-press delete / the screen's own tool |
    ///
    /// The two rules that make it collision-free: (1) [L] is answered on the
    /// press and its hold is a *second* answer that never undoes the first, so a
    /// hold cannot lose its tab move and two taps cannot be swallowed by a
    /// hold - while [R]'s tap waits for the release, because its hold opens
    /// 詳細 and a hold that also moved the sub-tab opened it on the wrong screen
    /// (hbedit::ShoulderTap); (2) a pick list owns L / R as movement before the
    /// sub-tab rule is reached (see `shoulder_moves`), exactly as it owns
    /// [◀] / [▶].
    pub fn handle_pad_key(&mut self, ed: &mut Editor, key: Key, _cur_buttons: u64) {
        // What a shoulder press means is the state machine's answer
        // (hbedit::Editor::shoulder_job): the sub-tab by default, the ten row
        // jump of a pick list, the screen's own key where a footer promises
        // "[R] 数値入力", and nothing at all under a form that owns the screen.
        //
        // The console used to keep its own copy of that rule as a list of
        // exceptions, and the list is what the review found twice: [R] on the
        // item box and on the hunt record moved the BOX tab instead of opening
        // the keyboard their footers promised, because neither screen was on
        // it. There is one answer now, and a test drives it.
        if matches!(key, Key::L | Key::R) {
            match ed.shoulder_job(key) {
                Shoulder::SubTab => {
                    self.cycle_sub_tab(ed, key == Key::R);
                    return;
                }
                // The three pick lists move through one rule (hbedit::pick_move):
                // [◀] / [▶] take ten rows and [▲] / [▼] one, and the second
                // stage of the bulk add steps its count the same way. The
                // shoulders are turned into that pair here rather than
                // scrolling on their own, so the two layers cannot disagree
                // about the same key.
                Shoulder::Move => {
                    ed.input(if key == Key::R { Key::Right } else { Key::Left });
                    return;
                }
                Shoulder::Swallow => return,
                // The screen's own key: fall through to the dispatch below, so
                // the screen's own arm answers it. That distinction matters -
                // [R] on the item box opens the count keyboard this layer owns
                // (hbedit's [R] opens the item picker, which is [A]'s job), so
                // handing the key straight to the state machine here would have
                // swapped the two keys instead of giving [R] its promise.
                Shoulder::Screen => {}
            }
        }

        // [B] on the file list (構成(.arc)) goes back to クエスト改造, not to
        // the arc list: the quest is still the thing being edited and this list
        // is its detail view. The working copies are flushed first, the same
        // way leaving them by their own [B] did.
        //
        // Only the list itself: the screens *inside* it (.mib / .gmd / 検査 /
        // the decoded view) unwind one level, back to the list. The arm used to
        // catch all five, so their own [B] arms - which say exactly that - were
        // dead code, and a press on the .mib screen skipped the list it came
        // from.
        if key == Key::B && ed.arc.is_some() && ed.screen == Screen::ArcEntries {
            safe_flush_quest_entry(ed);
            ed.screen = Screen::QuestEdit;
            ed.cursor = 0;
            ed.quest_edit_cursor = 0;
            ed.message = String::from("クエスト改造へ戻りました");
            return;
        }

        // Global X = QuickSave (except on the pickers, where X clears the
        // search filter). The set editor used to take X for its warnings, which
        // cost the pad its only way to save from that screen; the warnings moved
        // to Y instead.
        // 生データ used to be read-only and was exempt here for that reason:
        // a global save from a screen that has nothing to save is a key that
        // lies. It edits bytes now, so [X] is the save key there like anywhere
        // else - the header pill says 未保存 the moment a byte changes.
        let x_is_the_screens = matches!(
            ed.screen,
            Screen::ItemPick | Screen::SkillPick | Screen::JewelPick | Screen::EquipPick
        );
        if key == Key::X && !x_is_the_screens {
            self.apply_action(ed, UiAction::QuickSave);
            return;
        }

        match ed.screen {
            Screen::SaveList => match key {
                Key::Y => self.apply_action(ed, UiAction::Rescan),
                other => ed.input(other),
            },
            Screen::Slots => match key {
                Key::Up | Key::Left => {
                    let prev = (ed.slot + 2) % 3;
                    self.apply_action(ed, UiAction::SelectSlot(prev));
                }
                Key::Down | Key::Right => {
                    let next = (ed.slot + 1) % 3;
                    self.apply_action(ed, UiAction::SelectSlot(next));
                }
                // The same call the card's button makes, so a blank slot
                // answers with the sentence instead of an empty character
                // screen.
                Key::A => self.apply_action(ed, UiAction::OpenSlot(ed.slot)),
                Key::Y => self.apply_action(ed, UiAction::CharMaxAll),
                Key::B => {
                    ed.screen = Screen::SaveList;
                    ed.cursor = 0;
                }
                _ => {}
            },
            Screen::Char => {
                let (lo, hi) = if ed.field <= 9 { (0usize, 9usize) } else { (10usize, 16usize) };
                match key {
                    Key::Up => {
                        ed.field = if ed.field <= lo { hi } else { ed.field - 1 };
                        ed.cursor = ed.field;
                    }
                    Key::Down => {
                        ed.field = if ed.field >= hi { lo } else { ed.field + 1 };
                        ed.cursor = ed.field;
                    }
                    Key::A => {
                        if ed.field == 0 {
                            ed.input(Key::Right); // Open Name OSK
                        } else if ed.field == 13 {
                            // The 性別 row is the one [A] that is neither a MAX
                            // nor a keyboard. It flipped the byte and said
                            // nothing, which reads as a key that did nothing:
                            // the state machine names the value it wrote.
                            ed.char_gender_flip();
                        } else {
                            self.apply_action(ed, UiAction::SetRowMax(ed.field));
                        }
                    }
                    Key::Y => {
                        ed.step = (ed.step + 1) % STEPS.len();
                        ed.message = format!("刻みを ±{} に切り替えました", STEPS[ed.step]);
                    }
                    other => ed.input(other),
                }
            }
            Screen::Items => match key {
                // While the bulk menu is open the list underneath is frozen:
                // A runs the highlighted row and every other key is the
                // menu's (`-` / B close it, Up / Down move it).
                Key::A if ed.item_menu_open => ed.input(Key::A),
                // [A] is the picker and [R] the count keyboard - the same two
                // jobs the state machine's own arms have (hbedit::input_items),
                // so the pad and the headless build cannot swap them again.
                // [A] used to be sent as [R] because the state machine had them
                // the other way round.
                Key::A => ed.input(Key::A),
                // [Y] is the list's own state, as the footer promised all
                // along: 所持品のみ <-> 空きも表示. It used to be the step here,
                // which left the state machine's toggle unreachable - the one
                // thing this screen's heading can say that nothing else does.
                // The step is the deck's button now (every key on this screen
                // is spoken for: A picks, R types the count, X saves, ZR opens
                // the bulk menu, +/- are tools and quit, L/R are the BOX tabs,
                // ZL/ZR the sidebar, and [-] is the delete key with nothing to
                // delete here).
                Key::Y if ed.item_menu_open => {}
                Key::Y => self.apply_action(ed, UiAction::ToggleItemEmpty),
                // [R] types the count of the selected slot. It used to open the
                // same picker [A] opens, so nothing was lost by giving it to the
                // one number on this screen a user would rather type than step.
                // The state machine answers the same key the same way now, so
                // this is a fall-through rather than a second copy of the rule.
                Key::R => ed.input(Key::R),
                // [-] is the delete key and this screen has nothing to
                // delete. [ZR] opens the bulk menu (hbedit::Editor::
                // open_item_menu), and 元に戻す is one of its rows - the pill
                // on the bar and the deck's own row are the touch routes to the
                // same menu.
                other => ed.input(other),
            },
            Screen::ItemPick => match key {
                Key::A => {
                    let target_box = ed.item_pick_target;
                    let mode = ed.item_pick_mode;
                    ed.input(Key::A);
                    if mode == 0 && ed.screen == Screen::Items {
                        if let Some(pos) = ed.visible_items().iter().position(|&b| b == target_box) {
                            ed.cursor = pos;
                        }
                    }
                    // The pick comes back to the screen that opened the
                    // picker, and the state machine knows which one that is
                    // (hbedit::Editor::item_pick_return). This layer used to
                    // carry a flag for it, which only knew about one of the
                    // four routes and was never cleared when the pick was
                    // cancelled.
                }
                other => ed.input(other),
            },
            Screen::Charms => match key {
                // Both presses of the delete are the state machine's
                // (hbedit::Editor::charm_delete_armed); this layer only moves
                // the cursor, and that goes through as well.
                Key::Up | Key::Down => ed.input(key),
                // [A] opens the selected talisman's form (or creates the first
                // one when the box is empty) and never deletes. [Y] is the
                // list's search - the same key as the equipment box, the
                // quest list and the three pickers - and the state machine
                // turns it into a keyboard request, so it falls through to
                // ed.input below like every other key this layer does not use.
                // [A] opens the selected talisman's form, creates the first one
                // in an empty box, and says "no matches" when a search hides
                // every record. The three answers are the state machine's
                // (hbedit::Editor::charm_a_press): this layer used to ask
                // `charms().is_empty()` itself, and that is the *filtered*
                // list - so a search with no hits started a new talisman.
                Key::A => ed.charm_a_press(),
                // [-] is the one destructive key on every screen, twice: the
                // same key the equipment box, the sets and the palico panels
                // use. Both presses are the state machine's
                // (hbedit::Editor::charm_delete_armed), so a touch button and
                // the pad cannot cost a different number of presses.
                Key::Minus => {
                    ed.input(Key::Minus);
                }
                // [＋] starts a new talisman in the first free slot. It used to
                // call open_charms() first, which put the list cursor back at
                // the top - the equipment box's [＋] has always kept its row,
                // and the two now answer the same way.
                Key::Plus => {
                    ed.input(Key::L);
                }
                other => ed.input(other),
            },
            Screen::CharmEdit => match key {
                // [A] is the row's own action, and the state machine dispatches
                // it from the same value the footer and the inspector button
                // print (hbedit::CharmAction): 種類 / スロット数 cycle, the two
                // skill rows open the name picker, the two point rows step, and
                // the box-slot row writes. [Y] on a skill row is the search
                // route into the same picker, and says where it works on the
                // rows that have no list.
                other => ed.input(other),
            },
            // The palico equipment panel is a mode of these two screens: while
            // it is open every key belongs to it, the same shape the item menu
            // has on the item box.
            Screen::Roster | Screen::Palico if ed.palico_equip_open() => ed.input(key),
            // The skill panel is the second mode of the roster screen and owns
            // every key while it is open, the same shape the equipment panel has
            // (Y is its tab key there, not the roster's field key).
            Screen::Roster if ed.palico_skill_open() => ed.input(key),
            Screen::Roster => match key {
                Key::Y => {
                    ed.roster_field = (ed.roster_field + 1) % 3;
                }
                other => ed.input(other),
            },
            // The language is the state machine's now: ◀ / ▶ fall through to
            // Editor::input_palico, which is the same method the deck's button
            // calls (UiAction::TogglePalicoLang). The console used to keep its
            // own arm here, with its own sentence - a third implementation of
            // one key.
            Screen::Palico => match key {
                Key::Y => {
                    self.apply_action(ed, UiAction::InstallAllPalicoes);
                }
                other => ed.input(other),
            },
            Screen::Hunts => match key {
                Key::Y => {
                    ed.hunt_field = (ed.hunt_field + 1) % 4;
                }
                Key::A => {
                    ed.step = (ed.step + 1) % HUNT_STEPS.len();
                    ed.message = format!(
                        "狩猟記録の刻みを {} に切り替えました",
                        if HUNT_STEPS[ed.step] < 0 {
                            String::from("反転（▶で減る）")
                        } else {
                            format!("±{}", HUNT_STEPS[ed.step])
                        }
                    );
                }
                Key::R => {
                    ed.request_hunt_text();
                }
                other => ed.input(other),
            },
            // The guild screen's views own every key: on the arena log [Y] is
            // the direct time input, and on the other three it walks the
            // card's record views. The step is the deck's pill on this screen
            // now (it used to be [Y], which the view walk needs).
            Screen::Guild => ed.input(key),
            // The set editor is a mode of this screen: while it is open every
            // key belongs to it and the list underneath cannot move, the same
            // shape the item bulk menu uses. The list's own cursor lives in
            // this struct, so A is what hands the selected record to the state
            // machine.
            // Either set form owns every key while it is open: the equipment
            // one for its parts, the item one for its thirty-two entries.
            Screen::Sets if ed.set_editor_open() => match key {
                // [R] is ポーチへ適用 on the item set form: the footer names it
                // and hbedit::Editor::shoulder_job hands the key to the screen
                // (Shoulder::Screen), so this list is the only thing that was
                // dropping it - the one key the footer promised and the pad
                // could not press. The equipment form has no [R] arm, so the
                // key is a no-op there, exactly as every other unclaimed key.
                Key::Up | Key::Down | Key::Left | Key::Right | Key::A | Key::Y | Key::Minus
                | Key::R | Key::Z | Key::B => ed.input(key),
                _ => {},
            },
            Screen::Sets => match key {
                Key::Up => {
                    self.sets_cursor = self.sets_cursor.saturating_sub(1);
                }
                Key::Down => {
                    let len = sets_len(ed, self.sets_tab);
                    if len > 0 {
                        self.sets_cursor = (self.sets_cursor + 1).min(len - 1);
                    }
                }
                Key::A => {
                    // Both tabs have a form now, and each takes the record its
                    // own list is showing: the two arrays are unrelated.
                    if self.sets_tab == 0 {
                        if let Some(set) = item_set_at(ed, self.sets_cursor) {
                            ed.open_item_set_editor(set);
                        } else {
                            ed.message = String::from("編集できるアイテムセットがありません");
                        }
                    } else if let Some(set) = equip_set_at(ed, self.sets_cursor) {
                        ed.open_set_editor(set);
                    } else {
                        ed.message = String::from("編集できる装備セットがありません");
                    }
                }
                Key::Left | Key::Right | Key::Y => {
                    self.sets_tab = 1 - self.sets_tab;
                    self.sets_cursor = 0;
                }
                // [B] leaves for the talisman list, which is the destination
                // the footer names. The state machine answers the same way
                // (Editor::input_sets), so the two layers cannot disagree.
                Key::B => ed.input(Key::B),
                _ => {},
            },
            // The CSV screen is a folder list: A applies a file (twice, since
            // an apply replaces a box), Y writes both boxes out and Minus reads
            // the folder again.
            Screen::Csv => match key {
                Key::Up | Key::Down | Key::A | Key::Y | Key::Z | Key::B => {
                    ed.input(key);
                }
                _ => {}
            },
            // The three list tabs are one state machine now (hbedit::Editor::
            // quest_tab / quest_list_move / bundled_quest_press): the tab on
            // screen owns the cursor, so a row cannot be selected, deleted or
            // written out of a list that is not showing. This arm used to keep
            // its own copy of the tab and the two bundle cursors, and answered
            // for the save's list whatever tab was lit.
            Screen::SaveQuests => {
                ed.input(key);
                // Opening a quest from the save is the same promise the
                // card's quest list makes: クエスト改造, not the file list.
                // The state machine lands there itself now
                // (Editor::open_quest_from_save); this only pins the return
                // target, so [B] comes back to the list the quest was
                // opened from.
                if ed.quest_tab == 0
                    && matches!(key, Key::A)
                    && ed.screen == Screen::QuestEdit
                    && ed.arc.is_some()
                {
                    ed.enter_quest_edit_from(Screen::SaveQuests);
                }
            }
            Screen::ArcList => match key {
                // Through the action, so the pad and the deck's button run the
                // same two-press install (hbedit::Editor::arc_install_press).
                Key::Y => self.apply_action(ed, UiAction::InstallArcRow(ed.cursor)),
                // [ZR] writes the bundled quests out. It used to be [-],
                // which is the delete key.
                Key::Z => {
                    self.wants_extract_official_arcs = true;
                }
                // [B] leaves the quest section for the save list. It used to go
                // to セーブ内, whose [B] comes back here: one key, two screens,
                // neither ever leaving. The state machine answers the same way
                // (Editor::input_arc_list), so the footer's "[B] セーブ一覧" is
                // true of both layers.
                Key::B => ed.input(Key::B),
                other => ed.input(other),
            },
            Screen::ArcEntries => match key {
                Key::A => {
                    safe_flush_quest_entry(ed);
                    let ext = ed
                        .arc
                        .as_ref()
                        .and_then(|a| a.entries.get(ed.cursor))
                        .and_then(|e| extension_for(e.type_hash));
                    if matches!(ext, Some("mib") | Some("gmd")) {
                        if ext == Some("mib") {
                            ed.gmd = None;
                        }
                        ed.input(Key::A);
                    } else {
                        ed.gmd = None;
                        ed.input(Key::Y);
                    }
                }
                Key::Y => {
                    safe_flush_quest_entry(ed);
                    ed.gmd = None;
                    ed.run_check();
                }
                other => ed.input(other),
            },
            // クエスト改造: [A] is the row's own chooser and every other key is
            // the state machine's (it owns the cursor, the values, [-] and
            // [Y]). 詳細 is the last row of every tab and the chip on the bar:
            // ZR belongs to the sidebar, so it cannot be the pad's way in.
            // [Z] is still answered here for a controller that sends it.
            Screen::QuestEdit => match key {
                Key::A => {
                    match ed.quest_activate() {
                        QuestPick::Monster(field) => {
                            if !self.open_mib_picker(ed, field) {
                                ed.message = String::from("モンスターの一覧を作れません");
                            }
                        }
                        QuestPick::Area(slot) => {
                            self.open_quest_area_picker(ed, slot);
                        }
                        QuestPick::None => {}
                    }
                    // A reward / supply row opens the item chooser, and the
                    // pick comes back here: the state machine records that when
                    // it opens the picker (Editor::item_pick_return).
                }
                // [ZR] is the tab's own value switch now (the state
                // machine's), so the file screens keep their two other doors:
                // the [ファイル] pill on the bar and [R 長押し].
                other => ed.input(other),
            },
            // The equipment form: [A] on the 種別 row asks for the kind list,
            // which the state machine cannot open. The flag it sets is read on
            // the frame the press lands, so the row's own action is still the
            // one thing that decides what [A] means.
            Screen::EquipEdit => {
                ed.input(key);
                if ed.equip_kind_pick_pending {
                    ed.equip_kind_pick_pending = false;
                    self.open_equip_kind_picker(ed);
                }
            }
            Screen::Mib => {
                let is_mob = questfields::MIB_FIELDS
                    .get(ed.mib_cursor)
                    .map(|f| f.key.starts_with("mob_slot["))
                    .unwrap_or(false);
                match key {
                    Key::A => {
                        let idx = ed.mib_cursor;
                        self.open_mib_row_picker(ed, idx);
                    }
                    // [ZR] walks the eight values of a monster slot. It used
                    // to be [-], which is the delete key.
                    Key::Z if is_mob => {
                        self.mib_mob_sub = (self.mib_mob_sub + 1) % 8;
                        ed.message = format!(
                            "モンスター編集項目を [{}] に切り替えました ([◀/▶]で増減)",
                            MIB_MOB_SUB_LABELS[self.mib_mob_sub]
                        );
                    }
                    Key::Left | Key::Right if is_mob => {
                        let sign = if key == Key::Right { 1 } else { -1 };
                        adjust_mib_mob_sub(ed, ed.mib_cursor, self.mib_mob_sub, sign * (STEPS[ed.step] as i32));
                    }
                    Key::Y => {
                        ed.step = (ed.step + 1) % STEPS.len();
                        ed.message = format!("刻みを ±{} に切り替えました", STEPS[ed.step]);
                    }
                    Key::B => {
                        safe_flush_quest_entry(ed);
                        ed.screen = Screen::ArcEntries;
                    }
                    other => ed.input(other),
                }
            }
            Screen::Gmd => match key {
                Key::B => {
                    safe_flush_quest_entry(ed);
                    ed.gmd = None;
                    ed.screen = Screen::ArcEntries;
                }
                other => ed.input(other),
            },
            // Y is the only key this screen leaves free: ◀/▶ change a value,
            // A/X write the save, B goes back. It puts the newest snapshot
            // within one press instead of a new screen.
            Screen::Options => match key {
                Key::Y => self.wants_restore_snapshot = true,
                other => ed.input(other),
            },
            Screen::EntryView => {
                let ext = ed
                    .arc
                    .as_ref()
                    .and_then(|a| a.entries.get(ed.entry))
                    .and_then(|e| extension_for(e.type_hash));
                let len = list_len_of(ed, self);
                match key {
                    Key::Up => {
                        if len > 0 {
                            ed.entry_row_cursor = (ed.entry_row_cursor + len - 1) % len;
                        }
                    }
                    Key::Down => {
                        if len > 0 {
                            ed.entry_row_cursor = (ed.entry_row_cursor + 1) % len;
                        }
                    }
                    // The step table is the screen's own (hbedit::step_table):
                    // a reward / supply row holds a count and a chance, so its
                    // table stops at ±100 and the pills draw exactly that. The
                    // press used to cycle the shared five and then step by a
                    // number the writer clamped to 10.
                    Key::Y => {
                        let table = ed.step_table().0;
                        let n = table.len().max(1);
                        ed.step = (ed.step + 1) % n;
                        ed.message = format!(
                            "刻みを ±{} に切り替えました",
                            table[ed.step.min(n - 1)]
                        );
                    }
                    Key::Left | Key::Right
                        if matches!(ext, Some("sem") | Some("esl") | Some("qdp")) =>
                    {
                        let sign = if key == Key::Right { 1 } else { -1 };
                        adjust_entry_view_row(ed, ed.entry_row_cursor, sign * (STEPS[ed.step] as i32));
                    }
                    Key::A if matches!(ext, Some("sem") | Some("esl") | Some("qdp")) => {
                        adjust_entry_view_row(ed, ed.entry_row_cursor, STEPS[ed.step] as i32);
                    }
                    other => ed.input(other),
                }
            }
            _ => ed.input(key),
        }
    }

    // -----------------------------------------------------------------------
    // Touch Screen Handler
    // -----------------------------------------------------------------------
    pub fn handle_touch(&mut self, ed: &mut Editor, touching: bool, tx: i32, ty: i32) -> bool {
        // The save review is modal: while it is open only its two buttons
        // answer a tap, never the list or the bar underneath. A gesture that
        // was running when it opened is dropped rather than paused, so the
        // list cannot coast on under a dialog.
        if ed.review_open() {
            self.cancel_gestures();
            if touching && !self.was_touching {
                let hit = self
                    .hits
                    .iter()
                    .rev()
                    .find(|h| {
                        h.contains(tx, ty)
                            && matches!(
                                h.action,
                                UiAction::CommitReview | UiAction::CancelReview
                            )
                    })
                    .copied();
                if let Some(h) = hit {
                    self.apply_action(ed, h.action);
                }
            }
            self.was_touching = touching;
            return true;
        }

        // The update dialog is modal in exactly the way the review is: only its
        // own three answers hear a tap, and a gesture that was running when it
        // opened is dropped rather than paused.
        if self.update.dialog {
            self.cancel_gestures();
            if touching && !self.was_touching {
                let hit = self
                    .hits
                    .iter()
                    .rev()
                    .find(|h| {
                        h.contains(tx, ty)
                            && matches!(
                                h.action,
                                UiAction::UpdatePrimary
                                    | UiAction::UpdateLater
                                    | UiAction::UpdateToggle
                                    | UiAction::ModalBlank
                            )
                    })
                    .copied();
                if let Some(h) = hit {
                    self.apply_action(ed, h.action);
                }
            }
            self.was_touching = touching;
            return true;
        }

        let mut changed = false;

        if touching && !self.was_touching {
            // A new touch cancels the coast: the finger has taken over.
            self.fling = 0;
            self.drag_rows = 0;
            self.drag_start_y = ty;
            self.last_drag_y = ty;
            self.dragged = false;
            self.pending_activate_row = None;

            // A finger that lands on the scrollbar drives the bar, and nothing
            // else answers: a drag on the thumb must not also tick the row
            // that happens to be under it. The keyboard is modal, so its band
            // is exempt.
            if self.osk.is_none() && self.begin_bar_drag(ed, tx, ty) {
                self.dragged = true;
                self.was_touching = touching;
                return true;
            }

            let hit = self.hits.iter().rev().find(|h| h.contains(tx, ty)).copied();
            if let Some(h) = hit {
                if h.is_row_body {
                    if let UiAction::SelectRow(idx) = h.action {
                        if ed.screen == Screen::SaveQuests && ed.quest_tab > 0 {
                            if ed.quest_list_cursor() == idx {
                                self.pending_activate_row = Some(idx);
                            } else {
                                ed.set_quest_list_cursor(idx);
                                changed = true;
                            }
                        } else if active_cursor_of(ed, self) == idx {
                            self.pending_activate_row = Some(idx);
                        } else {
                            set_active_cursor(ed, self, idx);
                            changed = true;
                        }
                    }
                } else {
                    self.apply_action(ed, h.action);
                    changed = true;
                }
            }
        } else if touching && self.was_touching {
            if self.bar_drag.is_some() {
                // The thumb follows the finger, the window follows the thumb,
                // and the cursor keeps the place it held inside the window.
                if self.drag_bar(ed, ty) {
                    changed = true;
                }
            } else if self.mib_picker.is_some() {
                let dy = ty - self.last_drag_y;
                if (ty - self.drag_start_y).abs() > TAP_SLOP_PX {
                    self.dragged = true;
                    self.pending_activate_row = None;
                }
                // The speed a flick starts at is what the finger did on the
                // frame it left, so a finger that stopped before lifting does
                // not throw the list onwards.
                self.drag_rows = 0;
                if dy.abs() >= MIB_PICKER_PITCH {
                    let rows = -dy / MIB_PICKER_PITCH;
                    self.last_drag_y += -rows * MIB_PICKER_PITCH;
                    self.scroll_mib_picker(rows);
                    self.drag_rows = rows;
                    changed = true;
                }
            // The keyboard is modal too: a finger dragged over the band it
            // leaves open must not page the list underneath.
            } else if self.osk.is_none() && ty >= CONTENT_Y && ty < FOOTER_Y {
                let dy = ty - self.last_drag_y;
                if (ty - self.drag_start_y).abs() > TAP_SLOP_PX {
                    self.dragged = true;
                    self.pending_activate_row = None;
                }
                // One row of travel per row drawn: the rows move with the
                // finger instead of running away from it. A frame with no
                // travel is not a speed: a finger that stopped before it lifted
                // must not throw the list onwards.
                self.drag_rows = 0;
                if dy.abs() >= DRAG_ROW_PX {
                    let rows = -dy / DRAG_ROW_PX;
                    self.last_drag_y += -rows * DRAG_ROW_PX;
                    self.page_scroll(ed, rows);
                    self.drag_rows = rows;
                    changed = true;
                }
            }
        } else if !touching && self.was_touching {
            if let Some(drag) = self.bar_drag.take() {
                // A tap that never became a drag is a page: it is the one thing
                // a scrollbar does when it is not being held, and it gives the
                // bar a use that cannot be mistaken for a row tap.
                if !drag.moved && !drag.on_thumb {
                    self.page_by(ed, if drag.above { -1 } else { 1 });
                    changed = true;
                }
                // Letting go of the thumb ends the gesture where it is: no row
                // is ticked and nothing coasts onwards, because the finger was
                // holding the view and not flicking it.
                self.dragged = true;
            } else if !self.dragged {
                if let Some(idx) = self.pending_activate_row.take() {
                    self.apply_action(ed, UiAction::ActivateRow(idx));
                    changed = true;
                }
            } else if self.osk.is_none() && self.mib_picker.is_none() {
                // The flick: the list keeps the speed the finger left with and
                // gives it back a quarter a frame (tick_fling).
                self.fling = self.drag_rows.clamp(-6, 6) * FLING_UNIT;
            }
            self.pending_activate_row = None;
        } else if self.tick_fling(ed) {
            // No finger on the glass: the list coasts to a stop by itself.
            changed = true;
        }

        self.was_touching = touching;
        changed
    }

    // -----------------------------------------------------------------------
    // Action Execution & One-Tap Bulk Cheat Implementations
    // -----------------------------------------------------------------------
    fn apply_action(&mut self, ed: &mut Editor, action: UiAction) {
        // Any other input takes back the offer a two-press action made: the
        // second press has to be the next thing the user does, not something
        // they come back to a minute later.
        if !matches!(
            action,
            UiAction::Undo
                | UiAction::RosterMaxAll
                | UiAction::CommitReview
                | UiAction::CancelReview
                | UiAction::SlotBadge(_)
                // The bulk buttons keep their own offer: the state machine
                // carries which one was made (hbedit::BulkAction), so a press
                // can only spend the offer of the button that made it.
                | UiAction::CharMaxAll
                | UiAction::SetRowMax(_)
                | UiAction::InstallAllPalicoes
                | UiAction::InstallArcRow(_)
                // The 終了 pill is the second press of its own offer: taking
                // the offer back here would make the pill ask forever.
                | UiAction::Quit
        ) {
            self.undo_press.clear();
            self.roster_max.clear();
            ed.clear_bulk_press();
            // A slot switch offered once is offered for the next tap only: a
            // card press or any other action takes the offer back, so a badge
            // tap a minute later cannot switch the hunter on its own.
            self.slot_press.clear();
            // The same rule for the [＋] quit offer: the pad drops it on every
            // key that is not [＋] (hbedit::Editor::input), and a touch action
            // is a key - so a tap on a deck button or a tab takes the offer
            // back too. It used to survive, and a 終了 tapped once then re-tapped
            // a minute later quit without asking again.
            ed.clear_quit_arm();
        }
        match action {
            UiAction::Key(k) => self.handle_pad_key(ed, k, 0),
            // A tap outside a modal panel: it is here to stop, and stopping is
            // all it does.
            UiAction::ModalBlank => {}
            // The four answers the update layer takes. Each one is a method on
            // crate::update::UpdateState, so the pad and a finger run the same
            // code and neither can leave the updater in a state the other
            // cannot reach.
            UiAction::UpdateOpen => self.update.open_dialog(),
            UiAction::UpdatePrimary => self.update.primary(),
            UiAction::UpdateLater => self.update.dismiss(),
            UiAction::UpdateToggle => self.update.toggle_setting(),
            // The way out for a finger. It goes through the same two-press
            // quit as [＋] (hbedit::Editor::press_quit), so a dirty save is
            // never discarded by one tap either.
            UiAction::Quit => {
                ed.press_quit();
            }
            UiAction::FilterItems => {
                ed.request_item_box_filter();
            }
            UiAction::FilterCharms => {
                ed.request_charm_filter();
            }
            UiAction::FilterProgress => {
                ed.request_progress_filter();
            }
            // The quest view's second bit: [Y] is the tab key on all three
            // views now, so the write is this button.
            UiAction::ProgressSeen => ed.progress_seen_press(),
            // 報酬 / 支給品: the item-name search. [ZR]'s third stop runs the
            // same request, and [-] could not take it (the delete key).
            UiAction::FilterQuestItems => ed.request_quest_item_filter(),
            UiAction::SlotBadge(s) => {
                if s < 3 && s != ed.slot {
                    if self.slot_press.press() {
                        // The second tap: switch, through the same path a card
                        // press takes, so the cursor and the message agree.
                        self.apply_action(ed, UiAction::SelectSlot(s));
                    } else {
                        ed.message = format!(
                            "もう一度タップすると Slot {} に切り替えます（今は Slot {}）",
                            s + 1,
                            ed.slot + 1
                        );
                    }
                }
            }
            // The slot card, the header badge and the deck's slot button all land
            // here, and the change itself is the state machine's
            // (hbedit::Editor::switch_slot): it is the one place that knows which
            // forms hold a record of the slot that is going away. This used to
            // re-point `ed.slot` under an open form, so the form's next [A] wrote
            // the other character's same frame.
            UiAction::SelectSlot(s) => {
                if ed.switch_slot(s) {
                    // The modal this layer owns is a mode of the screen the
                    // switch just left (and its three writer flags name records
                    // of the old slot), and the keyboard's request names a
                    // record of the old slot: none of them survives it.
                    self.close_picker_modal();
                    self.osk = None;
                    self.osk_result = None;
                }
            }
            // A slot card's button: the slot is the card's, and an empty one
            // says so instead of opening a screen of zeroes.
            UiAction::OpenSlot(s) => {
                if ed.open_slot(s) {
                    self.close_picker_modal();
                    self.osk = None;
                    self.osk_result = None;
                }
            }
            UiAction::ItemSetToPouch => ed.item_set_to_pouch(),
            UiAction::GotoSection(sec) => self.goto_section(ed, sec),
            UiAction::SelectSubTab(idx) => self.select_sub_tab(ed, idx),
            UiAction::CycleSection(fwd) => self.cycle_sidebar_tab(ed, fwd),
            UiAction::CycleSubTab(fwd) => self.cycle_sub_tab(ed, fwd),
            UiAction::SetStep(s) => {
                if s < STEPS.len() {
                    ed.step = s;
                    ed.message = format!("刻みを ±{} に設定しました", STEPS[s]);
                }
            }
            UiAction::SelectRow(idx) => {
                set_active_cursor(ed, self, idx);
            }
            // A tap on one time cell moves the cursor and nothing else: the
            // edit still starts with [A], so a tap can never write a time.
            UiAction::ArenaCell(row, rank) => {
                ed.arena_row = row.min(ARENA_ROWS.saturating_sub(1));
                ed.arena_rank = rank.min(ARENA_COLUMNS.saturating_sub(1));
                ed.arena_edit = false;
            }
            UiAction::GuildWeaponCell(row, venue) => {
                ed.guild_wpn_row = row.min(mhnx_core::hbedit::GUILD_WEAPON_ROWS.saturating_sub(1));
                ed.guild_wpn_venue = venue.min(mhnx_core::hbedit::GUILD_VENUES.saturating_sub(1));
                // A tap on another cell is a cursor move: the step editor
                // belongs to the cell it was armed on.
                ed.guild_wpn_edit = false;
            }
            UiAction::ActivateRow(idx) => {
                set_active_cursor(ed, self, idx);
                // The tap and the pad press the same key: the state machine
                // answers a row of any of the three list tabs (Editor::
                // input_save_quests), so a tap cannot install a quest the pad
                // would refuse.
                self.handle_pad_key(ed, Key::A, 0);
            }
            UiAction::AdjustRow(idx, key) => {
                set_active_cursor(ed, self, idx);
                self.handle_pad_key(ed, key, 0);
            }
            UiAction::SetRowMax(idx) => {
                set_active_cursor(ed, self, idx);
                // The character screen's [A] is the one MAX that writes without
                // the row being a number under a stepper: it asks first, and
                // the row is part of the offer, so moving the cursor takes it
                // back (hbedit::Editor::char_row_max_press).
                if ed.screen == Screen::Char {
                    ed.char_row_max_press(idx);
                } else {
                    apply_row_max(ed, self.mib_mob_sub, idx);
                }
            }
            UiAction::SelectSubField(row_idx, sub) => {
                set_active_cursor(ed, self, row_idx);
                match ed.screen {
                    Screen::Hunts => ed.hunt_field = sub.min(3),
                    Screen::Roster => ed.roster_field = sub.min(2),
                    Screen::Guild => {
                        ed.guild_field = sub.min(mhnx_core::hbedit::GUILD_FIELDS - 1);
                        // A tap on a text row opens its keyboard; the number
                        // rows take a step, exactly what [A] does with the pad.
                        if sub >= mhnx_core::hbedit::GUILD_ROW_COMMENT {
                            ed.input(Key::A);
                        }
                    }
                    Screen::Mib => {
                        self.mib_mob_sub = sub.min(7);
                        ed.message = format!(
                            "モンスター編集項目を [{}] に切り替えました ([◀/▶]で増減)",
                            MIB_MOB_SUB_LABELS[self.mib_mob_sub]
                        );
                    }
                    _ => {}
                }
            }
            UiAction::Rescan => self.wants_rescan = true,
            UiAction::OpenHunts => ed.open_hunts(),
            // The save itself lives in the state machine
            // (hbedit::Editor::quick_save): it was the console's copy of that
            // rule that ran the row's own [A] on the two forms, so [X] opened
            // a picker instead of writing.
            UiAction::QuickSave => ed.quick_save(),
            // The whole-save undo, reachable from the footer pill as well as
            // from holding L on the pad. Both ask twice: undo_last pops the
            // step, so a tap that was not meant as one cannot be taken back
            // afterwards.
            UiAction::ReviewScroll(delta) => self.scroll_review(ed, delta),
            UiAction::Undo => {
                if ed.undo_depth() == 0 {
                    self.undo_press.clear();
                    ed.message = String::from("戻せる操作がありません");
                } else if self.undo_press.press() {
                    ed.undo_last();
                } else {
                    ed.message = format!(
                        "もう一度タップで元に戻します（取り消す操作: {}）",
                        ed.undo_label().unwrap_or("-")
                    );
                }
            }
            // The review dialog's own two answers. A restore is on the second
            // press of [A] the same way a save write is.
            UiAction::CommitReview => crate::editor::finish_reviewed_write(ed, self),
            UiAction::CancelReview => {
                ed.cancel_save_review();
                self.pending_snapshot = None;
            }
            // One press is one undo step, so the bulk MAX goes through the
            // editor instead of calling the field writer eleven times - and it
            // asks first now: one press used to write eleven fields with
            // nothing on screen saying so (hbedit::Editor::char_max_all_press).
            UiAction::CharMaxAll => {
                ed.char_max_all_press();
            }
            // Both bulk buttons run the same rows the menu runs: a touch and
            // the pad must never take two different code paths.
            UiAction::OpenItemMenu => ed.open_item_menu(),
            UiAction::ItemOp(op) => {
                ed.open_item_menu();
                ed.item_menu_cursor = op.min(ITEM_OPS.len() - 1);
                ed.item_armed = None;
                ed.run_item_op(ed.item_menu_cursor);
            }
            // The talisman screen has no bulk action and no preset. The
            // 合法最大値 button and the four 痛撃/斬れ味/溜め短縮/通常 presets
            // wrote a whole record in one press, which is exactly the shape the
            // per-kind ceiling exists to prevent; the form edits one field at a
            // time and [A] acts on the row the cursor is on instead.
            // The same press the pad makes, and nothing else: open_charms()
            // here put the list back at the top, so the touch button and the
            // [＋] key did the same job with two different outcomes. The pill
            // is only drawn on the talisman list, so the screen is already
            // right (hbedit::Editor::open_charm_editor keeps the row).
            UiAction::NewCharm => ed.input(Key::L),
            // The touch path and the pad press the same key: one delete rule,
            // one place that counts the presses.
            UiAction::DeleteCharmConfirm => ed.input(Key::Minus),
            // One tap used to write every palico with no undo step behind it.
            // It asks twice now, and the write itself is one undo step
            // (Editor::maximize_all_palicos), so the bulk MAX can be taken back.
            UiAction::RosterMaxAll => {
                let n = ed.palicos().len();
                if n == 0 {
                    self.roster_max.clear();
                    ed.message = String::from("オトモがいません（オトモを雇ってから実行してください）");
                } else if self.roster_max.press() {
                    ed.maximize_all_palicos();
                } else {
                    ed.message = format!(
                        "オトモ {} 匹を Lv99・なつき100・EXP最大にします。もう一度タップで実行",
                        n
                    );
                }
            }
            UiAction::InstallDlcCategory(cat) => {
                // The label is what the message says afterwards, so it carries
                // the count the bundle really has.
                let (official_n, dead_n) = mhnx_core::dlc::bundled_quest_counts();
                if cat == 1 {
                    ed.install_dlc_category(false, true, &format!("没クエスト({}件)", dead_n));
                } else {
                    ed.install_dlc_category(true, false, &format!("公式イベント({}件)", official_n));
                }
            }
            UiAction::OpenDetail => self.open_detail(ed),
            UiAction::MibPickerSelect(idx) => {
                if let Some(mut picker) = self.mib_picker.take() {
                    if picker.cursor == idx {
                        if self.equip_kind_picker {
                            self.equip_kind_picker = false;
                            if let Some((raw_val, _)) = picker.items.get(idx).cloned() {
                                ed.apply_equip_kind(raw_val as u8);
                            }
                            return;
                        }
                        if let Some((raw_val, label)) = picker.items.get(idx).cloned() {
                            // The area chooser is the same modal with a
                            // different writer. The tap used to go through the
                            // .mib field writer with the "no field" index, so it
                            // wrote nothing at all while the pad's [A] wrote the
                            // .sem - and the slot stayed armed, so the next
                            // monster pick wrote the area instead of the monster.
                            let target = match self.quest_area_slot.take() {
                                Some(slot) => mhnx_core::hbedit::MibPickTarget::Area(slot),
                                None => mhnx_core::hbedit::MibPickTarget::Field(picker.field_idx),
                            };
                            ed.apply_mib_pick(target, raw_val, &label);
                        }
                    } else {
                        picker.cursor = idx.min(picker.items.len().saturating_sub(1));
                        self.mib_picker = Some(picker);
                    }
                }
            }
            // Closing the chooser disarms it: an area slot left behind is what
            // made the next monster pick write the .sem, and a 種別 flag left
            // behind made the next list's [A] write a kind instead of the row.
            UiAction::MibPickerClose => {
                self.close_picker_modal();
            }
            // Both presets rewrite every monster / spawn of the record at
            // once, so they ask for a second press and say how many they would
            // touch (hbedit::Editor::mib_preset_label / sem_preset_label).
            UiAction::MibPreset(preset) => {
                ed.mib_preset_press(preset);
            }
            UiAction::SemPreset(preset) => {
                ed.sem_preset_press(preset);
            }
            // The whole downloadable roster of the language, on the second
            // press: the same key on the screen beside it only walks a column.
            UiAction::InstallAllPalicoes => {
                ed.install_all_palicoes_press();
            }
            // The one implementation is the state machine's: the pad's ◀ / ▶
            // reach it through Editor::input_palico, and this button calls the
            // same method, so the two cannot end up in different states or say
            // different sentences.
            UiAction::TogglePalicoLang => ed.toggle_palico_lang(),
            UiAction::ExtractOfficialArcs => self.wants_extract_official_arcs = true,
            UiAction::ExtractSelectedQuest => ed.request_extract_selected_quest(),
            UiAction::ExtractAllQuests => ed.request_extract_all_quests(),
            UiAction::ToggleHuntShowAll => ed.toggle_hunt_show_all(),
            UiAction::ClearEquipFilter => ed.apply_equip_filter(String::new()),
            UiAction::ToggleItemEmpty => ed.toggle_item_show_empty(),
            // Installing a file writes a quest record into the save, so the
            // first press only names the file and the slot
            // (hbedit::Editor::arc_install_press).
            UiAction::InstallArcRow(idx) => {
                ed.cursor = idx;
                if ed.arc_install_press(idx) {
                    self.wants_install_arc_idx = Some(idx);
                }
            }
            UiAction::OskTab(t) => {
                if let Some(osk) = &mut self.osk {
                    osk.tab = t.min(3);
                }
            }
            UiAction::OskCell(r, c) => {
                if let Some(osk) = &mut self.osk {
                    osk.row = r.min(4);
                    osk.col = c.min(9);
                    osk_press_cell(osk, r, c);
                }
            }
            UiAction::OskBackspace => {
                if let Some(osk) = &mut self.osk {
                    let _ = osk.buffer.pop();
                }
            }
            // The same two presses the pad's [X] takes: a finger and a key must
            // not cost a different number of presses for the same delete.
            UiAction::OskClear => {
                if let Some(osk) = &mut self.osk {
                    if osk.buffer.is_empty() {
                        osk.clear.clear();
                    } else if osk.clear.press() {
                        osk.buffer.clear();
                    }
                }
            }
            UiAction::OskModify => {
                if let Some(osk) = &mut self.osk {
                    osk_cycle_last_char(&mut osk.buffer);
                }
            }
            UiAction::OskSpace => {
                if let Some(osk) = &mut self.osk {
                    osk_push(osk, " ");
                }
            }
            UiAction::OskNewline => {
                if let Some(osk) = &mut self.osk {
                    osk_newline(osk);
                }
            }
            UiAction::OskConfirm => {
                if let Some(osk) = self.osk.take() {
                    self.osk_result = Some((osk.req_idx, osk.buffer));
                }
            }
            UiAction::OskCancel => {
                self.osk = None;
                self.osk_cancelled = true;
            }
        }
    }
}

const MIB_MOB_SUB_LABELS: [&str; 8] = [
    "体力(HP)",
    "攻撃力",
    "防御力",
    "サイズ%",
    "出現数",
    "スタミナ",
    "変動幅",
    "獰猛/特殊",
];

fn safe_flush_quest_entry(ed: &mut Editor) {
    let ext = ed
        .arc
        .as_ref()
        .and_then(|a| a.entries.get(ed.entry))
        .and_then(|e| extension_for(e.type_hash));
    match ext {
        Some("mib") => {
            if !ed.mib_bytes.is_empty() {
                if let Some(arc) = &mut ed.arc {
                    if let Some(e) = arc.entries.get_mut(ed.entry) {
                        e.data = ed.mib_bytes.clone();
                    }
                }
            }
        }
        Some("gmd") => {
            if let Some(g) = &ed.gmd {
                if let Some(arc) = &mut ed.arc {
                    if let Some(e) = arc.entries.get_mut(ed.entry) {
                        e.data = g.to_bytes();
                    }
                }
            }
        }
        _ => {}
    }
}

fn adjust_mib_mob_sub(ed: &mut Editor, field_idx: usize, sub: usize, delta: i32) {
    // One implementation, in the state machine. The aggregate (クエスト改造)
    // screen steps the same values through the same writer, so the .mib screen
    // and that screen cannot drift apart - and the labels, the offsets and the
    // clamps live in one place (hbedit::Editor::adjust_mib_mob).
    ed.adjust_mib_mob(field_idx, sub, delta);
}

fn adjust_entry_view_row(ed: &mut Editor, row_idx: usize, delta: i32) {
    let ext = ed
        .arc
        .as_ref()
        .and_then(|a| a.entries.get(ed.entry))
        .and_then(|e| extension_for(e.type_hash));
    match ext {
        Some("sem") => {
            // The six values of one placement are written by the state machine
            // (hbedit::Editor::adjust_sem_placement), which is also what the
            // aggregate (クエスト改造) screen steps. Same offsets, same clamps,
            // same messages - one implementation.
            let sem_k = row_idx / 6;
            let field = row_idx % 6;
            ed.adjust_sem_placement(sem_k, field, delta);
        }
        Some("esl") => {
            if let Some(arc) = &mut ed.arc {
                if let Some(e) = arc.entries.get_mut(ed.entry) {
                    if e.data.len() >= 8 {
                        let cur = u32::from_le_bytes([e.data[4], e.data[5], e.data[6], e.data[7]]) as i32;
                        let step_w = if delta >= 0 { 1 } else { -1 };
                        let next = (cur + step_w).clamp(0, 99) as u32;
                        e.data[4..8].copy_from_slice(&next.to_le_bytes());
                        ed.dirty = true;
                        ed.message = format!("小型モンスター Wave数を {} に変更しました", next);
                    }
                }
            }
        }
        Some("qdp") => {
            if let Some(arc) = &mut ed.arc {
                if let Some(e) = arc.entries.get_mut(ed.entry) {
                    let off = 8 + row_idx;
                    if off < e.data.len() {
                        let cur = e.data[off] as i32;
                        let next = (cur + delta).clamp(0, 255) as u8;
                        e.data[off] = next;
                        ed.dirty = true;
                        ed.message = format!("QDP Flag +0x{:02X} を 0x{:02X} に変更しました", off, next);
                    }
                }
            }
        }
        _ => {}
    }
}

/// The equipment set array index of one row of the my-sets list.
///
/// The list the console draws skips the empty records, so its row number is
/// not the record index the state machine needs.
fn equip_set_at(ed: &Editor, row: usize) -> Option<usize> {
    ed.equip_sets()
        .into_iter()
        .filter(|s| !s.is_empty())
        .nth(row)
        .map(|s| s.index)
}

/// The item set array index of one row of the my-sets item list.
///
/// The list skips the empty records, so its row number is not the record index
/// the form needs.
fn item_set_at(ed: &Editor, row: usize) -> Option<usize> {
    ed.item_sets()
        .into_iter()
        .filter(|s| !s.is_empty())
        .nth(row)
        .map(|s| s.index)
}

fn sets_len(ed: &Editor, tab: usize) -> usize {
    let Some(save) = &ed.save else { return 0 };
    if tab == 0 {
        mhnx_core::myset::item_sets(save, ed.slot)
            .into_iter()
            .filter(|s| !s.is_empty())
            .count()
    } else {
        mhnx_core::myset::equip_sets(save, ed.slot)
            .into_iter()
            .filter(|s| !s.is_empty())
            .count()
    }
}

fn active_cursor_of(ed: &Editor, gui: &GuiState) -> usize {
    match ed.screen {
        Screen::Slots => ed.slot,
        Screen::Char => ed.field,
        Screen::CharmEdit => ed.charm_edit.map(|c| c.field).unwrap_or(0),
        Screen::Items => {
            if ed.item_menu_open {
                ed.item_menu_cursor
            } else {
                ed.cursor
            }
        }
        Screen::ItemPick => ed.item_pick_cursor,
        Screen::SkillPick => ed.skill_pick_cursor,
        Screen::Equip => ed.equip_cursor,
        Screen::EquipEdit => ed.equip_edit.map(|e| e.field).unwrap_or(0),
        Screen::JewelPick => ed.jewel_pick_cursor,
        Screen::EquipPick => ed.equip_pick_cursor,
        Screen::Hunts => ed.hunt_cursor,
        Screen::Guild => match ed.guild_view() {
            GUILD_TAB_ARENA => ed.arena_row,
            GUILD_TAB_COUNTS => ed.guild_count_selected(),
            GUILD_TAB_WEAPONS => ed.guild_weapon_selected().0,
            _ => ed.guild_field,
        },
        // One cursor per view: the screen swaps the list under it.
        Screen::Progress => ed.progress_cursor(),
        // A mode's row while one is open, the list's row otherwise.
        Screen::Roster => match ed.palico_skill {
            Some(state) => state.field,
            None => match ed.palico_equip {
                Some(state) => state.field,
                None => ed.roster_cursor,
            },
        },
        // The list's row while the list is showing, and the form's row while
        // the set editor is open: the editor is a mode of this screen.
        Screen::Sets => match ed.set_edit {
            Some(e) => e.field,
            None => match ed.item_set_edit {
                Some(e) => e.field,
                None => gui.sets_cursor,
            },
        },
        Screen::Csv => ed.csv_cursor,
        // The tab on screen owns the cursor (hbedit::Editor::quest_list_cursor),
        // so a page turn, a tap and the scrollbar all move the same row.
        Screen::SaveQuests => ed.quest_list_cursor(),
        Screen::QuestEquip => {
            if ed.qe_open {
                ed.qe_item_cursor
            } else {
                ed.cursor
            }
        }
        Screen::Mib => ed.mib_cursor,
        Screen::QuestEdit => ed.quest_edit_cursor,
        Screen::Gmd => ed.gmd_cursor,
        Screen::Check => ed.check_cursor,
        Screen::EntryView => ed.entry_row_cursor,
        Screen::Palico => match ed.palico_equip {
            Some(state) => state.field,
            None => ed.palico_cursor,
        },
        Screen::Options => ed.option_cursor,
        // 生データ: a position in the filtered save map, not a body offset.
        Screen::SaveMap => ed.map_cursor,
        // 変更一覧: a position in the value list.
        Screen::Review => ed.rev_cursor,
        _ => ed.cursor,
    }
}

fn list_len_of(ed: &Editor, gui: &GuiState) -> usize {
    match ed.screen {
        Screen::SaveList => ed.save_files.len(),
        Screen::Slots => 3,
        Screen::Char => CHAR_FIELDS.len(),
        Screen::Items => {
            if ed.item_menu_open {
                ITEM_OPS.len()
            } else {
                ed.visible_items().len()
            }
        }
        Screen::ItemPick => ed.item_pick_matches().len(),
        Screen::Charms => ed.charms().len(),
        Screen::CharmEdit => CHARM_EDIT_ROWS,
        Screen::SkillPick => ed.skill_pick_matches().len(),
        Screen::Equip => ed.equip_rows().len(),
        // The form has two shapes: 9 rows for an existing record and 3 for a
        // new one. The count comes from the form itself so the cursor and the
        // renderer can never disagree about it.
        Screen::EquipEdit => ed.equip_edit.map(|e| e.rows()).unwrap_or(0),
        Screen::JewelPick => ed.jewel_pick_matches().len(),
        Screen::EquipPick => ed.equip_pick_choices().len(),
        Screen::Hunts => ed.hunt_rows().len(),
        Screen::Guild => match ed.guild_view() {
            GUILD_TAB_ARENA => ARENA_ROWS,
            GUILD_TAB_COUNTS => mhnx_core::hbedit::GUILD_COUNT_ROWS,
            GUILD_TAB_WEAPONS => mhnx_core::hbedit::GUILD_WEAPON_ROWS,
            // The card's four rows: HR, clock, comment, card name.
            _ => mhnx_core::hbedit::GUILD_FIELDS,
        },
        Screen::Progress => ed.progress_len(),
        Screen::Roster => match ed.palico_skill {
            Some(_) => ed.palico_skill_list_len(),
            None => match ed.palico_equip {
                Some(_) => PALICO_EQUIP_ROWS,
                None => ed.palicos().len(),
            },
        },
        Screen::Sets => {
            if ed.set_edit.is_some() {
                SET_EDIT_ROWS
            } else if ed.item_set_edit.is_some() {
                ITEM_SET_EDIT_ROWS
            } else {
                sets_len(ed, gui.sets_tab)
            }
        }
        Screen::Csv => ed.csv_files.len(),
        Screen::SaveQuests => ed.quest_list_len(),
        Screen::QuestEquip => {
            if ed.qe_open {
                ed.quest_equip_items().len()
            } else {
                ed.quest_equipment().len()
            }
        }
        Screen::ArcList => ed.arc_files.len(),
        Screen::ArcEntries => ed.arc.as_ref().map(|a| a.entries.len()).unwrap_or(0),
        Screen::EntryView => {
            let entry = ed.arc.as_ref().and_then(|a| a.entries.get(ed.entry));
            match entry {
                Some(e) => match extension_for(e.type_hash) {
                    Some("rem") | Some("sup") => ed.entry_record_count(),
                    Some("sem") => ed.sem_entry_indices().len() * 6,
                    Some("esl") => 1,
                    Some("qdp") => Qdp::parse(&e.data).map(|q| q.flags().len()).unwrap_or(0),
                    Some("qdl") | Some("qlv") => {
                        QuestLink::parse(&e.data).map(|q| q.refs.len()).unwrap_or(0)
                    }
                    _ => 0,
                },
                None => 0,
            }
        }
        Screen::Mib => questfields::MIB_FIELDS.len(),
        // One row list per tab; the core answers for the tab it is on.
        Screen::QuestEdit => ed.quest_row_count(),
        Screen::Gmd => ed.gmd.as_ref().map(|g| g.strings.len()).unwrap_or(0),
        Screen::Check => ed.findings.len(),
        Screen::Palico => match ed.palico_equip {
            Some(_) => PALICO_EQUIP_ROWS,
            None => dlcats::all_ids().len(),
        },
        Screen::Options => 4,
        Screen::Convert => 0,
        Screen::SaveMap => ed.save_map_rows().len(),
        // 変更一覧: one row per value that differs, so an empty list is a save
        // identical to the card and the length is 0 rather than a placeholder.
        Screen::Review => ed.review_len(),
    }
}

fn set_active_cursor(ed: &mut Editor, gui: &mut GuiState, idx: usize) {
    if ed.screen == Screen::SaveQuests && ed.quest_tab > 0 {
        ed.set_quest_list_cursor(idx);
        return;
    }
    if ed.screen == Screen::Sets {
        // While a form is open a tap moves its row cursor, not the list's.
        // A tap on another row is a cursor move like any other, so a pending
        // operation (which belongs to the row it was armed on) is dropped.
        if let Some(mut e) = ed.set_edit {
            e.field = idx.min(SET_EDIT_ROWS - 1);
            ed.set_edit = Some(e);
            ed.set_clear_armed = None;
            return;
        }
        if let Some(mut e) = ed.item_set_edit {
            e.field = idx.min(ITEM_SET_EDIT_ROWS - 1);
            ed.item_set_edit = Some(e);
            ed.item_set_armed = None;
            return;
        }
        let n = sets_len(ed, gui.sets_tab);
        if n > 0 {
            gui.sets_cursor = idx.min(n - 1);
        }
        return;
    }
    if ed.screen == Screen::Csv {
        let n = ed.csv_files.len();
        if n > 0 {
            let target = idx.min(n - 1);
            // A tap on another file drops the pending apply; a second tap on
            // the same one keeps it, because that is the confirmation.
            if target != ed.csv_cursor {
                ed.csv_armed = None;
            }
            ed.csv_cursor = target;
        }
        return;
    }
    let len = list_len_of(ed, gui);
    if len == 0 {
        return;
    }
    let clamped = idx.min(len - 1);
    match ed.screen {
        Screen::Slots => {
            ed.slot = clamped;
            ed.cursor = clamped;
        }
        Screen::Char => {
            ed.field = clamped;
            ed.cursor = clamped;
        }
        Screen::CharmEdit => {
            if let Some(mut ce) = ed.charm_edit {
                ce.field = clamped;
                ed.charm_edit = Some(ce);
            }
        }
        Screen::Items => {
            if ed.item_menu_open {
                // A tap on another row is a cursor move: the armed dedupe
                // belongs to the row it was armed on.
                ed.item_menu_cursor = clamped;
                ed.item_armed = None;
            } else {
                ed.cursor = clamped;
            }
        }
        Screen::ItemPick => ed.item_pick_cursor = clamped,
        Screen::SkillPick => ed.skill_pick_cursor = clamped,
        Screen::Equip => {
            ed.equip_cursor = clamped;
            // A tap on another row is a cursor move: the pending delete
            // belongs to the row it was armed on, not to the new one.
            ed.equip_delete_armed = None;
        }
        Screen::EquipEdit => {
            if let Some(mut ee) = ed.equip_edit {
                ee.field = clamped;
                ed.equip_edit = Some(ee);
            }
        }
        Screen::JewelPick => ed.jewel_pick_cursor = clamped,
        Screen::EquipPick => {
            // The row is an index into the filtered list, so a tap lands on
            // the row it drew; the scene it draws from is the state machine's.
            let n = ed.equip_pick_choices().len();
            if n > 0 {
                ed.equip_pick_cursor = clamped.min(n - 1);
            }
        }
        Screen::Hunts => ed.hunt_cursor = clamped,
        Screen::Guild => match ed.guild_view() {
            GUILD_TAB_ARENA => {
                ed.arena_row = clamped.min(ARENA_ROWS.saturating_sub(1));
                ed.arena_edit = false;
            }
            GUILD_TAB_COUNTS => {
                ed.guild_count_cursor =
                    clamped.min(mhnx_core::hbedit::GUILD_COUNT_ROWS.saturating_sub(1));
            }
            GUILD_TAB_WEAPONS => {
                ed.guild_wpn_row =
                    clamped.min(mhnx_core::hbedit::GUILD_WEAPON_ROWS.saturating_sub(1));
                ed.guild_wpn_edit = false;
            }
            _ => {
                ed.guild_field = clamped.min(mhnx_core::hbedit::GUILD_FIELDS - 1);
            }
        },
        Screen::Progress => ed.set_progress_cursor(clamped),
        Screen::Roster => {
            if let Some(mut state) = ed.palico_skill {
                state.field = clamped;
                ed.palico_skill = Some(state);
            } else if let Some(mut state) = ed.palico_equip {
                // A tap on another row is a cursor move, so the pending release
                // (which belongs to the row it was armed on) is dropped.
                state.field = clamped;
                state.clear_armed = None;
                ed.palico_equip = Some(state);
            } else {
                ed.roster_cursor = clamped;
            }
        }
        Screen::SaveQuests => {
            ed.save_quest_cursor = clamped;
            ed.cursor = clamped;
        }
        Screen::QuestEquip => {
            if ed.qe_open {
                ed.qe_item_cursor = clamped;
            } else {
                ed.cursor = clamped;
            }
        }
        Screen::Mib => {
            ed.mib_cursor = clamped;
            ed.cursor = clamped;
        }
        Screen::QuestEdit => {
            ed.quest_edit_cursor = clamped;
            ed.cursor = clamped;
        }
        Screen::Gmd => {
            ed.gmd_cursor = clamped;
            ed.cursor = clamped;
        }
        Screen::Check => {
            ed.check_cursor = clamped;
            ed.cursor = clamped;
        }
        Screen::EntryView => ed.entry_row_cursor = clamped,
        Screen::Palico => {
            if let Some(mut state) = ed.palico_equip {
                state.field = clamped;
                state.clear_armed = None;
                ed.palico_equip = Some(state);
            } else {
                ed.palico_cursor = clamped;
                ed.cursor = clamped;
            }
        }
        Screen::Options => ed.option_cursor = clamped,
        Screen::SaveMap => {
            // A tap on another row moves the region, and the byte editor
            // belongs to the row it was opened on: it is put away rather than
            // following the cursor into a row it never asked about.
            if ed.map_cursor != clamped {
                ed.map_edit = None;
            }
            ed.map_cursor = clamped;
        }
        Screen::Review => {
            // A tap on another line drops an armed 戻す, the same rule every
            // other two-press operation follows.
            ed.rev_armed = None;
            ed.rev_cursor = clamped;
        }
        _ => ed.cursor = clamped,
    }
}

fn scroll_active_cursor(ed: &mut Editor, delta: i32) {
    let dummy = GuiState::new();
    let len = list_len_of(ed, &dummy);
    if len == 0 {
        return;
    }
    let cur = active_cursor_of(ed, &dummy) as i32;
    let next = (cur + delta).clamp(0, len as i32 - 1) as usize;
    let mut dummy_mut = GuiState::new();
    set_active_cursor(ed, &mut dummy_mut, next);
}

fn apply_row_max(ed: &mut Editor, mib_mob_sub: usize, idx: usize) {
    match ed.screen {
        // The four screens that had no arm at all (狩猟記録 / ギルドカード /
        // 所持オトモ / 環境設定 / 護石編集) answer from the state machine, so the
        // numbers are the same ones the rows print and the tests can pin them.
        Screen::Char => {
            ed.char_row_max(idx);
        }
        Screen::Items => {
            ed.item_row_max(idx);
        }
        Screen::Hunts => {
            ed.hunt_row_max(idx);
        }
        Screen::Guild => match ed.guild_view() {
            GUILD_TAB_ARENA => {
                ed.message =
                    String::from("闘技大会の行に最大値はありません（[Y] で直接入力）");
            }
            GUILD_TAB_COUNTS => {
                ed.guild_count_row_max(idx);
            }
            GUILD_TAB_WEAPONS => {
                ed.guild_weapon_row_max();
            }
            _ => {
                ed.guild_row_max(idx);
            }
        },
        Screen::Roster => {
            ed.roster_row_max(idx);
        }
        Screen::Options => {
            ed.option_row_max(idx);
        }
        Screen::CharmEdit => {
            ed.charm_row_max(idx);
        }
        Screen::Charms => {
            ed.message = String::from("この一覧の行に最大値はありません（[A] で護石を開いてください）");
        }
        // The set form has no maximum: the rows name records, not numbers.
        // Taking a part out is [-], which asks for a second press, so MAX must
        // never do it in one.
        Screen::Sets => {
            ed.message = String::from("この行に最大値はありません（[-] 2回で部位を外せます）");
        }
        // [MAX] on the equipment form is per row: the level goes to its
        // ceiling, a decoration is taken out, and the two id rows have no
        // maximum to jump to.
        Screen::EquipEdit => {
            let Some(mut ee) = ed.equip_edit else {
                return;
            };
            match idx {
                1 => {
                    ee.level = 32;
                    ed.message = String::from("レベルを 32 にしました（A で書き込み）");
                }
                2 => {
                    ee.transmog_id = 0;
                    ed.message = String::from("着せ替えIDを 0（未設定）にしました（A で書き込み）");
                }
                3 => {
                    ee.transmog_level = 32;
                    ed.message = String::from("着せ替え元Lvを 32 にしました（A で書き込み）");
                }
                4..=6 => {
                    ee.decorations[idx - 4] = 0;
                    ed.message = format!("装飾品{} を外しました（A で書き込み）", idx - 3);
                }
                _ => {
                    ed.message = String::from("この行に最大値はありません");
                    return;
                }
            }
            ed.equip_edit = Some(ee);
        }
        Screen::QuestEquip => {
            if !ed.qe_open {
                return;
            }
            let Some(t) = ed.quest_equip_items().get(idx).copied() else {
                return;
            };
            if t.item == 0 {
                ed.message = String::from("空きスロットです。[A] で先にアイテムを選んでください");
                return;
            }
            let slot_index = ed.cursor;
            if let Some(s) = &mut ed.save {
                if mhnx_core::dlc::set_quest_equipment_item(
                    s,
                    slot_index,
                    t.record,
                    t.list,
                    t.index,
                    t.item,
                    99,
                )
                .is_ok()
                {
                    ed.dirty = true;
                    ed.message = format!("レコード {} 位置 {} の個数を 99 にしました", t.record, t.index);
                }
            }
        }
        Screen::Mib => {
            if let Some(f) = questfields::MIB_FIELDS.get(idx) {
                if f.key.starts_with("mob_slot[") {
                    let target = match mib_mob_sub {
                        3 => 200,
                        4 => 10,
                        _ => 255,
                    };
                    adjust_mib_mob_sub(ed, idx, mib_mob_sub, target);
                }
            }
        }
        Screen::EntryView => {
            let ext = ed
                .arc
                .as_ref()
                .and_then(|a| a.entries.get(ed.entry))
                .and_then(|e| extension_for(e.type_hash));
            if ext == Some("sem") {
                ed.entry_row_cursor = idx;
                // Through the same two-press path the button uses: a tap that
                // arms the preset must not write on the first press either.
                ed.sem_preset_press(1);
            } else {
                // The cell decides (hbedit::Editor::entry_row_max): the count
                // goes to 99, the chance to 100%, and the アイテム cell says
                // what [A] does instead of refusing ninety-nine times. The
                // loop here sent [▶] 99 times, which on the item cell wrote
                // nothing at all and on the others was 99 undo-free steps.
                ed.entry_row_cursor = idx;
                ed.entry_row_max();
            }
        }
        // [MAX] on the second stage of the bulk add asks for a full stack.
        Screen::ItemPick => {
            if ed.item_pick_mode == 3 && ed.item_pick_stage == 1 {
                ed.item_pick_add_count = mhnx_core::itembox::MAX_COUNT;
                ed.message = ed.item_add_message();
            } else {
                ed.message =
                    String::from("この一覧に最大値はありません（[A] で決定してください）");
            }
        }
        // Every other screen either does not draw the button (see
        // max_is_available) or has nothing to jump to: it says so rather than
        // swallowing the press.
        _ => {
            ed.message = String::from("この画面の、この行に最大値はありません");
        }
    }
}


/// One step pill's label. -1 is "reversed", which is what the sign means.
fn step_pill_label(step: i64) -> String {
    if step < 0 {
        String::from("反転")
    } else {
        format!("±{}", step)
    }
}

/// Does the row under the cursor have a ceiling [MAX] can jump to?
///
/// False hides the button (the slot prints a dash instead): the five screens
/// that answered "nothing" before, the candidate lists and the pickers have no
/// ceiling to jump to, so drawing a live button there would be a lie.
fn max_is_available(ed: &Editor, row: usize) -> bool {
    match ed.screen {
        // Row 0 is the name, which is text, and row 13 is the gender, which is
        // a 0 / 1 toggle: neither has a ceiling to jump to, and the button that
        // jumped anyway wrote 3 into the gender byte.
        Screen::Char => row != 0 && row != 13,
        // Both set forms edit a list of candidate records, not numbers.
        Screen::Sets => false,
        Screen::Items | Screen::Options | Screen::Roster | Screen::Hunts => true,
        // The card's rank and clock rows, and every row of the two record
        // views: those are all u16 numbers with a ceiling.
        Screen::Guild => match ed.guild_view() {
            GUILD_TAB_COUNTS | GUILD_TAB_WEAPONS => true,
            GUILD_TAB_ARENA => false,
            _ => row <= 1,
        },
        // 種類上限: slots and the two point rows. The kind row prints its
        // ceiling instead of jumping to it, and the box-slot row names a slot.
        Screen::CharmEdit => matches!(row, 2 | 4 | 6),
        // The id row of the equipment form has no ceiling.
        Screen::EquipEdit => row != 0,
        Screen::QuestEquip => ed.qe_open,
        Screen::Mib => questfields::MIB_FIELDS
            .get(row)
            .map(|f| f.key.starts_with("mob_slot["))
            .unwrap_or(false),
        Screen::EntryView => true,
        Screen::ItemPick => ed.item_pick_mode == 3 && ed.item_pick_stage == 1,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Drawing & Rendering Engine (Horizon Split-Studio UI)
// ---------------------------------------------------------------------------

/// Safely draws centered text inside `(x, y, w, h)` and clips it if wider than `w - 12`
/// so button/pill labels can NEVER overflow their box or overlap neighbors.
#[inline]
fn btn_text(
    c: &mut Canvas,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    s: &str,
    color: Color,
    size: FontSize,
) {
    let max_w = (w - 12).max(8);
    let tw = c.text_width_sized(s, size);
    if tw <= max_w {
        c.text_centered(x, y, w, h, s, color, size);
    } else {
        let th = c.line_height_sized(size);
        let ty = y + (h - th) / 2;
        c.text_clipped(x + 6, ty, max_w, s, color, size);
    }
}

fn push_unique(out: &mut Vec<String>, s: String) {
    if !out.iter().any(|e| *e == s) {
        out.push(s);
    }
}

/// Everything about the talisman under the cursor that the game would refuse.
///
/// `charm::grade_warnings` answers for the record that is in the box right
/// now, and the same questions are asked again about the values the form would
/// write: picking a skill in the form does not touch the box until `[A]`, so
/// the stored record alone would never flag a skill the grade cannot carry.
///
/// The kind is part of the answer now. The game has one point table per rarity
/// and one slot table per rarity, not one per kind, so the ceiling of a kind is
/// `charm::KIND_LIMITS`: 兵士 / 闘士 have one slot and no second skill, and
/// 女王 / 王 / 龍 stop at +8 on the first skill. Both are reported here rather
/// than on the box, because the form can hold a kind the box does not.
#[allow(clippy::too_many_arguments)]
fn charm_warnings(
    box_bytes: Option<&[u8]>,
    index: usize,
    grade: charm::TalismanGrade,
    kind: u8,
    slots: u8,
    skill1: u8,
    points1: i8,
    skill2: u8,
    points2: i8,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(b) = box_bytes {
        for w in charm::grade_warnings(b, index, grade) {
            push_unique(&mut out, w);
        }
    }
    let limits = charm::kind_limits(kind);
    let who = limits
        .map(|l| l.name())
        .unwrap_or_else(|| grade.name());
    let max_slots = limits
        .map(|l| l.max_slots.min(grade.max_slots()))
        .unwrap_or_else(|| grade.max_slots());
    if slots > max_slots {
        push_unique(
            &mut out,
            format!("スロット{}個は{}の上限{}を超えています", slots, who, max_slots),
        );
    }
    for (skill, pts, second, label) in [
        (skill1, points1, false, "スキル1"),
        (skill2, points2, true, "スキル2"),
    ] {
        if skill == 0 {
            continue;
        }
        // A kind without a second slot says so before any table is consulted:
        // rarity 97 has no slot-2 rows, so the row is 選択不可 and a value on it
        // is a record the game would not write.
        if second && !charm::second_skill_available(kind) {
            push_unique(
                &mut out,
                format!(
                    "{} は{}に設定できません（この種類は第2スキルを持てません）",
                    label, who
                ),
            );
            continue;
        }
        // One line per value: the per-skill range already has the kind's own
        // ceiling applied (charm::allowed_points_for_kind), so the two can
        // never disagree about the same number.
        let range = match limits {
            Some(l) => charm::allowed_points_for_kind(l.kind, skill, second),
            None => charm::allowed_points_rarity(grade.rarity(), skill, second),
        };
        match range {
            Some(r) if r.min == 0 && r.max == 0 => {
                push_unique(&mut out, format!("{} は{}では付けられません", label, who));
            }
            Some(r) => {
                if pts < r.min || pts > r.max {
                    push_unique(
                        &mut out,
                        format!(
                            "{} {} は {}〜{} の範囲外です",
                            label,
                            pts,
                            r.min,
                            r.max
                        ),
                    );
                }
            }
            None => {
                // A skill id past the end of the talisman tables: the game can
                // never roll it, so it is worth saying even though the value
                // alone would look harmless. (This build always carries the
                // gamedata tables - see switch/Cargo.toml.)
                push_unique(
                    &mut out,
                    format!("{} のスキルID {} は護石の表にないため付けられません", label, skill),
                );
            }
        }
    }
    out
}

impl GuiState {
    pub fn draw(&mut self, c: &mut Canvas, ed: &Editor) {
        self.hits.clear();
        // A dialog that is not on screen has no position: the next one starts
        // at its first line, so a scroll left over from the last write cannot
        // hide the headline of the next one.
        if ed.review.is_none() {
            self.review_scroll = 0;
        }
        let t = theme();
        c.clear(t.bg_canvas);

        // 1. Top 2-Tier Navigation Header (y = 0..92)
        self.draw_top_nav(c, ed);

        // 2. The one-line guide for this screen (y = 92..114)
        self.draw_guide(c, ed);

        // 3. Main Content Workspace (y = CONTENT_Y..FOOTER_Y)
        self.draw_content(c, ed);

        // 4. Minimal Unified Footer Bar (y = FOOTER_Y..720)
        self.draw_footer(c, ed);

        // 5. Modal Overlays (MIB Picker / Built-in OSK)
        //
        // The two panels cover most of the frame but not all of it, and the
        // screen under them still has live tap targets - including the footer's
        // undo pill at y670..700, which ran a whole-save undo from a tap on the
        // band the keyboard leaves open. This hit covers the whole frame and is
        // pushed before the panel's own targets, so a tap that lands on the
        // panel still reaches it (the search runs newest first) and a tap that
        // misses it stops here.
        if mhnx_core::confirm::needs_modal_guard(
            self.mib_picker.is_some(),
            self.osk.is_some(),
            ed.review_open(),
        ) {
            self.add_hit(0, 0, 1280, 720, UiAction::ModalBlank);
        }
        if self.mib_picker.is_some() {
            self.draw_mib_picker(c, ed);
        }
        if self.osk.is_some() {
            self.draw_osk(c);
        }
        // The save review sits on top of everything: it is the last thing
        // between an edit and the console's save.
        if ed.review_open() {
            self.draw_review(c, ed);
        }
        // The update dialog is above even that: it is the one panel a key or a
        // tap cannot land under, because it owns the pad while it is up.
        if self.update.dialog {
            self.add_hit(0, 0, 1280, 720, UiAction::ModalBlank);
            self.draw_update_dialog(c);
        }

        c.present();
    }

    /// The one-line guide under the sub-tab bar.
    ///
    /// It is the sentence a first-time player needs before touching anything:
    /// what [A] does on this screen, and where the bytes are when they want
    /// them. The words come from the state machine (Editor::screen_guide), so
    /// a screen cannot change without its guide following it, and the note on
    /// the right names the pad route to the same [詳細] pill.
    fn draw_guide(&mut self, c: &mut Canvas, ed: &Editor) {
        let t = theme();
        c.rect(0, GUIDE_Y, 1280, GUIDE_H, t.bg_subbar);
        c.hline(0, GUIDE_Y, 1280, t.border_subtle);
        // A small accent bar at the left, the same shape the selected sub-tab
        // has, so the line reads as part of the navigation rather than as a
        // message from the program.
        c.rect(MAIN_X - 6, GUIDE_Y + 4, 3, GUIDE_H - 8, t.accent_secondary);

        let mut right = 1262;
        // The update banner takes this end of the strip while it is up. It is
        // the quietest place a line can be put in this layout: the strip is one
        // line high, it already belongs to the program rather than to the play,
        // and nothing under it moves. It displaces the [R長押し] note while it
        // is shown, which is the right trade: the note is a hint about a key,
        // the banner is new information.
        if self.update.banner_visible() {
            // [A] opens the dialog on the start screen and nowhere else (every
            // other screen's [A] is its own key), so the banner only names the
            // key where the key is true. The pill itself is a tap target on
            // every screen, which is why the line can be drawn anywhere.
            let a_opens_it = ed.screen == Screen::SaveList;
            right = self.draw_update_banner(c, right, a_opens_it);
        }
        // Not on 生データ itself: the note would advertise the screen that is
        // already showing. It names the pill by the word the pill itself
        // carries (Editor::detail_pill_label), so the two cannot drift into
        // "詳細" and "生データ" for the same button.
        if !self.update.banner_visible() && detail_available(ed) {
            let word = ed
                .detail_pill_label()
                .trim_start_matches('[')
                .trim_end_matches(']');
            let note = format!("[R長押し] {}", word);
            let nw = c.text_width_sized(&note, FontSize::Small);
            c.text_right(right, GUIDE_Y + 5, &note, t.text_muted, FontSize::Small);
            right -= nw + 16;
        }
        let room = (right - (MAIN_X + 4)).max(120);
        // While the update banner is up the strip is narrower than the guide,
        // and a clipped guide loses its *end* - which is where the way out
        // ([B] / [＋]) is named. The tail is drawn instead, prefixed with an
        // ellipsis, so the last item survives the banner.
        let guide = ed.screen_guide();
        // The tail is measured against the room the marker leaves, not against
        // the whole strip. It used to be measured against `room` and then drawn
        // in `room - marker_w`, so text_clipped cut it a second time - and the
        // end of a guide is where the way out is named, so the item that lost
        // its last glyph or two was the one the strip exists for.
        let marker = "…";
        let marker_w = c.text_width_sized(marker, FontSize::Medium);
        let tail_room = (room - marker_w).max(0);
        let mut width = |s: &str| c.text_width_sized(s, FontSize::Medium);
        if width(&guide) <= room {
            c.text_clipped(
                MAIN_X + 4,
                GUIDE_Y + 3,
                room,
                &guide,
                t.text_secondary,
                FontSize::Medium,
            );
        } else {
            let tail = mhnx_core::hbedit::guide_tail(&guide, tail_room, &mut width);
            if !tail.is_empty() {
                c.text_clipped(
                    MAIN_X + 4,
                    GUIDE_Y + 3,
                    marker_w,
                    marker,
                    t.text_muted,
                    FontSize::Medium,
                );
                c.text_clipped(
                    MAIN_X + 4 + marker_w,
                    GUIDE_Y + 3,
                    tail_room,
                    tail,
                    t.text_secondary,
                    FontSize::Medium,
                );
            }
        }
    }

    fn draw_top_nav(&mut self, c: &mut Canvas, ed: &Editor) {
        let t = theme();
        let active_sec = section_of(ed.screen);

        // Tier 1: Main Sections [ZL] ... [ZR] + Active Slot + [X] Save (y = 0..48)
        c.rect_gradient_v(0, 0, 1280, TOP_SEC_H, t.bg_header, t.bg_canvas);
        c.hline(0, TOP_SEC_H - 1, 1280, t.border_subtle);

        // [ZL] button
        c.round_rect(14, 8, 44, 32, t.corner_r, t.bg_btn);
        c.round_rect_border(14, 8, 44, 32, t.corner_r, 1, t.border_subtle);
        btn_text(c, 14, 8, 44, 32, "ZL", t.accent_primary, FontSize::Small);
        self.add_hit(14, 8, 44, 32, UiAction::CycleSection(false));

        // 7 Main Sections, sized to their own label (x = 66..~785)
        //
        // Variable width is what let the seventh section exist: six fixed
        // 132px tabs fill the row, and the technical section has to be one of
        // the seven rather than a hidden mode. The labels are short, so the
        // row ends well before the slot badge and every tab still reads whole.
        let mut sx = 66i32;
        for &(sec, label) in MAIN_SECTIONS.iter() {
            let is_active = sec == active_sec;
            let tw = (c.text_width_sized(label, FontSize::Small) + 22).clamp(64, 132);
            if is_active {
                c.round_rect(sx, 6, tw, 36, 6, t.bg_selected);
                c.rect(sx + 10, TOP_SEC_H - 3, tw - 20, 3, t.accent_primary);
            }
            let fg = if is_active { t.text_primary } else { t.text_secondary };
            btn_text(c, sx, 6, tw, 36, label, fg, FontSize::Small);
            self.add_hit(sx, 6, tw, 36, UiAction::GotoSection(sec));
            sx += tw + 6;
        }

        // The section-forward button, straight after the last section.
        //
        // It used to be labelled "ZR", and that was a lie on most screens: the
        // pad's ZR is the screen's own second action wherever
        // hbedit::Editor::zr_job answers ZrJob::Screen (the item box's search
        // is the menu now, the progress screen's search, the .mib's 項目, the
        // quest's 個数/確率, the pickers' 種別 ...), and only the section bar
        // where it answers Sidebar. One label, two meanings - the review's
        // finding. The button is a tap target, so it says what the tap does and
        // names no key at all; [ZL] on the left keeps its name because the pad
        // really does deliver ZL to the section bar on every screen.
        let zr_label = "セクション送り";
        let zr_w = (c.text_width_sized(zr_label, FontSize::Small) + 16).clamp(44, 148);
        let zr_x = sx;
        c.round_rect(zr_x, 8, zr_w, 32, t.corner_r, t.bg_btn);
        c.round_rect_border(zr_x, 8, zr_w, 32, t.corner_r, 1, t.border_subtle);
        btn_text(
            c,
            zr_x,
            8,
            zr_w,
            32,
            zr_label,
            t.accent_primary,
            FontSize::Small,
        );
        self.add_hit(zr_x, 8, zr_w, 32, UiAction::CycleSection(true));

        // Right Status Area: Active Slot badge + [X] Save Pill
        //
        // Two lines, because one was never enough: "Slot 1: " and a name of up
        // to ten full-width glyphs is 238px and the space between the section
        // tabs and the save pill is 212, so the one-line badge cut every name
        // to three glyphs and "..". The badge takes the whole gap and puts the
        // slot number on its own line, which leaves the name its full 188px.
        // The badge takes the gap the sections left, so a wider row of short
        // labels gives the name more room rather than leaving a hole. It starts
        // after the section-forward button: the button is wider than the 44px
        // "ZR" it replaced, and the badge must not be drawn under it.
        let slot_x = zr_x + zr_w + 6;
        let slot_w = (1108 - slot_x).max(0);
        let slot_name = ed
            .save
            .as_ref()
            .and_then(|save| save.read_character(ed.slot))
            .map(|ch| ch.name)
            .unwrap_or_else(|| String::from("未読込"));
        c.round_rect(slot_x, 4, slot_w, 40, t.corner_r, t.bg_panel);
        c.round_rect_border(slot_x, 4, slot_w, 40, t.corner_r, 1, t.border_subtle);
        // The slot number and the first line share one row, which leaves the
        // second row for the second line of a name. The game draws that line,
        // so the badge does too; a third line and beyond say they are there.
        let mut first = format!("{}: {}", ed.slot + 1, mhnx_core::save::name_lines(&slot_name)[0]);
        if slot_name.contains('\n') {
            let lines = mhnx_core::save::name_lines(&slot_name);
            if lines.len() > 2 {
                first.push('…');
            }
        }
        c.text_clipped(slot_x + 10, 6, slot_w - 20, &first, t.text_primary, FontSize::Small);
        let second = mhnx_core::save::name_lines(&slot_name).get(1).copied().unwrap_or("");
        if !second.is_empty() || slot_name.contains('\n') {
            c.text_clipped(slot_x + 10, 26, slot_w - 20, second, t.text_muted, FontSize::Small);
        }
        self.add_hit(slot_x, 4, slot_w, 40, UiAction::SlotBadge((ed.slot + 1) % 3));

        // [X] Save Button
        let save_x = 1116;
        let save_w = 150;
        let (save_bg, save_bd, save_txt) = if ed.dirty {
            (t.accent_warn, t.text_primary, "[X] 未保存")
        } else {
            (t.accent_ok, t.border_subtle, "[X] 保存")
        };
        c.round_rect(save_x, 8, save_w, 32, t.corner_r, save_bg);
        c.round_rect_border(save_x, 8, save_w, 32, t.corner_r, 1, save_bd);
        btn_text(
            c,
            save_x,
            8,
            save_w,
            32,
            save_txt,
            white_or_dark(save_bg),
            FontSize::Small,
        );
        self.add_hit(save_x, 8, save_w, 32, UiAction::QuickSave);

        // Tier 2: Sub-Tabs [L] ... [R] (y = SUB_TAB_Y..SUB_TAB_Y + SUB_TAB_H)
        c.rect(0, SUB_TAB_Y, 1280, SUB_TAB_H, t.bg_subbar);
        c.hline(0, SUB_TAB_Y + SUB_TAB_H - 1, 1280, t.border_subtle);

        let sy = SUB_TAB_Y + 6;
        let sh = 32;

        // [L] Sub-Tab Prev
        c.round_rect(14, sy, 40, sh, 6, t.bg_btn);
        c.round_rect_border(14, sy, 40, sh, 6, 1, t.border_subtle);
        btn_text(c, 14, sy, 40, sh, "L", t.accent_primary, FontSize::Small);
        self.add_hit(14, sy, 40, sh, UiAction::CycleSubTab(false));

        let (subs, active_sub) = self.sub_tabs_for(ed);
        let mut sub_x = 60i32;
        for (idx, label) in subs.iter().enumerate() {
            let is_active = idx == active_sub;
            let tw = c.text_width_sized(label.as_str(), FontSize::Small) + 24;
            let w = tw.clamp(84, 156);
            // 800 rather than 880: the [R] button and the [詳細] pill share the
            // right end of this bar now, and a label that reached under them
            // would be a label nobody could tap.
            if sub_x + w > 800 {
                break;
            }
            let bg = if is_active { t.bg_selected } else { t.bg_subbar };
            let fg = if is_active { t.accent_primary } else { t.text_secondary };
            c.round_rect(sub_x, sy, w, sh, 5, bg);
            if is_active {
                c.rect(sub_x + 8, sy + sh - 3, w - 16, 3, t.accent_secondary);
            }
            btn_text(c, sub_x, sy, w, sh, label.as_str(), fg, FontSize::Small);
            self.add_hit(sub_x, sy, w, sh, UiAction::SelectSubTab(idx));
            sub_x += w + 6;
        }

        // [R] Sub-Tab Next
        c.round_rect(sub_x + 2, sy, 40, sh, 6, t.bg_btn);
        c.round_rect_border(sub_x + 2, sy, 40, sh, 6, 1, t.border_subtle);
        btn_text(c, sub_x + 2, sy, 40, sh, "R", t.accent_primary, FontSize::Small);
        self.add_hit(sub_x + 2, sy, 40, sh, UiAction::CycleSubTab(true));

        // [詳細] - the one button every screen carries.
        //
        // It is on this bar because this bar is where the screens are named:
        // the raw bytes of the screen that is showing are one press away from
        // wherever the user is, and the same pill is the touch target for the
        // pad's [R 長押し]. The label dims on the screens that have no bytes of
        // their own (the file lists and the detail screens themselves) instead
        // of disappearing, so the row never changes shape.
        let detail_here = detail_available(ed);
        let (d_bg, d_fg, d_bd) = if detail_here {
            (t.bg_btn_primary, t.text_primary, t.accent_primary)
        } else {
            (t.bg_btn, t.text_muted, t.border_subtle)
        };
        c.round_rect(DETAIL_X, sy, DETAIL_W, sh, 5, d_bg);
        c.round_rect_border(DETAIL_X, sy, DETAIL_W, sh, 5, 1, d_bd);
        btn_text(
            c,
            DETAIL_X,
            sy,
            DETAIL_W,
            sh,
            ed.detail_pill_label(),
            d_fg,
            FontSize::Small,
        );
        // A dimmed pill takes no tap. It used to be drawn dim and still answer
        // with a refusal, which is a button that invites a press it means to
        // turn down: the dimming now *is* the answer, and the guide line says
        // which screens have the bytes.
        if detail_here {
            self.add_hit(DETAIL_X, sy, DETAIL_W, sh, UiAction::OpenDetail);
        }

        // Right side of Sub-Tab Bar: Concise Tool Pills (x = 964..1266)
        self.draw_sub_bar_quick_tools(c, ed, sy, sh);
    }

    fn draw_sub_bar_quick_tools(&mut self, c: &mut Canvas, ed: &Editor, sy: i32, sh: i32) {
        let t = theme();
        let next_step = (ed.step + 1) % STEPS.len();
        match ed.screen {
            Screen::Char => {
                let step_lbl = format!("刻み ±{}", ed.step_value());
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, &step_lbl, t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::SetStep(next_step));

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                // 174px of label in a 158px pill: it drew as "資金・P 一括..".
                // The two-press rule is the message on the press
                // (Editor::char_max_all_press), not the label.
                btn_text(c, 1108, sy, 158, sh, "資金・P 一括MAX", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::CharMaxAll);
            }
            Screen::Items => {
                // The state the heading names, on the key that changes it:
                // 所持品のみ <-> 空きも表示. The step chip moved into the deck,
                // because a key for it would have to be taken from one of the
                // six this screen already uses.
                let empty_lbl = if ed.show_empty {
                    String::from("空きを隠す")
                } else {
                    String::from("空きも表示")
                };
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, &empty_lbl, t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::ToggleItemEmpty);

                // The bulk edits live in one menu now, so the pill opens it
                // instead of promising a single *99 that is only one row. The
                // key is [ZR] (hbedit::Editor::open_item_menu) - [-] is the
                // delete key and the menu had no key at all before this.
                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                if ed.item_menu_open {
                    btn_text(c, 1108, sy, 158, sh, "閉じる", t.text_primary, FontSize::Small);
                    self.add_hit(1108, sy, 158, sh, UiAction::Key(Key::B));
                } else {
                    btn_text(c, 1108, sy, 158, sh, &pill_word(ed, "ZR", "一括"), t.text_primary, FontSize::Small);
                    self.add_hit(1108, sy, 158, sh, UiAction::OpenItemMenu);
                }
            }
            // クエスト改造: the field switch on the left, and the small 詳細
            // button on the right that leads to the file-by-file screens.
            Screen::QuestEdit => {
                let chip = if ed.quest_edit_tab == QUEST_TAB_MONSTERS {
                    String::from(ed.quest_field_label())
                } else if ed.quest_edit_tab == QUEST_TAB_REWARDS {
                    // The two values [ZR] walks, named on the key that walks
                    // them (hbedit::QUEST_REWARD_*). The third stop held no
                    // value at all - it was the search - so the chip said 検索
                    // next to a key whose arrows could only refuse.
                    match ed.quest_reward_field {
                        mhnx_core::hbedit::QUEST_REWARD_CHANCE => String::from("確率%"),
                        mhnx_core::hbedit::QUEST_REWARD_COUNT => String::from("個数"),
                        _ => String::from("個数"),
                    }
                } else {
                    // 支給品 has one value and nothing to switch, so [ZR] there
                    // is the search itself. The chip said 項目 - a value switch
                    // on a tab with no second value.
                    String::from("検索")
                };
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, &chip, t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::Key(Key::Z));
                // The two item tabs' search is its own press: the 検索 pill
                // here, and [R] on the pad - [ZR] only walks the three stops
                // and never opens a keyboard on the user's behalf. [-] is the
                // delete key and nothing else.
                if matches!(ed.quest_edit_tab, QUEST_TAB_REWARDS | QUEST_TAB_SUPPLIES) {
                    let lbl = if ed.quest_item_filter.trim().is_empty() {
                        String::from("検索")
                    } else {
                        format!("検索: {}", ed.quest_item_filter.trim())
                    };
                    c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                    c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                    btn_text(c, 1108, sy, 158, sh, &lbl, t.text_primary, FontSize::Small);
                    self.add_hit(1108, sy, 158, sh, UiAction::FilterQuestItems);
                }
            }
            // The item set form's name has no row of its own (the thirty-two
            // rows are the entries), so the keyboard lives on [ZR] and the deck
            // gives a finger the same key. The pad already reaches it: the
            // state machine answers ZrJob::Screen on this form, and this pill
            // is the touch half of the same answer.
            Screen::Sets if ed.item_set_edit.is_some() => {
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, "セット名", t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::Key(Key::Z));
            }
            Screen::Charms => {
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                // One word for the action on every screen: 検索. The pill
                // says what it searches rather than a second verb.
                btn_text(c, 964, sy, 136, sh, "検索", t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::FilterCharms);

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, "新規護石", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::NewCharm);
            }
            // The talisman form has no bulk chip and no preset chip any more:
            // its [A] is the row's own action, which the inspector button and
            // the footer name for the selected row, and the ceiling of the kind
            // is printed beside the header (hbedit::CharmEdit::header_note)
            // instead of behind a 合法最大値 button.
            // The skill panel is the other mode of the roster screen, so its
            // two keys are the chips while it is open.
            Screen::Roster if ed.palico_skill_open() => {
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, "タブ切替", t.accent_secondary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::Key(Key::Y));

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, "差し替え", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::Key(Key::A));
            }
            // The equipment panel is a mode of both palico screens, so its two
            // keys are the only chips while it is open.
            Screen::Roster | Screen::Palico if ed.palico_equip_open() => {
                // The two pills are sized the other way round from the rest of
                // the deck: "[-] 解除（2回）" is 130px (it names the two-press
                // delete, which is the one thing this key must not be guessed
                // at) and "[A] 装着" is 70. 158 + 6 + 136 = 300 of the 302.
                c.round_rect(964, sy, 158, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 158, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 158, sh, "解除（2回）", t.accent_secondary, FontSize::Small);
                self.add_hit(964, sy, 158, sh, UiAction::Key(Key::Minus));

                c.round_rect(1128, sy, 136, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1128, sy, 136, sh, 5, 1, t.accent_primary);
                btn_text(c, 1128, sy, 136, sh, "装着", t.text_primary, FontSize::Small);
                self.add_hit(1128, sy, 136, sh, UiAction::Key(Key::A));
            }
            Screen::Roster => {
                // [Y] walks the three numbers the list edits (Lv / なつき /
                // EXP) and was on neither the footer nor the deck: a touch-only
                // player could not change the column at all.
                //
                // The pill names the column and nothing else. It used to print
                // the key and the column - 122px in a 96px pill, so it drew as
                // a clipped fragment - and the footer already names the key and
                // what it walks; the pill is the state.
                //
                // Three pills in the 302px the deck has (964..1266), sized to
                // their own labels: 99 + 6 + 91 + 6 + 99 = 301. The widest
                // label of the three columns is "[Y] なつき" at 87px, and
                // btn_text keeps 12px inside the pill.
                // The table's word plus the state: 列 is what [Y] walks and
                // なつき is where it is. The pill names no key - the footer
                // does - and the word is the table's, so the two agree.
                let y_lbl = format!("{} {}", pill_word(ed, "Y", "列"), ed.roster_column_label());
                c.round_rect(964, sy, 99, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 99, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 99, sh, &y_lbl, t.accent_secondary, FontSize::Small);
                self.add_hit(964, sy, 99, sh, UiAction::Key(Key::Y));

                c.round_rect(1069, sy, 91, sh, 5, t.bg_btn);
                c.round_rect_border(1069, sy, 91, sh, 5, 1, t.border_subtle);
                btn_text(c, 1069, sy, 91, sh, "装備", t.text_primary, FontSize::Small);
                self.add_hit(1069, sy, 91, sh, UiAction::Key(Key::Z));

                c.round_rect(1166, sy, 99, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1166, sy, 99, sh, 5, 1, t.accent_primary);
                btn_text(c, 1166, sy, 99, sh, "スキル", t.text_primary, FontSize::Small);
                self.add_hit(1166, sy, 99, sh, UiAction::Key(Key::Plus));
            }
            // One pill, not two: the bulk install is the deck's button (the
            // panel below the list is where this screen's operations live, and
            // it carries the language switch beside it). The bar drew the same
            // action a second time, which is what the sixth review counted as
            // "ボタン2つ" for one [Y].
            Screen::Palico => {
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, &pill_word(ed, "ZR", "装備"), t.accent_secondary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::Key(Key::Z));
            }
            Screen::Mib => {
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                // The two presets rewrite every monster of the quest, so the
                // chip says how many and the press is the first of two.
                let p0 = ed.mib_preset_label(0);
                btn_text(c, 964, sy, 136, sh, &p0, t.accent_secondary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::MibPreset(0));

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                let p1 = ed.mib_preset_label(1);
                btn_text(c, 1108, sy, 158, sh, &p1, t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::MibPreset(1));
            }
            // The arena view: the two keys a time that is not being stepped
            // still needs.
            Screen::Guild if ed.guild_view() == GUILD_TAB_ARENA => {
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, "直接入力", t.accent_secondary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::Key(Key::Y));

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                let lbl = if ed.arena_edit {
                    "編集終了"
                } else {
                    "編集開始"
                };
                btn_text(c, 1108, sy, 158, sh, lbl, t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::Key(Key::A));
            }
            // The guild card's two record views have no sub-tab (the bar is
            // full), so the key that opens them gets two pills - the touch
            // route to the same key, so the pad and a finger cannot disagree.
            Screen::Guild => {
                let (label, action) = match ed.guild_view() {
                    GUILD_TAB_CARD => (String::from("クエスト回数"), UiAction::Key(Key::Y)),
                    GUILD_TAB_COUNTS => (String::from("武器使用数"), UiAction::Key(Key::Y)),
                    GUILD_TAB_WEAPONS => (String::from("カードへ"), UiAction::Key(Key::Y)),
                    _ => (String::from("カードへ"), UiAction::Key(Key::B)),
                };
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, &label, t.accent_secondary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, action);

                // The step the ◀ / ▶ pair uses: the key it had on this screen
                // ([Y]) now walks the views, so the deck carries it - the
                // same solution the quest screen's step chip uses.
                let step_lbl = format!("刻み ±{}", ed.step_value());
                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, &step_lbl, t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::SetStep(next_step));
            }
            Screen::EntryView => {
                let step_lbl = format!("刻み ±{}", ed.step_value());
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, &step_lbl, t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::SetStep(next_step));

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                let sem = ed.sem_preset_label(0);
                btn_text(c, 1108, sy, 158, sh, &sem, t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::SemPreset(0));
            }
            // ---- the keys a finger had no way to reach ------------------
            //
            // These four are pad-only operations: nothing on screen ran them,
            // and a touch-only player could not use them at all. Each is one
            // pill in the deck, and each sends the same Key the pad sends, so
            // there is one implementation of the action and not two.
            Screen::Hunts => {
                // 狩猟記録: [Y] walks the four numbers of a monster.
                let step_lbl = if ed.step_value() < 0 {
                    String::from("刻み 反転")
                } else {
                    format!("刻み ±{}", ed.step_value())
                };
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, &step_lbl, t.text_secondary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::Key(Key::A));

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, "列切替", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::Key(Key::Y));
            }
            // 進行状況: the search is [ZR] on every view, and the pill sends the
            // same key ([A] toggles the bit, [Y] walks the views, [R] is the
            // quest view's second bit, the four directions move and [L] is the
            // tab). The quest view's [R] (既読) keeps its pill beside it; on the
            // other two views that key walks the views, which the tab bar above
            // already does for a finger.
            Screen::Progress => {
                let label = if ed.progress_filter.trim().is_empty() {
                    String::from("検索")
                } else {
                    String::from(ed.progress_filter.trim())
                };
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, &label, t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::FilterProgress);
                // 既読 is the quest view's second bit. [Y] moves the tab on all
                // three views, so the write has a key of its own: [R], which
                // the console takes back from the sub-tab rule on this view
                // (hbedit::Editor::shoulder_job). The pill names it and sends
                // the same method, so a finger and the pad write one bit.
                if ed.progress_view() == PROGRESS_QUESTS {
                    c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                    c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                    // The press is a toggle, so the label follows the bit: it
                    // said "既読にする" on a row that was already seen and the
                    // press then took the bit off. The row is the state
                    // machine's answer (Editor::progress_seen_label), the same
                    // one the press writes.
                    let seen = String::from(ed.progress_seen_label());
                    btn_text(c, 1108, sy, 158, sh, &seen, t.text_primary, FontSize::Small);
                    self.add_hit(1108, sy, 158, sh, UiAction::ProgressSeen);
                }
            }
            // 環境設定: [Y] puts the newest snapshot back. It is the only
            // destructive tool on the screen and the only one a finger had no
            // route to; the label says 復元 because the review dialog that
            // follows is the one that spells the rest out.
            Screen::Options => {
                c.round_rect(964, sy, 302, sh, 5, t.bg_btn_primary);
                c.round_rect_border(964, sy, 302, sh, 5, 1, t.accent_primary);
                btn_text(
                    c,
                    964,
                    sy,
                    302,
                    sh,
                    "スナップショットへ戻す",
                    t.text_primary,
                    FontSize::Small,
                );
                self.add_hit(964, sy, 302, sh, UiAction::Key(Key::Y));
            }
            // The four pick lists: [Y] 検索, [-] 種別 (the name picker only)
            // and [X] クリア on the bar, because the deck is drawn under a row
            // and a search that matches nothing has none - which is exactly
            // when the clear key is needed. The deck repeats them for the case
            // that has a row.
            Screen::ItemPick | Screen::SkillPick | Screen::JewelPick | Screen::EquipPick => {
                if ed.screen == Screen::EquipPick {
                    // Three pills in 302px, sized to their labels: 94 + 6 + 96
                    // + 6 + 100 = 302. "[X] クリア" is 87px and had 82 - the
                    // one pill of the three that drew as "[X] クリ..".
                    c.round_rect(964, sy, 94, sh, 5, t.bg_btn);
                    c.round_rect_border(964, sy, 94, sh, 5, 1, t.border_subtle);
                    btn_text(c, 964, sy, 94, sh, "検索", t.text_primary, FontSize::Small);
                    self.add_hit(964, sy, 94, sh, UiAction::Key(Key::Y));

                    c.round_rect(1064, sy, 96, sh, 5, t.bg_btn);
                    c.round_rect_border(1064, sy, 96, sh, 5, 1, t.border_subtle);
                    btn_text(c, 1064, sy, 96, sh, "種別", t.text_secondary, FontSize::Small);
                    self.add_hit(1064, sy, 96, sh, UiAction::Key(Key::Z));

                    c.round_rect(1166, sy, 100, sh, 5, t.bg_btn_primary);
                    c.round_rect_border(1166, sy, 100, sh, 5, 1, t.accent_primary);
                    btn_text(c, 1166, sy, 100, sh, "クリア", t.text_primary, FontSize::Small);
                    self.add_hit(1166, sy, 100, sh, UiAction::Key(Key::X));
                } else {
                    c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                    c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                    btn_text(c, 964, sy, 136, sh, "検索", t.text_primary, FontSize::Small);
                    self.add_hit(964, sy, 136, sh, UiAction::Key(Key::Y));

                    c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                    c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                    btn_text(c, 1108, sy, 158, sh, "クリア", t.text_primary, FontSize::Small);
                    self.add_hit(1108, sy, 158, sh, UiAction::Key(Key::X));
                }
            }
            // CSV 入出力: the two keys the screen has, on the bar rather than
            // only in the deck, because the deck is drawn under a row and an
            // empty folder has none - and writing the first CSV out is exactly
            // what an empty folder needs.
            Screen::Csv => {
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, "書き出し", t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::Key(Key::Y));

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, "再読込", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::Key(Key::Z));
            }
            // セーブ内クエスト / 公式 / 没: [Y] is the search box.
            Screen::SaveQuests => {
                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, "検索", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::Key(Key::Y));
            }
            // 生データ: the list is about a hundred named stretches, three of
            // them the same box for the other two characters. [Y] narrows it by
            // name or offset; the pill says so and, while a filter is on, names
            // it rather than leaving the shortened list unexplained.
            Screen::SaveMap if !ed.map_edit_open() => {
                let label = if ed.map_filter.trim().is_empty() {
                    String::from("領域を検索")
                } else {
                    format!("「{}」", ed.map_filter)
                };
                c.round_rect(964, sy, 302, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 302, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 302, sh, &label, t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 302, sh, UiAction::Key(Key::Y));
            }
            // 変更一覧: [Y] re-walks the values. The list is built once, so the
            // key is the one way to compare again after an edit made elsewhere.
            Screen::Review => {
                c.round_rect(964, sy, 302, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 302, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 302, sh, "一覧を作り直す", t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 302, sh, UiAction::Key(Key::Y));
            }
            _ => {}
        }
    }

    /// The footer is two lines and only two: the keys on the first, the status
    /// message on the second.
    ///
    /// Line 1: the whole-save undo pill on the left (how many steps are left of
    /// the four, and the two routes that reach it, because it is a tap target
    /// as well as a key), the 終了 pill on the right, and the screen's key guide
    /// between them. Line 2: the dirty dot, the message, and the input rules
    /// that are invisible until they bite - touch, drag and the right stick.
    fn draw_footer(&mut self, c: &mut Canvas, ed: &Editor) {
        let t = theme();
        c.rect(0, FOOTER_Y, 1280, FOOTER_H, t.bg_footer);
        c.hline(0, FOOTER_Y, 1280, t.border_subtle);

        // Line 1, left: the whole-save undo. The key is L held down; a plain L
        // is the sub-tab key everywhere.
        // Two holds run the undo (the first one only offers it), so the pill
        // says how many: a single "long press" would be a promise the second
        // hold does not keep.
        // The pill is a touch target, so its label has to say that a tap runs
        // it: it used to read "[L 長押し2回]" on the one control a finger was
        // most likely to press, which is a key name a finger cannot use. The
        // two routes are the same two presses either way.
        // The label is the state machine's (hbedit::Editor::undo_pill_label):
        // on the four pick lists a held [L] is the ten-row step, not the undo,
        // and the pill promised it anyway. The tap route is the same on every
        // screen, so only the key half of the label changes.
        let undo = ed.undo_pill_label();
        let undo_w = (c.text_width_sized(&undo, FontSize::Small) + 22).min(470);
        let has_undo = ed.undo_depth() > 0;
        let (undo_bg, undo_fg) = if has_undo {
            (t.bg_btn_primary, t.text_primary)
        } else {
            (t.bg_btn, t.text_muted)
        };
        let undo_bd = if has_undo { t.accent_primary } else { t.border_subtle };
        c.round_rect(14, FOOTER_Y + 4, undo_w, 26, 5, undo_bg);
        c.round_rect_border(14, FOOTER_Y + 4, undo_w, 26, 5, 1, undo_bd);
        c.text_clipped(24, FOOTER_Y + 9, undo_w - 12, &undo, undo_fg, FontSize::Small);
        self.add_hit(14, FOOTER_Y + 2, undo_w, 30, UiAction::Undo);

        // Line 1, right: the key guide, built by the state machine
        // (hbedit::Editor::footer_hint). A key guide is a claim about what the
        // keys do, so the one place that dispatches them is the one place that
        // writes it. It is measured before it is drawn: a guide longer than the
        // room the pill leaves is clipped at the pill's edge instead of being
        // drawn over it, which is what a bare right-aligned draw did.
        let hint = ed.footer_hint();
        let hint_left = 14 + undo_w + 12;
        // The way out, as a button. [＋] ends the session on every screen, and
        // it was the one operation a finger could not reach at all: the pad's
        // key is not on the glass. It sits at the right end of the key line,
        // where nothing else is drawn, and it runs the same two-press quit the
        // key does (a dirty save still asks first).
        //
        // Both numbers come from hbedit (footer_quit_x / footer_guide_right) so
        // the guide's right edge cannot be the pill's right edge again: it was,
        // and every hint short enough to fit beside the pill was still drawn
        // under it.
        let quit_w = mhnx_core::hbedit::FOOTER_QUIT_W;
        let quit_x = footer_quit_x();
        c.round_rect(quit_x, FOOTER_Y + 4, quit_w, 26, 5, t.bg_btn);
        c.round_rect_border(quit_x, FOOTER_Y + 4, quit_w, 26, 5, 1, t.border_subtle);
        c.text_centered(
            quit_x,
            FOOTER_Y + 8,
            quit_w,
            22,
            "終了",
            t.text_primary,
            FontSize::Small,
        );
        self.add_hit(quit_x, FOOTER_Y + 2, quit_w, 30, UiAction::Quit);
        let guide_right = footer_guide_right();
        let hint_room = guide_right - hint_left;
        let hint_w = c.text_width_sized(&hint, FontSize::Small);
        if hint_room > 120 {
            if hint_w <= hint_room {
                // Right aligned at the guide's own right edge, which stops
                // before the 終了 pill. It used to be the bar's edge (1262) -
                // the pill's right edge too - so the two overlapped.
                c.text_right(guide_right, FOOTER_Y + 9, &hint, t.text_secondary, FontSize::Small);
            } else {
                c.text_clipped(
                    hint_left,
                    FOOTER_Y + 9,
                    hint_room,
                    &hint,
                    t.text_secondary,
                    FontSize::Small,
                );
            }
        }

        // Line 2: the status message. It is clipped before the touch rule so
        // the two can never overlap, whatever the message says.
        let msg = if ed.message.is_empty() {
            "Ready"
        } else {
            &ed.message
        };
        // Touch has one rule that is invisible until it bites, and it is not
        // the one this line used to state. A button - every pill, every deck
        // control, the footer's undo and 終了 - runs on one tap; only a *row*
        // works the way the old sentence described, and only while it is not
        // the selected one - so the old sentence was wrong about most of the
        // screen. The right stick rides along because the list header
        // could drop it: the header only draws its note while it fits beside
        // the counter and the filter chip, so a long heading was enough to hide
        // the only words about the stick.
        let tap_note =
            "[タップ] ボタン=即実行 / 行=選択→再タップ  [Rスティック] ▲▼=1頁 ◀▶=端";
        let tap_w = c.text_width_sized(tap_note, FontSize::Small);
        c.text_right(1262, FOOTER_Y + 30, tap_note, t.text_muted, FontSize::Small);
        let max_msg_w = (1262 - tap_w - 60).max(160);
        c.circle(22, FOOTER_Y + 38, 4, if ed.dirty { t.accent_warn } else { t.accent_primary });
        c.text_clipped(36, FOOTER_Y + 30, max_msg_w, msg, t.text_primary, FontSize::Small);
    }

    /// The save review: what a write would change, and the two answers.
    ///
    /// Modal on purpose. The first press of the save key gets here instead of
    /// writing, so a write that moves 400 bytes of somebody's equipment box is
    /// something the user has read before it happens.
    ///
    /// The body scrolls. It used to be capped at the panel's edge with a
    /// "ほか N 行" line and no way to reach them, so the values the dialog
    /// exists to show were the part nobody could read; [▲] / [▼] (or the two
    /// buttons in its footer) now move the window, and the scrollbar on the
    /// right says how much of the body is on screen.
    fn draw_review(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(review) = ed.review.as_ref() else {
            return;
        };
        let t = theme();
        c.rect(0, 0, 1280, 720, rgba(0, 0, 0, 170));

        let mx = 140;
        let my = 54;
        let mw = 1000;
        let mh = 612;
        c.round_rect(mx, my, mw, mh, 12, t.bg_panel);
        c.round_rect_border(mx, my, mw, mh, 12, 2, t.border_focus);

        // The dialog answers two different writes now: the save key's, and the
        // snapshot restore the options screen starts. The title, the rule line
        // and the button all say which one is waiting.
        let restoring = self.pending_snapshot.is_some();
        c.text_clipped(
            mx + 26,
            my + 14,
            mw - 500,
            if restoring {
                "スナップショット復元の確認: 変更内容"
            } else {
                "保存前レビュー: 変更内容"
            },
            t.text_primary,
            FontSize::Large,
        );
        c.text_right(
            mx + mw - 26,
            my + 20,
            &review.headline(),
            t.accent_warn,
            FontSize::Small,
        );
        c.hline(mx + 20, my + 52, mw - 40, t.border_subtle);

        // The body is now summary + values + ranges, and the values alone can be
        // thirty lines (hbedit::SaveReview, 上限は diffvalues::VALUE_SHOWN).
        // The panel holds a fixed number of rows, so the walk stops on the
        // footer's rule and says how many rows it folded away: a fixed take(17)
        // would have cut the "ほか N 件" line off and hidden the ranges with no
        // word. Geometry, not a guess: a glyph is 17px tall and a row is 23px
        // (an empty spacer row is 10px), the body starts at my + 64 and the
        // footer rule sits at foot_y - 12, so the last row drawn here ends at
        // most 7px above the rule and the two can never overlap.
        // The body is the 22px row font now, not the 17px caption one: this
        // dialog exists to be read before a write, and it was the smallest type
        // on the screen. The row pitch follows the glyph (22 + 6), so a line
        // still has air around it; the window shows four fewer lines than it
        // did (16 instead of 20) and the body scrolls, which is what the
        // scrollbar and the ほか N 行 line are for.
        const ROW: i32 = 28;
        const SPACER: i32 = 12;
        const GLYPH: i32 = 22;
        let foot_y = my + mh - 56;
        let body_top = my + 64;
        let body_bottom = foot_y - 20;
        let total = review.lines.len();
        // The body scrolls. It used to stop at the panel's edge and say
        // "ほか N 行" with no way to reach them, so the values the dialog exists
        // to show - thirty of them, at worst - were the part nobody could read.
        let start = review.scroll_to(self.review_scroll);
        let mut y = body_top;
        let mut last = start;
        for (i, line) in review.lines.iter().enumerate().skip(start) {
            // Keep one row free for the count while lines remain after this
            // one, so the count is never the line that falls off.
            let reserve = if i + 1 < total { ROW } else { 0 };
            if y + GLYPH + reserve > body_bottom {
                break;
            }
            if line.is_empty() {
                y += SPACER;
                last = i + 1;
                continue;
            }
            let color = if line.starts_with('■') {
                t.accent_primary
            } else {
                t.text_primary
            };
            c.text_clipped(mx + 30, y, mw - 60, line, color, FontSize::Medium);
            y += ROW;
            last = i + 1;
        }
        // Where the window is, in the one line the body has left: both halves
        // matter, because the reader has to know that the headline is above
        // them as well as that the ranges are below.
        let above = start;
        let below = total.saturating_sub(last);
        if (above > 0 || below > 0) && y + GLYPH <= body_bottom {
            let mut note = String::new();
            if above > 0 {
                note.push_str(&format!("▲ 上に {} 行", above));
            }
            if below > 0 {
                if !note.is_empty() {
                    note.push_str(" / ");
                }
                note.push_str(&format!("▼ 下に {} 行", below));
            }
            note.push_str("（[▲/▼] 1行・[◀/▶] 10行・下のボタン）");
            c.text_clipped(mx + 30, y, mw - 60, &note, t.text_secondary, FontSize::Small);
        }
        // The scrollbar, in the panel's right-hand padding: a thumb that shows
        // how much of the body is on screen, drawn only when there is more.
        if total > 1 {
            let track_x = mx + mw - 14;
            let track_h = body_bottom - body_top;
            c.rect(track_x, body_top, 4, track_h, t.bg_val_box);
            let visible = (last - start).max(1);
            let thumb_h = ((track_h as i64 * visible as i64) / total as i64).max(18) as i32;
            let span = (track_h - thumb_h).max(0);
            let denom = (total - visible).max(1) as i64;
            let thumb_y = body_top + ((span as i64 * start as i64) / denom) as i32;
            c.round_rect(track_x, thumb_y, 4, thumb_h, 2, t.accent_primary);
        }

        c.hline(mx + 20, foot_y - 12, mw - 40, t.border_subtle);
        // The two scroll buttons sit left of the two answers, and the rule line
        // between them is shortened to fit: the words that used to run the
        // whole width now stop before the [B] button.
        let scroll_w = 46;
        let scroll_y = foot_y + 2;
        let up_x = mx + 30;
        let down_x = up_x + scroll_w + 8;
        let can_up = start > 0;
        let can_down = below > 0;
        for (x, label, delta, live) in [
            (up_x, "▲", -1i32, can_up),
            (down_x, "▼", 1i32, can_down),
        ] {
            let (bg, fg) = if live {
                (t.bg_btn, t.text_primary)
            } else {
                (t.bg_panel, t.text_muted)
            };
            c.round_rect(x, scroll_y, scroll_w, 38, 5, bg);
            c.round_rect_border(x, scroll_y, scroll_w, 38, 5, 1, t.border_subtle);
            btn_text(c, x, scroll_y, scroll_w, 38, label, fg, FontSize::Medium);
            if live {
                self.add_hit(x, scroll_y, scroll_w, 38, UiAction::ReviewScroll(delta));
            }
        }
        c.text_clipped(
            down_x + scroll_w + 16,
            foot_y + 12,
            (mx + mw - 26 - 190 - 12 - 150) - (down_x + scroll_w + 16),
            if restoring {
                "1回目は確認だけ・2回目で戻す"
            } else {
                "1回目は確認だけ・2回目で保存"
            },
            t.text_secondary,
            FontSize::Small,
        );

        let a_w = 190;
        let a_x = mx + mw - 26 - a_w;
        c.round_rect(a_x, foot_y, a_w, 42, 6, t.accent_primary);
        btn_text(
            c,
            a_x,
            foot_y,
            a_w,
            42,
            if restoring {
                "[A] この内容で戻す"
            } else {
                "[A] この内容で保存"
            },
            white_or_dark(t.accent_primary),
            FontSize::Small,
        );
        self.add_hit(a_x, foot_y, a_w, 42, UiAction::CommitReview);

        let b_w = 150;
        let b_x = a_x - 12 - b_w;
        c.round_rect(b_x, foot_y, b_w, 42, 6, t.bg_btn);
        c.round_rect_border(b_x, foot_y, b_w, 42, 6, 1, t.border_subtle);
        btn_text(c, b_x, foot_y, b_w, 42, "[B] 中止", t.text_primary, FontSize::Small);
        self.add_hit(b_x, foot_y, b_w, 42, UiAction::CancelReview);
    }

    // -----------------------------------------------------------------------
    // Main Content Dispatcher
    // -----------------------------------------------------------------------
    fn draw_content(&mut self, c: &mut Canvas, ed: &Editor) {
        match ed.screen {
            Screen::SaveList => self.draw_save_list(c, ed),
            Screen::Slots => self.draw_slots(c, ed),
            Screen::Convert => self.draw_convert(c, ed),
            Screen::Options => self.draw_options(c, ed),
            Screen::Char => self.draw_char(c, ed),
            Screen::Items => self.draw_items(c, ed),
            Screen::ItemPick => self.draw_item_pick(c, ed),
            Screen::SkillPick => self.draw_skill_pick(c, ed),
            Screen::Charms => self.draw_charms(c, ed),
            Screen::CharmEdit => self.draw_charm_edit(c, ed),
            Screen::Equip => self.draw_equip(c, ed),
            Screen::EquipEdit => self.draw_equip_edit(c, ed),
            Screen::JewelPick => self.draw_jewel_pick(c, ed),
            Screen::EquipPick => self.draw_equip_pick(c, ed),
            Screen::Sets => self.draw_sets(c, ed),
            Screen::Csv => self.draw_csv(c, ed),
            // The palico equipment panel is a mode of the two palico screens,
            // so it is dispatched before the screens themselves.
            Screen::Roster if ed.palico_skill_open() => self.draw_palico_skill(c, ed),
            Screen::Roster | Screen::Palico if ed.palico_equip_open() => {
                self.draw_palico_equip(c, ed)
            }
            Screen::Roster => self.draw_roster(c, ed),
            Screen::Palico => self.draw_palico(c, ed),
            Screen::Hunts => self.draw_hunts(c, ed),
            Screen::Guild if ed.guild_view() == GUILD_TAB_ARENA => self.draw_arena(c, ed),
            Screen::Guild if ed.guild_view() == GUILD_TAB_COUNTS => self.draw_guild_counts(c, ed),
            Screen::Guild if ed.guild_view() == GUILD_TAB_WEAPONS => {
                self.draw_guild_weapons(c, ed)
            }
            Screen::Guild => self.draw_guild(c, ed),
            Screen::Progress => self.draw_progress(c, ed),
            Screen::SaveQuests => self.draw_save_quests(c, ed),
            Screen::QuestEquip => self.draw_quest_equip(c, ed),
            Screen::QuestEdit => self.draw_quest_edit(c, ed),
            Screen::ArcList => self.draw_arc_list(c, ed),
            Screen::ArcEntries => self.draw_arc_entries(c, ed),
            Screen::Mib => self.draw_mib(c, ed),
            Screen::Gmd => self.draw_gmd(c, ed),
            Screen::EntryView => self.draw_entry_view(c, ed),
            Screen::Check => self.draw_check(c, ed),
            Screen::SaveMap => self.draw_save_map(c, ed),
            Screen::Review => self.draw_review_screen(c, ed),
        }
    }

    /// 生データ (Screen::SaveMap): the save map's rows on the left, the selected
    /// row's bytes on the right - and, behind [A], the byte editor.
    ///
    /// The screen answers in two modes and says which one it is in: the list
    /// view (a cursor, and [A] opens the editor) and the editor ([▲/▼] on a
    /// byte, [◀/▶] on its value, [A] confirms, [B] cancels). The panel's two
    /// buttons carry the same two keys, so a finger has both - a finger had no
    /// way out of here at all before the back button existed, and no way into
    /// the editor before it.
    ///
    /// The split is the one every other screen uses (796 / 428). It used to be
    /// 600 / 626 so a 16 byte hex line would fit whole, which made the
    /// inspector wider than the list - the one screen whose shape said
    /// "different program". The dump is cut to 8 bytes a line instead
    /// (hbedit::rewrap_hex): same bytes, same offsets, in the panel the rest of
    /// the tool has trained the eye on. The editor's window follows the caret
    /// and brackets the byte it is on (hbedit::Editor::map_edit_dump).
    fn draw_save_map(&mut self, c: &mut Canvas, ed: &Editor) {
        let t = theme();
        let rows = ed.save_map_rows();
        let total = rows.len();
        let editing = ed.map_edit_open();

        c.text_clipped(
            MAIN_X + 4,
            CONTENT_Y,
            620,
            "生データ",
            t.text_primary,
            FontSize::Medium,
        );
        c.text_right(
            1262,
            CONTENT_Y + 3,
            // 閲覧＋編集: the right panel is a hex dump *and* the byte editor
            // [A] opens. It said 読み取り専用 while the screen could only read -
            // the word was true then and is a lie now.
            &format!(
                "スロット {} / {} 行 / 閲覧＋編集{}",
                ed.slot + 1,
                total,
                if editing { "（編集中）" } else { "" }
            ),
            t.text_secondary,
            FontSize::Small,
        );

        let body_y = CONTENT_Y + 30;
        let body_h = FOOTER_Y - body_y - 10;
        const ROW_H: i32 = 50;
        // The window is the shared arithmetic, not a typed 9: the row count
        // follows the body height and the measured caption height, so a font
        // change moves it instead of drawing the last row over the panel's
        // edge (hbedit::list_window_rows; the strip is empty on this screen).
        let line_h = c.line_height_sized(FontSize::Small).max(16);
        let vis = mhnx_core::hbedit::list_window_rows(body_h, ROW_H, 0, line_h);
        let list_w = V1_LIST_W;
        let insp_x = V1_INSP_X; // 832
        let insp_w = V1_INSP_W; // 428
        let start = window_start(ed.map_cursor, total, vis);
        // The same 40px rule as the wide list: the row's tap area and its
        // right-hand text stop before the bar's touch area.
        let track = scroll_track(MAIN_X, list_w);

        // ---- left: the named stretches of the save
        c.round_rect(MAIN_X, body_y, list_w, body_h, t.corner_r, t.bg_panel);
        c.round_rect_border(MAIN_X, body_y, list_w, body_h, t.corner_r, 1, t.border_subtle);
        c.hline(MAIN_X + 12, body_y + 30, list_w - 24, t.border_subtle);

        for i in 0..vis {
            let pos = start + i;
            if pos >= total {
                break;
            }
            let idx = rows[pos];
            let ry = body_y + 34 + (i as i32) * ROW_H;
            let selected = pos == ed.map_cursor;
            let bg = if selected {
                t.bg_selected
            } else if i % 2 == 1 {
                t.bg_row_odd
            } else {
                t.bg_row_even
            };
            c.round_rect(MAIN_X + 8, ry, list_w - 16, ROW_H - 4, 5, bg);
            self.add_hit(
                MAIN_X + 8,
                ry,
                track.row_right - (MAIN_X + 8),
                ROW_H - 4,
                UiAction::SelectRow(pos),
            );
            if selected {
                c.rect(MAIN_X + 8, ry + 6, 3, ROW_H - 16, t.accent_primary);
            }
            let title_col = if selected { t.text_primary } else { t.text_secondary };
            // The name leads and the offset follows it: a reader looks for
            // 装備ボックス before whatever lives at 0x62EE.
            c.text_clipped(
                MAIN_X + 20,
                ry + 12,
                300,
                ed.save_map_row_name(idx),
                title_col,
                FontSize::Medium,
            );
            let at_text = match ed.save_map_row_offset(idx) {
                Some(o) => format!("+0x{:06X}", o),
                None => String::from("このセーブに無し"),
            };
            c.text_clipped(
                MAIN_X + 330,
                ry + 15,
                150,
                &at_text,
                t.text_muted,
                FontSize::Small,
            );
            c.text_right(
                track.text_right - 56,
                ry + 15,
                &format!("{}B", ed.save_map_row_len(idx)),
                t.text_secondary,
                FontSize::Small,
            );
            // The row being edited is marked in the list as well as in the
            // panel: "編集中" is the one fact a user scrolls away from.
            let badge = if idx == ed.save_map_selected().unwrap_or(usize::MAX) && editing {
                String::from("編集中")
            } else {
                format!("#{:03}", pos + 1)
            };
            c.text_right(
                track.text_right,
                ry + 15,
                &badge,
                if editing && badge == "編集中" {
                    t.accent_warn
                } else {
                    t.text_muted
                },
                FontSize::Small,
            );
        }
        if total == 0 {
            c.text_clipped(
                MAIN_X + 20,
                body_y + 44,
                list_w - 40,
                "このセーブに並べられる領域がありません",
                t.text_muted,
                FontSize::Small,
            );
        }
        let ids: Vec<usize> = rows.iter().enumerate().map(|(i, _)| i).collect();
        self.draw_list_bar(
            c,
            ed,
            MAIN_X,
            list_w,
            body_y + 34,
            (vis as i32) * ROW_H,
            ScrollBar::new(total, vis, start),
            &ids,
            false,
        );

        // ---- right: what that row is, and its bytes
        c.round_rect(insp_x, body_y, insp_w, body_h, t.corner_r, t.bg_panel);
        c.round_rect_border(insp_x, body_y, insp_w, body_h, t.corner_r, 1, t.border_subtle);

        // The two controls the panel owns, drawn before anything that can be
        // missing: they belong to the panel and not to the selected row, so a
        // save whose rows no longer resolve still offers a way out - and the
        // editor is the whole reason the pill is on every screen.
        //
        // [A] is one button with one meaning at a time: open the editor, write
        // what it asked for, or close it. [B] is the other half: it drops a
        // change that is waiting before it drops the screen, which is the
        // "start / end" shape the arena editor has.
        let pending = ed.map_edit.and_then(|e| e.pending);
        let btn_w = (insp_w - 48) / 2;
        let btn_y = body_y + body_h - 44;
        let a_x = insp_x + 20;
        let b_x = a_x + btn_w + 8;
        let a_label = if pending.is_some() {
            "[A] 決定"
        } else if editing {
            "編集終了"
        } else {
            "[A] 編集する"
        };
        let (a_bg, a_fg) = if pending.is_some() {
            (t.accent_warn, white_or_dark(t.accent_warn))
        } else if editing {
            (t.bg_btn, t.accent_primary)
        } else {
            (t.bg_btn_primary, white_or_dark(t.bg_btn_primary))
        };
        c.round_rect(a_x, btn_y, btn_w, 34, 5, a_bg);
        c.round_rect_border(a_x, btn_y, btn_w, 34, 5, 1, t.accent_primary);
        btn_text(c, a_x, btn_y, btn_w, 34, a_label, a_fg, FontSize::Small);
        self.add_hit(a_x, btn_y, btn_w, 34, UiAction::Key(Key::A));

        let b_label = if pending.is_some() {
            "[B] 取消"
        } else {
            "[B] 元の画面へ"
        };
        c.round_rect(b_x, btn_y, btn_w, 34, 5, t.bg_btn);
        c.round_rect_border(b_x, btn_y, btn_w, 34, 5, 1, t.border_subtle);
        btn_text(c, b_x, btn_y, btn_w, 34, b_label, t.text_primary, FontSize::Small);
        self.add_hit(b_x, btn_y, btn_w, 34, UiAction::Key(Key::B));

        // The byte editor's four directions, as buttons. They were the one
        // part of this screen a finger could not reach at all: the panel had
        // [A] and [B] and nothing that moved the caret or changed the value,
        // so a touch-only player could open the editor and do nothing in it.
        // Each sends the same Key the pad sends, so there is one implementation
        // of the step and not two.
        if editing {
            let step_y = btn_y - 44;
            let gap = 6i32;
            let sw = (insp_w - 40 - gap * 3) / 4;
            let labels = ["▲ 前", "▼ 次", "◀ -1", "▶ +1"];
            let keys = [Key::Up, Key::Down, Key::Left, Key::Right];
            for (i, (label, key)) in labels.iter().zip(keys.iter()).enumerate() {
                let x = insp_x + 20 + (i as i32) * (sw + gap);
                c.round_rect(x, step_y, sw, 34, 5, t.bg_btn);
                c.round_rect_border(x, step_y, sw, 34, 5, 1, t.border_subtle);
                btn_text(c, x, step_y, sw, 34, label, t.text_primary, FontSize::Small);
                self.add_hit(x, step_y, sw, 34, UiAction::Key(*key));
            }
        }

        let Some(selected) = ed.save_map_selected() else {
            c.text_clipped(
                insp_x + 20,
                body_y + 24,
                insp_w - 40,
                "行を選んでください",
                t.text_muted,
                FontSize::Small,
            );
            return;
        };
        c.text_clipped(
            insp_x + 20,
            body_y + 18,
            insp_w - 40,
            ed.save_map_row_name(selected),
            t.text_primary,
            FontSize::Medium,
        );
        c.text_clipped(
            insp_x + 20,
            body_y + 50,
            insp_w - 40,
            &ed.save_map_meta(),
            t.accent_secondary,
            FontSize::Small,
        );
        if let Some(value) = ed.save_map_value() {
            if !value.is_empty() {
                c.text_clipped(
                    insp_x + 20,
                    body_y + 74,
                    insp_w - 40,
                    &format!("値: {}", value),
                    t.text_secondary,
                    FontSize::Small,
                );
            }
        }
        c.hline(insp_x + 16, body_y + 100, insp_w - 32, t.border_subtle);

        // The dump: 8 bytes a line. 16 was what the wider panel could hold;
        // the inspector here is the standard 428 and 57 characters do not fit
        // it. The bytes and their offsets are untouched (hbedit::rewrap_hex).
        // In edit mode the window follows the caret and brackets the byte it
        // is on, so a 72,000 byte row is not always showing its first line.
        let line_h = c.line_height_sized(FontSize::Small).max(18);
        // The two buttons own the bottom 46px, and the lines above them belong
        // to the caret (edit mode) and the structural-byte warning (edit mode)
        // or the row counter (view mode). The dump stops above whichever of
        // those is drawn, so no hex line can land on the words that explain it.
        // Edit mode gives up two more lines than it used to: the four step
        // buttons and the two lines that explain the caret now sit between the
        // dump and the [A] / [B] pair.
        let dump_bottom = if editing { body_h - 140 } else { body_h - 70 };
        let room = ((dump_bottom - 112) / line_h.max(1)).max(1) as usize;
        let dump: Vec<String> = if editing {
            ed.map_edit_dump(room)
        } else {
            mhnx_core::hbedit::Editor::rewrap_hex(&ed.save_map_dump(), 8)
                .into_iter()
                .take(room)
                .collect()
        };
        if dump.is_empty() {
            c.text_clipped(
                insp_x + 20,
                body_y + 112,
                insp_w - 40,
                "このセーブにこの領域はありません",
                t.text_muted,
                FontSize::Small,
            );
        } else {
            for (i, line) in dump.iter().enumerate() {
                let caret_here = editing && line.contains('[');
                c.text_clipped(
                    insp_x + 20,
                    body_y + 112 + (i as i32) * line_h,
                    insp_w - 40,
                    line,
                    if caret_here { t.accent_warn } else { t.text_primary },
                    FontSize::Small,
                );
            }
        }

        // Line under the dump: where the caret is (edit mode) or where the
        // list is, and the structural-byte warning when there is one. The
        // warning is amber and never a refusal: the task asks the editor to say
        // what it is doing to a byte the format treats as structure.
        if editing {
            c.text_clipped(
                insp_x + 20,
                body_y + body_h - 108,
                insp_w - 40,
                &ed.map_edit_position(),
                t.text_secondary,
                FontSize::Small,
            );
            if let Some(warning) = ed.map_byte_warning() {
                c.text_clipped(
                    insp_x + 20,
                    body_y + body_h - 128,
                    insp_w - 40,
                    &format!("警告: {}", warning),
                    t.accent_warn,
                    FontSize::Small,
                );
            }
        } else {
            c.text_clipped(
                insp_x + 20,
                body_y + body_h - 70,
                insp_w - 40,
                &format!("{} / {} 行（[A] でこの領域を編集）", ed.map_cursor + 1, total),
                t.text_muted,
                FontSize::Small,
            );
        }
    }

    // -----------------------------------------------------------------------
    /// 変更一覧 (Screen::Review): the values the save differs from the card in,
    /// one line each, with the one-item undo the pre-save dialog does not offer.
    ///
    /// The layout is the split every other screen uses (796 / 428). The list
    /// prints the whole change ("スロット1 名前: ナナ → 差分テスト") so a value
    /// is readable without opening anything; the panel repeats the selected line,
    /// says which bytes it would put back - or why it cannot - and carries the
    /// three keys: 戻す / 作り直す / 戻る. Everything the rows say comes from
    /// mhnx_core::hbedit (ReviewRow), so the list and the pre-save review cannot
    /// word the same change two ways.
    fn draw_review_screen(&mut self, c: &mut Canvas, ed: &Editor) {
        let t = theme();
        let total = ed.review_len();
        let armed = ed.review_armed_row();
        let selected = ed.review_selected();

        c.text_clipped(
            MAIN_X + 4,
            CONTENT_Y,
            420,
            "変更一覧",
            t.text_primary,
            FontSize::Medium,
        );
        c.text_right(
            1262,
            CONTENT_Y + 3,
            &ed.review_heading(),
            t.text_secondary,
            FontSize::Small,
        );

        let body_y = CONTENT_Y + 30;
        let body_h = FOOTER_Y - body_y - 10;
        const ROW_H: i32 = 50;
        const VIS: usize = mhnx_core::hbedit::REVIEW_VISIBLE;
        let list_w = V1_LIST_W;
        let insp_x = V1_INSP_X;
        let insp_w = V1_INSP_W;
        let start = window_start(ed.rev_cursor, total, VIS);
        let track = scroll_track(MAIN_X, list_w);

        // ---- left: one line per value
        c.round_rect(MAIN_X, body_y, list_w, body_h, t.corner_r, t.bg_panel);
        c.round_rect_border(MAIN_X, body_y, list_w, body_h, t.corner_r, 1, t.border_subtle);
        c.hline(MAIN_X + 12, body_y + 30, list_w - 24, t.border_subtle);

        for i in 0..VIS {
            let pos = start + i;
            if pos >= total {
                break;
            }
            let ry = body_y + 34 + (i as i32) * ROW_H;
            let sel = pos == ed.rev_cursor;
            let bg = if sel {
                t.bg_selected
            } else if i % 2 == 1 {
                t.bg_row_odd
            } else {
                t.bg_row_even
            };
            c.round_rect(MAIN_X + 8, ry, list_w - 16, ROW_H - 4, 5, bg);
            self.add_hit(
                MAIN_X + 8,
                ry,
                track.row_right - (MAIN_X + 8),
                ROW_H - 4,
                UiAction::SelectRow(pos),
            );
            if sel {
                c.rect(MAIN_X + 8, ry + 6, 3, ROW_H - 16, t.accent_primary);
            }
            c.text_clipped(
                MAIN_X + 20,
                ry + 6,
                56,
                &format!("#{:03}", pos + 1),
                t.text_muted,
                FontSize::Small,
            );
            let col = if sel { t.text_primary } else { t.text_secondary };
            // The badge on the right is a fact about the line, not a key: a
            // value whose bytes are shared with another cannot be put back on
            // its own, and saying so here is what stops a press that refuses.
            let badge_w = 104;
            c.text_clipped(
                MAIN_X + 80,
                ry + 4,
                track.text_right - (MAIN_X + 80) - badge_w,
                &ed.review_line(pos),
                col,
                FontSize::Small,
            );
            let (badge, bcol) = if !ed.review_can_restore(pos) {
                (String::from("戻せません"), t.text_muted)
            } else if armed == Some(pos) {
                (String::from("[A] 2回目"), t.accent_warn)
            } else {
                (String::from("戻せます"), t.accent_secondary)
            };
            c.text_right(track.text_right, ry + 15, &badge, bcol, FontSize::Small);
        }
        if total == 0 {
            c.text_clipped(
                MAIN_X + 20,
                body_y + 44,
                list_w - 40,
                "保存した内容との違いはありません（[Y] で作り直せます）",
                t.text_muted,
                FontSize::Small,
            );
        }
        let ids: Vec<usize> = (0..total).collect();
        self.draw_list_bar(
            c,
            ed,
            MAIN_X,
            list_w,
            body_y + 34,
            (VIS as i32) * ROW_H,
            ScrollBar::new(total, VIS, start),
            &ids,
            false,
        );

        // ---- right: the selected line, its bytes and the three keys
        c.round_rect(insp_x, body_y, insp_w, body_h, t.corner_r, t.bg_panel);
        c.round_rect_border(insp_x, body_y, insp_w, body_h, t.corner_r, 1, t.border_subtle);

        let can = selected.map(|row| ed.review_can_restore(row)).unwrap_or(false);
        let btn_w = (insp_w - 48) / 2;
        let a_x = insp_x + 20;
        let b_x = a_x + btn_w + 8;
        let btn_y = body_y + body_h - 44;
        let (a_bg, a_fg) = if armed.is_some() {
            (t.accent_warn, white_or_dark(t.accent_warn))
        } else if can {
            (t.bg_btn_primary, white_or_dark(t.bg_btn_primary))
        } else {
            (t.bg_btn, t.text_muted)
        };
        c.round_rect(a_x, btn_y, btn_w, 34, 5, a_bg);
        c.round_rect_border(
            a_x,
            btn_y,
            btn_w,
            34,
            5,
            1,
            if can { t.accent_primary } else { t.border_subtle },
        );
        btn_text(c, a_x, btn_y, btn_w, 34, ed.review_a_label(), a_fg, FontSize::Small);
        self.add_hit(a_x, btn_y, btn_w, 34, UiAction::Key(Key::A));

        c.round_rect(b_x, btn_y, btn_w, 34, 5, t.bg_btn);
        c.round_rect_border(b_x, btn_y, btn_w, 34, 5, 1, t.border_subtle);
        btn_text(c, b_x, btn_y, btn_w, 34, "[B] 戻る", t.text_primary, FontSize::Small);
        self.add_hit(b_x, btn_y, btn_w, 34, UiAction::Key(Key::B));

        // [Y] sits above the pair: the list is built once, so the key that
        // rebuilds it is the one a finger needs between two edits.
        let y_y = btn_y - 42;
        c.round_rect(a_x, y_y, insp_w - 40, 34, 5, t.bg_btn);
        c.round_rect_border(a_x, y_y, insp_w - 40, 34, 5, 1, t.border_subtle);
        btn_text(
            c,
            a_x,
            y_y,
            insp_w - 40,
            34,
            "[Y] 一覧を作り直す",
            t.text_primary,
            FontSize::Small,
        );
        self.add_hit(a_x, y_y, insp_w - 40, 34, UiAction::Key(Key::Y));

        let Some(row) = selected else {
            c.text_clipped(
                insp_x + 20,
                body_y + 18,
                insp_w - 40,
                "保存した内容との違いはありません",
                t.text_muted,
                FontSize::Small,
            );
            return;
        };
        c.text_clipped(
            insp_x + 20,
            body_y + 18,
            insp_w - 40,
            &ed.review_line(row),
            t.text_primary,
            FontSize::Small,
        );
        let note = ed.review_note(row);
        if !note.is_empty() {
            c.text_clipped(
                insp_x + 20,
                body_y + 44,
                insp_w - 40,
                &note,
                if ed.review_can_restore(row) {
                    t.text_secondary
                } else {
                    t.accent_warn
                },
                FontSize::Small,
            );
        }
        if let Some(list) = ed.review_list() {
            c.text_clipped(
                insp_x + 20,
                body_y + 70,
                insp_w - 40,
                &format!(
                    "変更 {} バイト（{} バイト → {} バイト）",
                    list.changed,
                    list.old.len(),
                    list.new_len
                ),
                t.text_secondary,
                FontSize::Small,
            );
            let mut y = body_y + 96;
            for line in list.summary.iter().take(4) {
                if y > y_y - 26 {
                    break;
                }
                c.text_clipped(
                    insp_x + 20,
                    y,
                    insp_w - 40,
                    &format!("・{}", line),
                    t.text_secondary,
                    FontSize::Small,
                );
                y += 22;
            }
        }
        c.text_clipped(
            insp_x + 20,
            y_y - 26,
            insp_w - 40,
            &format!("{} / {} 件（[▲/▼] 行）", row + 1, total),
            t.text_muted,
            FontSize::Small,
        );
    }

    // -----------------------------------------------------------------------
    // Clean Split-Studio List + Inspector Renderer (Zero Text Overlap)
    // -----------------------------------------------------------------------
    /// Renders rows of `(row_idx, title, meta_info, value_text, badge_text)`.
    /// - Left Panel (`x = 20, w = 796`): Shows ONLY `[Badge] Title` on the left and `Value` on the right.
    ///   `Title` is dynamically clipped to `val_left_x - title_x - 16`, guaranteeing zero collision!
    /// - Right Panel (`x = 832, w = 428`): Shows full details (`badge`, `title`, `meta_info`, `value`),
    ///   `[A]` action button, numeric stepper, and concise tool buttons.
    #[allow(clippy::too_many_arguments)]
    fn draw_adaptive_rows(
        &mut self,
        c: &mut Canvas,
        ed: &Editor,
        header_title: &str,
        sub: &str,
        rows: &[(usize, String, String, String, String)],
        active_row_idx: usize,
        has_stepper: bool,
        primary_a_label: &str,
    ) {
        self.draw_adaptive_rows_warn(
            c,
            ed,
            header_title,
            sub,
            rows,
            active_row_idx,
            has_stepper,
            primary_a_label,
            &[],
            &[],
        );
    }

    /// Same list/inspector layout, plus a strip of at most two lines under the
    /// list: the real warnings first, in the warning colour, then one line of
    /// explanation in the ordinary ink.
    ///
    /// The strip used to be one colour for both, so the equipment box's
    /// permanent operating note ("[-] 2回で削除 / [＋] 新規…") was drawn in
    /// amber exactly where a real warning goes - and it took both lines, so a
    /// record that did not fit its own type had nowhere to be said. The note is
    /// still drawn; it is grey now, it is one line, and a warning always gets
    /// the strip first.
    #[allow(clippy::too_many_arguments)]
    fn draw_adaptive_rows_warn(
        &mut self,
        c: &mut Canvas,
        ed: &Editor,
        header_title: &str,
        sub: &str,
        rows: &[(usize, String, String, String, String)],
        active_row_idx: usize,
        has_stepper: bool,
        primary_a_label: &str,
        notice: &[String],
        warnings: &[String],
    ) {
        let t = theme();
        let total = rows.len();

        // Geometry first. The header now repeats facts that come off the window
        // (how many rows fit, and therefore whether there is a page to turn),
        // so it cannot be laid out before the window is known.
        let body_y = CONTENT_Y + 30; // 150
        let body_h = FOOTER_Y - body_y - 10; // 508

        // The strip needs one or two lines under the list, so the list gives up
        // a row rather than drawing text over the last entry. Warnings are
        // counted first and the explanation only gets what is left of the two
        // lines.
        let warn_lh = c.line_height_sized(FontSize::Small).max(16);
        let strip_lines = (warnings.len().min(2) + notice.len().min(1)).min(2) as i32;
        let row_h = LIST_ROW_H;
        // The scrollbar's own geometry decides where a row's text and its tap
        // area may end: the bar answers a finger 40px from the panel edge now,
        // and nothing the row draws may reach into those 40px - a tap that
        // landed on the value must not move the window, and a value drawn under
        // the thumb reads as part of it. Computed from the same call the bar is
        // drawn from (scroll_track), so the two cannot drift apart.
        let track = scroll_track(MAIN_X, V1_LIST_W);
        let row_hit_w = track.row_right - (MAIN_X + 8);
        // Nine rows when there is no strip, eight when there is one: the
        // arithmetic lives in hbedit so a host test can pin it (the old copy
        // here said the same thing in a comment and then could not deliver it -
        // the list was eight rows with or without a strip).
        let vis = mhnx_core::hbedit::list_window_rows(body_h, row_h, strip_lines, warn_lh);
        let cur_pos = rows
            .iter()
            .position(|r| r.0 == active_row_idx)
            .unwrap_or(0);
        let start = window_start(cur_pos, total, vis);

        // Compact Section Title Line (y = 100..130) — No redundant explanation string!
        let title_w = c.text_width_sized(header_title, FontSize::Medium);
        c.text_clipped(
            MAIN_X + 4,
            CONTENT_Y,
            620,
            header_title,
            t.text_primary,
            FontSize::Medium,
        );

        // The right hand end of this line holds the three facts a long list has
        // to state on screen at all times: where the window is (12 / 340 件),
        // whether a filter is narrowing it, and the keys that move by a page.
        // They are laid out right to left from the panel's edge, and the
        // subtitle only gets what is left, so however long any of these strings
        // get they cannot be drawn on top of one another.
        let mut right_limit = MAIN_X + V1_LIST_W - 8;
        if total > 0 {
            let counter = format!("{} / {} 件", cur_pos + 1, total);
            let counter_w = c.text_width_sized(&counter, FontSize::Small);
            c.text_right(
                right_limit,
                CONTENT_Y + 3,
                &counter,
                t.text_secondary,
                FontSize::Small,
            );
            right_limit -= counter_w + 14;
        }
        if let Some(chip) = filter_chip(ed) {
            let chip_w = c.text_width_sized(&chip, FontSize::Small).min(200);
            if right_limit - chip_w > MAIN_X + 4 + title_w + 18 {
                c.text_clipped(
                    right_limit - chip_w,
                    CONTENT_Y + 3,
                    chip_w,
                    &chip,
                    t.accent_secondary,
                    FontSize::Small,
                );
                right_limit -= chip_w + 14;
            }
        }
        // The right stick's rule is not here any more: it is drawn in the
        // footer's second line (draw_footer), which is always on screen. Here
        // it was the first thing squeezed out - the note only drew while it fit
        // beside the counter and the filter chip - so a long heading was enough
        // to hide the only words that named the stick.

        // The subtitle sits on the same line, right of the title. Screens that
        // have a second fact worth saying at all times (the talisman form's
        // measured ceiling) put it here: the list itself only draws a row's
        // title and value, so a fact put in a row's meta column would only be
        // visible in the inspector of the selected row.
        if !sub.is_empty() {
            let note_x = MAIN_X + 4 + title_w + 18;
            if note_x + 60 <= right_limit {
                c.text_clipped(
                    note_x,
                    CONTENT_Y + 4,
                    right_limit - note_x,
                    sub,
                    t.text_secondary,
                    FontSize::Small,
                );
            }
        }

        if total == 0 {
            // No rows is a state of the list, not the absence of a screen: the
            // inspector's deck is where a finger reaches 全モンスターを表示
            // (hunt records), 検索を解除 and [＋] 新規 (equipment box), and the
            // panel used to be skipped entirely - so the only way out of an
            // empty list was a key the footer names, on a screen that had just
            // said there was nothing to do.
            c.round_rect(MAIN_X, body_y, V1_LIST_W, 120, t.corner_r, t.bg_panel);
            c.round_rect_border(
                MAIN_X,
                body_y,
                V1_LIST_W,
                120,
                t.corner_r,
                1,
                t.border_subtle,
            );
            // The line says which empty this is: a search that matched nothing
            // names the needle and the key that changes it, an empty box names
            // the screen's own way to fill it (hbedit::Editor::empty_list_line).
            // One word for both states ("データがありません") told the user
            // nothing about which one they were looking at.
            let line = ed.empty_list_line();
            let lw = c
                .text_width_sized(&line, FontSize::Medium)
                .min(V1_LIST_W - 40);
            let lx = MAIN_X + (V1_LIST_W - lw) / 2;
            c.text_clipped(lx, body_y + 48, lw, &line, t.text_secondary, FontSize::Medium);
            // The strip belongs to the screen, not to the rows: a pending delete
            // or a record that does not fit its type is exactly why a list can
            // be empty, and this branch used to return before it was drawn.
            self.draw_list_strip(
                c,
                strip_lines,
                0,
                body_y,
                body_h,
                row_h,
                warn_lh,
                &warnings,
                &notice,
            );
            self.draw_studio_inspector(
                c,
                ed,
                body_y,
                body_h,
                None,
                0,
                0,
                false,
                primary_a_label,
            );
            // A bar that is not drawn must not take a finger: the state is
            // written by the last frame that drew one, and a 0-row list kept
            // the previous screen's bar in the 40px strip beside the panel.
            self.bar = None;
            return;
        }

        // Left List Panel (x = 20, y = 132, w = 796, h = 526)
        c.round_rect(MAIN_X, body_y, V1_LIST_W, body_h, t.corner_r, t.bg_panel);
        c.round_rect_border(MAIN_X, body_y, V1_LIST_W, body_h, t.corner_r, 1, t.border_subtle);

        for i in 0..vis {
            let Some((r_idx, title, _meta, val, badge)) = rows.get(start + i) else {
                break;
            };
            let row_idx = *r_idx;
            let ry = body_y + mhnx_core::hbedit::LIST_PAD_TOP + (i as i32) * row_h;
            let is_sel = row_idx == active_row_idx;

            if is_sel {
                c.round_rect(MAIN_X + 8, ry + 2, V1_LIST_W - 16, row_h - 4, 6, t.bg_selected);
                c.rect(MAIN_X + 8, ry + 8, 4, row_h - 16, t.accent_primary);
            } else if i + 1 < vis && start + i + 1 < total {
                c.hline(MAIN_X + 16, ry + row_h - 1, V1_LIST_W - 32, t.border_subtle);
            }

            // The row's tap area stops where the bar's starts.
            self.add_row_hit(MAIN_X + 8, ry + 2, row_hit_w, row_h - 4, row_idx);

            // 1. Left Badge
            let mut tx = MAIN_X + 20;
            if !badge.is_empty() {
                let bw = (c.text_width_sized(badge, FontSize::Small) + 16).clamp(44, 88);
                let bbg = if is_sel { t.accent_primary } else { t.bg_val_box };
                let bfg = if is_sel {
                    white_or_dark(t.accent_primary)
                } else {
                    t.text_secondary
                };
                c.round_rect(tx, ry + 15, bw, 24, 4, bbg);
                btn_text(c, tx, ry + 15, bw, 24, badge, bfg, FontSize::Small);
                tx += bw + 14;
            }

            // 2. Right Value (clipped to max 230px so it never crowds the left title)
            // Its right edge is 8px left of the bar's touch area
            // (hbedit::SCROLL_TEXT_GAP), so no glyph is ever drawn under the
            // thumb and no tap on a value can be read as a tap on the bar.
            let right_edge = track.text_right;
            // A name value may hold a line break, and the game draws the second
            // line: the row draws them both, small, instead of clipping after
            // the first one.
            let val_lines = mhnx_core::save::name_lines(val);
            let val_multiline = val_lines.len() > 1;
            let val_w = if val.is_empty() {
                0
            } else {
                let max_vw = 230;
                let val_col = if is_sel { t.accent_primary } else { t.text_primary };
                if val_multiline {
                    for (li, line) in val_lines.iter().take(2).enumerate() {
                        let mut drawn = String::from(*line);
                        if li == 1 && val_lines.len() > 2 {
                            drawn.push('…');
                        }
                        c.text_right(right_edge, ry + 7 + (li as i32) * 18, &drawn, val_col, FontSize::Small);
                    }
                    max_vw + 20
                } else {
                    let raw_vw = c.text_width_sized(val, FontSize::Medium);
                    let actual_vw = raw_vw.min(max_vw);
                    if raw_vw <= max_vw {
                        c.text_right(right_edge, ry + 16, val, val_col, FontSize::Medium);
                    } else {
                        c.text_clipped(
                            right_edge - max_vw,
                            ry + 16,
                            max_vw,
                            val,
                            val_col,
                            FontSize::Medium,
                        );
                    }
                    actual_vw + 20
                }
            };

            // 3. Left Title (strictly clipped to the space before `val`)
            // A name title keeps its lines too: the row is 56 tall, so two
            // lines of the small font fit where one line of the medium font
            // sat.
            let max_title_w = (right_edge - val_w - tx).max(60);
            let title_col = if is_sel { t.text_primary } else { t.text_secondary };
            if title.contains('\n') {
                draw_name_box(c, tx, ry + 7, max_title_w, title, 2, 18, title_col, FontSize::Small);
            } else {
                c.text_clipped(tx, ry + 16, max_title_w, title, title_col, FontSize::Medium);
            }
        }

        // The scrollbar, drawn in the panel's right hand padding. The row ids go
        // with it: they are what turns a drag on the thumb into a row, and the
        // list itself is rebuilt from the save every frame, so this is the only
        // place that knows which row a pixel of the bar means.
        let ids: Vec<usize> = rows.iter().map(|r| r.0).collect();
        self.draw_list_bar(
            c,
            ed,
            MAIN_X,
            V1_LIST_W,
            body_y + mhnx_core::hbedit::LIST_PAD_TOP,
            (vis as i32) * row_h,
            ScrollBar::new(total, vis, start),
            &ids,
            false,
        );

        // The strip, pinned to the bottom of the list panel: warnings first, in
        // the warning colour, then the one line of explanation in grey.
        self.draw_list_strip(
            c,
            strip_lines,
            total.min(vis),
            body_y,
            body_h,
            row_h,
            warn_lh,
            &warnings,
            &notice,
        );

        // Right Inspector Panel (x = 832, y = 132, w = 428, h = 526)
        //
        // Drawn even when the list is empty (total == 0 returns above with no
        // row): the deck under the row's own buttons is the screen's operations,
        // and an empty list still has them.
        self.draw_studio_inspector(
            c,
            ed,
            body_y,
            body_h,
            rows.get(cur_pos),
            cur_pos,
            total,
            has_stepper,
            primary_a_label,
        );
    }

    /// The warning / explanation strip under a list.
    ///
    /// Warnings first, in the warning colour, then the one line of explanation
    /// in grey. It is drawn for every list, including one with no rows: a
    /// pending delete, a record that does not fit its type and the one line of
    /// explanation are facts about the screen, not about the rows. The empty
    /// list used to return before this was drawn, so the two screens whose
    /// warning is exactly why the list is empty showed nothing at all.
    #[allow(clippy::too_many_arguments)]
    fn draw_list_strip(
        &mut self,
        c: &mut Canvas,
        strip_lines: i32,
        rows_drawn: usize,
        body_y: i32,
        body_h: i32,
        row_h: i32,
        warn_lh: i32,
        warnings: &[String],
        notice: &[String],
    ) {
        if strip_lines == 0 {
            return;
        }
        let t = theme();
        let mut lines: Vec<(String, bool)> = Vec::new();
        if warnings.is_empty() {
            if let Some(n) = notice.first() {
                lines.push((n.clone(), false));
            }
        } else {
            lines.push((warnings[0].clone(), true));
            if warnings.len() > 1 {
                let second = if warnings.len() == 2 {
                    warnings[1].clone()
                } else {
                    format!("ほか {} 件 — 上の行を直してください", warnings.len() - 1)
                };
                lines.push((second, true));
            } else if let Some(n) = notice.first() {
                lines.push((n.clone(), false));
            }
        }
        let rows_bottom = body_y + mhnx_core::hbedit::LIST_PAD_TOP + rows_drawn as i32 * row_h;
        let limit = body_y + body_h - 4;
        let mut strip_h = lines.len() as i32 * warn_lh;
        let mut top = (limit - strip_h).max(rows_bottom + 12);
        // If the strip would run past the panel (a tall font metric), drop the
        // second line instead of drawing over the footer.
        if top + strip_h > limit && lines.len() > 1 {
            lines.truncate(1);
            strip_h = warn_lh;
            top = (limit - strip_h).max(rows_bottom + 12);
        }
        if top - 8 > rows_bottom + 2 {
            c.hline(MAIN_X + 16, top - 8, V1_LIST_W - 32, t.border_subtle);
        }
        for (i, (line, is_warning)) in lines.iter().enumerate() {
            c.text_clipped(
                MAIN_X + 20,
                top + i as i32 * warn_lh,
                V1_LIST_W - 40,
                line,
                if *is_warning { t.accent_warn } else { t.text_secondary },
                FontSize::Small,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_studio_inspector(
        &mut self,
        c: &mut Canvas,
        ed: &Editor,
        iy: i32,
        ih: i32,
        row: Option<&(usize, String, String, String, String)>,
        cur_pos: usize,
        total: usize,
        has_stepper: bool,
        primary_a_label: &str,
    ) {
        let t = theme();
        let ix = V1_INSP_X; // 832
        let iw = V1_INSP_W; // 428
        // No row is a state of the list, not the absence of the panel: every
        // button in the deck below is a screen operation, and the two screens
        // whose only way out of a narrowed / empty list lives there (狩猟記録's
        // 全モンスターを表示, the equipment box's 検索を解除 and [＋] 新規) used
        // to draw nothing at all.
        let empty = row.is_none();
        let (row_idx, title, meta, val, badge) = match row {
            Some(r) => (r.0, r.1.clone(), r.2.clone(), r.3.clone(), r.4.clone()),
            None => (
                0usize,
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ),
        };

        c.round_rect(ix, iy, iw, ih, t.corner_r, t.bg_panel);
        c.round_rect_border(ix, iy, iw, ih, t.corner_r, 1, t.border_subtle);

        // Slot 1 (iy + 14): Badge on left, "#pos / total" on right
        let top_tag = if empty {
            "行がありません"
        } else if badge.is_empty() {
            "選択項目"
        } else {
            badge.as_str()
        };
        c.text_clipped(ix + 20, iy + 14, 240, top_tag, t.accent_primary, FontSize::Small);
        if !empty {
            c.text_right(
                ix + iw - 20,
                iy + 14,
                &format!("#{} / {}", cur_pos + 1, total),
                t.text_muted,
                FontSize::Small,
            );
        }

        if ed.screen == Screen::Gmd && !empty {
            let role_label = if meta.is_empty() { "クエスト文章" } else { meta.as_str() };
            c.text_clipped(ix + 20, iy + 40, iw - 40, role_label, t.text_primary, FontSize::Medium);

            let raw_str = ed
                .gmd
                .as_ref()
                .and_then(|g| g.strings.get(row_idx))
                .map(|s| s.as_str())
                .unwrap_or("");
            let key_str = ed
                .gmd
                .as_ref()
                .map(|g| g.key.as_str())
                .unwrap_or("GMD");
            let line_cnt = raw_str.lines().count().max(1);
            c.text_clipped(
                ix + 20,
                iy + 70,
                iw - 40,
                &format!("{}  ({} B / {} 行)", key_str, raw_str.len(), line_cnt),
                t.text_secondary,
                FontSize::Small,
            );
            c.hline(ix + 16, iy + 98, iw - 32, t.border_subtle);

            // Multi-line preview box (iy + 108, h = 196)
            let box_y = iy + 108;
            let box_h = 196;
            c.round_rect(ix + 20, box_y, iw - 40, box_h, 6, t.bg_val_box);
            c.round_rect_border(ix + 20, box_y, iw - 40, box_h, 6, 1, t.border_subtle);
            if raw_str.is_empty() {
                c.text_centered(
                    ix + 20,
                    box_y,
                    iw - 40,
                    box_h,
                    "(空のテキスト)",
                    t.text_muted,
                    FontSize::Small,
                );
            } else {
                for (li, line) in raw_str.split('\n').take(6).enumerate() {
                    let clean_line = line.trim_end_matches('\r');
                    c.text_clipped(
                        ix + 32,
                        box_y + 12 + (li as i32) * 28,
                        iw - 64,
                        clean_line,
                        t.text_primary,
                        FontSize::Small,
                    );
                }
            }

            let btn_y = box_y + box_h + 14;
            c.round_rect(ix + 20, btn_y, iw - 40, 44, 6, t.accent_primary);
            btn_text(
                c,
                ix + 20,
                btn_y,
                iw - 40,
                44,
                &pill_word(ed, "A", "文章編集"),
                white_or_dark(t.accent_primary),
                FontSize::Medium,
            );
            self.add_hit(ix + 20, btn_y, iw - 40, 44, UiAction::ActivateRow(row_idx));

            let save_y = btn_y + 56;
            self.draw_tool_button(
                c,
                ix + 20,
                save_y,
                iw - 40,
                40,
                &pill_word(ed, "X", "保存"),
                UiAction::QuickSave,
            );
            return;
        }

        // Slots 2..5 describe the selected row. With no rows there is nothing
        // to describe - and the deck below is still the screen's operations, so
        // the panel keeps its frame and says which state it is in.
        let mut cur_y = iy + 168;
        if empty {
            c.text_clipped(
                ix + 20,
                iy + 40,
                iw - 40,
                "操作する行がありません",
                t.text_secondary,
                FontSize::Medium,
            );
            c.text_clipped(
                ix + 20,
                iy + 70,
                iw - 40,
                "下のボタンはこの画面の操作です（行は選べません）",
                t.text_muted,
                FontSize::Small,
            );
            c.hline(ix + 16, iy + 98, iw - 32, t.border_subtle);
            // A list with no rows is still a screen with an [A] where the state
            // machine says so (hbedit::Editor::empty_a_label): the talisman
            // list's press creates the first record. The button was drawn only
            // under a row, so the word 作成 was computed, printed by the
            // footer, and never drawn as a button.
            if let Some(label) = ed.empty_a_label() {
                let btn_y = iy + 168;
                let a_txt = pill_word(ed, "A", label);
                c.round_rect(ix + 20, btn_y, iw - 40, 42, 6, t.accent_primary);
                btn_text(
                    c,
                    ix + 20,
                    btn_y,
                    iw - 40,
                    42,
                    &a_txt,
                    white_or_dark(t.accent_primary),
                    FontSize::Medium,
                );
                self.add_hit(ix + 20, btn_y, iw - 40, 42, UiAction::ActivateRow(0));
            }
        } else {
        // Slot 2 (iy + 40): Selected Item Title
        // A name keeps its lines here too: the second line sits where the meta
        // line would be, so a two-line name does not hide its own last line.
        if title.contains('\n') {
            let used = draw_name_box(
                c,
                ix + 20,
                iy + 38,
                iw - 40,
                &title,
                2,
                20,
                t.text_primary,
                FontSize::Small,
            );
            let _ = used;
        } else {
            c.text_clipped(ix + 20, iy + 40, iw - 40, &title, t.text_primary, FontSize::Medium);
        }

        // Slot 3 (iy + 70): Concise Meta Info (if non-empty)
        if !meta.is_empty() {
            c.text_clipped(ix + 20, iy + 70, iw - 40, &meta, t.text_secondary, FontSize::Small);
        }

        c.hline(ix + 16, iy + 98, iw - 32, t.border_subtle);

        // Slot 4 (iy + 110, h = 48): Current Value Box
        // It is a button too: the same row action the [A] button below runs,
        // so a tap on the big number does what the row's own key does instead
        // of nothing at all.
        let disp_val = if val.is_empty() { title.as_str() } else { val.as_str() };
        c.round_rect(ix + 20, iy + 110, iw - 40, 48, 6, t.bg_val_box);
        c.round_rect_border(ix + 20, iy + 110, iw - 40, 48, 6, 1, t.border_subtle);
        if disp_val.contains('\n') {
            draw_name_box(
                c,
                ix + 32,
                iy + 114,
                iw - 64,
                disp_val,
                2,
                20,
                t.accent_primary,
                FontSize::Small,
            );
        } else {
            btn_text(
                c,
                ix + 20,
                iy + 110,
                iw - 40,
                48,
                disp_val,
                t.accent_primary,
                FontSize::Large,
            );
        }
        self.add_hit(ix + 20, iy + 110, iw - 40, 48, UiAction::ActivateRow(row_idx));

        // Slot 5 (iy + 168, h = 42): Primary [A] Button
        let btn_y = iy + 168;
        let a_txt = pill_word(ed, "A", primary_a_label);
        c.round_rect(ix + 20, btn_y, iw - 40, 42, 6, t.accent_primary);
        btn_text(
            c,
            ix + 20,
            btn_y,
            iw - 40,
            42,
            &a_txt,
            white_or_dark(t.accent_primary),
            FontSize::Medium,
        );
        self.add_hit(ix + 20, btn_y, iw - 40, 42, UiAction::ActivateRow(row_idx));

        cur_y = btn_y + 52; // iy + 220
        }

        // The skill panel is the other mode of the roster screen. A row is one
        // slot of a learned list, so the deck gives the candidate stepper (the
        // tables run to 93 and 56 entries, so the pad steps ten with L / R), the
        // tab switch and the way back; the [A] button above writes the candidate
        // into the row the list shows, so that is not repeated here.
        if ed.palico_skill_open() {
            let slot = row_idx;
            c.hline(ix + 16, cur_y, iw - 32, t.border_subtle);
            cur_y += 8;
            let full_w = iw - 40;
            let half_w = (iw - 48) / 2;
            c.text_sized(ix + 20, cur_y, "候補をめくる・一覧を切り替える", t.text_muted, FontSize::Small);
            cur_y += 24;
            self.draw_tool_button(
                c,
                ix + 20,
                cur_y,
                half_w,
                38,
                "◀ 前の候補",
                UiAction::AdjustRow(slot, Key::Left),
            );
            self.draw_tool_button(
                c,
                ix + 28 + half_w,
                cur_y,
                half_w,
                38,
                "▶ 次の候補",
                UiAction::AdjustRow(slot, Key::Right),
            );
            cur_y += 44;
            self.draw_tool_button(
                c,
                ix + 20,
                cur_y,
                full_w,
                38,
                &pill_word(ed, "Y", "タブ"),
                UiAction::Key(Key::Y),
            );
            cur_y += 44;
            self.draw_tool_button(
                c,
                ix + 20,
                cur_y,
                full_w,
                38,
                &ed.b_word(),
                UiAction::Key(Key::B),
            );
            return;
        }

        // The palico equipment panel is a mode of the two palico screens and
        // has the two tools those screens do not: the candidate stepper and the
        // two-press release. The value box and the [A] button above already say
        // what the row holds and what [A] would write, so nothing is repeated.
        if ed.palico_equip_open() {
            let part = row_idx;
            c.hline(ix + 16, cur_y, iw - 32, t.border_subtle);
            cur_y += 8;
            let full_w = iw - 40;
            let half_w = (iw - 48) / 2;
            c.text_sized(ix + 20, cur_y, "候補をめくる・解除", t.text_muted, FontSize::Small);
            cur_y += 24;
            self.draw_tool_button(
                c,
                ix + 20,
                cur_y,
                half_w,
                38,
                "◀ 前の候補",
                UiAction::AdjustRow(part, Key::Left),
            );
            self.draw_tool_button(
                c,
                ix + 28 + half_w,
                cur_y,
                half_w,
                38,
                "▶ 次の候補",
                UiAction::AdjustRow(part, Key::Right),
            );
            cur_y += 44;
            // A touch press goes through the same key path the pad uses, so a
            // tap can never release a part the pad would ask twice about.
            let armed = ed.palico_equip.and_then(|s| s.clear_armed) == Some(part);
            let clear_lbl = if armed {
                "もう一度タップで解除"
            } else {
                &pill_word(ed, "-", "解除（2回）")
            };
            self.draw_tool_button(c, ix + 20, cur_y, full_w, 38, clear_lbl, UiAction::Key(Key::Minus));
            cur_y += 44;
            self.draw_tool_button(
                c,
                ix + 20,
                cur_y,
                full_w,
                38,
                &ed.b_word(),
                UiAction::Key(Key::B),
            );
            return;
        }

        // Slot 6: Numeric Stepper & Step Size Selector
        //
        // The pills are drawn only where the arrows read the table they draw
        // (Editor::step_pills_live): オトモ moves by one and 環境設定's flags are
        // 0 or 1, so the five pills there changed nothing at all. The stepper
        // row itself stays on those screens because [MAX] lives in it.
        if has_stepper && !empty {
            let bw = (iw - 56) / 3; // 124px each
            // [◀ -]
            c.round_rect(ix + 20, cur_y, bw, 38, 5, t.bg_btn);
            c.round_rect_border(ix + 20, cur_y, bw, 38, 5, 1, t.border_subtle);
            btn_text(c, ix + 20, cur_y, bw, 38, "◀ -", t.text_primary, FontSize::Small);
            self.add_hit(ix + 20, cur_y, bw, 38, UiAction::AdjustRow(row_idx, Key::Left));

            // [+ ▶]
            let bx2 = ix + 20 + bw + 8;
            c.round_rect(bx2, cur_y, bw, 38, 5, t.bg_btn);
            c.round_rect_border(bx2, cur_y, bw, 38, 5, 1, t.border_subtle);
            btn_text(c, bx2, cur_y, bw, 38, "+ ▶", t.text_primary, FontSize::Small);
            self.add_hit(bx2, cur_y, bw, 38, UiAction::AdjustRow(row_idx, Key::Right));

            // [MAX] is drawn only where the row under the cursor really has a
            // ceiling. A button that only ever answers "no" is the shape of
            // bug this review opened with (five screens, no arm, no message).
            let max_lbl = if ed.screen == Screen::EntryView
                && ed
                    .arc
                    .as_ref()
                    .and_then(|a| a.entries.get(ed.entry))
                    .and_then(|e| extension_for(e.type_hash))
                    == Some("sem")
            {
                "0リセット"
            } else {
                "MAX"
            };
            let bx3 = bx2 + bw + 8;
            if max_is_available(ed, row_idx) {
                c.round_rect(bx3, cur_y, bw, 38, 5, t.bg_btn_primary);
                c.round_rect_border(bx3, cur_y, bw, 38, 5, 1, t.accent_primary);
                btn_text(c, bx3, cur_y, bw, 38, max_lbl, t.text_primary, FontSize::Small);
                self.add_hit(bx3, cur_y, bw, 38, UiAction::SetRowMax(row_idx));
            } else {
                c.round_rect(bx3, cur_y, bw, 38, 5, t.bg_btn);
                c.round_rect_border(bx3, cur_y, bw, 38, 5, 1, t.border_subtle);
                btn_text(c, bx3, cur_y, bw, 38, "—", t.text_muted, FontSize::Small);
            }

            cur_y += 46; // iy + 266

            // Step size selector pills. The table is the one this screen really
            // steps by (the hunt screen's fifth entry is a direction flip, not
            // a size). The table is the state machine's (Editor::step_table),
            // and the number the arrows take is its clamped entry
            // (Editor::step_value) - the sub-bar's chip reads the same pair, so
            // the strip above the list and the press below it cannot print two
            // different steps for one pill.
            //
            // A screen whose arrows do not read the table draws no pills at all
            // (Editor::step_pills_live): オトモ moves by one and 環境設定's
            // flags are 0 or 1, so the five buttons there changed nothing.
            let (steps, _step_key) = ed.step_table();
            let step_lbl = ed.step_pill_label();
            if ed.step_pills_live() {
                c.text_sized(ix + 20, cur_y + 4, &step_lbl, t.text_secondary, FontSize::Small);
            }
            // [R] 数値入力: the three screens that hold a number a user would
            // rather type than step (個数 / HR / 時間 / 狩猟数).
            //
            // The word is the key table's (Editor::key_label_or), not a copy
            // written here: the pill said 数値入力 over a footer that said 数値
            // on the same screen, and the item box's line has since had to
            // shorten the word to fit the 763px strip - so the copy would have
            // drifted again the moment it did.
            if ed.direct_input_available(row_idx) {
                let rw = 116i32;
                let rx = ix + iw - 20 - rw;
                let r_lbl = pill_word(ed, "R", "数値入力");
                c.round_rect(rx, cur_y, rw, 24, 4, t.bg_btn);
                c.round_rect_border(rx, cur_y, rw, 24, 4, 1, t.border_subtle);
                btn_text(c, rx, cur_y, rw, 24, &r_lbl, t.text_primary, FontSize::Small);
                self.add_hit(rx, cur_y, rw, 24, UiAction::Key(Key::R));
            }
            // Two short lines (label, then pills) so all five pills fit inside
            // the panel, and 24 + 34 in total: the tool deck below still ends
            // above the panel's bottom edge on every screen that draws it.
            if ed.step_pills_live() {
            cur_y += 26;
            // Five pills, inside the panel: (428 - 40 of side margin - 4 gaps of
            // 6) / 5 = 72 wide, so the last one ends at ix+404 of ix+428.
            let gap = 6i32;
            let sw = ((iw - 40 - gap * 4) / 5).max(40);
            let mut sx = ix + 20;
            for (s_idx, &step_val) in steps.iter().enumerate() {
                // The lit pill is the step the arrows really take: a form's
                // table is shorter than the shared one (FORM_STEPS), so the
                // index is clamped the same way the writer clamps it.
                let is_s = ed.step.min(steps.len().saturating_sub(1)) == s_idx;
                let sbg = if is_s { t.bg_selected } else { t.bg_btn };
                let sbd = if is_s { t.accent_primary } else { t.border_subtle };
                let sfg = if is_s { t.accent_primary } else { t.text_secondary };
                c.round_rect(sx, cur_y, sw, 30, 4, sbg);
                c.round_rect_border(sx, cur_y, sw, 30, 4, 1, sbd);
                let slbl = step_pill_label(step_val);
                btn_text(c, sx, cur_y, sw, 30, &slbl, sfg, FontSize::Small);
                self.add_hit(sx, cur_y, sw, 30, UiAction::SetStep(s_idx));
                sx += sw + gap;
            }
            cur_y += 34;
            }
        }

        // Slot 7: Clean Tool Actions (No `★`, concise labels)
        c.hline(ix + 16, cur_y, iw - 32, t.border_subtle);
        cur_y += 8;

        let full_w = iw - 40;
        let half_w = (iw - 48) / 2;

        match ed.screen {
            Screen::Char | Screen::Slots => {
                c.text_sized(ix + 20, cur_y, "一括操作・プリセット", t.text_muted, FontSize::Small);
                cur_y += 24;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "所持金・HR・全P 一括MAX（2回）",
                    UiAction::CharMaxAll,
                );
                cur_y += 44;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "対象スロット切替 (Slot 1〜3)",
                    UiAction::SelectSlot((ed.slot + 1) % 3),
                );
            }
            Screen::Items => {
                if ed.item_menu_open {
                    // In the menu the panel explains the highlighted row. The
                    // four rows that rewrite the box print the price of their
                    // second press here, and say how many presses are left.
                    c.text_sized(ix + 20, cur_y, "一括操作メニュー", t.text_muted, FontSize::Small);
                    cur_y += 24;
                    if ed.item_armed == Some(ed.item_menu_cursor) {
                        let preview = ed.item_op_arm_preview(ed.item_menu_cursor);
                        c.text_clipped(
                            ix + 20,
                            cur_y,
                            full_w,
                            &preview,
                            t.accent_warn,
                            FontSize::Small,
                        );
                        cur_y += 26;
                        c.text_clipped(
                            ix + 20,
                            cur_y,
                            full_w,
                            "もう一度で実行（2回）",
                            t.accent_warn,
                            FontSize::Small,
                        );
                        cur_y += 26;
                    }
                    if !max_is_available(ed, ed.item_menu_cursor) {
                        c.text_clipped(
                            ix + 20,
                            cur_y,
                            full_w,
                            "この画面では保存と実行が別のキーです",
                            t.text_muted,
                            FontSize::Small,
                        );
                        cur_y += 26;
                    }
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        &ed.b_word(),
                        UiAction::Key(Key::B),
                    );
                } else {
                    c.text_sized(ix + 20, cur_y, "一括操作", t.text_muted, FontSize::Small);
                    cur_y += 24;
                    // Two columns, and the stack is measured against the panel
                    // (hbedit::deck_stack). Five rows of pills drawn one under
                    // another put the last row at 658..694 - the panel ends at
                    // 658, so those two buttons were drawn over the frame and
                    // cut off. Two columns make four rows (490..652 here), and
                    // the helper shrinks a row rather than let a deck leave the
                    // panel again.
                    let rows = deck_stack(cur_y, iy + ih - 6, 4, 36, 6);
                    let row_y = |row: usize| rows.get(row).map(|r| r.0).unwrap_or(cur_y);
                    let row_h = |row: usize| rows.get(row).map(|r| r.1).unwrap_or(36);
                    // Row 1: the menu itself. Every other pill on this deck is
                    // one of its rows, but the menu is where the armed state and
                    // the two-press preview live, so it keeps the top.
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        row_y(0),
                        full_w,
                        row_h(0),
                        &pill_word(ed, "ZR", "一括"),
                        UiAction::OpenItemMenu,
                    );
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        row_y(1),
                        half_w,
                        row_h(1),
                        "重複を整理",
                        UiAction::ItemOp(ITEM_OP_DEDUPE),
                    );
                    self.draw_tool_button(
                        c,
                        ix + 28 + half_w,
                        row_y(1),
                        half_w,
                        row_h(1),
                        "アイテムを追加",
                        UiAction::ItemOp(ITEM_OP_ADD),
                    );
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        row_y(2),
                        half_w,
                        row_h(2),
                        "空きを詰める",
                        UiAction::ItemOp(ITEM_OP_COMPACT),
                    );
                    self.draw_tool_button(
                        c,
                        ix + 28 + half_w,
                        row_y(2),
                        half_w,
                        row_h(2),
                        "上限を丸める",
                        UiAction::ItemOp(ITEM_OP_CLAMP),
                    );
                    // The add's name search: [ZR] (the one input the item box
                    // had left - [Y] is the 空きも表示 toggle, [A] the picker,
                    // [R] the count, [X] the save, and [-] is the delete key),
                    // so this pill sends the same key the pad does.
                    let search_lbl = if ed.item_box_filter.trim().is_empty() {
                        "検索"
                    } else {
                        "検索中"
                    };
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        row_y(3),
                        half_w,
                        row_h(3),
                        search_lbl,
                        UiAction::FilterItems,
                    );
                    // The one thing the pad cannot reach on this screen:
                    // 空きも表示 (the same action the [Y] press runs). The step
                    // that used to sit beside it is gone from the deck: the
                    // five step pills in the stepper above are the control, and
                    // this screen has no key for it.
                    let empty_lbl = if ed.show_empty {
                        "空きを隠す"
                    } else {
                        "空きも表示"
                    };
                    self.draw_tool_button(
                        c,
                        ix + 28 + half_w,
                        row_y(3),
                        half_w,
                        row_h(3),
                        empty_lbl,
                        UiAction::ToggleItemEmpty,
                    );
                }
            }
            // The talisman list's two pad-only keys, as buttons: the two-press
            // delete (the same key every other list uses) and the search. The
            // delete was already implemented as UiAction::DeleteCharmConfirm
            // and nothing drew it, so a finger had no way to remove a talisman
            // at all. The form itself has no deck: its [A] is the row's own
            // action, which the inspector button and the footer name.
            Screen::Charms => {
                c.text_sized(ix + 20, cur_y, "護石の操作", t.text_muted, FontSize::Small);
                cur_y += 24;
                let armed = ed.charm_delete_armed.is_some();
                let del_label = if armed {
                    "もう一度タップで削除"
                } else {
                    "選択中の護石を削除（2回）"
                };
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    del_label,
                    UiAction::DeleteCharmConfirm,
                );
                cur_y += 44;
                let filter_lbl = if ed.charm_filter.trim().is_empty() {
                    "検索"
                } else {
                    "検索を変更（空で解除）"
                };
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    filter_lbl,
                    UiAction::FilterCharms,
                );
            }
            Screen::CharmEdit => {}
            // The four pick lists: the three keys the pad has and a finger did
            // not. The deck said "[X] 変更を保存" on them, which is a lie - [X]
            // is クリア on a picker and the save key is not even sent there
            // (ui::handle_pad_key's x_is_the_screens).
            Screen::ItemPick | Screen::SkillPick | Screen::JewelPick | Screen::EquipPick => {
                c.text_sized(ix + 20, cur_y, "一覧の操作", t.text_muted, FontSize::Small);
                cur_y += 24;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "名前で検索",
                    UiAction::Key(Key::Y),
                );
                cur_y += 44;
                if ed.screen == Screen::EquipPick {
                    // The type filter: [ZR] walks every type of the scope and
                    // then the scope's first one, which is the only way to see
                    // the other half of a 6,435 row table. The button used to
                    // send [-] while its own label said [ZR], so the pad did
                    // nothing and the touch button did the one job [-] no
                    // longer has.
                    let kind_lbl = format!("{}: {}", pill_word(ed, "ZR", "種別"), ed.equip_pick_kind_label());
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        &kind_lbl,
                        UiAction::Key(Key::Z),
                    );
                    cur_y += 44;
                }
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "検索をクリア",
                    UiAction::Key(Key::X),
                );
            }
            Screen::Equip => {
                // Both buttons go through the same key path the pad uses, so
                // the touch build cannot delete anything the pad would refuse.
                c.text_sized(ix + 20, cur_y, "装備の操作", t.text_muted, FontSize::Small);
                cur_y += 24;
                let armed_index = ed.equip_delete_pending().map(|(index, _, _)| index);
                let cursor_index = ed.equip_rows().get(ed.equip_cursor).map(|e| e.index);
                let armed = armed_index.is_some() && armed_index == cursor_index;
                let del_label = if armed {
                    "削除する? もう一度タップ"
                } else {
                    "選択中の装備を削除（2回）"
                };
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    del_label,
                    UiAction::Key(Key::Minus),
                );
                cur_y += 44;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "[＋] 新規装備を追加",
                    UiAction::Key(Key::Plus),
                );
                cur_y += 44;
                // The filter is on the heading, and this is the way out of it
                // for a finger: [X] used to be the clear key and [X] is save.
                let filter_lbl = if ed.equip_filter.trim().is_empty() {
                    "検索"
                } else {
                    "検索を変更（空で解除）"
                };
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    filter_lbl,
                    UiAction::Key(Key::Y),
                );
                if !ed.equip_filter.trim().is_empty() {
                    cur_y += 44;
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        "検索を解除（全件）",
                        UiAction::ClearEquipFilter,
                    );
                }
            }
            // The set editor is a mode of the my-sets screen; a touch press
            // goes through the same key path the pad uses, so a tap can never
            // clear a part the pad would ask twice about.
            Screen::Sets => {
                if let Some(e) = ed.item_set_edit {
                    // The item form's own tools: the candidate stepper the
                    // equipment form gets from the numeric stepper, and the
                    // two operations the set exists for. Both go through the
                    // same key path the pad uses, so a tap can never write the
                    // pouch without the second press the pad asks for.
                    let _ = e;
                    c.text_sized(ix + 20, cur_y, "このセットの操作", t.text_muted, FontSize::Small);
                    cur_y += 24;
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        half_w,
                        38,
                        "◀ 前の候補",
                        UiAction::AdjustRow(row_idx, Key::Left),
                    );
                    self.draw_tool_button(
                        c,
                        ix + 28 + half_w,
                        cur_y,
                        half_w,
                        38,
                        "▶ 次の候補",
                        UiAction::AdjustRow(row_idx, Key::Right),
                    );
                    cur_y += 44;
                    let from_lbl = if ed.item_set_armed == Some(ItemSetOp::FromPouch) {
                        "もう一度タップで取り込む"
                    } else {
                        "ポーチから取り込む（2回）"
                    };
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        from_lbl,
                        UiAction::Key(Key::Y),
                    );
                    cur_y += 44;
                    let to_lbl = if ed.item_set_armed == Some(ItemSetOp::ToPouch) {
                        "もう一度タップで適用する"
                    } else {
                        // [R] is the same operation on the pad
                        // (hbedit::Editor::shoulder_job): the pill and the key
                        // run one method, so they cannot disagree.
                        "ポーチへ適用（2回）"
                    };
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        to_lbl,
                        UiAction::ItemSetToPouch,
                    );
                    cur_y += 44;
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        &ed.b_word(),
                        UiAction::Key(Key::B),
                    );
                } else if ed.set_edit.is_some() {
                    c.text_sized(ix + 20, cur_y, "このセットの操作", t.text_muted, FontSize::Small);
                    cur_y += 24;
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        // This sent X, and X is the save key (QuickSave): a
                        // tap on "re-read the warnings" opened the save review
                        // instead. The state machine answers Y on this screen.
                        "警告を読み直す",
                        UiAction::Key(Key::Y),
                    );
                    cur_y += 44;
                    let clear_label = if ed.set_clear_armed.is_some() {
                        "もう一度タップで部位を解除"
                    } else {
                        "部位を解除（2回）"
                    };
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        clear_label,
                        UiAction::Key(Key::Minus),
                    );
                    cur_y += 44;
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        &ed.b_word(),
                        UiAction::Key(Key::B),
                    );
                } else {
                    c.text_sized(ix + 20, cur_y, "マイセット", t.text_muted, FontSize::Small);
                    cur_y += 24;
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        // The table's word (タブ), not a second one: the
                        // footer said タブ and this button said 切替.
                        &pill_word(ed, "◀/▶", "タブ"),
                        UiAction::Key(Key::Right),
                    );
                }
            }
            Screen::Roster => {
                c.text_sized(ix + 20, cur_y, "一括操作・プリセット", t.text_muted, FontSize::Small);
                cur_y += 24;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "全オトモ Lv99・なつき100・EXP最大",
                    UiAction::RosterMaxAll,
                );
                cur_y += 44;
                // [Y] walks the three editable columns (Lv / なつき / EXP) and
                // had no touch route: the row's value and the ◀/▶ buttons only
                // ever moved the column the pad had left it on.
                let col = match ed.roster_field {
                    0 => "Lv",
                    1 => "なつき度",
                    _ => "EXP",
                };
                let col_lbl = format!("編集列: {}", col);
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    &col_lbl,
                    UiAction::Key(Key::Y),
                );
                cur_y += 44;
                // The panel the [A] button on the row above cannot open: this is
                // the skill / support move editor for the palico on the list.
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    &pill_word(ed, "＋", "スキル"),
                    UiAction::Key(Key::Plus),
                );
            }
            Screen::Palico => {
                c.text_sized(ix + 20, cur_y, "一括操作・プリセット", t.text_muted, FontSize::Small);
                cur_y += 24;
                // The language is the arrow pair on this build and had no touch
                // route at all: the two keys were the only way to reach it and
                // no line on the screen named them.
                // The words are the state machine's (Editor::palico_lang_label),
                // the same label the footer's fact prints: the button and the
                // key line cannot describe one key two ways.
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    &ed.palico_lang_label(),
                    UiAction::TogglePalicoLang,
                );
                cur_y += 44;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    // The key table's word, not a second sentence: the footer
                    // says 全導入2回 and this button said 全84匹を一括導入.
                    &pill_word(ed, "Y", "全導入2回"),
                    UiAction::InstallAllPalicoes,
                );
            }
            Screen::SaveQuests | Screen::ArcList => {
                c.text_sized(ix + 20, cur_y, "一括操作・プリセット", t.text_muted, FontSize::Small);
                cur_y += 24;
                // The counts come from the bundles themselves: the chips said
                // 189 / 34 as literals, so a bundle that grew left the button
                // lying.
                let (official_n, dead_n) = mhnx_core::dlc::bundled_quest_counts();
                let official = format!("公式イベント{}件 一括導入", official_n);
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    &official,
                    UiAction::InstallDlcCategory(0),
                );
                cur_y += 44;
                // The 没クエスト bundle had a tab and no bulk button at all:
                // one row at a time was the only way in.
                let dead = format!("没クエスト{}件 一括導入", dead_n);
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    &dead,
                    UiAction::InstallDlcCategory(1),
                );
                cur_y += 44;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "内蔵クエストをSDへ展開",
                    UiAction::ExtractOfficialArcs,
                );
                cur_y += 44;
                if ed.screen == Screen::SaveQuests {
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        "選択中のクエストをSDへ展開",
                        UiAction::ExtractSelectedQuest,
                    );
                    cur_y += 44;
                    // The save's whole quest list, one press: the [R] the
                    // console spends on the sub-tab.
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        "セーブ内クエストを全部SDへ展開",
                        UiAction::ExtractAllQuests,
                    );
                    cur_y += 44;
                    // The pad's two-press delete, reachable by finger too.
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        "選択中のクエストを削除（2回）",
                        UiAction::Key(Key::Minus),
                    );
                    cur_y += 44;
                }
                // The way out, which had no button on either list: [B] goes to
                // the screen the list belongs to (SD(.arc) for the save's
                // quests, the save list for the .arc list). A list with no rows
                // - an empty save, a folder with no .arc - left the four bulk
                // buttons as the only thing a finger could press.
                // The word is the state machine's (Editor::b_word): the
                // destination's name, which on these two lists is SD(.arc) for
                // the save's quests and セーブ一覧 for the .arc list.
                let out_word = ed.b_word();
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    &out_word,
                    UiAction::Key(Key::B),
                );
            }
            Screen::Mib => {
                let is_mob = questfields::MIB_FIELDS
                    .get(row_idx)
                    .map(|f| f.key.starts_with("mob_slot["))
                    .unwrap_or(false);
                if is_mob {
                    c.text_sized(
                        ix + 20,
                        cur_y,
                        "モンスター個別パラメータ（タップで選択）",
                        t.text_muted,
                        FontSize::Small,
                    );
                    cur_y += 22;
                    let mib = ed.mib_bytes();
                    let off = questfields::MIB_FIELDS
                        .get(row_idx)
                        .map(|f| f.offset)
                        .unwrap_or(0);
                    let pw = (iw - 58) / 4; // 92px
                    let ph = 32;
                    for s_i in 0..8usize {
                        let col = (s_i % 4) as i32;
                        let r = (s_i / 4) as i32;
                        let px = ix + 20 + col * (pw + 6);
                        let py = cur_y + r * (ph + 6);
                        let active = self.mib_mob_sub == s_i;
                        let bg = if active { t.bg_selected } else { t.bg_btn };
                        let bd = if active { t.accent_primary } else { t.border_subtle };
                        let fg = if active { t.accent_primary } else { t.text_secondary };
                        let v_str = if off + 13 <= mib.len() {
                            match s_i {
                                0 => format!("HP:{}", mib[off + 5]),
                                1 => format!("攻:{}", mib[off + 6]),
                                2 => format!("防:{}", mib[off + 7]),
                                3 => {
                                    let sz = u16::from_le_bytes([mib[off + 9], mib[off + 10]]);
                                    format!("{}%", sz)
                                }
                                4 => format!("数:{}", mib[off + 4]),
                                5 => format!("疲:{}", mib[off + 8]),
                                6 => format!("変:{}", mib[off + 11]),
                                _ => format!("特:{}", mib[off + 3]),
                            }
                        } else {
                            MIB_MOB_SUB_LABELS[s_i].to_string()
                        };
                        c.round_rect(px, py, pw, ph, 4, bg);
                        c.round_rect_border(px, py, pw, ph, 4, 1, bd);
                        btn_text(c, px, py, pw, ph, &v_str, fg, FontSize::Small);
                        self.add_hit(px, py, pw, ph, UiAction::SelectSubField(row_idx, s_i));
                    }
                    cur_y += 78;
                } else {
                    c.text_sized(ix + 20, cur_y, "一括操作・初期配置", t.text_muted, FontSize::Small);
                    cur_y += 24;
                }
                // Every chip names what it writes and how many monsters it
                // would touch, and none of them runs on one press.
                let p0 = ed.mib_preset_label(0);
                let p1 = ed.mib_preset_label(1);
                let p2 = ed.mib_preset_label(2);
                self.draw_tool_button(c, ix + 20, cur_y, half_w, 34, &p0, UiAction::MibPreset(0));
                self.draw_tool_button(c, ix + 28 + half_w, cur_y, half_w, 34, &p1, UiAction::MibPreset(1));
                cur_y += 38;
                self.draw_tool_button(c, ix + 20, cur_y, half_w, 34, &p2, UiAction::MibPreset(2));
                self.draw_tool_button(c, ix + 28 + half_w, cur_y, half_w, 34, "初期配置(.sem)", UiAction::SelectSubTab(1));
            }
            Screen::EntryView => {
                c.text_sized(ix + 20, cur_y, "初期配置・エントリ操作", t.text_muted, FontSize::Small);
                cur_y += 24;
                let ext = ed
                    .arc
                    .as_ref()
                    .and_then(|a| a.entries.get(ed.entry))
                    .and_then(|e| extension_for(e.type_hash));
                if ext == Some("sem") {
                    let copy_lbl = ed.sem_preset_label(0);
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        36,
                        &copy_lbl,
                        UiAction::SemPreset(0),
                    );
                    cur_y += 42;
                    let reset_lbl = ed.sem_preset_label(1);
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        36,
                        &reset_lbl,
                        UiAction::SemPreset(1),
                    );
                } else if ed.entry_has_numbers() {
                    // The row holds two numbers on a reward table and one on a
                    // supply box, so the deck names the key that picks between
                    // them and the key that types one.
                    let field = ed.entry_field_label();
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        34,
                        &format!("編集する項目: {}", field),
                        UiAction::Key(Key::Z),
                    );
                    cur_y += 38;
                    // The word is the key table's (Editor::key_label_or): this
                    // panel and the footer name one key.
                    let r_lbl = pill_word(ed, "R", "数値入力");
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        half_w,
                        34,
                        &r_lbl,
                        UiAction::Key(Key::R),
                    );
                    self.draw_tool_button(
                        c,
                        ix + 28 + half_w,
                        cur_y,
                        half_w,
                        34,
                        &pill_word(ed, "X", "保存"),
                        UiAction::QuickSave,
                    );
                } else {
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        &pill_word(ed, "X", "保存"),
                        UiAction::QuickSave,
                    );
                }
            }
            // The progress screen has no bulk edit to offer, so the deck gives
            // its three views instead: the pad's L / R do the same, and this is
            // where a touch player reaches for it. The save pill stays under
            // them, because [X] is the key that leaves this screen's edits on
            // the card.
            Screen::Progress => {
                c.text_sized(ix + 20, cur_y, "表示切替・絞り込み", t.text_muted, FontSize::Small);
                cur_y += 24;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    half_w,
                    38,
                    "◀ 前の表示",
                    UiAction::CycleSubTab(false),
                );
                self.draw_tool_button(
                    c,
                    ix + 28 + half_w,
                    cur_y,
                    half_w,
                    38,
                    "次の表示 ▶",
                    UiAction::CycleSubTab(true),
                );
                cur_y += 44;
                // The search: 1,508 quests and 380 request flags, and every key
                // on this screen is spoken for, so it is a pill.
                let search_lbl = if ed.progress_filter.trim().is_empty() {
                    "[検索] 名前で絞る"
                } else {
                    "[検索] 検索中（空で解除）"
                };
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    search_lbl,
                    UiAction::FilterProgress,
                );
                cur_y += 44;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    &pill_word(ed, "X", "保存"),
                    UiAction::QuickSave,
                );
            }
            // The hunt list is a filter, so its deck gives the filter switch
            // a touch target as well as the key. [-]/ ▲/▼ do not reach a
            // finger.
            Screen::Hunts => {
                c.text_sized(ix + 20, cur_y, "表示切替", t.text_muted, FontSize::Small);
                cur_y += 24;
                let lbl = if ed.hunt_show_all {
                    "記録ありのみに戻す"
                } else {
                    "全モンスターを表示"
                };
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    lbl,
                    UiAction::ToggleHuntShowAll,
                );
                cur_y += 44;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    &pill_word(ed, "X", "保存"),
                    UiAction::QuickSave,
                );
            }
            // CSV 入出力: the two keys the screen has and a finger did not.
            // The deck used to say "[X] 変更を保存" here, and [X] does nothing
            // at all on this screen (input_csv has no X arm) - a button that
            // promised a save that never happens.
            Screen::Csv => {
                c.text_sized(ix + 20, cur_y, "CSV の操作", t.text_muted, FontSize::Small);
                cur_y += 24;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "2つの BOX を書き出す",
                    UiAction::Key(Key::Y),
                );
                cur_y += 44;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "フォルダを読み直す",
                    UiAction::Key(Key::Z),
                );
                cur_y += 44;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    if ed.csv_armed.is_some() {
                        "もう一度で適用"
                    } else {
                        "選んだ CSV を適用（2回）"
                    },
                    UiAction::Key(Key::A),
                );
            }
            // セーブ一覧: the first screen, and the only one that can be empty
            // with nowhere to go. [Y] is 再読込 and had no hit area at all, so
            // a card with no saves offered exactly one button - 保存 - which
            // wrote nothing because no save was open. The reload is the
            // screen's own key and the only thing that can change the list, so
            // it is drawn first and always; the save pill is drawn only when
            // there is a save to write.
            Screen::SaveList => {
                c.text_sized(ix + 20, cur_y, "セーブの操作", t.text_muted, FontSize::Small);
                cur_y += 24;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "再読込（カードを読み直す）",
                    UiAction::Rescan,
                );
                cur_y += 44;
                if ed.save.is_some() {
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        &pill_word(ed, "X", "保存"),
                        UiAction::QuickSave,
                    );
                } else {
                    c.text_clipped(
                        ix + 20,
                        cur_y,
                        full_w,
                        if ed.save_files.is_empty() {
                            "セーブが見つかりません（switch/XXSE/saves/ に置いて再読込）"
                        } else {
                            "開いているセーブはありません（一覧から開いてください）"
                        },
                        t.text_muted,
                        FontSize::Small,
                    );
                }
            }
            // ギルドカード: the bar shows the card's four views while this
            // screen is up, so the section's other tabs (狩猟記録 / 進行状況)
            // are not on it at all. The two are one press away on the pad
            // ([ZR] and the section tab) and there was no way for a finger to
            // reach them: the deck carries both, and the card's way out.
            Screen::Guild => {
                c.text_sized(ix + 20, cur_y, "記録の操作", t.text_muted, FontSize::Small);
                cur_y += 24;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    half_w,
                    38,
                    &pill_word(ed, "ZR", "進行状況"),
                    UiAction::Key(Key::Z),
                );
                self.draw_tool_button(
                    c,
                    ix + 28 + half_w,
                    cur_y,
                    half_w,
                    38,
                    "狩猟記録",
                    UiAction::OpenHunts,
                );
                cur_y += 44;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    half_w,
                    38,
                    // The word is the key table's (Editor::key_label_or): the
                    // footer says キャラ and this button said スロット for the
                    // same press - two names for one destination.
                    &ed.b_word(),
                    UiAction::Key(Key::B),
                );
                self.draw_tool_button(
                    c,
                    ix + 28 + half_w,
                    cur_y,
                    half_w,
                    38,
                    &pill_word(ed, "X", "保存"),
                    UiAction::QuickSave,
                );
            }
            // 検査: the list of findings has no rows when a quest is clean, and
            // the only button was 保存. [B] goes back to the file list.
            Screen::Check => {
                c.text_sized(ix + 20, cur_y, "検査の操作", t.text_muted, FontSize::Small);
                cur_y += 24;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    &ed.b_word(),
                    UiAction::Key(Key::B),
                );
                cur_y += 44;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    ".arc 保存",
                    UiAction::QuickSave,
                );
            }
            _ => {
                c.text_sized(ix + 20, cur_y, "一括操作・プリセット", t.text_muted, FontSize::Small);
                cur_y += 24;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    &pill_word(ed, "X", "保存"),
                    UiAction::QuickSave,
                );
            }
        }
    }

    /// The word a pill prints for the key that runs it.
    ///
    /// A pill is a button, not a key. The footer is the one surface that names
    /// keys, and every pill that printed "[ZR]" was a second surface claiming
    /// one: the sixth review counted four wordings for one [ZR] press on the
    /// item box alone (一括操作 on the bar, 一括操作メニュー and 検索 in the deck,
    /// 一括 on the footer), and the 検索 pill named a key that does not search
    /// at all - the search is a row *inside* the menu [ZR] opens.
    ///
    /// The word is the table's (Editor::key_label_or), so a pill and the footer
    /// cannot describe one action two ways. A pill whose action the table does
    /// not carry (the two-stage add, an armed confirmation, a state toggle)
    /// keeps its own word and still names no key.
    fn draw_tool_button(
        &mut self,
        c: &mut Canvas,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        label: &str,
        action: UiAction,
    ) {
        let t = theme();
        c.round_rect(x, y, w, h, 5, t.bg_btn);
        c.round_rect_border(x, y, w, h, 5, 1, t.border_subtle);
        btn_text(c, x, y, w, h, label, t.text_primary, FontSize::Small);
        self.add_hit(x, y, w, h, action);
    }

    // -----------------------------------------------------------------------
    // Screen Implementations (Concise, Non-Redundant Strings)
    // -----------------------------------------------------------------------

    fn draw_save_list(&mut self, c: &mut Canvas, ed: &Editor) {
        let mut rows = Vec::with_capacity(ed.save_files.len());
        for (i, path) in ed.save_files.iter().enumerate() {
            let is_console = path == CONSOLE_SAVE;
            let badge = if is_console {
                String::from("本体")
            } else {
                format!("#{:02}", i + 1)
            };
            let title = if is_console {
                String::from("Switch 本体セーブデータ")
            } else {
                path.rsplit('/').next().unwrap_or(path.as_str()).to_string()
            };
            rows.push((i, title, path.clone(), String::new(), badge));
        }
        self.draw_adaptive_rows(
            c,
            ed,
            "セーブデータ一覧",
            "",
            &rows,
            ed.cursor,
            false,
            "開く",
        );
    }

    fn draw_slots(&mut self, c: &mut Canvas, ed: &Editor) {
        let t = theme();
        c.text_sized(MAIN_X + 4, CONTENT_Y, "キャラ選択", t.text_primary, FontSize::Medium);

        let card_w = 396i32;
        let card_h = 460i32;
        let gap = 26i32;
        let y = CONTENT_Y + 36;

        for s in 0..3usize {
            let x = MAIN_X + (s as i32) * (card_w + gap);
            let is_sel = ed.slot == s;
            let bg = if is_sel { t.bg_selected } else { t.bg_panel };
            let bd = if is_sel { t.border_focus } else { t.border_subtle };

            c.round_rect(x, y, card_w, card_h, t.corner_r, bg);
            c.round_rect_border(x, y, card_w, card_h, t.corner_r, if is_sel { 2 } else { 1 }, bd);

            let badge = format!("SLOT {}", s + 1);
            c.round_rect(
                x + 20,
                y + 20,
                92,
                28,
                5,
                if is_sel { t.accent_primary } else { t.bg_val_box },
            );
            btn_text(
                c,
                x + 20,
                y + 20,
                92,
                28,
                &badge,
                if is_sel {
                    white_or_dark(t.accent_primary)
                } else {
                    t.accent_primary
                },
                FontSize::Small,
            );

            let (summary, zenny_v, hr_v, hrp_v, wyc_v, play_v) = if let Some(save) = &ed.save {
                if let Some(ch) = save.read_character(s) {
                    (
                        if ch.name.is_empty() {
                            String::from("(未使用)")
                        } else {
                            ch.name
                        },
                        ch.funds,
                        ch.hunter_rank,
                        ch.hr_points,
                        ch.academy_points,
                        ch.play_time,
                    )
                } else {
                    (String::from("(空き)"), 0, 0, 0, 0, 0)
                }
            } else {
                (String::from("(未読込)"), 0, 0, 0, 0, 0)
            };
            // A name may hold a line break, and both lines are part of what the
            // card is: draw them where the single large line used to be.
            if summary.contains('\n') {
                draw_name_box(
                    c,
                    x + 20,
                    y + 60,
                    card_w - 40,
                    &summary,
                    2,
                    24,
                    t.text_primary,
                    FontSize::Medium,
                );
            } else {
                c.text_clipped(x + 20, y + 66, card_w - 40, &summary, t.text_primary, FontSize::Large);
            }
            c.hline(x + 20, y + 110, card_w - 40, t.border_subtle);

            if ed.save.is_some() {
                let zenny = format!("所持金: {} z", zenny_v);
                let hr = format!("HR: {}  (HRP: {})", hr_v, hrp_v);
                let wyc = format!("龍歴院P: {} pts", wyc_v);
                let play = format!("プレイ時間: {}h {:02}m", play_v / 3600, (play_v % 3600) / 60);
                c.text_clipped(x + 24, y + 132, card_w - 48, &zenny, t.accent_secondary, FontSize::Medium);
                c.text_clipped(x + 24, y + 170, card_w - 48, &hr, t.text_primary, FontSize::Medium);
                c.text_clipped(x + 24, y + 208, card_w - 48, &wyc, t.text_secondary, FontSize::Medium);
                c.text_clipped(x + 24, y + 246, card_w - 48, &play, t.text_secondary, FontSize::Small);
            }

            let btn_y = y + card_h - 62;
            let has_char = ed.slot_has_character(s);
            // A blank slot draws no button. It used to draw the same one as the
            // other two and send a bare [A], which opened `ed.slot` - the
            // highlighted card, not this one - so the button on card 2 opened
            // card 1's character, and on an empty card it opened a character
            // screen whose every writer refuses.
            if has_char {
                let btn_bg = if is_sel { t.accent_primary } else { t.bg_btn };
                let btn_fg = if is_sel {
                    white_or_dark(t.accent_primary)
                } else {
                    t.text_secondary
                };
                c.round_rect(x + 20, btn_y, card_w - 40, 42, 6, btn_bg);
                btn_text(
                    c,
                    x + 20,
                    btn_y,
                    card_w - 40,
                    42,
                    &pill_word(ed, "A", "このキャラを編集"),
                    btn_fg,
                    FontSize::Medium,
                );
            } else {
                // The shape of a button with nothing behind it: the card still
                // selects, and the label says why there is nothing to open.
                c.round_rect(x + 20, btn_y, card_w - 40, 42, 6, t.bg_val_box);
                c.round_rect_border(x + 20, btn_y, card_w - 40, 42, 6, 1, t.border_subtle);
                btn_text(
                    c,
                    x + 20,
                    btn_y,
                    card_w - 40,
                    42,
                    "この枠にキャラがいません",
                    t.text_muted,
                    FontSize::Small,
                );
            }

            // The whole card selects the slot; the button opens *this* card's
            // character. Pushed after the card, so `hits` (searched newest
            // first) finds the button first.
            self.add_hit(x, y, card_w, card_h, UiAction::SelectSlot(s));
            if has_char {
                self.add_hit(x + 20, btn_y, card_w - 40, 42, UiAction::OpenSlot(s));
            }
        }
    }

    fn draw_convert(&mut self, c: &mut Canvas, ed: &Editor) {
        let t = theme();
        let by = CONTENT_Y + 16;
        c.round_rect(MAIN_X, by, MAIN_W, 300, t.corner_r, t.bg_panel);
        c.round_rect_border(MAIN_X, by, MAIN_W, 300, t.corner_r, 1, t.border_subtle);

        c.text_sized(
            MAIN_X + 28,
            by + 22,
            "3DS / Switch 形式変換",
            t.accent_primary,
            FontSize::Large,
        );

        match (&ed.save, &ed.save_path) {
            (None, _) => {
                c.text_sized(
                    MAIN_X + 28,
                    by + 80,
                    "セーブデータを開いてください",
                    t.text_secondary,
                    FontSize::Medium,
                );
            }
            (Some(s), _) => {
                let src_kind = match s.kind() {
                    SaveKind::ThreeDs => "3DS (ヘッダなし)",
                    SaveKind::Switch => "Switch MHXX (36Bヘッダ)",
                    SaveKind::Mhgu => "Switch MHGU",
                };
                let dst_kind = if s.kind() == SaveKind::ThreeDs {
                    "Switch MHXX"
                } else {
                    "3DS MHXX"
                };
                // The brackets name where the conversion *writes*, not where it
                // read: the line printed the source path, which read as if [A]
                // wrote back over the save it had just opened. The path itself
                // is the state machine's (Editor::convert_output_path), so the
                // name on screen and the file on the card cannot disagree.
                let out = ed
                    .convert_output_path()
                    .unwrap_or_else(|| String::from("-"));

                c.text_sized(MAIN_X + 28, by + 76, &format!("変換元 : {}", src_kind), t.text_primary, FontSize::Medium);
                c.text_sized(MAIN_X + 28, by + 114, &format!("変換先 : {}", dst_kind), t.accent_secondary, FontSize::Medium);
                c.text_clipped(
                    MAIN_X + 28,
                    by + 152,
                    MAIN_W - 56,
                    &format!("出力先 : {}", out),
                    t.text_secondary,
                    FontSize::Small,
                );

                let btn_y = by + 216;
                c.round_rect(MAIN_X + 28, btn_y, 200, 44, 6, t.bg_btn);
                c.round_rect_border(MAIN_X + 28, btn_y, 200, 44, 6, 1, t.border_subtle);
                btn_text(c, MAIN_X + 28, btn_y, 200, 44, &pill_word(ed, "Y", "出力名変更"), t.text_primary, FontSize::Small);
                self.add_hit(MAIN_X + 28, btn_y, 200, 44, UiAction::Key(Key::Y));

                c.round_rect(MAIN_X + 244, btn_y, 220, 44, 6, t.accent_primary);
                btn_text(
                    c,
                    MAIN_X + 244,
                    btn_y,
                    220,
                    44,
                    &pill_word(ed, "A", "変換出力"),
                    white_or_dark(t.accent_primary),
                    FontSize::Medium,
                );
                self.add_hit(MAIN_X + 244, btn_y, 220, 44, UiAction::Key(Key::A));
            }
        }
    }

    fn draw_options(&mut self, c: &mut Canvas, ed: &Editor) {
        let o = ed
            .save
            .as_ref()
            .map(|s| s.options())
            .unwrap_or_else(Options::default);
        let rows = alloc::vec![
            (
                0,
                String::from("画面の明るさ"),
                String::from("Brightness"),
                format!(
                    "{}（上限 {}）",
                    o.brightness,
                    mhnx_core::hbedit::OPTION_BRIGHTNESS_MAX
                ),
                String::from("#01"),
            ),
            (
                1,
                String::from("振動設定"),
                String::from("Rumble"),
                format!(
                    "{}（上限 1）",
                    if o.rumble != 0 { "ON" } else { "OFF" }
                ),
                String::from("#02"),
            ),
            (
                2,
                String::from("3D表示無効"),
                String::from("3DS Flag"),
                format!(
                    "{}（上限 1）",
                    if o.disable_3d != 0 { "YES" } else { "NO" }
                ),
                String::from("#03"),
            ),
            (
                3,
                String::from("拡張スライドパッド"),
                String::from("3DS Flag"),
                format!(
                    "{}（上限 1）",
                    if o.circle_pad_pro != 0 { "ON" } else { "OFF" }
                ),
                String::from("#04"),
            ),
        ];
        let sub = format!(
            "明るさ 0〜{} / フラグは 0〜1",
            mhnx_core::hbedit::OPTION_BRIGHTNESS_MAX
        );
        // The two words the panel used to print were both wrong: "[A] 切替"
        // named a key that saves the card, and the step label named a key that
        // restores the snapshot. [A]'s word is the state machine's now
        // (options_a_label), and the step label names no key (step_table) - the
        // five pills are the only step control here. The stepper row stays
        // because [MAX] lives in it and really does clamp the row
        // (Editor::option_row_max); the arrows themselves step by one, which the
        // footer says.
        let a_label = ed.options_a_label();
        self.draw_adaptive_rows(
            c,
            ed,
            "環境設定",
            &sub,
            &rows,
            ed.option_cursor.min(3),
            true,
            a_label,
        );
    }

    fn draw_char(&mut self, c: &mut Canvas, ed: &Editor) {
        let is_appearance = ed.field >= 10;
        let range = if is_appearance {
            10..CHAR_FIELDS.len()
        } else {
            0..10.min(CHAR_FIELDS.len())
        };

        let mut rows = Vec::new();
        for f in range {
            let label = CHAR_FIELDS[f].1.to_string();
            let val = if f == 0 {
                ed.save
                    .as_ref()
                    .and_then(|s| s.read_character(ed.slot))
                    .map(|ch| ch.name)
                    .unwrap_or_else(|| String::from("-"))
            } else {
                let raw_v = ed.char_value(f);
                match f {
                    1 => format!("{} z", raw_v),
                    2 => format!("{}h {:02}m", raw_v / 3600, (raw_v % 3600) / 60),
                    5..=9 => format!("{} pts", raw_v),
                    13 => String::from(if raw_v == 0 { "男性 (0)" } else { "女性 (1)" }),
                    _ => format!("{}", raw_v),
                }
            };
            // The ceilings are the ones the state machine clamps to
            // (hbedit::clamp_char and the *_MAX constants), so the row cannot
            // advertise a number the writer refuses. The name is 31 **bytes**
            // (ten wide glyphs), not the 12 the row used to claim.
            let meta = match f {
                0 => format!("上限 {} バイト（全角10文字）", mhnx_core::hbedit::CHAR_NAME_MAX_BYTES),
                1 => format!("上限 {}", mhnx_core::hbedit::CHAR_FUNDS_MAX),
                2 => String::from("秒単位"),
                3 => format!("上限 {}", mhnx_core::hbedit::CHAR_HR_MAX),
                4 => format!("上限 {}", mhnx_core::hbedit::CHAR_HRP_MAX),
                5..=9 => format!("上限 {}", mhnx_core::hbedit::CHAR_POINT_MAX),
                13 => String::from("0: 男性 / 1: 女性"),
                10..=16 => format!("上限 {}", mhnx_core::hbedit::CHAR_LOOK_MAX),
                _ => String::from("外見パラメータ"),
            };
            let badge = format!("#{:02}", f + 1);
            rows.push((f, label, meta.to_string(), val, badge));
        }

        let hdr = if is_appearance {
            "外見・性別・ボイス"
        } else {
            "基本ステータス・所持金・ポイント"
        };
        // The [A] word follows the row, and the key table follows it too
        // (Editor::char_a_label): the footer said 最大（2回）/編集 while this
        // button said 名前変更 / 最大値 for the same press.
        let a_lbl = ed.key_label_or("A", "最大（2回）");

        self.draw_adaptive_rows(c, ed, hdr, "", &rows, ed.field, true, &a_lbl);
    }

    fn draw_items(&mut self, c: &mut Canvas, ed: &Editor) {
        if ed.item_menu_open {
            self.draw_item_menu(c, ed);
            return;
        }
        let vis = ed.visible_items();
        let all_slots = ed.save.as_ref().and_then(|s| s.read_item_box(ed.slot));
        let mut rows = Vec::with_capacity(vis.len());

        for (list_idx, &box_slot) in vis.iter().enumerate() {
            let (id, cnt) = all_slots
                .as_ref()
                .and_then(|b| b.get(box_slot))
                .map(|it| (it.id, it.count))
                .unwrap_or((0, 0));
            let name = if id == 0 {
                String::from("(空き)")
            } else {
                item_name_of(id)
            };
            let page = box_slot / 100 + 1;
            let pos = box_slot % 100 + 1;
            let val = if id == 0 {
                String::from("-")
            } else {
                format!("×{}（上限 {}）", cnt, mhnx_core::itembox::MAX_COUNT)
            };
            rows.push((
                list_idx,
                name,
                format!("BOX P{:02}-{:03} / ID:{:04}", page, pos, id),
                val,
                format!("#{:04}", box_slot + 1),
            ));
        }

        // Three states, one heading, and the heading is the state machine's
        // (hbedit::Editor::item_box_heading) so the PC build prints the same
        // words: a search result names the search, 空きも表示 says that empty
        // slots are being shown - the word was on screen even while the state
        // could not be reached - and the default says 所持品.
        let hdr = ed.item_box_heading();
        let sub = ed.item_box_search_line(rows.len());
        self.draw_adaptive_rows(
            c,
            ed,
            &hdr,
            &sub,
            &rows,
            ed.cursor.min(rows.len().saturating_sub(1)),
            true,
            // The [A] word is the key table's (Editor::key_label_or): the footer
            // said 変更 and this button said アイテム変更.
            &ed.key_label_or("A", "変更"),
        );
    }

    /// The bulk operation menu (`-` on the item screen).
    ///
    /// Every row shows how much it would change before it runs, and the
    /// dedupe row shows what the merge would drop: the two rows that can lose
    /// data never run on the first press.
    fn draw_item_menu(&mut self, c: &mut Canvas, ed: &Editor) {
        let stats = ed.item_menu_stats();
        let mut rows = Vec::with_capacity(ITEM_OPS.len());
        for (i, (label, note)) in ITEM_OPS.iter().enumerate() {
            let value = stats.value(i);
            let armed_row = ed.item_armed == Some(i);
            // "2回" says a second press is wanted before it is asked for, and
            // the armed row says which press is left. Short enough not to push
            // the value column past its 230px ceiling.
            let badge = if armed_row {
                "2回目"
            } else if Editor::item_op_needs_second(i) {
                "2回"
            } else if i == ITEM_OP_UNDO {
                "-"
            } else {
                "A"
            };
            rows.push((
                i,
                String::from(*label),
                String::from(*note),
                value,
                String::from(badge),
            ));
        }
        let armed = ed.item_armed == Some(ed.item_menu_cursor);
        let hdr = if armed {
            String::from("アイテム一括操作 — もう一度 [A] で実行（[B] 中止）")
        } else {
            format!(
                "アイテム一括操作  (使用 {} 枠 / 空き {} 枠)",
                mhnx_core::itembox::TOTAL_ITEM_SLOTS - stats.holes,
                stats.holes
            )
        };
        let a_lbl = if armed {
            "実行する"
        } else if Editor::item_op_needs_second(ed.item_menu_cursor) {
            "確認（2回）"
        } else if ed.item_menu_cursor == ITEM_OP_ADD {
            "選ぶ"
        } else {
            "実行"
        };
        self.draw_adaptive_rows(
            c,
            ed,
            &hdr,
            "",
            &rows,
            ed.item_menu_cursor.min(ITEM_OPS.len().saturating_sub(1)),
            false,
            a_lbl,
        );
    }

    fn draw_item_pick(&mut self, c: &mut Canvas, ed: &Editor) {
        let matches = ed.item_pick_matches();
        let adding = ed.item_pick_mode == 3;
        // Stage 2 of the bulk add: the list stays (it names the item) but the
        // number under the cursor is what Left / Right is changing.
        let count_stage = adding && ed.item_pick_stage == 1;
        let mut rows = Vec::with_capacity(matches.len());
        for (i, &id) in matches.iter().enumerate() {
            let val = if count_stage && i == ed.item_pick_cursor {
                format!("×{}", ed.item_pick_add_count)
            } else {
                String::new()
            };
            rows.push((
                i,
                item_name_of(id),
                format!("Item ID: {}", id),
                val,
                format!("{:04}", id),
            ));
        }
        let hdr = if count_stage {
            format!(
                "アイテム追加 2/2  個数: {} ×{}",
                item_name_of(ed.item_pick_add_id),
                ed.item_pick_add_count
            )
        } else if adding {
            String::from("アイテム追加 1/2  アイテムを選択")
        } else {
            String::from("アイテム選択")
        };
        // What the filter is and what it left, beside the title: "検索: ほげ（12件）",
        // the whole table, or the notice that this build can only search ids.
        // The state machine writes the line, so the drawn header and the text
        // console cannot word it differently.
        let sub = ed.item_pick_search_line(matches.len());
        self.draw_adaptive_rows(
            c,
            ed,
            &hdr,
            &sub,
            &rows,
            ed.item_pick_cursor.min(rows.len().saturating_sub(1)),
            count_stage,
            if count_stage { "この個数を追加" } else { "決定" },
        );
    }

    fn draw_skill_pick(&mut self, c: &mut Canvas, ed: &Editor) {
        let matches = ed.skill_pick_matches();
        let mut rows = Vec::with_capacity(matches.len());
        for (i, &id) in matches.iter().enumerate() {
            let name = mhnx_core::skills::skill_name(id).unwrap_or("?").to_string();
            rows.push((
                i,
                name,
                format!("Skill ID: {}", id),
                String::new(),
                format!("{:03}", id),
            ));
        }
        // The heading names the slot and the kind and says how many ids the
        // kind offers, so a short list explains itself: the pool is the game's
        // own point table for that rarity and slot (62 for a 天's first skill,
        // 0 for any 兵士 second skill), not the 306 entry name table.
        let hdr = ed.skill_pick_heading();
        self.draw_adaptive_rows(
            c,
            ed,
            &hdr,
            "",
            &rows,
            ed.skill_pick_cursor.min(rows.len().saturating_sub(1)),
            false,
            "決定",
        );
    }

    fn draw_charms(&mut self, c: &mut Canvas, ed: &Editor) {
        let list = ed.charms();
        let mut rows = Vec::with_capacity(list.len());
        for (i, chm) in list.iter().enumerate() {
            let s1 = mhnx_core::skills::skill_name(chm.skill1).unwrap_or("なし");
            let s2 = mhnx_core::skills::skill_name(chm.skill2).unwrap_or("なし");
            let summary = if chm.skill2 != 0 {
                format!("{} {:+} / {} {:+}", s1, chm.points1, s2, chm.points2)
            } else {
                format!("{} {:+}", s1, chm.points1)
            };
            // The kind is what the game calls the talisman, so it opens the
            // row: 「龍の護石 攻撃 +10」 reads like the box screen.
            let kind = ed
                .save
                .as_ref()
                .and_then(|s| s.charm_kind_name(ed.slot, chm.index))
                .unwrap_or("—");
            // The measured ceiling of this kind, from charm::KIND_LIMITS. It
            // is a constant, so the list can say what each row is allowed to
            // hold without reading the romfs tables.
            let limits = ed
                .save
                .as_ref()
                .and_then(|s| mhnx_core::charm::charm_kind(s, ed.slot, chm.index))
                .and_then(mhnx_core::charm::kind_limits);
            // The kind's slot ceiling goes on the row itself: the meta line is
            // the inspector's and only the selected row is on screen there.
            let slots_str = match limits {
                Some(l) => format!("S{}（上限 S{}）", chm.slots, l.max_slots),
                None => format!("S{}（上限 —）", chm.slots),
            };
            rows.push((
                i,
                format!("{} {}", kind, summary),
                format!(
                    "装備BOX枠 #{:04} / {}",
                    chm.index,
                    limits
                        .map(|l| l.short())
                        .unwrap_or_else(|| String::from("上限 —"))
                ),
                slots_str,
                format!("#{:02}", i + 1),
            ));
        }
        self.draw_adaptive_rows(
            c,
            ed,
            &ed.charm_filter_heading(),
            "",
            &rows,
            ed.cursor.min(rows.len().saturating_sub(1)),
            false,
            // 編集 / 作成 / 該当なし: the word is the state machine's
            // (Editor::charm_a_label), so the button names the press it runs.
            ed.charm_a_label(),
        );
    }

    fn draw_charm_edit(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(ce) = ed.charm_edit else {
            return;
        };
        // The seven rows and the strings they carry live in the state machine
        // (hbedit::charm_form_rows), so the host tests read exactly what this
        // list draws. The measured ceiling of the kind is NOT one of the row
        // strings the list draws: this list draws a row's title and value only,
        // and a row's meta is the inspector's line for the selected row. It is
        // passed below as the header note instead, where it is always on
        // screen.
        let rows: alloc::vec::Vec<(usize, String, String, String, String)> = charm_form_rows(&ce)
            .into_iter()
            .map(|r| (r.field, r.title, r.meta, r.value, r.badge))
            .collect();

        // Warnings for the record in the box and for what the form would write.
        let box_bytes = ed.save.as_ref().and_then(|s| s.equipment_box(ed.slot));
        let warnings = charm_warnings(
            box_bytes,
            ce.index,
            ce.grade,
            ce.kind,
            ce.slots,
            ce.skill1,
            ce.points1,
            ce.skill2,
            ce.points2,
        );

        // [A] means one thing per row, and the label, the footer and the key
        // all come from the same value (hbedit::CharmAction), so the button can
        // no longer promise 反映 while the key opens the skill list.
        self.draw_adaptive_rows_warn(
            c,
            ed,
            "護石編集",
            &ce.header_note(),
            &rows,
            // The form has seven rows (0..=6); clamping to 5 left the last row
            // unhighlighted whenever the second skill's value was being edited.
            ce.field.min(CHARM_EDIT_ROWS - 1),
            true,
            ce.a_label(),
            &[],
            &warnings,
        );
    }

    fn draw_equip(&mut self, c: &mut Canvas, ed: &Editor) {
        // The box as this screen lists it: weapons and armour. A talisman is
        // type 6 and has a screen and a form of its own (護石一覧 / 護石編集),
        // so it is not a row here (hbedit::Editor::equip_rows).
        let all = ed.equip_rows();
        let mut rows = Vec::with_capacity(all.len());
        for (i, eq) in all.iter().enumerate() {
            let tname = mhnx_core::equip::type_name(eq.kind);
            let ename = mhnx_core::equip::name(eq).to_string();
            // The name leads and the id is beside it, in hex: the spelling
            // every equipment row uses. The decimal number is nowhere on this
            // screen, so a reader who copies an id off it can type it back.
            rows.push((
                i,
                ename,
                format!("枠 #{:04} / {} / ID 0x{:02X}", eq.index, tname, eq.id),
                format!("Lv{}", eq.level),
                tname.to_string(),
            ));
        }
        // The strip under the list is for warnings only: a pending operation
        // (amber - the key guide cannot say which record is armed, and both a
        // delete and a type change fire on one press unless the screen says
        // they are waiting) and a record that does not fit its own type. The
        // permanent key explanation that used to live here is the footer's job,
        // and it cost the list a row: eight rows with nothing to warn about.
        let mut warnings: Vec<String> = Vec::new();
        if let Some((index, refs, _)) = ed.equip_delete_pending() {
            warnings.push(format!(
                "削除の確認中: 枠 #{:04} / 参照 {} 件",
                index, refs
            ));
            warnings.push(if refs == 0 {
                String::from("もう一度 [-] で削除確定 / [B] で取りやめ")
            } else {
                String::from("もう一度 [-] で強制削除 / 参照は下のメッセージ")
            });
        } else if let Some((_, _, text)) = ed.equip_kind_pending() {
            // The second [A] writes; anything else (a cursor move, [B]) drops
            // it, so the line has to say what the press is waiting for.
            warnings.push(text);
            warnings.push(String::from("前の種別のデータは消しません（警告のみ）"));
        }
        // The record under the cursor: what does not fit the type it carries.
        // A kind change keeps every byte, so this is where a converted record
        // shows why it reads oddly - and it is the same list the PC build
        // prints under the record (equipops::kind_warnings).
        if let Some(save) = &ed.save {
            if let Some(eq) = all.get(ed.equip_cursor) {
                for w in mhnx_core::equipops::kind_warnings(save, ed.slot, eq.index)
                    .into_iter()
                    .take(2)
                {
                    warnings.push(w);
                }
            }
        }
        // The heading carries the filter: a filtered list that said nothing
        // looked like a box that had lost rows, and the key that cleared it
        // was [X], which is the save key everywhere.
        self.draw_adaptive_rows_warn(
            c,
            ed,
            &ed.equip_filter_heading(),
            "",
            &rows,
            ed.equip_cursor.min(rows.len().saturating_sub(1)),
            false,
            ed.equip_a_label(),
            &[],
            &warnings,
        );
    }

    /// The equipment form. Nine rows when an existing record is edited, three
    /// when a new one is being written: one row per field, the value on the
    /// right, the hint in the meta column, so no row needs two lines and
    /// nothing can overlap.
    fn draw_equip_edit(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(ee) = ed.equip_edit else {
            return;
        };
        let eq = ed
            .save
            .as_ref()
            .and_then(|s| s.equipment_at(ed.slot, ee.index));
        let ename = eq
            .as_ref()
            .map(|q| mhnx_core::equip::name(q).to_string())
            .unwrap_or_default();
        // The rows and the words for [A] live in the state machine
        // (hbedit::equip_form_rows / EquipFormAction), so the host tests read
        // exactly what this list draws - the same arrangement the talisman form
        // uses. Each title already carries its own "▸ 選ぶ" / "▸ 書き込み", so a
        // row says what [A] does without the footer being read first.
        let rows: alloc::vec::Vec<(usize, String, String, String, String)> =
            mhnx_core::hbedit::equip_form_rows(&ee, &ename)
                .into_iter()
                .map(|r| (r.field, r.title, r.meta, r.value, r.badge))
                .collect();

        if ee.new {
            // One line, grey: it explains the keys, it is not a warning. The
            // sentence that matters (the last [A] creates the record) is the
            // one that stays.
            let notice = alloc::vec![String::from(
                "最後の [A] で空き枠に作成します（[B] で中止・種別と ID は [A] で一覧）",
            )];
            self.draw_adaptive_rows_warn(
                c,
                ed,
                "装備を新規作成",
                "",
                &rows,
                ee.field.min(rows.len().saturating_sub(1)),
                true,
                ee.a_label(),
                &notice,
                &[],
            );
            return;
        }

        // Whether [A] patches the record or replaces it is decided by these
        // two fields, so the strip says which one is about to happen.
        let identity_changed = eq
            .as_ref()
            .map(|q| q.kind != ee.kind || q.id != ee.id)
            .unwrap_or(false);
        // The identity rows decide whether [A] patches the record or replaces
        // it, and that is a real warning: amber, and it gets the strip. The
        // ordinary explanation is one grey line.
        let (notice, warnings): (Vec<String>, Vec<String>) = if identity_changed {
            let mut w = alloc::vec![String::from(
                "種別/ID を変えて書き込むと差し替えます（装飾品3枠は外れます）",
            )];
            w.push(if eq.as_ref().map(|q| q.kind != ee.kind).unwrap_or(false) {
                // A type change asks once more before it writes: the first [A]
                // only says what the change costs (hbedit::Editor::
                // equip_kind_pending).
                String::from("種別が変わります: もう一度 [A] で実行 / [B] で取りやめ")
            } else {
                String::from("名前行は [A] で一覧、書き込みは「装備枠」の行の [A] です")
            });
            (Vec::new(), w)
        } else {
            // Nothing to say: the footer names [A] 行の操作 and [X] 保存, and a
            // strip here cost the nine-row form its ninth row.
            (Vec::new(), Vec::new())
        };

        self.draw_adaptive_rows_warn(
            c,
            ed,
            "装備編集",
            "",
            &rows,
            ee.field.min(rows.len().saturating_sub(1)),
            true,
            ee.a_label(),
            &notice,
            &warnings,
        );
    }

    /// Decoration name picker, opened from the equipment form with [A] on a
    /// socket row ([Y] opens the same list with the search box already up).
    fn draw_jewel_pick(&mut self, c: &mut Canvas, ed: &Editor) {
        let matches = ed.jewel_pick_matches();
        let mut rows = Vec::with_capacity(matches.len());
        for (i, &id) in matches.iter().enumerate() {
            rows.push((
                i,
                decoration_name_of(id),
                if id == 0 {
                    String::from("空きスロットに戻す")
                } else {
                    format!("Decoration ID: {}", id)
                },
                String::new(),
                format!("{:04}", id),
            ));
        }
        let hdr = if ed.jewel_pick_filter.is_empty() {
            String::from("装飾品選択")
        } else {
            format!("装飾品選択 (検索: {})", ed.jewel_pick_filter)
        };
        self.draw_adaptive_rows(
            c,
            ed,
            &hdr,
            "",
            &rows,
            ed.jewel_pick_cursor.min(rows.len().saturating_sub(1)),
            false,
            "決定",
        );
    }

    /// Equipment name picker, opened from the form's 種別 / ID row with [A]
    /// (the primary route; [Y] opens the same list with the search box up).
    ///
    /// The row is the name with the id beside it - 「爆砕の大剣  0x28」 - and the
    /// id is hex because that is what the box, the form and this list print;
    /// no screen in this build spells an equipment id in decimal. The type goes
    /// on the inspector's meta line, which only the selected row draws, so a
    /// list of 6,435 rows does not build 6,435 of those strings.
    fn draw_equip_pick(&mut self, c: &mut Canvas, ed: &Editor) {
        let choices = ed.equip_pick_choices();
        let sel = ed.equip_pick_cursor;
        let mut rows = Vec::with_capacity(choices.len());
        for (i, ch) in choices.iter().enumerate() {
            let meta = if i == sel {
                format!(
                    "{} / {} / ID {}",
                    mhnx_core::equip::type_name(ch.kind),
                    ed.equip_pick_scope.label(),
                    ch.hex()
                )
            } else {
                String::new()
            };
            rows.push((i, ch.name().to_string(), meta, ch.hex(), String::new()));
        }
        // What the list is narrowed to and what is left: "検索: 大剣（137件）",
        // "全 1,606件", or the notice that this build can only search ids. The
        // state machine words it, so the drawn header and the status line say
        // the same thing.
        let hdr = format!("装備を名前で選ぶ（{}）", ed.equip_pick_kind_label());
        let sub = ed.equip_pick_search_line(choices.len());
        self.draw_adaptive_rows(
            c,
            ed,
            &hdr,
            &sub,
            &rows,
            sel.min(rows.len().saturating_sub(1)),
            false,
            "この装備を選ぶ",
        );
    }

    fn draw_sets(&mut self, c: &mut Canvas, ed: &Editor) {
        // A form is a mode of this screen, so it owns the panel while it is
        // open and the list comes back on B.
        if ed.set_edit.is_some() {
            self.draw_set_edit(c, ed);
            return;
        }
        if ed.item_set_edit.is_some() {
            self.draw_item_set_edit(c, ed);
            return;
        }
        let Some(save) = &ed.save else {
            return;
        };
        let mut rows = Vec::new();
        if self.sets_tab == 0 {
            for (i, s) in mhnx_core::myset::item_sets(save, ed.slot).into_iter().enumerate() {
                if !s.is_empty() {
                    let n = s.used_items().len();
                    let idx = rows.len();
                    rows.push((
                        idx,
                        s.name,
                        format!("アイテムセット #{}", i + 1),
                        format!("{}枠", n),
                        format!("#{:02}", i + 1),
                    ));
                }
            }
        } else {
            for (i, s) in mhnx_core::myset::equip_sets(save, ed.slot).into_iter().enumerate() {
                if !s.is_empty() {
                    // NONE is 0xFFFF, not 0: counting "not zero" used to call
                    // an empty part a used one and print 7 部位 for every set.
                    let n = s
                        .parts
                        .iter()
                        .filter(|&&p| p != mhnx_core::myset::NONE)
                        .count();
                    let idx = rows.len();
                    rows.push((
                        idx,
                        s.name,
                        format!("装備セット #{}", i + 1),
                        format!("{}部位", n),
                        format!("#{:02}", i + 1),
                    ));
                }
            }
        }
        let hdr = if self.sets_tab == 0 {
            "マイセット (アイテム)"
        } else {
            "マイセット (装備)"
        };
        self.draw_adaptive_rows(
            c,
            ed,
            hdr,
            "",
            &rows,
            self.sets_cursor.min(rows.len().saturating_sub(1)),
            false,
            "編集",
        );
    }

    /// The equipment set form: one row for the name, one per part.
    ///
    /// Every row fits on the panel at once (eight rows), so there is nothing to
    /// page and the cursor is the row. The amber strip carries `myset::warnings`
    /// for the record as it is stored: [X] re-reads them, they are collected when
    /// the editor opens and after every write, never while drawing.
    fn draw_set_edit(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(e) = ed.set_edit else {
            return;
        };
        let name = ed.editing_set().map(|r| r.name).unwrap_or_default();
        let shown_name = if name.is_empty() {
            String::from("(無題)")
        } else {
            name.clone()
        };
        let mut rows = Vec::with_capacity(SET_EDIT_ROWS);
        rows.push((
            SET_ROW_NAME,
            String::from("名前"),
            String::from("最大31バイト / 改行可（[A] で入力）"),
            shown_name,
            String::from("#01"),
        ));
        for part in 0..mhnx_core::myset::EQUIP_SET_PARTS {
            let row = part + 1;
            let idx = ed.set_pick(part);
            let meta = if idx == mhnx_core::myset::NONE {
                String::from("◀ ▶ で装備BOX の使用中を巡回")
            } else {
                format!("枠 #{:04} / ◀ ▶ で巡回  [A] で決定", idx)
            };
            let armed = ed.set_clear_armed == Some(part);
            rows.push((
                row,
                String::from(mhnx_core::myset::PART_NAMES[part]),
                meta,
                ed.set_part_label(part),
                if armed {
                    String::from("解除?")
                } else {
                    format!("#{:02}", row + 1)
                },
            ));
        }
        let notice: Vec<String> = ed.set_warnings.clone();
        let hdr = format!("マイセット (装備) #{:02} を編集", e.set + 1);
        self.draw_adaptive_rows_warn(
            c,
            ed,
            &hdr,
            "",
            &rows,
            e.field.min(rows.len().saturating_sub(1)),
            // No numeric stepper: a row names an equipment record, and the
            // candidate walk is in this form's own tool deck below. That also
            // means no [MAX] here, which is the truth - the row has no ceiling
            // to jump to.
            false,
            if e.field == SET_ROW_NAME {
                "名前入力"
            } else {
                "決定"
            },
            &notice,
            &[],
        );
    }

    /// The item set form: one row per entry, thirty-two of them.
    ///
    /// The same list/inspector shape the equipment form uses, with the rows
    /// paged by the list itself: a set holds more rows than the panel shows, so
    /// the cursor is the scroll. The amber strip carries
    /// `myset::item_set_warnings` for the record as it is stored.
    fn draw_item_set_edit(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(e) = ed.item_set_edit else {
            return;
        };
        let set_name = ed
            .editing_item_set()
            .map(|r| r.name)
            .unwrap_or_default();
        let used = ed
            .editing_item_set()
            .map(|r| r.used_items().len())
            .unwrap_or(0);
        let mut rows = Vec::with_capacity(ITEM_SET_EDIT_ROWS);
        for row in 0..ITEM_SET_EDIT_ROWS {
            let item = e.pick[row];
            let empty = item.id == 0;
            let armed = ed.item_set_armed.is_some() && row == e.field;
            let badge = if armed {
                match ed.item_set_armed {
                    Some(ItemSetOp::FromPouch) => String::from("取込?"),
                    Some(ItemSetOp::ToPouch) => String::from("適用?"),
                    None => format!("#{:02}", row + 1),
                }
            } else {
                format!("#{:02}", row + 1)
            };
            rows.push((
                row,
                if empty {
                    String::from("空き")
                } else {
                    item_name_of(item.id)
                },
                if empty {
                    String::from("◀ ▶ でアイテムBOX の使用中を巡回")
                } else {
                    format!("ID {} / ◀ ▶ で巡回  [A] で決定", item.id)
                },
                if empty {
                    String::new()
                } else {
                    format!("×{}", item.count)
                },
                badge,
            ));
        }
        let notice: Vec<String> = ed.item_set_warnings.clone();
        let hdr = format!(
            "マイセット (アイテム) #{:02} を編集 — {} / {} 枠",
            e.set + 1,
            if set_name.is_empty() {
                String::from("(無題)")
            } else {
                mhnx_core::hbedit::name_preview(&set_name)
            },
            used
        );
        self.draw_adaptive_rows_warn(
            c,
            ed,
            &hdr,
            "",
            &rows,
            e.field,
            false,
            "決定",
            &notice,
            &[],
        );
    }

    /// The CSV screen: the folder's files, one row each.
    ///
    /// The rows say what each file holds (the name's stem, then the header when
    /// it is applied) and the amber strip carries the last report - the counts
    /// the apply or the export produced.
    fn draw_csv(&mut self, c: &mut Canvas, ed: &Editor) {
        let mut rows = Vec::with_capacity(ed.csv_files.len());
        for (i, path) in ed.csv_files.iter().enumerate() {
            let name = path.rsplit('/').next().unwrap_or(path);
            let kind = mhnx_core::hbedit::csv_kind_label(path);
            let armed = ed.csv_armed == Some(i);
            rows.push((
                i,
                String::from(name),
                format!("{} / {}", kind, CSV_DIR),
                if armed {
                    String::from("もう一度 [A]")
                } else {
                    String::from(kind)
                },
                format!("#{:02}", i + 1),
            ));
        }
        let notice: Vec<String> = ed.csv_report.clone();
        let hdr = format!("CSV 入出力 — slot {} / {}", ed.slot + 1, CSV_DIR);
        self.draw_adaptive_rows_warn(
            c,
            ed,
            &hdr,
            "",
            &rows,
            ed.csv_cursor.min(rows.len().saturating_sub(1)),
            false,
            if ed.csv_armed.is_some() {
                "確定"
            } else {
                "選択"
            },
            &notice,
            &[],
        );
    }

    /// The palico equipment panel: weapon, head and body, one row each.
    ///
    /// A mode of the roster and download-palico screens, so this draws the
    /// three rows and the note in the same list/inspector layout those screens
    /// use - the row's value is the candidate [A] would write and the meta line
    /// is what the palico wears now.
    fn draw_palico_equip(&mut self, c: &mut Canvas, ed: &Editor) {
        let panel: Vec<PalicoEquipRow> = ed.palico_equip_rows();
        let field = ed.palico_equip.map(|s| s.field).unwrap_or(0);
        let (who, roster_no) = match ed.palico_equip_target() {
            Some(p) => (p.name, p.index + 1),
            None => (
                String::from("?"),
                ed.palico_equip.map(|s| s.palico + 1).unwrap_or(0),
            ),
        };
        let mut rows = Vec::with_capacity(panel.len());
        for row in panel.iter() {
            rows.push((
                row.part,
                String::from(palico_part_name(row.part)),
                format!("現在: {}", row.worn_text()),
                row.pick_text(),
                format!("#{:02}", row.part + 1),
            ));
        }
        let hdr = format!(
            "オトモ装備 — {}（ロスター #{}）",
            mhnx_core::hbedit::name_preview(&who),
            roster_no
        );
        // The names are not in the catalogue, so the one line that says so is
        // pinned under the list, where `draw_adaptive_rows_warn` reserves room
        // for it instead of letting it draw over the last row.
        let notice = alloc::vec![String::from(PALICO_EQUIP_NAME_NOTE)];
        self.draw_adaptive_rows_warn(
            c,
            ed,
            &hdr,
            "",
            &rows,
            field.min(PALICO_EQUIP_ROWS.saturating_sub(1)),
            false,
            "装着",
            &notice,
            &[],
        );
    }

    /// The palico skill / support move panel: the twelve learned skills or the
    /// sixteen learned support moves, one row each.
    ///
    /// The other mode of the roster screen, so it uses the same list/inspector
    /// layout those screens use: a row is the id the slot holds and the candidate
    /// [A] would write, the inspector meta line carries the two tendencies the
    /// record stores, and the warnings the core reports are the amber strip under
    /// the list instead of a line drawn over the last row.
    fn draw_palico_skill(&mut self, c: &mut Canvas, ed: &Editor) {
        let panel: Vec<PalicoSkillRow> = ed.palico_skill_rows();
        let field = ed.palico_skill.map(|s| s.field).unwrap_or(0);
        let tab = ed.palico_skill_tab();
        let (used, rows_total) = ed.palico_skill_slots();
        let (who, roster_no) = match ed.palico_skill_target() {
            Some(p) => (p.name, p.index + 1),
            None => (
                String::from("?"),
                ed.palico_skill.map(|s| s.palico + 1).unwrap_or(0),
            ),
        };
        // The tendencies belong to the record, not to the row, so every row
        // carries the same line: the inspector shows whichever row is open.
        let tendencies = ed.palico_skill_tendencies();
        let mut rows = Vec::with_capacity(panel.len());
        for row in panel.iter() {
            // A slot past the length byte is the fill the game leaves there: it
            // is listed, and it says which value it holds, because [A] on it is
            // refused and the user has to be able to see why.
            let title = if !row.counted {
                format!("未使用（{}）", row.current)
            } else if row.is_empty() {
                format!("空き {}/{}", row.index + 1, rows_total)
            } else {
                row.current_text()
            };
            rows.push((
                row.index,
                title,
                tendencies.clone(),
                row.pick_text(),
                format!("#{:02}", row.index),
            ));
        }
        // Name and tab first: the list column is clipped at 620px, so the
        // part that identifies the record has to survive the clip.
        let hdr = format!(
            "オトモ{} — {}（#{}）  {}/{} 枠",
            palico_skill_tab_name(tab),
            mhnx_core::hbedit::name_preview(&who),
            roster_no,
            used,
            rows_total
        );
        let warnings = ed.palico_skill_warnings();
        self.draw_adaptive_rows_warn(c, ed, &hdr, "", &rows, field, false, "差し替え", &[], &warnings);
    }

    fn draw_roster(&mut self, c: &mut Canvas, ed: &Editor) {
        let list = ed.palicos();
        let target_col = match ed.roster_field {
            0 => "Lv",
            1 => "なつき度",
            _ => "EXP",
        };
        let mut rows = Vec::with_capacity(list.len());
        for (i, p) in list.iter().enumerate() {
            let val = match ed.roster_field {
                0 => format!("Lv {}（上限 {}）", p.level, mhnx_core::hbedit::PALICO_LEVEL_MAX),
                1 => format!(
                    "なつき {}（上限 {}）",
                    p.enthusiasm,
                    mhnx_core::hbedit::PALICO_ENTHUSIASM_MAX
                ),
                _ => format!("EXP {}（上限 {}）", p.exp, mhnx_core::hbedit::PALICO_EXP_MAX),
            };
            rows.push((
                i,
                p.name.clone(),
                format!("Lv:{} / なつき:{} / EXP:{}", p.level, p.enthusiasm, p.exp),
                val,
                format!("#{:02}", p.index + 1),
            ));
        }
        let hdr = format!("所持オトモ (編集列: {})", target_col);
        let sub = format!(
            "Lv {} / なつき {} / EXP {} が上限",
            mhnx_core::hbedit::PALICO_LEVEL_MAX,
            mhnx_core::hbedit::PALICO_ENTHUSIASM_MAX,
            mhnx_core::hbedit::PALICO_EXP_MAX
        );
        self.draw_adaptive_rows(
            c,
            ed,
            &hdr,
            &sub,
            &rows,
            ed.roster_cursor.min(rows.len().saturating_sub(1)),
            true,
            // The [A] word is the key table's: the footer said 名前 and this
            // button said 名前変更.
            &ed.key_label_or("A", "名前"),
        );
    }

    fn draw_palico(&mut self, c: &mut Canvas, ed: &Editor) {
        let ids = dlcats::all_ids();
        let lang = if ed.palico_lang == 0 { Lang::Jp } else { Lang::En };
        let mut rows = Vec::with_capacity(ids.len());
        for (i, &id) in ids.iter().enumerate() {
            let name = dlcats::record(lang, id)
                .map(dlcats::record_name)
                .unwrap_or_else(|| format!("DLC #{}", id));
            // The install state is what [A] would do to this row, and the two
            // words come from the state machine so the row, the chip and the
            // key cannot disagree (hbedit::Editor::palico_row_value).
            rows.push((
                i,
                name,
                format!("DLC ID: {}", id),
                ed.palico_row_value(id).to_string(),
                format!("#{:02}", i + 1),
            ));
        }
        // The heading counts the list and the save: 一覧 / JP / EN / 導入済み.
        self.draw_adaptive_rows(
            c,
            ed,
            &ed.palico_bulk_label(),
            "",
            &rows,
            ed.palico_cursor.min(rows.len().saturating_sub(1)),
            false,
            ed.palico_row_action(ids.get(ed.palico_cursor).copied().unwrap_or(0)),
        );
    }

    fn draw_hunts(&mut self, c: &mut Canvas, ed: &Editor) {
        let hunt_list = ed.hunt_rows();
        let col_name = match ed.hunt_field {
            0 => "討伐数",
            1 => "捕獲数",
            2 => "最小サイズ",
            _ => "最大サイズ",
        };
        // The count cap and the size window are printed on every row: the
        // counters stop at 9999 in the game itself and a size record is a
        // percentage between 20 and 300 (the verified save spans exactly that).
        let count_cap = mhnx_core::hbedit::HUNT_COUNT_MAX;
        let size_cap = mhnx_core::hbedit::HUNT_SIZE_MAX;
        let mut rows = Vec::with_capacity(hunt_list.len());
        for (i, &(id, slay, cap, lo, hi)) in hunt_list.iter().enumerate() {
            let mname = monster_name(id);
            let val = match ed.hunt_field {
                0 => format!("討伐 {}（上限 {}）", slay, count_cap),
                1 => format!("捕獲 {}（上限 {}）", cap, count_cap),
                2 => format!("最小 {}%（上限 {}%）", lo, size_cap),
                _ => format!("最大 {}%（上限 {}%）", hi, size_cap),
            };
            rows.push((
                i,
                mname.to_string(),
                format!("討伐:{} / 捕獲:{} / 最小:{}% / 最大:{}%", slay, cap, lo, hi),
                val,
                format!("#{:03}", id + 1),
            ));
        }
        // The heading says which list is on screen and the deck's [-] chip
        // switches it: 記録ありのみ is the default, and without the other list a
        // never-hunted monster could never be given a first record.
        let hdr = format!("{} (編集列: {})", ed.hunt_heading(), col_name);
        let sub = format!(
            "討伐・捕獲の上限 {} / サイズ {}〜{}%",
            count_cap,
            20,
            size_cap
        );
        self.draw_adaptive_rows(
            c,
            ed,
            &hdr,
            &sub,
            &rows,
            ed.hunt_cursor.min(rows.len().saturating_sub(1)),
            true,
            "刻み切替",
        );
    }

    fn draw_guild(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(save) = &ed.save else {
            return;
        };
        let hr = mhnx_core::guildcard::hunter_rank(save, ed.slot).unwrap_or(0);
        let time = mhnx_core::guildcard::play_time(save, ed.slot).unwrap_or(0);
        let comment = mhnx_core::guildcard::comment(save, ed.slot).unwrap_or_default();
        let card_name = mhnx_core::guildcard::card_name(save, ed.slot).unwrap_or_default();
        let rows = alloc::vec![
            (
                0,
                String::from("ギルドカード HR"),
                String::from("Hunter Rank"),
                format!("HR {}（上限 {}）", hr, mhnx_core::hbedit::GUILD_HR_MAX),
                String::from("#01"),
            ),
            (
                1,
                String::from("プレイ時間"),
                String::from("Play Time"),
                format!(
                    "{}h {:02}m（上限 {}h）",
                    time / 3600,
                    (time % 3600) / 60,
                    mhnx_core::hbedit::GUILD_TIME_MAX / 3600
                ),
                String::from("#02"),
            ),
            (
                mhnx_core::hbedit::GUILD_ROW_COMMENT,
                String::from("コメント"),
                String::from("Greeting / 改行可"),
                comment,
                String::from("#03"),
            ),
            (
                mhnx_core::hbedit::GUILD_ROW_NAME,
                String::from("カード名"),
                String::from("Name / 改行可（カード側の写し）"),
                card_name,
                String::from("#04"),
            ),
        ];
        // The card ends with the arena log, so the list says where the other
        // view is: the user reported the log as missing, and a tab that is only
        // in the bar is the kind of thing that stays missing.
        let mut warnings: Vec<String> = Vec::new();
        // The two record views are one [Y] away, and the bar cannot name them:
        // the guide line is where a user finds out.
        warnings.push(String::from(
            "[Y] クエスト回数 → 武器使用数（15 種 × 3 会場、[R] で数値入力）",
        ));
        // The bar names the four views while this screen is showing
        // (sub_tabs_for), so [L/R] walks them and [B] unwinds one level at a
        // time - the line says both, because one key with two depths is the
        // kind of thing that reads as a bug until it is written down.
        // [L] alone: [R] is 数値入力 on three of the four views (the footer
        // says so, and hbedit::Editor::shoulder_job hands the key to the
        // screen), so a line that promised the shoulder pair walked the views
        // named the same key two different jobs on one screen.
        warnings.push(String::from(
            "[L] 4ビュー: カード → 回数 → 武器 → 闘技大会（闘技大会だけ [R] も / [B] は1段ずつ戻る）",
        ));
        let sub = format!(
            "HR 上限 {} / 時間 上限 {}h",
            mhnx_core::hbedit::GUILD_HR_MAX,
            mhnx_core::hbedit::GUILD_TIME_MAX / 3600
        );
        self.draw_adaptive_rows_warn(
            c,
            ed,
            "ギルドカード",
            &sub,
            &rows,
            ed.guild_field.min(mhnx_core::hbedit::GUILD_FIELDS - 1),
            true,
            // The row's own word: コメント is text ([A] opens the keyboard),
            // HR and 時間 are numbers ([A] adds one step, like ▶).
            ed.guild_a_label(),
            &[],
            &warnings,
        );
    }

    /// クエスト回数: the card's seven u16 counters, one row each.
    ///
    /// The card draws one bar per counter and shows their sum capped at 99,999;
    /// the sum is not a field of the save, so the header carries it and the rows
    /// are the seven things a write can touch. Each row is one u16, so the
    /// stepper and the keyboard both stop at 65,535 - the ceiling the PC build's
    /// grid prints beside the number.
    fn draw_guild_counts(&mut self, c: &mut Canvas, ed: &Editor) {
        let counts = ed.guild_counts();
        let mut rows: Vec<(usize, String, String, String, String)> = Vec::new();
        for i in 0..mhnx_core::hbedit::GUILD_COUNT_ROWS {
            rows.push((
                i,
                String::from(mhnx_core::guildcard::quest_count_label(i)),
                format!("u16 #{:02}", i + 1),
                format!("{}", counts.get(i).copied().unwrap_or(0)),
                format!("#{:02}", i + 1),
            ));
        }
        let sum: u32 = counts.iter().sum();
        let sub = format!(
            "合計 {} / カード表示 {}（上限 {}）",
            sum,
            ed.guild_count_total(),
            mhnx_core::guildcard::QUEST_TOTAL_CAP
        );
        let warnings = alloc::vec![
            String::from("[Y] 武器使用数へ / [R] で数値を直接入力"),
            String::from("合計はセーブに入りません（カードが表示のため足す値）"),
        ];
        self.draw_adaptive_rows_warn(
            c,
            ed,
            "クエスト回数",
            &sub,
            &rows,
            ed.guild_count_selected(),
            true,
            ed.guild_a_label(),
            &[],
            &warnings,
        );
    }

    /// 武器使用数: fifteen weapon kinds by the three venues, on one grid.
    ///
    /// The card stores three independent u16 arrays and no total; the "All"
    /// graph is the sum the game computes at display time. So the grid is the
    /// whole screen - one row per weapon, one column per venue, the row's total
    /// on the right - and the inspector edits the one cell the pad is on.
    ///
    /// Geometry, from the same 796 wide list panel the other screens use:
    ///
    ///   title    y 100..130, heading plus the cell counter on the right
    ///   header   y 140..172, the five column names
    ///   rows     y 172 + i*28 for i in 0..15, last row 564..592, panel ends 658
    ///     index  right-aligned to x 58 (two digits, 18px)
    ///     name   x 66..256 (190 wide; the longest name is 9 CJK = 153)
    ///     cells  x 262 + v*118, 110 wide -> 262/380/498, last ends 608
    ///     total  right-aligned to x 790
    ///   inspector x 832..1260: the cell's value, the three venues under it,
    ///     then the [A] / [R] / [X] / [B] buttons, the same shape draw_arena has.
    fn draw_guild_weapons(&mut self, c: &mut Canvas, ed: &Editor) {
        let t = theme();
        let usage = ed.guild_weapon_usage();
        let (cur_row, cur_venue) = ed.guild_weapon_selected();

        c.text_clipped(
            MAIN_X + 4,
            CONTENT_Y,
            620,
            &format!("武器使用数  slot {}", ed.slot + 1),
            t.text_primary,
            FontSize::Medium,
        );
        c.text_right(
            MAIN_X + V1_LIST_W - 4,
            CONTENT_Y + 3,
            &format!("{} 種 × {} 会場", GUILD_WEAPON_ROWS, GUILD_VENUES),
            t.text_secondary,
            FontSize::Small,
        );

        let body_y = CONTENT_Y + 30; // 132
        let body_h = FOOTER_Y - body_y - 10; // 526

        c.round_rect(MAIN_X, body_y, V1_LIST_W, body_h, t.corner_r, t.bg_panel);
        c.round_rect_border(MAIN_X, body_y, V1_LIST_W, body_h, t.corner_r, 1, t.border_subtle);

        const IDX_R: i32 = MAIN_X + 38; // 58
        const NAME_X: i32 = MAIN_X + 46; // 66
        const NAME_W: i32 = 190;
        const CELL_X: i32 = MAIN_X + 242; // 262
        const CELL_W: i32 = 110;
        const CELL_GAP: i32 = 8;
        const TOTAL_R: i32 = MAIN_X + 770; // 790
        const ROW_H: i32 = 28;
        let rows_y = body_y + 40;

        c.text_sized(NAME_X, body_y + 8, "武器", t.text_muted, FontSize::Small);
        for v in 0..GUILD_VENUES {
            let ven = mhnx_core::guildcard::Venue::ALL[v];
            let sel = v == cur_venue;
            c.text_centered(
                CELL_X + (v as i32) * (CELL_W + CELL_GAP),
                body_y + 8,
                CELL_W,
                18,
                mhnx_core::guildcard::venue_name(ven),
                if sel { t.accent_primary } else { t.text_muted },
                FontSize::Small,
            );
        }
        c.text_right(TOTAL_R, body_y + 8, "合計", t.text_muted, FontSize::Small);
        c.hline(MAIN_X + 12, body_y + 32, V1_LIST_W - 24, t.border_subtle);

        for row in 0..GUILD_WEAPON_ROWS {
            let ry = rows_y + (row as i32) * ROW_H;
            let is_row = row == cur_row;
            if is_row {
                c.round_rect(MAIN_X + 8, ry, V1_LIST_W - 16, ROW_H, 5, t.bg_selected);
            } else if row % 2 == 1 {
                c.round_rect(MAIN_X + 8, ry, V1_LIST_W - 16, ROW_H, 5, t.bg_row_odd);
            }
            // A tap on the name moves the row and keeps the venue; the cells
            // add their own, narrower hits on top of it.
            self.add_hit(
                MAIN_X + 8,
                ry,
                CELL_X - MAIN_X - 8,
                ROW_H,
                UiAction::GuildWeaponCell(row, cur_venue),
            );
            c.text_right(
                IDX_R,
                ry + 6,
                &format!("{:02}", row + 1),
                if is_row { t.accent_primary } else { t.text_muted },
                FontSize::Small,
            );
            c.text_clipped(
                NAME_X,
                ry + 6,
                NAME_W,
                mhnx_core::guildcard::weapon_type_name(row),
                if is_row { t.text_primary } else { t.text_secondary },
                FontSize::Small,
            );
            for v in 0..GUILD_VENUES {
                let cx = CELL_X + (v as i32) * (CELL_W + CELL_GAP);
                let n = usage.get(v).and_then(|a| a.get(row)).copied().unwrap_or(0);
                let sel = is_row && v == cur_venue;
                self.add_hit(cx, ry, CELL_W, ROW_H, UiAction::GuildWeaponCell(row, v));
                if sel {
                    c.round_rect(cx + 2, ry + 2, CELL_W - 4, ROW_H - 4, 4, t.bg_val_box);
                    c.round_rect_border(
                        cx + 2,
                        ry + 2,
                        CELL_W - 4,
                        ROW_H - 4,
                        4,
                        1,
                        t.border_focus,
                    );
                }
                // Zero is what an unused weapon reads as, and the game writes
                // it as 0 rather than leaving the cell out, so it is printed as
                // a number here instead of as a dash.
                c.text_right(
                    cx + CELL_W - 8,
                    ry + 6,
                    &format!("{}", n),
                    if sel {
                        t.accent_primary
                    } else if is_row {
                        t.text_primary
                    } else {
                        t.text_secondary
                    },
                    FontSize::Small,
                );
            }
            c.text_right(
                TOTAL_R,
                ry + 6,
                &format!("{}", ed.guild_weapon_total(row)),
                if is_row { t.text_primary } else { t.text_muted },
                FontSize::Small,
            );
        }

        self.draw_guild_weapon_inspector(c, ed, body_y, body_h, cur_row, cur_venue);
    }

    /// The right-hand panel of the weapon-usage grid: the cell the pad is on.
    fn draw_guild_weapon_inspector(
        &mut self,
        c: &mut Canvas,
        ed: &Editor,
        iy: i32,
        ih: i32,
        cur_row: usize,
        cur_venue: usize,
    ) {
        let t = theme();
        let ix = V1_INSP_X; // 832
        let iw = V1_INSP_W; // 428
        c.round_rect(ix, iy, iw, ih, t.corner_r, t.bg_panel);
        c.round_rect_border(ix, iy, iw, ih, t.corner_r, 1, t.border_subtle);

        c.text_sized(ix + 20, iy + 14, "選択中", t.accent_primary, FontSize::Small);
        c.text_right(
            ix + iw - 20,
            iy + 14,
            &format!("#{} / {}", cur_row + 1, GUILD_WEAPON_ROWS),
            t.text_muted,
            FontSize::Small,
        );
        c.text_clipped(
            ix + 20,
            iy + 40,
            iw - 40,
            mhnx_core::guildcard::weapon_type_name(cur_row),
            t.text_primary,
            FontSize::Medium,
        );
        let venue = mhnx_core::guildcard::Venue::ALL[cur_venue];
        c.text_clipped(
            ix + 20,
            iy + 68,
            iw - 40,
            &format!("{} の使用数", mhnx_core::guildcard::venue_name(venue)),
            t.text_secondary,
            FontSize::Small,
        );

        // The value: the cell in the largest size the panel has, with the three
        // venues underneath so a step has somewhere to show.
        let value = ed.guild_weapon_at(cur_row, cur_venue);
        c.round_rect(ix + 20, iy + 92, iw - 40, 54, 6, t.bg_val_box);
        c.round_rect_border(ix + 20, iy + 92, iw - 40, 54, 6, 1, t.border_subtle);
        btn_text(
            c,
            ix + 20,
            iy + 92,
            iw - 40,
            54,
            &format!("{}", value),
            if value == 0 { t.text_muted } else { t.accent_primary },
            FontSize::Large,
        );

        let part_w = (iw - 56) / 3; // 124
        for v in 0..GUILD_VENUES {
            let px = ix + 20 + (v as i32) * (part_w + 8);
            let active = v == cur_venue;
            let bg = if active { t.bg_selected } else { t.bg_val_box };
            c.round_rect(px, iy + 156, part_w, 64, 6, bg);
            c.round_rect_border(
                px,
                iy + 156,
                part_w,
                64,
                6,
                1,
                if active { t.border_focus } else { t.border_subtle },
            );
            let ven = mhnx_core::guildcard::Venue::ALL[v];
            let n = ed.guild_weapon_at(cur_row, v);
            c.text_sized(
                px + 10,
                iy + 164,
                mhnx_core::guildcard::venue_name(ven),
                if active { t.accent_primary } else { t.text_muted },
                FontSize::Small,
            );
            c.text_right(
                px + part_w - 10,
                iy + 186,
                &format!("{}", n),
                if active { t.accent_primary } else { t.text_primary },
                FontSize::Medium,
            );
            self.add_hit(
                px,
                iy + 156,
                part_w,
                64,
                UiAction::GuildWeaponCell(cur_row, v),
            );
        }

        c.hline(ix + 16, iy + 232, iw - 32, t.border_subtle);
        c.text_clipped(
            ix + 20,
            iy + 242,
            iw - 40,
            if ed.guild_wpn_edit {
                "編集中: [▲/▼] ±刻み  [◀/▶] ±10  [A] 終了"
            } else {
                "[A] で編集開始（値の増減）/ [R] で数値入力"
            },
            if ed.guild_wpn_edit { t.accent_warn } else { t.text_muted },
            FontSize::Small,
        );

        let a_label = if ed.guild_wpn_edit {
            "[A] 編集を終了"
        } else {
            "[A] 編集を開始"
        };
        c.round_rect(ix + 20, iy + 268, iw - 40, 44, 6, t.accent_primary);
        btn_text(
            c,
            ix + 20,
            iy + 268,
            iw - 40,
            44,
            a_label,
            white_or_dark(t.accent_primary),
            FontSize::Medium,
        );
        self.add_hit(ix + 20, iy + 268, iw - 40, 44, UiAction::Key(Key::A));

        let half = (iw - 48) / 2; // 190
        // The card's [R] word is the table's: the card view says 数値 (its line
        // is full) and the other three views say 数値入力, so a copy written
        // here would be wrong on one of them. The button prints no key: the
        // footer names it (the sixth review's leftovers).
        let r_lbl = pill_word(ed, "R", "数値入力");
        self.draw_tool_button(c, ix + 20, iy + 322, half, 40, &r_lbl, UiAction::Key(Key::R));
        self.draw_tool_button(
            c,
            ix + 28 + half,
            iy + 322,
            half,
            40,
            &pill_word(ed, "X", "保存"),
            UiAction::QuickSave,
        );
        self.draw_tool_button(
            c,
            ix + 20,
            iy + 372,
            iw - 40,
            40,
            &ed.b_word(),
            UiAction::Key(Key::B),
        );

        c.text_clipped(
            ix + 20,
            iy + 424,
            iw - 40,
            &format!("3 会場の合計 {}（カードが表示のため足す値）", ed.guild_weapon_total(cur_row)),
            t.text_muted,
            FontSize::Small,
        );
        c.text_clipped(
            ix + 20,
            iy + 448,
            iw - 40,
            "各欄は u16。上限 65535 で頭打ちです",
            t.text_muted,
            FontSize::Small,
        );
    }
    /// 闘技大会: the arena log at the end of the guild card.
    ///
    /// The PC build edits the same entries on its ギルド tab through
    /// guildcard::set_arena_time; this is the same writer with a pad. The list
    /// is the overview - every recorded time of the card on one screen, which
    /// is exactly what the user could not find - and the right panel is the one
    /// cell the cursor is on, split into 分 / 秒 / 1/100.
    ///
    /// Geometry, checked against the three font sizes this build loads
    /// (Small 17px, Medium 22px, Large 28px; a CJK glyph advances the full
    /// size, ASCII about half of it, so the widest thing a column can hold is
    /// what decides its width):
    ///
    ///   title     y 102..124 (Medium), clipped to 620 wide from x 24, so it
    ///             ends by 644; the counter on the right is "17 × 5"
    ///             (6 ASCII = 54) and starts at 812 - 54 = 758.
    ///   list      x 20..816, y 132..658.
    ///     header  y 140..157, rule y 164
    ///     rows    y 172 + i*28, i in 0..17 -> last row 620..648, panel ends
    ///             658, so ten pixels of slack.
    ///     index   right-aligned to x 58 ("17" = 18, so it starts by 40)
    ///     name    x 60..296 (236 wide; the longest name is 7 CJK = 119)
    ///     times   x 308 + r*92, 92 wide -> 308/400/492/584/676, the last ends
    ///             at 768, forty pixels inside the panel's 808 edge. A time is
    ///             at most "10:55.35", 8 ASCII = 72 at Small, right-aligned to
    ///             x + 84, so it starts no further left than 92 - 72 = 20 into
    ///             its cell: no two columns can touch.
    ///   inspector x 832..1260 (428 wide, 20 and 388 used).
    ///     y 14  caption (17 tall, ends 31)
    ///     y 40  arena name (22 tall, ends 62)
    ///     y 68  rank caption (17, ends 85)
    ///     y 92  value box 54 tall -> ends 146
    ///     y 156 parts, 64 tall -> ends 220
    ///     y 232 rule
    ///     y 242 hint (17, ends 259)
    ///     y 268 [A] 44 tall -> ends 312
    ///     y 322 two buttons 40 tall -> ends 362
    ///     y 372 [B] 40 tall -> ends 412
    ///     y 424/448 notes (17 each) -> ends 465, panel ends 658
    fn draw_arena(&mut self, c: &mut Canvas, ed: &Editor) {
        let t = theme();
        let grid = ed.arena_grid();
        let (cur_row, cur_rank) = ed.arena_selected();

        c.text_clipped(
            MAIN_X + 4,
            CONTENT_Y,
            620,
            &format!("闘技大会の記録  slot {}", ed.slot + 1),
            t.text_primary,
            FontSize::Medium,
        );
        c.text_right(
            MAIN_X + V1_LIST_W - 4,
            CONTENT_Y + 3,
            &format!("{} × {}", ARENA_ROWS, ARENA_COLUMNS),
            t.text_secondary,
            FontSize::Small,
        );

        let body_y = CONTENT_Y + 30; // 132
        let body_h = FOOTER_Y - body_y - 10; // 526

        // ---- left: the grid
        c.round_rect(MAIN_X, body_y, V1_LIST_W, body_h, t.corner_r, t.bg_panel);
        c.round_rect_border(MAIN_X, body_y, V1_LIST_W, body_h, t.corner_r, 1, t.border_subtle);

        const IDX_X: i32 = MAIN_X + 16; // 36
        const NAME_X: i32 = MAIN_X + 40; // 60
        const TIME_X: i32 = MAIN_X + 288; // 308
        const TIME_W: i32 = 92;
        const ROW_H: i32 = 28;
        let rows_y = body_y + 40;

        c.text_sized(NAME_X, body_y + 8, "アリーナ", t.text_muted, FontSize::Small);
        for r in 0..ARENA_COLUMNS {
            let sel = r == cur_rank;
            c.text_centered(
                TIME_X + (r as i32) * TIME_W,
                body_y + 8,
                TIME_W,
                18,
                arena_rank_name(r),
                if sel { t.accent_primary } else { t.text_muted },
                FontSize::Small,
            );
        }
        c.hline(MAIN_X + 12, body_y + 32, V1_LIST_W - 24, t.border_subtle);

        for a in 0..ARENA_ROWS {
            let ry = rows_y + (a as i32) * ROW_H;
            let is_row = a == cur_row;
            if is_row {
                c.round_rect(MAIN_X + 8, ry, V1_LIST_W - 16, ROW_H, 5, t.bg_selected);
            } else if a % 2 == 1 {
                c.round_rect(MAIN_X + 8, ry, V1_LIST_W - 16, ROW_H, 5, t.bg_row_odd);
            }
            // A tap on the name area moves the row and keeps the rank; the
            // cells below add their own, narrower hits on top of it.
            self.add_hit(
                MAIN_X + 8,
                ry,
                TIME_X - MAIN_X - 8,
                ROW_H,
                UiAction::ArenaCell(a, cur_rank),
            );
            c.text_right(
                IDX_X + 22,
                ry + 6,
                &format!("{:02}", a + 1),
                if is_row { t.accent_primary } else { t.text_muted },
                FontSize::Small,
            );
            c.text_clipped(
                NAME_X,
                ry + 6,
                236,
                arena_name(a),
                if is_row { t.text_primary } else { t.text_secondary },
                FontSize::Small,
            );
            for r in 0..ARENA_COLUMNS {
                let col_x = TIME_X + (r as i32) * TIME_W;
                let cell = grid.get(a).and_then(|row| row.get(r)).copied().unwrap_or(0);
                let sel = is_row && r == cur_rank;
                self.add_hit(col_x, ry, TIME_W, ROW_H, UiAction::ArenaCell(a, r));
                if sel {
                    c.round_rect(col_x + 2, ry + 2, TIME_W - 4, ROW_H - 4, 4, t.bg_val_box);
                    c.round_rect_border(
                        col_x + 2,
                        ry + 2,
                        TIME_W - 4,
                        ROW_H - 4,
                        4,
                        1,
                        t.border_focus,
                    );
                }
                let (text, color) = if cell == 0 {
                    (
                        String::from("-"),
                        if sel { t.accent_warn } else { t.text_muted },
                    )
                } else {
                    (
                        arena_clock(cell),
                        if sel {
                            t.accent_primary
                        } else if is_row {
                            t.text_primary
                        } else {
                            t.text_secondary
                        },
                    )
                };
                c.text_right(col_x + TIME_W - 8, ry + 6, &text, color, FontSize::Small);
            }
        }

        self.draw_arena_inspector(c, ed, body_y, body_h, cur_row, cur_rank);
    }

    /// The right-hand panel of the arena view: the cell the pad is on.
    fn draw_arena_inspector(
        &mut self,
        c: &mut Canvas,
        ed: &Editor,
        iy: i32,
        ih: i32,
        cur_row: usize,
        cur_rank: usize,
    ) {
        let t = theme();
        let ix = V1_INSP_X; // 832
        let iw = V1_INSP_W; // 428
        c.round_rect(ix, iy, iw, ih, t.corner_r, t.bg_panel);
        c.round_rect_border(ix, iy, iw, ih, t.corner_r, 1, t.border_subtle);

        c.text_sized(ix + 20, iy + 14, "選択中", t.accent_primary, FontSize::Small);
        c.text_right(
            ix + iw - 20,
            iy + 14,
            &format!("#{} / {}", cur_row + 1, ARENA_ROWS),
            t.text_muted,
            FontSize::Small,
        );
        c.text_clipped(ix + 20, iy + 40, iw - 40, arena_name(cur_row), t.text_primary, FontSize::Medium);
        c.text_clipped(
            ix + 20,
            iy + 68,
            iw - 40,
            &format!("{} のタイム", arena_rank_name(cur_rank)),
            t.text_secondary,
            FontSize::Small,
        );

        // The value: the whole time in the largest size the panel has, with the
        // three parts underneath so a step has somewhere to show.
        let centis = ed.arena_centis();
        let value = if centis == 0 {
            String::from("0:00.00")
        } else {
            arena_clock(centis)
        };
        c.round_rect(ix + 20, iy + 92, iw - 40, 54, 6, t.bg_val_box);
        c.round_rect_border(ix + 20, iy + 92, iw - 40, 54, 6, 1, t.border_subtle);
        btn_text(
            c,
            ix + 20,
            iy + 92,
            iw - 40,
            54,
            &value,
            if centis == 0 { t.text_muted } else { t.accent_primary },
            FontSize::Large,
        );

        let part_w = (iw - 56) / 3; // 124
        let part = ed.arena_part.min(ARENA_PARTS - 1);
        for p in 0..ARENA_PARTS {
            let px = ix + 20 + (p as i32) * (part_w + 8);
            let active = p == part;
            let bg = if active { t.bg_selected } else { t.bg_val_box };
            c.round_rect(px, iy + 156, part_w, 64, 6, bg);
            c.round_rect_border(
                px,
                iy + 156,
                part_w,
                64,
                6,
                1,
                if active { t.border_focus } else { t.border_subtle },
            );
            c.text_sized(
                px + 10,
                iy + 164,
                arena_part_name(p),
                if active { t.accent_primary } else { t.text_muted },
                FontSize::Small,
            );
            c.text_right(
                px + part_w - 10,
                iy + 186,
                &format!("{}", ed.arena_part_value(p)),
                if active { t.accent_primary } else { t.text_primary },
                FontSize::Medium,
            );
        }
        // The arrow that steps the parts is the one the footer names; the label
        // above the boxes says which part is under it while editing.
        c.hline(ix + 16, iy + 232, iw - 32, t.border_subtle);
        c.text_clipped(
            ix + 20,
            iy + 242,
            iw - 40,
            if ed.arena_edit {
                "編集中: [◀/▶] 項目  [▲/▼] 増減  [A] 終了"
            } else {
                "[A] で編集開始（分・秒・1/100 を増減）"
            },
            if ed.arena_edit { t.accent_warn } else { t.text_muted },
            FontSize::Small,
        );

        let a_label = if ed.arena_edit {
            String::from("[A] 編集を終了")
        } else {
            String::from("[A] 編集を開始")
        };
        c.round_rect(ix + 20, iy + 268, iw - 40, 44, 6, t.accent_primary);
        btn_text(
            c,
            ix + 20,
            iy + 268,
            iw - 40,
            44,
            &a_label,
            white_or_dark(t.accent_primary),
            FontSize::Medium,
        );
        self.add_hit(ix + 20, iy + 268, iw - 40, 44, UiAction::Key(Key::A));

        let half = (iw - 48) / 2; // 190
        self.draw_tool_button(c, ix + 20, iy + 322, half, 40, &pill_word(ed, "Y", "直接入力"), UiAction::Key(Key::Y));
        self.draw_tool_button(
            c,
            ix + 28 + half,
            iy + 322,
            half,
            40,
            &pill_word(ed, "X", "保存"),
            UiAction::QuickSave,
        );
        self.draw_tool_button(
            c,
            ix + 20,
            iy + 372,
            iw - 40,
            40,
            &ed.b_word(),
            UiAction::Key(Key::B),
        );

        c.text_clipped(
            ix + 20,
            iy + 424,
            iw - 40,
            "タイムは 1/100 秒。0:00.00 で記録を消します",
            t.text_muted,
            FontSize::Small,
        );
        c.text_clipped(
            ix + 20,
            iy + 448,
            iw - 40,
            "アリーナ名はクエスト一覧の順（実機未確認）",
            t.text_muted,
            FontSize::Small,
        );
    }


    /// The progress screen: the three bit maps of the character slot.
    ///
    /// Awards, request flags and quest indices are all one bit (or one index)
    /// per row, so the row is the whole interface: the title names the bit, the
    /// value says what the bit is now, and [A] flips it. The header carries the
    /// count the player checks (取得 80 / 132, 立っているビット 742) and the
    /// amber strip carries the facts a write needs: the three award copies
    /// disagreeing, the flags set outside the request bands, the cleared-in-seen
    /// invariant the core reports and deliberately never repairs.
    ///
    /// The rows are the *whole* list of the view (132 / 380 / 1508) and the
    /// list widget windows them around the cursor, exactly like the item box
    /// does with 5463 slots: one code path, one place where a row can overlap
    /// another.
    fn draw_progress(&mut self, c: &mut Canvas, ed: &Editor) {
        let mut rows: Vec<(usize, String, String, String, String)> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();
        let header: String;
        // The rows the search left, in the view's own row numbers. The list
        // draws these and nothing else, so a narrowed view cannot show a row
        // the search hid - and the ids stay the underlying rows, so [A] still
        // toggles the bit the row names (hbedit::Editor::progress_rows).
        let visible = ed.progress_rows();
        let search = ed.progress_search_line();
        match ed.progress_view() {
            PROGRESS_AWARDS => {
                let (held, total) = ed.progress_awards();
                header = format!("勲章  取得 {} / {}", held, total);
                rows.reserve(visible.len());
                for &award in visible.iter() {
                    let on = ed
                        .save
                        .as_ref()
                        .and_then(|s| mhnx_core::awards::has(s, ed.slot, award))
                        .unwrap_or(false);
                    let name = ed.award_name(award);
                    let title = if name.is_empty() {
                        // The bundled table names all 132, so this is the case
                        // of a build with neither table: the number is all
                        // there is, and the row says that instead of pretending.
                        format!("勲章 {:03}  名前不明", award + 1)
                    } else {
                        format!("勲章 {:03}  {}", award + 1, name)
                    };
                    rows.push((
                        award,
                        title,
                        String::from("ギルドカード勲章 / 3コピー同時"),
                        String::from(if on { "取得" } else { "未取得" }),
                        format!("#{:03}", award + 1),
                    ));
                }
                // A disagreement is not something [A] fixes; say it before the
                // first press rather than after it.
                if ed.progress_awards_agree() == Some(false) {
                    warnings.push(String::from("勲章の3つのコピーが一致しません"));
                    warnings.push(String::from(
                        "書き込みは3コピー同時です。先に中身を確認してください",
                    ));
                }
            }
            PROGRESS_REQUESTS => {
                let set = ed.progress_request_count();
                header = format!(
                    "依頼フラグ  立っているビット {} / {}",
                    set,
                    mhnx_core::requests::FLAGS
                );
                rows.reserve(visible.len());
                for &row in visible.iter() {
                    let Some(flag) = ed.request_flag_at(row) else {
                        continue;
                    };
                    let on = ed
                        .save
                        .as_ref()
                        .and_then(|s| mhnx_core::requests::get(s, ed.slot, flag))
                        .unwrap_or(false);
                    rows.push((
                        row,
                        format!("フラグ {:04}", flag),
                        ed.request_flag_label(flag),
                        String::from(if on { "ON" } else { "OFF" }),
                        format!("#{:03}", row + 1),
                    ));
                }
                // The flag space is shared with other unlocks; a player who
                // reads "742 bits" as "742 requests" is wrong by 444.
                let outside = set.saturating_sub(ed.progress_request_band_count());
                if outside > 0 {
                    warnings.push(format!(
                        "依頼の帯の外にも {} ビット立っています（他のイベント）",
                        outside
                    ));
                }
            }
            _ => {
                let (cleared, seen, failed) = ed.progress_quest_counts();
                header = format!(
                    "クエスト進捗  クリア {} / 既読 {} / 失敗 {}",
                    cleared, seen, failed
                );
                rows.reserve(visible.len());
                for &row in visible.iter() {
                    let (is_cleared, is_seen, _) = ed.progress_quest_state(row);
                    let value = if is_cleared {
                        "クリア"
                    } else if is_seen {
                        "既読のみ"
                    } else {
                        "-"
                    };
                    rows.push((
                        row,
                        // The index is what the save holds; the name is what the
                        // game shows, from the bundled table (mhnx_core::idmemo).
                        match ed.progress_quest_name(row) {
                            "" => format!("クエスト {:04}  名前不明", row + 1),
                            name => format!("クエスト {:04}  {}", row + 1, name),
                        },
                        // The failure bit is read-only, so its line says so
                        // (hbedit::Editor::progress_quest_meta).
                        ed.progress_quest_meta(row),
                        String::from(value),
                        format!("#{:04}", row + 1),
                    ));
                }
                let (page, pages) = ed.progress_quest_page();
                // Two short facts on one line: the page the cursor is on and
                // the rule that makes [A] and [Y] interact. Both are clipped
                // by the strip if a font metric makes them wider than the
                // panel, so they are ordered by importance.
                warnings.push(format!(
                    "ページ {} / {}（[◀/▶] {}件ずつ）・クリアで既読も立つ",
                    page,
                    pages,
                    mhnx_core::hbedit::PROGRESS_JUMP
                ));
                let broken = ed.progress_clears_without_seen();
                if broken > 0 {
                    warnings.push(format!(
                        "クリア済みなのに未読が {} 件あります（自動修正はしません）",
                        broken
                    ));
                }
            }
        }
        let a_label = match ed.progress_view() {
            PROGRESS_AWARDS => "取得切替",
            PROGRESS_REQUESTS => "フラグ切替",
            _ => "クリア切替",
        };
        // The search state rides on the heading: a narrowed list that says
        // nothing reads as a view that lost its rows, which is the mistake the
        // equipment box made before its heading learned to print the filter.
        let sub = if search.is_empty() {
            String::new()
        } else {
            search
        };
        self.draw_adaptive_rows_warn(
            c,
            ed,
            &header,
            &sub,
            &rows,
            ed.progress_cursor(),
            false,
            a_label,
            &[],
            &warnings,
        );
    }

    fn draw_save_quests(&mut self, c: &mut Canvas, ed: &Editor) {
        if ed.quest_tab == 0 {
            let list = ed.filtered_quests();
            let mut rows = Vec::with_capacity(list.len());
            for (i, q) in list.iter().enumerate() {
                let title = ed.quest_title(q.quest_id);
                rows.push((
                    i,
                    // The README's rule for a row the tables cannot name is
                    // 「名前不明」, and the progress screen and the PC build
                    // already print it. This list printed a bare "Quest 12345",
                    // which reads like a title the game has.
                    if title.is_empty() {
                        format!("クエスト {} 名前不明", q.quest_id)
                    } else {
                        title.to_string()
                    },
                    format!("Quest ID: {}", q.quest_id),
                    format!("ID {}", q.quest_id),
                    format!("#{:03}", q.slot + 1),
                ));
            }
            self.draw_adaptive_rows(
                c,
                ed,
                &format!("セーブ内クエスト ({})", rows.len()),
                "",
                &rows,
                ed.quest_list_cursor().min(rows.len().saturating_sub(1)),
                false,
                "編集",
            );
        } else {
            let list = ed.bundled_quests_for(ed.quest_tab);
            let in_save = ed.save_quests();
            let mut rows = Vec::with_capacity(list.len());
            for (i, q) in list.iter().enumerate() {
                let installed = in_save.iter().any(|sq| sq.quest_id == q.quest_id);
                let val = if installed { "導入済" } else { "未導入" };
                rows.push((
                    i,
                    q.title.to_string(),
                    format!("Quest ID: {}", q.quest_id),
                    val.to_string(),
                    format!("#{:03}", i + 1),
                ));
            }
            // The count is the list's own length, not the table's: with a
            // filter on, the list is shorter and this heading used to keep
            // claiming the whole table.
            let hdr = if ed.quest_tab == 1 {
                format!("公式イベントクエスト ({})", rows.len())
            } else {
                format!("没クエスト ({})", rows.len())
            };
            let cur = ed.quest_list_cursor().min(rows.len().saturating_sub(1));
            self.draw_adaptive_rows(c, ed, &hdr, "", &rows, cur, false, "導入/解除");
        }
    }

    fn draw_quest_equip(&mut self, c: &mut Canvas, ed: &Editor) {
        if ed.qe_open {
            let items = ed.quest_equip_items();
            let mut rows = Vec::with_capacity(items.len());
            for (i, it) in items.iter().enumerate() {
                let kind_str = match it.list {
                    mhnx_core::dlc::QuestEquipList::Supply => "支給",
                    mhnx_core::dlc::QuestEquipList::Ammo => "弾",
                };
                let iname = if it.item == 0 {
                    String::from("(空き)")
                } else {
                    item_name_of(it.item)
                };
                rows.push((
                    i,
                    iname,
                    format!("Rec {} / {}[{}] / ID:{}", it.record, kind_str, it.index, it.item),
                    format!("×{}", it.count),
                    format!("{}{:02}", kind_str, it.index + 1),
                ));
            }
            self.draw_adaptive_rows(
                c,
                ed,
                "固定装備・支給品アイテム",
                "",
                &rows,
                ed.qe_item_cursor.min(rows.len().saturating_sub(1)),
                true,
                "変更",
            );
        } else {
            let list = ed.quest_equipment();
            let mut rows = Vec::with_capacity(list.len());
            for (i, e) in list.iter().enumerate() {
                let title = if e.occupied {
                    format!("{} (ID:{})", ed.quest_title(e.quest_id), e.quest_id)
                } else {
                    String::from("(未使用)")
                };
                rows.push((
                    i,
                    title,
                    format!("固定装備枠 #{:02}", e.index),
                    String::from(if e.occupied { "使用中" } else { "-" }),
                    format!("#{:02}", e.index),
                ));
            }
            self.draw_adaptive_rows(
                c,
                ed,
                "固定装備枠一覧",
                "",
                &rows,
                ed.cursor.min(rows.len().saturating_sub(1)),
                false,
                "開く",
            );
        }
    }

    // -----------------------------------------------------------------------
    // クエスト改造: the whole quest on one screen
    // -----------------------------------------------------------------------

    /// Draw the aggregate quest screen.
    ///
    /// Four tabs over the files the quest owns, drawn with the list /
    /// inspector pair every other screen uses. The rows themselves come from
    /// hbedit (quest_basic_rows / quest_monsters / quest_rewards /
    /// quest_supplies), so what a host test reads and what this draws are the
    /// same strings - there is no second table of names here.
    fn draw_quest_edit(&mut self, c: &mut Canvas, ed: &Editor) {
        let tab = ed.quest_edit_tab.min(QUEST_TABS - 1);
        let sub = format!("{}　{}", ed.quest_heading(), QUEST_TAB_LABELS[tab]);

        match tab {
            QUEST_TAB_BASIC => {
                let basic = ed.quest_basic_rows();
                let rows: Vec<(usize, String, String, String, String)> = basic
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        let meta = match r.text_index {
                            Some(idx) => format!("[A] テキスト編集（.gmd {}）", idx),
                            None => String::from("[◀/▶] で増減（.mib 制限時間）"),
                        };
                        (
                            i,
                            String::from(r.label),
                            meta,
                            r.value.clone(),
                            quest_row_badge(i),
                        )
                    })
                    .collect();
                let cur = ed.quest_edit_cursor.min(rows.len().saturating_sub(1));
                // The button's word follows the row: three of the four rows are
                // .gmd text ([A] opens the keyboard) and the fourth is the .mib
                // 制限時間 (which steps with [◀/▶] and has no text at all).
                let a_label = ed.quest_basic_a_label();
                // The stepper is drawn on all four tabs: 制限時間 is a number
                // the arrows step, and it was the one row of the screen a
                // finger could not change (the other three tabs had the
                // stepper and this one did not).
                self.draw_adaptive_rows(c, ed, "クエスト改造", &sub, &rows, cur, true, a_label);
            }
            QUEST_TAB_MONSTERS => {
                let monsters = ed.quest_monsters();
                let mut rows: Vec<(usize, String, String, String, String)> = Vec::new();
                // The five slots, and nothing else: the ▸詳細 rows that used to
                // appear under an opened slot pushed slots 4 and 5 out of the
                // window. The numbers they held are values of the row now -
                // [ZR] walks all eleven, the .sem five included - and the
                // placement is printed on the inspector's second line.
                for slot in ed.quest_mon_rows() {
                    let Some(m) = monsters.get(slot) else { continue };
                    // The name first, the hex id small beside it, then the five
                    // numbers, so the row carries what the tab promises. The
                    // area is the value column: it is the number a custom quest
                    // gets wrong.
                    let title = format!(
                        "{}  HP:{} 攻:{} 防:{} 寸:{}% 数:{}",
                        m.label, m.hp, m.atk, m.def, m.size, m.count
                    );
                    let value = match m.area {
                        Some(a) if a > 0 => format!("エリア {}", a),
                        Some(_) => String::from("エリア 0 (未使用)"),
                        None => String::from("エリア -"),
                    };
                    // The meta names the value [ZR] is on and the slot's .sem
                    // placement. It does not name [Y]: that key is the step
                    // size here and has opened nothing since the ▸詳細 rows
                    // moved onto the row.
                    let meta = format!("スロット{}  {}", slot + 1, ed.quest_mon_meta(slot));
                    let idx = rows.len();
                    rows.push((idx, title, meta, value, quest_row_badge(idx)));
                }
                let a_label = if ed.quest_edit_field == QUEST_MON_AREA {
                    "エリア選択"
                } else if ed.quest_edit_field >= mhnx_core::hbedit::QUEST_MON_SEM_X {
                    "増減は [◀/▶]"
                } else {
                    "モンスター選択"
                };
                let cur = ed.quest_edit_cursor.min(rows.len().saturating_sub(1));
                self.draw_adaptive_rows(c, ed, "クエスト改造", &sub, &rows, cur, true, a_label);
            }
            QUEST_TAB_REWARDS => {
                let rewards = ed.quest_rewards();
                let mut rows: Vec<(usize, String, String, String, String)> = Vec::new();
                for (i, r) in rewards.iter().enumerate() {
                    let value = format!("×{}  {}%", r.count, r.chance);
                    // The file and the group are in the meta; the badge is the
                    // row's place in the tab, the same on all four tabs. It
                    // used to be "G1" - the group - which repeated itself
                    // across files, so two rows of rem_000000 and rem_000001
                    // carried the same badge.
                    let meta = format!(
                        "{}  G{} 合計{}% / {}枠{}",
                        r.file,
                        r.group + 1,
                        r.group_total,
                        r.group_len,
                        if r.dummy { "  ダミー(未読込)" } else { "" }
                    );
                    rows.push((i, r.label.clone(), meta, value, quest_row_badge(i)));
                }
                // A group that does not add up to 100 is reported, never
                // corrected - the rule the .rem screen already follows.
                let mut seen: Vec<usize> = Vec::new();
                let mut warnings: Vec<String> = Vec::new();
                for r in &rewards {
                    if r.dummy || r.group_total == 100 || seen.contains(&r.entry) {
                        continue;
                    }
                    seen.push(r.entry);
                    warnings.push(format!(
                        "{}: グループ{} の合計が {}% です（100% に届いていません・自動補正はしません）",
                        r.file,
                        r.group + 1,
                        r.group_total
                    ));
                }
                let cur = ed.quest_edit_cursor.min(rows.len().saturating_sub(1));
                let heading = ed.quest_item_heading();
                self.draw_adaptive_rows_warn(
                    c,
                    ed,
                    &heading,
                    &sub,
                    &rows,
                    cur,
                    true,
                    "アイテム選択",
                    &[],
                    &warnings,
                );
            }
            _ => {
                let supplies = ed.quest_supplies();
                let rows: Vec<(usize, String, String, String, String)> = supplies
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        (
                            i,
                            r.label.clone(),
                            format!("{}  {} 行目（確率なし）", r.file, r.row + 1),
                            format!("×{}", r.count),
                            quest_row_badge(i),
                        )
                    })
                    .collect();
                let cur = ed.quest_edit_cursor.min(rows.len().saturating_sub(1));
                let heading = ed.quest_item_heading();
                self.draw_adaptive_rows(c, ed, &heading, &sub, &rows, cur, true, "アイテム選択");
            }
        }
    }

    fn draw_arc_list(&mut self, c: &mut Canvas, ed: &Editor) {
        let mut rows = Vec::with_capacity(ed.arc_files.len());
        for (i, path) in ed.arc_files.iter().enumerate() {
            let base = path.rsplit('/').next().unwrap_or(path.as_str());
            let qid_opt = base
                .strip_prefix('q')
                .and_then(|s| s.strip_suffix(".arc"))
                .and_then(|s| s.parse::<i32>().ok());
            let title = match qid_opt.and_then(mhnx_core::dlc_bundle::official_quest_title) {
                Some(t) if !t.is_empty() => t.to_string(),
                _ => base.to_string(),
            };
            rows.push((
                i,
                title,
                path.clone(),
                base.to_string(),
                format!("#{:03}", i + 1),
            ));
        }
        self.draw_adaptive_rows(
            c,
            ed,
            "SDカード .arc 一覧",
            "",
            &rows,
            ed.cursor.min(rows.len().saturating_sub(1)),
            false,
            "開く",
        );
    }

    fn draw_arc_entries(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(arc) = &ed.arc else {
            return;
        };
        let mut rows = Vec::with_capacity(arc.entries.len());
        for (i, e) in arc.entries.iter().enumerate() {
            let ext = extension_for(e.type_hash).unwrap_or("bin");
            let (desc, val_txt) = match ext {
                "mib" => (String::from("クエスト条件 (.mib)"), format!("{} B", e.data.len())),
                "gmd" => (String::from("テキスト (.gmd)"), format!("{} B", e.data.len())),
                "rem" => (String::from("報酬テーブル (.rem)"), format!("{} B", e.data.len())),
                "sup" => (String::from("支給品テーブル (.sup)"), format!("{} B", e.data.len())),
                "sem" => {
                    if e.data.len() >= 0x20 {
                        let area = u32::from_le_bytes([e.data[0x0C], e.data[0x0D], e.data[0x0E], e.data[0x0F]]);
                        let x = f32::from_le_bytes([e.data[0x10], e.data[0x11], e.data[0x12], e.data[0x13]]);
                        let y = f32::from_le_bytes([e.data[0x14], e.data[0x15], e.data[0x16], e.data[0x17]]);
                        let z = f32::from_le_bytes([e.data[0x18], e.data[0x19], e.data[0x1A], e.data[0x1B]]);
                        (
                            format!("大型初期配置 (.sem) — ({:.0}, {:.0}, {:.0})", x, y, z),
                            if area == 0 {
                                String::from("未使用(0)")
                            } else {
                                format!("エリア {}", area)
                            },
                        )
                    } else {
                        (String::from("大型初期配置 (.sem)"), format!("{} B", e.data.len()))
                    }
                }
                "esl" => (String::from("小型配置 (.esl)"), format!("{} B", e.data.len())),
                _ => (String::from("データ"), format!("{} B", e.data.len())),
            };
            rows.push((
                i,
                format!("{}.{}", e.name, ext),
                desc,
                val_txt,
                ext.to_uppercase(),
            ));
        }
        self.draw_adaptive_rows(
            c,
            ed,
            "ARC内ファイル構成",
            "",
            &rows,
            ed.cursor.min(rows.len().saturating_sub(1)),
            false,
            "開く",
        );
    }

    fn draw_mib(&mut self, c: &mut Canvas, ed: &Editor) {
        let mib = ed.mib_bytes();
        let total = questfields::MIB_FIELDS.len();
        let mut rows = Vec::with_capacity(total);

        for (i, f) in questfields::MIB_FIELDS.iter().enumerate() {
            let raw_val = if f.key.starts_with("mob_slot[") {
                if f.offset + 2 <= mib.len() {
                    u16::from_le_bytes([mib[f.offset], mib[f.offset + 1]]) as u32
                } else {
                    0
                }
            } else {
                questfields::read_u32(mib, f.offset, f.kind).unwrap_or(0)
            };

            let (meta, val_str) = if f.key.starts_with("mob_slot[") {
                if f.offset + 13 <= mib.len() {
                    let mid = u16::from_le_bytes([mib[f.offset], mib[f.offset + 1]]) as u32;
                    let tbl = mib[f.offset + 3];
                    let cnt = mib[f.offset + 4];
                    let hp = mib[f.offset + 5];
                    let atk = mib[f.offset + 6];
                    let def = mib[f.offset + 7];
                    let sta = mib[f.offset + 8];
                    let sz = u16::from_le_bytes([mib[f.offset + 9], mib[f.offset + 10]]);
                    let var = mib[f.offset + 11];
                    let sub_stat = match self.mib_mob_sub {
                        0 => format!("HP:{}", hp),
                        1 => format!("攻:{}", atk),
                        2 => format!("防:{}", def),
                        3 => format!("{}%", sz),
                        4 => format!("数:{}", cnt),
                        5 => format!("疲:{}", sta),
                        6 => format!("変:{}", var),
                        _ => format!("特:{}", tbl),
                    };
                    let m_meta = format!(
                        "HP:{} 攻:{} 防:{} 寸:{}% 数:{} 疲:{} 変:{} 特:{}",
                        hp, atk, def, sz, cnt, sta, var, tbl
                    );
                    let m_val = if mid == 0 {
                        format!("なし [{}]", sub_stat)
                    } else {
                        format!("{} [{}]", monster_name_for_mint(mid), sub_stat)
                    };
                    (m_meta, m_val)
                } else {
                    (
                        format!("offset +0x{:03X} ({}B)", f.offset, f.size),
                        format!("{}", raw_val),
                    )
                }
            } else if let Some(ename) = f.enum_name {
                let label = questfields::enum_table(ename)
                    .and_then(|t| t.get(raw_val as usize).copied())
                    .unwrap_or("?");
                (
                    format!("offset +0x{:03X} ({}B)", f.offset, f.size),
                    format!("{} ({})", label, raw_val),
                )
            } else {
                // A com* row says which 種類 byte gave the number its meaning,
                // so a 納品 row can never be read as a monster.
                let meta = match mhnx_core::hbedit::quest_target_info(mib, f.key) {
                    Some(info) => format!(
                        "+0x{:03X}  {}={}",
                        f.offset, info.kind_key, info.kind_label
                    ),
                    None => format!("offset +0x{:03X} ({}B)", f.offset, f.size),
                };
                (meta, mib_extra_value_label(mib, f.key, raw_val))
            };

            // The row is headed with what the id names: 対象モンスター /
            // 対象アイテム / 対象ID（未確定）. One word, from the 種類 byte.
            let title = match mhnx_core::hbedit::quest_target_info(mib, f.key) {
                Some(info) => info.row_title(f.ja),
                // 部位破壊: these bytes are the part, not a count, and the
                // row must not call them one.
                None => match mhnx_core::hbedit::quest_part_field(mib, f.key) {
                    Some(pf) if pf.is_part_break => f.ja.replace("数", "部位"),
                    _ => f.ja.to_string(),
                },
            };
            rows.push((i, title, meta, val_str, format!("#{:02}", i + 1)));
        }

        let cur = ed.mib_cursor.min(total.saturating_sub(1));
        let a_lbl = match mhnx_core::hbedit::mib_pick_kind(mib, cur) {
            mhnx_core::hbedit::MibPickKind::Item => "アイテム選択",
            mhnx_core::hbedit::MibPickKind::Part => "部位選択",
            _ if mib_has_picker(mib, cur) => "一覧選択",
            _ => "変更",
        };
        // A 大連続 names no monster in its 種類 row: the five mob_slot rows are
        // the target list, and the screen says so instead of leaving the 0
        // in the id column to be read as "no monster".
        let sub = ed.quest_mob_slot_note().unwrap_or_default();

        self.draw_adaptive_rows(
            c,
            ed,
            "クエスト条件 (.mib)",
            &sub,
            &rows,
            cur,
            true,
            a_lbl,
        );
    }

    fn draw_gmd(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(gmd) = &ed.gmd else {
            return;
        };
        let mut rows = Vec::with_capacity(gmd.strings.len());
        for (i, s) in gmd.strings.iter().enumerate() {
            let role = mhnx_core::quest::Gmd::LABELS
                .get(i)
                .copied()
                .unwrap_or("テキスト");
            let clean = s.replace("\r\n", "\n");
            let one_line = if clean.trim().is_empty() {
                String::from("(空)")
            } else {
                clean.replace('\n', " / ")
            };
            rows.push((
                i,
                format!("[{}]  {}", role, one_line),
                role.to_string(),
                String::new(),
                format!("#{:02}", i + 1),
            ));
        }
        self.draw_adaptive_rows(
            c,
            ed,
            "クエスト文章 (.gmd)",
            "",
            &rows,
            ed.gmd_cursor.min(rows.len().saturating_sub(1)),
            false,
            "テキスト編集",
        );
    }

    fn draw_entry_view(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(entry) = ed.arc.as_ref().and_then(|a| a.entries.get(ed.entry)) else {
            return;
        };
        let ext = extension_for(entry.type_hash).unwrap_or("bin");
        let mut rows = Vec::new();

        match ext {
            "rem" | "sup" => {
                if let Ok(tbl) = ItemTable::parse(&entry.data) {
                    // Which roll group a row belongs to: each group is rolled
                    // on its own, so the screen names it beside the row.
                    let groups = tbl.groups();
                    let mut group_of = alloc::vec![0usize; tbl.records.len()];
                    for (gi, g) in groups.iter().enumerate() {
                        for k in g.start..(g.start + g.len).min(group_of.len()) {
                            group_of[k] = gi;
                        }
                    }
                    for (i, rec) in tbl.records.iter().enumerate() {
                        let iname = if rec.item == 0 {
                            String::from("(なし)")
                        } else {
                            item_name_of(rec.item)
                        };
                        let sub = if ext == "rem" {
                            format!(
                                "枠{:02} / G{} / ID:{}",
                                i + 1,
                                group_of.get(i).copied().unwrap_or(0) + 1,
                                rec.item
                            )
                        } else {
                            format!("枠{:02} / 表示スロット {} / ID:{}", i + 1, rec.slot, rec.item)
                        };
                        // Both numbers the row holds are on screen, so the one
                        // [-] is currently on cannot be mistaken for the other.
                        let val = if ext == "rem" {
                            format!("×{}  {}%", rec.count, rec.slot)
                        } else {
                            format!("×{}", rec.count)
                        };
                        rows.push((i, iname, sub, val, format!("#{:03}", i + 1)));
                    }
                }
            }
            "sem" => {
                let sems = ed.sem_entry_indices();
                if let Some(arc) = &ed.arc {
                    for (k, &e_idx) in sems.iter().enumerate() {
                        let Some(se) = arc.entries.get(e_idx) else {
                            continue;
                        };
                        if se.data.len() < 0x20 {
                            continue;
                        }
                        let wave = u32::from_le_bytes([se.data[0x08], se.data[0x09], se.data[0x0A], se.data[0x0B]]);
                        let area = u32::from_le_bytes([se.data[0x0C], se.data[0x0D], se.data[0x0E], se.data[0x0F]]);
                        let x = f32::from_le_bytes([se.data[0x10], se.data[0x11], se.data[0x12], se.data[0x13]]);
                        let y = f32::from_le_bytes([se.data[0x14], se.data[0x15], se.data[0x16], se.data[0x17]]);
                        let z = f32::from_le_bytes([se.data[0x18], se.data[0x19], se.data[0x1A], se.data[0x1B]]);
                        let w = f32::from_le_bytes([se.data[0x1C], se.data[0x1D], se.data[0x1E], se.data[0x1F]]);

                        let base_row = k * 6;
                        let tag = format!("S{}", k + 1);
                        let meta_str = format!("{}.sem  (エリア:{} / X:{:.0} Y:{:.0} Z:{:.0})", se.name, area, x, y, z);
                        let area_val = if area == 0 {
                            String::from("0 (未使用)")
                        } else {
                            format!("エリア {}", area)
                        };

                        rows.push((
                            base_row,
                            format!("配置{} ({}) — 初期エリア番号", k + 1, se.name),
                            meta_str.clone(),
                            area_val,
                            tag.clone(),
                        ));
                        rows.push((
                            base_row + 1,
                            format!("配置{} ({}) — 初期配置 X座標", k + 1, se.name),
                            meta_str.clone(),
                            format!("{:.1}", x),
                            tag.clone(),
                        ));
                        rows.push((
                            base_row + 2,
                            format!("配置{} ({}) — 初期配置 Y座標(高さ)", k + 1, se.name),
                            meta_str.clone(),
                            format!("{:.1}", y),
                            tag.clone(),
                        ));
                        rows.push((
                            base_row + 3,
                            format!("配置{} ({}) — 初期配置 Z座標", k + 1, se.name),
                            meta_str.clone(),
                            format!("{:.1}", z),
                            tag.clone(),
                        ));
                        rows.push((
                            base_row + 4,
                            format!("配置{} ({}) — 向き角度(W)", k + 1, se.name),
                            meta_str.clone(),
                            format!("{:.1}°", w),
                            tag.clone(),
                        ));
                        rows.push((
                            base_row + 5,
                            format!("配置{} ({}) — 出現ウェーブ/条件", k + 1, se.name),
                            meta_str,
                            format!("Wave {}", wave),
                            tag,
                        ));
                    }
                }
            }
            "esl" => {
                if let Ok(esl) = Esl::parse(&entry.data) {
                    rows.push((
                        0,
                        String::from("小型Wave数"),
                        format!("{} bytes", esl.raw.len()),
                        format!("{} Waves", esl.waves),
                        String::from("ESL"),
                    ));
                }
            }
            "qdp" => {
                if let Ok(qdp) = Qdp::parse(&entry.data) {
                    for (i, &b) in qdp.flags().iter().enumerate() {
                        rows.push((
                            i,
                            format!("Flag +0x{:02X}", i + 8),
                            String::from("QDP"),
                            format!("0x{:02X}", b),
                            format!("F{:02}", i),
                        ));
                    }
                }
            }
            "qdl" | "qlv" => {
                if let Ok(q) = QuestLink::parse(&entry.data) {
                    for (i, r) in q.refs.iter().enumerate() {
                        rows.push((
                            i,
                            r.name.clone(),
                            format!("0x{:08X}", r.type_hash),
                            String::from("LINK"),
                            format!("L{:02}", i + 1),
                        ));
                    }
                }
            }
            _ => {}
        }

        let hdr = match ext {
            "rem" => "報酬テーブル (.rem)",
            "sup" => "支給品テーブル (.sup)",
            "sem" => "大型初期配置座標・エリア (.sem)",
            "esl" => "小型配置 (.esl)",
            _ => "エントリ詳細",
        };
        // A reward table says what its chances add up to and complains when a
        // group is not 100%; a supply box says it has no chance column at all.
        let is_table = matches!(ext, "rem" | "sup");
        let sub = if is_table {
            ed.entry_reward_sub()
        } else {
            String::new()
        };
        let warnings = if is_table {
            ed.entry_reward_warnings()
        } else {
            Vec::new()
        };
        self.draw_adaptive_rows_warn(
            c,
            ed,
            hdr,
            &sub,
            &rows,
            ed.entry_row_cursor.min(rows.len().saturating_sub(1)),
            matches!(ext, "rem" | "sup" | "sem" | "esl" | "qdp"),
            // The reward row's [A] is the cell's own action (hbedit::EntryAction),
            // so the button is labelled with the word the footer prints.
            if is_table { ed.entry_a_label() } else { "変更" },
            &[],
            &warnings,
        );
    }

    fn draw_check(&mut self, c: &mut Canvas, ed: &Editor) {
        let mut rows = Vec::with_capacity(ed.findings.len());
        for (i, f) in ed.findings.iter().enumerate() {
            let sev = match f.severity {
                Severity::Error => "ERR",
                Severity::Warn => "WARN",
                Severity::Info => "OK",
            };
            let act = if f.fixable { "修正可" } else { "-" };
            rows.push((
                i,
                f.message.clone(),
                String::from("整合性検査"),
                act.to_string(),
                sev.to_string(),
            ));
        }
        self.draw_adaptive_rows(
            c,
            ed,
            "クエスト整合性検査",
            "",
            &rows,
            ed.check_cursor.min(rows.len().saturating_sub(1)),
            false,
            "自動修正",
        );
    }

    // -----------------------------------------------------------------------
    // Modal Overlays: MIB Picker & Built-in OSK
    // -----------------------------------------------------------------------
    fn draw_mib_picker(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(picker) = self.mib_picker.clone() else {
            return;
        };
        let t = theme();
        c.rect(0, 0, 1280, 720, rgba(0, 0, 0, 165));

        let mx = 160;
        let my = 48;
        let mw = 960;
        let mh = 624;
        c.round_rect(mx, my, mw, mh, 12, t.bg_panel);
        c.round_rect_border(mx, my, mw, mh, 12, 2, t.border_focus);

        c.text_clipped(mx + 24, my + 16, mw - 380, &picker.title, t.text_primary, FontSize::Medium);
        c.text_right(
            mx + mw - 24,
            my + 20,
            "[▲/▼] 選択  [L/R] ±10  [A] 決定  [B] 閉じる",
            t.text_secondary,
            FontSize::Small,
        );
        c.hline(mx + 16, my + 52, mw - 32, t.border_subtle);

        let total = picker.items.len();
        let visible = MIB_PICKER_VISIBLE;
        let start = window_start(picker.cursor, total, visible);
        let row_h = 46;
        let list_x = mx + 24;
        let list_w = mw - 48;
        let list_y0 = my + 64;
        // The bar's 40px touch area, the same rule the lists use: the row's tap
        // area and the 決定 button stop before it, so a tap that aimed at the
        // button cannot grab the thumb instead.
        let track = scroll_track(list_x, list_w);

        for row in 0..visible {
            let idx = start + row;
            let Some((_, label)) = picker.items.get(idx) else {
                break;
            };
            let ry = list_y0 + (row as i32) * (row_h + 4);
            let selected = idx == picker.cursor;

            let bg = if selected {
                t.bg_selected
            } else if row % 2 == 0 {
                t.bg_row_even
            } else {
                t.bg_row_odd
            };
            c.round_rect(list_x, ry, list_w, row_h, 6, bg);
            if selected {
                c.round_rect_border(list_x, ry, list_w, row_h, 6, 2, t.border_focus);
                c.round_rect(list_x + 3, ry + 5, 4, row_h - 10, 2, t.accent_primary);
            }

            // The button stops 6px short of the bar's touch area: those 40px
            // belong to the scrollbar, and a tap that aimed at 決定 must not
            // grab the thumb instead. The label beside it is clipped to what is
            // left, so it cannot run under the button either.
            let bw = 94;
            let bx = track.hit_x - 6 - bw;
            let label_w = (bx - 12 - (list_x + 18)).max(80);
            c.text_clipped(
                list_x + 18,
                ry + 11,
                label_w,
                label,
                if selected { t.accent_primary } else { t.text_primary },
                FontSize::Medium,
            );
            if selected {
                c.round_rect(bx, ry + 6, bw, 34, 5, t.accent_primary);
                btn_text(
                    c,
                    bx,
                    ry + 6,
                    bw,
                    34,
                    "[A] 決定",
                    white_or_dark(t.accent_primary),
                    FontSize::Small,
                );
            }
            self.add_hit(
                list_x,
                ry,
                track.row_right - list_x,
                row_h,
                UiAction::MibPickerSelect(idx),
            );
        }

        // The picker's own bar. The monster table runs to sixty entries, and a
        // modal that leaves the reader no way to see where in it the cursor is
        // has the same complaint the lists had. Its rows are 50px apart, so the
        // track is measured from the row pitch rather than from a grown thumb.
        let ids: Vec<usize> = (0..total).collect();
        self.draw_list_bar(
            c,
            ed,
            list_x,
            list_w,
            list_y0,
            (visible as i32 - 1) * MIB_PICKER_PITCH + row_h,
            ScrollBar::new(total, visible, start),
            &ids,
            true,
        );

        let foot_y = my + mh - 50;
        c.text_sized(
            mx + 24,
            foot_y + 10,
            &format!("{} / {} 件", picker.cursor + 1, total),
            t.text_secondary,
            FontSize::Small,
        );
        let close_x = mx + mw - 180;
        c.round_rect(close_x, foot_y, 156, 38, 6, t.bg_btn);
        c.round_rect_border(close_x, foot_y, 156, 38, 6, 1, t.border_subtle);
        btn_text(c, close_x, foot_y, 156, 38, "[B] 閉じる", t.text_primary, FontSize::Small);
        self.add_hit(close_x, foot_y, 156, 38, UiAction::MibPickerClose);
    }

    fn draw_osk(&mut self, c: &mut Canvas) {
        let Some(osk) = self.osk.clone() else { return };
        let t = theme();

        c.rect(0, 0, 1280, 720, rgba(0, 0, 0, 165));
        let mx = 100;
        let my = 44;
        let mw = 1080;
        let mh = 632;
        c.round_rect(mx, my, mw, mh, 12, t.bg_panel);
        c.round_rect_border(mx, my, mw, mh, 12, 2, t.border_focus);

        // The title carries the one refusal the modal has to explain: a
        // character the field cannot hold was dropped, never typed.
        let mut title = format!("文字入力: {}", osk.header);
        if let Some(c) = osk.refused {
            title.push_str(&format!(" / その文字は使えません（U+{:04X}）", c as u32));
        }
        c.text_clipped(mx + 24, my + 16, 640, &title, t.text_primary, FontSize::Medium);
        // The fallback note sits under the title: it only exists when the
        // system keyboard could not be launched, and it is the answer to "why
        // am I looking at this instead of the console's own keyboard?".
        if !osk.note.is_empty() {
            c.text_clipped(
                mx + 24,
                my + 38,
                760,
                &osk.note,
                t.accent_warn,
                FontSize::Small,
            );
        }
        // The key row, one table for the buttons at the bottom and the legend
        // here. The legend's word for [B] was the short one while the button's
        // was 1字削除, it called the clear key 全消去 where the button said
        // クリア, and it
        // named neither [Y] nor [-] while the row carried both: the two were
        // written twice and had drifted. The legend is this table rendered, so a
        // key added here is on both surfaces.
        let newline_bg = if osk.spec.multiline {
            t.bg_btn_primary
        } else {
            t.bg_btn
        };
        let actions: [(&str, &str, i32, Color, Color, UiAction); 7] = [
            ("B", "1字削除", 128, t.bg_btn, t.border_subtle, UiAction::OskBackspace),
            ("X", "クリア（2回）", 150, t.bg_danger, t.accent_danger, UiAction::OskClear),
            ("Y", "濁点/小字", 148, t.bg_btn_primary, t.accent_primary, UiAction::OskModify),
            (
                if osk.spec.multiline { "ZR" } else { "" },
                if osk.spec.multiline { "改行" } else { "改行不可" },
                124,
                newline_bg,
                t.accent_primary,
                UiAction::OskNewline,
            ),
            ("", "空白", 84, t.bg_btn, t.border_subtle, UiAction::OskSpace),
            ("-", "取消", 128, t.bg_btn, t.border_subtle, UiAction::OskCancel),
            ("＋", "確定", 180, t.accent_ok, t.accent_ok, UiAction::OskConfirm),
        ];
        // The two keys that are not buttons: [A] presses the selected cell and
        // the shoulders walk the four character tabs.
        let mut legend = String::from("[A] 入力  [L/R] 文字種");
        for &(key, word, ..) in actions.iter() {
            if key.is_empty() {
                continue;
            }
            legend.push_str("  ");
            legend.push_str(&osk_key_label(key, word));
        }
        c.text_right(mx + mw - 24, my + 20, &legend, t.text_secondary, FontSize::Small);

        let ib_x = mx + 24;
        let ib_y = my + 56;
        let ib_w = mw - 48;
        let ib_h = 50;
        c.round_rect(ib_x, ib_y, ib_w, ib_h, 6, t.bg_val_box);
        c.round_rect_border(ib_x, ib_y, ib_w, ib_h, 6, 1, t.border_focus);
        // Every line of the text is drawn - an empty one included, because the
        // game draws it and the cursor sits on it. The box shows the last two,
        // which is where the cursor is when a name gets long; an ellipsis says
        // that something is above.
        let lines = mhnx_core::save::name_lines(&osk.buffer);
        let shown = lines.len().saturating_sub(2);
        for (li, line) in lines.iter().skip(shown).enumerate() {
            let mut text = String::from(*line);
            if li == 0 && shown > 0 {
                text.insert(0, '…');
            }
            if shown + li + 1 == lines.len() {
                text.push('|');
            }
            c.text_clipped(
                ib_x + 12,
                ib_y + 5 + (li as i32) * 22,
                ib_w - 200,
                &text,
                t.accent_primary,
                FontSize::Small,
            );
        }

        // Bytes and lines, the same figure the status row prints:
        // 「13/31 バイト（2行）」.
        let cnt_str = osk.spec.length_label(&osk.buffer);
        c.text_right(ib_x + ib_w - 16, ib_y + 16, &cnt_str, t.text_secondary, FontSize::Small);

        // Keyboard Mode Tabs (L / R or Touch)
        let tab_y = my + 118;
        let tab_w = 246;
        let tab_h = 38;
        for (ti, &tname) in OSK_TAB_NAMES.iter().enumerate() {
            let tx = ib_x + (ti as i32) * (tab_w + 16);
            let active = osk.tab == ti;
            c.round_rect(tx, tab_y, tab_w, tab_h, 6, if active { t.bg_selected } else { t.bg_row_even });
            c.round_rect_border(
                tx,
                tab_y,
                tab_w,
                tab_h,
                6,
                if active { 2 } else { 1 },
                if active { t.border_focus } else { t.border_subtle },
            );
            btn_text(
                c,
                tx,
                tab_y,
                tab_w,
                tab_h,
                tname,
                if active { t.accent_primary } else { t.text_secondary },
                FontSize::Small,
            );
            self.add_hit(tx, tab_y, tab_w, tab_h, UiAction::OskTab(ti));
        }

        // Character Grid (10 columns x 5 rows)
        let grid = osk_grid_for(osk.tab);
        let gx0 = ib_x;
        let gy0 = my + 170;
        let cell_w = 96;
        let cell_h = 68;
        let gap_x = 8;
        let gap_y = 8;

        for r in 0..5 {
            for col in 0..10 {
                let cx = gx0 + (col as i32) * (cell_w + gap_x);
                let cy = gy0 + (r as i32) * (cell_h + gap_y);
                let sel = osk.row == r && osk.col == col;
                let label = grid[r][col];
                let is_mod = matches!(label, "゛濁" | "゜半" | "小ぁ" | "小ァ" | "a↔A");
                let bg = if sel {
                    t.bg_selected
                } else if is_mod {
                    t.bg_btn_primary
                } else {
                    t.bg_row_even
                };
                let border = if sel {
                    t.border_focus
                } else if is_mod {
                    t.accent_primary
                } else {
                    t.border_subtle
                };
                c.round_rect(cx, cy, cell_w, cell_h, 6, bg);
                c.round_rect_border(cx, cy, cell_w, cell_h, 6, if sel { 2 } else { 1 }, border);
                btn_text(
                    c,
                    cx,
                    cy,
                    cell_w,
                    cell_h,
                    label,
                    if sel { t.accent_primary } else { t.text_primary },
                    if osk.tab == 3 { FontSize::Small } else { FontSize::Medium },
                );
                self.add_hit(cx, cy, cell_w, cell_h, UiAction::OskCell(r, col));
            }
        }

        // Bottom Action Buttons Row
        let by = my + 560;
        let bh = 48;
        // The same table the legend above is built from: a button prints the
        // key and the word the legend prints, so the two cannot describe one
        // press two ways.
        let mut bx = ib_x;
        for &(key, word, bw, bg, border, act) in actions.iter() {
            let label = osk_key_label(key, word);
            c.round_rect(bx, by, bw, bh, 6, bg);
            c.round_rect_border(bx, by, bw, bh, 6, 1, border);
            btn_text(c, bx, by, bw, bh, &label, white_or_dark(bg), FontSize::Small);
            self.add_hit(bx, by, bw, bh, act);
            bx += bw + 12;
        }
    }
}

// ---------------------------------------------------------------------------
// Pure Helper Functions (Pagination, MIB Choices, OSK Character Tables)
// ---------------------------------------------------------------------------

/// Draw the lines of a name into a box, one glyph line per line break.
///
/// The game draws every line a name holds, an empty one included, so this does
/// too: a name that ends with a break keeps its empty last line. `max_lines`
/// bounds the box; when there are more, the last drawn line gets an ellipsis so
/// the truncation is visible rather than silent. Returns the height used.
fn draw_name_box(
    c: &mut Canvas,
    x: i32,
    y: i32,
    w: i32,
    text: &str,
    max_lines: usize,
    line_h: i32,
    color: Color,
    size: FontSize,
) -> i32 {
    let lines = mhnx_core::save::name_lines(text);
    let shown = lines.len().min(max_lines.max(1));
    for (i, line) in lines.iter().take(shown).enumerate() {
        let mut drawn = String::from(*line);
        if i + 1 == shown && lines.len() > shown {
            drawn.push('…');
        }
        c.text_clipped(x, y + (i as i32) * line_h, w, &drawn, color, size);
    }
    (shown as i32) * line_h
}
/// The filter state a screen keeps in its header, when it is not already in the
/// title.
///
/// A narrowed list used to look exactly like a short table: twelve rows, and no
/// word anywhere about the other three hundred. The pickers and the equipment
/// box already carry their filter word in the title ("検索: ほげ（12 件）",
/// "装備BOX（検索: …）"), so they are not repeated here; the item box's
/// hide-empty toggle is the one state nothing else on the screen names.
///
/// One word describes a narrowed list on every screen: 検索. The verbs 絞る /
/// 絞り込み used to appear here, on the talisman pill and in two headings for
/// the same thing, which is the drift the review found.
/// The word a pill prints for the key that runs it: the table's, never a key
/// name. See the note above `GuiState::draw_tool_button`.
fn pill_word(ed: &Editor, key: &str, fallback: &str) -> String {
    ed.key_label_or(key, fallback)
}

/// One button of the software keyboard's key row, as the legend and the button
/// both print it.
///
/// A keyless entry ("空白") is a button and not a key: it prints its word and
/// the legend leaves it out, because the legend is a list of keys.
fn osk_key_label(key: &str, word: &str) -> String {
    if key.is_empty() {
        String::from(word)
    } else {
        format!("[{}] {}", key, word)
    }
}

fn filter_chip(ed: &Editor) -> Option<String> {
    match ed.screen {
        // The item box's hide-empty toggle. The name search has its own line:
        // the heading says 検索結果 and the subtitle carries "検索: …（n 枠）",
        // so it is not repeated here.
        Screen::Items => Some(String::from(if ed.show_empty {
            "空きも表示"
        } else {
            "空き非表示"
        })),
        // The quest filter narrows both the save's list and the two bundles,
        // and neither heading named it: a filtered list that says nothing is
        // the bug this chip exists for.
        Screen::SaveQuests => {
            let needle = ed.quest_filter.trim();
            if needle.is_empty() {
                None
            } else {
                Some(format!("検索: {}", needle))
            }
        }
        // The talisman list's name search. Its heading carries the needle too
        // (hbedit::Editor::charm_filter_heading), but the heading is the first
        // thing clipped on a narrow panel and this chip is not. "検索:" is the
        // same word the pickers and the item box print.
        Screen::Charms => {
            let needle = ed.charm_filter.trim();
            if needle.is_empty() {
                None
            } else {
                Some(format!("検索: {}", needle))
            }
        }
        _ => None,
    }
}

fn window_start(cursor: usize, total: usize, visible: usize) -> usize {
    if total <= visible {
        return 0;
    }
    let half = visible / 2;
    let mut start = cursor.saturating_sub(half);
    if start + visible > total {
        start = total - visible;
    }
    start
}

fn monster_name_for_mint(v: u32) -> &'static str {
    let mint = questfields::ENUM_MINT_;
    let mons = questfields::ENUM_MONS_;
    for (i, &id_str) in mint.iter().enumerate() {
        if id_str.parse::<u32>().ok() == Some(v) {
            return mons.get(i).copied().unwrap_or("?");
        }
    }
    "?"
}

fn mib_has_picker(mib: &[u8], field_idx: usize) -> bool {
    let Some(f) = questfields::MIB_FIELDS.get(field_idx) else {
        return false;
    };
    if f.enum_name.is_some() {
        return true;
    }
    // A com* row only has a list when its 種類 names one: the monster table
    // for 狩猟/討伐, the item picker for 納品, nothing for 大連続 and the rest.
    if mhnx_core::hbedit::mib_pick_kind(mib, field_idx) != mhnx_core::hbedit::MibPickKind::None {
        return true;
    }
    matches!(f.key, "map_id" | "quest_level" | "icn1" | "icn2" | "icn3" | "icn4" | "icn5")
}

/// The monster table as the .mib picker lists it: name first, hex id beside.
fn mib_monster_items() -> Vec<(u32, String)> {
    mhnx_core::hbedit::monster_id_choices()
        .into_iter()
        .map(|(id, name)| (id as u32, format!("{} 0x{:X}", name, id)))
        .collect()
}

fn mib_field_choices(mib: &[u8], field_idx: usize) -> Option<(String, Vec<(u32, String)>, u32)> {
    let f = questfields::MIB_FIELDS.get(field_idx)?;
    if f.key.starts_with("mob_slot[") {
        let cur = if f.offset + 2 <= mib.len() {
            u16::from_le_bytes([mib[f.offset], mib[f.offset + 1]]) as u32
        } else {
            0
        };
        return Some((format!("{} (モンスター選択)", f.ja), mib_monster_items(), cur));
    }

    // 部位破壊: the two bytes after the id are a part code, and the shipped
    // quests name the parts of 34 monsters. Offer exactly those; a monster
    // nobody measured has no list and the row stays a number.
    if let Some(pf) = mhnx_core::hbedit::quest_part_field(mib, f.key) {
        if pf.is_part_break {
            let items: Vec<(u32, String)> = mhnx_core::hbedit::quest_part_choices(pf.monster)
                .into_iter()
                .map(|p| (p.code as u32, format!("{} ({})", p.label, p.code)))
                .collect();
            if items.is_empty() {
                return None;
            }
            return Some((
                format!("{} の部位（{}）", monster_name_for_mint(pf.monster as u32), f.ja),
                items,
                pf.code as u32,
            ));
        }
    }

    let cur = questfields::read_u32(mib, f.offset, f.kind).unwrap_or(0);
    // A com* row is not "a monster id" by itself: the 種類 byte beside it says
    // whether the game reads the number as a monster or as an item. 納品 and
    // 採取 rows are item ids, and their list is the item picker's own screen
    // (Editor::open_mib_item_pick), so this function answers None for them.
    if let Some(info) = mhnx_core::hbedit::quest_target_info(mib, f.key) {
        if info.id_kind != mhnx_core::hbedit::QuestTargetId::Monster {
            return None;
        }
        return Some((
            format!("{}を選ぶ", info.row_title(f.ja)),
            mib_monster_items(),
            cur,
        ));
    }
    match f.key {
        "ko1" | "ko2" | "ko3" => {
            // The small monsters are a byte: only the ids it can hold.
            let mut items = mib_monster_items();
            if f.size == 1 {
                items.retain(|(id, _)| *id <= 255);
            }
            Some((format!("{} (モンスター選択)", f.ja), items, cur))
        }
        "map_id" => {
            let mut items = Vec::with_capacity(questfields::ENUM_MAP_.len());
            for (i, &name) in questfields::ENUM_MAP_.iter().enumerate() {
                let raw = if i < 28 { i as u32 } else { (i + 73) as u32 };
                items.push((raw, format!("{:03}: {}", raw, name)));
            }
            Some((String::from("マップ選択"), items, cur))
        }
        "quest_level" => {
            let mut items = Vec::with_capacity(questfields::ENUM_LEVEL_.len());
            for (i, &name) in questfields::ENUM_LEVEL_.iter().enumerate() {
                let raw = if i < 16 { i as u32 } else { (i + 85) as u32 };
                items.push((raw, format!("{:03}: {}", raw, name)));
            }
            Some((String::from("クエストLv選択"), items, cur))
        }
        "icn1" | "icn2" | "icn3" | "icn4" | "icn5" => {
            let mut items = Vec::with_capacity(questfields::ENUM_ICN_.len());
            for (i, &name) in questfields::ENUM_ICN_.iter().enumerate() {
                items.push((i as u32, format!("{:03}: {}", i, name)));
            }
            Some((format!("{} (アイコン選択)", f.ja), items, cur))
        }
        "ran2" | "ran3" | "ran4" => {
            let table = f.enum_name.and_then(questfields::enum_table)?;
            let mut items = Vec::with_capacity(table.len());
            for (i, &name) in table.iter().enumerate() {
                let raw = (i + 1) as u32;
                items.push((raw, format!("{}: {}", raw, name)));
            }
            Some((format!("{}", f.ja), items, cur))
        }
        _ => {
            let table = f.enum_name.and_then(questfields::enum_table)?;
            let mut items = Vec::with_capacity(table.len());
            for (i, &name) in table.iter().enumerate() {
                items.push((i as u32, format!("{:02}: {}", i, name)));
            }
            Some((format!("{}", f.ja), items, cur))
        }
    }
}

/// The value column of one .mib row that carries no enum table.
///
/// For a com* row the number is named in the space its 種類 byte names: a
/// 狩猟/討伐 row prints the monster, a 納品 row prints the item. Printing the
/// monster name and falling back to the item name (what this used to do) made
/// a delivery target read like a monster that happened to share the id.
fn mib_extra_value_label(mib: &[u8], key: &str, v: u32) -> String {
    // 部位破壊: the value is a part code and the name comes from the shipped
    // quests - 「尻尾 (7)」 - never from a guess.
    if let Some(pf) = mhnx_core::hbedit::quest_part_field(mib, key) {
        if pf.is_part_break {
            return pf.label;
        }
    }
    if let Some(info) = mhnx_core::hbedit::quest_target_info(mib, key) {
        return format!("{} ({})", info.label, info.kind_label);
    }
    match key {
        "ko1" | "ko2" | "ko3" => {
            if v == 0 {
                String::from("なし")
            } else {
                let mname = monster_name_for_mint(v);
                if mname != "?" {
                    format!("{} ({})", mname, v)
                } else {
                    format!("{} ({})", item_name_of(v as u16), v)
                }
            }
        }
        "map_id" => {
            let idx = if v > 100 { (v - 73) as usize } else { v as usize };
            let mname = questfields::ENUM_MAP_.get(idx).copied().unwrap_or("?");
            format!("{} ({})", mname, v)
        }
        "quest_level" => {
            let idx = if v > 100 { (v - 85) as usize } else { v as usize };
            let lname = questfields::ENUM_LEVEL_.get(idx).copied().unwrap_or("?");
            format!("{} ({})", lname, v)
        }
        "icn1" | "icn2" | "icn3" | "icn4" | "icn5" => {
            let iname = questfields::ENUM_ICN_.get(v as usize).copied().unwrap_or("?");
            format!("{} ({})", iname, v)
        }
        "time_limit" => format!("{} 分", v),
        "z1" | "z2" | "z3" => format!("{} z", v),
        _ => format!("{}", v),
    }
}

const OSK_TAB_NAMES: [&str; 4] = ["ひらがな", "カタカナ", "英数字", "単語・漢字"];

const OSK_GRID_HIRA: [[&str; 10]; 5] = [
    ["あ", "い", "う", "え", "お", "か", "き", "く", "け", "こ"],
    ["さ", "し", "す", "せ", "そ", "た", "ち", "つ", "て", "と"],
    ["な", "に", "ぬ", "ね", "の", "は", "ひ", "ふ", "へ", "ほ"],
    ["ま", "み", "む", "め", "も", "や", "ゆ", "よ", "ら", "り"],
    ["る", "れ", "ろ", "わ", "を", "ん", "ー", "゛濁", "゜半", "小ぁ"],
];

const OSK_GRID_KATA: [[&str; 10]; 5] = [
    ["ア", "イ", "ウ", "エ", "オ", "カ", "キ", "ク", "ケ", "コ"],
    ["サ", "シ", "ス", "セ", "ソ", "タ", "チ", "ツ", "テ", "ト"],
    ["ナ", "ニ", "ヌ", "ネ", "ノ", "ハ", "ヒ", "フ", "ヘ", "ホ"],
    ["マ", "ミ", "ム", "メ", "モ", "ヤ", "ユ", "ヨ", "ラ", "リ"],
    ["ル", "レ", "ロ", "ワ", "ヲ", "ン", "ー", "゛濁", "゜半", "小ァ"],
];

const OSK_GRID_ASCII: [[&str; 10]; 5] = [
    ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"],
    ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"],
    ["K", "L", "M", "N", "O", "P", "Q", "R", "S", "T"],
    ["U", "V", "W", "X", "Y", "Z", "_", "-", "!", "?"],
    ["a↔A", ".", "/", "+", "*", "(", ")", "[", "]", " "],
];

const OSK_GRID_WORDS: [[&str; 10]; 5] = [
    ["回復", "秘薬", "強走", "剛力", "護石", "天鱗", "宝玉", "紅玉", "逆鱗", "獰猛"],
    ["攻撃", "達人", "会心", "痛撃", "斬れ味", "匠", "業物", "聴覚", "風圧", "回避"],
    ["砲術", "装填", "貫通", "通常", "散弾", "速射", "溜め", "属性", "耐性", "ガード"],
    ["千里眼", "運気", "体力", "気絶", "捕獲", "剥ぎ取", "笛", "採取", "広域", "食事"],
    ["龍", "火", "水", "雷", "氷", "毒", "麻痺", "睡眠", "爆破", "G"],
];

fn osk_grid_for(tab: usize) -> &'static [[&'static str; 10]; 5] {
    match tab {
        0 => &OSK_GRID_HIRA,
        1 => &OSK_GRID_KATA,
        2 => &OSK_GRID_ASCII,
        _ => &OSK_GRID_WORDS,
    }
}

/// Put typed text into the keyboard buffer, refusing what the field cannot hold.
///
/// This is the only door into the buffer, which is why a tab or a NUL cannot be
/// entered at all: the field spec drops it before it is stored and remembers the
/// code point, so the modal can say which one it refused. Nothing is added past
/// the capacity the field stores.
fn osk_push(osk: &mut OskState, text: &str) {
    if let Some(c) = osk.spec.push_str(&mut osk.buffer, text) {
        osk.refused = Some(c);
    }
}

/// Insert the line break, when the field holds one (a name does).
fn osk_newline(osk: &mut OskState) {
    if !osk.spec.multiline {
        // A filter or a number has no second line; the footer says so and the
        // key does nothing rather than blaming a character.
        return;
    }
    osk_push(osk, "\n");
}

fn osk_press_cell(osk: &mut OskState, row: usize, col: usize) {
    let grid = osk_grid_for(osk.tab);
    let Some(&cell) = grid.get(row).and_then(|r| r.get(col)) else {
        return;
    };
    match cell {
        "゛濁" => osk_apply_dakuten(&mut osk.buffer),
        "゜半" => osk_apply_handakuten(&mut osk.buffer),
        "小ぁ" | "小ァ" | "a↔A" => osk_apply_small_or_case(&mut osk.buffer),
        text => osk_push(osk, text),
    }
}

fn osk_apply_dakuten(buf: &mut String) {
    let Some(ch) = buf.pop() else { return };
    let next = match ch {
        'か' => 'が', 'き' => 'ぎ', 'く' => 'ぐ', 'け' => 'げ', 'こ' => 'ご',
        'が' => 'か', 'ぎ' => 'き', 'ぐ' => 'く', 'げ' => 'け', 'ご' => 'こ',
        'さ' => 'ざ', 'し' => 'じ', 'す' => 'ず', 'せ' => 'ぜ', 'そ' => 'ぞ',
        'ざ' => 'さ', 'じ' => 'し', 'ず' => 'す', 'ぜ' => 'せ', 'ぞ' => 'そ',
        'た' => 'だ', 'ち' => 'ぢ', 'つ' | 'っ' => 'づ', 'て' => 'で', 'と' => 'ど',
        'だ' => 'た', 'ぢ' => 'ち', 'づ' => 'つ', 'で' => 'て', 'ど' => 'と',
        'は' | 'ぱ' => 'ば', 'ひ' | 'ぴ' => 'び', 'ふ' | 'ぷ' => 'ぶ', 'へ' | 'ぺ' => 'べ', 'ほ' | 'ぽ' => 'ぼ',
        'ば' => 'は', 'び' => 'ひ', 'ぶ' => 'ふ', 'べ' => 'へ', 'ぼ' => 'ほ',
        'う' => 'ゔ', 'ゔ' => 'う',
        'カ' => 'ガ', 'キ' => 'ギ', 'ク' => 'グ', 'ケ' => 'ゲ', 'コ' => 'ゴ',
        'ガ' => 'カ', 'ギ' => 'キ', 'グ' => 'ク', 'ゲ' => 'ケ', 'ゴ' => 'コ',
        'サ' => 'ザ', 'シ' => 'ジ', 'ス' => 'ズ', 'セ' => 'ゼ', 'ソ' => 'ゾ',
        'ザ' => 'サ', 'ジ' => 'シ', 'ズ' => 'ス', 'ゼ' => 'セ', 'ゾ' => 'ソ',
        'タ' => 'ダ', 'チ' => 'ヂ', 'ツ' | 'ッ' => 'ヅ', 'テ' => 'デ', 'ト' => 'ド',
        'ダ' => 'タ', 'ヂ' => 'チ', 'ヅ' => 'ツ', 'デ' => 'テ', 'ド' => 'ト',
        'ハ' | 'パ' => 'バ', 'ヒ' | 'ピ' => 'ビ', 'フ' | 'プ' => 'ブ', 'ヘ' | 'ペ' => 'ベ', 'ホ' | 'ポ' => 'ボ',
        'バ' => 'ハ', 'ビ' => 'ヒ', 'ブ' => 'フ', 'ベ' => 'ヘ', 'ボ' => 'ホ',
        'ウ' => 'ヴ', 'ヴ' => 'ウ',
        other => other,
    };
    buf.push(next);
}

fn osk_apply_handakuten(buf: &mut String) {
    let Some(ch) = buf.pop() else { return };
    let next = match ch {
        'は' | 'ば' => 'ぱ', 'ひ' | 'び' => 'ぴ', 'ふ' | 'ぶ' => 'ぷ', 'へ' | 'べ' => 'ぺ', 'ほ' | 'ぼ' => 'ぽ',
        'ぱ' => 'は', 'ぴ' => 'ひ', 'ぷ' => 'ふ', 'ぺ' => 'へ', 'ぽ' => 'ほ',
        'ハ' | 'バ' => 'パ', 'ヒ' | 'ビ' => 'ピ', 'フ' | 'ブ' => 'プ', 'ヘ' | 'ベ' => 'ペ', 'ホ' | 'ボ' => 'ポ',
        'パ' => 'ハ', 'ピ' => 'ヒ', 'プ' => 'フ', 'ペ' => 'ヘ', 'ポ' => 'ホ',
        other => other,
    };
    buf.push(next);
}

fn osk_apply_small_or_case(buf: &mut String) {
    let Some(ch) = buf.pop() else { return };
    let next = match ch {
        'あ' => 'ぁ', 'い' => 'ぃ', 'う' => 'ぅ', 'え' => 'ぇ', 'お' => 'ぉ',
        'ぁ' => 'あ', 'ぃ' => 'い', 'ぅ' => 'う', 'ぇ' => 'え', 'ぉ' => 'お',
        'つ' => 'っ', 'っ' => 'つ',
        'や' => 'ゃ', 'ゆ' => 'ゅ', 'よ' => 'ょ', 'わ' => 'ゎ',
        'ゃ' => 'や', 'ゅ' => 'ゆ', 'ょ' => 'よ', 'ゎ' => 'わ',
        'ア' => 'ァ', 'イ' => 'ィ', 'ウ' => 'ゥ', 'エ' => 'ェ', 'オ' => 'ォ',
        'ァ' => 'ア', 'ィ' => 'イ', 'ゥ' => 'ウ', 'ェ' => 'エ', 'ォ' => 'オ',
        'ツ' => 'ッ', 'ッ' => 'ツ',
        'ヤ' => 'ャ', 'ユ' => 'ュ', 'ヨ' => 'ョ', 'ワ' => 'ヮ', 'カ' => 'ヵ', 'ケ' => 'ヶ',
        'ャ' => 'ヤ', 'ュ' => 'ユ', 'ョ' => 'ヨ', 'ヮ' => 'ワ', 'ヵ' => 'カ', 'ヶ' => 'ケ',
        c if c.is_ascii_uppercase() => c.to_ascii_lowercase(),
        c if c.is_ascii_lowercase() => c.to_ascii_uppercase(),
        other => other,
    };
    buf.push(next);
}

fn osk_cycle_last_char(buf: &mut String) {
    let Some(ch) = buf.chars().last() else { return };
    match ch {
        'は' | 'ひ' | 'ふ' | 'へ' | 'ほ' | 'ハ' | 'ヒ' | 'フ' | 'ヘ' | 'ホ' => {
            osk_apply_dakuten(buf);
        }
        'ば' | 'び' | 'ぶ' | 'べ' | 'ぼ' | 'バ' | 'ビ' | 'ブ' | 'ベ' | 'ボ' => {
            osk_apply_handakuten(buf);
        }
        'ぱ' | 'ぴ' | 'ぷ' | 'ぺ' | 'ぽ' | 'パ' | 'ピ' | 'プ' | 'ペ' | 'ポ' => {
            osk_apply_handakuten(buf);
        }
        'つ' | 'ツ' => osk_apply_small_or_case(buf),
        'っ' | 'ッ' => osk_apply_dakuten(buf),
        'づ' | 'ヅ' => osk_apply_dakuten(buf),
        'あ' | 'い' | 'え' | 'お' | 'や' | 'ゆ' | 'よ' | 'ぁ' | 'ぃ' | 'ぇ' | 'ぉ' | 'ゃ' | 'ゅ' | 'ょ'
        | 'ア' | 'イ' | 'エ' | 'オ' | 'ヤ' | 'ユ' | 'ヨ' | 'ァ' | 'ィ' | 'ェ' | 'ォ' | 'ャ' | 'ュ' | 'ョ' => {
            osk_apply_small_or_case(buf);
        }
        c if c.is_ascii_alphabetic() => osk_apply_small_or_case(buf),
        _ => osk_apply_dakuten(buf),
    }
}

// ---------------------------------------------------------------------------
// The update banner and its dialog
// ---------------------------------------------------------------------------
//
// Two shapes and no third one: a banner that fits in the guide strip's right
// end, and a dialog that is only ever opened on purpose. Nothing here knows how
// a release is read or where a file goes - crate::update owns all of that, and
// every state this code draws is a state that module produced.

impl GuiState {
    /// Draw the banner at the right end of the guide strip, and return the x the
    /// screen's own guide has to stop at.
    ///
    /// The pill is the tap target; the pad route to the same dialog is [A] on
    /// the start screen (and [R] held there at any time). Its width is capped so
    /// that a long version string cannot eat the guide line it sits beside.
    fn draw_update_banner(&mut self, c: &mut Canvas, right: i32, a_opens_it: bool) -> i32 {
        let t = theme();
        let text = if a_opens_it {
            format!("{}  [A] 詳細", self.update.banner)
        } else {
            format!("{}  タップで詳細", self.update.banner)
        };
        let w = (c.text_width_sized(&text, FontSize::Small) + 24).clamp(200, 640);
        let x = (right - w).max(MAIN_X);
        let h = GUIDE_H - 4;
        let y = GUIDE_Y + 2;
        c.round_rect(x, y, w, h, 5, t.bg_btn_primary);
        c.round_rect_border(x, y, w, h, 5, 1, t.border_focus);
        c.text_clipped(
            x + 10,
            GUIDE_Y + 2,
            w - 16,
            &text,
            white_or_dark(t.bg_btn_primary),
            FontSize::Small,
        );
        self.add_hit(x, GUIDE_Y, w, GUIDE_H, UiAction::UpdateOpen);
        x - 12
    }

    /// The details dialog.
    ///
    /// Three answers and no scrolling: [A] is whatever the current phase can do
    /// (確認 / ダウンロード / 中止 / もう一度), [B] puts the banner away until the
    /// next start, [Y] flips the start-up check. The body is the release notes
    /// or the reason there are none, and while a transfer runs it is a bar with
    /// the percentage the worker published.
    fn draw_update_dialog(&mut self, c: &mut Canvas) {
        use crate::update::Phase;
        let t = theme();
        c.rect(0, 0, 1280, 720, rgba(0, 0, 0, 185));

        let mx = 170;
        let my = 84;
        let mw = 940;
        let mh = 552;
        c.round_rect(mx, my, mw, mh, 12, t.bg_panel);
        c.round_rect_border(mx, my, mw, mh, 12, 2, t.border_focus);

        // Title: the version when there is one, the tool when there is not.
        let title = match &self.update.version {
            Some(v) => mhnx_core::update::dialog_title(v),
            None => String::from("更新の確認"),
        };
        c.text_clipped(mx + 26, my + 14, mw - 52, &title, t.text_primary, FontSize::Large);
        let state_word = match self.update.phase {
            Phase::Idle => "待機",
            Phase::Checking => "確認中",
            Phase::Available => "更新あり",
            Phase::Downloading => "ダウンロード中",
            Phase::Updated => "完了",
            Phase::UpdatedManual => "完了（手動で再起動）",
            Phase::Failed => "失敗",
        };
        c.text_right(mx + mw - 26, my + 20, state_word, t.accent_secondary, FontSize::Small);
        c.hline(mx + 20, my + 52, mw - 40, t.border_subtle);

        // The line under the rule: what happened, or what is happening.
        if !self.update.message.is_empty() {
            let color = match self.update.phase {
                Phase::Failed => t.accent_danger,
                Phase::Available | Phase::Updated | Phase::UpdatedManual => t.accent_ok,
                _ => t.text_secondary,
            };
            c.text_clipped(mx + 30, my + 62, mw - 60, &self.update.message, color, FontSize::Small);
        }

        // The body: a bar while a transfer runs, notes otherwise.
        let body_top = my + 92;
        let body_bottom = my + mh - 96;
        if self.update.phase == Phase::Downloading {
            let pct = self.update.progress.min(100);
            let bx = mx + 30;
            let bw = mw - 60;
            let by = body_top + 8;
            c.round_rect(bx, by, bw, 16, 7, t.bg_val_box);
            let filled = (bw as i64 * pct as i64 / 100) as i32;
            if filled > 0 {
                c.round_rect(bx, by, filled.max(6), 16, 7, t.accent_primary);
            }
            c.text_centered(bx, by + 22, bw, 24, &format!("{}%", pct), t.text_primary, FontSize::Medium);
            c.text_clipped(
                bx,
                by + 54,
                bw,
                "中止すると元のファイルはそのまま残ります（[A] 中止）",
                t.text_muted,
                FontSize::Small,
            );
        } else {
            let fallback: Vec<String> = if self.update.notes.is_empty() {
                let mut one = Vec::with_capacity(1);
                one.push(String::from(
                    "起動時に更新を確認します。オフにすると次の起動から通信しません。",
                ));
                one
            } else {
                Vec::new()
            };
            let lines: &[String] = if fallback.is_empty() {
                &self.update.notes
            } else {
                &fallback
            };
            let mut y = body_top;
            let row = 22;
            for line in lines {
                if y + row > body_bottom {
                    c.text_clipped(mx + 30, y, mw - 60, "…", t.text_muted, FontSize::Small);
                    break;
                }
                c.text_clipped(mx + 30, y, mw - 60, line, t.text_primary, FontSize::Small);
                y += row;
            }
        }

        // The three answers.
        let by = my + mh - 74;
        let primary_w = 280;
        let (bg, fg, bd) = match self.update.phase {
            Phase::Checking => (t.bg_btn, t.text_muted, t.border_subtle),
            Phase::Failed => (t.bg_danger, t.text_primary, t.text_primary),
            _ => (t.bg_btn_primary, white_or_dark(t.bg_btn_primary), t.border_focus),
        };
        c.round_rect(mx + 26, by, primary_w, 40, 6, bg);
        c.round_rect_border(mx + 26, by, primary_w, 40, 6, 1, bd);
        btn_text(
            c,
            mx + 26,
            by,
            primary_w,
            40,
            &format!("[A] {}", self.update.primary_label()),
            fg,
            FontSize::Medium,
        );
        self.add_hit(mx + 26, by, primary_w, 40, UiAction::UpdatePrimary);

        let later_w = 170;
        let later_x = mx + 26 + primary_w + 12;
        c.round_rect(later_x, by, later_w, 40, 6, t.bg_btn);
        c.round_rect_border(later_x, by, later_w, 40, 6, 1, t.border_subtle);
        btn_text(c, later_x, by, later_w, 40, "[B] あとで", t.text_secondary, FontSize::Medium);
        self.add_hit(later_x, by, later_w, 40, UiAction::UpdateLater);

        let check_w = 320;
        let check_x = mx + mw - 26 - check_w;
        c.round_rect(check_x, by, check_w, 40, 6, t.bg_btn);
        c.round_rect_border(check_x, by, check_w, 40, 6, 1, t.border_subtle);
        let check = format!(
            "[Y] 起動時の確認: {}",
            if self.update.settings.check_on_start { "オン" } else { "オフ" }
        );
        btn_text(c, check_x, by, check_w, 40, &check, t.text_secondary, FontSize::Small);
        self.add_hit(check_x, by, check_w, 40, UiAction::UpdateToggle);
    }
}

