# GUI completeness review (adversarial)

Scope: does the PC GUI still do everything it claims? Reviewed commit **38d47a1**
(HEAD at the start of this session), release build `cargo build --release -p mhnx-gui`,
`crates/mhnx-gui/src/app.rs` (2,081 lines) plus the core code it calls.

Method. The documented headless hooks were used for screenshots and for the
save-in/arc-in loops. Because the client surface cannot be screen-captured in this
session (CopyFromScreen and PrintWindow both return a white client area), every
interactive claim below was verified by **injecting real mouse/keyboard input into the
window and then diffing the resulting save file byte for byte** — not by looking at
pixels. Screen geometry was taken from the egui framebuffer PNGs and cross-checked
against the click effects.

Real data (read-only): `C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system`,
4,726,188 B, SHA-256 `D06B72520E379F462D49098E55C270BA48B203C10A9DBF39A1F8EE7E410C112A`.
Every write went to a copy under `C:\anyWorkspace\_re`; the real save's hash and
timestamp are unchanged.

Workspace note: the tree kept moving while I reviewed. A third commit appeared
(1fd6375, "console: fix blockers found by the NRO review"); `git diff --stat 38d47a1 1fd6375
-- crates/mhnx-gui` is empty, so everything below applies to HEAD as well. The
*uncommitted* working tree is another session's work in progress
(`crates/mhnx-core/src/{dlc,entries,quest}.rs`, plus an untracked `docs/re_review_ux.md`)
and currently fails 2 `dlc::tests::*` tests because those edits tightened
`install_quest` without updating its tests; that is not a defect of 38d47a1, which I
re-checked in a pristine `git worktree` and which passes **105/105** workspace tests.

Hygiene: commit 1fd6375 also committed this review's scratch directory `.re_review/`
and other reviewers' `.re_review_ux/` (screenshots and probe scripts). They are not
source and should be removed from the repository.

---

## Verdict

Most of what the GUI claims now works, and the data layer is byte-exact where I could
measure it: item rename, character edit, quest field edit, save→arc→save write-back,
DLC extract/wipe, download Palico add/remove, charm select/edit/delete, 3DS→Switch
conversion, and all six secondary container viewers. Four things keep this from being
"complete":

1. **A debug build of the GUI crashes on any quest** (`app.rs:1354`, shift overflow);
   in release the same code path renders 10 dead controls that silently never write.
2. **The 249-byte hex editor and both "反映" buttons are laid out below the visible
   panel** at the shipped window size, so two documented features are unreachable.
3. **The DLC "全件抽出" writes to the process working directory when 抽出先 is empty**
   (its default), with a success message.
4. **Switching character slots silently discards unsaved edits.**

On top of that, three README/docs promises have no implementation in the GUI:
the one-click "1人限定" fix, installing a new download quest, and automatic
installed-bit maintenance. Details, evidence and repros below.

---

## CONFIRMED (my own measurements)

1. **Item rename by name survives 上書き保存.** Opened a copy, Items tab, selected
   箱 #4 (id 4), typed `回復薬` in 種類を変更, clicked the `9 回復薬` row, clicked
   上書き保存. Result: **exactly one byte** differs from the original (file offset
   0x126717, inside slot 1's item box). Re-reading the file: item slot index 3 is now
   `{id: 9, count: 99}` (was `{id: 4, count: 99}`); every other item slot, character and
   quest is untouched.
2. **Character edit survives 上書き保存.** Dragged 所持金 from 9,997,199 to 9,999,999,
   saved: 4 bytes changed (the two copies of the field: slot+0x24 and slot+0x280F).
3. **Quest field edit reaches the save.** Open quest 1010005 (from the save and from
   the DLC 編集 button), drag the 村ランク field, セーブに書き戻す, 上書き保存:
   the `.mib` byte at +0x11 changed 0x0A→0x19, 110 bytes differ in total and **all of
   them are inside the one 0x1400-byte quest slot**; 175 quests before and after; the
   character slots do not change.
4. **Unedited write-back is byte-identical** (README §セーブ内でクエストを編集する).
   `MHNXEDITOR_OPEN_QUEST=1010005 MHNXEDITOR_WRITE_BACK=1 MHNXEDITOR_SAVE_OUT=...`:
   SHA-256 of the output equals the input (`D06B…12A`).
5. **The DLC 編集 button works** (not just the env hook): 編集 on row 0 → drag a mib
   field → セーブに書き戻す → 上書き保存 gives the same one-byte `.mib` change.
6. **Download Palico count is right: 7 / 50.** 外す on #0 zeroes exactly one 0x144-byte
   record (139 non-zero bytes changed, file 0x4E..0x191; id 0x64 → 0). Re-adding the
   selected Palico and saving restores the file **byte for byte** (hash equals the
   original).
7. **Charms: select, edit, write, delete.** Selecting #224 and changing 装飾スロット
   3→0 then この内容で書き込む + 上書き保存 changes **exactly one byte** (file
   0x12E716 = record 224 +0x10). 削除 removes it: 188 → 187 charms, 9 bytes zeroed
   inside record 224 only. (Core-level: `set_charm` with unchanged values is a 0-byte
   no-op; a changed point is a 1-byte write; both read back correctly.)
8. **Slot export/import round trip** (core-level; the GUI uses file dialogs):
   `export_slot(0)` then `import_slot(0, …)` on the real save reproduces the file
   exactly (0 differing bytes, 1,173,440-byte slot).
9. **3DS → Switch conversion through the GUI.** Opened a 3DS-size body, clicked
   「3DS から Switch へ変換 (ヘッダを合成)」, 上書き保存: file 4,726,152 → 4,726,188 B,
   header +0x14 = `74 EE B2 36` (crc32("system")^0xFFFFFFFF), +0x1C = `88 1D 48 00`
   (body size), and the body is byte-identical to the 3DS body.
10. **DLC extraction and wipe.** 「ここへ全件抽出」 with a real path wrote 175 `.arc`
    files, all byte-identical to the save payloads. Per-row 抽出 wrote
    `q1010005.arc` (1,821 B). Per-row 空にする zeroes the slot (1,746 non-zero bytes
    in 0xF898..0x10C97), quests 175 → 174, and — as documented — the installed bit set
    does not move.
11. **Six secondary container viewers render real data**: .rem (9 records with names
    and editable count/slot), .sup, .sem (position + 実体/ダミー), .esl (waves),
    .qdp (flag hex), .qdl (15 refs). The .qdl duplicates I saw are in the file itself
    (raw records `b_m00em000_00` ×3, `z_m00d_000` ×2), not a parser bug.
12. **The quest checker runs and is quiet at Error level**: 141 shipped quests → 0
    errors, 2 warnings; 175 save quests → 0 errors, 0 warnings. The checker's
    `supply-summary` correctly counts the whole .sup (30 kinds / 167 items for
    1010001) — which is precisely what the .sup *viewer* fails to do (E4).
13. **Save-level identities**: `from_bytes`→`to_bytes` is identical; the item box
    decodes/re-encodes with 0 differing bytes on all three slots; rewriting an
    untouched character writes 0 bytes.
14. **MIB table**: 95 fields, 15 enum tables, every enum name referenced by the table
    resolves.
15. **Screenshot mode works for every tab** (save/items/quest/dlc/extras/about) and
    for each container type; no panics in the release build.

---

## ERRORS (file:line + repro)

### E1 — Debug builds panic on every quest; release builds show 10 dead controls
`crates/mhnx-gui/src/app.rs:1354` (`let hi = … (1i64 << width) - 1`), reached from the
field grid at `app.rs:1339-1374`.

The table has 10 `FieldKind::Bytes` fields, including `mob_slot[0..4]` (13 bytes,
`questfields.rs:99-103`) and `gmd_ref_name` (14 bytes). The GUI computes
`width = f.size * 8` = 104/112 and shifts an `i64` by it.

Repro:
```
cargo build -p mhnx-gui
MHNXEDITOR_SCREENSHOT=x.png MHNXEDITOR_OPEN_TAB=quest \
MHNXEDITOR_OPEN_ARC=dist/quests_all/q1010001.arc MHNXEDITOR_OPEN_ENTRY=14 \
target/debug/mhnxeditor.exe
```
→ `thread 'main' panicked at crates\mhnx-gui\src\app.rs:1354:37: attempt to shift left
with overflow`, exit 101, **no PNG**. This fires for any archive with a `.mib`, i.e.
every quest — a first-run `cargo run` dies as soon as the Quest tab opens.

In release the shift count is masked, so the app survives, but `read_u32` returns
`None` for `Bytes` (`questfields.rs:224`) and `write_u32` returns `false`
(`questfields.rs:242`). The widget still renders a `DragValue`; editing it does nothing
and the value snaps back to 0 on the next frame. Ten fields are deceptive no-ops.

### E2 — "全件抽出" with the default (empty) folder dumps 175 files into the CWD
`app.rs:117` (`dlc_outdir: String::new()`), `app.rs:503-517` (`extract_dlc`),
`app.rs:1708-1712` / `app.rs:1761-1765` (callers).

`std::fs::create_dir_all("")` returns `Ok(())` and `Path::new("").join("q…arc")` is a
relative path, so the empty default is accepted and everything is written to the
process working directory. Repro: start on the DLC tab, leave 抽出先 empty, click
「ここへ全件抽出」 → `files in cwd: 175` (measured in an empty directory used as the
app's working directory); the status line reads `175 件を  に書き出しました`.
Started from Explorer, the CWD is the exe's folder, so this silently litters `dist\`.
The same applies to the per-row 抽出.

### E3 — Switching character slots silently discards unsaved edits
`app.rs:801-804` — the slot click handler calls `reload_from_save()` without
`apply_to_save()` and without a confirmation. `reload_from_save` also clears
`self.items` (`app.rs:152-165`).

Repro: Save tab → drag 所持金 (9,997,199 → 9,999,999) → click slot 2 in the side rail
→ click slot 1 → 上書き保存. Measured result: **0 bytes differ** from the original
file; the edit is gone with no warning (the dirty dot was never set either).
The same happens to item edits (the row DragValue at `app.rs:1048-1052` does not even
set `dirty`).

### E4 — The .sup viewer stops at the first empty record and disagrees with the checker
`crates/mhnx-core/src/entries.rs:100-112` — `ItemTable::parse` breaks on the first
all-zero record for both .rem and .sup. On quest 1010001's `supp_1010001` (168 B) the
records are non-zero for indices 0..21 **and 32..39**; the viewer shows
「アイテム 22 件 / 収容 40 件」 and the eight records after the gap cannot be seen or
edited, while the checker on the same screen prints 「支給品 30 種類 / 合計 167 個」.
Since `to_bytes` writes `records[i]` at `first + i*4` (`entries.rs:128-134`), saving an
edit would also write the wrong slots for a table with a gap. (The current dirty
worktree contains an in-progress fix for exactly this; 38d47a1 does not.)

### E5 — "名前を付けて保存" keeps the old file name's CRC in the header
`app.rs:647-655` writes `save.to_bytes()` under any chosen name. `Save::refresh_header`
(`save.rs:730-734`) and `header_name` (`save.rs:740`) exist but are **never called**.
The tool's own model says the header at +0x14 is crc32(file name)
(`save.rs:184-192`, tests at `save.rs:917-928`), so saving as `system_backup` produces
a `system_backup` file carrying the `system` hash — inconsistent with the pair written
by `write_switch_pair` (`app.rs:203-223`), which does build both names correctly.
I did not verify what the console does with the wrong hash; the inconsistency is in
the file the GUI writes.

### E6 — The DLC list has no titles and the filter cannot match them
`app.rs:66` documents the filter as "id or title"; `app.rs:1738` only tests
`format!("{}", q.quest_id)`. The list shows slot / id / size only (`app.rs:1728-1753`).
The CLI already decodes the title from the .gmd (`mhnx-cli/src/main.rs:537-545`), so
the GUI is behind its own command-line tool here.

### E7 — The entry counter is printed backwards
`app.rs:1210-1217` prints `"{len} / {selected+1}"`. With 15 entries and entry 0
selected the tab reads 「エントリ 15 / 1」, which reads as "15 of 1".

### E8 — Dead expression in the DLC list
`app.rs:1736-1737`: `let name = item_name(0); let _ = name;` — a leftover that does
nothing (and would be the wrong name anyway).

### E9 — Three public methods are dead code (compiler warnings)
`scan_quest_dir` (`app.rs:449`), `install_dlc` (`app.rs:465`), `remove_dlc`
(`app.rs:553`). `cargo build` prints "methods … are never used". The first two are the
remains of the (now missing) folder → save installer; the third duplicates the inline
call in the DLC tab.

### E10 — The item name picker silently truncates at 40 hits
`app.rs:1115-1126`. Match counts on the shipped table: 薬 29, 玉 60, 弾 133, の 1,876.
Anything past the 40th match cannot be picked by name at all; there is no paging and no
hint that the list is truncated.

### E11 — The checker's fixable finding has no action in the GUI
`app.rs:1662-1684` renders findings as labels only. `check.rs:99-110` marks
one-player-only as `fixable()`, core ships `clear_one_player_only` (`check.rs:342`),
and both the CLI (`mhnx-cli/src/main.rs:261`) and the console state machine
(`hbedit.rs:846`) use it — but the PC GUI never calls it, and there is no button.
`README.md:39` says 「**その場で解除できる**」 and `docs/quest_troubleshooting.md:25,44`
says 「PC 版は解除ボタン」 / 「「1人限定」の行を選んで … (PC 版は解除ボタン)」. There is
no such button. The condition *can* be changed by hand in the 参加条件1 dropdown, but
the promised one-click fix does not exist.

---

## Missing compared with README / docs

* **Installing a new download quest** (README.md:49 ".arc をセーブの DLC 領域 (205 枠) に
  直接インストール / 削除"). The GUI can only *replace* a quest the save already lists
  (open → edit → 書き戻す) and *delete* one. `install_dlc`/`scan_quest_dir` are dead
  (E9), and the core refuses anything not already present
  (`dlc.rs:307-309`), so no code path can add a quest today.
* **"インストール済みビット列の自動設定"** (README.md:50). `dlc.rs:19-40` documents at
  length that the module deliberately **never writes the set** (it is catalogue-indexed,
  not slot-indexed); only the tests poke bits (`dlc.rs:496-505`). The README promises
  the opposite of the design.
* **3D 表示 / スライドパッドプロ** (README.md:133 lists them for OPTIONS;
  `save.rs:755-764` carries them). The PC 「本体オプション」 section
  (`app.rs:1796-1812`) exposes only 明るさ and 振動, so two options are read from and
  written back to the save but can never be changed (they survive because
  `set_options` writes the whole struct).
* **"その場で解除"** (README.md:39, docs/quest_troubleshooting.md:25) — see E11.
* **Save checking**: `check_save` (`check.rs:353`) is reachable from the CLI but no
  GUI tab ever calls it. Not promised by the README, so listed for completeness only.
* The README's "arc の再構築は元の圧縮バイトを保持するため、**サイズが変わりません**"
  (README.md:376) is not true for an edited entry: changing one `.mib` byte grew quest
  1010005's archive from 1,821 to 1,832 bytes (only untouched entries keep their
  original zlib stream, `arc.rs:205-224`). The save survives because the slot has
  0x1400 bytes; the sentence is still wrong.
* README.md:45 "141 件すべてで試し、誤って警告を出さない" is not literally true:
  q1010165 and q1011045 produce a Warn (cond1 = 0x2F = 「１人限定」 in the `san_`
  table). Either those official quests really are solo-only (then the warning is a
  false positive) or the claim needs rewording.
* **DLC titles** are missing from the list (E6), although the CLI prints them.

Minor: the .rem/.sup record editors let you change 個数 and スロット but not the
**item id** (the most important field); `.rem`/`.sup` also write into the arc
immediately on change (`app.rs:1565-1573`), which makes the 「反映」 button cosmetic.

---

## Unreachable UI

The Quest tab is a single non-scrolling column (`tab_quest`, `app.rs:1158-1483`);
only its inner lists scroll. At the shipped default window (1280x820 logical, which is
what the 1600x1025 framebuffer is at 125% scaling) the content needs roughly 1,200
logical px.

* **U1 — the 249-byte hex grid (`app.rs:1391-1453`) and the
  「.mib の変更を .arc に反映」 button (`app.rs:1450-1453`) are never drawn.** In the
  `.mib` screenshots the field list starts at y≈580 and is cut by the panel edge; the
  heading 「全 249 バイト (16 進)」 does not appear at all, although it is drawn
  unconditionally after a 420-logical-px field list. There is no outer ScrollArea and
  no way to reach the block (resizing only helps on a >1440p-class window).
  README.md:29 ("未確定領域は 249 バイトの 16 進グリッドで直接編集") is therefore
  false in practice.
* **U2 — 「.gmd の変更を .arc に反映」 (`app.rs:1476-1479`) is below the panel** for the
  same reason: the gmd ScrollArea alone is 360 logical px after ~430 px of header
  content. The screenshots show the gmd text running into the status bar and no button.
* **U3 — the instruction at `app.rs:1394-1396` ("変更後は下の反映ボタンを押してください")
  points at U1's invisible button.** (The data is not actually lost: `store_arc_entry`
  also runs on entry switch, `.arc` save and セーブに書き戻す.)

Everything else fits: the Items detail panel was deliberately moved above the table
(`app.rs:987-989`), the DLC list is bounded at 420, the Palico list at 140 and the
charm list at 150, and the Extras tab's last row (slot export/import) is visible at the
default size.

---

## Not verified (explicitly)

* File-dialog features: セーブを開く, 名前を付けて保存, Switch 2-file output,
  3DS export, .arc open/save, slot export/import, 3DS body import. The code is simple
  and the core side is tested, but I could not drive `rfd` headlessly.
* MHGU: no sample save; only the code path was read (the DLC tab refuses MHGU by
  design).
* Behaviour on real hardware (the game's reaction to a stale header, to a wiped quest
  slot, etc.) — outside what a review can measure here.

---

## Prioritised fixes

1. **`app.rs:1354`** — handle `FieldKind::Bytes` explicitly (per-byte hex or a read-only
   view) instead of computing `1i64 << (size*8)`. Removes the debug crash and 10
   no-op controls.
2. **Empty 抽出先 (E2)** — reject empty/relative paths, default to a real folder, add a
   folder picker; never write to the CWD.
3. **Slot switch (E3)** — call `apply_to_save()` (or confirm) before
   `reload_from_save()`; set `dirty` when a row count changes.
4. **Quest tab layout (U1/U2)** — wrap the tab in a `ScrollArea`, or move the hex grid
   and both 反映 buttons above the entry list (or into a collapsible section); fix the
   instruction text either way.
5. **.sup viewer (E4)** — scan all 40 fixed slots (that is already being fixed in the
   dirty tree) so the viewer and the checker agree and the post-gap records are editable.
6. **One-player fix (E11)** — a 解除 button on the finding, using
   `check::clear_one_player_only`; otherwise delete the claim from README/docs.
7. **DLC install** — either implement it (which needs the catalogue bit) or
   correct README.md:49-50; do not leave a dead `install_dlc` behind.
8. **DLC list titles + title filter (E6)** — the CLI already has the decoder; reuse it.
9. **Options (3D / circle pad)** — expose 3D / スライドパッドプロ or remove them from the model and
   the README.
10. **Header on save-as (E5)** — call `refresh_header(name)` with the chosen file name,
    or refuse names other than system/system_backup.
11. Small: entry counter order (E7), dead code and warnings (E8/E9), picker paging
    (E10), .rem/.sup item-id editing.
