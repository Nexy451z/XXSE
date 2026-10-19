# Re-review: Nintendo Switch (NRO) build vs the desktop GUI

Scope: `crates/mhnx-core/src/hbedit.rs`, `switch/src/*.rs`, `crates/mhnx-gui/src/app.rs`.
Data: the Ryujinx save at `C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system`
(read only) and the 141 shipped `*.arc` in `C:\anyWorkspace\nx\mhxx\save_editor\mhxx_quetool\quest`.

Every claim was produced by running something in this workspace unless marked
**by inspection** or **unverified**. Nothing was run on a Switch or an emulator. No
repository file was modified for this review other than this report; two throw-away
integration tests were used and deleted (the tree is clean apart from the pre-existing
untracked scratch dirs `.re_review/` and `.re_review_ux/`).

---

## 1. Verdict

**The NRO builds and links, the shared state machine is real, and the FFI layer is
byte-correct against the installed libnx headers. It is still not a usable port, and it
is not safe to point at an un-backed-up save.**

The build is sound (section 6). The problems are behavioural, and four are blockers:

1. **Quest text edits are truncated to 60 characters and then overwrite the original**
   string. All 141 shipped quests contain a `.gmd` string longer than 60 characters
   (longest 117), so opening any quest description and pressing OK destroys half of it.
2. **The item picker is dead.** `mhnx-core` is built for the Switch with
   `features = []`, so `gamedata` is off, `item_pick_matches()` always returns an empty
   list, and **the console has no working way to change an item id at all.**
3. **Unsaved `.mib`/`.gmd` edits are silently discarded** when the user moves to another
   arc entry or runs the quest checker. The GUI flushes explicitly
   (`select_arc_entry`, app.rs:389-396); `hbedit` does not.
4. **Three screens draw a cursor that never moves** (CHARACTER SLOT, QUEST CHECK,
   DOWNLOAD PALICO). On the checker this is dangerous: the highlight stays on finding
   #1 and A applies the fix to finding #1 whatever the user navigated to.

Beyond that, about 20 GUI features have no console equivalent (six entry viewers,
3DS conversion, `.saveslot`, raw `.mib` hex editing, monster-slot editing, item-type
editing, DLC uninstall, ...), and the test suite that is supposed to prove the on-device
flow only passes because of a cargo feature-unification accident.

---

## 2. Screen inventory

### 2.1 Console (`hbedit::Screen`, 15 screens)

| Screen | Keys | What it actually does |
|---|---|---|
| SaveList | Up/Down, A, Y, X, + | Lists saves from 3 folders; A asks the I/O layer to read one; Y -> ArcList; X -> Options (opens even with no save; edits are then dropped, hbedit.rs:794) |
| Slots | Up/Down, A, X, L, R, Y, B | Picks 1 of 3 slots; A -> Char, X -> Items, L -> Charms, R -> Palico, Y -> ArcList |
| Char | Up/Down, Left/Right, L/R, X, A, B | 17 scalar fields / name via keyboard; L/R switch slot; X cycles step; A writes the save |
| Items | Up/Down, Left/Right, X, Y, R, A, B | 2300 slots, count only (clamped 0..127), show/hide empty, R -> ItemPick |
| ItemPick | Up/Down, Left/Right (+-10), Y, X, A, B | Name filter -> set the item id. **Non-functional, see E1** |
| ArcList | Up/Down, A, X, Y, + | `.arc` files from 5 folders; A reads one; X -> SaveQuests |
| SaveQuests | Up/Down, A, Minus, R, B | Quests inside the save; A opens one, Minus/R extract 1/all |
| ArcEntries | Up/Down, A, X, L, Minus, R, B | Entry list; A edits `.mib`/`.gmd` only (anything else: "this entry type cannot be edited"); X installs into the save; L runs the checker; Minus/R extract |
| Mib | Up/Down, Left/Right, X, A, B | 87 of 95 fields; the 8 `FieldKind::Bytes` fields (93 bytes, incl. all 5 monster slots) are read-only; A "writes the .arc" |
| Gmd | Up/Down, A, X, B | One string at a time through the keyboard; X writes the `.arc` |
| Check | Up/Down, A, X, B | Checker findings; A applies the one-player-only fix |
| Charms | Up/Down, A, Y, L, X, B | List/delete (A), edit (Y), new (L), write save (X) |
| CharmEdit | Up/Down, Left/Right, X, A, B | 6 rows: slot, deco slots, both skills and points |
| Palico | Up/Down, A, L/R, X, B | Download Palico add/remove, JP/EN names |
| Options | Up/Down, Left/Right, A, B | Brightness, rumble, disable 3D, circle-pad-pro |

### 2.2 GUI (`app.rs`, 6 tabs + top bar)

* **Top bar** (632-722): open save, overwrite, save-as, "Switch 用 (system 2 ファイル)",
  "3DS 形式で書き出す", open `.arc`, 3DS->Switch conversion, import a 3DS body.
* **Save tab** (850-953): name, play time, funds, HR, HRP, academy, 4 village points,
  gender, voice/eye/clothing/hairstyle/face/features, **4 RGBA colour pickers**, apply,
  **used/unused toggle**.
* **Items tab** (955-1156): name/id filter, show-empty, **all -> 99**, **dedupe**, table
  with names, per-row count, detail panel (numeric id, count, clear), **name search**.
* **Quest tab** (1158-1483): entry list **with name filter**, `.mib` 95 fields with
  Japanese labels, confidence colours and a Quedixx column, enum drop-downs, **249-byte
  hex grid**, `.gmd` multiline editor, **`.rem/.sup/.sem/.esl/.qdp/.qdl` viewers**,
  checker, save-as, write back into the save.
* **DLC tab** (1686-1778): installed quests, filter, extract all/one to a chosen folder,
  edit, **wipe a slot**, MHGU guard.
* **Extras tab** (1780-2051): brightness, rumble, Palico add/remove, charms,
  **`.saveslot` export/import**.
* About tab, plus an offscreen screenshot smoke test (596-623).

---

## 3. Parity table - GUI features with no (working) console equivalent

Y = present, ~ = present but broken/partial, N = absent.

| GUI feature | GUI ref | Console | Note |
|---|---|---|---|
| `.rem` / `.sup` item tables (view + edit counts/slots) | app.rs:1512-1582 | N | ArcEntries A handles mib/gmd only (hbedit.rs:1253-1263) |
| `.sem` monster placement | app.rs:1583-1602 | N | |
| `.esl` small-monster waves | app.rs:1603-1614 | N | |
| `.qdp` quest flags | app.rs:1615-1624 | N | |
| `.qdl` reference list | app.rs:1625-1646 | N | |
| Item **name** search / name column | app.rs:1042-1047, 1097-1155 | ~ | Screen exists, always 0 matches (E1); names print as `#123` |
| Item id numeric edit | app.rs:1074-1081 | N | Count only; the id can only come from the dead picker |
| "All items 99" / dedupe / clear slot | app.rs:964-984, 1090-1093 | N | |
| Item filter by name or id | app.rs:992-1006 | N | Only "show empty" |
| `.mib` 249-byte hex grid | app.rs:1392-1449 | N | 8 `Bytes` fields incl. 5 x 13-byte monster slots are uneditable (hbedit.rs:1308-1311, questfields.rs:98-103) |
| `.mib` Japanese names / confidence / Quedixx | app.rs:1292-1381 | N | Console prints `key` truncated to 16 chars |
| Quest-id decode (kind/rank/serial) | app.rs:1255-1272 | N | |
| `.gmd` multiline edit | app.rs:1454-1479 | ~ | Keyboard capped at 60 chars -> data loss (E2) |
| `.arc` entry name filter | app.rs:1236-1243 | N | |
| DLC list filter | app.rs:1716-1740 | N | |
| Wipe/uninstall a DLC quest | app.rs:1750-1752, 1766-1777 | N | `dlc::uninstall_quest` is never referenced by hbedit or switch |
| Extract to a chosen folder | app.rs:1706-1712 | N | Fixed `OUT_QUEST_DIR` |
| Save-as / export 3DS / import 3DS body | app.rs:647-669, 700-715 | N | Console only writes back in place |
| 3DS <-> Switch conversion | app.rs:259-273, 691-699 | N | The scanner will happily open a 3DS-sized file named `system` |
| system + system_backup pair export | app.rs:203-223, 656-660 | ~ | Only the in-place backup; always Switch-format, even for a 3DS save (E9) |
| `.saveslot` export/import | app.rs:2012-2050 | N | |
| Slot used/unused toggle | app.rs:944-950 | N | |
| 4 x RGBA character colours | app.rs:919-937 | N | `CHAR_FIELDS` has no colour rows (hbedit.rs:167-185) |
| Save info panel (DLC offset, slot bases, header+0x14) | app.rs:757-780 | N | Kind + body size only (hbedit.rs:1444-1458) |
| MHGU guard in the DLC tab | app.rs:1692-1697 | N | `SaveKind::Mhgu` appears only in a display string (hbedit.rs:1452) |
| Checker **fix** | - | Y (console only) | The GUI lost this in the rewrite |
| Options: disable 3D, circle-pad-pro | - | Y (console only) | GUI has brightness/rumble only |
| Auto-rewrite `system_backup` on save | - | Y (console only) | The GUI "上書き保存" leaves the backup stale |

### On "equipment name search"

There is **no equipment editor or equipment search in the current GUI either**
(grep for `装備`/`equip` under `crates/mhnx-gui` finds only a comment in main.rs:4). This
is not a GUI-vs-console gap; it looks like something the truncation removed from *both*
sides. Evidence: `gamedata.rs` carries 17 `EQUIP_*` / `EQUIPMENT_TYPES` tables (lines
384-585, ~240 KB of the 308 KB file) and **nothing outside gamedata.rs references any of
them** - not the GUI, not the CLI, not hbedit. `ITEM_NAME_LIST` is the only table with a
consumer. Whoever restores the equipment UI has to re-add the caller, not just the data.

---

## 4. CONFIRMED

Measured, with the command or snippet that produced each result.

### C1. `gamedata` is off in the Switch build -> no item names, dead picker

`switch/Cargo.toml` uses `mhnx-core = { ..., default-features = false }` and never
enables `gamedata`. The build fingerprint says so:

```
Get-Content switch\target\aarch64-nintendo-switch-libnx\release\build\mhnx-core\*\fingerprint\lib-mhnx_core.json
{"rustc":..., "features":"[]", "declared_features":"[\"default\", \"gamedata\", \"std\"]", ...}
```

Independent confirmation from the linked artifact: the shipped `dist/mhnxeditor.nro`
(575,685 B) contains none of the gamedata-only strings `Petrified Blade`,
`Leather Headgear`, `Great Sword`, `(unknown `; it does contain `PICK ITEM` and
`アイテム名で絞り込み`, so the artifact is not stale. Consequence: hbedit.rs:1065-1069
returns an empty Vec on every call, and hbedit.rs:160-162 prints `#<id>` instead of a
name.

### C2. The console cannot change an item type at all

With C1, `Screen::ItemPick` shows `filter: (all)   (0 match)` for every input and A can
never select anything (hbedit.rs:1096-1118). No other screen writes `ItemSlot.id` (the
only assignment reachable from input is hbedit.rs:1103). The GUI can still type an id
(app.rs:1074-1081) even without gamedata; the console cannot.

### C3. `.gmd` edits are truncated to 60 characters

`switch/src/editor.rs:484` calls `swkbd::input_text(header, &initial, 60)` for **every**
text request. `swkbd.rs:406-409` turns that into `chars = 60`, sets `stringLenMax = 60`
(swkbd.rs:425) **and pre-truncates the initial text to 60 UTF-16 units**
(`c_string(initial, 60)`, swkbd.rs:342-356). The result goes to `apply_text`
(hbedit.rs:339-347), which replaces the whole string. Opening a long quest description
and pressing OK rewrites it as its first 60 characters.

Measured over the 141 shipped quests (throw-away integration test, deleted):

```
quests=141 strings=987 max_strings_per_quest=7
strings longer than 60 chars: 141 in 141 quests
longest string: 117 chars in q1010105.arc
```

**Every shipped quest loses text** through this path. The GUI multiline editor has no
such cap.

### C4. Unsaved `.mib`/`.gmd` edits are discarded on navigation

`input_arc_entries` (hbedit.rs:1245-1247) does `self.entry = self.cursor; self.load_entry();`
without `store_entry()`. `load_entry` (hbedit.rs:418-438) clears
`mib_bytes` and `gmd`. `run_check` (hbedit.rs:819-820) calls `load_entry()` too, so
**pressing L also throws the edits away**. Only `request_write_arc` (hbedit.rs:894) and
`install_into_save` (hbedit.rs:1384) flush.

Repro (ran, then deleted):

```rust
let mut ed = Editor::new();
ed.open_arc_bytes(ARC, &std::fs::read(ARC).unwrap());
ed.input(Key::A);                      // .mib screen
let before = ed.mib_bytes().to_vec();
ed.input(Key::Right);                  // edit a field: bytes change, dirty = true
ed.input(Key::B);                      // back to the entry list
ed.input(Key::Down); ed.input(Key::A); // open the next entry
// the archive still holds the ORIGINAL .mib bytes
assert_eq!(ed.arc.unwrap().entries[mib_idx].data, mib_orig);  // passes => edit lost
```

### C5. Three screens draw a cursor that never follows the input

Two cursors exist (`cursor` plus a per-screen one); on these screens the input moves one
and the renderer draws the other.

| Screen | input moves | renderer highlights | measured |
|---|---|---|---|
| Slots | `self.slot` (hbedit.rs:935-936) | `i == self.cursor` (hbedit.rs:1473) | slot=2 after 2xDown, marker still on row 0; A opens "CHARACTER slot 3" while slot 1 is highlighted |
| Check | `self.cursor` via `move_cursor` (833-834) | `check_cursor` (1752); A uses `check_cursor` (838) | 3 findings; Down -> cursor=1, check_cursor=0, marker on finding 1 |
| Palico | `self.cursor` (737-738) | `palico_cursor` (1873); A uses `self.cursor` (748) | cursor=2, palico_cursor=0, marker on row 0 -> A installs the hidden row |

On Check this is worse than cosmetic: the fix is applied to finding #1 no matter which
row the user thinks is selected, and it writes into the quest.

### C6. "A write the .arc" is a silent no-op for a quest opened from the save

`request_write_arc` (hbedit.rs:894-907) only acts `if let Some(p) = self.arc_path`;
there is no `else`. `open_quest_from_save` sets `arc_path = None` (hbedit.rs:1192) and
the Mib footer still says `A write .arc` (hbedit.rs:1673-1678 is the entry screen).
Repro (ran): open the save -> Y -> X -> A, enter the `.mib`, edit, press A ->
`take_write()` is `None` and `ed.message` is unchanged.

### C7. Duplicate match arm hides the item-picker hint

`switch/src/editor.rs:352` and `:356` both match `Items`. The compiler says so on every
build (`warning: unreachable pattern --> src\editor.rs:356:9`), and the reachable arm is
the stale one: it says `Y filter` (Y is actually show-empty) and never mentions `R`, so
the only route to the picker is undiscoverable on the device.

### C8. `extract_quests` reports stale write failures

`LAST_EXTRACT_FAILED` (editor.rs:177) is only stored at the end of `extract_quests`
(editor.rs:172). The early return at editor.rs:151-153 (`list.is_empty()`) and the
"no save" path at editor.rs:432-435 leave the previous value in place, and editor.rs:437
loads it unconditionally, appending `- N write(s) FAILED (folder missing?)` to an
unrelated message. A quest whose payload is empty is skipped silently (editor.rs:161-163)
and reported as `nothing written ... no matching quest in this save`, although core
distinguishes "no payload (size N)" (hbedit.rs:1174-1176).

### C9. `make_dirs` ignores every error

editor.rs:132-142 calls `make_dir` and drops the `bool`. The failure is visible only
through the counter above. If the SD card is read-only the user gets "write(s) FAILED
(folder missing?)" instead of "cannot create directory", and there is no way to tell
"folder missing" from "disk full".

### C10. `OUT_QUEST_DIR` and `QUEST_DIRS` **do** agree - the mismatch is elsewhere

editor.rs:64 `OUT_QUEST_DIR = /switch/mhnxeditor/mhxx/quest/arc` is literally
`QUEST_DIRS[0]` (editor.rs:52), so extracted files are rescanned and listed. The real
inconsistencies are documentation-level:

* hbedit.rs:1636 prints `/switch/mhnxeditor/quest/` as the quest folder - one of the
  five scan roots, but **not** where extraction writes.
* README.md:121-122 puts `q1010165.arc` directly in `quest/`, while README.md:316 and
  README.md:333 use `mhxx/quest/arc/` and `quest/arc/`.
* hbedit.rs:1434 prints `/switch/mhnxeditor/saves/` while `SAVE_DIRS` has three roots.

### C11. The post-extract rescan moves the entry cursor

editor.rs:443 calls `ed.set_arc_files(scan_quests())` after every extraction;
`set_arc_files` (hbedit.rs:316-321) resets the **shared** `cursor` when it is out of
range. On `Screen::ArcEntries` that cursor is the entry index. Measured: cursor 3 -> 0
after a rescan with one file in the list. Pressing A afterwards loads entry 0 and wipes
the `.mib` buffer (C4).

### C12. Documentation drift (measured)

* README.md:83 says the NRO is "約 244 KB"; switch/README.md:76 says both routes produce a
  `235,717` byte NRO. The artifact built here is **575,685 bytes**
  (`dist/mhnxeditor.nro`, NRO header size field 0x87000).
* README.md:152 says Y switches save/quest editing - true on the save/arc lists, but Y
  on Items toggles empty slots and on Charms edits a charm.
* README.md:153 says L/R switch character slots - on the slot screen L opens charms and
  R opens Palico (hbedit.rs:937-942).
* README.md:136 says the console edits "95 個の項目" - it edits 87; 8 are read-only.
* README.md:161-162 ("クエスト名やアイテム名もそのまま読めます") implies item names on the
  console; there are none.

### C13. `cargo test --workspace` hides the console behaviour

`mhnx-cli` depends on `mhnx-core` with `features = ["std","gamedata"]`; cargo unifies
features across the workspace, so the hbedit tests run with `gamedata` **on** while the
Switch build runs with it **off**.

```
cargo test --workspace                       -> 105 passed, 0 failed
cargo test -p mhnx-core --tests --no-fail-fast
  hbedit_flow: 24 passed; 2 failed:
    r_on_the_item_box_opens_the_picker            (hbedit_flow.rs:850)
    filtering_and_choosing_changes_the_item_type  (hbedit_flow.rs:870)
```

The README own verification command is `cargo test -p mhnx-core` (README.md:254), which
is red. The two tests encode exactly the assumption the console build violates.

### C14. FFI declarations match the installed libnx

Compiled a C probe (`-c`, all `_Static_assert`) against `C:/devkitPro/libnx/include`
reproducing every `const _: () = {...}` assertion in `sys.rs` and `swkbd.rs` (struct
sizes/offsets, `PAD_ANY_ID_MASK = 0x1000100FF`, `HidNpadStyleSet_NpadStandard = 0x1F`,
the 13 button bits, `FsDirEntryType_File`, `SwkbdConfig`/`SwkbdArgCommon`/`SwkbdArgV7`).
gcc exited 0. This is the one part of the port verifiable to the end without a console.

### C15. Item-picker sentinel and bounds: no collision, no OOB, but silent

* `TEXT_NAME = usize::MAX` (hbedit.rs:147), `TEXT_ITEM_FILTER = usize::MAX - 1`
  (hbedit.rs:164). `.gmd` indices are `gmd_cursor < strings.len()` (max 7 in the shipped
  quests, hbedit.rs:1364), so no legitimate request can equal either sentinel.
  editor.rs:474-480 tests them in the right order before the `apply_text` fallback.
* The match list is **not** bounded (up to 4095 ids with an empty filter,
  hbedit.rs:1055-1062) but the renderer caps at 16 rows (hbedit.rs:1570) and the cursor
  arithmetic is modulo-based, so it cannot leave the list.
* `apply_item_filter` resets the cursor to 0 (hbedit.rs:1075) and X clears both
  (hbedit.rs:1092-1095), so a shrinking filter cannot strand the cursor.
* **A with no matches is safe** - `matches.get(cursor)` returns `None` and nothing
  happens (hbedit.rs:1098). But the screen stays put with no message, so the user gets
  no feedback. `the_item_picker_does_not_panic_on_an_empty_match` (hbedit_flow.rs:914-933)
  locks in the no-panic property, not the missing feedback.

---

## 5. ERRORS - file:line + repro

Severity: **S1** destroys user data, **S2** silently wrong behaviour, **S3** cosmetic/doc.

| # | Sev | File:line | Problem | Repro |
|---|---|---|---|---|
| E1 | S2 | switch/Cargo.toml:20, hbedit.rs:1065-1069 | `gamedata` off -> picker always empty, no item names anywhere | `cargo test -p mhnx-core --test hbedit_flow`; or the fingerprint json (C1) |
| E2 | **S1** | switch/src/editor.rs:484, swkbd.rs:409 | text edits capped at 60 units and the initial text is truncated before display -> rewriting a `.gmd` string amputates it | measurement in C3 (141/141 quests); on device: open a quest, enter the description, OK |
| E3 | **S1** | hbedit.rs:1245-1247, 819-820 | `.mib`/`.gmd` working copy discarded on entry switch and on checker run | snippet in C4 (ran) |
| E4 | **S1** | hbedit.rs:1473 / 1752 / 1873 | three screens highlight a cursor that never moves; Check applies the fix to the wrong finding | snippet in C5 (ran) |
| E5 | S2 | hbedit.rs:894-907 + 1192 | A on the `.mib` of a save-resident quest writes nothing, says nothing | C6 (ran) |
| E6 | S2 | switch/src/editor.rs:352, 356 | duplicate `Items` arm; the lost arm is the accurate hint | `pwsh -File switch\build.ps1` prints `unreachable pattern` |
| E7 | S2 | switch/src/editor.rs:151-153, 172, 437 | stale `LAST_EXTRACT_FAILED`; empty payload skipped without explanation | by inspection (switch-only code, not runnable on host) |
| E8 | S2 | hbedit.rs:1308-1311, 1712 | 8 `Bytes` fields (93 B incl. 5 monster slots) uneditable; the cursor still lands on them because `selectable` is ignored by `move_cursor` | open q1010165, Down to `mob_slot[0]`, press Left/Right |
| E9 | S3 | switch/src/editor.rs:456-464 | for a 3DS save the backup is written as a *Switch-format* `system_backup` (+36 B header) next to a 3DS `system` | copy a 3DS save as `system`, open it, press A on Char |
| E10 | S3 | save.rs:412-416 (`raw.len().min(31)`) | a name cut at 31 **bytes** can split a UTF-8 sequence; the console allows up to 60 chars | by inspection (shared with the GUI) |
| E11 | S3 | hbedit.rs:316-321 + switch/src/editor.rs:443 | rescan after extraction resets the arc-entry cursor | C11 (ran) |
| E12 | S3 | switch/src/editor.rs:544-546 | `+` exits before the editor sees it, so `Key::Plus`/`ed.quit` is dead code on the console | by inspection |
| E13 | S3 | switch/src/editor.rs:562-573 | a held direction outranks any newly pressed A/B/X/Y (`key_from` order), so buttons are swallowed while a direction is held | by inspection |
| E14 | S3 | hbedit.rs:1636, 1434; README.md:121-122/333 | on-screen folder names != `QUEST_DIRS`/`SAVE_DIRS` != README layouts | C10 |
| E15 | S3 | README.md:83, switch/README.md:76 | NRO size claims (244 KB / 235,717 B) vs 575,685 B | `(Get-Item dist\mhnxeditor.nro).Length` |

---

## 6. Build status (verified here)

```
pwsh -File switch\build.ps1
  devkitPro  = C:/devkitPro
  rustc 1.101.0-nightly (b6eeb93c9 2026-10-18)
  warning: unreachable pattern --> src\editor.rs:356:9
  NroStart : marker=HOMEBREW
  NroHeader: magic=NRO0 size=0x87000 (552960 bytes)
  file size: 575685 bytes
  mod0     : MOD0 @ 0xF0
  OK: mhnxeditor.nro built.
```

The build script verifies only the NRO container (magic, size, MOD0). It never executes
the binary.

---

## 7. What cannot be verified without hardware

Everything below is **unverified**; I did not run the NRO anywhere.

1. **Does anything draw at all?** `Canvas::new` (text.rs:292) needs a `pl:u` session and
   the standard shared font. If it returns `None` the code falls back to `consoleInit`
   (editor.rs:512-516) - two very different UIs, and only source reading covers either.
2. **Panics are invisible in canvas mode.** `print_panic` (app.rs:40-50) uses `printf` and
   `consoleUpdate(NULL)`, but when the Canvas is up `consoleInit` was deliberately
   skipped (editor.rs:507-516). Whether libnx default stdio is a null device before
   `consoleInit` decides whether a crash prints anything or freezes on the last frame.
3. **Software keyboard availability.** `swkbd::is_available()` is false when the process
   is not `AppletType_Application` (swkbd.rs:374-379). Launched from the album, all text
   entry (name, `.gmd`, item filter) is unavailable and the only feedback is
   "keyboard unavailable in this mode" (editor.rs:481-482). README.md:117/163 does not
   mention this requirement.
4. **Whether the swkbd assumptions hold** - `MAX_TEXT_UNITS = 248`, the 249-unit
   initial-text conversion and the cursor-at-end behaviour are transcribed from a header
   probe, not measured on a console.
5. **File-system semantics.** Whether newlib `mkdir("/switch/...")` works through libnx
   devoptab, what it returns for an existing directory, and how a write-protected SD card
   fails. `make_dirs` "ignore everything" cannot be validated by reasoning alone.
6. **Whether the game accepts what the tool writes.** The core save/arc round-trips are
   tested against real data (105 tests), but no console-side load test exists.
7. **Rendering and input feel**: the 22 px row pitch, the `y > h - 90` clip
   (editor.rs:332), the 12-frame auto-repeat (editor.rs:68, 562-573), and whether long
   Check/ArcEntries lines are clipped at the right edge.
8. **The `-Route c` link path** (`tools/rustmain.c` + staticlib) was not built here; only
   the default `cargo` route was.

---

## 8. Prioritised fixes

**P0 - data safety, before anyone runs this on a real save**

1. `switch/src/editor.rs:484`: pass `0` (or 248) instead of `60` for `.gmd`, and stop
   pre-truncating the initial text handed back to the caller. Better: edit long strings
   in a scrolling view; the keyboard alone cannot replace a 117-char description.
2. Flush before every `load_entry()`: call `store_entry()` in the A arm of
   `input_arc_entries` and at the top of `run_check` (or inside `load_entry`). Add the
   regression test used in C4.
3. Unify the cursors on Slots/Check/Palico - move the rendered cursor or render
   `self.slot`/`check_cursor`/`palico_cursor`. On Check this is a correctness bug.
4. Make the picker work in the no-gamedata build: enable `gamedata` for the `.nro`
   (measure the size cost; gamedata.rs is 308 KB of source) or add a compact id->name
   table behind a small feature. Until then remove `Screen::ItemPick` and its hint
   instead of shipping a dead screen.

**P1 - silent wrong behaviour**

5. Give `request_write_arc` an `else` branch that points at X-on-the-entry-screen (E5).
6. Delete the duplicate `Items` hint arm (E6) and make the survivor list R and the real
   meaning of Y.
7. Reset `LAST_EXTRACT_FAILED` at the top of `extract_quests` and report empty payloads
   separately (E7).
8. Either make `FieldKind::Bytes` fields editable as raw hex, or skip them in
   `move_cursor` so the cursor cannot land on a row that refuses input (E8). Ideally
   both - the monster slots are the main reason to edit a quest at all.
9. Guard the 3DS-format backup (E9): skip the auto-backup, or convert first.
10. Cut the character name at a UTF-8 boundary and cap it at 31 bytes before the keyboard
    (E10).

**P2 - parity and docs**

11. Port missing features in order of value: `.mib` hex grid -> entry viewers
    (`.sem`/`.rem`/`.sup` first) -> DLC uninstall -> item id entry -> `.saveslot` ->
    3DS conversion -> colours/slot-used -> list filters. Each is already a pure function
    in `mhnx-core`; only hbedit needs a screen.
12. Restore `check::clear_one_player_only` to the GUI checker panel (the console has it,
    the GUI lost it) and re-add the equipment box + name search, whose `EQUIP_*` tables
    are currently dead code.
13. Add a guard so the console feature set is tested: run
    `cargo test -p mhnx-core --no-default-features` (or a gamedata-off job) and fix
    README.md:254.
14. Fix the stale sizes and the folder/key tables in README.md and switch/README.md, and
    state the hbmenu application-mode requirement for text entry.

---

### Appendix - files read

`crates/mhnx-core/src/hbedit.rs` (1923 lines, all), `switch/src/{app,editor,lib,main,swkbd,sys}.rs`,
`switch/src/text.rs` (header + `Canvas::new`), `switch/{Cargo.toml,README.md,build.ps1,rust-toolchain.toml,.cargo/config.toml}`,
`crates/mhnx-gui/src/{app,main}.rs` (all), `crates/mhnx-core/src/{save,questfields,gamedata}.rs`
(the parts referenced), `crates/mhnx-core/tests/hbedit_flow.rs` (item-picker and Minus
tests), `README.md`, `docs/save_manager_review.md`, and the installed libnx headers under
`C:/devkitPro/libnx/include`.
