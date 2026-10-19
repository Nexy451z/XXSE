# GUI review: where the editor makes you think, hunt, or guess

Reviewed build: `cargo build --release -p mhnx-gui`, egui/eframe 0.32.3, `crates/mhnx-gui/src/app.rs` (~2,080 lines).
Driven headlessly with the documented hooks; screenshots in `.re_review_ux/` (save.png, items.png, items_sel_blank.png, items_sel_hit.png, items_sel_nohit.png, extras.png, dlc.png, quest.png).
Real data, read-only: the Ryujinx save. Slot 1 holds **2,242 item stacks (58 blank of 2,300)**, **188 talismans** in the shared 2,000-record equipment box, **175 installed quests**.

**Verdict.** The foundations are better than the screens. The name tables are already wired (2,991 item names, 306 skill names, quest titles decoded in the CLI, talisman rarity and point-range tables sitting unused in `gamedata.rs`), the item table virtualises 2,242 rows, and the app is honest about memory-vs-file. But both screens you named spend the user's attention on *decoding numbers* and on *finding controls*, and the item screen quietly loses edits while telling the user nothing changed. The fixes are mostly wiring, not invention.

---

## 1. What works (keep it)

1. **Names exist in the model.** `ITEM_NAME_LIST` is Japanese and complete; `Charm::summary()` already renders `[3 slots] 属性会心 +6 / 達人 +10` (`charm.rs:110-128`). The charm *list* proves the table wiring works; only the *editor* throws the name away.
2. **The item table is real.** `TableBuilder` with a per-row `DragValue` renders 2,242 rows and scrolls smoothly in release; inline count editing is the right instinct.
3. **The save model is explicit.** Every message says whether it touched memory or the file ("まだファイルには書いていません"), and the bottom bar has a dirty dot. More than most save editors do.
4. **The quest screen tells a story.** Entry list with extension + size, colour-coded findings, and an explicit "セーブに書き戻す" button make the save→arc→save loop legible.
5. **The DLC screen has per-row verbs** (編集 / 抽出 / 空にする) and a working extract-all; the MHGU refusal states *why*.
6. **The save-info rail is genuinely useful** (slot bases, DLC section, header CRC) for support and for trusting the tool.
7. **Japanese UI with a font fallback chain**; no tofu anywhere in the screenshots.
8. **The headless hooks are a usability asset.** Keep them, and add `MHNXEDITOR_CHARM_SELECT` / `MHNXEDITOR_ITEM_COUNT` so the *selected* state of each editor can be screenshotted — today the charm editor's only observable state is the broken default (see 3.1).

---

## 2. "アイテム編集" — redesign of the item type picker

### 2.1 Measured today

The picker is not a modal and not a popup: it is a block **above** the table, in the same column, whose height follows the number of hits (`app.rs:1061-1155`; the code comment at `app.rs:987` says this was deliberate so the table cannot push it off-screen).

| Step | What actually happens | Cost |
|---|---|---|
| Select | Only the *box-number* cell is a `selectable_label` (`app.rs:1034-1041`). Clicking the item name or the ID does nothing. The label is only as wide as the digits (≈10–30 px). | Hunt |
| Look | The editor appears at the top of the page, up to 800 px from the row you clicked; only the number cell changes colour. | Think |
| Search | The field matches **Japanese substrings only** (`app.rs:1115-1126`). No ID search, no romaji. It stops at **40 hits** and silently drops the rest: 133 item names contain 弾, 29 contain 薬. The table's own filter *does* match IDs (`app.rs:1004`), so the two search boxes disagree. | Guess |
| While typing | The result block changes height (0–140 px), so the whole table jumps under the cursor on every keystroke (compare items_sel_hit.png with items_sel_nohit.png). | Think |
| No match | `一致するアイテムがありません` in weak grey (`app.rs:1128`): the query is not echoed, no ID fallback, no suggestion, no reason. | Guess |
| Commit | The filter is **cleared** on pick (`app.rs:1147`), so the next slot means retyping the name. | Work |
| Count | Three different limits for one number: row `0..=127` (`app.rs:1050`), panel `0..=127` (`app.rs:1084`), bulk action sets 99 (`app.rs:967`). The game stacks at 99. | Think |
| Count durability | Editing the count **in the row** mutates `self.items` only (`app.rs:1048-1052`): the document is not marked dirty, the bar still says ○ 変更なし, and switching save slots rebuilds the box from the save and discards the edit silently. | Data loss |
| Bulk | "全アイテムを 99 個にする" and "重複を除去" are one click, no confirmation, no undo, generic status message (`app.rs:964-984`). Dedupe keeps the *first* stack: this save has 6 回復薬 stacks, so one click deletes 5. | Data loss |
| Copy to many | Impossible. No multi-select, no stamp, no "fill N blanks". | Work |
| Undo | None anywhere in the GUI — a grep for `undo|history|consume_shortcut|request_focus` over `crates/` returns **0 matches**. | Trust |
| Empty slots | Reachable only by finding the "空きスロットも表示" checkbox (default off, `app.rs:963`), which sits in the same row as the two destructive buttons. | Hunt |

### 2.2 Target layout

```
┌ アイテム ───────────────────────────────────────────────────────────────┐
│ [🔍 名前 / ID で絞り込み______] [表示: 使用中のみ ▾] [＋ 空き枠] [一括 ▾] [⟲] │
├──────────────────────────────────────────────┬──────────────────────────┤
│  箱#   名前                        ID   個数  │ 箱 #6   回復薬           │
│   1    調合書①入門編               1   [99]  │ ID 9 / スタック 99       │
│   2    調合書②初級編               2   [99]  │ 個数 [99]   (0–99)       │
│   3    調合書③中級編               3   [99]  │ [名前を変更…]  [空にする] │
│  ▸ 6   回復薬                     9   [99]  │ ─────────────────────    │
│   7    回復薬                     9   [99]  │ 最近: 回復薬 / 回復薬グレート│
│  …                                          │ [スタンプ: 切]           │
└──────────────────────────────────────────────┴──────────────────────────┘
status: 3 件の変更（未保存）   [⟲ 元に戻す Ctrl+Z]
```

**Widgets**
- Toolbar: `TextEdit::singleline` with `hint_text("名前 / ID で絞り込み")`; `ComboBox::from_id_salt("item_view")` with すべて / 使用中のみ / 空きのみ (replaces the checkbox); `Button::new("＋ 空き枠")`; `MenuButton::new("一括操作")`; `Button::new("⟲")`.
- Table: `TableBuilder::new(ui).sense(egui::Sense::click())`; `row.response().clicked()` selects the **whole row** and `row.set_selected(true)` paints it; `double_clicked()` (and Enter) opens the picker. Columns: 箱# (`Column::exact(60.0)`), 名前 (`Column::remainder()`), ID (`Column::exact(64.0)`, monospace + weak), 個数 (`Column::exact(90.0)`, `DragValue::new(&mut count).range(0..=99)`). Row hover: `on_hover_text("ダブルクリックで変更 / Delete で空にする")`.
- Detail pane: `SidePanel::right("item_detail").show_inside(ui)` (320 px) with `ui.heading(name)`, a weak monospace `ID 9 / 箱 #6`, the count `DragValue`, `Button("名前を変更…")`, `Button::new("空にする").fill(dark_red)`, and a 5-entry "最近使った" list of `selectable_label`.
- Picker: `egui::Modal::new(egui::Id::new("item_picker"))` — fixed 520 px width, no layout movement behind it. Header `箱 #6 のアイテムを変更（現在: 回復薬）`; a `TextEdit` that is focused on open via `ui.memory_mut(|m| m.request_focus(id))`; a `ScrollArea::vertical().max_height(320.0)` of `selectable_label` rows rendered as `RichText::new(name).strong()` plus a dim monospace `#9` on the right; a `DragValue "個数"` and a `DragValue "空き枠に N 枠"` in the footer; primary `Button("決定")`, secondary `Button("キャンセル")`. `Modal::should_close()` handles Esc/backdrop.
- Stamp: a toolbar `selectable_label(stamp_mode, "スタンプ: 切/入")`. While on, a single click on any row writes the *current* item+count into that row, no modal.

### 2.3 The interaction, step by step

1. Click the アイテム tab. Rows read "箱# / 名前 / ID / 個数"; the header says `2,242 枠（空き 58）`. The name is the widest column because names are what you scan.
2. Type `かいふく` with the IME (or `9` for the ID) in the toolbar. The table filters live, the label becomes `8 件 / 2,242`, nothing moves vertically.
3. Click *anywhere* on a row. The whole row highlights; the right pane shows `回復薬 / ID 9 / 箱 #6 / 個数 99`; the row additionally gets a hover hint.
4. Press Enter (or double-click, or click "名前を変更…"). The modal covers the table with the search box focused. The footer shows the target's current count.
5. Type `回復`. Every keystroke updates a **fixed-height** list of 回復薬, 回復薬グレート, LV1 回復弾… with IDs; the header says `8 件 — ↑↓ で選択 / Enter で決定 / Esc で閉じる`.
6. ArrowDown, Enter. The modal closes, the row now reads 回復薬グレート, the status bar says `箱 #6: 回復薬 → 回復薬グレート（Ctrl+Z で戻す）`, the dirty dot turns on.
7. Next slot: click, Enter, the **same query is still in the box**, ArrowDown/Enter. Three actions per slot instead of five, and no retyping.
8. Bulk copy: in the modal set `空き枠に 5 枠` and press Enter → five blank records become 回復薬 ×99; the status names the count. For sweeping, turn on スタンプ and click rows: each click is one undo entry.
9. Ctrl+Z (or ⟲) reverts the last action and names it in the status bar. 32 steps of a 2,300-slot box cost ~9 KB/step (294 KB total) — no reason not to.
10. The two bulk operations move under 一括操作 and open a `Modal` that states the *count* before applying ("6 枠のうち 5 枠を空にします"; "全 2,242 枠を 99 個にします") with a note that Ctrl+Z undoes it.

### 2.4 The six questions, answered directly

- **How many steps?** Today 5 pointer actions per slot (row number → panel field → type → result → repeat) plus retyping; after the change, 3 (row → type → Enter) and the query persists.
- **Discoverability:** the whole row is clickable; Enter and double-click both open the editor; the detail pane names the action "名前を変更…"; rows carry a hover hint. Nothing depends on hitting a 10 px number.
- **While nothing matches:** the modal stays open, keeps focus, echoes the query, and offers `ID で探す: 42 = 回復薬` when the text is numeric (or `ID は 0〜2990` when it is out of range), otherwise `名前の一部（例: 回復）で検索できます`. No layout jump.
- **Count editable at the same time?** Yes, in three places that share one value and one undo entry: the row `DragValue`, the detail pane, and the modal footer. The limit is 99 everywhere, matching the game's stack and the existing bulk action.
- **Copy one item to many slots?** Yes, three ways, cheapest first: (a) the modal's "空き枠に N 枠"; (b) スタンプ mode; (c) Ctrl+click multi-select with "選択した N 枠をこのアイテムに" (phase 2 — needs `BTreeSet<usize>` and a "N 枠を選択中" header).
- **Undo?** Yes: one `edit_item()` entry point pushes a snapshot, marks dirty, writes the box to memory; Ctrl+Z pops. The same mechanism covers 空にする and the bulk menu.

### 2.5 Implementation notes

- One mutation function: every path (row DragValue, picker, stamp, clear, bulk) calls `fn edit_item(&mut self, idx, slot: ItemSlot)` which (1) pushes `self.items.clone()` to `item_history` (cap 32), (2) sets `dirty = true`, (3) `apply_to_save()`. That alone fixes the "count edit is lost while the bar says 変更なし" bug.
- The picker needs its own state: `item_picker: Option<usize>` (target), `item_pick_filter` (keep on commit), `item_pick_cursor: usize`, `item_recent: Vec<u16>`. Handle `ArrowUp/Down/Enter/Escape` with `ui.input_mut(|i| i.consume_key(...))`.
- Remove the silent 40-result cap: cap at 200 and label it `133 件中 200 件を表示 — 絞り込んでください`, or just cap and *say* so.
- Search rule: `name.contains(q) || id_string.starts_with(q)`; if `q` parses to `u16` in 1..=2990, always offer it as a "ID 42 = 回復薬" row at the top.
- Keep the ID visible but secondary everywhere (`RichText::monospace().weak()`); the CLI and other editors speak IDs, users speak names.
- `TableBuilder::scroll_to_row(selected, Some(Align::Center))` after a pick, so the edited row stays on screen.
- Optional: type-ahead into the filter when the table has focus and no text widget does (file-manager behaviour). Nice, not required.

---

## 3. "お守りの編集" — replacement design

### 3.1 Measured today

- It lives under **設定/オトモ** (`app.rs:738`) — a tab whose name contains neither お守り nor 装備. It is the *third* block in that tab, after options and palico.
- The tab has **no outer ScrollArea** (`app.rs:1780-2051`). At the 900×600 minimum window the charm form is below the bottom bar and there is no way to scroll to it. It fits at the default 1280×820 with ~100 px to spare, and not at 600 px of height.
- 188 charms in a `ScrollArea` of `max_height(150.0)` → **7 rows visible, ~27 screens**, no filter, no sort, no search (`app.rs:1884-1909`).
- The first column is the **equipment-box record index**, and those indices are fragmented because weapons and armour share the box: charm indices in this save are `224–229` then `1397–1695` then `1802–1994` — the list jumps from #229 to #1397 in one step. The number is meaningless to a player.
- The form is raw bytes with labels (`app.rs:1910-1929`):

  `枠 [0] 装飾スロット [0]` — then `スキル1 [1] [1] スキル2 [0] [0]`

  Two unlabeled numbers per skill; which is the ID and which is the points value is never stated; the IDs are a `DragValue` 0..=255 with **no name**; the points are -128..=127 with **no legality range**. The name is visible in the list summary and disappears at the exact moment you edit.
- **Rarity and class are not decoded.** `Charm` carries five fields (`charm.rs:84-104`); +0x02 (class) and +0x12 (rarity) are dropped by `decode_box`. The tables that make those bytes meaningful already exist in `gamedata.rs` and are used by nobody: `TALISMAN_RARITY` (11 entries), `EQUIP_TALISMAN_NAMES` (11), and the four `TALISMAN_*_SKILL` range tables (206 entries each). The reference editor uses exactly these to clamp points; ours can therefore write records the game's own per-rarity table forbids (already flagged in `docs/rereview_charm.md`).
- **The default state is broken and looks live.** Nothing is selected, so the form points at `枠 0` (a Great Sword) with 毒 +1. Pressing 書き込む fails with `書き込めません（武器・防具の枠は保護されています）` — the first action a new user takes fails, and nothing warned them beforehand (see extras.png).
- **No create, no duplicate.** `charm::create_charm`, `template_from_box`, `first_writable` all exist and the terminal editor already offers "L = new charm" (`hbedit.rs:598-610`); the GUI never calls them.
- **Clicking another row silently overwrites the form**; there is no 適用/取消 and no unsaved marker. **削除 ignores the return value** (`app.rs:1985-1992`) and reports success even if the write was refused.

### 3.2 Target layout — a new お守り tab, master + detail

```
┌ お守り ─────────────────────────────────────────────────────────────────────┐
│ スロット [1 ▾] 188 個 [🔍 スキル名で絞り込み____] [スキル ▾] [並び: 枠順 ▾] [＋ 新規] │
├──────────────────────────────────────────────┬──────────────────────────────┤
│ No.   スキル1            スキル2         枠   │ お守り 42 / 188   #229        │
│  1    属性会心  +6       達人     +10   ●●●  │ 創世の護石（レアリティ 100）   │
│  2    属性会心  +6       爆弾強化 +10   ●●●  │ スキル1 [属性会心 ▾] + [ 6 ] 0〜10 │
│  3    属性会心  +6       真・青電主 +3  ●●●  │ スキル2 [達人 ▾]     + [10] -7〜13 │
│  …                                          │ 装飾スロット [───][●──][●●─][●●●] │
│  (whole-row select; #229 in the tooltip)      │ [適用] [取消] [複製] [削除]   │
│                                              │ ▸ 生データ (+0x02=0x0A, +0x12=100) │
└──────────────────────────────────────────────┴──────────────────────────────┘
```

**Widgets**
- Navigation: `Tab::Charms`, label **お守り** (or 装備/お守り). Rename **設定/オトモ → 設定** and move palico to its own tab, or keep palico there once charms are out.
- Header: `ComboBox::from_id_salt("charm_slot")` (1/2/3 — the slot is currently chosen only in the side rail); count label; `TextEdit::singleline` with `hint_text("スキル名で絞り込み")`; `ComboBox` "スキル" (すべて + お守り対象の 205 名); `ComboBox` "並び" (枠順 / スキル1 / 合計ポイント); `Button::new("＋ 新規")`.
- List: `TableBuilder::new(ui).sense(egui::Sense::click())`; `row.response().clicked()` selects, `row.set_selected(true)` paints. Columns: No. (ordinal 1..N, `Column::exact(48.0)`), スキル1 (`name + signed points`, right-aligned points via `ui.with_layout(Layout::right_to_left(Align::Center))`), スキル2, 枠 (`"●".repeat(slots)` + `"─".repeat(3 - slots)`, `RichText::monospace()`). The box index lives in `on_hover_text("装備箱のレコード #229")`.
- Detail: `Frame::group` with a heading, a rarity badge, and the fields below.
- Skill picker: **a searchable combo, not a giant dropdown.** `ComboBox::from_id_salt("charm_skill1")` whose `selected_text` is `skill_name(id).unwrap_or("なし")`; inside, a `TextEdit` (`hint_text("スキル名で検索")`) over a `ScrollArea::vertical().max_height(260.0)` of `selectable_label` rows. Each row is the name, with the legal range for the *current* rarity right-aligned (`+6  0〜10`), and skills with no talisman range (id > 205) greyed with `(お守り不可)`. Click commits, calls `ui.close()`, and clamps the points to the new range.
- Points: `DragValue::new(&mut points).range(min..=max).speed(1)` when the value is legal; when it is not, paint the value red and show `範囲外: 0〜0（レアリティ 100）` with `Button("範囲内に補正")`.
- Slots: four `selectable_value(&mut slots, n, "●".repeat(n) + "─".repeat(3-n))` in a `ui.horizontal`, 0..=3. This is the one field that is genuinely visual in the game; a number 0–3 makes the user translate.
- Buttons: `Button::new("適用").fill(accent)`; `Button::new("取消")` enabled only when the form differs; `Button::new("複製")` (creates in the first writable record via `first_writable` + `create_charm` with the selected record as template); `Button::new("削除").fill(dark_red)` behind a `Modal` confirm that names the charm and says it can be undone.
- Raw data: `CollapsingHeader::new("生データ (書き換えません)")` with a `Grid` of +0x02 / +0x12 / +0x06..+0x09 and one sentence: these bytes are copied from the game's record and never invented.

### 3.3 The interaction, step by step

1. Click お守り. With nothing selected the pane says `お守りを選ぶか「＋ 新規」を押してください` — no live form over record 0, so the first action cannot fail mysteriously.
2. Scroll/filter: type `連撃` (or pick スキル = 連撃). The list drops from 188 to the 3 charms that have it, numbered 1..3. The No. column is stable and meaningful; the box index is a tooltip.
3. Click a row. The right pane fills: `お守り 42 / 188`, `創世の護石（レアリティ 100）`, skill names with points and allowed ranges, and the slot dots.
4. Click the スキル1 combo. A 260 px scroll list opens with a search field focused; type `じゅくれん` → `熟練 +0〜0`…; click 属性会心. The combo closes, the field now reads 属性会心, the points DragValue's range changes to 0〜10 and the current value is clamped if needed.
5. Adjust points with the DragValue (or drag). The label next to it always shows the legal interval, so no one has to guess whether +11 is legal.
6. Click 適用. The list row updates in place, the status says `お守り #229 を更新（Ctrl+Z で戻す）`, the dirty dot turns on. If you click another row instead, the pane shows `未適用の変更があります [適用] [破棄]` rather than discarding silently.
7. Click ＋新規. The first writable record is used, seeded from a real template (legal skills, real rarity bytes), the pane opens on it with the skill combo focused. 複製 does the same from the selected charm.
8. 削除 opens a modal that names the charm, then reports the core's actual result (today the bool is dropped).

### 3.4 Data and table work (all reuses what exists)

| Need | Source | Work |
|---|---|---|
| Rarity + class per charm | `gamedata::TALISMAN_RARITY` (index = +0x02 class), +0x12 | add `class: u8`, `rarity: u8` to `Charm` and fill them in `decode_box` (`charm.rs:130`) — S |
| Talisman type name | `EQUIP_TALISMAN_NAMES[class]` (11 entries; 0x0A is valid = last entry) | show it next to the rarity; tables are English today, so either translate or show `創世の護石 / Creator Talisman` — S |
| Legal points per rarity | `TALISMAN_{MYSTERY,SHINING,TIMEWORN,ENDURING}_SKILL` (206 entries, index = skill id), selected by rarity | `charm::point_range(skill, slot, rarity) -> Option<(i8,i8)>` parsing `"a~b:c~d"` — S |
| Talisman-legal skill list | the range tables have **206** rows; `skills.rs` has **306** names | show all 306 but mark ids ≥206 `(お守り不可)`; the reference editor's talisman combo holds exactly those 206 entries (including 「なし」) — S |
| New / duplicate | `create_charm`, `template_from_box`, `first_writable` (used by `hbedit.rs:598`) | GUI wrapper + `save::duplicate_charm` for 複製 — S–M |
| Undo | `equipment_box` snapshot (72,000 B) or a 5-field diff | S |

**One trap worth a test before wiring the ranges:** the Japanese name table and the C#-derived range tables agree for skill ids 1–202 but **disagree on 203–205**. `skills.rs` has 秘術 / 護石強化 / 胴系統倍加 where `gamedata::SKILL_NAMES` has Torso Up / Mystery / Talisman+. If you index the range table by the Japanese id, those three skills get the wrong range. Add a test that pins a handful of (id, name) pairs and decide which order is correct; until then, do not clamp 203–205. (Real charms in this save use ids 5–179, so the bug is latent, not visible.)

---

## 4. Should the GUI show names everywhere a name is known?

**Yes — with a rule.** The name is the primary text; the raw id stays visible as secondary, dim, monospace, because it is the interchange format with the CLI, the quest tools and other editors, and because a wrong name must be debuggable. Never show an id alone. Where the map is missing, write `不明 (0x0A)`, never an empty cell.

| Data | Table | Today | Show it in |
|---|---|---|---|
| Items | `ITEM_NAME_LIST` (2,991) | table + picker results | detail pane title, picker rows (name first, `#id` right), ID field hover, status messages (already) |
| Skills | `skills.rs` (306) | charm list summary only | charm combo, charm list, filter, any place an id is printed; mark ids ≥206 お守り不可 |
| Talisman type / rarity | `EQUIP_TALISMAN_NAMES`, `TALISMAN_RARITY` | nowhere | charm badge: type name + `レアリティ 100`; raw class in the 生データ section |
| Equipment (weapons/armour) | `EQUIP_*_NAMES` + `*_I_DS` (all English) | nowhere — there is no equipment screen | a read-only 装備 list in the same tab: 種別 / 名前 / 枠 #, using the type byte + id. This also explains the charm index gaps and shows what a mistyped index would overwrite. Editing weapons is out of scope; seeing them is not. |
| Decorations | `JWL_NAMES` | nowhere | only if a decoration-slot editor is added; not now |
| Quests | `Gmd::title()` (per `.arc`) | nowhere; the DLC filter even promises "id or title" (`app.rs:66`) but only matches ids (`app.rs:1738`) | DLC list column (クエスト名), DLC filter, quest tab header. **Cache once per save** (`HashMap<i32,String>`) — the previous all-titles-per-frame bug (`docs/final_review_loop.md` E11) was 6.2 ms/frame; decoding 175 titles once on open is <10 ms. |
| Monsters | `MONSTER_HUNT_NAMES` (English, no proven em-id map) | nowhere; `.sem` shows coordinates only | later, if the `em###` → id mapping is proven; do not guess |
| Palico | `dlcats` | yes | keep |

Two policy consequences:
1. **Language consistency.** Items and skills are Japanese; every equipment and monster table is still English. The moment you surface equipment names, a screen would mix `回復薬` with `Rathalos Helm`. Translate the equipment tables (the source is `GameConstants.cs`, `docs/japanese_names_handoff.md` lists the exact arrays) or show both names deliberately — but do not ship a half-translated list by accident.
2. **Search must accept names *and* ids** in every filter, because that is the one place where both audiences (player vs. reverse-engineer) meet. Today the item table does, the item picker doesn't, and the DLC filter claims to but doesn't.

---

## 5. Prioritised improvements

| # | Change | Effort | Why it matters |
|---|---|---|---|
| 1 | Charm: searchable **skill-name combo** in the form (name + legal range per row) | medium | The complaint. Without it the editor is a hex editor with a friendly list above it; 306 names are already in the crate. |
| 2 | Charm: show rarity and enforce the per-rarity point range; red when out of range + "範囲内に補正" | medium | The tables exist and are unused; today the tool can write records the game's own table forbids (already a known bug class). |
| 3 | Charm: move to its own **お守り** tab, master-detail, No. ordinals, list filter/sort | medium | 188 rows in a 150 px box, reachable only via 設定/オトモ; the index jumps 229→1397. |
| 4 | Item: **one mutation path** with dirty flag + **undo** (Ctrl+Z, 32 steps, ⟲ button) | medium | Today an inline count edit is not dirty, is lost on slot switch, and the bar says ○ 変更なし. No undo exists anywhere. |
| 5 | Item: picker becomes a **fixed-size modal** (focus on open, keys, no layout jump, fixed result height) | medium | 5 mouse actions per slot, the table jumps under the cursor while typing, and there is no keyboard path at all. |
| 6 | Item: picker searches **ID too**, with an explicit "N 件中 M 件" instead of a silent 40-hit cap | small | 133 names contain 弾; 40 are reachable; the table's filter and the picker disagree about ids. |
| 7 | Item: whole-row **selection** (`TableBuilder::sense(click)`), double-click / Enter to edit, Delete to clear | small | Only the 10 px number cell is clickable; users click names and nothing happens. |
| 8 | Charm: **新規 / 複製 / 削除(確認)**; no live form when nothing is selected | small | Core functions already exist; today the first write on a fresh screen fails on a weapon record. |
| 9 | Charm: decode and show **class + rarity** (type name), raw bytes preserved | small | Needed for #2 and for trust; the bytes are already preserved by the writer. |
| 10 | Item: copy to many — **スタンプ** mode + "空き枠に N 枠" (multi-select later) | medium | Explicit complaint; a 2,300-slot box is where bulk edits are the whole point. |
| 11 | Bulk ops move under **一括操作** with a count-confirming modal | small | One stray click on 重複を除去 deletes 5 of 6 回復薬 stacks, with no confirmation, no undo, generic message. |
| 12 | One count limit (**99**) for row, pane, modal and bulk | small | Three limits today (127/127/99); 127 is not a legal stack. |
| 13 | Wrap every tab in an outer `ScrollArea`; test at 900×600 | small | `tab_extras` has no outer scroll: at the minimum window the charm form is unreachable. |
| 14 | DLC: show the **quest title** (cached `Gmd::title`) and make the filter match it | medium | 175 rows of bare ids; the field's own comment promises titles; the perf problem is fixed by caching once. |
| 15 | Quest: show the title in the header | small | You can edit a quest without ever seeing its name. |
| 16 | Navigation: tabs to a top bar; ファイル / 変換 `MenuButton`; one primary 保存; 情報 → "?" | medium | 9 permanent buttons + a hint line; the tab rail lives inside the resizable info panel. |
| 17 | Unify the apply model (character tab's 反映 vs. immediate apply elsewhere) | medium | The same window has two mental models; one tab needs a button, the others don't. |
| 18 | Equipment: read-only **装備** list with names (+ translate the tables) | large | 601 weapon/armour records are invisible; the charm indices collide with them; names exist but are English. |
| 19 | Save safety: automatic `.bak` on overwrite + confirm on close with unsaved changes | medium | A destructive editor with no undo and no backup is one misclick from a lost save. |
| 20 | Monster names in `.sem` once the `em###` → id map is proven | large | Currently coordinates only; names exist but the mapping is unproven — do not guess. |

Ordering rationale: 1–5 are the complaints themselves; 6–9 are near-free and remove the remaining hunt; 10–13 make bulk edits safe; 14–18 are the "names everywhere" answer; 19 is the safety net the whole tool needs; 20 is explicitly speculative.

---

## 6. Screens I would merge or split

**Split**
1. **設定/オトモ → 設定 + オトモ + お守り.** It currently holds four unrelated jobs (options, palico, charms, whole-slot I/O) behind a name that mentions none of them. The charm editor is the only *content editor* in there and is the hardest to find in the app.
2. **クエスト tab into panes.** Entry list on the left (`SidePanel`, 180 px, keep the filter), editor in the centre, checker findings in a right `SidePanel` or a collapsible under the editor. Today the checker pushes the field editor down; on a small window the editor is below the fold.
3. **アイテム and お守り into list + right detail pane.** Both currently stack the editor above/below a long list, which is the source of the eye travel and the layout jump.
4. **Move "キャラクタをまるごと入出力"** out of 設定: it acts on the current slot, so it belongs on the キャラ tab (or in the ファイル menu), not next to brightness.

**Merge**
1. **情報 tab → a "?" button / ヘルプ menu.** It is a static about-box; it should not cost a tab.
2. **The 9 top-bar file/convert buttons → ファイル + 変換 menus**, with one primary 保存 button. "3DS 形式で書き出す" (rare) should not weigh the same as "上書き保存".
3. **".arc を開く" → the quest tab's empty state** (`まだ .arc を開いていません [.arc を開く] [セーブ内のクエストから選ぶ]`) and the DLC tab. It is the only way in, and it is currently in the global toolbar.
4. **The permanent hint line** ("ヒント: 編集後は…") → the export menu item's tooltip. It competes with the status bar, which is where real errors appear.

**Do not merge:** DLC and クエスト. One is a catalogue, one is a document; but the "編集" jump needs a breadcrumb: `← DLC 一覧に戻る` and the DLC list scroll position preserved.

---

## 7. Words that cost the user a second

- **枠 is overloaded four ways**: item box slot (箱 #), equipment record index (枠 229), DLC quest slot (枠 0), decoration slots (装飾スロット). Rename: **箱 #** / **装備枠 #** / **クエスト枠 #** / **装飾スロット**.
- **空にする** appears on an item slot, a DLC quest slot and (via 削除) a charm, with three different consequences.
- **切り離す** (quest tab) does not say from what or what happens after.
- **このスロットを使う/使わない 切替** is a stateful toggle phrased as an action.
- **重複を除去** does not say which duplicate survives, or how many will be removed.
- **編集内容を反映 (メモリのみ)** implies a mode that only the character tab honours.
- **スキル1 [1] [1]** is the worst label in the app: two identical numbers, no unit, no name. Once the combo exists, the row should read `スキル1  属性会心  +6（0〜10）`.

---

## 8. Evidence and reproduction

- Build: `cargo build --release -p mhnx-gui` (up to date, 3 warnings).
- Screenshots: `MHNXEDITOR_OPEN_SAVE=<save> MHNXEDITOR_OPEN_TAB=items MHNXEDITOR_ITEM_SELECT=3 MHNXEDITOR_ITEM_FILTER=回復 MHNXEDITOR_SCREENSHOT=...` (and the same for `extras`, `dlc`, `quest`, plus a no-hit and a blank-filter variant). Written to `.re_review_ux/`; inspected at 1600×1025 (egui at 1.25×) and with cropped 2× enlargements of the picker and the charm form.
- Charm inventory: `target/release/mhnx.exe charms <save> 1` → 188 charms at indices 224–229, 1397–1695, 1802–1994.
- Code references are to `crates/mhnx-gui/src/app.rs` unless stated; `charm.rs`, `hbedit.rs`, `quest.rs`, `gamedata.rs` for the data layer.
- No headless hook exists for selecting a charm, so the screenshot of the charm editor necessarily shows the default state (nothing selected, form on record 0) — which is itself one of the findings.
- The save was opened read-only; nothing was written back. `.re_review_ux/` is untracked scratch for this review.
