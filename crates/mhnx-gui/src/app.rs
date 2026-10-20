//! Application state and UI.

use std::path::PathBuf;

use eframe::egui;
use egui_extras::{Column, TableBuilder};

use mhnx_core::arc::{extension_for, Arc};
use mhnx_core::check::{self, Finding, Severity};
use mhnx_core::dlc;
use mhnx_core::itembox::ItemSlot;
use mhnx_core::quest::{Gmd, Mib};
use mhnx_core::questfields::FieldKind;
use mhnx_core::save::{Character, Save, SaveKind, BODY_SIZE, SWITCH_SIZE};

/// Which page is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Save,
    Items,
    Quest,
    Dlc,
    Charm,
    Extras,
    About,
}

/// The whole application.
pub struct MhnxEditor {
    pub tab: Tab,
    pub status: String,
    pub message: String,

    // ---- save ----
    pub save_path: Option<PathBuf>,
    pub save: Option<Save>,
    pub dirty: bool,
    pub slot: usize,
    pub edit: Character,
    pub items: Vec<ItemSlot>,
    pub item_filter: String,
    pub item_selected: Option<usize>,
    pub item_show_empty: bool,

    // ---- quest ----
    pub arc_path: Option<PathBuf>,
    pub arc: Option<Arc>,
    pub arc_selected: usize,
    pub arc_filter: String,
    /// Quest id the open archive was pulled out of the save with, if any.
    ///
    /// When this is set the quest tab can write straight back into the save,
    /// so the whole edit loop happens without touching a file on disk.
    pub arc_from_save: Option<i32>,
    pub mib: Option<Mib>,
    /// Editable copy of the 249 MIB bytes.
    pub mib_bytes: Vec<u8>,
    pub gmd: Option<Gmd>,
    /// Problems found by the quest checker.
    pub findings: Vec<Finding>,
    /// Download Palico installer: 0 = Japanese, 1 = English.
    pub cat_lang: usize,
    /// Download Palico installer: index into the id list.
    pub cat_id: usize,
    /// Free-text filter used by the item picker.
    pub item_pick_filter: String,
    /// Free-text filter for the installed quest list (id or title).
    pub dlc_filter: String,
    /// Output folder for the quest extractor.
    pub dlc_outdir: String,
    /// Snapshots for 元に戻す, newest last. Bounded so a long session cannot
    /// grow without limit; one item box is 2242 entries.
    pub item_undo: Vec<Vec<ItemSlot>>,
    /// Keep the picker filter after choosing, so several slots can be filled
    /// from one search.
    pub item_pick_keep_filter: bool,
    /// Row selected inside the .rem / .sup reward table.
    pub entry_item_sel: Option<usize>,
    /// Free-text filter for the talisman list.
    pub charm_filter: String,
    /// Search text inside the two skill pickers.
    pub skill_search1: String,
    pub skill_search2: String,
    /// Charm editor fields.
    pub charm_index: usize,
    pub charm_skill1: u8,
    pub charm_points1: i8,
    pub charm_skill2: u8,
    pub charm_points2: i8,
    pub charm_slots: u8,

    // ---- dlc ----
    pub dlc_files: Vec<PathBuf>,
    pub dlc_log: Vec<String>,

    // ---- headless screenshot / smoke test mode ----
    /// When set, the app renders a few frames, saves a PNG there and exits.
    pub screenshot_path: Option<PathBuf>,
    pub screenshot_frames: u32,
    pub screenshot_requested: bool,
}

impl Default for MhnxEditor {
    fn default() -> Self {
        MhnxEditor {
            tab: Tab::Save,
            status: String::new(),
            message: String::new(),
            save_path: None,
            save: None,
            dirty: false,
            slot: 0,
            edit: Character::default(),
            items: Vec::new(),
            item_filter: String::new(),
            item_selected: None,
            item_show_empty: false,
            arc_path: None,
            arc: None,
            arc_selected: 0,
            arc_filter: String::new(),
            arc_from_save: None,
            mib: None,
            mib_bytes: Vec::new(),
            gmd: None,
            findings: Vec::new(),
            cat_lang: 0,
            cat_id: 0,
            item_pick_filter: String::new(),
            dlc_filter: String::new(),
            dlc_outdir: String::new(),
            item_undo: Vec::new(),
            item_pick_keep_filter: true,
            entry_item_sel: None,
            charm_filter: String::new(),
            skill_search1: String::new(),
            skill_search2: String::new(),
            charm_index: 0,
            charm_skill1: 1,
            charm_points1: 1,
            charm_skill2: 0,
            charm_points2: 0,
            charm_slots: 0,
            dlc_files: Vec::new(),
            dlc_log: Vec::new(),
            screenshot_path: None,
            screenshot_frames: 0,
            screenshot_requested: false,
        }
    }
}

impl MhnxEditor {
    // ---------------------------------------------------------------- save
    pub fn open_save(&mut self, path: PathBuf) {
        match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| {
            Save::from_bytes(&b).map_err(|e| e.to_string())
        }) {
            Ok(s) => {
                self.save = Some(s);
                self.save_path = Some(path.clone());
                self.dirty = false;
                self.message = format!("読み込みました: {}", path.display());
                self.reload_from_save();
            }
            Err(e) => self.message = format!("読み込み失敗: {}", e),
        }
    }

    /// Reload the working copies (character + item box) from the save.
    pub fn reload_from_save(&mut self) {
        self.items.clear();
        self.item_selected = None;
        let Some(s) = &self.save else { return };
        if let Some(c) = s.read_character(self.slot) {
            self.edit = c;
        } else {
            self.edit = Character {
                slot: self.slot,
                ..Default::default()
            };
        }
        if let Some(items) = s.read_item_box(self.slot) {
            self.items = items;
        }
    }

    /// Push the working copies back into the save.
    pub fn apply_to_save(&mut self) {
        let Some(s) = &mut self.save else { return };
        self.edit.slot = self.slot;
        if !s.slot_used(self.slot) {
            s.set_slot_used(self.slot, true);
        }
        if let Err(e) = s.write_character(&self.edit) {
            self.message = format!("適用失敗: {}", e);
            return;
        }
        if !self.items.is_empty() {
            if let Err(e) = s.write_item_box(self.slot, &self.items) {
                self.message = format!("アイテム適用失敗: {}", e);
                return;
            }
        }
        self.dirty = true;
        self.message = "編集内容を保存データに反映しました (まだファイルには書いていません)".to_owned();
    }

    pub fn write_current(&mut self, path: PathBuf) {
        self.apply_to_save();
        let Some(s) = &self.save else { return };
        match std::fs::write(&path, s.to_bytes()) {
            Ok(_) => {
                self.dirty = false;
                self.save_path = Some(path.clone());
                self.message = format!("書き出しました: {}", path.display());
            }
            Err(e) => self.message = format!("書き出し失敗: {}", e),
        }
    }

    /// Write a Switch save as both system and system_backup into a directory.
    pub fn write_switch_pair(&mut self, dir: PathBuf) {
        self.apply_to_save();
        let Some(s) = &self.save else { return };
        if s.kind() == SaveKind::ThreeDs {
            self.message = "3DS セーブのままです。先に Switch 形式へ変換してください".to_owned();
            return;
        }
        let body = s.body().to_vec();
        let sys = mhnx_core::save::switch_header("system", body.len());
        let bak = mhnx_core::save::switch_header("system_backup", body.len());
        let mut a = sys.to_vec();
        a.extend_from_slice(&body);
        let mut b = bak.to_vec();
        b.extend_from_slice(&body);
        let r1 = std::fs::write(dir.join("system"), &a);
        let r2 = std::fs::write(dir.join("system_backup"), &b);
        self.message = match (r1, r2) {
            (Ok(_), Ok(_)) => format!("system と system_backup を書き出しました: {}", dir.display()),
            _ => "書き出しに失敗しました".to_owned(),
        };
    }

    /// Export the body as a 3DS save.
    pub fn export_3ds(&mut self, path: PathBuf) {
        self.apply_to_save();
        let Some(s) = &self.save else { return };
        match std::fs::write(&path, s.to_3ds_bytes()) {
            Ok(_) => self.message = format!("3DS 形式で書き出しました: {}", path.display()),
            Err(e) => self.message = format!("書き出し失敗: {}", e),
        }
    }

    /// Import a 3DS body into the currently open Switch save.
    pub fn import_3ds_body(&mut self, path: PathBuf) {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                self.message = format!("読み込み失敗: {}", e);
                return;
            }
        };
        let Some(s) = &mut self.save else {
            self.message = "先に Switch セーブを開いてください".to_owned();
            return;
        };
        match s.replace_body(&bytes) {
            Ok(_) => {
                self.dirty = true;
                self.message = "3DS の本体データを取り込みました".to_owned();
                self.reload_from_save();
            }
            Err(e) => self.message = format!("取り込み失敗: {}", e),
        }
    }

    /// Turn the current 3DS save into a Switch save (header is synthesised).
    pub fn convert_to_switch(&mut self) {
        let Some(s) = &self.save else { return };
        if s.kind() != SaveKind::ThreeDs {
            self.message = "すでに Switch 形式です".to_owned();
            return;
        }
        match Save::from_3ds_body(s.body(), "system") {
            Ok(ns) => {
                self.save = Some(ns);
                self.dirty = true;
                self.message = "Switch 形式に変換しました (ヘッダは計算で生成)".to_owned();
            }
            Err(e) => self.message = format!("変換失敗: {}", e),
        }
    }

    // --------------------------------------------------------------- quest
    pub fn open_arc(&mut self, path: PathBuf) {
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| Arc::parse(&b).map_err(|e| e.to_string()))
        {
            Ok(a) => {
                // Prefer the quest parameter record, then the text container.
                let idx = a
                    .entries
                    .iter()
                    .position(|e| extension_for(e.type_hash) == Some("mib"))
                    .or_else(|| a.entries.iter().position(|e| extension_for(e.type_hash) == Some("gmd")))
                    .unwrap_or(0);
                self.arc = Some(a);
                self.arc_path = Some(path.clone());
                self.arc_from_save = None;
                self.arc_selected = idx;
                self.select_arc_entry(idx);
                self.run_quest_check();
                self.message = format!("クエストを読み込みました: {}", path.display());
            }
            Err(e) => self.message = format!("読み込み失敗: {}", e),
        }
    }

    /// Open one of the save's own download quests in the quest editor.
    ///
    /// The arc is copied out of the save into memory, so the editor works on a
    /// copy and nothing is written until the user asks for it. Returns true
    /// when the archive opened.
    pub fn open_arc_from_save(&mut self, quest_id: i32) -> bool {
        let Some(save) = &self.save else {
            self.message = "先にセーブを開いてください".to_owned();
            return false;
        };
        let Some(q) = dlc::list_quests(save)
            .into_iter()
            .find(|q| q.quest_id == quest_id)
        else {
            self.message = format!("ID {} のクエストがこのセーブにありません", quest_id);
            return false;
        };
        if q.data.is_empty() {
            self.message = format!("ID {} のデータが壊れています（サイズ {}）", quest_id, q.size);
            return false;
        }
        match Arc::parse(&q.data) {
            Ok(a) => {
                let idx = a
                    .entries
                    .iter()
                    .position(|e| extension_for(e.type_hash) == Some("mib"))
                    .or_else(|| a.entries.iter().position(|e| extension_for(e.type_hash) == Some("gmd")))
                    .unwrap_or(0);
                self.arc = Some(a);
                // No path: this arc lives in the save, not on disk.
                self.arc_path = None;
                self.arc_from_save = Some(quest_id);
                self.arc_selected = idx;
                self.select_arc_entry(idx);
                self.run_quest_check();
                self.tab = Tab::Quest;
                self.message = format!(
                    "セーブのクエスト {} を開きました（{} バイト）",
                    quest_id,
                    q.data.len()
                );
                true
            }
            Err(e) => {
                self.message = format!("ID {} は .arc として読めません: {}", quest_id, e);
                false
            }
        }
    }

    /// Rebuild the open archive and write it back into the save.
    ///
    /// Only works for an archive that came out of the save, and only for a
    /// quest the save already holds: adding one would need its catalogue flag,
    /// which a slot number cannot address.
    pub fn write_arc_to_save(&mut self) {
        let Some(id) = self.arc_from_save else {
            self.message = "セーブから開いたクエストではありません".to_owned();
            return;
        };
        self.store_arc_entry(self.arc_selected);
        let bytes = match self.arc.as_ref().map(|a| a.to_bytes()) {
            Some(Ok(b)) => b,
            Some(Err(e)) => {
                self.message = format!("再構築失敗: {}", e);
                return;
            }
            None => return,
        };
        let Some(save) = &mut self.save else {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        };
        match dlc::install_quest(save, id, &bytes) {
            Ok(slot) => {
                self.dirty = true;
                self.message = format!(
                    "クエスト {} を枠 {} に書き戻しました（{} バイト）",
                    id,
                    slot,
                    bytes.len()
                );
            }
            Err(e) => self.message = format!("書き戻し失敗: {}", e),
        }
    }

    pub fn select_arc_entry(&mut self, index: usize) {
        // Flush the buffer of the entry being left first: mib_bytes and gmd
        // hold the live edits, so switching entries without this threw away
        // whatever the user had typed in the previous one.
        let prev = self.arc_selected;
        if self.arc.is_some() && prev != index {
            self.store_arc_entry(prev);
        }
        self.mib = None;
        self.gmd = None;
        self.mib_bytes.clear();
        let Some(a) = &self.arc else { return };
        let Some(e) = a.entries.get(index) else { return };
        match extension_for(e.type_hash) {
            Some("mib") => {
                if let Ok(m) = Mib::from_bytes(&e.data) {
                    self.mib_bytes = m.raw.clone();
                    self.mib = Some(m);
                }
            }
            Some("gmd") => {
                if let Ok(g) = Gmd::from_bytes(&e.data) {
                    self.gmd = Some(g);
                }
            }
            _ => {}
        }
    }

    pub fn store_arc_entry(&mut self, index: usize) {
        if let Some(m) = &mut self.mib {
            if self.mib_bytes.len() == m.raw.len() {
                m.raw.copy_from_slice(&self.mib_bytes);
            }
        }
        let mib_new = self.mib.as_ref().map(|m| m.raw.clone());
        let gmd_new = self.gmd.as_ref().map(|g| g.to_bytes());
        let Some(a) = &mut self.arc else { return };
        let Some(e) = a.entries.get_mut(index) else { return };
        if let Some(d) = mib_new {
            e.data = d;
        }
        if let Some(d) = gmd_new {
            e.data = d;
        }
    }

    pub fn save_arc(&mut self, path: PathBuf) {
        self.store_arc_entry(self.arc_selected);
        let Some(a) = &self.arc else { return };
        match a.to_bytes() {
            Ok(bytes) => match std::fs::write(&path, &bytes) {
                Ok(_) => self.message = format!(".arc を書き出しました: {}", path.display()),
                Err(e) => self.message = format!("書き出し失敗: {}", e),
            },
            Err(e) => self.message = format!("再構築失敗: {}", e),
        }
    }

    /// Collect every .arc in a folder.
    pub fn scan_quest_dir(&mut self, dir: PathBuf) {
        let mut v = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("arc") {
                    v.push(p);
                }
            }
        }
        v.sort();
        self.message = format!("{} 件の .arc を検出しました", v.len());
        self.dlc_files = v;
    }

    /// Install the selected .arc files into the open save.
    pub fn install_dlc(&mut self) {
        let files = self.dlc_files.clone();
        let Some(save) = &mut self.save else {
            self.message = "先にセーブを開いてください".to_owned();
            return;
        };
        self.dlc_log.clear();
        let mut ok = 0usize;
        for f in &files {
            let id = match f
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_prefix('q'))
                .and_then(|s| s.parse::<i32>().ok())
            {
                Some(v) => v,
                None => continue,
            };
            match std::fs::read(f) {
                Ok(bytes) => match dlc::install_quest(save, id, &bytes) {
                    Ok(slot) => {
                        self.dlc_log.push(format!("q{} -> スロット {}", id, slot));
                        ok += 1;
                    }
                    Err(e) => self.dlc_log.push(format!("q{}: {}", id, e)),
                },
                Err(e) => self.dlc_log.push(format!("q{}: {}", id, e)),
            }
        }
        self.dirty = true;
        self.message = format!("{} 件をインストールしました", ok);
    }

    /// Write quest arcs from the save back out as files.
    ///
    /// With an empty id list every quest is written. The payload is copied byte
    /// for byte: the save already holds a finished arc, so nothing is re-packed.
    /// Returns the number of files written.
    pub fn extract_dlc(&mut self, outdir: &str, ids: &[i32]) -> usize {
        let Some(save) = &self.save else {
            self.message = "先にセーブを開いてください".to_owned();
            return 0;
        };
        let list = dlc::list_quests(save);
        if list.is_empty() {
            self.message = "このセーブに配信クエストはありません".to_owned();
            return 0;
        }
        let dir = std::path::Path::new(outdir);
        if let Err(e) = std::fs::create_dir_all(dir) {
            self.message = format!("出力先を作れません: {}", e);
            return 0;
        }
        let mut written = 0usize;
        let mut missing: Vec<i32> = ids.to_vec();
        for q in &list {
            if !ids.is_empty() && !ids.contains(&q.quest_id) {
                continue;
            }
            if let Some(i) = missing.iter().position(|x| *x == q.quest_id) {
                missing.remove(i);
            }
            if q.data.is_empty() {
                continue;
            }
            let path = dir.join(format!("q{:07}.arc", q.quest_id));
            if std::fs::write(&path, &q.data).is_ok() {
                written += 1;
            }
        }
        self.message = if missing.is_empty() {
            format!("{} 件を {} に書き出しました", written, outdir)
        } else {
            format!(
                "{} 件を書き出しました。見つからない ID: {}",
                written,
                missing.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(", ")
            )
        };
        written
    }

    /// Zero one quest slot.
    ///
    /// This is a one-way door: the record disappears but the installed flag
    /// lives in a catalogue indexed set that a slot number cannot address, so
    /// the flag stays set and the tool can never put the quest back. The UI
    /// asks for confirmation and says so.
    pub fn remove_dlc(&mut self, slot: usize) {
        let Some(save) = &mut self.save else { return };
        match dlc::uninstall_quest(save, slot) {
            Ok(_) => {
                self.dirty = true;
                self.message = format!(
                    "スロット {} を空にしました（元に戻せません。ゲーム側には空の項目が残る可能性があります）",
                    slot
                );
            }
            Err(e) => self.message = format!("削除失敗: {}", e),
        }
    }
}

// ------------------------------------------------------------------ drawing

impl eframe::App for MhnxEditor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.top_bar(ctx);
        self.side_panel(ctx);
        self.bottom_bar(ctx);

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Save => self.tab_save(ui),
            Tab::Items => self.tab_items(ui),
            Tab::Quest => self.tab_quest(ui),
            Tab::Dlc => self.tab_dlc(ui),
            Tab::Charm => self.tab_charm(ui),
            Tab::Extras => self.tab_extras(ui),
            Tab::About => self.tab_about(ui),
        });

        self.drive_screenshot(ctx);
    }
}

impl MhnxEditor {
    /// Headless verification hook.
    ///
    /// When a screenshot path is configured the app renders a few frames,
    /// asks egui for a framebuffer copy, writes it as a PNG and quits. This
    /// gives an automated smoke test for the whole UI: if any tab panics the
    /// PNG is never produced.
    fn drive_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot_path.clone() else {
            return;
        };
        self.screenshot_frames += 1;
        if self.screenshot_frames >= 3 && !self.screenshot_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.screenshot_requested = true;
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot {
            let (w, h) = (img.size[0] as u32, img.size[1] as u32);
            let raw: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
            match image::RgbaImage::from_raw(w, h, raw) {
                Some(buf) => match buf.save(&path) {
                    Ok(_) => eprintln!("screenshot written to {}", path.display()),
                    Err(e) => eprintln!("screenshot save failed: {}", e),
                },
                None => eprintln!("screenshot buffer mismatch"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl MhnxEditor {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("mhnxeditor");
                ui.separator();
                if ui.button("セーブを開く").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("MHXX save (system)", &["system", "sav", "*"])
                        .set_title("system ファイルを選択")
                        .pick_file()
                    {
                        self.open_save(p);
                    }
                }
                let has_save = self.save.is_some();
                if ui.add_enabled(has_save, egui::Button::new("上書き保存")).clicked() {
                    if let Some(p) = self.save_path.clone() {
                        self.write_current(p);
                    }
                }
                if ui.add_enabled(has_save, egui::Button::new("名前を付けて保存")).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_file_name("system")
                        .set_title("保存先")
                        .save_file()
                    {
                        self.write_current(p);
                    }
                }
                if ui.add_enabled(has_save, egui::Button::new("Switch 用 (system 2 ファイル)")).clicked() {
                    if let Some(d) = rfd::FileDialog::new().set_title("出力フォルダ").pick_folder() {
                        self.write_switch_pair(d);
                    }
                }
                if ui.add_enabled(has_save, egui::Button::new("3DS 形式で書き出す")).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .set_file_name("system")
                        .set_title("3DS 形式の保存先")
                        .save_file()
                    {
                        self.export_3ds(p);
                    }
                }
                ui.separator();
                if ui.button(".arc を開く").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("quest archive", &["arc"])
                        .set_title("クエスト .arc を選択")
                        .pick_file()
                    {
                        self.open_arc(p);
                    }
                }
            });

            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("3DS <-> Switch 変換").strong());
                let is_3ds = self.save.as_ref().map(|s| s.kind() == SaveKind::ThreeDs).unwrap_or(false);
                let is_switch = self
                    .save
                    .as_ref()
                    .map(|s| s.kind() != SaveKind::ThreeDs)
                    .unwrap_or(false);

                if ui
                    .add_enabled(is_3ds, egui::Button::new("3DS から Switch へ変換 (ヘッダを合成)"))
                    .on_hover_text(
                        "36 バイトのヘッダは CRC32(ファイル名) から計算できるため、実機で作った空セーブは不要です",
                    )
                    .clicked()
                {
                    self.convert_to_switch();
                }
                if ui
                    .add_enabled(
                        is_switch,
                        egui::Button::new("Switch セーブに 3DS の本体を取り込む"),
                    )
                    .on_hover_text("ヘッダはそのままに、本体 4,726,152 バイトだけを差し替えます")
                    .clicked()
                {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("3DS save (system)", &["system", "sav", "*"])
                        .set_title("3DS の system を選択")
                        .pick_file()
                    {
                        self.import_3ds_body(p);
                    }
                }
                ui.separator();
                ui.label(
                    egui::RichText::new("ヒント: 編集後は「Switch 用 (system 2 ファイル)」で system と system_backup を出力できます")
                        .weak()
                        .small(),
                );
            });
        });
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("side")
            .resizable(true)
            .default_width(340.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.tab, Tab::Save, "キャラ");
                        ui.selectable_value(&mut self.tab, Tab::Items, "アイテム");
                        ui.selectable_value(&mut self.tab, Tab::Quest, "クエスト");
                        ui.selectable_value(&mut self.tab, Tab::Dlc, "DLC");
                        ui.selectable_value(&mut self.tab, Tab::Charm, "お守り");
                ui.selectable_value(&mut self.tab, Tab::Extras, "設定/オトモ");
        ui.selectable_value(&mut self.tab, Tab::About, "情報");
                    });
                    ui.separator();

                    ui.label(egui::RichText::new("セーブ情報").strong());
                    let mut pending_slot: Option<usize> = None;
                    match &self.save {
                        None => {
                            ui.label("まだセーブを開いていません");
                            ui.label("Switch: system / system_backup (4,726,188 B)");
                            ui.label("3DS: system (4,726,152 B)");
                        }
                        Some(s) => {
                            let kind = match s.kind() {
                                SaveKind::ThreeDs => "3DS (MHXX)",
                                SaveKind::Switch => "Switch (MHXX)",
                                SaveKind::Mhgu => "Switch (MHGU)",
                            };
                            egui::Grid::new("saveinfo").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
                                ui.label("形式");
                                ui.label(kind);
                                ui.end_row();
                                ui.label("ファイルサイズ");
                                ui.label(format!("{} B", s.to_bytes().len()));
                                ui.end_row();
                                ui.label("本体サイズ");
                                ui.label(format!("{} B", s.body().len()));
                                ui.end_row();
                                ui.label("DLC セクション");
                                ui.label(format!("0x{:X}", s.dlc_section_offset()));
                                ui.end_row();
                                for (i, b) in s.slot_bases().iter().enumerate() {
                                    ui.label(format!("スロット{} 基点", i + 1));
                                    ui.label(format!("0x{:X} ({})", b, b));
                                    ui.end_row();
                                }
                                ui.label("ヘッダ +0x14");
                                ui.label(format!("0x{:08X}", u32::from_le_bytes([
                                    s.header()[0x14], s.header()[0x15], s.header()[0x16], s.header()[0x17]
                                ])));
                                ui.end_row();
                            });
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("保存スロット").strong());
                            for i in 0..3 {
                                let used = s.slot_used(i);
                                let name = s
                                    .read_character(i)
                                    .map(|c| c.name)
                                    .unwrap_or_default();
                                let label = format!(
                                    "{}: {}{}",
                                    i + 1,
                                    if name.is_empty() { "(空)" } else { name.as_str() },
                                    if used { "" } else { " [未使用]" }
                                );
                                if ui.selectable_label(self.slot == i, label).clicked() {
                                    pending_slot = Some(i);
                                }
                            }
                        }
                    }
                    if let Some(i) = pending_slot {
                        // Commit the edits first: reload_from_save() rebuilds
                        // the character, item box and talisman buffers from the
                        // save, so anything not applied yet was silently lost.
                        if self.dirty {
                            self.apply_to_save();
                        }
                        self.slot = i;
                        self.reload_from_save();
                        self.message = format!("スロット {} に切り替えました", i + 1);
                    }

                    ui.separator();
                    ui.label(egui::RichText::new("クエスト情報").strong());
                    match &self.arc {
                        None => {
                            ui.label("まだ .arc を開いていません");
                        }
                        Some(a) => {
                            if let Some(p) = &self.arc_path {
                                ui.label(p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
                            }
                            egui::Grid::new("arcinfo").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
                                ui.label("バージョン");
                                ui.label(format!("{} (0x{:X})", a.version, a.version));
                                ui.end_row();
                                ui.label("エントリ数");
                                ui.label(format!("{}", a.entries.len()));
                                ui.end_row();
                                ui.label("合計 (展開後)");
                                ui.label(format!("{} B", a.entries.iter().map(|e| e.data.len()).sum::<usize>()));
                                ui.end_row();
                            });
                        }
                    }
                });
            });
    }

    fn bottom_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if self.dirty {
                    ui.label(egui::RichText::new("● 未保存").color(egui::Color32::from_rgb(240, 180, 60)));
                } else {
                    ui.label("○ 変更なし");
                }
                ui.separator();
                ui.label(self.message.clone());
            });
            if !self.status.is_empty() {
                ui.label(egui::RichText::new(self.status.clone()).weak().small());
            }
        });
    }

    fn tab_save(&mut self, ui: &mut egui::Ui) {
        if self.save.is_none() {
            ui.centered_and_justified(|ui| ui.label("左上の「セーブを開く」から system ファイルを選んでください"));
            return;
        }
        ui.heading(format!("キャラクター スロット {}", self.slot + 1));
        ui.add_space(4.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("char").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("名前");
                ui.text_edit_singleline(&mut self.edit.name);
                ui.end_row();

                ui.label("プレイ時間 (秒)");
                let mut secs = self.edit.play_time;
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut secs).speed(60).range(0..=100_000_000));
                    ui.label(format!("= {}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60));
                });
                self.edit.play_time = secs;
                ui.end_row();

                ui.label("所持金 (z)");
                ui.add(egui::DragValue::new(&mut self.edit.funds).speed(1000).range(0..=9_999_999));
                ui.end_row();

                ui.label("ハンターランク");
                ui.add(egui::DragValue::new(&mut self.edit.hunter_rank).speed(1).range(0..=999));
                ui.end_row();

                ui.label("HR ポイント");
                ui.add(egui::DragValue::new(&mut self.edit.hr_points).speed(1000).range(0..=99_999_999));
                ui.end_row();

                ui.label("アカデミー点数");
                ui.add(egui::DragValue::new(&mut self.edit.academy_points).speed(100).range(0..=9_999_999));
                ui.end_row();

                for (i, name) in ["ベルナ", "ココット", "ポッケ", "ユクモ"].iter().enumerate() {
                    ui.label(format!("{} ポイント", name));
                    ui.add(egui::DragValue::new(&mut self.edit.village_points[i]).speed(100).range(0..=9_999_999));
                    ui.end_row();
                }
            });

            ui.separator();
            ui.label(egui::RichText::new("外見").strong());
            egui::Grid::new("look").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label("性別");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.edit.gender, 0, "男性");
                    ui.selectable_value(&mut self.edit.gender, 1, "女性");
                });
                ui.end_row();
                for (label, v, max) in [
                    ("ボイス", &mut self.edit.voice, 40u8),
                    ("瞳の色", &mut self.edit.eye_color, 40),
                    ("服", &mut self.edit.clothing, 40),
                    ("髪型", &mut self.edit.hairstyle, 60),
                    ("顔", &mut self.edit.face, 40),
                    ("特徴", &mut self.edit.features, 40),
                ] {
                    ui.label(label);
                    ui.add(egui::DragValue::new(v).speed(1).range(0..=max));
                    ui.end_row();
                }
            });

            ui.separator();
            ui.label(egui::RichText::new("カラー (RGBA)").strong());
            for (label, color) in [
                ("肌", &mut self.edit.skin_color),
                ("髪", &mut self.edit.hair_color),
                ("特徴", &mut self.edit.features_color),
                ("服", &mut self.edit.clothing_color),
            ] {
                ui.horizontal(|ui| {
                    ui.label(format!("{:<4}", label));
                    let mut c = egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        *color = [c.r(), c.g(), c.b(), c.a()];
                    }
                    ui.label(format!(
                        "R{} G{} B{} A{}",
                        color[0], color[1], color[2], color[3]
                    ));
                });
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("編集内容を反映 (メモリのみ)").clicked() {
                    self.apply_to_save();
                }
                if ui.button("このスロットを使う/使わない 切替").clicked() {
                    let used = self.save.as_ref().map(|s| s.slot_used(self.slot)).unwrap_or(false);
                    if let Some(s) = &mut self.save {
                        s.set_slot_used(self.slot, !used);
                        self.dirty = true;
                    }
                }
            });
        });
    }

    fn tab_items(&mut self, ui: &mut egui::Ui) {
        if self.items.is_empty() {
            ui.centered_and_justified(|ui| ui.label("セーブを開くとアイテムボックスを表示します"));
            return;
        }
        ui.horizontal(|ui| {
            ui.label("絞り込み");
            ui.text_edit_singleline(&mut self.item_filter);
            ui.checkbox(&mut self.item_show_empty, "空きスロットも表示");
            if ui
                .button("全アイテムを 99 個にする")
                .on_hover_text("1 回で戻せます")
                .clicked()
            {
                self.push_item_undo();
                for s in self.items.iter_mut() {
                    if s.id != 0 {
                        s.count = 99;
                    }
                }
                self.apply_to_save();
            }
            // Dedupe throws stacks away, so it asks first.
            ui.menu_button("重複を除去", |ui| {
                ui.label(egui::RichText::new("同じアイテムの 2 個目以降を空にします").small());
                if ui.button("実行").clicked() {
                    self.push_item_undo();
                    let mut seen = std::collections::HashSet::new();
                    let mut removed = 0;
                    for s in self.items.iter_mut() {
                        if s.id == 0 {
                            continue;
                        }
                        if !seen.insert(s.id) {
                            s.id = 0;
                            s.count = 0;
                            removed += 1;
                        }
                    }
                    self.apply_to_save();
                    self.message = format!("重複を {} 件消しました（元に戻せます）", removed);
                    ui.close();
                }
            });
            if ui
                .add_enabled(!self.item_undo.is_empty(), egui::Button::new("元に戻す"))
                .on_hover_text("直前のアイテム変更を取り消します")
                .clicked()
            {
                self.undo_item();
            }
        });

        // The detail panel sits above the table: the table fills whatever is
        // left, so anything below it ends up off the bottom of the window.
        self.item_detail_panel(ui);
        ui.separator();

        let filter = self.item_filter.to_lowercase();
        let show_empty = self.item_show_empty;
        let rows: Vec<usize> = (0..self.items.len())
            .filter(|&i| {
                let s = self.items[i];
                if !show_empty && s.id == 0 {
                    return false;
                }
                if filter.is_empty() {
                    return true;
                }
                let name = item_name(s.id).to_lowercase();
                name.contains(&filter) || format!("{}", s.id).contains(&filter)
            })
            .collect();

        ui.label(format!("{} 件", rows.len()));
        let mut selected = self.item_selected;
        TableBuilder::new(ui)
            .striped(true)
            // Without this only the 20px box-number cell is clickable, which
            // nobody discovers.
            .sense(egui::Sense::click())
            .column(Column::exact(70.0))
            .column(Column::exact(80.0))
            .column(Column::remainder())
            .column(Column::exact(90.0))
            .header(22.0, |mut h| {
                h.col(|ui| {
                    ui.strong("箱 #");
                });
                h.col(|ui| {
                    ui.strong("ID");
                });
                h.col(|ui| {
                    ui.strong("名前");
                });
                h.col(|ui| {
                    ui.strong("個数");
                });
            })
            .body(|body| {
                body.rows(20.0, rows.len(), |mut row| {
                    let idx = rows[row.index()];
                    let (id, count) = (self.items[idx].id, self.items[idx].count);
                    row.col(|ui| {
                        if ui
                            .selectable_label(selected == Some(idx), format!("{}", idx + 1))
                            .clicked()
                        {
                            selected = Some(idx);
                        }
                    });
                    row.col(|ui| {
                        ui.label(format!("{}", id));
                    });
                    row.col(|ui| {
                        ui.label(item_name(id));
                    });
                    row.col(|ui| {
                        let mut c = count;
                        let r = ui.add(egui::DragValue::new(&mut c).range(0..=127));
                        // One snapshot per drag, not per pixel.
                        if r.drag_started() || (r.changed() && !r.dragged()) {
                            self.push_item_undo();
                        }
                        if r.changed() {
                            self.items[idx].count = c;
                            self.apply_to_save();
                        }
                    });
                    if row.response().clicked() {
                        selected = Some(idx);
                    }
                });
            });
        self.item_selected = selected;
    }

    /// The selected slot's editor, drawn above the table so the list can never
    /// push it off the screen.
    /// Remember the box before a change so it can be undone.
    fn push_item_undo(&mut self) {
        self.item_undo.push(self.items.clone());
        if self.item_undo.len() > 32 {
            self.item_undo.remove(0);
        }
    }

    /// Put the box back the way it was.
    fn undo_item(&mut self) {
        if let Some(prev) = self.item_undo.pop() {
            self.items = prev;
            self.apply_to_save();
            self.message = String::from("アイテムボックスを元に戻しました");
        } else {
            self.message = String::from("戻せる変更がありません");
        }
    }

    fn item_detail_panel(&mut self, ui: &mut egui::Ui) {
        let Some(i) = self.item_selected else {
            ui.label(egui::RichText::new("行をクリックすると、ここで種類と個数を変更できます").weak());
            return;
        };
        if i >= self.items.len() {
            return;
        }
        // Everything below edits self.items; apply_to_save() pushes that into
        // the save. Forgetting it made the edits look like they did nothing.
        let mut touched = false;
        ui.horizontal(|ui| {
            ui.label(format!("選択中: 箱 #{} ", i + 1));
            let mut id = self.items[i].id;
            if ui
                .add(egui::DragValue::new(&mut id).range(0..=4095).prefix("ID "))
                .changed()
            {
                self.items[i].id = id;
                touched = true;
            }
            let mut c = self.items[i].count;
            if ui
                .add(egui::DragValue::new(&mut c).range(0..=127).prefix("個数 "))
                .changed()
            {
                self.items[i].count = c;
                touched = true;
            }
            if ui.button("空にする").clicked() {
                self.push_item_undo();
                self.items[i] = ItemSlot::default();
                touched = true;
            }
        });

        // Pick a different item by name instead of typing a number.
        ui.horizontal(|ui| {
            ui.label("種類を変更");
            ui.add(
                egui::TextEdit::singleline(&mut self.item_pick_filter)
                    .hint_text("アイテム名で検索")
                    .desired_width(200.0),
            );
            if ui.small_button("クリア").clicked() {
                self.item_pick_filter.clear();
            }
            ui.checkbox(&mut self.item_pick_keep_filter, "選んだあとも絞り込みを残す")
                .on_hover_text("同じ名前のアイテムを続けて複数の枠へ入れやすくなります");
        });
        let needle = self.item_pick_filter.trim().to_lowercase();
        if needle.is_empty() {
            if touched {
                self.apply_to_save();
            }
            return;
        }
        let mut hits: Vec<(u16, &'static str)> = Vec::new();
        for (id, name) in mhnx_core::gamedata::ITEM_NAME_LIST.iter().enumerate() {
            if id == 0 || id > 4095 {
                continue;
            }
            if name.to_lowercase().contains(&needle) {
                hits.push((id as u16, name));
            }
            if hits.len() >= 40 {
                break;
            }
        }
        if hits.is_empty() {
            ui.label(egui::RichText::new("一致するアイテムがありません").weak());
            if touched {
                self.apply_to_save();
            }
            return;
        }
        // Fixed height: a block that grows with the hit count moved the table
        // under the pointer while typing.
        egui::ScrollArea::vertical()
            .id_salt("itempick")
            .max_height(160.0)
            .min_scrolled_height(160.0)
            .show(ui, |ui| {
                for (id, name) in &hits {
                    if ui
                        .selectable_label(self.items[i].id == *id, format!("{:<6} {}", id, name))
                        .clicked()
                    {
                        self.push_item_undo();
                        self.items[i].id = *id;
                        if self.items[i].count == 0 {
                            self.items[i].count = 1;
                        }
                        if !self.item_pick_keep_filter {
                            self.item_pick_filter.clear();
                        }
                        touched = true;
                    }
                }
            });
        if touched {
            self.apply_to_save();
            self.message = format!("箱 #{} を {} に変更しました", i + 1, item_name(self.items[i].id));
        }
    }

    fn tab_quest(&mut self, ui: &mut egui::Ui) {
        // The 249-byte hex grid and both 反映 buttons sit below the field
        // list; without a scroll they were off the bottom of the window.
        egui::ScrollArea::vertical()
            .id_salt("questtab")
            .show(ui, |ui| self.quest_tab_body(ui));
    }

    fn quest_tab_body(&mut self, ui: &mut egui::Ui) {
        if self.arc.is_none() {
            ui.centered_and_justified(|ui| {
                ui.label("「.arc を開く」か、DLC タブのクエスト一覧で「編集」を押してください")
            });
            return;
        }
        // An archive pulled out of the save can go straight back into it.
        match self.arc_from_save {
            Some(id) => {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("セーブ内のクエスト {} を編集中", id))
                            .strong()
                            .color(egui::Color32::from_rgb(0x6F, 0xC0, 0x6F)),
                    );
                    if ui
                        .button("セーブに書き戻す")
                        .on_hover_text("編集内容を再構築して、同じ ID の枠へ書き戻します")
                        .clicked()
                    {
                        self.write_arc_to_save();
                    }
                    if ui.small_button("切り離す").on_hover_text("セーブとの関連を切る").clicked() {
                        self.arc_from_save = None;
                    }
                });
            }
            None => {
                ui.label(
                    egui::RichText::new("ディスク上の .arc を編集中")
                        .weak()
                        .small(),
                );
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("絞り込み");
            ui.text_edit_singleline(&mut self.arc_filter);
            if ui.button("選択中の .arc を名前を付けて保存").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("quest archive", &["arc"])
                    .set_file_name("quest.arc")
                    .save_file()
                {
                    self.save_arc(p);
                }
            }
        });

        let filter = self.arc_filter.to_lowercase();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("エントリ").strong());
            ui.label(format!(
                "{} / {}",
                self.arc.as_ref().map(|a| a.entries.len()).unwrap_or(0),
                self.arc_selected + 1
            ));
        });

        let entries: Vec<(usize, String, usize)> = self
            .arc
            .as_ref()
            .map(|a| {
                a.entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| (i, e.file_name(), e.data.len()))
                    .collect()
            })
            .unwrap_or_default();
        let mut pending_arc: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("arclist")
            .max_height(180.0)
            .show(ui, |ui| {
                for (i, name, len) in &entries {
                    if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                        continue;
                    }
                    let label = format!("{:>3}  {:<52} {:>7} B", i, name, len);
                    if ui.selectable_label(self.arc_selected == *i, label).clicked() {
                        pending_arc = Some(*i);
                    }
                }
            });
        if let Some(i) = pending_arc {
            self.arc_selected = i;
            self.select_arc_entry(i);
        }

        ui.separator();
        self.quest_check_panel(ui);
        ui.separator();
        if self.mib.is_some() {
            ui.label(egui::RichText::new("クエストパラメータ (.mib, 249 バイト固定)").strong());
            let qid = self.mib.as_ref().map(|m| m.quest_id()).unwrap_or(0);
            let parts = mhnx_core::quest::decode_quest_id(qid);
            ui.label(format!(
                "クエスト ID: {}  ({} / {} / #{})",
                qid,
                match parts.kind {
                    1 => "通常",
                    11 => "オトモ",
                    _ => "不明",
                },
                match parts.rank {
                    0 => "下位",
                    1 => "上位",
                    2 => "G級/訓練",
                    _ => "不明",
                },
                parts.serial
            ));
            ui.separator();

            ui.label(egui::RichText::new("フィールド (Quedixx 対応表)").strong());
            ui.label(
                egui::RichText::new("緑 = 確定 / 黄 = 推定。右端は Quedixx のコントロール名です。")
                    .weak()
                    .small(),
            );
            let mut new_id: Option<u32> = None;
            egui::ScrollArea::vertical()
                .id_salt("mibfields")
                .max_height(420.0)
                .show(ui, |ui| {
                    egui::Grid::new("mibf").num_columns(4).spacing([10.0, 3.0]).show(ui, |ui| {
                        ui.label(egui::RichText::new("off").strong().monospace());
                        ui.label(egui::RichText::new("項目").strong());
                        ui.label(egui::RichText::new("値").strong());
                        ui.label(egui::RichText::new("Quedixx").strong());
                        ui.end_row();
                        for f in mhnx_core::questfields::MIB_FIELDS {
                            let ok = f.confidence == "confirmed";
                            let color = if ok {
                                egui::Color32::from_rgb(120, 220, 120)
                            } else {
                                egui::Color32::from_rgb(235, 190, 90)
                            };
                            ui.label(
                                egui::RichText::new(format!("{:02X}", f.offset))
                                    .monospace()
                                    .color(color),
                            );
                            ui.label(egui::RichText::new(f.ja).color(color));
                            let cur = mhnx_core::questfields::read_u32(
                                &self.mib_bytes, f.offset, f.kind,
                            )
                            .unwrap_or(0);
                            let table = f
                                .enum_name
                                .and_then(mhnx_core::questfields::enum_table);
                            if let Some(t) = table {
                                let shown = t.get(cur as usize).copied().unwrap_or("?");
                                egui::ComboBox::from_id_salt(("mibf", f.offset))
                                    .selected_text(format!("{} ({})", shown, cur))
                                    .width(220.0)
                                    .show_ui(ui, |ui| {
                                        for (i, name) in t.iter().enumerate() {
                                            if ui
                                                .selectable_label(
                                                    cur as usize == i,
                                                    format!("{} ({})", name, i),
                                                )
                                                .clicked()
                                            {
                                                mhnx_core::questfields::write_u32(
                                                    &mut self.mib_bytes,
                                                    f.offset,
                                                    f.kind,
                                                    i as u32,
                                                );
                                                if f.key == "quest_id" {
                                                    new_id = Some(i as u32);
                                                }
                                            }
                                        }
                                    });
                            } else if f.kind == FieldKind::Bytes {
                                // Byte blobs (the five monster slots and the
                                // rest) are not numbers: a 64 bit shift used to
                                // panic here in a debug build and render ten
                                // controls that could never write anything.
                                let mut s = String::new();
                                for b in self.mib_bytes.iter().skip(f.offset).take(f.size) {
                                    s.push_str(&format!("{:02X}", b));
                                }
                                ui.label(egui::RichText::new(s).monospace());
                                ui.label(
                                    egui::RichText::new("生バイト（下の 16 進表で編集）")
                                        .small()
                                        .weak(),
                                );
                                ui.end_row();
                                continue;
                            } else {
                                let width = ((f.size.min(8)) * 8) as u32;
                                let signed = matches!(
                                    f.kind,
                                    FieldKind::I8 | FieldKind::I16 | FieldKind::I32
                                );
                                let mut v: i64 = if signed {
                                    let shift = 64 - width;
                                    ((cur as u64) << shift) as i64 >> shift
                                } else {
                                    cur as i64
                                };
                                let lo = if signed { -(1i64 << (width - 1)) } else { 0 };
                                let hi = if signed {
                                    (1i64 << (width - 1)) - 1
                                } else {
                                    (1i64 << width) - 1
                                };
                                if width > 0
                                    && ui
                                        .add(
                                            egui::DragValue::new(&mut v)
                                                .range(lo..=hi)
                                                .speed(1),
                                        )
                                        .changed()
                                {
                                    mhnx_core::questfields::write_u32(
                                        &mut self.mib_bytes,
                                        f.offset,
                                        f.kind,
                                        (v as u64 & ((1u64 << width) - 1)) as u32,
                                    );
                                    if f.key == "quest_id" {
                                        new_id = Some(v as u32);
                                    }
                                }
                            }
                            ui.label(
                                egui::RichText::new(if f.ui.is_empty() { "-" } else { f.ui })
                                    .small()
                                    .weak(),
                            );
                            ui.end_row();
                        }
                    });
                });
            if let Some(id) = new_id {
                if let Some(m) = &mut self.mib {
                    m.set_quest_id(id);
                }
            }

            // The button belongs next to the fields it commits: it used to sit
            // under the hex grid, which put it off the bottom of the window.
            if ui.button(".mib の変更を .arc に反映").clicked() {
                self.store_arc_entry(self.arc_selected);
                self.message = "反映しました".to_owned();
            }

            ui.add_space(6.0);
            ui.label(egui::RichText::new("全 249 バイト (16 進)").strong());
            ui.label(
                egui::RichText::new(
                    "未確定の領域は生バイトで直接編集できます。変更後は下の反映ボタンを押してください。",
                )
                .weak()
                .small(),
            );
            let n = self.mib_bytes.len();
            egui::ScrollArea::vertical()
                .id_salt("mibbytes")
                .max_height(300.0)
                .show(ui, |ui| {
                    egui::Grid::new("mibhex")
                        .num_columns(18)
                        .spacing([4.0, 2.0])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("off").strong().monospace());
                            for c in 0..16 {
                                ui.label(egui::RichText::new(format!("{:X}", c)).strong().monospace());
                            }
                            ui.label(egui::RichText::new("ascii").strong().monospace());
                            ui.end_row();
                            for base in (0..n).step_by(16) {
                                ui.label(
                                    egui::RichText::new(format!("{:04X}", base))
                                        .monospace()
                                        .weak(),
                                );
                                for c in 0..16 {
                                    let o = base + c;
                                    if o >= n {
                                        ui.label("");
                                        continue;
                                    }
                                    let mut b = self.mib_bytes[o] as i32;
                                    if ui
                                        .add(egui::DragValue::new(&mut b).range(0..=255).speed(1))
                                        .changed()
                                    {
                                        self.mib_bytes[o] = b as u8;
                                    }
                                }
                                let txt: String = (0..16)
                                    .filter_map(|c| self.mib_bytes.get(base + c))
                                    .map(|b| {
                                        if *b >= 32 && *b < 127 {
                                            *b as char
                                        } else {
                                            '.'
                                        }
                                    })
                                    .collect();
                                ui.label(egui::RichText::new(txt).monospace().weak());
                                ui.end_row();
                            }
                        });
                });
        } else if self.gmd.is_some() {
            ui.label(egui::RichText::new("クエストテキスト (.gmd)").strong());
            let key = self.gmd.as_ref().map(|g| g.key.clone()).unwrap_or_default();
            ui.label(format!("キー: {}", key));
            ui.add_space(4.0);
            let labels = mhnx_core::quest::Gmd::LABELS;
            let n = self.gmd.as_ref().map(|g| g.strings.len()).unwrap_or(0);
            egui::ScrollArea::vertical()
                .id_salt("gmd")
                .max_height(360.0)
                .show(ui, |ui| {
                    if let Some(g) = &mut self.gmd {
                        for i in 0..n {
                            ui.label(labels.get(i).copied().unwrap_or("その他"));
                            let mut s = g.strings[i].clone();
                            if ui.text_edit_multiline(&mut s).changed() {
                                g.strings[i] = s;
                            }
                            ui.add_space(4.0);
                        }
                    }
                });
            if ui.button(".gmd の変更を .arc に反映").clicked() {
                self.store_arc_entry(self.arc_selected);
                self.message = "反映しました".to_owned();
            }
        } else {
            self.entry_panel(ui);
        }
    }

    /// Read-only views for .rem / .sup / .sem / .esl / .qdp / .qdl.
    ///
    /// The layouts come from measuring real quests; the .rem and .sup record
    /// lists are editable, the rest are shown as they are.
    fn entry_panel(&mut self, ui: &mut egui::Ui) {
        use mhnx_core::entries::{Esl, ItemTable, Qdp, QuestLink, SemEntry};

        let Some(arc) = &self.arc else {
            return;
        };
        let Some(e) = arc.entries.get(self.arc_selected) else {
            return;
        };
        let ext = extension_for(e.type_hash).unwrap_or("?");
        let data = e.data.clone();
        ui.label(
            egui::RichText::new(format!(
                "{} ({} バイト, 型 {:#010x})",
                e.file_name(),
                data.len(),
                e.type_hash
            ))
            .strong(),
        );
        ui.add_space(4.0);

        match ext {
            "rem" | "sup" => {
                let mut table = match ItemTable::parse(&data) {
                    Ok(t) => t,
                    Err(err) => {
                        ui.colored_label(
                            egui::Color32::LIGHT_RED,
                            format!("解析できません: {}", err),
                        );
                        return;
                    }
                };
                ui.label(format!(
                    "アイテム {} 件 / 収容 {} 件（{}）",
                    table.records.len(),
                    table.capacity,
                    if ext == "sup" { "支給品" } else { "報酬" }
                ));
                let mut changed = false;
                let mut sel = self.entry_item_sel;
                egui::ScrollArea::vertical()
                    .id_salt("itemtable")
                    .max_height(320.0)
                    .show(ui, |ui| {
                        egui::Grid::new("itab")
                            .striped(true)
                            .num_columns(4)
                            .show(ui, |ui| {
                                for h in ["#", "個数", "スロット", "アイテム名"] {
                                    ui.strong(h);
                                }
                                ui.end_row();
                                for (i, r) in table.records.iter_mut().enumerate() {
                                    if ui
                                        .selectable_label(
                                            sel == Some(i),
                                            format!("{}", i),
                                        )
                                        .clicked()
                                    {
                                        sel = Some(i);
                                    }
                                    let mut c = r.count as i32;
                                    if ui
                                        .add(egui::DragValue::new(&mut c).range(0..=127))
                                        .changed()
                                    {
                                        r.count = c as u8;
                                        changed = true;
                                    }
                                    let mut s = r.slot as i32;
                                    if ui
                                        .add(egui::DragValue::new(&mut s).range(0..=255))
                                        .changed()
                                    {
                                        r.slot = s as u8;
                                        changed = true;
                                    }
                                    ui.label(format!("{:<6} {}", r.item, item_name(r.item)));
                                    ui.end_row();
                                }
                            });
                    });
                // Changing the item itself is the point of a reward table;
                // only the count and the slot were editable before.
                self.entry_item_sel = sel;
                if let Some(i) = sel {
                    if i < table.records.len() {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "{} 行目のアイテムを変更",
                                i
                            ));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.item_pick_filter)
                                    .hint_text("アイテム名で検索")
                                    .desired_width(180.0),
                            );
                            if ui.small_button("クリア").clicked() {
                                self.item_pick_filter.clear();
                            }
                        });
                        let needle = self.item_pick_filter.trim().to_lowercase();
                        if !needle.is_empty() {
                            let mut hits: Vec<(u16, &'static str)> = Vec::new();
                            for (id, name) in
                                mhnx_core::gamedata::ITEM_NAME_LIST.iter().enumerate()
                            {
                                if id == 0 || id > 4095 {
                                    continue;
                                }
                                if name.to_lowercase().contains(&needle) {
                                    hits.push((id as u16, name));
                                }
                                if hits.len() >= 30 {
                                    break;
                                }
                            }
                            egui::ScrollArea::vertical()
                                .id_salt("entryitempick")
                                .max_height(140.0)
                                .min_scrolled_height(140.0)
                                .show(ui, |ui| {
                                    for (id, name) in &hits {
                                        if ui
                                            .selectable_label(
                                                table.records[i].item == *id,
                                                format!("{:<6} {}", id, name),
                                            )
                                            .clicked()
                                        {
                                            table.records[i].item = *id;
                                            if table.records[i].count == 0 {
                                                table.records[i].count = 1;
                                            }
                                            changed = true;
                                            self.item_pick_filter.clear();
                                        }
                                    }
                                });
                        }
                    }
                }
                if changed {
                    if let Some(a) = &mut self.arc {
                        if let Some(e) = a.entries.get_mut(self.arc_selected) {
                            e.data = table.to_bytes();
                        }
                    }
                    self.message =
                        format!("{} を更新しました（反映ボタンで .arc へ）", ext);
                }
                ui.add_space(4.0);
                if ui
                    .button(format!(".{} の変更を .arc に反映", ext))
                    .clicked()
                {
                    self.store_arc_entry(self.arc_selected);
                    self.message = "反映しました".to_owned();
                }
            }
            "sem" => {
                ui.label("モンスター配置 (.sem, 32 バイト単位)");
                egui::Grid::new("semg")
                    .striped(true)
                    .num_columns(3)
                    .show(ui, |ui| {
                        for h in ["#", "位置 (x, y, z)", "有効"] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for (i, c) in data.chunks_exact(32).enumerate() {
                            let s = SemEntry { raw: c.to_vec() };
                            let (x, y, z, _) = s.position();
                            ui.label(format!("{}", i));
                            ui.label(format!("{:.1}, {:.1}, {:.1}", x, y, z));
                            ui.label(if s.is_dummy() { "ダミー" } else { "実体" });
                            ui.end_row();
                        }
                    });
            }
            "esl" => match Esl::parse(&data) {
                Ok(l) => {
                    ui.label(format!("小モンスター出現 (.esl) / ウェーブ数 {}", l.waves));
                    ui.label(
                        egui::RichText::new("中身は未解読です（サイズはクエストにより可変）")
                            .weak(),
                    );
                }
                Err(err) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("解析できません: {}", err));
                }
            },
            "qdp" => match Qdp::parse(&data) {
                Ok(q) => {
                    ui.label("クエスト追加フラグ (.qdp)");
                    let hex: String = q.flags().iter().map(|b| format!("{:02X}", b)).collect();
                    ui.label(format!("ヘッダ後のフラグ: {}", hex));
                }
                Err(err) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("解析できません: {}", err));
                }
            },
            "qdl" => match QuestLink::parse(&data) {
                Ok(q) => {
                    ui.label(format!("クエスト構成ファイル一覧 (.qdl) / {} 件", q.refs.len()));
                    egui::Grid::new("qdlg")
                        .striped(true)
                        .num_columns(2)
                        .show(ui, |ui| {
                            for h in ["型", "名前"] {
                                ui.strong(h);
                            }
                            ui.end_row();
                            for r in &q.refs {
                                ui.label(extension_for(r.type_hash).unwrap_or("?"));
                                ui.label(&r.name);
                                ui.end_row();
                            }
                        });
                }
                Err(err) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("解析できません: {}", err));
                }
            },
            other => {
                ui.label(format!("このエントリ型 (.{}) は未対応です", other));
            }
        }
    }

    /// Refresh the checker findings for the open archive.
    pub fn run_quest_check(&mut self) {
        self.findings = match &self.arc {
            Some(a) => check::check_quest(a),
            None => Vec::new(),
        };
    }

    /// Run the quest checker on the open archive and list what it found.
    fn quest_check_panel(&mut self, ui: &mut egui::Ui) {
        if ui.button("このクエストをチェック").clicked() {
            self.findings = match &self.arc {
                Some(a) => check::check_quest(a),
                None => Vec::new(),
            };
            self.message = format!("{} 件の指摘", self.findings.len());
        }
        // Cloned so the fix below can take &mut self while the list is shown.
        let findings = self.findings.clone();
        let mut want_fix = false;
        for f in &findings {
            let (color, tag) = match f.severity {
                Severity::Error => (egui::Color32::from_rgb(235, 120, 120), "エラー"),
                Severity::Warn => (egui::Color32::from_rgb(235, 190, 90), "注意"),
                Severity::Info => (egui::Color32::from_rgb(150, 190, 235), "情報"),
            };
            let at = match &f.location {
                Some((name, off)) => format!(" ({} +0x{:X})", name, off),
                None => String::new(),
            };
            let mut fix_this = false;
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("[{}] {}{}", tag, f.message, at)).color(color),
                );
                // README promised a one-click fix for the 1人限定 problem and
                // only the console ever had it.
                if f.fixable && ui.small_button("修正").clicked() {
                    fix_this = true;
                }
            });
            if fix_this {
                want_fix = true;
            }
        }
        if want_fix {
            {
                let before = self.arc.as_mut().and_then(|a| {
                    check::clear_one_player_only(a, 0x00)
                });
                match before {
                    Some(old) => {
                        // The fix edits the archive entry, so the .mib working
                        // copy has to be refreshed or the next write puts the
                        // old byte back.
                        self.store_arc_entry(self.arc_selected);
                        self.run_quest_check();
                        self.message = format!(
                            "参加条件1 を 0x{:02X} から 0x00 に直しました（.arc へ反映してください）",
                            old
                        );
                    }
                    None => {
                        self.message = String::from("この指摘は自動で直せません");
                    }
                }
            }
        }
    }

    fn tab_dlc(&mut self, ui: &mut egui::Ui) {
        ui.heading("ダウンロードクエスト");
        let Some(save) = &self.save else {
            ui.label("セーブを開くと、導入済みのクエストを一覧できます");
            return;
        };
        if save.kind() == SaveKind::Mhgu {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                "MHGU はセクション配置が違うため、このツールでは扱いません",
            );
            return;
        }
        let quests = dlc::list_quests(save);
        ui.horizontal(|ui| {
            ui.label(format!("導入済み {} 件", quests.len()));
            ui.label("絞り込み");
            ui.text_edit_singleline(&mut self.dlc_filter);
        });
        ui.horizontal(|ui| {
            ui.label("抽出先");
            ui.text_edit_singleline(&mut self.dlc_outdir);
            if ui.button("ここへ全件抽出").clicked() {
                let dir = self.dlc_outdir.clone();
                let n = self.extract_dlc(&dir, &[]);
                self.message = format!("{} 件を {} へ書き出しました", n, dir);
            }
        });
        ui.separator();

        let needle = self.dlc_filter.trim().to_lowercase();
        let mut open_edit: Option<i32> = None;
        let mut extract_one: Option<i32> = None;
        let mut wipe: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt("dlclist")
            .max_height(420.0)
            .show(ui, |ui| {
                egui::Grid::new("dlcg")
                    .striped(true)
                    .num_columns(5)
                    .show(ui, |ui| {
                        for h in ["枠", "クエスト ID", "サイズ", "タイトル", ""] {
                            ui.strong(h);
                        }
                        ui.end_row();
                        for q in &quests {
                            if q.data.is_empty() {
                                continue;
                            }
                            // The .gmd inside the archive carries the
                            // title; decoding it here is what makes the filter
                            // able to match on one at all.
                            let title = Arc::parse(&q.data)
                                .ok()
                                .and_then(|a| {
                                    a.by_ext("gmd")
                                        .and_then(|e| Gmd::from_bytes(&e.data).ok())
                                })
                                .and_then(|g| g.title().map(|t| t.to_owned()))
                                .unwrap_or_default();
                            if !needle.is_empty()
                                && !format!("{}", q.quest_id).contains(&needle)
                                && !title.to_lowercase().contains(&needle)
                            {
                                continue;
                            }
                            ui.label(format!("{}", q.slot));
                            ui.label(format!("{}", q.quest_id));
                            ui.label(format!("{} B", q.size));
                            ui.label(title);
                            if ui.small_button("編集").clicked() {
                                open_edit = Some(q.quest_id);
                            }
                            if ui.small_button("抽出").clicked() {
                                extract_one = Some(q.quest_id);
                            }
                            if ui.small_button("空にする").clicked() {
                                wipe = Some(q.slot);
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(id) = open_edit {
            self.open_arc_from_save(id);
            self.tab = Tab::Quest;
        }
        if let Some(id) = extract_one {
            let dir = self.dlc_outdir.clone();
            let n = self.extract_dlc(&dir, &[id]);
            self.message = format!("{} 件を書き出しました", n);
        }
        if let Some(slot) = wipe {
            if let Some(s) = &mut self.save {
                match dlc::uninstall_quest(s, slot) {
                    Ok(()) => {
                        self.dirty = true;
                        self.message =
                            "空にしました（導入フラグは消せないため、ゲーム側には残ります）".to_owned();
                    }
                    Err(e) => self.message = format!("空にできません: {}", e),
                }
            }
        }
    }

    /// Searchable skill picker: 306 Japanese names, index = id - 1.
    fn skill_combo(
        ui: &mut egui::Ui,
        id_salt: &str,
        current: &mut u8,
        search: &mut String,
    ) -> bool {
        let shown = mhnx_core::skills::skill_name(*current)
            .map(|s| format!("{} ({})", s, current))
            .unwrap_or_else(|| format!("なし ({})", current));
        let mut picked: Option<u8> = None;
        egui::ComboBox::from_id_salt(id_salt)
            .selected_text(shown)
            .width(260.0)
            .show_ui(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(search)
                        .hint_text("スキル名で検索")
                        .desired_width(240.0),
                );
                if ui.small_button("クリア").clicked() {
                    search.clear();
                }
                if *current != 0 && ui.small_button("なし に戻す").clicked() {
                    picked = Some(0);
                }
                let needle = search.trim().to_lowercase();
                egui::ScrollArea::vertical()
                    .id_salt(id_salt)
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for (i, name) in mhnx_core::skills::SKILL_NAMES.iter().enumerate() {
                            let id = (i + 1) as u8;
                            if !needle.is_empty() && !name.to_lowercase().contains(&needle) {
                                continue;
                            }
                            if ui
                                .selectable_label(*current == id, format!("{:<4} {}", id, name))
                                .clicked()
                            {
                                picked = Some(id);
                            }
                        }
                    });
            });
        match picked {
            Some(id) => {
                *current = id;
                search.clear();
                true
            }
            None => false,
        }
    }

    /// Talisman editing: the list on the left, the editor on the right.
    fn tab_charm(&mut self, ui: &mut egui::Ui) {
        let Some(save) = &self.save else {
            ui.centered_and_justified(|ui| ui.label("セーブを開いてください"));
            return;
        };
        let charms = save.charms(self.slot).unwrap_or_default();
        // Copied out of the save so the editor closures can hold &mut self.
        let charm_box: Vec<u8> = save
            .equipment_box(self.slot)
            .map(|b| b.to_vec())
            .unwrap_or_default();
        let total = charms.len();
        ui.horizontal(|ui| {
            ui.heading("お守り");
            ui.label(format!(
                "スロット {} / {} 個（装備ボックス 2000 枠のうち）",
                self.slot + 1,
                total
            ));
            ui.label("絞り込み");
            ui.add(
                egui::TextEdit::singleline(&mut self.charm_filter)
                    .hint_text("スキル名・番号")
                    .desired_width(180.0),
            );
        });

        let needle = self.charm_filter.trim().to_lowercase();
        let shown: Vec<_> = charms
            .iter()
            .filter(|c| {
                if needle.is_empty() {
                    return true;
                }
                c.summary().to_lowercase().contains(&needle)
                    || format!("{}", c.index).contains(&needle)
            })
            .collect();
        ui.label(format!("{} 件表示", shown.len()));

        let mut pick: Option<usize> = None;
        let mut drop_idx: Option<usize> = None;
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                egui::ScrollArea::vertical()
                    .id_salt("charmlist")
                    .max_height(460.0)
                    .min_scrolled_height(460.0)
                    .show(ui, |ui| {
                        for c in &shown {
                            ui.horizontal(|ui| {
                                if ui
                                    .selectable_label(
                                        self.charm_index == c.index,
                                        format!("{:<6} {}", c.index, c.summary()),
                                    )
                                    .clicked()
                                {
                                    pick = Some(c.index);
                                }
                                if ui.small_button("削除").clicked() {
                                    drop_idx = Some(c.index);
                                }
                            });
                        }
                    });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_min_width(360.0);
                ui.label(egui::RichText::new("選択中のお守り").strong());
                ui.horizontal(|ui| {
                    ui.label("枠");
                    ui.add(egui::DragValue::new(&mut self.charm_index).range(0..=1999));
                    ui.label(egui::RichText::new("装飾スロット").weak());
                    for n in 0..=3u8 {
                        if ui
                            .selectable_label(self.charm_slots == n, format!("{}", "●".repeat(n as usize)))
                            .clicked()
                        {
                            self.charm_slots = n;
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("スキル1");
                    Self::skill_combo(ui, "sk1", &mut self.charm_skill1, &mut self.skill_search1);
                    let mut p = self.charm_points1 as i32;
                    if ui
                        .add(egui::DragValue::new(&mut p).range(-128..=127).prefix("+"))
                        .changed()
                    {
                        self.charm_points1 = p as i8;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("スキル2");
                    Self::skill_combo(ui, "sk2", &mut self.charm_skill2, &mut self.skill_search2);
                    let mut p = self.charm_points2 as i32;
                    if ui
                        .add(egui::DragValue::new(&mut p).range(-128..=127).prefix("+"))
                        .changed()
                    {
                        self.charm_points2 = p as i8;
                    }
                });
                // Show what the box itself says is normal for this skill, so
                // a wild value is visible before it is written. The shipped
                // range tables could not be indexed with confidence, so this
                // is derived from the save rather than guessed.
                if !charm_box.is_empty() {
                    let b = &charm_box[..];
                    for (label, sid, pts) in [
                        ("スキル1", self.charm_skill1, self.charm_points1),
                        ("スキル2", self.charm_skill2, self.charm_points2),
                    ] {
                        if sid == 0 {
                            continue;
                        }
                        match mhnx_core::charm::observed_points_range(b, 100, sid) {
                            Some(r) if pts < r.low || pts > r.high => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(235, 190, 90),
                                    format!(
                                        "{} {} は {:+} ですが、レア度100 の実在値は {:+}〜{:+}（{}件）",
                                        label,
                                        mhnx_core::skills::skill_name(sid).unwrap_or("?"),
                                        pts,
                                        r.low,
                                        r.high,
                                        r.samples
                                    ),
                                );
                            }
                            Some(r) => {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} {} の実在値 {:+}〜{:+}（{}件）",
                                        label,
                                        mhnx_core::skills::skill_name(sid).unwrap_or("?"),
                                        r.low,
                                        r.high,
                                        r.samples
                                    ))
                                    .weak()
                                    .small(),
                                );
                            }
                            None => {}
                        }
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("適用").clicked() {
                        let c = mhnx_core::charm::Charm {
                            index: self.charm_index,
                            slots: self.charm_slots,
                            skill1: self.charm_skill1,
                            points1: self.charm_points1,
                            skill2: self.charm_skill2,
                            points2: self.charm_points2,
                        };
                        if let Some(s) = &mut self.save {
                            if s.set_charm(self.slot, &c) {
                                self.dirty = true;
                                self.message = format!("お守り {} を更新しました", c.index);
                            } else {
                                self.message = String::from(
                                    "書き込めません（武器・防具の枠は保護されています）",
                                );
                            }
                        }
                    }
                    if ui.button("空き枠に新規作成").clicked() {
                        let new = mhnx_core::charm::Charm {
                            index: self.charm_index,
                            slots: 0,
                            skill1: 0,
                            points1: 0,
                            skill2: 0,
                            points2: 0,
                        };
                        if let Some(s) = &mut self.save {
                            if s.set_charm(self.slot, &new) {
                                self.dirty = true;
                                self.message =
                                    format!("空き枠 {} にお守りを作りました", self.charm_index);
                            } else {
                                self.message = String::from(
                                    "その枠は空きではないか、保護されています",
                                );
                            }
                        }
                    }
                });
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(
                        "装飾スロットは ● の数、ポイントは符号付きです。レア度ごとの許容範囲は未検証なので、実在するお守りに近い値にしてください。",
                    )
                    .weak()
                    .small(),
                );
            });
        });

        if let Some(i) = pick {
            if let Some(c) = charms.iter().find(|c| c.index == i) {
                self.charm_index = c.index;
                self.charm_slots = c.slots;
                self.charm_skill1 = c.skill1;
                self.charm_points1 = c.points1;
                self.charm_skill2 = c.skill2;
                self.charm_points2 = c.points2;
            }
        }
        if let Some(i) = drop_idx {
            if let Some(s) = &mut self.save {
                if s.clear_charm(self.slot, i) {
                    self.dirty = true;
                    self.message = format!("お守り {} を削除しました", i);
                }
            }
        }
    }


    fn tab_extras(&mut self, ui: &mut egui::Ui) {
        if self.save.is_none() {
            ui.label("セーブを開いてください");
            return;
        }
        // Everything is gathered up front and applied at the end: the buttons
        // need &mut self.save while the widgets borrow it for reading.
        let save = self.save.as_ref().unwrap().clone();
        let mut set_opts: Option<mhnx_core::save::Options> = None;
        let mut add_cat: Option<(usize, usize)> = None;
        let mut drop_cat: Option<usize> = None;
        let mut drop_charm: Option<usize> = None;
        let mut write_charm = false;
        let mut want_export = false;
        let mut want_import = false;

        ui.heading("本体オプション");
        let o = save.options();
        ui.horizontal(|ui| {
            ui.label("明るさ");
            let mut b = o.brightness as i32;
            if ui.add(egui::Slider::new(&mut b, 0..=255)).changed() {
                let mut n = o;
                n.brightness = b as u8;
                set_opts = Some(n);
            }
        });
        let mut rumble = o.rumble != 0;
        if ui.checkbox(&mut rumble, "振動").changed() {
            let mut n = o;
            n.rumble = if rumble { 1 } else { 0 };
            set_opts = Some(n);
        }

        ui.separator();
        ui.heading("配信オトモ");
        let cats: Vec<_> = save
            .dlc_palicoes()
            .into_iter()
            .filter(|c| c.id != 0)
            .collect();
        ui.label(format!("{} / 50 枠 使用中", cats.len()));
        // Bounded: a scroll area that grows pushes everything below off screen.
        egui::ScrollArea::vertical()
            .id_salt("palicolist")
            .max_height(140.0)
            .show(ui, |ui| {
                for c in &cats {
                    ui.horizontal(|ui| {
                        ui.label(format!("#{:<3} id 0x{:02X} {}", c.index, c.id, c.name));
                        if ui.small_button("外す").clicked() {
                            drop_cat = Some(c.index);
                        }
                    });
                }
            });
        ui.horizontal(|ui| {
            ui.label("日本語版");
            egui::ComboBox::from_id_salt("catlang")
                .selected_text(if self.cat_lang == 0 { "日本語版" } else { "英語版" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.cat_lang, 0, "日本語版");
                    ui.selectable_value(&mut self.cat_lang, 1, "英語版");
                });
            let lang = if self.cat_lang == 0 {
                mhnx_core::dlcats::Lang::Jp
            } else {
                mhnx_core::dlcats::Lang::En
            };
            let ids = mhnx_core::dlcats::all_ids();
            let shown = ids
                .get(self.cat_id)
                .and_then(|id| mhnx_core::dlcats::record(lang, *id))
                .map(mhnx_core::dlcats::record_name)
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("catid")
                .selected_text(shown)
                .width(240.0)
                .show_ui(ui, |ui| {
                    for (i, id) in ids.iter().enumerate() {
                        let name = mhnx_core::dlcats::record(lang, *id)
                            .map(mhnx_core::dlcats::record_name)
                            .unwrap_or_default();
                        let used = save.has_dlc_palico(*id);
                        ui.selectable_value(
                            &mut self.cat_id,
                            i,
                            if used {
                                format!("{} (導入済み)", name)
                            } else {
                                name
                            },
                        );
                    }
                });
            if ui.button("追加").clicked() {
                add_cat = Some((self.cat_lang, self.cat_id));
            }
        });

        ui.separator();
        ui.heading("お守り");
        let charms = save.charms(self.slot).unwrap_or_default();
        ui.label(format!("スロット {}: {} 個のお守り", self.slot + 1, charms.len()));
        egui::ScrollArea::vertical()
            .id_salt("charms")
            .max_height(150.0)
            .show(ui, |ui| {
                for c in &charms {
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(
                                self.charm_index == c.index,
                                format!("#{:<5} {}", c.index, c.summary()),
                            )
                            .clicked()
                        {
                            self.charm_index = c.index;
                            self.charm_slots = c.slots;
                            self.charm_skill1 = c.skill1;
                            self.charm_points1 = c.points1;
                            self.charm_skill2 = c.skill2;
                            self.charm_points2 = c.points2;
                        }
                        if ui.small_button("削除").clicked() {
                            drop_charm = Some(c.index);
                        }
                    });
                }
            });
        ui.horizontal(|ui| {
            ui.label("枠");
            ui.add(egui::DragValue::new(&mut self.charm_index).range(0..=1999));
            ui.label("装飾スロット");
            ui.add(egui::DragValue::new(&mut self.charm_slots).range(0..=3));
        });
        ui.horizontal(|ui| {
            ui.label("スキル1");
            ui.add(egui::DragValue::new(&mut self.charm_skill1).range(0..=255));
            let mut p = self.charm_points1 as i32;
            if ui.add(egui::DragValue::new(&mut p).range(-128..=127)).changed() {
                self.charm_points1 = p as i8;
            }
            ui.label("スキル2");
            ui.add(egui::DragValue::new(&mut self.charm_skill2).range(0..=255));
            let mut p2 = self.charm_points2 as i32;
            if ui.add(egui::DragValue::new(&mut p2).range(-128..=127)).changed() {
                self.charm_points2 = p2 as i8;
            }
        });
        if ui.button("この内容で書き込む").clicked() {
            write_charm = true;
        }

        ui.separator();
        ui.heading("キャラクタをまるごと入出力");
        ui.label(
            egui::RichText::new("1 スロット分（キャラクタ本体 + アイテム）を .saveslot として出し入れします")
                .weak()
                .small(),
        );
        ui.horizontal(|ui| {
            if ui.button("このスロットを書き出す").clicked() {
                want_export = true;
            }
            if ui.button("このスロットに読み込む").clicked() {
                want_import = true;
            }
        });

        // ---- apply ----
        if let Some(o) = set_opts {
            if let Some(s) = &mut self.save {
                s.set_options(&o);
                self.dirty = true;
                self.message = String::from("オプションを更新しました");
            }
        }
        if let Some((lang, idx)) = add_cat {
            let ls = if lang == 0 {
                mhnx_core::dlcats::Lang::Jp
            } else {
                mhnx_core::dlcats::Lang::En
            };
            let ids = mhnx_core::dlcats::all_ids();
            if let Some(id) = ids.get(idx).copied() {
                if let Some(s) = &mut self.save {
                    match s.install_dlc_palico(ls, id) {
                        Ok(slot) => {
                            self.dirty = true;
                            self.message = format!("オトモ 0x{:02X} を枠 {} に追加しました", id, slot);
                        }
                        Err(e) => self.message = format!("追加できません: {}", e),
                    }
                }
            }
        }
        if let Some(i) = drop_cat {
            if let Some(s) = &mut self.save {
                if s.remove_dlc_palico(i) {
                    self.dirty = true;
                    self.message = format!("枠 {} を外しました", i);
                }
            }
        }
        if let Some(i) = drop_charm {
            if let Some(s) = &mut self.save {
                if s.clear_charm(self.slot, i) {
                    self.dirty = true;
                    self.message = format!("お守り {} を削除しました", i);
                }
            }
        }
        if write_charm {
            let c = mhnx_core::charm::Charm {
                index: self.charm_index,
                slots: self.charm_slots,
                skill1: self.charm_skill1,
                points1: self.charm_points1,
                skill2: self.charm_skill2,
                points2: self.charm_points2,
            };
            if let Some(s) = &mut self.save {
                if s.set_charm(self.slot, &c) {
                    self.dirty = true;
                    self.message = format!("お守り {} を更新しました", c.index);
                } else {
                    self.message =
                        String::from("書き込めません（武器・防具の枠は保護されています）");
                }
            }
        }
        if want_export {
            if let Some(p) = rfd::FileDialog::new()
                .set_file_name(format!("slot{}.saveslot", self.slot + 1))
                .save_file()
            {
                if let Some(bytes) = save.export_slot(self.slot) {
                    match std::fs::write(&p, &bytes) {
                        Ok(()) => self.message = format!("書き出しました: {}", p.display()),
                        Err(e) => self.message = format!("書き出せません: {}", e),
                    }
                }
            }
        }
        if want_import {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("save slot", &["saveslot"])
                .pick_file()
            {
                match std::fs::read(&p) {
                    Ok(bytes) => {
                        let mut err = None;
                        if let Some(s) = &mut self.save {
                            if let Err(e) = s.import_slot(self.slot, &bytes) {
                                err = Some(format!("{}", e));
                            }
                        }
                        match err {
                            Some(e) => self.message = e,
                            None => {
                                self.dirty = true;
                                self.reload_from_save();
                                self.message = format!("読み込みました: {}", p.display());
                            }
                        }
                    }
                    Err(e) => self.message = format!("読めません: {}", e),
                }
            }
        }
    }

    fn tab_about(&mut self, ui: &mut egui::Ui) {
        ui.heading("mhnxeditor");
        ui.label("Monster Hunter XX / MHGU のセーブとクエストを編集するツールです。");
        ui.add_space(6.0);
        ui.label("・セーブは 3DS / Switch の両形式に対応");
        ui.label("・ダウンロードクエストの一覧・差し替え・抽出");
        ui.label("・.arc 内の .mib / .gmd / .rem / .sup / .sem / .esl / .qdp / .qdl を表示");
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("実機での動作は未検証です。必ずセーブを控えてから使ってください。")
                .weak(),
        );
    }
}

/// Item name looked up in the generated game data tables.
fn item_name(id: u16) -> String {
    #[cfg(feature = "gamedata")]
    {
        mhnx_core::gamedata::ITEM_NAME_LIST
            .get(id as usize)
            .map(|s| (*s).to_owned())
            .unwrap_or_else(|| format!("(unknown {})", id))
    }
    #[cfg(not(feature = "gamedata"))]
    {
        format!("#{}", id)
    }
}
