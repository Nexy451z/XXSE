//! Rich 2D Graphical User Interface and Touch/Controller engine for Switch (.nro).
//!
//! Renders `mhnx_core::hbedit::Editor` onto `crate::text::Canvas` (1280x720)
//! with a 2-pane Horizon-OS / JKSV-inspired layout:
//!   - Top Header Bar (Title, Character Slot Tabs, Save Dirty Indicator, Quick Save)
//!   - Left Navigation Sidebar (One-tap screen switching + ZL tab cycling)
//!   - Context Toolbar (Interactive Step Selector `±1..±10000`, Search/Filter Bar, Quick Actions)
//!   - Tailored Screen Views (Cards, Value Steppers `[-] val [+]`, `[MAX]`, Japanese Names, Notes)
//!   - Proportional Scrollbar + Page Up/Down Touch Controls
//!   - Interactive Footer Action Chips

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use mhnx_core::arc::extension_for;
use mhnx_core::check::Severity;
use mhnx_core::charm;
use mhnx_core::dlcats::{self, Lang};
use mhnx_core::entries::{Esl, ItemTable, Qdp, QuestLink, SemEntry};
use mhnx_core::hbedit::{
    item_name_of, monster_name, Editor, Key, Screen, CHAR_FIELDS, CONSOLE_SAVE, STEPS,
};
use mhnx_core::quest::Gmd;
use mhnx_core::questfields::{self, FieldKind};
use mhnx_core::save::{Options, SaveKind};

use crate::text::{rgb, Canvas, Color, FontSize};

// ---------------------------------------------------------------------------
// Theme Palette
// ---------------------------------------------------------------------------

const BG_MAIN: Color = rgb(14, 17, 24);
const BG_HEADER_TOP: Color = rgb(26, 33, 48);
const BG_HEADER_BOT: Color = rgb(18, 23, 34);
const BG_SIDEBAR: Color = rgb(18, 22, 31);
const BG_TOOLBAR: Color = rgb(22, 27, 38);
const BG_CARD: Color = rgb(25, 31, 44);
const BG_CARD_ALT: Color = rgb(29, 36, 50);
const BG_SELECTED: Color = rgb(24, 58, 92);
const BG_VAL_BOX: Color = rgb(15, 19, 27);
const BG_VAL_ACTIVE: Color = rgb(20, 44, 70);
const BG_BTN: Color = rgb(38, 48, 66);
const BG_BTN_PRIMARY: Color = rgb(24, 102, 156);
const BG_BTN_WARN: Color = rgb(130, 82, 20);
const BG_FOOTER: Color = rgb(19, 24, 34);

const BORDER_SUBTLE: Color = rgb(42, 52, 72);
const BORDER_CARD: Color = rgb(52, 64, 88);
const BORDER_BTN: Color = rgb(72, 88, 118);
const ACCENT_CYAN: Color = rgb(0, 214, 236);
const ACCENT_GOLD: Color = rgb(245, 196, 64);
const ACCENT_GREEN: Color = rgb(46, 198, 126);
const ACCENT_ORANGE: Color = rgb(245, 158, 42);
const ACCENT_RED: Color = rgb(236, 84, 84);
const ACCENT_PURPLE: Color = rgb(168, 122, 245);

const TEXT_PRIMARY: Color = rgb(242, 245, 250);
const TEXT_SECONDARY: Color = rgb(176, 188, 208);
const TEXT_MUTED: Color = rgb(118, 132, 156);
const TEXT_CYAN: Color = rgb(88, 228, 248);
const TEXT_GOLD: Color = rgb(255, 214, 92);

// Layout constants (1280 x 720)
const HEADER_H: i32 = 54;
const FOOTER_Y: i32 = 644;
const FOOTER_H: i32 = 76;
const SIDEBAR_W: i32 = 236;
const MAIN_X: i32 = SIDEBAR_W + 12;
const MAIN_W: i32 = 1280 - MAIN_X - 12;
const TOOLBAR_Y: i32 = HEADER_H + 8;
const TOOLBAR_H: i32 = 42;
const CONTENT_Y: i32 = TOOLBAR_Y + TOOLBAR_H + 8;
const CONTENT_H: i32 = FOOTER_Y - CONTENT_Y - 8;

const HUNT_STEPS: [i64; 5] = [1, 10, 100, 1000, -1];

// ---------------------------------------------------------------------------
// Interactive HitBoxes & Touch Actions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub enum UiAction {
    /// Send a virtual button press to Editor::input().
    Key(Key),
    /// Switch active character slot (0..2).
    SelectSlot(usize),
    /// Navigate directly to a screen from the sidebar or quick card buttons.
    GotoScreen(Screen),
    /// Set `ed.step` index (0..5).
    SetStep(usize),
    /// Select a row in the current screen's list. ` immediate == false ` means list body tap.
    SelectRow(usize),
    /// Select a row and nudge its value with Left or Right.
    AdjustRow(usize, Key),
    /// Set the row's value to its natural maximum (or toggle).
    SetRowMax(usize),
    /// On `Items`, select row `idx` and open `ItemPick` (`Key::R`).
    OpenItemPicker(usize),
    /// On `Hunts` / `Roster` / `Guild`, select `(row, sub_field)`.
    SelectSubField(usize, usize),
    /// On `CharmEdit`, open skill picker for field `2` or `4`.
    OpenCharmSkill(usize),
    /// Scroll the active cursor by `delta` rows (`-10` / `+10`).
    PageScroll(i32),
    /// Trigger SD card rescan.
    Rescan,
    /// Save changes immediately (Save or Arc).
    QuickSave,
    /// Built-in On-Screen Keyboard actions (used when swkbd is unavailable in Applet mode).
    OskTab(usize),
    OskCell(usize, usize),
    OskBackspace,
    OskClear,
    OskModify,
    OskSpace,
    OskConfirm,
    OskCancel,
}

/// Built-in On-Screen Keyboard state for Applet mode (where Horizon OS blocks library applets).
#[derive(Clone)]
pub struct OskState {
    pub req_idx: usize,
    pub header: String,
    pub buffer: String,
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

/// Persistent GUI state (touch tracking + hitboxes from the last rendered frame).
pub struct GuiState {
    hits: Vec<HitBox>,
    was_touching: bool,
    drag_start_y: i32,
    last_drag_y: i32,
    dragged: bool,
    pending_activate_row: Option<usize>,
    pub wants_rescan: bool,
    pub osk: Option<OskState>,
    pub osk_result: Option<(usize, String)>,
    pub osk_cancelled: bool,
}

impl GuiState {
    pub fn new() -> Self {
        Self {
            hits: Vec::with_capacity(96),
            was_touching: false,
            drag_start_y: 0,
            last_drag_y: 0,
            dragged: false,
            pending_activate_row: None,
            wants_rescan: false,
            osk: None,
            osk_result: None,
            osk_cancelled: false,
        }
    }

    /// Open the built-in On-Screen Keyboard (fallback for Applet mode).
    pub fn open_osk(&mut self, req_idx: usize, header: &str, initial: &str) {
        self.osk = Some(OskState {
            req_idx,
            header: String::from(header),
            buffer: String::from(initial),
            tab: 0,
            row: 0,
            col: 0,
        });
    }

    /// Handle controller key press while the built-in OSK is open.
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

    /// Cycle through main sidebar tabs when `ZL` is pressed.
    pub fn cycle_sidebar_tab(&mut self, ed: &mut Editor, forward: bool) {
        let tabs: &[Screen] = if ed.save.is_some() {
            &[
                Screen::Slots,
                Screen::Char,
                Screen::Items,
                Screen::Equip,
                Screen::Charms,
                Screen::Roster,
                Screen::Palico,
                Screen::Hunts,
                Screen::Guild,
                Screen::Sets,
                Screen::SaveQuests,
                Screen::ArcList,
                Screen::Options,
            ]
        } else {
            &[Screen::SaveList, Screen::ArcList]
        };
        let cur_group = canonical_tab(ed.screen);
        let pos = tabs.iter().position(|&s| s == cur_group).unwrap_or(0);
        let next = if forward {
            (pos + 1) % tabs.len()
        } else {
            (pos + tabs.len() - 1) % tabs.len()
        };
        self.apply_action(ed, UiAction::GotoScreen(tabs[next]));
    }

    /// Process touch screen state for one frame. Returns `true` if `ed` changed and needs redraw.
    pub fn handle_touch(&mut self, ed: &mut Editor, touching: bool, tx: i32, ty: i32) -> bool {
        let mut changed = false;

        if touching && !self.was_touching {
            // Touch Down edge
            self.drag_start_y = ty;
            self.last_drag_y = ty;
            self.dragged = false;
            self.pending_activate_row = None;

            // Search in reverse order so buttons drawn on top of rows win over the row body!
            let hit = self.hits.iter().rev().find(|h| h.contains(tx, ty)).copied();
            if let Some(h) = hit {
                if h.is_row_body {
                    if let UiAction::SelectRow(idx) = h.action {
                        if active_cursor_of(ed) == idx {
                            // Already selected: activate on release if the user doesn't drag
                            self.pending_activate_row = Some(idx);
                        } else {
                            set_active_cursor(ed, idx);
                            changed = true;
                        }
                    }
                } else {
                    self.apply_action(ed, h.action);
                    changed = true;
                }
            }
        } else if touching && self.was_touching {
            // Dragging inside the main content area scrolls lists smoothly
            if tx >= MAIN_X && ty >= CONTENT_Y && ty < FOOTER_Y {
                let dy = ty - self.last_drag_y;
                if (ty - self.drag_start_y).abs() > 18 {
                    self.dragged = true;
                    self.pending_activate_row = None;
                }
                let step_px = 36;
                if dy.abs() >= step_px {
                    let rows = -dy / step_px;
                    self.last_drag_y += -rows * step_px;
                    scroll_active_cursor(ed, rows);
                    changed = true;
                }
            }
        } else if !touching && self.was_touching {
            // Touch Up edge
            if !self.dragged {
                if let Some(idx) = self.pending_activate_row.take() {
                    activate_selected_row(ed, idx);
                    changed = true;
                }
            }
            self.pending_activate_row = None;
        }

        self.was_touching = touching;
        changed
    }

    fn apply_action(&mut self, ed: &mut Editor, action: UiAction) {
        match action {
            UiAction::Key(k) => ed.input(k),
            UiAction::SelectSlot(s) => {
                if s < 3 {
                    ed.slot = s;
                    if ed.screen == Screen::Slots {
                        ed.cursor = s;
                    }
                    ed.message = format!("スロット {} に切り替えました", s + 1);
                }
            }
            UiAction::GotoScreen(target) => {
                if target != Screen::SaveList && target != Screen::ArcList && ed.save.is_none() {
                    ed.message = String::from("先にセーブデータを開いてください");
                    return;
                }
                match target {
                    Screen::SaveList => {
                        ed.screen = Screen::SaveList;
                        ed.cursor = 0;
                    }
                    Screen::Slots => {
                        ed.screen = Screen::Slots;
                        ed.cursor = ed.slot;
                    }
                    Screen::Char => {
                        ed.screen = Screen::Char;
                        ed.cursor = ed.field;
                    }
                    Screen::Items => {
                        ed.screen = Screen::Items;
                        ed.cursor = 0;
                    }
                    Screen::Equip => {
                        ed.screen = Screen::Equip;
                        ed.equip_cursor = 0;
                    }
                    Screen::Charms => {
                        ed.open_charms();
                    }
                    Screen::Roster => {
                        ed.screen = Screen::Roster;
                        ed.roster_cursor = 0;
                    }
                    Screen::Palico => {
                        ed.screen = Screen::Palico;
                        ed.cursor = ed.palico_cursor;
                    }
                    Screen::Hunts => {
                        ed.screen = Screen::Hunts;
                        ed.hunt_cursor = 0;
                    }
                    Screen::Guild => {
                        ed.screen = Screen::Guild;
                    }
                    Screen::Sets => {
                        ed.screen = Screen::Sets;
                    }
                    Screen::SaveQuests => {
                        ed.refresh_quest_titles();
                        ed.screen = Screen::SaveQuests;
                        let n = ed.filtered_quests().len();
                        ed.save_quest_cursor = ed.save_quest_cursor.min(n.saturating_sub(1));
                        ed.cursor = ed.save_quest_cursor;
                    }
                    Screen::QuestEquip => {
                        ed.screen = Screen::QuestEquip;
                        ed.qe_open = false;
                        ed.cursor = 0;
                    }
                    Screen::ArcList => {
                        ed.screen = Screen::ArcList;
                        ed.cursor = 0;
                    }
                    Screen::Options => {
                        ed.screen = Screen::Options;
                        ed.option_cursor = 0;
                    }
                    Screen::Convert => {
                        ed.screen = Screen::Convert;
                    }
                    other => {
                        ed.screen = other;
                    }
                }
            }
            UiAction::SetStep(s) => {
                if s < STEPS.len() {
                    ed.step = s;
                    ed.message = format!("変更の刻み幅を ±{} に設定しました", STEPS[s]);
                }
            }
            UiAction::SelectRow(idx) => {
                set_active_cursor(ed, idx);
            }
            UiAction::AdjustRow(idx, key) => {
                set_active_cursor(ed, idx);
                ed.input(key);
            }
            UiAction::SetRowMax(idx) => {
                set_active_cursor(ed, idx);
                apply_row_max(ed, idx);
            }
            UiAction::OpenItemPicker(idx) => {
                set_active_cursor(ed, idx);
                ed.input(Key::R);
            }
            UiAction::SelectSubField(row_idx, sub) => {
                set_active_cursor(ed, row_idx);
                match ed.screen {
                    Screen::Hunts => ed.hunt_field = sub.min(3),
                    Screen::Roster => ed.roster_field = sub.min(2),
                    Screen::Guild => {
                        ed.guild_field = sub.min(2);
                        if sub == 2 {
                            ed.input(Key::A);
                        }
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
            UiAction::PageScroll(delta) => {
                scroll_active_cursor(ed, delta);
            }
            UiAction::Rescan => {
                self.wants_rescan = true;
            }
            UiAction::QuickSave => {
                trigger_quick_save(ed);
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
                    osk.buffer.push(' ');
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

fn canonical_tab(screen: Screen) -> Screen {
    match screen {
        Screen::ItemPick => match Screen::Items {
            s => s,
        },
        Screen::CharmEdit | Screen::SkillPick => Screen::Charms,
        Screen::QuestEquip => Screen::SaveQuests,
        Screen::ArcEntries | Screen::EntryView | Screen::Mib | Screen::Gmd | Screen::Check => {
            Screen::ArcList
        }
        Screen::Convert => Screen::Options,
        other => other,
    }
}

fn active_cursor_of(ed: &Editor) -> usize {
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
        Screen::SaveQuests => ed.save_quest_cursor,
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

fn list_len_of(ed: &Editor) -> usize {
    match ed.screen {
        Screen::SaveList => ed.save_files.len(),
        Screen::Slots => 3,
        Screen::Char => CHAR_FIELDS.len(),
        Screen::Items => ed.visible_items().len(),
        Screen::ItemPick => ed.item_pick_matches().len(),
        Screen::Charms => ed.charms().len(),
        Screen::CharmEdit => 6,
        Screen::SkillPick => ed.skill_pick_matches().len(),
        Screen::Equip => ed.equipment().len(),
        Screen::Hunts => ed.hunt_rows().len(),
        Screen::Guild => 3,
        Screen::Roster => ed.palicos().len(),
        Screen::SaveQuests => ed.filtered_quests().len(),
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
        Screen::Sets | Screen::Convert => 0,
    }
}

fn set_active_cursor(ed: &mut Editor, idx: usize) {
    let len = list_len_of(ed);
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
    let len = list_len_of(ed);
    if len == 0 {
        return;
    }
    let cur = active_cursor_of(ed) as i32;
    let next = (cur + delta).clamp(0, len as i32 - 1) as usize;
    set_active_cursor(ed, next);
}

fn activate_selected_row(ed: &mut Editor, idx: usize) {
    set_active_cursor(ed, idx);
    match ed.screen {
        Screen::SaveList
        | Screen::Slots
        | Screen::ItemPick
        | Screen::SkillPick
        | Screen::SaveQuests
        | Screen::QuestEquip
        | Screen::ArcList
        | Screen::ArcEntries
        | Screen::EntryView
        | Screen::Gmd
        | Screen::Check
        | Screen::Palico
        | Screen::Roster => {
            ed.input(Key::A);
        }
        Screen::Char => {
            if idx == 0 {
                // Name row -> open software keyboard
                ed.input(Key::Right);
            }
        }
        Screen::Items => {
            // Tapping selected item row opens item picker
            ed.input(Key::R);
        }
        Screen::Charms => {
            // Tapping selected charm row edits that charm
            ed.input(Key::Y);
        }
        Screen::CharmEdit => {
            if idx == 2 || idx == 4 {
                ed.input(Key::Y);
            }
        }
        _ => {}
    }
}

fn apply_row_max(ed: &mut Editor, idx: usize) {
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
                    items[box_idx].count = 99;
                    if let Some(s) = &mut ed.save {
                        let _ = s.write_item_box(ed.slot, &items);
                        ed.dirty = true;
                        ed.message = format!("枠 #{} の個数を 99 に設定しました", box_idx + 1);
                    }
                }
            }
        }
        _ => {}
    }
}

fn trigger_quick_save(ed: &mut Editor) {
    match ed.screen {
        Screen::Char | Screen::Items | Screen::Options => ed.input(Key::A),
        Screen::Charms | Screen::Palico => ed.input(Key::X),
        Screen::CharmEdit => {
            // Apply charm edit then write save
            ed.input(Key::A);
            ed.input(Key::X);
        }
        Screen::Mib => ed.input(Key::A),
        Screen::Gmd | Screen::EntryView | Screen::Check | Screen::ArcEntries => ed.input(Key::X),
        _ => {
            // Switch to Char temporarily to invoke request_write_save, then restore screen
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
// Main Drawing Entry Point
// ---------------------------------------------------------------------------

impl GuiState {
    pub fn draw(&mut self, c: &mut Canvas, ed: &Editor) {
        self.hits.clear();

        c.clear(BG_MAIN);
        self.draw_header(c, ed);
        self.draw_sidebar(c, ed);
        self.draw_toolbar(c, ed);
        self.draw_content(c, ed);
        self.draw_footer(c, ed);

        if self.osk.is_some() {
            // Replace background hitboxes so touches only interact with the keyboard modal
            self.hits.clear();
            self.draw_osk(c);
        }

        c.present();
    }

    // -----------------------------------------------------------------------
    // 1. Top Header Bar
    // -----------------------------------------------------------------------
    fn draw_header(&mut self, c: &mut Canvas, ed: &Editor) {
        let w = c.width();
        c.rect_gradient_v(0, 0, w, HEADER_H, BG_HEADER_TOP, BG_HEADER_BOT);
        c.hline(0, HEADER_H - 1, w, BORDER_CARD);

        // App Brand Pill
        c.round_rect(12, 10, 146, 34, 8, rgb(28, 66, 108));
        c.round_rect_border(12, 10, 146, 34, 8, 1, ACCENT_CYAN);
        c.text_centered(12, 10, 146, 34, "MHXX EDITOR", TEXT_CYAN, FontSize::Small);

        // Screen Title
        let title_str = crate::editor::title(ed.screen);
        c.text_sized(172, 12, title_str, TEXT_PRIMARY, FontSize::Large);

        // Character Slot Switcher Tabs (when a save is loaded)
        if let Some(save) = &ed.save {
            let mut sx = 436;
            for i in 0..3 {
                let active = ed.slot == i;
                let used = save.slot_used(i);
                let cname = save
                    .read_character(i)
                    .map(|ch| ch.name)
                    .unwrap_or_default();
                let label = if !used && cname.is_empty() {
                    format!("枠{}: 空き", i + 1)
                } else if cname.is_empty() {
                    format!("枠{}: ---", i + 1)
                } else {
                    format!("枠{}: {}", i + 1, cname)
                };
                let pw = 132;
                let bg = if active {
                    BG_SELECTED
                } else {
                    rgb(22, 28, 40)
                };
                let border = if active { ACCENT_CYAN } else { BORDER_SUBTLE };
                let fg = if active {
                    TEXT_CYAN
                } else if used {
                    TEXT_SECONDARY
                } else {
                    TEXT_MUTED
                };
                c.round_rect(sx, 10, pw, 34, 7, bg);
                c.round_rect_border(sx, 10, pw, 34, 7, if active { 2 } else { 1 }, border);
                c.text_clipped(sx + 10, 16, pw - 16, &label, fg, FontSize::Small);
                self.add_hit(sx, 10, pw, 34, UiAction::SelectSlot(i));
                sx += pw + 6;
            }
        }

        // Dirty / Save Status Pill
        let status_x = 856;
        let status_w = 228;
        if ed.dirty {
            c.round_rect(status_x, 10, status_w, 34, 8, BG_BTN_WARN);
            c.round_rect_border(status_x, 10, status_w, 34, 8, 2, ACCENT_ORANGE);
            c.circle(status_x + 16, 27, 5, ACCENT_GOLD);
            c.text_sized(
                status_x + 28,
                16,
                "未保存 (タップで保存)",
                TEXT_GOLD,
                FontSize::Small,
            );
            self.add_hit(status_x, 10, status_w, 34, UiAction::QuickSave);
        } else if ed.save.is_some() {
            c.round_rect(status_x, 10, status_w, 34, 8, rgb(18, 46, 36));
            c.round_rect_border(status_x, 10, status_w, 34, 8, 1, ACCENT_GREEN);
            c.circle(status_x + 16, 27, 5, ACCENT_GREEN);
            let src_label = if ed.save_path.as_deref() == Some(CONSOLE_SAVE) {
                "本体セーブ (保存済)"
            } else {
                "SDセーブ (保存済)"
            };
            c.text_sized(status_x + 28, 16, src_label, ACCENT_GREEN, FontSize::Small);
        }

        // Rescan & Quit quick buttons
        let rescan_x = 1094;
        c.round_rect(rescan_x, 10, 86, 34, 7, BG_BTN);
        c.round_rect_border(rescan_x, 10, 86, 34, 7, 1, BORDER_BTN);
        c.text_centered(rescan_x, 10, 86, 34, "[-] 再読込", TEXT_SECONDARY, FontSize::Small);
        self.add_hit(rescan_x, 10, 86, 34, UiAction::Rescan);

        let quit_x = 1188;
        c.round_rect(quit_x, 10, 80, 34, 7, rgb(68, 28, 34));
        c.round_rect_border(quit_x, 10, 80, 34, 7, 1, ACCENT_RED);
        c.text_centered(quit_x, 10, 80, 34, "[+] 終了", rgb(255, 180, 180), FontSize::Small);
        self.add_hit(quit_x, 10, 80, 34, UiAction::Key(Key::Plus));
    }

    // -----------------------------------------------------------------------
    // 2. Left Sidebar Navigation
    // -----------------------------------------------------------------------
    fn draw_sidebar(&mut self, c: &mut Canvas, ed: &Editor) {
        let sy = HEADER_H;
        let sh = FOOTER_Y - HEADER_H;
        c.rect(0, sy, SIDEBAR_W, sh, BG_SIDEBAR);
        c.vline(SIDEBAR_W - 1, sy, sh, BORDER_SUBTLE);

        // Sidebar Header Hint (ZL cycles tabs)
        c.round_rect(10, sy + 8, SIDEBAR_W - 20, 28, 6, rgb(26, 34, 48));
        c.text_centered(
            10,
            sy + 8,
            SIDEBAR_W - 20,
            28,
            "[ZL] / タップで画面切替",
            TEXT_CYAN,
            FontSize::Small,
        );

        let nav_items: &[(Screen, &str, Color)] = &[
            (Screen::SaveList, "セーブデータ選択", ACCENT_CYAN),
            (Screen::Slots, "キャラクター枠選択", ACCENT_CYAN),
            (Screen::Char, "ステータス・外見", ACCENT_GOLD),
            (Screen::Items, "アイテムボックス", ACCENT_GREEN),
            (Screen::Equip, "装備ボックス一覧", ACCENT_GOLD),
            (Screen::Charms, "お守り (護石) 編集", ACCENT_PURPLE),
            (Screen::Roster, "オトモ一覧・育成", ACCENT_ORANGE),
            (Screen::Palico, "配信オトモ導入", ACCENT_ORANGE),
            (Screen::Hunts, "狩猟数・サイズ記録", ACCENT_RED),
            (Screen::Guild, "ギルドカード・闘技", ACCENT_CYAN),
            (Screen::Sets, "マイセット一覧", TEXT_SECONDARY),
            (Screen::SaveQuests, "セーブ内DLクエスト", ACCENT_PURPLE),
            (Screen::ArcList, "SDクエスト (.arc)", ACCENT_PURPLE),
            (Screen::Options, "環境設定・3DS変換", TEXT_SECONDARY),
        ];

        let cur_tab = canonical_tab(ed.screen);
        let has_save = ed.save.is_some();
        let mut iy = sy + 42;
        let item_h = 37;

        for &(scr, label, tag_col) in nav_items {
            let active = cur_tab == scr;
            let enabled = has_save || scr == Screen::SaveList || scr == Screen::ArcList;
            let bg = if active {
                BG_SELECTED
            } else if enabled {
                rgb(23, 29, 41)
            } else {
                rgb(17, 21, 30)
            };
            c.round_rect(8, iy, SIDEBAR_W - 16, item_h, 6, bg);
            if active {
                c.round_rect_border(8, iy, SIDEBAR_W - 16, item_h, 6, 2, ACCENT_CYAN);
                c.round_rect(10, iy + 5, 5, item_h - 10, 2, ACCENT_CYAN);
            } else {
                c.round_rect(12, iy + 11, 4, item_h - 22, 2, if enabled { tag_col } else { BORDER_SUBTLE });
            }
            let fg = if active {
                TEXT_PRIMARY
            } else if enabled {
                TEXT_SECONDARY
            } else {
                TEXT_MUTED
            };
            c.text_sized(24, iy + 8, label, fg, FontSize::Small);
            self.add_hit(8, iy, SIDEBAR_W - 16, item_h, UiAction::GotoScreen(scr));
            iy += item_h + 2;
        }
    }

    // -----------------------------------------------------------------------
    // 3. Context Toolbar (Step Selector, Search Bar, Screen Toggles)
    // -----------------------------------------------------------------------
    fn draw_toolbar(&mut self, c: &mut Canvas, ed: &Editor) {
        c.round_rect(MAIN_X, TOOLBAR_Y, MAIN_W, TOOLBAR_H, 8, BG_TOOLBAR);
        c.round_rect_border(MAIN_X, TOOLBAR_Y, MAIN_W, TOOLBAR_H, 8, 1, BORDER_SUBTLE);

        let uses_step = matches!(
            ed.screen,
            Screen::Char
                | Screen::Items
                | Screen::CharmEdit
                | Screen::Mib
                | Screen::Hunts
                | Screen::Guild
        );

        let mut x = MAIN_X + 12;
        let y = TOOLBAR_Y + 6;
        let h = TOOLBAR_H - 12;

        if uses_step {
            c.text_sized(x, y + 5, "刻み幅 [X]:", TEXT_SECONDARY, FontSize::Small);
            x += 92;
            for (idx, &st) in STEPS.iter().enumerate() {
                let disp = if ed.screen == Screen::Hunts {
                    HUNT_STEPS[idx]
                } else {
                    st
                };
                let label = if disp < 0 {
                    format!("{}", disp)
                } else {
                    format!("±{}", disp)
                };
                let pw = (label.chars().count() as i32) * 10 + 22;
                let active = ed.step == idx;
                let bg = if active { BG_BTN_PRIMARY } else { BG_BTN };
                let border = if active { ACCENT_CYAN } else { BORDER_BTN };
                let fg = if active { TEXT_PRIMARY } else { TEXT_SECONDARY };
                c.round_rect(x, y, pw, h, 6, bg);
                c.round_rect_border(x, y, pw, h, 6, if active { 2 } else { 1 }, border);
                c.text_centered(x, y, pw, h, &label, fg, FontSize::Small);
                self.add_hit(x, y, pw, h, UiAction::SetStep(idx));
                x += pw + 6;
            }
        }

        // Screen-specific toolbar buttons on the right side
        match ed.screen {
            Screen::Items => {
                // Toggle show empty (Y), Undo (L), Change Item (R), Save (A)
                let emp_label = if ed.show_empty {
                    "[Y] 空き枠: 表示中"
                } else {
                    "[Y] 空き枠: 非表示"
                };
                self.draw_toolbar_btn(
                    c,
                    x + 8,
                    y,
                    148,
                    h,
                    emp_label,
                    ed.show_empty,
                    UiAction::Key(Key::Y),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 162,
                    y,
                    116,
                    h,
                    "[L] 元に戻す",
                    false,
                    UiAction::Key(Key::L),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 284,
                    y,
                    136,
                    h,
                    "[R] 種類変更",
                    true,
                    UiAction::Key(Key::R),
                );
            }
            Screen::ItemPick | Screen::SkillPick | Screen::Equip | Screen::SaveQuests => {
                let filter_str = match ed.screen {
                    Screen::ItemPick => &ed.item_pick_filter,
                    Screen::SkillPick => &ed.skill_pick_filter,
                    Screen::Equip => &ed.equip_filter,
                    Screen::SaveQuests => &ed.quest_filter,
                    _ => "",
                };
                let box_w = 420;
                c.round_rect(x, y, box_w, h, 6, BG_VAL_BOX);
                c.round_rect_border(x, y, box_w, h, 6, 1, ACCENT_CYAN);
                let disp = if filter_str.is_empty() {
                    String::from("[Y] 検索フィルタ: すべて表示 (タップして文字入力)")
                } else {
                    format!("[Y] 検索中: 「{}」 (タップで変更)", filter_str)
                };
                c.text_clipped(
                    x + 10,
                    y + 5,
                    box_w - 20,
                    &disp,
                    if filter_str.is_empty() {
                        TEXT_SECONDARY
                    } else {
                        TEXT_GOLD
                    },
                    FontSize::Small,
                );
                self.add_hit(x, y, box_w, h, UiAction::Key(Key::Y));

                self.draw_toolbar_btn(
                    c,
                    x + box_w + 8,
                    y,
                    118,
                    h,
                    "[X] クリア",
                    false,
                    UiAction::Key(Key::X),
                );
                if ed.screen == Screen::SaveQuests {
                    self.draw_toolbar_btn(
                        c,
                        x + box_w + 134,
                        y,
                        140,
                        h,
                        "[R] 全件書出",
                        false,
                        UiAction::Key(Key::R),
                    );
                    self.draw_toolbar_btn(
                        c,
                        x + box_w + 282,
                        y,
                        140,
                        h,
                        "[L] クエ装備",
                        false,
                        UiAction::Key(Key::L),
                    );
                }
            }
            Screen::Charms => {
                self.draw_toolbar_btn(
                    c,
                    x,
                    y,
                    156,
                    h,
                    "[L] 新規お守り作成",
                    true,
                    UiAction::Key(Key::L),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 164,
                    y,
                    156,
                    h,
                    "[Y] 選択枠を編集",
                    false,
                    UiAction::Key(Key::Y),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 328,
                    y,
                    140,
                    h,
                    "[A] 選択枠を削除",
                    false,
                    UiAction::Key(Key::A),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 476,
                    y,
                    148,
                    h,
                    "[X] セーブに保存",
                    true,
                    UiAction::Key(Key::X),
                );
            }
            Screen::Palico => {
                let lang_str = if ed.palico_lang == 0 {
                    "[L/R] 言語: 日本語 (JP)"
                } else {
                    "[L/R] 言語: 英語 (EN)"
                };
                self.draw_toolbar_btn(
                    c,
                    x,
                    y,
                    210,
                    h,
                    lang_str,
                    true,
                    UiAction::Key(Key::L),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 220,
                    y,
                    180,
                    h,
                    "[A] 導入 / 解除",
                    false,
                    UiAction::Key(Key::A),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 410,
                    y,
                    156,
                    h,
                    "[X] セーブ保存",
                    true,
                    UiAction::Key(Key::X),
                );
            }
            Screen::ArcList => {
                self.draw_toolbar_btn(
                    c,
                    x,
                    y,
                    150,
                    h,
                    "[A] .arc を開く",
                    true,
                    UiAction::Key(Key::A),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 158,
                    y,
                    200,
                    h,
                    "[ZR] セーブへ追加",
                    false,
                    UiAction::Key(Key::Z),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 366,
                    y,
                    190,
                    h,
                    "[X] セーブ内クエスト",
                    false,
                    UiAction::Key(Key::X),
                );
            }
            Screen::ArcEntries => {
                self.draw_toolbar_btn(
                    c,
                    x,
                    y,
                    140,
                    h,
                    "[A] 条件/文編集",
                    true,
                    UiAction::Key(Key::A),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 148,
                    y,
                    140,
                    h,
                    "[Y] 中身を表示",
                    false,
                    UiAction::Key(Key::Y),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 296,
                    y,
                    170,
                    h,
                    "[X] セーブへ書込",
                    true,
                    UiAction::Key(Key::X),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 474,
                    y,
                    140,
                    h,
                    "[L] クエ検査",
                    false,
                    UiAction::Key(Key::L),
                );
                self.draw_toolbar_btn(
                    c,
                    x + 622,
                    y,
                    140,
                    h,
                    "[-] .arc抽出",
                    false,
                    UiAction::Key(Key::Minus),
                );
            }
            Screen::SaveList => {
                c.text_sized(
                    x,
                    y + 5,
                    "編集するセーブデータを選択してください（先頭がSwitch本体のセーブデータです）",
                    TEXT_SECONDARY,
                    FontSize::Small,
                );
                self.draw_toolbar_btn(
                    c,
                    MAIN_X + MAIN_W - 180,
                    y,
                    168,
                    h,
                    "[L] 3DS/Switch変換",
                    false,
                    UiAction::Key(Key::L),
                );
            }
            Screen::Slots => {
                c.text_sized(
                    x,
                    y + 5,
                    "編集するハンターを選択し、下の各ボタンまたは左メニューをタップしてください",
                    TEXT_SECONDARY,
                    FontSize::Small,
                );
            }
            _ => {}
        }

        // Item count / Position counter on the far right of toolbar if applicable
        let total = list_len_of(ed);
        if total > 0 {
            let cur = active_cursor_of(ed).min(total.saturating_sub(1)) + 1;
            let pos_str = format!("{} / {}", cur, total);
            c.text_right(
                MAIN_X + MAIN_W - 14,
                y + 5,
                &pos_str,
                TEXT_CYAN,
                FontSize::Small,
            );
        }
    }

    fn draw_toolbar_btn(
        &mut self,
        c: &mut Canvas,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        label: &str,
        primary: bool,
        action: UiAction,
    ) {
        let bg = if primary { BG_BTN_PRIMARY } else { BG_BTN };
        let border = if primary { ACCENT_CYAN } else { BORDER_BTN };
        c.round_rect(x, y, w, h, 6, bg);
        c.round_rect_border(x, y, w, h, 6, 1, border);
        c.text_centered(x, y, w, h, label, TEXT_PRIMARY, FontSize::Small);
        self.add_hit(x, y, w, h, action);
    }

    // -----------------------------------------------------------------------
    // 4. Tailored Main Content Area per Screen
    // -----------------------------------------------------------------------
    fn draw_content(&mut self, c: &mut Canvas, ed: &Editor) {
        match ed.screen {
            Screen::SaveList => self.draw_save_list(c, ed),
            Screen::Slots => self.draw_slots(c, ed),
            Screen::Char => self.draw_char(c, ed),
            Screen::Items => self.draw_items(c, ed),
            Screen::ItemPick => self.draw_item_pick(c, ed),
            Screen::Charms => self.draw_charms(c, ed),
            Screen::CharmEdit => self.draw_charm_edit(c, ed),
            Screen::SkillPick => self.draw_skill_pick(c, ed),
            Screen::Equip => self.draw_equip(c, ed),
            Screen::Hunts => self.draw_hunts(c, ed),
            Screen::Guild => self.draw_guild(c, ed),
            Screen::Roster => self.draw_roster(c, ed),
            Screen::Sets => self.draw_sets(c, ed),
            Screen::Palico => self.draw_palico(c, ed),
            Screen::Options => self.draw_options(c, ed),
            Screen::Convert => self.draw_convert(c, ed),
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

    // --- Screen: SaveList --------------------------------------------------
    fn draw_save_list(&mut self, c: &mut Canvas, ed: &Editor) {
        let total = ed.save_files.len();
        let visible = 7;
        let start = window_start(ed.cursor, total, visible);
        let row_h = 68;
        let list_w = MAIN_W - 44;

        for row in 0..visible {
            let idx = start + row;
            let Some(path) = ed.save_files.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 6);
            let selected = idx == ed.cursor;
            let is_nand = path == CONSOLE_SAVE;

            let bg = if selected {
                BG_SELECTED
            } else if is_nand {
                rgb(24, 42, 58)
            } else {
                BG_CARD
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 8, bg);
            c.round_rect_border(
                MAIN_X,
                ry,
                list_w,
                row_h,
                8,
                if selected { 2 } else { 1 },
                if selected {
                    ACCENT_CYAN
                } else if is_nand {
                    ACCENT_GREEN
                } else {
                    BORDER_CARD
                },
            );
            if selected {
                c.round_rect(MAIN_X + 3, ry + 6, 5, row_h - 12, 2, ACCENT_CYAN);
            }

            // Badge
            let (badge_txt, badge_col) = if is_nand {
                ("NAND 本体", ACCENT_GREEN)
            } else {
                ("SD カード", ACCENT_CYAN)
            };
            c.round_rect(MAIN_X + 16, ry + 12, 96, 24, 5, badge_col);
            c.text_centered(MAIN_X + 16, ry + 12, 96, 24, badge_txt, BG_MAIN, FontSize::Small);

            // Title & Description
            let main_title = if is_nand {
                "Nintendo Switch 本体のセーブデータ (直接マウント編集)"
            } else {
                path.as_str()
            };
            let sub_desc = if is_nand {
                "保存時に自動で /switch/mhnxeditor/backups/ にバックアップを作成します"
            } else {
                "SDカード内のセーブデータファイル（保存時に .bak を作成します）"
            };
            c.text_clipped(
                MAIN_X + 124,
                ry + 10,
                list_w - 260,
                main_title,
                TEXT_PRIMARY,
                FontSize::Medium,
            );
            c.text_clipped(
                MAIN_X + 124,
                ry + 38,
                list_w - 260,
                sub_desc,
                TEXT_SECONDARY,
                FontSize::Small,
            );

            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

            // Open button
            let bx = MAIN_X + list_w - 118;
            let by = ry + 16;
            c.round_rect(bx, by, 104, 36, 6, if selected { BG_BTN_PRIMARY } else { BG_BTN });
            c.round_rect_border(bx, by, 104, 36, 6, 1, ACCENT_CYAN);
            c.text_centered(bx, by, 104, 36, "[A] 開く", TEXT_PRIMARY, FontSize::Small);
            if selected {
                self.add_hit(bx, by, 104, 36, UiAction::Key(Key::A));
            } else {
                self.add_hit(bx, by, 104, 36, UiAction::SelectRow(idx));
            }
        }

        self.draw_scrollbar(c, ed.cursor, total, visible);
    }

    // --- Screen: Slots (3 Large Hunter Profile Cards) ----------------------
    fn draw_slots(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(save) = &ed.save else {
            c.text_sized(
                MAIN_X + 20,
                CONTENT_Y + 20,
                "セーブデータが開かれていません",
                TEXT_SECONDARY,
                FontSize::Medium,
            );
            return;
        };

        let card_h = 152;
        for i in 0..3 {
            let cy = CONTENT_Y + (i as i32) * (card_h + 10);
            let selected = ed.slot == i;
            let used = save.slot_used(i);
            let ch = save.read_character(i);

            let bg = if selected { BG_SELECTED } else { BG_CARD };
            let border = if selected { ACCENT_CYAN } else { BORDER_CARD };
            c.round_rect(MAIN_X, cy, MAIN_W, card_h, 10, bg);
            c.round_rect_border(MAIN_X, cy, MAIN_W, card_h, 10, if selected { 2 } else { 1 }, border);
            if selected {
                c.round_rect(MAIN_X + 4, cy + 8, 6, card_h - 16, 3, ACCENT_CYAN);
            }

            // Slot number badge
            c.round_rect(
                MAIN_X + 20,
                cy + 14,
                110,
                30,
                6,
                if selected { ACCENT_CYAN } else { BG_BTN },
            );
            c.text_centered(
                MAIN_X + 20,
                cy + 14,
                110,
                30,
                &format!("SLOT {}", i + 1),
                if selected { BG_MAIN } else { TEXT_PRIMARY },
                FontSize::Small,
            );

            if let Some(ch) = ch {
                let name_str = if ch.name.is_empty() {
                    "(名前なし)"
                } else {
                    ch.name.as_str()
                };
                c.text_sized(MAIN_X + 144, cy + 12, name_str, TEXT_PRIMARY, FontSize::Large);

                let status_badge = if used { "使用中" } else { "未使用枠" };
                c.text_sized(
                    MAIN_X + 420,
                    cy + 18,
                    status_badge,
                    if used { ACCENT_GREEN } else { TEXT_MUTED },
                    FontSize::Small,
                );

                // Stats row
                let hours = ch.play_time / 3600;
                let mins = (ch.play_time % 3600) / 60;
                let gender_str = if ch.gender == 0 { "男性" } else { "女性" };
                let stats_text = format!(
                    "HR: {}    所持金: {} z    プレイ時間: {}時間{:02}分    性別: {}    龍歴院P: {} pts",
                    ch.hunter_rank, ch.funds, hours, mins, gender_str, ch.academy_points
                );
                c.text_sized(
                    MAIN_X + 22,
                    cy + 56,
                    &stats_text,
                    TEXT_GOLD,
                    FontSize::Medium,
                );
            }

            self.add_row_hit(MAIN_X, cy, MAIN_W, card_h, i);

            // Quick Action Buttons inside each Slot card
            let quick_btns: &[(&str, Screen)] = &[
                ("[A] ステータス編集", Screen::Char),
                ("[X] アイテムBOX", Screen::Items),
                ("[-] 装備BOX", Screen::Equip),
                ("[L] お守り編集", Screen::Charms),
                ("[R] 配信オトモ", Screen::Palico),
                ("[Y] クエスト管理", Screen::SaveQuests),
            ];
            let mut bx = MAIN_X + 20;
            let by = cy + 98;
            for &(b_label, target_scr) in quick_btns {
                let bw = 154;
                let bh = 38;
                c.round_rect(
                    bx,
                    by,
                    bw,
                    bh,
                    6,
                    if selected { BG_BTN_PRIMARY } else { BG_BTN },
                );
                c.round_rect_border(bx, by, bw, bh, 6, 1, if selected { ACCENT_CYAN } else { BORDER_BTN });
                c.text_centered(bx, by, bw, bh, b_label, TEXT_PRIMARY, FontSize::Small);
                self.add_hit(bx, by, bw, bh, UiAction::SelectSlot(i));
                self.add_hit(bx, by, bw, bh, UiAction::GotoScreen(target_scr));
                bx += bw + 8;
            }
        }
    }

    // --- Screen: Char (Character Status & Appearance) ----------------------
    fn draw_char(&mut self, c: &mut Canvas, ed: &Editor) {
        let total = CHAR_FIELDS.len();
        let visible = 10;
        let start = window_start(ed.field, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;

        for row in 0..visible {
            let idx = start + row;
            let Some(&(key, ja)) = CHAR_FIELDS.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 4);
            let selected = idx == ed.field;

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            // Category tag
            let (cat_txt, cat_col) = if idx == 0 {
                ("名前", ACCENT_CYAN)
            } else if idx <= 4 {
                ("基本", ACCENT_GOLD)
            } else if idx <= 9 {
                ("ポイント", ACCENT_GREEN)
            } else {
                ("外見", ACCENT_PURPLE)
            };
            c.round_rect(MAIN_X + 14, ry + 9, 68, 28, 5, BG_VAL_BOX);
            c.round_rect_border(MAIN_X + 14, ry + 9, 68, 28, 5, 1, cat_col);
            c.text_centered(MAIN_X + 14, ry + 9, 68, 28, cat_txt, cat_col, FontSize::Small);

            // Label
            c.text_sized(MAIN_X + 94, ry + 11, ja, TEXT_PRIMARY, FontSize::Medium);
            c.text_sized(MAIN_X + 290, ry + 14, key, TEXT_MUTED, FontSize::Small);

            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

            if idx == 0 {
                // Character Name row
                let name_val = ed.character().map(|ch| ch.name).unwrap_or_default();
                let val_x = MAIN_X + list_w - 430;
                c.round_rect(val_x, ry + 6, 260, 34, 6, BG_VAL_BOX);
                c.round_rect_border(val_x, ry + 6, 260, 34, 6, 1, ACCENT_CYAN);
                c.text_centered(val_x, ry + 6, 260, 34, &name_val, TEXT_GOLD, FontSize::Medium);
                self.add_hit(val_x, ry + 6, 260, 34, UiAction::AdjustRow(idx, Key::Right));

                let btn_x = val_x + 270;
                c.round_rect(btn_x, ry + 6, 146, 34, 6, BG_BTN_PRIMARY);
                c.round_rect_border(btn_x, ry + 6, 146, 34, 6, 1, ACCENT_CYAN);
                c.text_centered(
                    btn_x,
                    ry + 6,
                    146,
                    34,
                    "名前変更 [</>] ",
                    TEXT_PRIMARY,
                    FontSize::Small,
                );
                self.add_hit(btn_x, ry + 6, 146, 34, UiAction::AdjustRow(idx, Key::Right));
            } else {
                // Numeric field row with [-] [ value ] [+] [MAX]
                let raw_v = ed.char_value(idx);
                let disp_v = match idx {
                    1 => format!("{} z", raw_v),
                    2 => format!(
                        "{}時間{:02}分 ({}秒)",
                        raw_v / 3600,
                        (raw_v % 3600) / 60,
                        raw_v
                    ),
                    3 => format!("HR {}", raw_v),
                    13 => format!(
                        "{} ({})",
                        if raw_v == 0 { "男性" } else { "女性" },
                        raw_v
                    ),
                    _ => format!("{}", raw_v),
                };

                let has_max = matches!(idx, 1 | 3 | 4 | 5..=9);
                self.draw_value_stepper(
                    c,
                    MAIN_X + list_w - 430,
                    ry + 6,
                    idx,
                    &disp_v,
                    selected,
                    has_max,
                );
            }
        }

        self.draw_scrollbar(c, ed.field, total, visible);
    }

    // --- Screen: Items (Item Box with Japanese Item Names & Steppers) ------
    fn draw_items(&mut self, c: &mut Canvas, ed: &Editor) {
        let vis = ed.visible_items();
        let total = vis.len();
        let visible = 10;
        let start = window_start(ed.cursor, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;

        let Some(items) = ed.save.as_ref().and_then(|s| s.read_item_box(ed.slot)) else {
            return;
        };

        for row in 0..visible {
            let v_idx = start + row;
            let Some(&box_idx) = vis.get(v_idx) else {
                break;
            };
            let slot = items[box_idx];
            let ry = CONTENT_Y + (row as i32) * (row_h + 4);
            let selected = v_idx == ed.cursor;

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            // Box Page & Slot # badge
            let page = box_idx / 100 + 1;
            c.round_rect(MAIN_X + 14, ry + 8, 108, 30, 5, BG_VAL_BOX);
            c.text_centered(
                MAIN_X + 14,
                ry + 8,
                108,
                30,
                &format!("P{:02} #{}", page, box_idx + 1),
                TEXT_SECONDARY,
                FontSize::Small,
            );

            // Item ID & Japanese Item Name!
            let item_name = if slot.id == 0 {
                String::from("(空きスロット)")
            } else {
                item_name_of(slot.id)
            };
            let id_str = format!("ID:{:04}", slot.id);
            c.text_sized(MAIN_X + 132, ry + 14, &id_str, TEXT_MUTED, FontSize::Small);
            c.text_clipped(
                MAIN_X + 206,
                ry + 11,
                290,
                &item_name,
                if slot.id == 0 {
                    TEXT_MUTED
                } else {
                    TEXT_PRIMARY
                },
                FontSize::Medium,
            );

            self.add_row_hit(MAIN_X, ry, list_w, row_h, v_idx);

            // Change item type button [R 変更]
            let pick_x = MAIN_X + list_w - 498;
            c.round_rect(pick_x, ry + 6, 84, 34, 6, BG_BTN);
            c.round_rect_border(pick_x, ry + 6, 84, 34, 6, 1, BORDER_BTN);
            c.text_centered(pick_x, ry + 6, 84, 34, "[R] 変更", TEXT_CYAN, FontSize::Small);
            self.add_hit(pick_x, ry + 6, 84, 34, UiAction::OpenItemPicker(v_idx));

            // Count Stepper: [-] [ x99 ] [+] [99]
            let cnt_str = format!("x {}", slot.count);
            self.draw_value_stepper(
                c,
                MAIN_X + list_w - 406,
                ry + 6,
                v_idx,
                &cnt_str,
                selected,
                true,
            );
        }

        self.draw_scrollbar(c, ed.cursor, total, visible);
    }

    // --- Screen: ItemPick & SkillPick --------------------------------------
    fn draw_item_pick(&mut self, c: &mut Canvas, ed: &Editor) {
        let matches = ed.item_pick_matches();
        let total = matches.len();
        let visible = 10;
        let start = window_start(ed.item_pick_cursor, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;

        for row in 0..visible {
            let idx = start + row;
            let Some(&id) = matches.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 4);
            let selected = idx == ed.item_pick_cursor;

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            c.round_rect(MAIN_X + 14, ry + 8, 86, 30, 5, BG_VAL_BOX);
            c.text_centered(
                MAIN_X + 14,
                ry + 8,
                86,
                30,
                &format!("ID {:04}", id),
                TEXT_CYAN,
                FontSize::Small,
            );

            let name = item_name_of(id);
            c.text_clipped(
                MAIN_X + 114,
                ry + 11,
                list_w - 250,
                &name,
                TEXT_PRIMARY,
                FontSize::Medium,
            );

            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

            let bx = MAIN_X + list_w - 116;
            c.round_rect(bx, ry + 6, 102, 34, 6, if selected { BG_BTN_PRIMARY } else { BG_BTN });
            c.round_rect_border(bx, ry + 6, 102, 34, 6, 1, ACCENT_CYAN);
            c.text_centered(bx, ry + 6, 102, 34, "[A] 決定", TEXT_PRIMARY, FontSize::Small);
            if selected {
                self.add_hit(bx, ry + 6, 102, 34, UiAction::Key(Key::A));
            } else {
                self.add_hit(bx, ry + 6, 102, 34, UiAction::SelectRow(idx));
            }
        }

        self.draw_scrollbar(c, ed.item_pick_cursor, total, visible);
    }

    fn draw_skill_pick(&mut self, c: &mut Canvas, ed: &Editor) {
        let matches = ed.skill_pick_matches();
        let total = matches.len();
        let visible = 10;
        let start = window_start(ed.skill_pick_cursor, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;

        for row in 0..visible {
            let idx = start + row;
            let Some(&id) = matches.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 4);
            let selected = idx == ed.skill_pick_cursor;

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            c.round_rect(MAIN_X + 14, ry + 8, 86, 30, 5, BG_VAL_BOX);
            c.text_centered(
                MAIN_X + 14,
                ry + 8,
                86,
                30,
                &format!("ID {:03}", id),
                ACCENT_PURPLE,
                FontSize::Small,
            );

            let name = mhnx_core::skills::skill_name(id).unwrap_or("?");
            c.text_clipped(
                MAIN_X + 114,
                ry + 11,
                list_w - 250,
                name,
                TEXT_PRIMARY,
                FontSize::Medium,
            );

            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

            let bx = MAIN_X + list_w - 116;
            c.round_rect(bx, ry + 6, 102, 34, 6, if selected { BG_BTN_PRIMARY } else { BG_BTN });
            c.round_rect_border(bx, ry + 6, 102, 34, 6, 1, ACCENT_CYAN);
            c.text_centered(bx, ry + 6, 102, 34, "[A] 決定", TEXT_PRIMARY, FontSize::Small);
            if selected {
                self.add_hit(bx, ry + 6, 102, 34, UiAction::Key(Key::A));
            } else {
                self.add_hit(bx, ry + 6, 102, 34, UiAction::SelectRow(idx));
            }
        }

        self.draw_scrollbar(c, ed.skill_pick_cursor, total, visible);
    }

    // --- Screen: Charms & CharmEdit ----------------------------------------
    fn draw_charms(&mut self, c: &mut Canvas, ed: &Editor) {
        let list = ed.charms();
        let total = list.len();
        if total == 0 {
            c.round_rect(MAIN_X, CONTENT_Y, MAIN_W, 120, 10, BG_CARD);
            c.text_centered(
                MAIN_X,
                CONTENT_Y,
                MAIN_W,
                120,
                "お守りがありません。上部の「[L] 新規お守り作成」ボタンを押して作成できます。",
                TEXT_SECONDARY,
                FontSize::Medium,
            );
            return;
        }
        let visible = 10;
        let start = window_start(ed.cursor, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;

        for row in 0..visible {
            let idx = start + row;
            let Some(chm) = list.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 4);
            let selected = idx == ed.cursor;

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            // Box index
            c.round_rect(MAIN_X + 14, ry + 8, 92, 30, 5, BG_VAL_BOX);
            c.text_centered(
                MAIN_X + 14,
                ry + 8,
                92,
                30,
                &format!("枠 #{:04}", chm.index),
                ACCENT_PURPLE,
                FontSize::Small,
            );

            // Skill 1 & Skill 2
            let s1_name = mhnx_core::skills::skill_name(chm.skill1).unwrap_or("なし");
            let s2_name = mhnx_core::skills::skill_name(chm.skill2).unwrap_or("なし");
            let s1_txt = format!("第1: {} {:+}", s1_name, chm.points1);
            let s2_txt = if chm.skill2 == 0 {
                String::from("第2: (なし)")
            } else {
                format!("第2: {} {:+}", s2_name, chm.points2)
            };
            c.text_clipped(MAIN_X + 120, ry + 11, 240, &s1_txt, TEXT_PRIMARY, FontSize::Medium);
            c.text_clipped(MAIN_X + 370, ry + 11, 240, &s2_txt, TEXT_GOLD, FontSize::Medium);

            // Slots indicator
            let slots_str = match chm.slots {
                0 => "スロ: ーーー (0)",
                1 => "スロ: ◯ーー (1)",
                2 => "スロ: ◯◯ー (2)",
                _ => "スロ: ◯◯◯ (3)",
            };
            c.text_sized(MAIN_X + 620, ry + 13, slots_str, TEXT_CYAN, FontSize::Small);

            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

            let bx = MAIN_X + list_w - 116;
            c.round_rect(bx, ry + 6, 102, 34, 6, if selected { BG_BTN_PRIMARY } else { BG_BTN });
            c.round_rect_border(bx, ry + 6, 102, 34, 6, 1, ACCENT_CYAN);
            c.text_centered(bx, ry + 6, 102, 34, "[Y] 編集", TEXT_PRIMARY, FontSize::Small);
            if selected {
                self.add_hit(bx, ry + 6, 102, 34, UiAction::Key(Key::Y));
            } else {
                self.add_hit(bx, ry + 6, 102, 34, UiAction::SelectRow(idx));
            }
        }

        self.draw_scrollbar(c, ed.cursor, total, visible);
    }

    fn draw_charm_edit(&mut self, c: &mut Canvas, ed: &Editor) {
        let ce = ed.charm_edit.unwrap_or_default();
        let s1_name = mhnx_core::skills::skill_name(ce.skill1).unwrap_or("なし");
        let s2_name = mhnx_core::skills::skill_name(ce.skill2).unwrap_or("なし");
        let slots_disp = match ce.slots {
            0 => "ーーー (0スロ)",
            1 => "◯ーー (1スロ)",
            2 => "◯◯ー (2スロ)",
            _ => "◯◯◯ (3スロ)",
        };

        let rows: [(&str, String, bool); 6] = [
            ("装備BOX枠番号", format!("枠 #{}", ce.index), false),
            ("スロット数", String::from(slots_disp), false),
            ("第1スキル種類", format!("{} (ID: {})", s1_name, ce.skill1), true),
            ("第1スキルポイント", format!("{:+}", ce.points1), false),
            ("第2スキル種類", format!("{} (ID: {})", s2_name, ce.skill2), true),
            ("第2スキルポイント", format!("{:+}", ce.points2), false),
        ];

        let row_h = 52;
        for (i, (label, val, is_skill_id)) in rows.iter().enumerate() {
            let ry = CONTENT_Y + (i as i32) * (row_h + 6);
            let selected = ce.field == i;

            c.round_rect(
                MAIN_X,
                ry,
                MAIN_W,
                row_h,
                8,
                if selected { BG_SELECTED } else { BG_CARD },
            );
            c.round_rect_border(
                MAIN_X,
                ry,
                MAIN_W,
                row_h,
                8,
                if selected { 2 } else { 1 },
                if selected { ACCENT_CYAN } else { BORDER_CARD },
            );
            if selected {
                c.round_rect(MAIN_X + 3, ry + 6, 5, row_h - 12, 2, ACCENT_CYAN);
            }

            c.text_sized(MAIN_X + 20, ry + 14, label, TEXT_PRIMARY, FontSize::Medium);
            self.add_row_hit(MAIN_X, ry, MAIN_W, row_h, i);

            if *is_skill_id {
                let pick_x = MAIN_X + MAIN_W - 510;
                c.round_rect(pick_x, ry + 9, 150, 34, 6, BG_BTN_PRIMARY);
                c.round_rect_border(pick_x, ry + 9, 150, 34, 6, 1, ACCENT_CYAN);
                c.text_centered(
                    pick_x,
                    ry + 9,
                    150,
                    34,
                    "[Y] 名前から選択",
                    TEXT_PRIMARY,
                    FontSize::Small,
                );
                self.add_hit(pick_x, ry + 9, 150, 34, UiAction::OpenCharmSkill(i));
            }

            self.draw_value_stepper(
                c,
                MAIN_X + MAIN_W - 350,
                ry + 9,
                i,
                val,
                selected,
                false,
            );
        }

        // Legality / Allowed Skill Points Inspector Panel at bottom of CharmEdit
        let panel_y = CONTENT_Y + 6 * (row_h + 6) + 8;
        let panel_h = CONTENT_Y + CONTENT_H - panel_y;
        if panel_h >= 60 {
            c.round_rect(MAIN_X, panel_y, MAIN_W, panel_h, 8, rgb(19, 25, 36));
            c.round_rect_border(MAIN_X, panel_y, MAIN_W, panel_h, 8, 1, BORDER_CARD);

            let ty = ed
                .save
                .as_ref()
                .and_then(|s| s.equipment_box(ed.slot))
                .and_then(|b| b.get(ce.index * charm::CHARM_RECORD + 2).copied())
                .unwrap_or(0);
            let rarity_str = charm::rarity_of(ty)
                .map(|r| format!("護石種別: 0x{:02X} (レア度 {})", ty, r))
                .unwrap_or_else(|| format!("護石種別: 0x{:02X}", ty));
            c.text_sized(
                MAIN_X + 18,
                panel_y + 10,
                &format!("適正範囲チェック — {}", rarity_str),
                TEXT_CYAN,
                FontSize::Small,
            );

            let mut py = panel_y + 36;
            for (tag, sid, pts, second) in [
                ("第1スキル", ce.skill1, ce.points1, false),
                ("第2スキル", ce.skill2, ce.points2, true),
            ] {
                if sid == 0 {
                    c.text_sized(
                        MAIN_X + 24,
                        py,
                        &format!("{}: 未設定 (問題なし)", tag),
                        TEXT_SECONDARY,
                        FontSize::Small,
                    );
                } else if let Some(r) = charm::allowed_points(ty, sid, second) {
                    let sname = mhnx_core::skills::skill_name(sid).unwrap_or("?");
                    if r.min == 0 && r.max == 0 {
                        c.text_sized(
                            MAIN_X + 24,
                            py,
                            &format!(
                                "⚠ {}: 「{}」はこの護石種別には付与できません（ゲーム内で消去されます）",
                                tag, sname
                            ),
                            ACCENT_RED,
                            FontSize::Small,
                        );
                    } else if pts < r.min || pts > r.max {
                        c.text_sized(
                            MAIN_X + 24,
                            py,
                            &format!(
                                "⚠ {}: 「{} {:+}」は範囲外です（許容範囲: {:+} 〜 {:+}）",
                                tag, sname, pts, r.min, r.max
                            ),
                            ACCENT_ORANGE,
                            FontSize::Small,
                        );
                    } else {
                        c.text_sized(
                            MAIN_X + 24,
                            py,
                            &format!(
                                "✓ {}: 「{} {:+}」は正常範囲内です（許容範囲: {:+} 〜 {:+}）",
                                tag, sname, pts, r.min, r.max
                            ),
                            ACCENT_GREEN,
                            FontSize::Small,
                        );
                    }
                }
                py += 26;
            }

            // Apply button inside panel
            let apply_x = MAIN_X + MAIN_W - 190;
            let apply_y = panel_y + panel_h - 46;
            c.round_rect(apply_x, apply_y, 174, 36, 7, BG_BTN_PRIMARY);
            c.round_rect_border(apply_x, apply_y, 174, 36, 7, 2, ACCENT_CYAN);
            c.text_centered(
                apply_x,
                apply_y,
                174,
                36,
                "[A] お守りを反映",
                TEXT_PRIMARY,
                FontSize::Small,
            );
            self.add_hit(apply_x, apply_y, 174, 36, UiAction::Key(Key::A));
        }
    }

    // --- Screen: Equip -----------------------------------------------------
    fn draw_equip(&mut self, c: &mut Canvas, ed: &Editor) {
        let all = ed.equipment();
        let total = all.len();
        let visible = 10;
        let start = window_start(ed.equip_cursor, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;

        for row in 0..visible {
            let idx = start + row;
            let Some(eq) = all.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 4);
            let selected = idx == ed.equip_cursor;

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            c.round_rect(MAIN_X + 14, ry + 8, 92, 30, 5, BG_VAL_BOX);
            c.text_centered(
                MAIN_X + 14,
                ry + 8,
                92,
                30,
                &format!("枠 #{:04}", eq.index),
                TEXT_SECONDARY,
                FontSize::Small,
            );

            let tname = mhnx_core::equip::type_name(eq.kind);
            c.round_rect(MAIN_X + 114, ry + 8, 120, 30, 5, rgb(32, 44, 64));
            c.text_centered(MAIN_X + 114, ry + 8, 120, 30, tname, TEXT_CYAN, FontSize::Small);

            c.text_sized(
                MAIN_X + 248,
                ry + 13,
                &format!("Lv {}", eq.level),
                TEXT_GOLD,
                FontSize::Small,
            );

            let ename = mhnx_core::equip::name(eq);
            c.text_clipped(
                MAIN_X + 320,
                ry + 11,
                list_w - 340,
                &ename,
                TEXT_PRIMARY,
                FontSize::Medium,
            );

            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);
        }

        self.draw_scrollbar(c, ed.equip_cursor, total, visible);
    }

    // --- Screen: Hunts (Monster Hunt / Capture / Crown Sizes) --------------
    fn draw_hunts(&mut self, c: &mut Canvas, ed: &Editor) {
        let rows = ed.hunt_rows();
        let total = rows.len();
        let visible = 9;
        let start = window_start(ed.hunt_cursor, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;

        // Column header bar
        c.round_rect(MAIN_X, CONTENT_Y, list_w, 32, 6, rgb(20, 26, 38));
        c.text_sized(MAIN_X + 16, CONTENT_Y + 6, "モンスター名", TEXT_SECONDARY, FontSize::Small);
        let col_labels = ["討伐数", "捕獲数", "最小サイズ", "最大サイズ"];
        for (f, col_name) in col_labels.iter().enumerate() {
            let cx = MAIN_X + 330 + (f as i32) * 108;
            let active_f = ed.hunt_field == f;
            c.text_centered(
                cx,
                CONTENT_Y + 4,
                100,
                24,
                col_name,
                if active_f { TEXT_GOLD } else { TEXT_SECONDARY },
                FontSize::Small,
            );
            self.add_hit(
                cx,
                CONTENT_Y,
                100,
                32,
                UiAction::SelectSubField(ed.hunt_cursor, f),
            );
        }

        for row in 0..visible {
            let idx = start + row;
            let Some(&(id, slay, cap, lo, hi)) = rows.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + 38 + (row as i32) * (row_h + 4);
            let selected = idx == ed.hunt_cursor;

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            c.text_clipped(
                MAIN_X + 16,
                ry + 11,
                304,
                &format!("#{:03}  {}", id, monster_name(id)),
                TEXT_PRIMARY,
                FontSize::Medium,
            );
            self.add_row_hit(MAIN_X, ry, 320, row_h, idx);

            let vals = [slay, cap, lo, hi];
            for (f, &v) in vals.iter().enumerate() {
                let cx = MAIN_X + 330 + (f as i32) * 108;
                let is_active_cell = selected && ed.hunt_field == f;
                c.round_rect(
                    cx,
                    ry + 6,
                    100,
                    34,
                    6,
                    if is_active_cell {
                        BG_VAL_ACTIVE
                    } else {
                        BG_VAL_BOX
                    },
                );
                c.round_rect_border(
                    cx,
                    ry + 6,
                    100,
                    34,
                    6,
                    if is_active_cell { 2 } else { 1 },
                    if is_active_cell {
                        ACCENT_GOLD
                    } else {
                        BORDER_SUBTLE
                    },
                );
                c.text_centered(
                    cx,
                    ry + 6,
                    100,
                    34,
                    &format!("{}", v),
                    if is_active_cell {
                        TEXT_GOLD
                    } else {
                        TEXT_PRIMARY
                    },
                    FontSize::Small,
                );
                self.add_hit(cx, ry + 6, 100, 34, UiAction::SelectSubField(idx, f));
            }

            // [-] [+] buttons on the right of each monster row
            let bx = MAIN_X + list_w - 172;
            self.draw_mini_pm_buttons(c, bx, ry + 6, idx, selected);
        }

        self.draw_scrollbar(c, ed.hunt_cursor, total, visible);
    }

    // --- Screen: Guild (Guild Card & Arena Records) ------------------------
    fn draw_guild(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(save) = &ed.save else {
            return;
        };
        let hr = mhnx_core::guildcard::hunter_rank(save, ed.slot).unwrap_or(0);
        let time = mhnx_core::guildcard::play_time(save, ed.slot).unwrap_or(0);
        let comment = mhnx_core::guildcard::comment(save, ed.slot).unwrap_or_default();

        let fields: [(&str, String); 3] = [
            ("ギルドカード HR", format!("HR {}", hr)),
            (
                "ギルドカード プレイ時間",
                format!("{}時間{:02}分{:02}秒", time / 3600, (time % 3600) / 60, time % 60),
            ),
            ("ギルドカード コメント [A]", comment),
        ];

        for (i, (label, val)) in fields.iter().enumerate() {
            let ry = CONTENT_Y + (i as i32) * 56;
            let selected = ed.guild_field == i;
            c.round_rect(
                MAIN_X,
                ry,
                MAIN_W,
                50,
                8,
                if selected { BG_SELECTED } else { BG_CARD },
            );
            c.round_rect_border(
                MAIN_X,
                ry,
                MAIN_W,
                50,
                8,
                if selected { 2 } else { 1 },
                if selected { ACCENT_CYAN } else { BORDER_CARD },
            );
            c.text_sized(MAIN_X + 20, ry + 13, label, TEXT_PRIMARY, FontSize::Medium);
            self.add_hit(MAIN_X, ry, MAIN_W, 50, UiAction::SelectSubField(i, i));

            if i < 2 {
                self.draw_value_stepper(
                    c,
                    MAIN_X + MAIN_W - 350,
                    ry + 8,
                    i,
                    val,
                    selected,
                    false,
                );
            } else {
                let vx = MAIN_X + 320;
                let vw = MAIN_W - 340;
                c.round_rect(vx, ry + 8, vw, 34, 6, BG_VAL_BOX);
                c.round_rect_border(vx, ry + 8, vw, 34, 6, 1, ACCENT_CYAN);
                c.text_clipped(vx + 12, ry + 12, vw - 24, val, TEXT_GOLD, FontSize::Small);
                self.add_hit(vx, ry + 8, vw, 34, UiAction::SelectSubField(2, 2));
            }
        }

        // Arena records table below
        let ay = CONTENT_Y + 180;
        let ah = CONTENT_Y + CONTENT_H - ay;
        c.round_rect(MAIN_X, ay, MAIN_W, ah, 8, BG_CARD);
        c.round_rect_border(MAIN_X, ay, MAIN_W, ah, 8, 1, BORDER_CARD);
        c.text_sized(MAIN_X + 18, ay + 12, "闘技大会 記録一覧", TEXT_CYAN, FontSize::Medium);

        let log = mhnx_core::guildcard::arena(save, ed.slot);
        let mut ry = ay + 46;
        for a in 0..mhnx_core::guildcard::ARENA_COUNT {
            if ry + 26 > ay + ah {
                break;
            }
            let times: Vec<String> = log
                .iter()
                .filter(|e| e.arena == a && !e.is_empty())
                .map(|e| {
                    let (m, s, cs) = e.as_clock();
                    format!("{}:{:02}.{:02}", m, s, cs)
                })
                .collect();
            if times.is_empty() {
                continue;
            }
            c.text_clipped(
                MAIN_X + 24,
                ry,
                MAIN_W - 48,
                &format!("闘技 #{:02} :   {}", a, times.join("    ")),
                TEXT_SECONDARY,
                FontSize::Small,
            );
            ry += 26;
        }
    }

    // --- Screen: Roster (Owned Palicoes) -----------------------------------
    fn draw_roster(&mut self, c: &mut Canvas, ed: &Editor) {
        let list = ed.palicos();
        let total = list.len();
        let visible = 9;
        let start = window_start(ed.roster_cursor, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;

        c.round_rect(MAIN_X, CONTENT_Y, list_w, 32, 6, rgb(20, 26, 38));
        c.text_sized(
            MAIN_X + 16,
            CONTENT_Y + 6,
            "オトモ名 (タップ/Aで名前変更)",
            TEXT_SECONDARY,
            FontSize::Small,
        );
        let cols = ["レベル (Lv)", "なつき度", "経験値 (EXP)"];
        for (f, col_name) in cols.iter().enumerate() {
            let cx = MAIN_X + 330 + (f as i32) * 140;
            let active_f = ed.roster_field == f;
            c.text_centered(
                cx,
                CONTENT_Y + 4,
                130,
                24,
                col_name,
                if active_f { TEXT_GOLD } else { TEXT_SECONDARY },
                FontSize::Small,
            );
            self.add_hit(
                cx,
                CONTENT_Y,
                130,
                32,
                UiAction::SelectSubField(ed.roster_cursor, f),
            );
        }

        for row in 0..visible {
            let idx = start + row;
            let Some(p) = list.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + 38 + (row as i32) * (row_h + 4);
            let selected = idx == ed.roster_cursor;

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            c.text_clipped(
                MAIN_X + 16,
                ry + 11,
                300,
                &format!("#{:02}  {}", p.index, p.name),
                TEXT_PRIMARY,
                FontSize::Medium,
            );
            self.add_row_hit(MAIN_X, ry, 320, row_h, idx);

            let vals = [
                format!("Lv {}", p.level),
                format!("{}", p.enthusiasm),
                format!("{}", p.exp),
            ];
            for (f, v_str) in vals.iter().enumerate() {
                let cx = MAIN_X + 330 + (f as i32) * 140;
                let is_active = selected && ed.roster_field == f;
                c.round_rect(
                    cx,
                    ry + 6,
                    130,
                    34,
                    6,
                    if is_active { BG_VAL_ACTIVE } else { BG_VAL_BOX },
                );
                c.round_rect_border(
                    cx,
                    ry + 6,
                    130,
                    34,
                    6,
                    if is_active { 2 } else { 1 },
                    if is_active { ACCENT_GOLD } else { BORDER_SUBTLE },
                );
                c.text_centered(
                    cx,
                    ry + 6,
                    130,
                    34,
                    v_str,
                    if is_active { TEXT_GOLD } else { TEXT_PRIMARY },
                    FontSize::Small,
                );
                self.add_hit(cx, ry + 6, 130, 34, UiAction::SelectSubField(idx, f));
            }

            let bx = MAIN_X + list_w - 172;
            self.draw_mini_pm_buttons(c, bx, ry + 6, idx, selected);
        }

        self.draw_scrollbar(c, ed.roster_cursor, total, visible);
    }

    // --- Screen: Sets (Item Sets & Equipment Sets) -------------------------
    fn draw_sets(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(save) = &ed.save else {
            return;
        };
        let col_w = (MAIN_W - 16) / 2;

        // Left Card: Item Sets
        c.round_rect(MAIN_X, CONTENT_Y, col_w, CONTENT_H, 8, BG_CARD);
        c.round_rect_border(MAIN_X, CONTENT_Y, col_w, CONTENT_H, 8, 1, BORDER_CARD);
        c.text_sized(
            MAIN_X + 18,
            CONTENT_Y + 12,
            "アイテムマイセット",
            ACCENT_GREEN,
            FontSize::Medium,
        );
        let mut iy = CONTENT_Y + 48;
        for set in mhnx_core::myset::item_sets(save, ed.slot) {
            if set.is_empty() || iy + 28 > CONTENT_Y + CONTENT_H {
                continue;
            }
            let n = set.used_items().len();
            c.text_clipped(
                MAIN_X + 20,
                iy,
                col_w - 40,
                &format!("[{:02}]  {}  ({}枠)", set.index, set.name, n),
                TEXT_PRIMARY,
                FontSize::Small,
            );
            iy += 28;
        }

        // Right Card: Equipment Sets
        let rx = MAIN_X + col_w + 16;
        c.round_rect(rx, CONTENT_Y, col_w, CONTENT_H, 8, BG_CARD);
        c.round_rect_border(rx, CONTENT_Y, col_w, CONTENT_H, 8, 1, BORDER_CARD);
        c.text_sized(
            rx + 18,
            CONTENT_Y + 12,
            "装備マイセット",
            ACCENT_GOLD,
            FontSize::Medium,
        );
        let mut ey = CONTENT_Y + 48;
        for set in mhnx_core::myset::equip_sets(save, ed.slot) {
            if set.is_empty() || ey + 28 > CONTENT_Y + CONTENT_H {
                continue;
            }
            c.text_clipped(
                rx + 20,
                ey,
                col_w - 40,
                &format!("[{:02}]  {}", set.index, set.name),
                TEXT_PRIMARY,
                FontSize::Small,
            );
            ey += 28;
        }
    }

    // --- Screen: Palico (Download Palico Catalogue) ------------------------
    fn draw_palico(&mut self, c: &mut Canvas, ed: &Editor) {
        let lang = if ed.palico_lang == 0 {
            Lang::Jp
        } else {
            Lang::En
        };
        let installed: Vec<u8> = ed
            .save
            .as_ref()
            .map(|s| s.dlc_palicoes().iter().map(|cat| cat.id).collect())
            .unwrap_or_default();
        let ids = dlcats::all_ids();
        let total = ids.len();
        let visible = 10;
        let start = window_start(ed.palico_cursor, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;

        for row in 0..visible {
            let idx = start + row;
            let Some(&id) = ids.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 4);
            let selected = idx == ed.palico_cursor;
            let has_it = installed.contains(&id);

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            // Installed badge
            let (b_txt, b_col) = if has_it {
                ("導入済", ACCENT_GREEN)
            } else {
                ("未導入", BORDER_BTN)
            };
            c.round_rect(MAIN_X + 14, ry + 8, 84, 30, 5, b_col);
            c.text_centered(
                MAIN_X + 14,
                ry + 8,
                84,
                30,
                b_txt,
                if has_it { BG_MAIN } else { TEXT_PRIMARY },
                FontSize::Small,
            );

            c.text_sized(
                MAIN_X + 112,
                ry + 13,
                &format!("ID 0x{:02X}", id),
                TEXT_MUTED,
                FontSize::Small,
            );

            let name = dlcats::record(lang, id)
                .map(dlcats::record_name)
                .unwrap_or_default();
            c.text_clipped(
                MAIN_X + 206,
                ry + 11,
                list_w - 360,
                &name,
                TEXT_PRIMARY,
                FontSize::Medium,
            );

            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

            let bx = MAIN_X + list_w - 134;
            let btn_lbl = if has_it { "[A] 解除する" } else { "[A] 導入する" };
            c.round_rect(bx, ry + 6, 120, 34, 6, if has_it { BG_BTN } else { BG_BTN_PRIMARY });
            c.round_rect_border(bx, ry + 6, 120, 34, 6, 1, ACCENT_CYAN);
            c.text_centered(bx, ry + 6, 120, 34, btn_lbl, TEXT_PRIMARY, FontSize::Small);
            if selected {
                self.add_hit(bx, ry + 6, 120, 34, UiAction::Key(Key::A));
            } else {
                self.add_hit(bx, ry + 6, 120, 34, UiAction::SelectRow(idx));
            }
        }

        self.draw_scrollbar(c, ed.palico_cursor, total, visible);
    }

    // --- Screen: Options & Convert -----------------------------------------
    fn draw_options(&mut self, c: &mut Canvas, ed: &Editor) {
        let o = ed
            .save
            .as_ref()
            .map(|s| s.options())
            .unwrap_or_else(Options::default);
        let rows: [(&str, String); 4] = [
            ("画面の明るさ (Brightness)", format!("{}", o.brightness)),
            (
                "コントローラー振動 (Rumble)",
                String::from(if o.rumble != 0 { "ON (有効)" } else { "OFF (無効)" }),
            ),
            (
                "3D表示の無効化 (3DSフラグ)",
                String::from(if o.disable_3d != 0 { "YES" } else { "NO" }),
            ),
            (
                "拡張スライドパッド (3DSフラグ)",
                String::from(if o.circle_pad_pro != 0 { "ON" } else { "OFF" }),
            ),
        ];

        for (i, (label, val)) in rows.iter().enumerate() {
            let ry = CONTENT_Y + (i as i32) * 64;
            let selected = ed.option_cursor == i;
            c.round_rect(
                MAIN_X,
                ry,
                MAIN_W,
                56,
                8,
                if selected { BG_SELECTED } else { BG_CARD },
            );
            c.round_rect_border(
                MAIN_X,
                ry,
                MAIN_W,
                56,
                8,
                if selected { 2 } else { 1 },
                if selected { ACCENT_CYAN } else { BORDER_CARD },
            );
            c.text_sized(MAIN_X + 22, ry + 15, label, TEXT_PRIMARY, FontSize::Medium);
            self.add_row_hit(MAIN_X, ry, MAIN_W, 56, i);
            self.draw_value_stepper(
                c,
                MAIN_X + MAIN_W - 350,
                ry + 11,
                i,
                val,
                selected,
                false,
            );
        }

        // Link card to Convert screen
        let cy = CONTENT_Y + 4 * 64 + 16;
        c.round_rect(MAIN_X, cy, MAIN_W, 74, 8, BG_CARD);
        c.round_rect_border(MAIN_X, cy, MAIN_W, 74, 8, 1, ACCENT_GOLD);
        c.text_sized(
            MAIN_X + 22,
            cy + 14,
            "3DS ⇔ Switch セーブデータコンテナ変換",
            TEXT_GOLD,
            FontSize::Medium,
        );
        c.text_sized(
            MAIN_X + 22,
            cy + 44,
            "3DS版MHXXとSwitch版MHXXの間でセーブデータのヘッダ変換を行います",
            TEXT_SECONDARY,
            FontSize::Small,
        );
        let bx = MAIN_X + MAIN_W - 190;
        c.round_rect(bx, cy + 18, 170, 38, 6, BG_BTN_PRIMARY);
        c.round_rect_border(bx, cy + 18, 170, 38, 6, 1, ACCENT_CYAN);
        c.text_centered(
            bx,
            cy + 18,
            170,
            38,
            "変換画面を開く",
            TEXT_PRIMARY,
            FontSize::Small,
        );
        self.add_hit(bx, cy + 18, 170, 38, UiAction::GotoScreen(Screen::Convert));
    }

    fn draw_convert(&mut self, c: &mut Canvas, ed: &Editor) {
        c.round_rect(MAIN_X, CONTENT_Y, MAIN_W, 290, 10, BG_CARD);
        c.round_rect_border(MAIN_X, CONTENT_Y, MAIN_W, 290, 10, 1, BORDER_CARD);

        c.text_sized(
            MAIN_X + 24,
            CONTENT_Y + 18,
            "3DS ⇔ Nintendo Switch セーブフォーマット変換",
            TEXT_CYAN,
            FontSize::Large,
        );

        match (&ed.save, &ed.save_path) {
            (None, _) => {
                c.text_sized(
                    MAIN_X + 24,
                    CONTENT_Y + 70,
                    "先に「セーブデータ選択」から変換元のセーブを開いてください。",
                    TEXT_SECONDARY,
                    FontSize::Medium,
                );
            }
            (Some(s), p) => {
                let src_kind = match s.kind() {
                    SaveKind::ThreeDs => "3DS版 MHXX (ヘッダなし)",
                    SaveKind::Switch => "Switch版 MHXX (36バイトヘッダ付き)",
                    SaveKind::Mhgu => "Switch版 MHGU",
                };
                let dst_kind = if s.kind() == SaveKind::ThreeDs {
                    "Switch版 MHXX (36バイトヘッダを付与)"
                } else {
                    "3DS版 MHXX (ヘッダを除去)"
                };
                let stem = if ed.convert_name.trim().is_empty() {
                    "system"
                } else {
                    ed.convert_name.trim()
                };
                let ext = if s.kind() == SaveKind::ThreeDs {
                    "switch"
                } else {
                    "3ds"
                };

                c.text_sized(
                    MAIN_X + 28,
                    CONTENT_Y + 68,
                    &format!("変換元フォーマット :   {}", src_kind),
                    TEXT_PRIMARY,
                    FontSize::Medium,
                );
                c.text_sized(
                    MAIN_X + 28,
                    CONTENT_Y + 106,
                    &format!("変換先フォーマット :   {}", dst_kind),
                    TEXT_GOLD,
                    FontSize::Medium,
                );
                c.text_sized(
                    MAIN_X + 28,
                    CONTENT_Y + 144,
                    &format!(
                        "出力ファイル名     :   {}.{}   (元パス: {})",
                        stem,
                        ext,
                        p.as_deref().unwrap_or("-")
                    ),
                    TEXT_SECONDARY,
                    FontSize::Small,
                );

                // Buttons
                let by = CONTENT_Y + 206;
                c.round_rect(MAIN_X + 28, by, 220, 46, 8, BG_BTN);
                c.round_rect_border(MAIN_X + 28, by, 220, 46, 8, 1, BORDER_BTN);
                c.text_centered(
                    MAIN_X + 28,
                    by,
                    220,
                    46,
                    "[Y] 出力ファイル名変更",
                    TEXT_PRIMARY,
                    FontSize::Small,
                );
                self.add_hit(MAIN_X + 28, by, 220, 46, UiAction::Key(Key::Y));

                c.round_rect(MAIN_X + 268, by, 240, 46, 8, BG_BTN_PRIMARY);
                c.round_rect_border(MAIN_X + 268, by, 240, 46, 8, 2, ACCENT_CYAN);
                c.text_centered(
                    MAIN_X + 268,
                    by,
                    240,
                    46,
                    "[A] 変換して書き出す",
                    TEXT_PRIMARY,
                    FontSize::Medium,
                );
                self.add_hit(MAIN_X + 268, by, 240, 46, UiAction::Key(Key::A));
            }
        }
    }

    // --- Screen: SaveQuests & QuestEquip -----------------------------------
    fn draw_save_quests(&mut self, c: &mut Canvas, ed: &Editor) {
        let list = ed.filtered_quests();
        let total = list.len();
        let visible = 10;
        let start = window_start(ed.save_quest_cursor, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;

        for row in 0..visible {
            let idx = start + row;
            let Some(q) = list.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 4);
            let selected = idx == ed.save_quest_cursor;

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            c.round_rect(MAIN_X + 14, ry + 8, 80, 30, 5, BG_VAL_BOX);
            c.text_centered(
                MAIN_X + 14,
                ry + 8,
                80,
                30,
                &format!("枠 {:03}", q.slot),
                ACCENT_PURPLE,
                FontSize::Small,
            );

            c.text_sized(
                MAIN_X + 104,
                ry + 13,
                &format!("ID {}", q.quest_id),
                TEXT_CYAN,
                FontSize::Small,
            );

            let title = ed.quest_title(q.quest_id);
            let disp_title = if title.is_empty() {
                "(タイトル未取得)"
            } else {
                title
            };
            c.text_clipped(
                MAIN_X + 216,
                ry + 11,
                list_w - 450,
                disp_title,
                TEXT_PRIMARY,
                FontSize::Medium,
            );

            c.text_right(
                MAIN_X + list_w - 216,
                ry + 14,
                &format!("{} B", q.size),
                TEXT_MUTED,
                FontSize::Small,
            );

            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

            // [A 編集] and [- 書出]
            let bx1 = MAIN_X + list_w - 204;
            c.round_rect(bx1, ry + 6, 92, 34, 6, if selected { BG_BTN_PRIMARY } else { BG_BTN });
            c.round_rect_border(bx1, ry + 6, 92, 34, 6, 1, ACCENT_CYAN);
            c.text_centered(bx1, ry + 6, 92, 34, "[A] 編集", TEXT_PRIMARY, FontSize::Small);
            if selected {
                self.add_hit(bx1, ry + 6, 92, 34, UiAction::Key(Key::A));
            } else {
                self.add_hit(bx1, ry + 6, 92, 34, UiAction::SelectRow(idx));
            }

            let bx2 = MAIN_X + list_w - 104;
            c.round_rect(bx2, ry + 6, 92, 34, 6, BG_BTN);
            c.round_rect_border(bx2, ry + 6, 92, 34, 6, 1, BORDER_BTN);
            c.text_centered(bx2, ry + 6, 92, 34, "[-] 抽出", TEXT_SECONDARY, FontSize::Small);
            if selected {
                self.add_hit(bx2, ry + 6, 92, 34, UiAction::Key(Key::Minus));
            } else {
                self.add_hit(bx2, ry + 6, 92, 34, UiAction::SelectRow(idx));
            }
        }

        self.draw_scrollbar(c, ed.save_quest_cursor, total, visible);
    }

    fn draw_quest_equip(&mut self, c: &mut Canvas, ed: &Editor) {
        let visible = 10;
        let row_h = 46;
        let list_w = MAIN_W - 44;

        if ed.qe_open {
            let items = ed.quest_equip_items();
            let total = items.len();
            let start = window_start(ed.qe_item_cursor, total, visible);
            for row in 0..visible {
                let idx = start + row;
                let Some(it) = items.get(idx) else {
                    break;
                };
                let ry = CONTENT_Y + (row as i32) * (row_h + 4);
                let selected = idx == ed.qe_item_cursor;

                c.round_rect(
                    MAIN_X,
                    ry,
                    list_w,
                    row_h,
                    7,
                    if selected { BG_SELECTED } else { BG_CARD },
                );
                if selected {
                    c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                }

                let kind_str = match it.list {
                    mhnx_core::dlc::QuestEquipList::Supply => "支給品",
                    mhnx_core::dlc::QuestEquipList::Ammo => "弾ポーチ",
                };
                c.round_rect(MAIN_X + 14, ry + 8, 120, 30, 5, BG_VAL_BOX);
                c.text_centered(
                    MAIN_X + 14,
                    ry + 8,
                    120,
                    30,
                    &format!("rec{} {}[{}]", it.record, kind_str, it.index),
                    TEXT_CYAN,
                    FontSize::Small,
                );

                let iname = if it.item == 0 {
                    String::from("(なし)")
                } else {
                    item_name_of(it.item)
                };
                c.text_clipped(
                    MAIN_X + 148,
                    ry + 11,
                    list_w - 340,
                    &format!("{}  (ID:{})   x{}", iname, it.item, it.count),
                    TEXT_PRIMARY,
                    FontSize::Medium,
                );
                self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

                let bx = MAIN_X + list_w - 134;
                c.round_rect(bx, ry + 6, 120, 34, 6, BG_BTN_PRIMARY);
                c.text_centered(bx, ry + 6, 120, 34, "[A] 変更", TEXT_PRIMARY, FontSize::Small);
                self.add_hit(bx, ry + 6, 120, 34, UiAction::Key(Key::A));
            }
            self.draw_scrollbar(c, ed.qe_item_cursor, total, visible);
        } else {
            let list = ed.quest_equipment();
            let total = list.len();
            let start = window_start(ed.cursor, total, visible);
            for row in 0..visible {
                let idx = start + row;
                let Some(e) = list.get(idx) else {
                    break;
                };
                let ry = CONTENT_Y + (row as i32) * (row_h + 4);
                let selected = idx == ed.cursor;
                c.round_rect(
                    MAIN_X,
                    ry,
                    list_w,
                    row_h,
                    7,
                    if selected { BG_SELECTED } else { BG_CARD },
                );
                if selected {
                    c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                }
                let q_str = if e.occupied {
                    format!("クエストID: {}  ({})", e.quest_id, ed.quest_title(e.quest_id))
                } else {
                    String::from("(未使用スロット)")
                };
                c.text_sized(
                    MAIN_X + 20,
                    ry + 11,
                    &format!("クエスト装備枠 #{:02}    {}", e.index, q_str),
                    if e.occupied { TEXT_PRIMARY } else { TEXT_MUTED },
                    FontSize::Medium,
                );
                self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);
            }
            self.draw_scrollbar(c, ed.cursor, total, visible);
        }
    }

    // --- Screen: ArcList & ArcEntries --------------------------------------
    fn draw_arc_list(&mut self, c: &mut Canvas, ed: &Editor) {
        let total = ed.arc_files.len();
        if total == 0 {
            c.round_rect(MAIN_X, CONTENT_Y, MAIN_W, 120, 10, BG_CARD);
            c.text_centered(
                MAIN_X,
                CONTENT_Y,
                MAIN_W,
                120,
                "SDカード内に .arc クエストファイルが見つかりません (/switch/mhnxeditor/quest/ 等)",
                TEXT_SECONDARY,
                FontSize::Medium,
            );
            return;
        }
        let visible = 10;
        let start = window_start(ed.cursor, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;

        for row in 0..visible {
            let idx = start + row;
            let Some(path) = ed.arc_files.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 4);
            let selected = idx == ed.cursor;

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            let base_name = path.rsplit('/').next().unwrap_or(path.as_str());
            c.round_rect(MAIN_X + 14, ry + 8, 120, 30, 5, BG_VAL_BOX);
            c.text_centered(MAIN_X + 14, ry + 8, 120, 30, base_name, ACCENT_PURPLE, FontSize::Small);

            c.text_clipped(
                MAIN_X + 148,
                ry + 13,
                list_w - 280,
                path,
                TEXT_PRIMARY,
                FontSize::Small,
            );
            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

            let bx = MAIN_X + list_w - 116;
            c.round_rect(bx, ry + 6, 102, 34, 6, if selected { BG_BTN_PRIMARY } else { BG_BTN });
            c.round_rect_border(bx, ry + 6, 102, 34, 6, 1, ACCENT_CYAN);
            c.text_centered(bx, ry + 6, 102, 34, "[A] 開く", TEXT_PRIMARY, FontSize::Small);
            if selected {
                self.add_hit(bx, ry + 6, 102, 34, UiAction::Key(Key::A));
            } else {
                self.add_hit(bx, ry + 6, 102, 34, UiAction::SelectRow(idx));
            }
        }

        self.draw_scrollbar(c, ed.cursor, total, visible);
    }

    fn draw_arc_entries(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(arc) = &ed.arc else {
            return;
        };
        let total = arc.entries.len();
        let visible = 10;
        let start = window_start(ed.cursor, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;

        for row in 0..visible {
            let idx = start + row;
            let Some(e) = arc.entries.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 4);
            let selected = idx == ed.cursor;
            let ext = extension_for(e.type_hash).unwrap_or("?");

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            let (type_label, badge_col) = match ext {
                "mib" => ("MIB クエスト条件", ACCENT_GOLD),
                "gmd" => ("GMD テキスト", ACCENT_CYAN),
                "rem" => ("REM 報酬テーブル", ACCENT_GREEN),
                "sup" => ("SUP 支給品", ACCENT_GREEN),
                "sem" => ("SEM 大型配置", ACCENT_ORANGE),
                "esl" => ("ESL 小型配置", ACCENT_PURPLE),
                _ => (ext, TEXT_SECONDARY),
            };
            c.round_rect(MAIN_X + 14, ry + 8, 148, 30, 5, BG_VAL_BOX);
            c.round_rect_border(MAIN_X + 14, ry + 8, 148, 30, 5, 1, badge_col);
            c.text_centered(MAIN_X + 14, ry + 8, 148, 30, type_label, badge_col, FontSize::Small);

            c.text_clipped(
                MAIN_X + 174,
                ry + 11,
                list_w - 400,
                &e.file_name(),
                TEXT_PRIMARY,
                FontSize::Medium,
            );
            c.text_right(
                MAIN_X + list_w - 196,
                ry + 14,
                &format!("{} B", e.data.len()),
                TEXT_MUTED,
                FontSize::Small,
            );

            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

            let is_direct_edit = matches!(ext, "mib" | "gmd");
            let bx = MAIN_X + list_w - 180;
            let (btn_txt, key_act) = if is_direct_edit {
                ("[A] 条件/文を編集", Key::A)
            } else {
                ("[Y] 中身を開く", Key::Y)
            };
            c.round_rect(bx, ry + 6, 166, 34, 6, if selected { BG_BTN_PRIMARY } else { BG_BTN });
            c.round_rect_border(bx, ry + 6, 166, 34, 6, 1, ACCENT_CYAN);
            c.text_centered(bx, ry + 6, 166, 34, btn_txt, TEXT_PRIMARY, FontSize::Small);
            if selected {
                self.add_hit(bx, ry + 6, 166, 34, UiAction::Key(key_act));
            } else {
                self.add_hit(bx, ry + 6, 166, 34, UiAction::SelectRow(idx));
            }
        }

        self.draw_scrollbar(c, ed.cursor, total, visible);
    }

    // --- Screen: Mib (Quest Parameters with Japanese Labels & Notes!) ------
    fn draw_mib(&mut self, c: &mut Canvas, ed: &Editor) {
        let fields = questfields::MIB_FIELDS;
        let total = fields.len();
        let visible = 8;
        let start = window_start(ed.mib_cursor, total, visible);
        let row_h = 46;
        let list_w = MAIN_W - 44;
        let mib = ed.mib_bytes();

        for row in 0..visible {
            let idx = start + row;
            let Some(f) = fields.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 4);
            let selected = idx == ed.mib_cursor;

            let bg = if selected {
                BG_SELECTED
            } else if row % 2 == 0 {
                BG_CARD
            } else {
                BG_CARD_ALT
            };
            c.round_rect(MAIN_X, ry, list_w, row_h, 7, bg);
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                c.round_rect(MAIN_X + 3, ry + 5, 5, row_h - 10, 2, ACCENT_CYAN);
            }

            // Offset badge
            c.round_rect(MAIN_X + 12, ry + 9, 64, 28, 5, BG_VAL_BOX);
            c.text_centered(
                MAIN_X + 12,
                ry + 9,
                64,
                28,
                &format!("+0x{:02X}", f.offset),
                TEXT_CYAN,
                FontSize::Small,
            );

            // Japanese field name (`f.ja`) + English key (`f.key`)
            c.text_clipped(
                MAIN_X + 86,
                ry + 11,
                220,
                f.ja,
                TEXT_PRIMARY,
                FontSize::Medium,
            );
            c.text_clipped(
                MAIN_X + 312,
                ry + 14,
                140,
                f.key,
                TEXT_MUTED,
                FontSize::Small,
            );

            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

            if f.kind == FieldKind::Bytes {
                let a = f.offset.min(mib.len());
                let b = (f.offset + f.size).min(mib.len());
                let hex: String = mib[a..b]
                    .iter()
                    .take(10)
                    .map(|x| format!("{:02X} ", x))
                    .collect();
                c.text_right(
                    MAIN_X + list_w - 18,
                    ry + 14,
                    &hex,
                    TEXT_MUTED,
                    FontSize::Small,
                );
            } else {
                let v = questfields::read_u32(mib, f.offset, f.kind).unwrap_or(0);
                let disp = match f.enum_name.and_then(questfields::enum_table) {
                    Some(t) => format!("{} ({})", t.get(v as usize).copied().unwrap_or("?"), v),
                    None => mib_extra_value_label(f.key, v),
                };
                self.draw_value_stepper(
                    c,
                    MAIN_X + list_w - 406,
                    ry + 6,
                    idx,
                    &disp,
                    selected,
                    false,
                );
            }
        }

        // Field Note / Help Inspector Card at bottom of MIB screen!
        let info_y = CONTENT_Y + visible as i32 * (row_h + 4) + 6;
        let info_h = CONTENT_Y + CONTENT_H - info_y;
        if info_h >= 48 {
            c.round_rect(MAIN_X, info_y, list_w, info_h, 8, rgb(18, 24, 36));
            c.round_rect_border(MAIN_X, info_y, list_w, info_h, 8, 1, ACCENT_GOLD);
            if let Some(cur_f) = fields.get(ed.mib_cursor) {
                c.text_clipped(
                    MAIN_X + 16,
                    info_y + 8,
                    list_w - 32,
                    &format!("項目解説 [{} / {}]:", cur_f.ja, cur_f.key),
                    TEXT_GOLD,
                    FontSize::Small,
                );
                let note = if cur_f.note.is_empty() {
                    "左右キーまたは [-] [+] ボタンで値を変更できます。[A] で保存します。"
                } else {
                    cur_f.note
                };
                c.text_clipped(
                    MAIN_X + 16,
                    info_y + 32,
                    list_w - 32,
                    note,
                    TEXT_SECONDARY,
                    FontSize::Small,
                );
            }
        }

        self.draw_scrollbar(c, ed.mib_cursor, total, visible);
    }

    // --- Screen: Gmd (Quest Text Editor) -----------------------------------
    fn draw_gmd(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(g) = &ed.gmd else {
            return;
        };
        let total = g.strings.len();
        let visible = 8;
        let start = window_start(ed.gmd_cursor, total, visible);
        let row_h = 56;
        let list_w = MAIN_W - 44;

        for row in 0..visible {
            let idx = start + row;
            let Some(s) = g.strings.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 6);
            let selected = idx == ed.gmd_cursor;
            let label = Gmd::LABELS.get(idx).copied().unwrap_or("その他");

            c.round_rect(
                MAIN_X,
                ry,
                list_w,
                row_h,
                8,
                if selected { BG_SELECTED } else { BG_CARD },
            );
            c.round_rect_border(
                MAIN_X,
                ry,
                list_w,
                row_h,
                8,
                if selected { 2 } else { 1 },
                if selected { ACCENT_CYAN } else { BORDER_CARD },
            );

            c.round_rect(MAIN_X + 14, ry + 13, 120, 30, 5, BG_VAL_BOX);
            c.text_centered(MAIN_X + 14, ry + 13, 120, 30, label, TEXT_CYAN, FontSize::Small);

            let one_line = s.replace('\n', " / ").replace('\r', "");
            c.text_clipped(
                MAIN_X + 148,
                ry + 15,
                list_w - 290,
                &one_line,
                TEXT_PRIMARY,
                FontSize::Medium,
            );
            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

            let bx = MAIN_X + list_w - 130;
            c.round_rect(bx, ry + 11, 116, 34, 6, BG_BTN_PRIMARY);
            c.text_centered(bx, ry + 11, 116, 34, "[A] 文字編集", TEXT_PRIMARY, FontSize::Small);
            if selected {
                self.add_hit(bx, ry + 11, 116, 34, UiAction::Key(Key::A));
            } else {
                self.add_hit(bx, ry + 11, 116, 34, UiAction::SelectRow(idx));
            }
        }

        self.draw_scrollbar(c, ed.gmd_cursor, total, visible);
    }

    // --- Screen: EntryView (.rem / .sup / .sem / .esl / .qdp / .qdl) -------
    fn draw_entry_view(&mut self, c: &mut Canvas, ed: &Editor) {
        let Some(entry) = ed.arc.as_ref().and_then(|a| a.entries.get(ed.entry)) else {
            return;
        };
        let ext = extension_for(entry.type_hash).unwrap_or("?");
        let list_w = MAIN_W - 44;

        match ext {
            "rem" | "sup" => {
                if let Ok(t) = ItemTable::parse(&entry.data) {
                    let total = t.records.len();
                    let visible = 10;
                    let start = window_start(ed.entry_row_cursor, total, visible);
                    let row_h = 46;
                    for row in 0..visible {
                        let idx = start + row;
                        let Some(r) = t.records.get(idx) else {
                            break;
                        };
                        let ry = CONTENT_Y + (row as i32) * (row_h + 4);
                        let selected = idx == ed.entry_row_cursor;

                        c.round_rect(
                            MAIN_X,
                            ry,
                            list_w,
                            row_h,
                            7,
                            if selected { BG_SELECTED } else { BG_CARD },
                        );
                        if selected {
                            c.round_rect_border(MAIN_X, ry, list_w, row_h, 7, 2, ACCENT_CYAN);
                        }

                        c.round_rect(MAIN_X + 14, ry + 8, 92, 30, 5, BG_VAL_BOX);
                        c.text_centered(
                            MAIN_X + 14,
                            ry + 8,
                            92,
                            30,
                            &format!("#{:02} 枠{}", idx, r.slot),
                            ACCENT_GREEN,
                            FontSize::Small,
                        );

                        c.text_clipped(
                            MAIN_X + 120,
                            ry + 11,
                            list_w - 280,
                            &format!(
                                "{}  (ID: {})   x{}",
                                item_name_of(r.item),
                                r.item,
                                r.count
                            ),
                            TEXT_PRIMARY,
                            FontSize::Medium,
                        );
                        self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

                        let bx = MAIN_X + list_w - 134;
                        c.round_rect(bx, ry + 6, 120, 34, 6, BG_BTN_PRIMARY);
                        c.text_centered(
                            bx,
                            ry + 6,
                            120,
                            34,
                            "[A] アイテム変更",
                            TEXT_PRIMARY,
                            FontSize::Small,
                        );
                        if selected {
                            self.add_hit(bx, ry + 6, 120, 34, UiAction::Key(Key::A));
                        } else {
                            self.add_hit(bx, ry + 6, 120, 34, UiAction::SelectRow(idx));
                        }
                    }
                    self.draw_scrollbar(c, ed.entry_row_cursor, total, visible);
                }
            }
            "sem" => {
                c.round_rect(MAIN_X, CONTENT_Y, MAIN_W, CONTENT_H, 8, BG_CARD);
                c.text_sized(
                    MAIN_X + 20,
                    CONTENT_Y + 14,
                    "大型モンスター初期配置 (.sem)",
                    ACCENT_ORANGE,
                    FontSize::Medium,
                );
                let mut ry = CONTENT_Y + 54;
                for (i, ch) in entry.data.chunks_exact(32).take(12).enumerate() {
                    let s = SemEntry { raw: ch.to_vec() };
                    let (x, y, z, _) = s.position();
                    c.text_sized(
                        MAIN_X + 24,
                        ry,
                        &format!(
                            "#{:02}   X: {:>8.1}   Y: {:>8.1}   Z: {:>8.1}   ({})",
                            i,
                            x,
                            y,
                            z,
                            if s.is_dummy() { "ダミー" } else { "実体" }
                        ),
                        TEXT_PRIMARY,
                        FontSize::Small,
                    );
                    ry += 32;
                }
            }
            "esl" => {
                c.round_rect(MAIN_X, CONTENT_Y, MAIN_W, 120, 8, BG_CARD);
                if let Ok(l) = Esl::parse(&entry.data) {
                    c.text_sized(
                        MAIN_X + 24,
                        CONTENT_Y + 24,
                        &format!("小型モンスター配置パック (.esl) — ウェーブ数: {}", l.waves),
                        TEXT_PRIMARY,
                        FontSize::Medium,
                    );
                }
            }
            "qdp" => {
                c.round_rect(MAIN_X, CONTENT_Y, MAIN_W, 120, 8, BG_CARD);
                if let Ok(q) = Qdp::parse(&entry.data) {
                    let hex: String = q.flags().iter().map(|b| format!("{:02X} ", b)).collect();
                    c.text_sized(
                        MAIN_X + 24,
                        CONTENT_Y + 24,
                        &format!("クエストフラグ (.qdp): {}", hex),
                        TEXT_PRIMARY,
                        FontSize::Medium,
                    );
                }
            }
            "qdl" => {
                c.round_rect(MAIN_X, CONTENT_Y, MAIN_W, CONTENT_H, 8, BG_CARD);
                if let Ok(q) = QuestLink::parse(&entry.data) {
                    let mut ry = CONTENT_Y + 20;
                    for r in q.refs.iter().take(15) {
                        c.text_sized(
                            MAIN_X + 24,
                            ry,
                            &format!(
                                "[{}]   {}",
                                extension_for(r.type_hash).unwrap_or("?"),
                                r.name
                            ),
                            TEXT_PRIMARY,
                            FontSize::Small,
                        );
                        ry += 30;
                    }
                }
            }
            _ => {
                c.round_rect(MAIN_X, CONTENT_Y, MAIN_W, 100, 8, BG_CARD);
                c.text_sized(
                    MAIN_X + 24,
                    CONTENT_Y + 34,
                    "このエントリは前の画面に戻り [A] キーで編集できます。",
                    TEXT_SECONDARY,
                    FontSize::Medium,
                );
            }
        }
    }

    // --- Screen: Check (Quest Integrity Checker) ---------------------------
    fn draw_check(&mut self, c: &mut Canvas, ed: &Editor) {
        let total = ed.findings.len();
        if total == 0 {
            c.round_rect(MAIN_X, CONTENT_Y, MAIN_W, 120, 10, BG_CARD);
            c.text_centered(
                MAIN_X,
                CONTENT_Y,
                MAIN_W,
                120,
                "✓ 問題は検出されませんでした（クエストデータは正常です）",
                ACCENT_GREEN,
                FontSize::Medium,
            );
            return;
        }
        let visible = 8;
        let start = window_start(ed.check_cursor, total, visible);
        let row_h = 58;
        let list_w = MAIN_W - 44;

        for row in 0..visible {
            let idx = start + row;
            let Some(f) = ed.findings.get(idx) else {
                break;
            };
            let ry = CONTENT_Y + (row as i32) * (row_h + 6);
            let selected = idx == ed.check_cursor;

            c.round_rect(
                MAIN_X,
                ry,
                list_w,
                row_h,
                8,
                if selected { BG_SELECTED } else { BG_CARD },
            );
            if selected {
                c.round_rect_border(MAIN_X, ry, list_w, row_h, 8, 2, ACCENT_CYAN);
            }

            let (tag, col) = match f.severity {
                Severity::Error => ("ERROR", ACCENT_RED),
                Severity::Warn => ("WARN", ACCENT_ORANGE),
                Severity::Info => ("INFO", ACCENT_CYAN),
            };
            c.round_rect(MAIN_X + 14, ry + 14, 76, 30, 5, col);
            c.text_centered(MAIN_X + 14, ry + 14, 76, 30, tag, BG_MAIN, FontSize::Small);

            c.text_clipped(
                MAIN_X + 104,
                ry + 8,
                list_w - 250,
                &f.message,
                TEXT_PRIMARY,
                FontSize::Medium,
            );
            c.text_clipped(
                MAIN_X + 104,
                ry + 34,
                list_w - 250,
                f.code,
                TEXT_MUTED,
                FontSize::Small,
            );

            self.add_row_hit(MAIN_X, ry, list_w, row_h, idx);

            if f.fixable {
                let bx = MAIN_X + list_w - 134;
                c.round_rect(bx, ry + 12, 120, 34, 6, BG_BTN_PRIMARY);
                c.text_centered(
                    bx,
                    ry + 12,
                    120,
                    34,
                    "[A] 自動修正",
                    TEXT_PRIMARY,
                    FontSize::Small,
                );
                self.add_hit(bx, ry + 12, 120, 34, UiAction::Key(Key::A));
            }
        }

        self.draw_scrollbar(c, ed.check_cursor, total, visible);
    }

    // -----------------------------------------------------------------------
    // Shared Widgets: Value Stepper, Mini PM, Scrollbar, Footer
    // -----------------------------------------------------------------------

    fn draw_value_stepper(
        &mut self,
        c: &mut Canvas,
        x: i32,
        y: i32,
        row_idx: usize,
        val_text: &str,
        selected: bool,
        show_max: bool,
    ) {
        let btn_w = 42;
        let h = 34;
        let val_w = if show_max { 220 } else { 250 };

        // [-] button
        c.round_rect(x, y, btn_w, h, 6, BG_BTN);
        c.round_rect_border(x, y, btn_w, h, 6, 1, BORDER_BTN);
        c.text_centered(x, y, btn_w, h, "◀ -", TEXT_CYAN, FontSize::Small);
        self.add_hit(x, y, btn_w, h, UiAction::AdjustRow(row_idx, Key::Left));

        // Value box
        let vx = x + btn_w + 4;
        c.round_rect(
            vx,
            y,
            val_w,
            h,
            6,
            if selected { BG_VAL_ACTIVE } else { BG_VAL_BOX },
        );
        c.round_rect_border(
            vx,
            y,
            val_w,
            h,
            6,
            if selected { 2 } else { 1 },
            if selected { ACCENT_GOLD } else { BORDER_SUBTLE },
        );
        c.text_centered(
            vx,
            y,
            val_w,
            h,
            val_text,
            if selected { TEXT_GOLD } else { TEXT_PRIMARY },
            FontSize::Small,
        );

        // [+] button
        let px = vx + val_w + 4;
        c.round_rect(px, y, btn_w, h, 6, BG_BTN);
        c.round_rect_border(px, y, btn_w, h, 6, 1, BORDER_BTN);
        c.text_centered(px, y, btn_w, h, "+ ▶", TEXT_CYAN, FontSize::Small);
        self.add_hit(px, y, btn_w, h, UiAction::AdjustRow(row_idx, Key::Right));

        // Optional [MAX] button
        if show_max {
            let mx = px + btn_w + 6;
            c.round_rect(mx, y, 56, h, 6, rgb(58, 46, 20));
            c.round_rect_border(mx, y, 56, h, 6, 1, ACCENT_GOLD);
            c.text_centered(mx, y, 56, h, "MAX", TEXT_GOLD, FontSize::Small);
            self.add_hit(mx, y, 56, h, UiAction::SetRowMax(row_idx));
        }
    }

    fn draw_mini_pm_buttons(
        &mut self,
        c: &mut Canvas,
        x: i32,
        y: i32,
        row_idx: usize,
        selected: bool,
    ) {
        let bw = 76;
        let h = 34;
        let bg = if selected { BG_BTN_PRIMARY } else { BG_BTN };
        c.round_rect(x, y, bw, h, 6, bg);
        c.round_rect_border(x, y, bw, h, 6, 1, BORDER_BTN);
        c.text_centered(x, y, bw, h, "◀ 減", TEXT_PRIMARY, FontSize::Small);
        self.add_hit(x, y, bw, h, UiAction::AdjustRow(row_idx, Key::Left));

        c.round_rect(x + bw + 6, y, bw, h, 6, bg);
        c.round_rect_border(x + bw + 6, y, bw, h, 6, 1, BORDER_BTN);
        c.text_centered(x + bw + 6, y, bw, h, "増 ▶", TEXT_PRIMARY, FontSize::Small);
        self.add_hit(x + bw + 6, y, bw, h, UiAction::AdjustRow(row_idx, Key::Right));
    }

    fn draw_scrollbar(&mut self, c: &mut Canvas, cursor: usize, total: usize, visible: usize) {
        let sx = MAIN_X + MAIN_W - 34;
        let sy = CONTENT_Y;
        let sh = CONTENT_H;

        // Page Up button [▲]
        c.round_rect(sx, sy, 34, 40, 6, BG_BTN);
        c.round_rect_border(sx, sy, 34, 40, 6, 1, BORDER_BTN);
        c.text_centered(sx, sy, 34, 40, "▲", TEXT_CYAN, FontSize::Small);
        self.add_hit(sx, sy, 34, 40, UiAction::PageScroll(-(visible as i32)));

        // Page Down button [▼]
        let by = sy + sh - 40;
        c.round_rect(sx, by, 34, 40, 6, BG_BTN);
        c.round_rect_border(sx, by, 34, 40, 6, 1, BORDER_BTN);
        c.text_centered(sx, by, 34, 40, "▼", TEXT_CYAN, FontSize::Small);
        self.add_hit(sx, by, 34, 40, UiAction::PageScroll(visible as i32));

        // Track & Thumb
        let track_y = sy + 46;
        let track_h = (sh - 92).max(20);
        c.round_rect(sx + 11, track_y, 12, track_h, 6, BG_VAL_BOX);

        if total > 0 {
            let thumb_h = ((visible as i32 * track_h) / (total.max(visible) as i32)).clamp(24, track_h);
            let max_off = track_h - thumb_h;
            let thumb_y = if total <= 1 {
                track_y
            } else {
                track_y + ((cursor.min(total - 1) as i32) * max_off) / ((total - 1) as i32)
            };
            c.round_rect(sx + 11, thumb_y, 12, thumb_h, 6, ACCENT_CYAN);
        }
    }

    // -----------------------------------------------------------------------
    // 5. Bottom Status & Interactive Action Bar
    // -----------------------------------------------------------------------
    fn draw_footer(&mut self, c: &mut Canvas, ed: &Editor) {
        let w = c.width();
        c.rect(0, FOOTER_Y, w, FOOTER_H, BG_FOOTER);
        c.hline(0, FOOTER_Y, w, BORDER_CARD);

        // Status message line
        c.circle(20, FOOTER_Y + 16, 4, ACCENT_GOLD);
        c.text_clipped(
            32,
            FOOTER_Y + 6,
            w - 48,
            &ed.message,
            TEXT_GOLD,
            FontSize::Small,
        );

        // Interactive Button Chips
        let mut x = 14;
        let cy = FOOTER_Y + 34;
        let ch = 34;

        for &(k_str, label) in crate::editor::actions(ed.screen) {
            let badge_col = match k_str {
                "A" => ACCENT_GREEN,
                "B" => ACCENT_RED,
                "X" => rgb(56, 148, 232),
                "Y" => ACCENT_ORANGE,
                _ => rgb(88, 106, 138),
            };
            let key_w = (k_str.chars().count() as i32) * 10 + 18;
            let lbl_w = c.text_width_sized(label, FontSize::Small);
            let chip_w = key_w + lbl_w + 22;

            c.round_rect(x, cy, chip_w, ch, 8, BG_BTN);
            c.round_rect_border(x, cy, chip_w, ch, 8, 1, BORDER_BTN);

            // Key pill inside chip
            c.round_rect(x + 4, cy + 4, key_w, ch - 8, 6, badge_col);
            c.text_centered(
                x + 4,
                cy + 4,
                key_w,
                ch - 8,
                k_str,
                white_or_dark(badge_col),
                FontSize::Small,
            );

            // Label
            c.text_sized(
                x + key_w + 10,
                cy + 7,
                label,
                TEXT_PRIMARY,
                FontSize::Small,
            );

            if let Some(key_enum) = parse_chip_key(k_str) {
                self.add_hit(x, cy, chip_w, ch, UiAction::Key(key_enum));
            }

            x += chip_w + 8;
            if x > w - 80 {
                break;
            }
        }
    }
}

#[inline]
const fn white_or_dark(_c: Color) -> Color {
    rgb(255, 255, 255)
}

fn parse_chip_key(k: &str) -> Option<Key> {
    match k {
        "A" => Some(Key::A),
        "B" => Some(Key::B),
        "X" => Some(Key::X),
        "Y" => Some(Key::Y),
        "L" => Some(Key::L),
        "R" => Some(Key::R),
        "-" => Some(Key::Minus),
        "+" => Some(Key::Plus),
        "Z" => Some(Key::Z),
        "</>" => Some(Key::Right),
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

/// Provide human-readable monster / item names for MIB target ID fields.
fn mib_extra_value_label(key: &str, v: u32) -> String {
    match key {
        "com0" | "com1" | "com2" | "com3" | "com4" | "com5" | "com6" | "com7" | "com8" | "com9" => {
            if v == 0 {
                String::from("0 (なし)")
            } else if (v as usize) < mhnx_core::questfields::ENUM_MONS_.len() {
                format!(
                    "{} ({})",
                    v,
                    mhnx_core::questfields::ENUM_MONS_[v as usize]
                )
            } else {
                format!("{} ({})", v, item_name_of(v as u16))
            }
        }
        "time_limit" => format!("{} 分", v),
        "z1" | "z2" | "z3" => format!("{} z", v),
        _ => format!("{}", v),
    }
}

// ---------------------------------------------------------------------------
// Built-in On-Screen Keyboard (OSK) for Applet Mode
// ---------------------------------------------------------------------------

const OSK_TAB_NAMES: [&str; 4] = [
    "ひらがな",
    "カタカナ",
    "英数字・記号",
    "よく使う漢字・検索語",
];

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
        text => osk.buffer.push_str(text),
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

impl GuiState {
    fn draw_osk(&mut self, c: &mut Canvas) {
        let Some(osk) = self.osk.clone() else { return };

        // Full-screen darkened backdrop
        let mx = 100;
        let my = 44;
        let mw = 1080;
        let mh = 632;
        c.round_rect(mx, my, mw, mh, 14, rgb(14, 18, 28));
        c.round_rect_border(mx, my, mw, mh, 14, 2, ACCENT_CYAN);

        // Title & Applet Mode Note
        c.text_sized(
            mx + 24,
            my + 16,
            &format!("文字入力: {}", osk.header),
            TEXT_PRIMARY,
            FontSize::Large,
        );
        c.text_sized(
            mx + 560,
            my + 22,
            "※タッチ または 十字キー+A で文字を入力し [+] で決定",
            TEXT_CYAN,
            FontSize::Small,
        );

        // Text Input Box
        let ib_x = mx + 24;
        let ib_y = my + 58;
        let ib_w = mw - 48;
        let ib_h = 52;
        c.round_rect(ib_x, ib_y, ib_w, ib_h, 8, BG_VAL_BOX);
        c.round_rect_border(ib_x, ib_y, ib_w, ib_h, 8, 2, ACCENT_GOLD);
        let disp_text = format!("{}|", osk.buffer);
        c.text_sized(ib_x + 16, ib_y + 12, &disp_text, TEXT_GOLD, FontSize::Large);

        // Keyboard Mode Tabs (L / R or Touch)
        let tab_y = my + 122;
        let tab_w = 240;
        let tab_h = 40;
        for (ti, &tname) in OSK_TAB_NAMES.iter().enumerate() {
            let tx = ib_x + (ti as i32) * (tab_w + 12);
            let active = osk.tab == ti;
            c.round_rect(
                tx,
                tab_y,
                tab_w,
                tab_h,
                7,
                if active { BG_SELECTED } else { BG_CARD },
            );
            c.round_rect_border(
                tx,
                tab_y,
                tab_w,
                tab_h,
                7,
                if active { 2 } else { 1 },
                if active { ACCENT_CYAN } else { BORDER_BTN },
            );
            c.text_centered(
                tx,
                tab_y,
                tab_w,
                tab_h,
                tname,
                if active { TEXT_CYAN } else { TEXT_SECONDARY },
                FontSize::Medium,
            );
            self.add_hit(tx, tab_y, tab_w, tab_h, UiAction::OskTab(ti));
        }

        // Character Grid (10 columns x 5 rows)
        let grid = osk_grid_for(osk.tab);
        let gx0 = ib_x;
        let gy0 = my + 174;
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
                    BG_SELECTED
                } else if is_mod {
                    BG_BTN_PRIMARY
                } else {
                    BG_CARD
                };
                let border = if sel {
                    ACCENT_GOLD
                } else if is_mod {
                    ACCENT_CYAN
                } else {
                    BORDER_CARD
                };
                c.round_rect(cx, cy, cell_w, cell_h, 8, bg);
                c.round_rect_border(cx, cy, cell_w, cell_h, 8, if sel { 2 } else { 1 }, border);
                c.text_centered(
                    cx,
                    cy,
                    cell_w,
                    cell_h,
                    label,
                    if sel {
                        TEXT_GOLD
                    } else if is_mod {
                        TEXT_CYAN
                    } else {
                        TEXT_PRIMARY
                    },
                    if osk.tab == 3 {
                        FontSize::Medium
                    } else {
                        FontSize::Large
                    },
                );
                self.add_hit(cx, cy, cell_w, cell_h, UiAction::OskCell(r, col));
            }
        }

        // Bottom Action Buttons Row
        let by = my + 560;
        let bh = 52;
        let actions: &[(&str, i32, Color, Color, UiAction)] = &[
            ("[B] 1文字消す", 160, BG_BTN, BORDER_BTN, UiAction::OskBackspace),
            ("[X] 全消去", 140, BG_BTN_WARN, ACCENT_RED, UiAction::OskClear),
            (
                "[Y] 濁点/小文字",
                176,
                BG_BTN_PRIMARY,
                ACCENT_CYAN,
                UiAction::OskModify,
            ),
            ("[空白]", 110, BG_BTN, BORDER_BTN, UiAction::OskSpace),
            (
                "[-] キャンセル",
                170,
                BG_BTN,
                BORDER_BTN,
                UiAction::OskCancel,
            ),
            (
                "[+] 決定 (OK)",
                216,
                rgb(22, 92, 64),
                ACCENT_GREEN,
                UiAction::OskConfirm,
            ),
        ];
        let mut bx = ib_x;
        for &(label, bw, bg, border, act) in actions {
            c.round_rect(bx, by, bw, bh, 8, bg);
            c.round_rect_border(bx, by, bw, bh, 8, 2, border);
            c.text_centered(bx, by, bw, bh, label, TEXT_PRIMARY, FontSize::Medium);
            self.add_hit(bx, by, bw, bh, act);
            bx += bw + 12;
        }
    }
}
