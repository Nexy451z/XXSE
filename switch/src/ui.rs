//! Redesigned Switch UI/UX Engine for XXSE (`XXSE-1.nro` & `XXSE-2.nro`).
//!
//! Architecture & Ergonomics:
//!   - `ZL / ZR`: Cycle the 6 top-level Sections (`ファイル`, `ハンター`, `アイテム`, `護石・装備`, `オトモ・記録`, `クエスト`).
//!   - `L / R`: Cycle the Sub-Tabs of the current Section (NEVER switches character slot unexpectedly!).
//!   - `A`: Always Open / Edit / Choose from list (unified across every screen, never deletes!).
//!   - `◀ / ▶`: Always adjust numeric values (`±step`).
//!   - `X`: Always Save / Apply changes (`セーブ保存` / `.arc保存` / `お守り反映`).
//!   - `Y`: Secondary tool action (`刻み幅切替` / `検索` / `合法MAX` / `2段階確認削除`).
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
    item_name_of, monster_name, Editor, Key, Screen, CHAR_FIELDS, CONSOLE_SAVE, STEPS,
};
use mhnx_core::questfields;
use mhnx_core::save::{Options, SaveKind};

use crate::text::{rgb, rgba, Canvas, Color, FontSize};

const HUNT_STEPS: [i64; 5] = [1, 10, 100, 1000, -1];

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
    text_muted: rgb(106, 118, 136),
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
    text_muted: rgb(128, 116, 104),
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

// Layout geometry (1280 x 720) — Full-width 2-tier header + body + footer
const TOP_SEC_H: i32 = 48;
const SUB_TAB_Y: i32 = TOP_SEC_H;
const SUB_TAB_H: i32 = 44;
const CONTENT_Y: i32 = TOP_SEC_H + SUB_TAB_H + 10; // 102
const FOOTER_Y: i32 = 668;
const FOOTER_H: i32 = 52;
const CONTENT_H: i32 = FOOTER_Y - CONTENT_Y - 8; // 558
const MAIN_X: i32 = 20;
const MAIN_W: i32 = 1240;

// Split-Studio geometry for Variant 1 (`XXSE-1.nro`)
const V1_LIST_W: i32 = 796;
const V1_INSP_X: i32 = MAIN_X + V1_LIST_W + 16; // 832
const V1_INSP_W: i32 = 1280 - V1_INSP_X - 20; // 428

// ---------------------------------------------------------------------------
// 6 Top-Level Sections (ZL / ZR) & In-Section Sub-Tabs (L / R)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MainSection {
    File = 0,
    Hunter = 1,
    Items = 2,
    Gear = 3,
    PalicoRecord = 4,
    Quest = 5,
}

const MAIN_SECTIONS: [(MainSection, &str); 6] = [
    (MainSection::File, "ファイル"),
    (MainSection::Hunter, "ハンター"),
    (MainSection::Items, "アイテム"),
    (MainSection::Gear, "護石・装備"),
    (MainSection::PalicoRecord, "オトモ・記録"),
    (MainSection::Quest, "クエスト"),
];

pub fn section_of(screen: Screen) -> MainSection {
    match screen {
        Screen::SaveList | Screen::Convert | Screen::Options => MainSection::File,
        Screen::Slots | Screen::Char => MainSection::Hunter,
        Screen::Items | Screen::ItemPick => MainSection::Items,
        Screen::Charms | Screen::CharmEdit | Screen::SkillPick | Screen::Equip | Screen::Sets => {
            MainSection::Gear
        }
        Screen::Roster | Screen::Palico | Screen::Hunts | Screen::Guild => {
            MainSection::PalicoRecord
        }
        Screen::SaveQuests
        | Screen::QuestEquip
        | Screen::ArcList
        | Screen::ArcEntries
        | Screen::Mib
        | Screen::Gmd
        | Screen::EntryView
        | Screen::Check => MainSection::Quest,
    }
}

fn is_quest_inner(screen: Screen) -> bool {
    matches!(
        screen,
        Screen::ArcEntries | Screen::Mib | Screen::Gmd | Screen::EntryView | Screen::Check
    )
}

// ---------------------------------------------------------------------------
// Interactive HitBoxes & Actions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub enum UiAction {
    Key(Key),
    SelectSlot(usize),
    GotoSection(MainSection),
    SelectSubTab(usize),
    CycleSection(bool),
    CycleSubTab(bool),
    SetStep(usize),
    SelectRow(usize),
    ActivateRow(usize),
    AdjustRow(usize, Key),
    AdjustBy(i64),
    SetRowMax(usize),
    OpenItemPicker(usize),
    SelectSubField(usize, usize),
    OpenCharmSkill(usize),
    PageScroll(i32),
    Rescan,
    QuickSave,
    // One-tap Bulk Cheats & Presets
    CharMaxAll,
    ItemsMaxAll,
    CharmLegalMax,
    CharmPreset(u8),
    NewCharm,
    DeleteCharmConfirm,
    RosterMaxAll,
    InstallOfficialDlc,
    InstallDlcCategory(u8),
    SetQuestTab(usize),
    ToggleBundledQuest(i32),
    RemoveSaveQuest(i32),
    OpenBundledQuest(i32),
    OpenMibPicker(usize),
    MibPickerSelect(usize),
    MibPickerScroll(i32),
    MibPickerClose,
    MibPreset(u8),
    SemPreset(u8),
    InstallAllPalicoes,
    TogglePalicoLang,
    ExtractOfficialArcs,
    InstallArcRow(usize),
    // Built-in OSK
    OskTab(usize),
    OskCell(usize, usize),
    OskBackspace,
    OskClear,
    OskModify,
    OskSpace,
    OskConfirm,
    OskCancel,
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
    pub max_bytes: usize,
    pub tab: usize,
    pub row: usize,
    pub col: usize,
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
    pub quest_tab: usize,
    pub quest_cat_cursor: [usize; 4],
    pub mib_picker: Option<MibPickerState>,
    pub mib_mob_sub: usize,
    pub osk: Option<OskState>,
    pub osk_result: Option<(usize, String)>,
    pub osk_cancelled: bool,
    pub pending_delete: Option<i32>,
    pub pending_delete_charm: Option<usize>,
    pub sets_cursor: usize,
    pub sets_tab: usize,
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
            quest_tab: 0,
            quest_cat_cursor: [0; 4],
            mib_picker: None,
            mib_mob_sub: 0,
            osk: None,
            osk_result: None,
            osk_cancelled: false,
            pending_delete: None,
            pending_delete_charm: None,
            sets_cursor: 0,
            sets_tab: 0,
        }
    }

    pub fn open_osk(&mut self, req_idx: usize, header: &str, initial: &str, max_bytes: usize) {
        self.osk = Some(OskState {
            req_idx,
            header: String::from(header),
            buffer: String::from(initial),
            max_bytes: max_bytes.max(8),
            tab: 0,
            row: 0,
            col: 0,
        });
    }

    pub fn handle_osk_key(&mut self, key: Key) -> bool {
        let Some(mut osk) = self.osk.take() else {
            return false;
        };
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
            Key::X => {
                osk.buffer.clear();
                self.osk = Some(osk);
            }
            Key::Y => {
                osk_cycle_last_char(&mut osk.buffer);
                self.osk = Some(osk);
            }
            Key::Plus | Key::Z => {
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
        let allowed: &[MainSection] = if ed.save.is_some() {
            &[
                MainSection::File,
                MainSection::Hunter,
                MainSection::Items,
                MainSection::Gear,
                MainSection::PalicoRecord,
                MainSection::Quest,
            ]
        } else {
            &[MainSection::File, MainSection::Quest]
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

    fn goto_section(&mut self, ed: &mut Editor, sec: MainSection) {
        self.pending_delete = None;
        self.pending_delete_charm = None;
        if sec != MainSection::File && sec != MainSection::Quest && ed.save.is_none() {
            ed.message = String::from("先にセーブデータを開いてください");
            return;
        }
        match sec {
            MainSection::File => {
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
                if ed.save.is_some() {
                    ed.refresh_quest_titles();
                    ed.screen = Screen::SaveQuests;
                    let n = ed.filtered_quests().len();
                    ed.save_quest_cursor = ed.save_quest_cursor.min(n.saturating_sub(1));
                    ed.cursor = ed.save_quest_cursor;
                } else {
                    ed.screen = Screen::ArcList;
                    ed.cursor = 0;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // L / R: Sub-Tab System Inside the Current Section!
    // -----------------------------------------------------------------------
    fn sub_tabs_for(&self, ed: &Editor) -> (&'static [&'static str], usize) {
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
            return (labels, active);
        }

        match section_of(ed.screen) {
            MainSection::File => {
                let labels: &'static [&'static str] =
                    &["セーブ一覧", "形式変換", "環境設定"];
                let active = match ed.screen {
                    Screen::SaveList => 0,
                    Screen::Convert => 1,
                    Screen::Options => 2,
                    _ => 0,
                };
                (labels, active)
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
                (labels, active)
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
                (labels, active)
            }
            MainSection::Gear => {
                let labels: &'static [&'static str] = &[
                    "護石一覧",
                    "護石編集",
                    "装備BOX",
                    "マイセット",
                ];
                let active = match ed.screen {
                    Screen::Charms => 0,
                    Screen::CharmEdit | Screen::SkillPick => 1,
                    Screen::Equip => 2,
                    Screen::Sets => 3,
                    _ => 0,
                };
                (labels, active)
            }
            MainSection::PalicoRecord => {
                let labels: &'static [&'static str] = &[
                    "所持オトモ",
                    "配信オトモ",
                    "狩猟記録",
                    "ギルドカード",
                ];
                let active = match ed.screen {
                    Screen::Roster => 0,
                    Screen::Palico => 1,
                    Screen::Hunts => 2,
                    Screen::Guild => 3,
                    _ => 0,
                };
                (labels, active)
            }
            MainSection::Quest => {
                let labels: &'static [&'static str] = &[
                    "セーブ内",
                    "公式(189)",
                    "没クエ(34)",
                    "SD(.arc)",
                    "固定装備",
                ];
                let active = match ed.screen {
                    Screen::SaveQuests => self.quest_tab.min(2),
                    Screen::ArcList => 3,
                    Screen::QuestEquip => 4,
                    _ => 0,
                };
                (labels, active)
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
        self.pending_delete = None;
        self.pending_delete_charm = None;

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
                    let sems = sem_entry_indices(ed);
                    if let Some(&pos) = sems.first() {
                        ed.gmd = None;
                        ed.entry = pos;
                        ed.cursor = pos;
                        ed.entry_row_cursor = 0;
                        ed.screen = Screen::EntryView;
                        ed.message = String::from("初期配置座標 (.sem): [◀/▶]でエリア・X/Y/Z座標を変更 ([Y]で刻み幅)");
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
            MainSection::File => match idx {
                0 => {
                    ed.screen = Screen::SaveList;
                    ed.cursor = 0;
                }
                1 => {
                    if ed.save.is_none() {
                        ed.message = String::from("変換するには先にセーブデータを開いてください");
                    } else {
                        ed.screen = Screen::Convert;
                    }
                }
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
                1 => {
                    if ed.screen != Screen::CharmEdit {
                        ed.open_charms();
                        if ed.charms().is_empty() {
                            ed.input(Key::L);
                        } else {
                            ed.input(Key::Y);
                        }
                    }
                }
                2 => {
                    ed.screen = Screen::Equip;
                    ed.equip_cursor = 0;
                }
                _ => {
                    ed.screen = Screen::Sets;
                    self.sets_cursor = 0;
                }
            },
            MainSection::PalicoRecord => match idx {
                0 => {
                    ed.screen = Screen::Roster;
                    ed.roster_cursor = 0;
                }
                1 => {
                    ed.screen = Screen::Palico;
                    ed.cursor = ed.palico_cursor;
                }
                2 => {
                    ed.screen = Screen::Hunts;
                    ed.hunt_cursor = 0;
                }
                _ => {
                    ed.screen = Screen::Guild;
                }
            },
            MainSection::Quest => match idx {
                0..=2 => {
                    if ed.save.is_none() {
                        ed.message = String::from("先にセーブデータを開いてください");
                        return;
                    }
                    self.quest_tab = idx;
                    ed.refresh_quest_titles();
                    ed.screen = Screen::SaveQuests;
                }
                3 => {
                    ed.screen = Screen::ArcList;
                    ed.cursor = 0;
                }
                _ => {
                    if ed.save.is_none() {
                        ed.message = String::from("先にセーブデータを開いてください");
                        return;
                    }
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
        if ed.screen == Screen::SaveQuests && self.quest_tab > 0 {
            let len = bundled_quests_for_tab(self.quest_tab, &ed.quest_filter).len();
            if len > 0 {
                let cur = self.quest_cat_cursor[self.quest_tab] as i32;
                let next = (cur + delta).clamp(0, len as i32 - 1) as usize;
                self.quest_cat_cursor[self.quest_tab] = next;
            }
            return;
        }
        if ed.screen == Screen::Sets {
            let len = sets_len(ed, self.sets_tab);
            if len > 0 {
                let next = (self.sets_cursor as i32 + delta).clamp(0, len as i32 - 1) as usize;
                self.sets_cursor = next;
            }
            return;
        }
        scroll_active_cursor(ed, delta);
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
                    apply_mib_choice(ed, picker.field_idx, raw_val, &label);
                }
            }
            Key::B | Key::Minus => {}
            _ => {
                self.mib_picker = Some(picker);
            }
        }
        true
    }

    // -----------------------------------------------------------------------
    // Unified Controller Key Handler (A=Open/Edit, L/R=SubTab, X=Save, Y=Tool)
    // -----------------------------------------------------------------------
    pub fn handle_pad_key(&mut self, ed: &mut Editor, key: Key, _cur_buttons: u64) {
        // Picker screens use L/R for fast ±10 page jump; EVERY other screen uses L/R for Sub-Tab switching!
        if matches!(key, Key::L | Key::R) {
            if matches!(ed.screen, Screen::ItemPick | Screen::SkillPick) {
                let d = if key == Key::Right || key == Key::R { 10 } else { -10 };
                scroll_active_cursor(ed, d);
                return;
            }
            self.cycle_sub_tab(ed, key == Key::R);
            return;
        }

        // Global X = QuickSave (except on ItemPick / SkillPick where X clears search filter)
        if key == Key::X && !matches!(ed.screen, Screen::ItemPick | Screen::SkillPick) {
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
                Key::A => {
                    ed.screen = Screen::Char;
                    ed.field = 0;
                    ed.cursor = 0;
                }
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
                            let cur = ed.char_value(13);
                            ed.set_char_value(13, if cur == 0 { 1 } else { 0 });
                        } else {
                            self.apply_action(ed, UiAction::SetRowMax(ed.field));
                        }
                    }
                    Key::Y => {
                        ed.step = (ed.step + 1) % STEPS.len();
                        ed.message = format!("刻み幅を ±{} に切り替えました", STEPS[ed.step]);
                    }
                    other => ed.input(other),
                }
            }
            Screen::Items => match key {
                Key::A => {
                    // A opens the Item Picker for the selected slot!
                    ed.input(Key::R);
                }
                Key::Y => {
                    ed.step = (ed.step + 1) % STEPS.len();
                    ed.message = format!("個数の刻み幅を ±{} に切り替えました", STEPS[ed.step]);
                }
                Key::Minus => {
                    ed.input(Key::L); // Undo last item change
                }
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
                }
                other => ed.input(other),
            },
            Screen::Charms => match key {
                Key::Up | Key::Down => {
                    self.pending_delete_charm = None;
                    ed.input(key);
                }
                Key::A => {
                    // A safely opens the Charm Editor!
                    if ed.charms().is_empty() {
                        ed.input(Key::L);
                    } else {
                        ed.input(Key::Y);
                    }
                }
                Key::Y => {
                    self.apply_action(ed, UiAction::DeleteCharmConfirm);
                }
                Key::Minus => {
                    ed.input(Key::L); // Create new charm in free slot
                }
                other => ed.input(other),
            },
            Screen::CharmEdit => match key {
                Key::A => {
                    let field = ed.charm_edit.map(|c| c.field).unwrap_or(0);
                    if field == 2 || field == 4 {
                        ed.input(Key::Y); // Open Skill Picker
                    } else if field == 1 {
                        ed.input(Key::Right); // Cycle slots 0..3
                    } else {
                        ed.input(Key::A); // Apply charm to box
                    }
                }
                Key::Y => {
                    self.apply_action(ed, UiAction::CharmLegalMax);
                }
                other => ed.input(other),
            },
            Screen::Roster => match key {
                Key::Y => {
                    ed.roster_field = (ed.roster_field + 1) % 3;
                }
                other => ed.input(other),
            },
            Screen::Palico => match key {
                Key::Y => {
                    self.apply_action(ed, UiAction::InstallAllPalicoes);
                }
                Key::Left | Key::Right => {
                    self.apply_action(ed, UiAction::TogglePalicoLang);
                }
                other => ed.input(other),
            },
            Screen::Hunts => match key {
                Key::Y => {
                    ed.hunt_field = (ed.hunt_field + 1) % 4;
                }
                Key::A => {
                    ed.step = (ed.step + 1) % STEPS.len();
                    ed.message = format!("狩猟記録の刻み幅を {} に切り替えました", HUNT_STEPS[ed.step]);
                }
                other => ed.input(other),
            },
            Screen::Guild => match key {
                Key::Y => {
                    ed.step = (ed.step + 1) % STEPS.len();
                }
                other => ed.input(other),
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
                Key::Left | Key::Right | Key::Y => {
                    self.sets_tab = 1 - self.sets_tab;
                    self.sets_cursor = 0;
                }
                Key::B => {
                    ed.open_charms();
                }
                _ => {}
            },
            Screen::SaveQuests => match key {
                Key::Up if self.quest_tab > 0 => {
                    let list = bundled_quests_for_tab(self.quest_tab, &ed.quest_filter);
                    let n = list.len();
                    if n > 0 {
                        self.quest_cat_cursor[self.quest_tab] =
                            (self.quest_cat_cursor[self.quest_tab] + n - 1) % n;
                    }
                }
                Key::Down if self.quest_tab > 0 => {
                    let list = bundled_quests_for_tab(self.quest_tab, &ed.quest_filter);
                    let n = list.len();
                    if n > 0 {
                        self.quest_cat_cursor[self.quest_tab] =
                            (self.quest_cat_cursor[self.quest_tab] + 1) % n;
                    }
                }
                Key::A if self.quest_tab > 0 => {
                    let list = bundled_quests_for_tab(self.quest_tab, &ed.quest_filter);
                    let cur = self.quest_cat_cursor[self.quest_tab];
                    if let Some(q) = list.get(cur) {
                        ed.toggle_bundled_quest(q.quest_id);
                    }
                }
                Key::A if ed.save_quests().is_empty() => {
                    ed.install_official_dlc();
                }
                Key::Y => {
                    ed.input(Key::Y); // Search filter
                }
                Key::Left | Key::Right => {
                    // Do NOT switch tab on Left/Right; L/R handles tab switching!
                }
                other => ed.input(other),
            },
            Screen::ArcList => match key {
                Key::Y => {
                    self.wants_install_arc_idx = Some(ed.cursor);
                }
                Key::Minus => {
                    self.wants_extract_official_arcs = true;
                }
                Key::B => {
                    if ed.save.is_some() {
                        self.quest_tab = 0;
                        ed.screen = Screen::SaveQuests;
                    } else {
                        ed.screen = Screen::SaveList;
                    }
                }
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
            Screen::Mib => {
                let is_mob = questfields::MIB_FIELDS
                    .get(ed.mib_cursor)
                    .map(|f| f.key.starts_with("mob_slot["))
                    .unwrap_or(false);
                match key {
                    Key::A => {
                        if !self.open_mib_picker(ed, ed.mib_cursor) {
                            ed.message = String::from("この項目は [◀ / ▶] キーで数値を直接変更できます");
                        }
                    }
                    Key::Minus if is_mob => {
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
                        ed.message = format!("刻み幅を ±{} に切り替えました", STEPS[ed.step]);
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
                    Key::Y => {
                        ed.step = (ed.step + 1) % STEPS.len();
                        ed.message = format!("刻み幅を ±{} に切り替えました", STEPS[ed.step]);
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
        let mut changed = false;

        if touching && !self.was_touching {
            self.drag_start_y = ty;
            self.last_drag_y = ty;
            self.dragged = false;
            self.pending_activate_row = None;

            let hit = self.hits.iter().rev().find(|h| h.contains(tx, ty)).copied();
            if let Some(h) = hit {
                if h.is_row_body {
                    if let UiAction::SelectRow(idx) = h.action {
                        if ed.screen == Screen::SaveQuests && self.quest_tab > 0 {
                            if self.quest_cat_cursor[self.quest_tab] == idx {
                                self.pending_activate_row = Some(idx);
                            } else {
                                self.quest_cat_cursor[self.quest_tab] = idx;
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
            if self.mib_picker.is_some() {
                let dy = ty - self.last_drag_y;
                if (ty - self.drag_start_y).abs() > 18 {
                    self.dragged = true;
                    self.pending_activate_row = None;
                }
                let step_px = 36;
                if dy.abs() >= step_px {
                    let rows = -dy / step_px;
                    self.last_drag_y += -rows * step_px;
                    self.scroll_mib_picker(rows);
                    changed = true;
                }
            } else if ty >= CONTENT_Y && ty < FOOTER_Y {
                let dy = ty - self.last_drag_y;
                if (ty - self.drag_start_y).abs() > 18 {
                    self.dragged = true;
                    self.pending_activate_row = None;
                }
                let step_px = 36;
                if dy.abs() >= step_px {
                    let rows = -dy / step_px;
                    self.last_drag_y += -rows * step_px;
                    self.page_scroll(ed, rows);
                    changed = true;
                }
            }
        } else if !touching && self.was_touching {
            if !self.dragged {
                if let Some(idx) = self.pending_activate_row.take() {
                    self.apply_action(ed, UiAction::ActivateRow(idx));
                    changed = true;
                }
            }
            self.pending_activate_row = None;
        }

        self.was_touching = touching;
        changed
    }

    // -----------------------------------------------------------------------
    // Action Execution & One-Tap Bulk Cheat Implementations
    // -----------------------------------------------------------------------
    fn apply_action(&mut self, ed: &mut Editor, action: UiAction) {
        match action {
            UiAction::Key(k) => self.handle_pad_key(ed, k, 0),
            UiAction::SelectSlot(s) => {
                if s < 3 {
                    ed.slot = s;
                    if ed.screen == Screen::Slots {
                        ed.cursor = s;
                    }
                    ed.message = format!("ハンター枠 Slot {} に切り替えました", s + 1);
                }
            }
            UiAction::GotoSection(sec) => self.goto_section(ed, sec),
            UiAction::SelectSubTab(idx) => self.select_sub_tab(ed, idx),
            UiAction::CycleSection(fwd) => self.cycle_sidebar_tab(ed, fwd),
            UiAction::CycleSubTab(fwd) => self.cycle_sub_tab(ed, fwd),
            UiAction::SetStep(s) => {
                if s < STEPS.len() {
                    ed.step = s;
                    ed.message = format!("刻み幅を ±{} に設定しました", STEPS[s]);
                }
            }
            UiAction::SelectRow(idx) => {
                set_active_cursor(ed, self, idx);
            }
            UiAction::ActivateRow(idx) => {
                set_active_cursor(ed, self, idx);
                if ed.screen == Screen::SaveQuests && self.quest_tab > 0 {
                    let list = bundled_quests_for_tab(self.quest_tab, &ed.quest_filter);
                    if let Some(q) = list.get(idx) {
                        ed.toggle_bundled_quest(q.quest_id);
                    }
                } else {
                    self.handle_pad_key(ed, Key::A, 0);
                }
            }
            UiAction::AdjustRow(idx, key) => {
                set_active_cursor(ed, self, idx);
                self.handle_pad_key(ed, key, 0);
            }
            UiAction::AdjustBy(delta) => {
                let prev_step = ed.step;
                let abs_d = delta.abs();
                ed.step = STEPS.iter().position(|&s| s == abs_d).unwrap_or(0);
                self.handle_pad_key(ed, if delta >= 0 { Key::Right } else { Key::Left }, 0);
                ed.step = prev_step;
            }
            UiAction::SetRowMax(idx) => {
                set_active_cursor(ed, self, idx);
                apply_row_max(ed, self.mib_mob_sub, idx);
            }
            UiAction::OpenItemPicker(idx) => {
                set_active_cursor(ed, self, idx);
                ed.input(Key::R);
            }
            UiAction::SelectSubField(row_idx, sub) => {
                set_active_cursor(ed, self, row_idx);
                match ed.screen {
                    Screen::Hunts => ed.hunt_field = sub.min(3),
                    Screen::Roster => ed.roster_field = sub.min(2),
                    Screen::Guild => {
                        ed.guild_field = sub.min(2);
                        if sub == 2 {
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
            UiAction::OpenCharmSkill(field_idx) => {
                if let Some(mut ce) = ed.charm_edit {
                    ce.field = field_idx;
                    ed.charm_edit = Some(ce);
                    ed.input(Key::Y);
                }
            }
            UiAction::PageScroll(delta) => self.page_scroll(ed, delta),
            UiAction::Rescan => self.wants_rescan = true,
            UiAction::QuickSave => trigger_quick_save(ed),
            UiAction::CharMaxAll => {
                if ed.save.is_some() {
                    ed.set_char_value(1, 9_999_999);
                    ed.set_char_value(3, 999);
                    ed.set_char_value(4, 99_999_999);
                    for f in 5..=9 {
                        ed.set_char_value(f, 9_999_999);
                    }
                    ed.dirty = true;
                    ed.message = String::from("所持金・HR・全ポイントを最大値に設定しました");
                }
            }
            UiAction::ItemsMaxAll => {
                if let Some(mut items) = ed.save.as_ref().and_then(|s| s.read_item_box(ed.slot)) {
                    let mut count = 0usize;
                    for slot in items.iter_mut() {
                        if slot.id != 0 && slot.count < 99 {
                            slot.count = 99;
                            count += 1;
                        }
                    }
                    if let Some(s) = &mut ed.save {
                        let _ = s.write_item_box(ed.slot, &items);
                        ed.dirty = true;
                        ed.message = format!("所持アイテム {} 枠を×99に設定しました", count);
                    }
                }
            }
            UiAction::CharmLegalMax => {
                if let Some(mut ce) = ed.charm_edit {
                    // The ceiling now comes from the grade the form shows, not
                    // from the type byte still stored in the box: those two
                    // disagree the moment the rarity row is moved.
                    let grade = ce.grade;
                    ce.slots = grade.max_slots();
                    for (skill, pts, second) in [
                        (ce.skill1, &mut ce.points1, false),
                        (ce.skill2, &mut ce.points2, true),
                    ] {
                        if skill == 0 {
                            continue;
                        }
                        if let Some(r) = charm::allowed_points_rarity(grade.rarity(), skill, second)
                        {
                            if r.max > 0 {
                                *pts = r.max;
                            }
                        }
                    }
                    ed.charm_edit = Some(ce);
                    ed.message = format!(
                        "護石を合法最大値・スロ{}に設定しました（{}）",
                        ce.slots,
                        grade.name()
                    );
                }
            }
            UiAction::CharmPreset(preset_id) => {
                if let Some(mut ce) = ed.charm_edit {
                    let find_skill = |target: &str| -> u8 {
                        mhnx_core::skills::SKILL_NAMES
                            .iter()
                            .position(|&n| n == target)
                            .map(|i| (i + 1) as u8)
                            .unwrap_or(0)
                    };
                    let (s1_name, p1, s2_name, p2) = match preset_id {
                        0 => ("痛撃", 6i8, "達人", 10i8),
                        1 => ("斬れ味", 6, "達人", 10),
                        2 => ("溜め短縮", 6, "達人", 10),
                        3 => ("通常弾強化", 6, "達人", 10),
                        _ => ("貫通弾強化", 6, "達人", 10),
                    };
                    let grade = ce.grade;
                    ce.skill1 = find_skill(s1_name);
                    ce.skill2 = find_skill(s2_name);
                    // A preset asks for the strongest version of a build; the
                    // grade still decides how many slots it may carry and how
                    // high each of its two skills may go.
                    ce.slots = grade.max_slots();
                    ce.points1 = preset_points(grade, ce.skill1, p1, false);
                    ce.points2 = preset_points(grade, ce.skill2, p2, true);
                    ed.charm_edit = Some(ce);
                    ed.message = format!(
                        "プリセット ({} {:+} / {} {:+} / S{}) を適用しました（{}）",
                        s1_name,
                        ce.points1,
                        s2_name,
                        ce.points2,
                        ce.slots,
                        grade.name()
                    );
                }
            }
            UiAction::NewCharm => {
                ed.open_charms();
                ed.input(Key::L);
            }
            UiAction::DeleteCharmConfirm => {
                let list = ed.charms();
                if let Some(chm) = list.get(ed.cursor) {
                    if self.pending_delete_charm == Some(chm.index) {
                        self.pending_delete_charm = None;
                        ed.input(Key::A);
                    } else {
                        self.pending_delete_charm = Some(chm.index);
                        ed.message = format!(
                            "枠 #{:04} を削除しますか？ もう一度 [Y] で削除確定",
                            chm.index
                        );
                    }
                }
            }
            UiAction::RosterMaxAll => {
                let list = ed.palicos();
                let slot = ed.slot;
                if let Some(save) = &mut ed.save {
                    let mut cnt = 0usize;
                    for p in &list {
                        let _ = mhnx_core::palico::set_level(save, slot, p.index, 99);
                        let _ = mhnx_core::palico::set_enthusiasm(save, slot, p.index, 100);
                        let _ = mhnx_core::palico::set_exp(save, slot, p.index, 999_999);
                        cnt += 1;
                    }
                    if cnt > 0 {
                        ed.dirty = true;
                        ed.message = format!("オトモ {} 匹を Lv99・最大に設定しました", cnt);
                    }
                }
            }
            UiAction::InstallOfficialDlc => ed.install_official_dlc(),
            UiAction::InstallDlcCategory(cat) => match cat {
                1 => ed.install_dlc_category(false, true, "没データ(34件)"),
                _ => ed.install_dlc_category(true, false, "公式イベント(189件)"),
            },
            UiAction::SetQuestTab(tab) => {
                self.quest_tab = tab.min(2);
                ed.screen = Screen::SaveQuests;
            }
            UiAction::ToggleBundledQuest(qid) => ed.toggle_bundled_quest(qid),
            UiAction::RemoveSaveQuest(qid) => {
                if self.pending_delete == Some(qid) {
                    self.pending_delete = None;
                    ed.remove_save_quest(qid);
                } else {
                    self.pending_delete = Some(qid);
                }
            }
            UiAction::OpenBundledQuest(qid) => ed.open_bundled_quest(qid),
            UiAction::OpenMibPicker(field_idx) => {
                let _ = self.open_mib_picker(ed, field_idx);
            }
            UiAction::MibPickerSelect(idx) => {
                if let Some(mut picker) = self.mib_picker.take() {
                    if picker.cursor == idx {
                        if let Some((raw_val, label)) = picker.items.get(idx).cloned() {
                            apply_mib_choice(ed, picker.field_idx, raw_val, &label);
                        }
                    } else {
                        picker.cursor = idx.min(picker.items.len().saturating_sub(1));
                        self.mib_picker = Some(picker);
                    }
                }
            }
            UiAction::MibPickerScroll(delta) => self.scroll_mib_picker(delta),
            UiAction::MibPickerClose => self.mib_picker = None,
            UiAction::MibPreset(preset) => apply_mib_preset(ed, preset),
            UiAction::SemPreset(preset) => apply_sem_preset(ed, preset),
            UiAction::InstallAllPalicoes => ed.install_all_dlc_palicoes(),
            UiAction::TogglePalicoLang => {
                ed.palico_lang = 1 - ed.palico_lang;
                ed.message = if ed.palico_lang == 0 {
                    String::from("配信オトモ言語: 日本語に切り替えました")
                } else {
                    String::from("配信オトモ言語: 英語 (English) に切り替えました")
                };
            }
            UiAction::ExtractOfficialArcs => self.wants_extract_official_arcs = true,
            UiAction::InstallArcRow(idx) => {
                ed.cursor = idx;
                self.wants_install_arc_idx = Some(idx);
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
            UiAction::OskClear => {
                if let Some(osk) = &mut self.osk {
                    osk.buffer.clear();
                }
            }
            UiAction::OskModify => {
                if let Some(osk) = &mut self.osk {
                    osk_cycle_last_char(&mut osk.buffer);
                }
            }
            UiAction::OskSpace => {
                if let Some(osk) = &mut self.osk {
                    if osk.buffer.len() < osk.max_bytes {
                        osk.buffer.push(' ');
                    }
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

fn sem_entry_indices(ed: &Editor) -> Vec<usize> {
    let Some(arc) = &ed.arc else {
        return Vec::new();
    };
    let mut active = Vec::new();
    let mut dummy = Vec::new();
    for (i, e) in arc.entries.iter().enumerate() {
        if extension_for(e.type_hash) == Some("sem") && e.data.len() >= 0x20 {
            let area = u32::from_le_bytes([e.data[0x0C], e.data[0x0D], e.data[0x0E], e.data[0x0F]]);
            if e.name.contains("em000") && area == 0 {
                dummy.push(i);
            } else {
                active.push(i);
            }
        }
    }
    active.extend(dummy);
    active
}

fn adjust_mib_mob_sub(ed: &mut Editor, field_idx: usize, sub: usize, delta: i32) {
    let Some(f) = questfields::MIB_FIELDS.get(field_idx) else {
        return;
    };
    if !f.key.starts_with("mob_slot[") || f.offset + 13 > ed.mib_bytes.len() {
        return;
    }
    let off = f.offset;
    let label = MIB_MOB_SUB_LABELS.get(sub).copied().unwrap_or("値");
    match sub {
        0 => {
            let cur = ed.mib_bytes[off + 5] as i32;
            let next = (cur + delta).clamp(1, 255) as u8;
            ed.mib_bytes[off + 5] = next;
            ed.dirty = true;
            ed.message = format!("{}: {} を {} に変更しました", f.ja, label, next);
        }
        1 => {
            let cur = ed.mib_bytes[off + 6] as i32;
            let next = (cur + delta).clamp(1, 255) as u8;
            ed.mib_bytes[off + 6] = next;
            ed.dirty = true;
            ed.message = format!("{}: {} を {} に変更しました", f.ja, label, next);
        }
        2 => {
            let cur = ed.mib_bytes[off + 7] as i32;
            let next = (cur + delta).clamp(1, 255) as u8;
            ed.mib_bytes[off + 7] = next;
            ed.dirty = true;
            ed.message = format!("{}: {} を {} に変更しました", f.ja, label, next);
        }
        3 => {
            let cur = u16::from_le_bytes([ed.mib_bytes[off + 9], ed.mib_bytes[off + 10]]) as i32;
            let next = (cur + delta).clamp(10, 500) as u16;
            ed.mib_bytes[off + 9..off + 11].copy_from_slice(&next.to_le_bytes());
            ed.dirty = true;
            ed.message = format!("{}: {} を {}% に変更しました", f.ja, label, next);
        }
        4 => {
            let cur = ed.mib_bytes[off + 4] as i32;
            let next = (cur + delta).clamp(0, 99) as u8;
            ed.mib_bytes[off + 4] = next;
            ed.dirty = true;
            ed.message = format!("{}: {} を {} に変更しました", f.ja, label, next);
        }
        5 => {
            let cur = ed.mib_bytes[off + 8] as i32;
            let next = (cur + delta).clamp(0, 255) as u8;
            ed.mib_bytes[off + 8] = next;
            ed.dirty = true;
            ed.message = format!("{}: {} を {} に変更しました", f.ja, label, next);
        }
        6 => {
            let cur = ed.mib_bytes[off + 11] as i32;
            let next = (cur + delta).clamp(0, 255) as u8;
            ed.mib_bytes[off + 11] = next;
            ed.dirty = true;
            ed.message = format!("{}: {} を {} に変更しました", f.ja, label, next);
        }
        _ => {
            let cur = ed.mib_bytes[off + 3] as i32;
            let next = (cur + delta).clamp(0, 255) as u8;
            ed.mib_bytes[off + 3] = next;
            ed.dirty = true;
            ed.message = format!("{}: {} を {} に変更しました", f.ja, label, next);
        }
    }
}

fn adjust_entry_view_row(ed: &mut Editor, row_idx: usize, delta: i32) {
    let ext = ed
        .arc
        .as_ref()
        .and_then(|a| a.entries.get(ed.entry))
        .and_then(|e| extension_for(e.type_hash));
    match ext {
        Some("sem") => {
            let sems = sem_entry_indices(ed);
            let sem_k = row_idx / 6;
            let field = row_idx % 6;
            let Some(&entry_idx) = sems.get(sem_k) else {
                return;
            };
            let Some(arc) = &mut ed.arc else {
                return;
            };
            let Some(e) = arc.entries.get_mut(entry_idx) else {
                return;
            };
            if e.data.len() < 0x20 {
                return;
            }
            match field {
                0 => {
                    let cur = u32::from_le_bytes([e.data[0x0C], e.data[0x0D], e.data[0x0E], e.data[0x0F]]) as i32;
                    let step_a = if delta >= 0 { 1 } else { -1 };
                    let next = (cur + step_a).clamp(0, 20) as u32;
                    e.data[0x0C..0x10].copy_from_slice(&next.to_le_bytes());
                    if next > 0 {
                        let w0 = f32::from_le_bytes([e.data[0], e.data[1], e.data[2], e.data[3]]);
                        if w0 == 0.0 {
                            e.data[0x00..0x04].copy_from_slice(&1.0f32.to_le_bytes());
                            e.data[0x04..0x08].copy_from_slice(&1u32.to_le_bytes());
                        }
                    }
                    ed.dirty = true;
                    ed.message = format!("配置{} ({}) の初期エリア番号を {} に変更しました", sem_k + 1, e.name, next);
                }
                1..=4 => {
                    let off = 0x10 + (field - 1) * 4;
                    let cur = f32::from_le_bytes([e.data[off], e.data[off + 1], e.data[off + 2], e.data[off + 3]]);
                    let next = cur + delta as f32;
                    let clamped = if field == 4 {
                        next.clamp(-360.0, 360.0)
                    } else {
                        next.clamp(-99999.0, 99999.0)
                    };
                    e.data[off..off + 4].copy_from_slice(&clamped.to_le_bytes());
                    let axis = match field {
                        1 => "X座標",
                        2 => "Y座標(高さ)",
                        3 => "Z座標",
                        _ => "向き角度(W)",
                    };
                    ed.dirty = true;
                    ed.message = format!("配置{} ({}) の {} を {:.1} に変更しました", sem_k + 1, e.name, axis, clamped);
                }
                _ => {
                    let cur = u32::from_le_bytes([e.data[0x08], e.data[0x09], e.data[0x0A], e.data[0x0B]]) as i32;
                    let step_w = if delta >= 0 { 1 } else { -1 };
                    let next = (cur + step_w).clamp(0, 10) as u32;
                    e.data[0x08..0x0C].copy_from_slice(&next.to_le_bytes());
                    ed.dirty = true;
                    ed.message = format!("配置{} ({}) の出現ウェーブを {} に変更しました", sem_k + 1, e.name, next);
                }
            }
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

fn apply_sem_preset(ed: &mut Editor, preset: u8) {
    let sems = sem_entry_indices(ed);
    if sems.is_empty() {
        return;
    }
    match preset {
        0 => {
            let first_idx = sems[0];
            let Some(arc) = &mut ed.arc else { return };
            let Some(src) = arc.entries.get(first_idx).map(|e| e.data.clone()) else {
                return;
            };
            if src.len() < 0x20 {
                return;
            }
            let mut count = 0usize;
            for &idx in sems.iter().skip(1) {
                if let Some(e) = arc.entries.get_mut(idx) {
                    if !e.name.contains("em000") && e.data.len() >= 0x20 {
                        e.data[0x0C..0x20].copy_from_slice(&src[0x0C..0x20]);
                        count += 1;
                    }
                }
            }
            ed.dirty = true;
            if count > 0 {
                ed.message = format!("配置1のエリア・座標を他 {} 体の初期配置へコピーしました", count);
            } else {
                ed.message = String::from("コピー対象の2体目以降の初期配置がありません");
            }
        }
        _ => {
            let row_idx = ed.entry_row_cursor;
            let sem_k = row_idx / 6;
            let field = row_idx % 6;
            let Some(&entry_idx) = sems.get(sem_k) else { return };
            let Some(arc) = &mut ed.arc else { return };
            let Some(e) = arc.entries.get_mut(entry_idx) else { return };
            if e.data.len() < 0x20 {
                return;
            }
            if (1..=4).contains(&field) {
                let off = 0x10 + (field - 1) * 4;
                e.data[off..off + 4].copy_from_slice(&0.0f32.to_le_bytes());
                ed.dirty = true;
                ed.message = format!("配置{} ({}) の選択座標を 0.0 にリセットしました", sem_k + 1, e.name);
            } else {
                e.data[0x10..0x20].fill(0);
                ed.dirty = true;
                ed.message = format!("配置{} ({}) の X/Y/Z/W をすべて 0.0 にリセットしました", sem_k + 1, e.name);
            }
        }
    }
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
        Screen::ItemPick => ed.item_pick_cursor,
        Screen::SkillPick => ed.skill_pick_cursor,
        Screen::Equip => ed.equip_cursor,
        Screen::Hunts => ed.hunt_cursor,
        Screen::Guild => ed.guild_field,
        Screen::Roster => ed.roster_cursor,
        Screen::Sets => gui.sets_cursor,
        Screen::SaveQuests => {
            if gui.quest_tab > 0 {
                gui.quest_cat_cursor[gui.quest_tab]
            } else {
                ed.save_quest_cursor
            }
        }
        Screen::QuestEquip => {
            if ed.qe_open {
                ed.qe_item_cursor
            } else {
                ed.cursor
            }
        }
        Screen::Mib => ed.mib_cursor,
        Screen::Gmd => ed.gmd_cursor,
        Screen::Check => ed.check_cursor,
        Screen::EntryView => ed.entry_row_cursor,
        Screen::Palico => ed.palico_cursor,
        Screen::Options => ed.option_cursor,
        _ => ed.cursor,
    }
}

fn list_len_of(ed: &Editor, gui: &GuiState) -> usize {
    match ed.screen {
        Screen::SaveList => ed.save_files.len(),
        Screen::Slots => 3,
        Screen::Char => CHAR_FIELDS.len(),
        Screen::Items => ed.visible_items().len(),
        Screen::ItemPick => ed.item_pick_matches().len(),
        Screen::Charms => ed.charms().len(),
        Screen::CharmEdit => 7,
        Screen::SkillPick => ed.skill_pick_matches().len(),
        Screen::Equip => ed.equipment().len(),
        Screen::Hunts => ed.hunt_rows().len(),
        Screen::Guild => 3,
        Screen::Roster => ed.palicos().len(),
        Screen::Sets => sets_len(ed, gui.sets_tab),
        Screen::SaveQuests => {
            if gui.quest_tab > 0 {
                bundled_quests_for_tab(gui.quest_tab, &ed.quest_filter).len()
            } else {
                ed.filtered_quests().len()
            }
        }
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
                    Some("rem") | Some("sup") => {
                        ItemTable::parse(&e.data).map(|t| t.records.len()).unwrap_or(0)
                    }
                    Some("sem") => sem_entry_indices(ed).len() * 6,
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
        Screen::Gmd => ed.gmd.as_ref().map(|g| g.strings.len()).unwrap_or(0),
        Screen::Check => ed.findings.len(),
        Screen::Palico => dlcats::all_ids().len(),
        Screen::Options => 4,
        Screen::Convert => 0,
    }
}

fn set_active_cursor(ed: &mut Editor, gui: &mut GuiState, idx: usize) {
    if ed.screen == Screen::SaveQuests && gui.quest_tab > 0 {
        let n = bundled_quests_for_tab(gui.quest_tab, &ed.quest_filter).len();
        if n > 0 {
            gui.quest_cat_cursor[gui.quest_tab] = idx.min(n - 1);
        }
        return;
    }
    if ed.screen == Screen::Sets {
        let n = sets_len(ed, gui.sets_tab);
        if n > 0 {
            gui.sets_cursor = idx.min(n - 1);
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
        Screen::ItemPick => ed.item_pick_cursor = clamped,
        Screen::SkillPick => ed.skill_pick_cursor = clamped,
        Screen::Equip => ed.equip_cursor = clamped,
        Screen::Hunts => ed.hunt_cursor = clamped,
        Screen::Guild => ed.guild_field = clamped,
        Screen::Roster => ed.roster_cursor = clamped,
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
            ed.palico_cursor = clamped;
            ed.cursor = clamped;
        }
        Screen::Options => ed.option_cursor = clamped,
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
        Screen::Char => {
            let max_val = match idx {
                1 => 9_999_999,
                3 => 999,
                4 => 99_999_999,
                5..=9 => 9_999_999,
                _ => return,
            };
            ed.set_char_value(idx, max_val);
            ed.message = format!("{} を最大値 ({}) に設定しました", CHAR_FIELDS[idx].1, max_val);
        }
        Screen::Items => {
            let vis = ed.visible_items();
            if let Some(&box_idx) = vis.get(idx) {
                if let Some(mut items) = ed.save.as_ref().and_then(|s| s.read_item_box(ed.slot)) {
                    if items[box_idx].id == 0 {
                        ed.message = String::from("空きスロットです。[A] で先にアイテムを選んでください");
                        return;
                    }
                    items[box_idx].count = 99;
                    if let Some(s) = &mut ed.save {
                        let _ = s.write_item_box(ed.slot, &items);
                        ed.dirty = true;
                        ed.message = format!("枠 #{} の個数を 99 に設定しました", box_idx + 1);
                    }
                }
            }
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
                apply_sem_preset(ed, 1);
            } else {
                let prev = ed.entry_row_cursor;
                ed.entry_row_cursor = idx;
                for _ in 0..99 {
                    ed.input(Key::Right);
                }
                ed.entry_row_cursor = prev;
            }
        }
        _ => {}
    }
}

fn apply_mib_preset(ed: &mut Editor, preset: u8) {
    let mib = &mut ed.mib_bytes;
    if mib.len() < 0x180 {
        return;
    }
    match preset {
        0 => {
            for f in questfields::MIB_FIELDS.iter() {
                if f.key.starts_with("mob_slot[") && f.offset + 13 <= mib.len() {
                    let mid = u16::from_le_bytes([mib[f.offset], mib[f.offset + 1]]);
                    if mid != 0 {
                        mib[f.offset + 5] = 1;
                    }
                }
            }
            ed.dirty = true;
            ed.message = String::from("全モンスターの体力を最小(1)に設定しました");
        }
        1 => {
            for f in questfields::MIB_FIELDS.iter() {
                if f.key.starts_with("mob_slot[") && f.offset + 13 <= mib.len() {
                    let mid = u16::from_le_bytes([mib[f.offset], mib[f.offset + 1]]);
                    if mid != 0 {
                        mib[f.offset + 9..f.offset + 11].copy_from_slice(&200u16.to_le_bytes());
                    }
                }
            }
            ed.dirty = true;
            ed.message = String::from("全モンスターのサイズを 200% (最大金冠) に設定しました");
        }
        _ => {
            for f in questfields::MIB_FIELDS.iter() {
                if f.key.starts_with("mob_slot[") && f.offset + 13 <= mib.len() {
                    let mid = u16::from_le_bytes([mib[f.offset], mib[f.offset + 1]]);
                    if mid != 0 {
                        mib[f.offset + 9..f.offset + 11].copy_from_slice(&50u16.to_le_bytes());
                    }
                }
            }
            ed.dirty = true;
            ed.message = String::from("全モンスターのサイズを 50% (最小金冠) に設定しました");
        }
    }
}

fn trigger_quick_save(ed: &mut Editor) {
    match ed.screen {
        Screen::CharmEdit => {
            ed.input(Key::A);
            if ed.save.is_some() {
                ed.input(Key::X);
            }
        }
        Screen::Mib | Screen::Gmd | Screen::EntryView | Screen::Check | Screen::ArcEntries => {
            safe_flush_quest_entry(ed);
            if ed.arc_path.is_some() {
                if ed.screen == Screen::Mib {
                    ed.input(Key::A);
                } else {
                    let prev = ed.gmd.take();
                    ed.input(Key::X);
                    if ed.screen == Screen::Gmd {
                        ed.gmd = prev;
                    }
                }
            } else if ed.save.is_some() {
                ed.install_into_save();
                let prev_screen = ed.screen;
                let prev_cursor = ed.cursor;
                ed.screen = Screen::Char;
                ed.input(Key::A);
                ed.screen = prev_screen;
                ed.cursor = prev_cursor;
            }
        }
        Screen::Char | Screen::Items | Screen::Options => ed.input(Key::A),
        Screen::Charms | Screen::Palico => ed.input(Key::X),
        _ => {
            if ed.save.is_some() {
                let prev_screen = ed.screen;
                let prev_cursor = ed.cursor;
                ed.screen = Screen::Char;
                ed.input(Key::A);
                ed.screen = prev_screen;
                ed.cursor = prev_cursor;
            }
        }
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
#[allow(clippy::too_many_arguments)]
fn charm_warnings(
    box_bytes: Option<&[u8]>,
    index: usize,
    grade: charm::TalismanGrade,
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
    let max_slots = grade.max_slots();
    if slots > max_slots {
        push_unique(
            &mut out,
            format!(
                "スロット{}個は{}の上限{}を超えています",
                slots,
                grade.name(),
                max_slots
            ),
        );
    }
    for (skill, pts, second, label) in [
        (skill1, points1, false, "スキル1"),
        (skill2, points2, true, "スキル2"),
    ] {
        if skill == 0 {
            continue;
        }
        match charm::allowed_points_rarity(grade.rarity(), skill, second) {
            Some(r) if r.min == 0 && r.max == 0 => {
                push_unique(
                    &mut out,
                    format!("{} は{}では付けられません", label, grade.name()),
                );
            }
            Some(r) => {
                if pts < r.min || pts > r.max {
                    push_unique(
                        &mut out,
                        format!("{} {} は {}〜{} の範囲外です", label, pts, r.min, r.max),
                    );
                }
            }
            None => {}
        }
    }
    out
}

/// A preset's requested points, clamped to what the grade actually allows.
///
/// The four presets used to force `slots = 3` and their nominal `+6/+10`
/// whatever the grade was, which is exactly the record the game would reject.
fn preset_points(grade: charm::TalismanGrade, skill: u8, want: i8, second: bool) -> i8 {
    if skill == 0 {
        return 0;
    }
    match charm::allowed_points_rarity(grade.rarity(), skill, second) {
        Some(r) if r.max > 0 => want.clamp(r.min, r.max),
        // "0~0": the grade cannot carry this skill, so do not write points for it.
        Some(_) => 0,
        None => want,
    }
}

impl GuiState {
    pub fn draw(&mut self, c: &mut Canvas, ed: &Editor) {
        self.hits.clear();
        let t = theme();
        c.clear(t.bg_canvas);

        // 1. Top 2-Tier Navigation Header (y = 0..92)
        self.draw_top_nav(c, ed);

        // 2. Main Content Workspace (y = CONTENT_Y..FOOTER_Y)
        self.draw_content(c, ed);

        // 3. Minimal Unified Footer Bar (y = FOOTER_Y..720)
        self.draw_footer(c, ed);

        // 4. Modal Overlays (MIB Picker / Built-in OSK)
        if self.mib_picker.is_some() {
            self.draw_mib_picker(c);
        }
        if self.osk.is_some() {
            self.draw_osk(c);
        }

        c.present();
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

        // 6 Main Sections (x = 66..894)
        let mut sx = 66i32;
        let sec_w = 132i32;
        for &(sec, label) in MAIN_SECTIONS.iter() {
            let is_active = sec == active_sec;
            if is_active {
                c.round_rect(sx, 6, sec_w, 36, 6, t.bg_selected);
                c.rect(sx + 12, TOP_SEC_H - 3, sec_w - 24, 3, t.accent_primary);
            }
            let fg = if is_active { t.text_primary } else { t.text_secondary };
            btn_text(c, sx, 6, sec_w, 36, label, fg, FontSize::Small);
            self.add_hit(sx, 6, sec_w, 36, UiAction::GotoSection(sec));
            sx += sec_w + 6;
        }

        // [ZR] button
        c.round_rect(sx, 8, 44, 32, t.corner_r, t.bg_btn);
        c.round_rect_border(sx, 8, 44, 32, t.corner_r, 1, t.border_subtle);
        btn_text(c, sx, 8, 44, 32, "ZR", t.accent_primary, FontSize::Small);
        self.add_hit(sx, 8, 44, 32, UiAction::CycleSection(true));

        // Right Status Area: Active Slot badge + [X] Save Pill
        let slot_x = 964;
        let slot_w = 144;
        let slot_label = if let Some(save) = &ed.save {
            let sname = save
                .read_character(ed.slot)
                .map(|ch| ch.name)
                .unwrap_or_else(|| String::from("-"));
            format!("Slot {}: {}", ed.slot + 1, sname)
        } else {
            String::from("未読込")
        };
        c.round_rect(slot_x, 8, slot_w, 32, t.corner_r, t.bg_panel);
        c.round_rect_border(slot_x, 8, slot_w, 32, t.corner_r, 1, t.border_subtle);
        btn_text(c, slot_x, 8, slot_w, 32, &slot_label, t.text_secondary, FontSize::Small);
        self.add_hit(slot_x, 8, slot_w, 32, UiAction::SelectSlot((ed.slot + 1) % 3));

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
        for (idx, &label) in subs.iter().enumerate() {
            let is_active = idx == active_sub;
            let tw = c.text_width_sized(label, FontSize::Small) + 24;
            let w = tw.clamp(84, 156);
            if sub_x + w > 880 {
                break;
            }
            let bg = if is_active { t.bg_selected } else { t.bg_subbar };
            let fg = if is_active { t.accent_primary } else { t.text_secondary };
            c.round_rect(sub_x, sy, w, sh, 5, bg);
            if is_active {
                c.rect(sub_x + 8, sy + sh - 3, w - 16, 3, t.accent_secondary);
            }
            btn_text(c, sub_x, sy, w, sh, label, fg, FontSize::Small);
            self.add_hit(sub_x, sy, w, sh, UiAction::SelectSubTab(idx));
            sub_x += w + 6;
        }

        // [R] Sub-Tab Next
        c.round_rect(sub_x + 2, sy, 40, sh, 6, t.bg_btn);
        c.round_rect_border(sub_x + 2, sy, 40, sh, 6, 1, t.border_subtle);
        btn_text(c, sub_x + 2, sy, 40, sh, "R", t.accent_primary, FontSize::Small);
        self.add_hit(sub_x + 2, sy, 40, sh, UiAction::CycleSubTab(true));

        // Right side of Sub-Tab Bar: Concise Tool Pills (x = 964..1266)
        self.draw_sub_bar_quick_tools(c, ed, sy, sh);
    }

    fn draw_sub_bar_quick_tools(&mut self, c: &mut Canvas, ed: &Editor, sy: i32, sh: i32) {
        let t = theme();
        let next_step = (ed.step + 1) % STEPS.len();
        match ed.screen {
            Screen::Char => {
                let step_lbl = format!("[Y] 刻み ±{}", STEPS[ed.step]);
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, &step_lbl, t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::SetStep(next_step));

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, "資金・P 一括MAX", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::CharMaxAll);
            }
            Screen::Items => {
                let step_lbl = format!("[Y] 刻み ±{}", STEPS[ed.step]);
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, &step_lbl, t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::SetStep(next_step));

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, "所持品 全枠×99", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::ItemsMaxAll);
            }
            Screen::Charms => {
                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, "新規護石作成", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::NewCharm);
            }
            Screen::CharmEdit => {
                // Slot ceiling follows the grade on screen, never a fixed S3.
                let max_s = ed.charm_edit.map(|c| c.grade.max_slots()).unwrap_or(3);
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, "[Y] 合法最大値", t.accent_secondary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::CharmLegalMax);

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                let preset_lbl = format!("痛撃6 達人10 S{}", max_s);
                btn_text(c, 1108, sy, 158, sh, &preset_lbl, t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::CharmPreset(0));
            }
            Screen::Roster => {
                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, "全オトモ Lv99", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::RosterMaxAll);
            }
            Screen::Palico => {
                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, "全28匹 一括導入", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::InstallAllPalicoes);
            }
            Screen::Mib => {
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, "大型体力=1", t.accent_secondary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::MibPreset(0));

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, "最大金冠 200%", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::MibPreset(1));
            }
            Screen::EntryView => {
                let step_lbl = format!("[Y] 刻み ±{}", STEPS[ed.step]);
                c.round_rect(964, sy, 136, sh, 5, t.bg_btn);
                c.round_rect_border(964, sy, 136, sh, 5, 1, t.border_subtle);
                btn_text(c, 964, sy, 136, sh, &step_lbl, t.text_primary, FontSize::Small);
                self.add_hit(964, sy, 136, sh, UiAction::SetStep(next_step));

                c.round_rect(1108, sy, 158, sh, 5, t.bg_btn_primary);
                c.round_rect_border(1108, sy, 158, sh, 5, 1, t.accent_primary);
                btn_text(c, 1108, sy, 158, sh, "配置1座標コピー", t.text_primary, FontSize::Small);
                self.add_hit(1108, sy, 158, sh, UiAction::SemPreset(0));
            }
            _ => {}
        }
    }

    fn draw_footer(&mut self, c: &mut Canvas, ed: &Editor) {
        let t = theme();
        c.rect(0, FOOTER_Y, 1280, FOOTER_H, t.bg_footer);
        c.hline(0, FOOTER_Y, 1280, t.border_subtle);

        // Right: Concise, non-redundant key guide
        let hint = match ed.screen {
            Screen::SaveList => "[A] 開く  [Y] 再読込  [L/R] サブタブ  [ZL/ZR] セクション",
            Screen::Slots => "[◀/▶] 枠選択  [A] 決定  [Y] 一括MAX  [L/R] サブタブ",
            Screen::Char => "[◀/▶] 増減  [A] 最大/編集  [Y] 刻み  [L/R] サブタブ  [X] 保存",
            Screen::Items => "[A] 変更  [◀/▶] 個数  [Y] 刻み  [L/R] ページ  [X] 保存",
            Screen::ItemPick | Screen::SkillPick => "[A] 決定  [Y] 検索  [X] クリア  [L/R] ±10行  [B] 戻る",
            Screen::Charms => "[A] 編集  [Y×2] 削除  [-] 新規  [L/R] サブタブ  [X] 保存",
            Screen::CharmEdit => {
                "[A] BOXへ反映  [▲/▼] 行  [◀/▶] 値  [Y] スキル選択  [X] 刻み  [B] 戻る"
            }
            Screen::Options => "[◀/▶] 値変更  [Y] 直近スナップショットへ戻す  [X] 保存  [B] 戻る",
            Screen::Roster => "[◀/▶] 増減  [Y] 項目切替  [A] 名前  [L/R] サブタブ  [X] 保存",
            Screen::Palico => "[A] 導入/解除  [Y] 全導入  [◀/▶] 言語  [L/R] サブタブ  [X] 保存",
            Screen::Hunts => "[◀/▶] 増減  [Y] 列切替  [A] 刻み  [L/R] サブタブ  [X] 保存",
            Screen::SaveQuests => "[A] 編集/導入  [Y] 検索  [L/R] サブタブ  [X] 保存",
            Screen::ArcEntries => "[A] 開く  [Y] 検査  [L/R] サブタブ  [X] 保存  [B] 戻る",
            Screen::Mib => "[A] 一覧選択  [◀/▶] 増減  [-] 項目切替  [Y] 刻み  [L/R] タブ  [X] 保存",
            Screen::Gmd => "[A] 文章編集  [L/R] サブタブ  [X] 保存  [B] 戻る",
            Screen::EntryView => "[◀/▶] 座標・数値変更  [Y] 刻み  [L/R] サブタブ  [X] 保存  [B] 戻る",
            _ => "[A] 決定  [◀/▶] 調整  [L/R] サブタブ  [ZL/ZR] セクション  [X] 保存",
        };
        let hint_w = c.text_width_sized(hint, FontSize::Small);
        c.text_right(1262, FOOTER_Y + 17, hint, t.text_secondary, FontSize::Small);

        // Left: Status message dynamically clipped so it NEVER collides with `hint`
        let msg = if ed.message.is_empty() {
            "Ready"
        } else {
            &ed.message
        };
        c.circle(22, FOOTER_Y + 26, 4, if ed.dirty { t.accent_warn } else { t.accent_primary });
        let max_msg_w = (1262 - hint_w - 60).max(180);
        c.text_clipped(36, FOOTER_Y + 17, max_msg_w, msg, t.text_primary, FontSize::Small);
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
            Screen::Sets => self.draw_sets(c, ed),
            Screen::Roster => self.draw_roster(c, ed),
            Screen::Palico => self.draw_palico(c, ed),
            Screen::Hunts => self.draw_hunts(c, ed),
            Screen::Guild => self.draw_guild(c, ed),
            Screen::SaveQuests => self.draw_save_quests(c, ed),
            Screen::QuestEquip => self.draw_quest_equip(c, ed),
            Screen::ArcList => self.draw_arc_list(c, ed),
            Screen::ArcEntries => self.draw_arc_entries(c, ed),
            Screen::Mib => self.draw_mib(c, ed),
            Screen::Gmd => self.draw_gmd(c, ed),
            Screen::EntryView => self.draw_entry_view(c, ed),
            Screen::Check => self.draw_check(c, ed),
        }
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
        _unused_sub: &str,
        rows: &[(usize, String, String, String, String)],
        active_row_idx: usize,
        has_stepper: bool,
        primary_a_label: &str,
    ) {
        self.draw_adaptive_rows_warn(
            c,
            ed,
            header_title,
            rows,
            active_row_idx,
            has_stepper,
            primary_a_label,
            &[],
        );
    }

    /// Same list/inspector layout, plus 1〜2 amber warning lines under the list.
    #[allow(clippy::too_many_arguments)]
    fn draw_adaptive_rows_warn(
        &mut self,
        c: &mut Canvas,
        ed: &Editor,
        header_title: &str,
        rows: &[(usize, String, String, String, String)],
        active_row_idx: usize,
        has_stepper: bool,
        primary_a_label: &str,
        warnings: &[String],
    ) {
        let t = theme();
        let total = rows.len();

        // Compact Section Title Line (y = 100..130) — No redundant explanation string!
        c.text_clipped(
            MAIN_X + 4,
            CONTENT_Y,
            620,
            header_title,
            t.text_primary,
            FontSize::Medium,
        );

        let cur_pos = rows
            .iter()
            .position(|r| r.0 == active_row_idx)
            .unwrap_or(0);

        if total > 0 {
            let pos_txt = format!("{} / {}", cur_pos + 1, total);
            c.text_right(
                MAIN_X + V1_LIST_W - 4,
                CONTENT_Y + 3,
                &pos_txt,
                t.text_secondary,
                FontSize::Small,
            );
        }

        let body_y = CONTENT_Y + 30; // 132
        let body_h = FOOTER_Y - body_y - 10; // 526

        if total == 0 {
            c.round_rect(MAIN_X, body_y, MAIN_W, 120, t.corner_r, t.bg_panel);
            c.round_rect_border(MAIN_X, body_y, MAIN_W, 120, t.corner_r, 1, t.border_subtle);
            c.text_centered(
                MAIN_X,
                body_y,
                MAIN_W,
                120,
                "データがありません",
                t.text_secondary,
                FontSize::Medium,
            );
            return;
        }

        // Left List Panel (x = 20, y = 132, w = 796, h = 526)
        c.round_rect(MAIN_X, body_y, V1_LIST_W, body_h, t.corner_r, t.bg_panel);
        c.round_rect_border(MAIN_X, body_y, V1_LIST_W, body_h, t.corner_r, 1, t.border_subtle);

        // A warning strip needs one or two lines under the list, so the list
        // gives up a row rather than drawing text over the last entry.
        let warn_lh = c.line_height_sized(FontSize::Small).max(16);
        let warn_reserve = if warnings.is_empty() {
            0
        } else {
            warnings.len().min(2) as i32 * warn_lh + 16
        };
        let row_h = 56i32;
        let rows_that_fit = ((body_h - 10 - warn_reserve).max(row_h)) / row_h;
        let vis = (rows_that_fit.max(1) as usize).min(9);
        let vis = if warnings.is_empty() { vis } else { vis.min(8) };
        let start = window_start(cur_pos, total, vis);

        for i in 0..vis {
            let Some((r_idx, title, _meta, val, badge)) = rows.get(start + i) else {
                break;
            };
            let row_idx = *r_idx;
            let ry = body_y + 10 + (i as i32) * row_h;
            let is_sel = row_idx == active_row_idx;

            if is_sel {
                c.round_rect(MAIN_X + 8, ry + 2, V1_LIST_W - 16, row_h - 4, 6, t.bg_selected);
                c.rect(MAIN_X + 8, ry + 8, 4, row_h - 16, t.accent_primary);
            } else if i + 1 < vis && start + i + 1 < total {
                c.hline(MAIN_X + 16, ry + row_h - 1, V1_LIST_W - 32, t.border_subtle);
            }

            self.add_row_hit(MAIN_X + 8, ry + 2, V1_LIST_W - 16, row_h - 4, row_idx);

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
            let right_edge = MAIN_X + V1_LIST_W - 22;
            let val_w = if val.is_empty() {
                0
            } else {
                let raw_vw = c.text_width_sized(val, FontSize::Medium);
                let max_vw = 230;
                let actual_vw = raw_vw.min(max_vw);
                let val_col = if is_sel { t.accent_primary } else { t.text_primary };
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
            };

            // 3. Left Title (strictly clipped to the space before `val`)
            let max_title_w = (right_edge - val_w - tx).max(60);
            c.text_clipped(
                tx,
                ry + 16,
                max_title_w,
                title,
                if is_sel { t.text_primary } else { t.text_secondary },
                FontSize::Medium,
            );
        }

        // Amber warning strip, pinned to the bottom of the list panel.
        if !warnings.is_empty() {
            let mut lines: Vec<String> = Vec::new();
            lines.push(warnings[0].clone());
            if warnings.len() == 2 {
                lines.push(warnings[1].clone());
            } else if warnings.len() > 2 {
                lines.push(format!("ほか {} 件 — 上の行を直してください", warnings.len() - 1));
            }
            let drawn = total.min(vis);
            let rows_bottom = body_y + 10 + drawn as i32 * row_h;
            let limit = body_y + body_h - 4;
            let mut strip_h = lines.len() as i32 * warn_lh;
            let mut top = (limit - strip_h).max(rows_bottom + 12);
            // If the strip would run past the panel (a tall font metric), drop
            // the second line instead of drawing over the footer.
            if top + strip_h > limit && lines.len() > 1 {
                lines.truncate(1);
                strip_h = warn_lh;
                top = (limit - strip_h).max(rows_bottom + 12);
            }
            if top - 8 > rows_bottom + 2 {
                c.hline(MAIN_X + 16, top - 8, V1_LIST_W - 32, t.border_subtle);
            }
            for (i, line) in lines.iter().enumerate() {
                c.text_clipped(
                    MAIN_X + 20,
                    top + i as i32 * warn_lh,
                    V1_LIST_W - 40,
                    line,
                    t.accent_warn,
                    FontSize::Small,
                );
            }
        }

        // Right Inspector Panel (x = 832, y = 132, w = 428, h = 526)
        if let Some(sel_row) = rows.get(cur_pos) {
            self.draw_studio_inspector(
                c,
                ed,
                body_y,
                body_h,
                sel_row,
                cur_pos,
                total,
                has_stepper,
                primary_a_label,
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
        row: &(usize, String, String, String, String),
        cur_pos: usize,
        total: usize,
        has_stepper: bool,
        primary_a_label: &str,
    ) {
        let t = theme();
        let ix = V1_INSP_X; // 832
        let iw = V1_INSP_W; // 428
        let (row_idx, ref title, ref meta, ref val, ref badge) = *row;

        c.round_rect(ix, iy, iw, ih, t.corner_r, t.bg_panel);
        c.round_rect_border(ix, iy, iw, ih, t.corner_r, 1, t.border_subtle);

        // Slot 1 (iy + 14): Badge on left, "#pos / total" on right
        let top_tag = if badge.is_empty() { "選択項目" } else { badge.as_str() };
        c.text_clipped(ix + 20, iy + 14, 240, top_tag, t.accent_primary, FontSize::Small);
        c.text_right(
            ix + iw - 20,
            iy + 14,
            &format!("#{} / {}", cur_pos + 1, total),
            t.text_muted,
            FontSize::Small,
        );

        if ed.screen == Screen::Gmd {
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
                "[A] テキスト編集",
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
                "[X] 変更を保存",
                UiAction::QuickSave,
            );
            return;
        }

        // Slot 2 (iy + 40): Selected Item Title
        c.text_clipped(ix + 20, iy + 40, iw - 40, title, t.text_primary, FontSize::Medium);

        // Slot 3 (iy + 70): Concise Meta Info (if non-empty)
        if !meta.is_empty() {
            c.text_clipped(ix + 20, iy + 70, iw - 40, meta, t.text_secondary, FontSize::Small);
        }

        c.hline(ix + 16, iy + 98, iw - 32, t.border_subtle);

        // Slot 4 (iy + 110, h = 48): Current Value Box
        let disp_val = if val.is_empty() { title.as_str() } else { val.as_str() };
        c.round_rect(ix + 20, iy + 110, iw - 40, 48, 6, t.bg_val_box);
        c.round_rect_border(ix + 20, iy + 110, iw - 40, 48, 6, 1, t.border_subtle);
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

        // Slot 5 (iy + 168, h = 42): Primary [A] Button
        let btn_y = iy + 168;
        let a_txt = format!("[A] {}", primary_a_label);
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

        let mut cur_y = btn_y + 52; // iy + 220

        // Slot 6: Numeric Stepper & Step Size Selector
        if has_stepper {
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

            // [MAX]
            let bx3 = bx2 + bw + 8;
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
            c.round_rect(bx3, cur_y, bw, 38, 5, t.bg_btn_primary);
            c.round_rect_border(bx3, cur_y, bw, 38, 5, 1, t.accent_primary);
            btn_text(c, bx3, cur_y, bw, 38, max_lbl, t.text_primary, FontSize::Small);
            self.add_hit(bx3, cur_y, bw, 38, UiAction::SetRowMax(row_idx));

            cur_y += 46; // iy + 266

            // Step size selector pills [Y]
            c.text_sized(ix + 20, cur_y + 6, "刻み [Y]", t.text_secondary, FontSize::Small);
            let mut sx = ix + 104;
            let sw = 72i32;
            for (s_idx, &step_val) in STEPS.iter().enumerate() {
                let is_s = ed.step == s_idx;
                let sbg = if is_s { t.bg_selected } else { t.bg_btn };
                let sbd = if is_s { t.accent_primary } else { t.border_subtle };
                let sfg = if is_s { t.accent_primary } else { t.text_secondary };
                c.round_rect(sx, cur_y, sw, 30, 4, sbg);
                c.round_rect_border(sx, cur_y, sw, 30, 4, 1, sbd);
                let slbl = format!("±{}", step_val);
                btn_text(c, sx, cur_y, sw, 30, &slbl, sfg, FontSize::Small);
                self.add_hit(sx, cur_y, sw, 30, UiAction::SetStep(s_idx));
                sx += sw + 6;
            }
            cur_y += 40; // iy + 306
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
                    "所持金・HR・全P 一括MAX",
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
                c.text_sized(ix + 20, cur_y, "一括操作・プリセット", t.text_muted, FontSize::Small);
                cur_y += 24;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "所持アイテム 全枠×99",
                    UiAction::ItemsMaxAll,
                );
            }
            Screen::Charms | Screen::CharmEdit => {
                // The slot count on these labels is whatever the current grade
                // allows, so a preset can no longer promise an S3 the game
                // refuses at rarity 97/98.
                let max_s = ed.charm_edit.map(|c| c.grade.max_slots()).unwrap_or(3);
                c.text_sized(ix + 20, cur_y, "一括操作・プリセット", t.text_muted, FontSize::Small);
                cur_y += 24;
                let legal_lbl = format!("合法最大値 + スロ{}をセット", max_s);
                self.draw_tool_button(c, ix + 20, cur_y, full_w, 36, &legal_lbl, UiAction::CharmLegalMax);
                cur_y += 42;
                let p0 = format!("痛撃6 達人10 S{}", max_s);
                let p1 = format!("斬れ味6 達人10 S{}", max_s);
                self.draw_tool_button(c, ix + 20, cur_y, half_w, 36, &p0, UiAction::CharmPreset(0));
                self.draw_tool_button(c, ix + 28 + half_w, cur_y, half_w, 36, &p1, UiAction::CharmPreset(1));
                cur_y += 42;
                let p2 = format!("溜め6 達人10 S{}", max_s);
                let p3 = format!("通常6 達人10 S{}", max_s);
                self.draw_tool_button(c, ix + 20, cur_y, half_w, 36, &p2, UiAction::CharmPreset(2));
                self.draw_tool_button(c, ix + 28 + half_w, cur_y, half_w, 36, &p3, UiAction::CharmPreset(3));
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
            }
            Screen::Palico => {
                c.text_sized(ix + 20, cur_y, "一括操作・プリセット", t.text_muted, FontSize::Small);
                cur_y += 24;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "配信オトモ 全28匹を一括導入",
                    UiAction::InstallAllPalicoes,
                );
            }
            Screen::SaveQuests | Screen::ArcList => {
                c.text_sized(ix + 20, cur_y, "一括操作・プリセット", t.text_muted, FontSize::Small);
                cur_y += 24;
                self.draw_tool_button(
                    c,
                    ix + 20,
                    cur_y,
                    full_w,
                    38,
                    "公式イベント189件 一括導入",
                    UiAction::InstallOfficialDlc,
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
                        "モンスター個別パラメータ ([-] またはタッチで選択)",
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
                self.draw_tool_button(c, ix + 20, cur_y, half_w, 34, "大型体力=1", UiAction::MibPreset(0));
                self.draw_tool_button(c, ix + 28 + half_w, cur_y, half_w, 34, "最大金冠 200%", UiAction::MibPreset(1));
                cur_y += 38;
                self.draw_tool_button(c, ix + 20, cur_y, half_w, 34, "最小金冠 50%", UiAction::MibPreset(2));
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
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        36,
                        "配置1のエリア・座標を他モンスターへコピー",
                        UiAction::SemPreset(0),
                    );
                    cur_y += 42;
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        36,
                        "選択中の座標を 0.0 にリセット",
                        UiAction::SemPreset(1),
                    );
                } else {
                    self.draw_tool_button(
                        c,
                        ix + 20,
                        cur_y,
                        full_w,
                        38,
                        "[X] 変更を保存",
                        UiAction::QuickSave,
                    );
                }
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
                    "[X] 変更を保存",
                    UiAction::QuickSave,
                );
            }
        }
    }

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
        c.text_sized(MAIN_X + 4, CONTENT_Y, "スロット選択", t.text_primary, FontSize::Medium);

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
            c.text_clipped(x + 20, y + 66, card_w - 40, &summary, t.text_primary, FontSize::Large);
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
            let btn_bg = if is_sel { t.accent_primary } else { t.bg_btn };
            let btn_fg = if is_sel {
                white_or_dark(t.accent_primary)
            } else {
                t.text_secondary
            };
            c.round_rect(x + 20, btn_y, card_w - 40, 42, 6, btn_bg);
            btn_text(c, x + 20, btn_y, card_w - 40, 42, "[A] 選択", btn_fg, FontSize::Medium);

            self.add_hit(x, y, card_w, card_h, UiAction::SelectSlot(s));
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
            (Some(s), p) => {
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
                let stem = if ed.convert_name.trim().is_empty() {
                    "system"
                } else {
                    ed.convert_name.trim()
                };
                let ext = if s.kind() == SaveKind::ThreeDs { "switch" } else { "3ds" };

                c.text_sized(MAIN_X + 28, by + 76, &format!("変換元 : {}", src_kind), t.text_primary, FontSize::Medium);
                c.text_sized(MAIN_X + 28, by + 114, &format!("変換先 : {}", dst_kind), t.accent_secondary, FontSize::Medium);
                c.text_clipped(
                    MAIN_X + 28,
                    by + 152,
                    MAIN_W - 56,
                    &format!("出力名 : {}.{}  ({})", stem, ext, p.as_deref().unwrap_or("-")),
                    t.text_secondary,
                    FontSize::Small,
                );

                let btn_y = by + 216;
                c.round_rect(MAIN_X + 28, btn_y, 200, 44, 6, t.bg_btn);
                c.round_rect_border(MAIN_X + 28, btn_y, 200, 44, 6, 1, t.border_subtle);
                btn_text(c, MAIN_X + 28, btn_y, 200, 44, "[Y] 出力名変更", t.text_primary, FontSize::Small);
                self.add_hit(MAIN_X + 28, btn_y, 200, 44, UiAction::Key(Key::Y));

                c.round_rect(MAIN_X + 244, btn_y, 220, 44, 6, t.accent_primary);
                btn_text(
                    c,
                    MAIN_X + 244,
                    btn_y,
                    220,
                    44,
                    "[A] 変換出力",
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
                format!("{}", o.brightness),
                String::from("#01"),
            ),
            (
                1,
                String::from("振動設定"),
                String::from("Rumble"),
                String::from(if o.rumble != 0 { "ON" } else { "OFF" }),
                String::from("#02"),
            ),
            (
                2,
                String::from("3D表示無効"),
                String::from("3DS Flag"),
                String::from(if o.disable_3d != 0 { "YES" } else { "NO" }),
                String::from("#03"),
            ),
            (
                3,
                String::from("拡張スライドパッド"),
                String::from("3DS Flag"),
                String::from(if o.circle_pad_pro != 0 { "ON" } else { "OFF" }),
                String::from("#04"),
            ),
        ];
        self.draw_adaptive_rows(
            c,
            ed,
            "環境設定",
            "",
            &rows,
            ed.option_cursor.min(3),
            true,
            "切替",
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
            let meta = match f {
                0 => "最大 12 Byte",
                1 => "上限 9,999,999",
                2 => "秒単位",
                3 => "上限 999",
                4 => "上限 99,999,999",
                5..=9 => "上限 9,999,999",
                13 => "0: 男性 / 1: 女性",
                _ => "外見パラメータ",
            };
            let badge = format!("#{:02}", f + 1);
            rows.push((f, label, meta.to_string(), val, badge));
        }

        let (hdr, a_lbl) = if is_appearance {
            (
                "外見・性別・ボイス",
                if ed.field == 13 { "性別切替" } else { "変更" },
            )
        } else {
            (
                "基本ステータス・所持金・ポイント",
                if ed.field == 0 { "名前変更" } else { "最大値" },
            )
        };

        self.draw_adaptive_rows(c, ed, hdr, "", &rows, ed.field, true, a_lbl);
    }

    fn draw_items(&mut self, c: &mut Canvas, ed: &Editor) {
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
                format!("×{}", cnt)
            };
            rows.push((
                list_idx,
                name,
                format!("BOX P{:02}-{:03} / ID:{:04}", page, pos, id),
                val,
                format!("#{:04}", box_slot + 1),
            ));
        }

        let hdr = if ed.show_empty {
            "アイテムBOX (全枠)"
        } else {
            "アイテムBOX (所持品)"
        };
        self.draw_adaptive_rows(
            c,
            ed,
            hdr,
            "",
            &rows,
            ed.cursor.min(rows.len().saturating_sub(1)),
            true,
            "アイテム変更",
        );
    }

    fn draw_item_pick(&mut self, c: &mut Canvas, ed: &Editor) {
        let matches = ed.item_pick_matches();
        let mut rows = Vec::with_capacity(matches.len());
        for (i, &id) in matches.iter().enumerate() {
            rows.push((
                i,
                item_name_of(id),
                format!("Item ID: {}", id),
                String::new(),
                format!("{:04}", id),
            ));
        }
        let hdr = if ed.item_pick_filter.is_empty() {
            String::from("アイテム選択")
        } else {
            format!("アイテム選択 (検索: {})", ed.item_pick_filter)
        };
        self.draw_adaptive_rows(
            c,
            ed,
            &hdr,
            "",
            &rows,
            ed.item_pick_cursor.min(rows.len().saturating_sub(1)),
            false,
            "決定",
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
        let hdr = if ed.skill_pick_filter.is_empty() {
            String::from("スキル選択")
        } else {
            format!("スキル選択 (検索: {})", ed.skill_pick_filter)
        };
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
            let slots_str = format!("S{}", chm.slots);
            rows.push((
                i,
                summary,
                format!("装備BOX枠 #{:04}", chm.index),
                slots_str,
                format!("#{:02}", i + 1),
            ));
        }
        self.draw_adaptive_rows(
            c,
            ed,
            "護石一覧",
            "",
            &rows,
            ed.cursor.min(rows.len().saturating_sub(1)),
            false,
            "編集",
        );
    }

    fn draw_charm_edit(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(ce) = ed.charm_edit else {
            return;
        };
        let s1 = mhnx_core::skills::skill_name(ce.skill1).unwrap_or("なし");
        let s2 = mhnx_core::skills::skill_name(ce.skill2).unwrap_or("なし");
        // The allowed range follows the grade now, so it is read from the
        // table instead of being printed as a fixed -10..+14 on every row.
        let range_of = |skill: u8, second: bool| -> alloc::string::String {
            if skill == 0 {
                return String::from("なし");
            }
            match mhnx_core::charm::allowed_points_rarity(ce.grade.rarity(), skill, second) {
                Some(r) if r.min == 0 && r.max == 0 => String::from("このレア度では不可"),
                Some(r) => format!("{} 〜 {}", r.min, r.max),
                None => String::from("—"),
            }
        };
        let rows = alloc::vec![
            (
                0,
                String::from("装備BOX枠"),
                String::from("対象スロット"),
                format!("#{:04}", ce.index),
                String::from("#01"),
            ),
            (
                1,
                String::from("レア度"),
                // The ceiling lives in the meta column so the value column
                // stays inside its 230px budget (the grade name alone already
                // reads 古びた護石 R99).
                format!("◀ ▶ で切替 / 最大 S{}", ce.grade.max_slots()),
                String::from(ce.grade.name()),
                String::from("#02"),
            ),
            (
                2,
                String::from("スロット数"),
                format!("0 〜 {}", ce.grade.max_slots()),
                format!("S{} ({})", ce.slots, "◯".repeat(ce.slots.min(3) as usize)),
                String::from("#03"),
            ),
            (
                3,
                String::from("第1スキル"),
                format!("Skill ID: {}", ce.skill1),
                s1.to_string(),
                String::from("#04"),
            ),
            (
                4,
                String::from("第1スキル値"),
                range_of(ce.skill1, false),
                format!("{:+}", ce.points1),
                String::from("#05"),
            ),
            (
                5,
                String::from("第2スキル"),
                format!("Skill ID: {}", ce.skill2),
                s2.to_string(),
                String::from("#06"),
            ),
            (
                6,
                String::from("第2スキル値"),
                range_of(ce.skill2, true),
                format!("{:+}", ce.points2),
                String::from("#07"),
            ),
        ];

        // Warnings for the record in the box and for what the form would write.
        let box_bytes = ed.save.as_ref().and_then(|s| s.equipment_box(ed.slot));
        let warnings = charm_warnings(
            box_bytes,
            ce.index,
            ce.grade,
            ce.slots,
            ce.skill1,
            ce.points1,
            ce.skill2,
            ce.points2,
        );

        // [A] always writes the form into the box (core: Key::A on any row),
        // so every row carries the same label; [Y] is the skill picker.
        let a_lbl = "BOXへ反映";

        self.draw_adaptive_rows_warn(
            c,
            ed,
            "護石編集",
            &rows,
            // The form has seven rows (0..=6); clamping to 5 left the last row
            // unhighlighted whenever the second skill's value was being edited.
            ce.field.min(6),
            true,
            a_lbl,
            &warnings,
        );
    }

    fn draw_equip(&mut self, c: &mut Canvas, ed: &Editor) {
        let all = ed.equipment();
        let mut rows = Vec::with_capacity(all.len());
        for (i, eq) in all.iter().enumerate() {
            let tname = mhnx_core::equip::type_name(eq.kind);
            let ename = mhnx_core::equip::name(eq).to_string();
            rows.push((
                i,
                ename,
                format!("枠 #{:04} / {}", eq.index, tname),
                format!("Lv{}", eq.level),
                tname.to_string(),
            ));
        }
        self.draw_adaptive_rows(
            c,
            ed,
            "装備BOX",
            "",
            &rows,
            ed.equip_cursor.min(rows.len().saturating_sub(1)),
            false,
            "選択",
        );
    }

    fn draw_sets(&mut self, c: &mut Canvas, ed: &Editor) {
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
                    let n = s.parts.iter().filter(|&&p| p != 0).count();
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
            "切替",
        );
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
                0 => format!("Lv {}", p.level),
                1 => format!("なつき {}", p.enthusiasm),
                _ => format!("EXP {}", p.exp),
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
        self.draw_adaptive_rows(
            c,
            ed,
            &hdr,
            "",
            &rows,
            ed.roster_cursor.min(rows.len().saturating_sub(1)),
            true,
            "名前変更",
        );
    }

    fn draw_palico(&mut self, c: &mut Canvas, ed: &Editor) {
        let ids = dlcats::all_ids();
        let lang = if ed.palico_lang == 0 { Lang::Jp } else { Lang::En };
        let installed_ids: Vec<u8> = ed
            .save
            .as_ref()
            .map(|s| s.dlc_palicoes().iter().map(|cat| cat.id).collect())
            .unwrap_or_default();
        let mut rows = Vec::with_capacity(ids.len());
        for (i, &id) in ids.iter().enumerate() {
            let name = dlcats::record(lang, id)
                .map(dlcats::record_name)
                .unwrap_or_else(|| format!("DLC #{}", id));
            let installed = installed_ids.contains(&id);
            let status = if installed { "導入済" } else { "未導入" };
            rows.push((
                i,
                name,
                format!("DLC ID: {}", id),
                status.to_string(),
                format!("#{:02}", i + 1),
            ));
        }
        self.draw_adaptive_rows(
            c,
            ed,
            "配信オトモ (全28匹)",
            "",
            &rows,
            ed.palico_cursor.min(rows.len().saturating_sub(1)),
            false,
            "導入/解除",
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
        let mut rows = Vec::with_capacity(hunt_list.len());
        for (i, &(id, slay, cap, lo, hi)) in hunt_list.iter().enumerate() {
            let mname = monster_name(id);
            let val = match ed.hunt_field {
                0 => format!("討伐 {}", slay),
                1 => format!("捕獲 {}", cap),
                2 => format!("最小 {}%", lo),
                _ => format!("最大 {}%", hi),
            };
            rows.push((
                i,
                mname.to_string(),
                format!("討伐:{} / 捕獲:{} / 最小:{}% / 最大:{}%", slay, cap, lo, hi),
                val,
                format!("#{:03}", id + 1),
            ));
        }
        let hdr = format!("狩猟記録 (編集列: {})", col_name);
        self.draw_adaptive_rows(
            c,
            ed,
            &hdr,
            "",
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
        let rows = alloc::vec![
            (
                0,
                String::from("ギルドカード HR"),
                String::from("Hunter Rank"),
                format!("HR {}", hr),
                String::from("#01"),
            ),
            (
                1,
                String::from("プレイ時間"),
                String::from("Play Time"),
                format!("{}h {:02}m", time / 3600, (time % 3600) / 60),
                String::from("#02"),
            ),
            (
                2,
                String::from("コメント"),
                String::from("Greeting"),
                comment,
                String::from("#03"),
            ),
        ];
        self.draw_adaptive_rows(
            c,
            ed,
            "ギルドカード",
            "",
            &rows,
            ed.guild_field.min(2),
            true,
            "編集",
        );
    }

    fn draw_save_quests(&mut self, c: &mut Canvas, ed: &Editor) {
        if self.quest_tab == 0 {
            let list = ed.filtered_quests();
            let mut rows = Vec::with_capacity(list.len());
            for (i, q) in list.iter().enumerate() {
                let title = ed.quest_title(q.quest_id);
                rows.push((
                    i,
                    if title.is_empty() {
                        format!("Quest {}", q.quest_id)
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
                "セーブ内クエスト",
                "",
                &rows,
                ed.save_quest_cursor.min(rows.len().saturating_sub(1)),
                false,
                "編集",
            );
        } else {
            let list = bundled_quests_for_tab(self.quest_tab, &ed.quest_filter);
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
            let hdr = if self.quest_tab == 1 {
                "公式イベントクエスト (189)"
            } else {
                "没クエスト (34)"
            };
            let cur = self.quest_cat_cursor[self.quest_tab].min(rows.len().saturating_sub(1));
            self.draw_adaptive_rows(
                c,
                ed,
                hdr,
                "",
                &rows,
                cur,
                false,
                "導入/解除",
            );
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
                (
                    format!("offset +0x{:03X} ({}B)", f.offset, f.size),
                    mib_extra_value_label(f.key, raw_val),
                )
            };

            rows.push((i, f.ja.to_string(), meta, val_str, format!("#{:02}", i + 1)));
        }

        let cur = ed.mib_cursor.min(total.saturating_sub(1));
        let a_lbl = if questfields::MIB_FIELDS
            .get(cur)
            .map(|f| mib_has_picker(f))
            .unwrap_or(false)
        {
            "一覧選択"
        } else {
            "変更"
        };

        self.draw_adaptive_rows(
            c,
            ed,
            "クエスト条件 (.mib)",
            "",
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
                    for (i, rec) in tbl.records.iter().enumerate() {
                        let iname = if rec.item == 0 {
                            String::from("(なし)")
                        } else {
                            item_name_of(rec.item)
                        };
                        let sub = if ext == "rem" {
                            format!("確率/重み: {}% / ID:{}", rec.slot, rec.item)
                        } else {
                            format!("Slot: {} / ID:{}", rec.slot, rec.item)
                        };
                        rows.push((
                            i,
                            iname,
                            sub,
                            format!("×{}", rec.count),
                            format!("#{:03}", i + 1),
                        ));
                    }
                }
            }
            "sem" => {
                let sems = sem_entry_indices(ed);
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
        self.draw_adaptive_rows(
            c,
            ed,
            hdr,
            "",
            &rows,
            ed.entry_row_cursor.min(rows.len().saturating_sub(1)),
            matches!(ext, "rem" | "sup" | "sem" | "esl" | "qdp"),
            "変更",
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
    fn draw_mib_picker(&mut self, c: &mut Canvas) {
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
        let visible = 10;
        let start = window_start(picker.cursor, total, visible);
        let row_h = 46;
        let list_x = mx + 24;
        let list_w = mw - 48;
        let list_y0 = my + 64;

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

            c.text_clipped(
                list_x + 18,
                ry + 11,
                list_w - 140,
                label,
                if selected { t.accent_primary } else { t.text_primary },
                FontSize::Medium,
            );

            if selected {
                let bx = list_x + list_w - 108;
                c.round_rect(bx, ry + 6, 94, 34, 5, t.accent_primary);
                btn_text(
                    c,
                    bx,
                    ry + 6,
                    94,
                    34,
                    "[A] 決定",
                    white_or_dark(t.accent_primary),
                    FontSize::Small,
                );
            }
            self.add_hit(list_x, ry, list_w, row_h, UiAction::MibPickerSelect(idx));
        }

        let foot_y = my + mh - 50;
        c.text_sized(
            mx + 24,
            foot_y + 10,
            &format!("{} / {}", picker.cursor + 1, total),
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

        c.text_clipped(
            mx + 24,
            my + 16,
            500,
            &format!("文字入力: {}", osk.header),
            t.text_primary,
            FontSize::Medium,
        );
        c.text_right(
            mx + mw - 24,
            my + 20,
            "[A] 入力  [B] 削除  [L/R] 文字種  [+] 確定",
            t.text_secondary,
            FontSize::Small,
        );

        let ib_x = mx + 24;
        let ib_y = my + 56;
        let ib_w = mw - 48;
        let ib_h = 50;
        c.round_rect(ib_x, ib_y, ib_w, ib_h, 6, t.bg_val_box);
        c.round_rect_border(ib_x, ib_y, ib_w, ib_h, 6, 1, t.border_focus);
        let disp_text = format!("{}|", osk.buffer);
        c.text_clipped(ib_x + 16, ib_y + 11, ib_w - 160, &disp_text, t.accent_primary, FontSize::Large);

        let cnt_str = format!("{} / {} B", osk.buffer.len(), osk.max_bytes);
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
        let actions: &[(&str, i32, Color, Color, UiAction)] = &[
            ("[B] 1字削除", 160, t.bg_btn, t.border_subtle, UiAction::OskBackspace),
            ("[X] クリア", 140, t.bg_danger, t.accent_danger, UiAction::OskClear),
            ("[Y] 濁点/小字", 176, t.bg_btn_primary, t.accent_primary, UiAction::OskModify),
            ("空白", 110, t.bg_btn, t.border_subtle, UiAction::OskSpace),
            ("[-] 取消", 160, t.bg_btn, t.border_subtle, UiAction::OskCancel),
            ("[+] 確定", 220, t.accent_ok, t.accent_ok, UiAction::OskConfirm),
        ];
        let mut bx = ib_x;
        for &(label, bw, bg, border, act) in actions {
            c.round_rect(bx, by, bw, bh, 6, bg);
            c.round_rect_border(bx, by, bw, bh, 6, 1, border);
            btn_text(c, bx, by, bw, bh, label, white_or_dark(bg), FontSize::Small);
            self.add_hit(bx, by, bw, bh, act);
            bx += bw + 12;
        }
    }
}

// ---------------------------------------------------------------------------
// Pure Helper Functions (Pagination, MIB Choices, OSK Character Tables)
// ---------------------------------------------------------------------------

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

fn bundled_quests_for_tab(
    tab: usize,
    filter: &str,
) -> Vec<&'static mhnx_core::dlc_bundle::OfficialQuestMeta> {
    use mhnx_core::dlc_bundle::{DlcQuestCategory, OFFICIAL_QUESTS};
    let needle = filter.trim().to_lowercase();
    let target_cat = match tab {
        2 => DlcQuestCategory::Dead,
        _ => DlcQuestCategory::Official,
    };
    OFFICIAL_QUESTS
        .iter()
        .filter(|q| q.category == target_cat)
        .filter(|q| {
            if needle.is_empty() {
                return true;
            }
            format!("{}", q.quest_id).contains(&needle) || q.title.to_lowercase().contains(&needle)
        })
        .collect()
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

fn mib_has_picker(f: &questfields::FieldDef) -> bool {
    if f.enum_name.is_some() {
        return true;
    }
    if f.key.starts_with("mob_slot[") {
        return true;
    }
    matches!(
        f.key,
        "com0"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "ko1"
            | "ko2"
            | "ko3"
            | "map_id"
            | "quest_level"
            | "icn1"
            | "icn2"
            | "icn3"
            | "icn4"
            | "icn5"
    )
}

fn mib_field_choices(mib: &[u8], field_idx: usize) -> Option<(String, Vec<(u32, String)>, u32)> {
    let f = questfields::MIB_FIELDS.get(field_idx)?;
    if f.key.starts_with("mob_slot[") {
        let cur = if f.offset + 2 <= mib.len() {
            u16::from_le_bytes([mib[f.offset], mib[f.offset + 1]]) as u32
        } else {
            0
        };
        let mut items = Vec::with_capacity(questfields::ENUM_MONS_.len());
        for (i, (&id_str, &mname)) in questfields::ENUM_MINT_
            .iter()
            .zip(questfields::ENUM_MONS_.iter())
            .enumerate()
        {
            if i == questfields::ENUM_MINT_.len() - 1 && id_str == "0" && i > 0 {
                continue;
            }
            if let Ok(raw_id) = id_str.parse::<u32>() {
                items.push((raw_id, format!("ID {:04}: {}", raw_id, mname)));
            }
        }
        return Some((format!("{} (モンスター選択)", f.ja), items, cur));
    }

    let cur = questfields::read_u32(mib, f.offset, f.kind).unwrap_or(0);
    match f.key {
        "com0" | "com1" | "com2" | "com3" | "com4" | "com5" | "com6" | "com7" | "com8" | "com9"
        | "ko1" | "ko2" | "ko3" => {
            let mut items = Vec::with_capacity(questfields::ENUM_MONS_.len());
            for (i, (&id_str, &mname)) in questfields::ENUM_MINT_
                .iter()
                .zip(questfields::ENUM_MONS_.iter())
                .enumerate()
            {
                if i == questfields::ENUM_MINT_.len() - 1 && id_str == "0" && i > 0 {
                    continue;
                }
                if let Ok(raw_id) = id_str.parse::<u32>() {
                    if f.size == 1 && raw_id > 255 {
                        continue;
                    }
                    items.push((raw_id, format!("ID {:04}: {}", raw_id, mname)));
                }
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

fn apply_mib_choice(ed: &mut Editor, field_idx: usize, raw_val: u32, label: &str) {
    let Some(f) = questfields::MIB_FIELDS.get(field_idx) else {
        return;
    };
    if f.key.starts_with("mob_slot[") {
        if f.offset + 13 <= ed.mib_bytes.len() {
            let mid = raw_val as u16;
            ed.mib_bytes[f.offset..f.offset + 2].copy_from_slice(&mid.to_le_bytes());
            if mid > 0 && ed.mib_bytes[f.offset + 4] == 0 {
                ed.mib_bytes[f.offset + 4] = 1;
                ed.mib_bytes[f.offset + 5] = 50;
                ed.mib_bytes[f.offset + 6] = 50;
                ed.mib_bytes[f.offset + 7] = 50;
                ed.mib_bytes[f.offset + 9..f.offset + 11].copy_from_slice(&100u16.to_le_bytes());
                ed.mib_bytes[f.offset + 12] = 1;
            } else if mid == 0 {
                ed.mib_bytes[f.offset..f.offset + 13].fill(0);
            }
            ed.dirty = true;
            ed.message = format!("{}: {}", f.ja, label);
        }
        return;
    }
    if questfields::write_u32(&mut ed.mib_bytes, f.offset, f.kind, raw_val) {
        ed.dirty = true;
        ed.message = format!("{}: {}", f.ja, label);
    }
}

fn mib_extra_value_label(key: &str, v: u32) -> String {
    match key {
        "com0" | "com1" | "com2" | "com3" | "com4" | "com5" | "com6" | "com7" | "com8" | "com9"
        | "ko1" | "ko2" | "ko3" => {
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

fn osk_press_cell(osk: &mut OskState, row: usize, col: usize) {
    let grid = osk_grid_for(osk.tab);
    let Some(&cell) = grid.get(row).and_then(|r| r.get(col)) else {
        return;
    };
    match cell {
        "゛濁" => osk_apply_dakuten(&mut osk.buffer),
        "゜半" => osk_apply_handakuten(&mut osk.buffer),
        "小ぁ" | "小ァ" | "a↔A" => osk_apply_small_or_case(&mut osk.buffer),
        text => {
            if osk.buffer.len() + text.len() <= osk.max_bytes {
                osk.buffer.push_str(text);
            }
        }
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
