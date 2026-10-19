# Final review: "pick a quest out of the save, edit it, write it back"

Scope: the three implementations of the save-internal quest loop —
GUI (crates/mhnx-gui/src/app.rs, crates/mhnx-gui/src/main.rs),
console state machine (crates/mhnx-core/src/hbedit.rs) and the Switch
front end (switch/src/editor.rs) — plus the arc writer they all depend on
(crates/mhnx-core/src/arc.rs).

Earlier reviews were read first and are not repeated here:
docs/review_quest_injection.md, docs/review_save_layout.md,
docs/rereview_quest.md, docs/rereview_charm.md. Where a finding overlaps
with theirs it is marked as such.

Real data was opened read-only:
C:/Users/haruk/AppData/Roaming/Ryujinx/bis/user/save/0000000000000001/0/system
4,726,188 bytes, SHA-256 D06B72520E379F462D49098E55C270BA48B203C10A9DBF39A1F8EE7E410C112A,
LastWriteTime 2026-10-19 15:01:28 (unchanged after this review).
Every experiment below ran on in-memory copies; the one end-to-end GUI run
wrote only to a temporary file, which I then deleted. My probe tests were
temporary and have been removed from the tree.

---

## Verdict

**The storage layer is right; the seam between the editor and the storage is
wrong, and the two tests named for the two central claims cannot fail.**

* The no-op claim is true and independently measured: a quest taken out of the
  save and written back untouched produces a byte-identical save, both through
  the library and through the GUI (env hooks, whole-file SHA-256 match).
* The slot claim is true for the library: an edit lands only inside the
  target 0x1400-byte slot, and the installed bit set does not move (measured on
  slot 0 and on slot 100).
* But on the console path the edit never reaches the arc that is written back.
  `install_into_save` repacks `Editor::arc`, and the .mib / .gmd working
  copies only ever get into that arc inside `request_write_arc`
  (hbedit.rs:838-851). The documented flow "edit -> X" therefore writes the
  original bytes: I measured whole-save diff **0** while the editor reported
  "installed quest 1010005 into slot 0".
* The A key from the save's quest list does not select the .mib entry it just
  loaded. `open_quest_from_save` sets `entry = <mib index>` but
  `cursor = 0` (hbedit.rs:1046-1049), unlike `open_arc_bytes`
  (hbedit.rs:380). For the save's first quest the mib is entry 11; the
  highlighted row is entry 0, and pressing A loads entry 0 and clears the
  249-byte .mib buffer with "this entry type cannot be edited".
* "Repacking an arc is lossless" holds byte-for-byte for all 175 save copies
  and for 140 of the 141 shipped files; the one exception is
  q1010165.arc itself — the file the repo's own arc test names. That test is
  disabled by a mangled string literal and would fail if it ran.
* The Switch front end compiles and links against the real devkitPro (I ran the
  release build here) but has never been run on hardware, and it writes
  extracted quests into a folder its own scanner does not scan.

---

## CONFIRMED (my own measurements)

### Claim 1 - an untouched write-back is a no-op. CONFIRMED, twice.

| what | result |
|---|---|
| `Save::from_bytes` -> `to_bytes` on the real file | 4,726,188 -> 4,726,188 B, **0 differing bytes** |
| GUI: MHNXEDITOR_OPEN_SAVE + MHNXEDITOR_OPEN_QUEST=1010005 + MHNXEDITOR_WRITE_BACK + MHNXEDITOR_SAVE_OUT | output 4,726,188 B, **diff 0**, SHA-256 identical to the real save |
| `dlc::install_quest(id, existing payload)` on a copy | no byte changes; 175 ids, 175 bits before and after |

The GUI run used the rebuilt binary (target/debug/mhnxeditor.exe, built by me
from the current sources) and wrote to a temp path, never to the real save.

### Claim 2 - an edit changes only its slot. CONFIRMED for the library.

* Slot 0, quest 1010005, one byte flipped in the payload: **1 byte changed**,
  at file offset **63,712** = 0x24 + 0xB280 + 0x45F4 + 8 + 0x40 (slot start
  63,640 + the flipped payload byte). 0 bytes outside.
* Slot 100, quest 1010145, one byte flipped: **1 byte changed**, 0 outside the
  window **575,640..580,760** (= 0x24 + 0xB280 + 0x45F4 + 100*0x1400, length
  0x1400).
* The installed bit set was byte-identical before and after both writes
  (`ff ff ff fd ff 01 00 00 ...`).
* All 175 records have unique ids; 40 are in the equipment catalogue and all 40
  of their catalogue bits are set (`installed_flag` returns Some(true) for
  every one).

This is stronger than the repo's own test, which uses slot 0 (see V5).

### Claim 3 - repacking an arc is lossless. CONFIRMED for the reviewed data,
with one named exception.

| folder | files | entries | byte-identical round trips |
|---|---|---|---|
| dist/quests_all (extracted from the save) | 175 | 2,359 | **175 / 175** (403,576 B total) |
| save_editor/mhxx_quetool/quest (the "141 shipped") | 141 | 1,950 | **140 / 141** |

The single failure is **q1010165.arc, 2,233 B -> 2,217 B, first difference at
offset 88, 994 differing bytes**. Cause, measured: its first payload offset is
**1,232**, while `to_bytes` puts the data area at
`align32(12 + 80*15) = 1,216`; the other 140 files use align32 and the 175
save copies are all contiguous from align32. The 16-byte difference shifts
every entry offset and the whole payload area. The 15 payloads themselves are
identical after repacking (`Arc::parse(out)` yields the same names, hashes and
data), so this is a layout change, not data loss — but it is not "lossless" in
the byte sense the claim uses.

The same file doubles as the test fixture for `arc.rs`'s lossless test, and
that test cannot open it (see E5).

### Claim 4 - ArcEntry equality ignores the original stream. CONFIRMED, and no
caller depends on the old derived equality.

`impl PartialEq for ArcEntry` (arc.rs:81-85) compares name, type_hash and data;
`Eq` is kept. A grep of crates/ and switch/ finds no comparison of `Arc` or
`ArcEntry` values outside tests, and no test compares them either. The field is
a cache used only by `to_bytes` (arc.rs:191-194) and filled by `parse`
(arc.rs:157), so ignoring it cannot make a production check pass silently.
My probe confirmed both directions: a clone with `original = None` compares
equal, and an entry with a bogus `original` compares equal to the same entry
without one. There is no test in the repo that would notice this (there is no
test that compares arcs at all), but there is also no code that relies on it.

### Other facts confirmed on the way

* Extraction from the console side works: on the SaveQuests screen Down x3 then
  Minus emits `ExtractQuests { ids: [1010171] }` (the highlighted row), R
  emits an empty id list, and the row decodes the gmd title
  ("ゼルダの伝説・力の試練").
* A corrupt record is handled without panicking: with `size = 0x2000` the
  record appears with empty data and `open_quest_from_save` reports
  "quest 1010005 has no payload (size 8192)" and leaves the screen alone. A
  `size = 0` record gives the same message with size 0 (wording, not a crash).
* All 175 payloads carry a .mib whose id equals the slot record id (0
  mismatches), and the longest arc entry name is 37 bytes (the 63-byte name
  truncation in `to_bytes` is never exercised by this data).
* The Switch crate builds: in switch/, with DEVKITPRO=C:/devkitPro and
  C:/devkitPro/devkitA64/bin on PATH, `cargo build --release` finishes with
  exit 0 (I ran it; it recompiled mhnx-core and mhnx-switch and linked). A
  plain `cargo build` in this shell fails before that because the ambient
  DEVKITPRO is /opt/devkitpro (a Linux path) and .cargo/config.toml uses
  force = false, so the ambient value wins; that is an environment quirk, not
  a source problem.
* libnx' startup really does mount the SD card for stdio: disassembling
  `init.o` from C:/devkitPro/libnx/lib/libnx.a shows `fsInitialize` followed by
  `fsdevMountSdmc`.

---

## ERRORS

### E1 (blocking) hbedit.rs:1049 - the quest list's "A edit" does not select the .mib

`open_quest_from_save` (hbedit.rs:1023-1054) chooses the entry to show
(`idx` = the .mib index) and stores it in `self.entry`, but then sets
`self.cursor = 0` (line 1049). `open_arc_bytes` sets both to `idx`
(hbedit.rs:379-380). The render (hbedit.rs:1455-1466) and the A handler
(hbedit.rs:1096-1099) use `self.cursor`, so the highlighted row and the loaded
entry disagree.

Measured (quest 1010005, slot 0):
```
after open_quest_from_save: screen=ArcEntries entry=11 cursor=0 mib_buf=249
press A:                    screen=ArcEntries entry=0  cursor=0 mib_buf=0
                            message="this entry type cannot be edited"
```
The mib index varies by quest (over the first 30 quests: 11 x1, 12 x6, 13 x18,
14 x4, 15 x1), so this hits every quest, not a special case. The README recipe
(README.md:354-363) says "A でクエストエディタを開く"; on the console that A
press does not open the field editor and throws away the loaded buffer.

### E2 (blocking) hbedit.rs:1231-1263 - edits are not in the arc that is installed

`install_into_save` reads the quest id and the payload from `self.arc`
(lines 1236-1251). Edits are written only into `self.mib_bytes`
(input_mib:1189) and `self.gmd`; those reach `self.arc` only through
`store_entry` (hbedit.rs:410-418), whose only caller is
`request_write_arc` (hbedit.rs:839). `input_arc_entries`'s X handler
(hbedit.rs:1116) calls `install_into_save` directly, without `store_entry`.

Measured with the real save in memory:
```
SaveQuests -> A (open 1010005) -> cursor=entry (11) -> A (Mib)
-> Down Down -> Right (version 1 -> 2)
-> B (back to ArcEntries) -> X (install)
   message = "installed quest 1010005 into slot 0"
   installed payload == original payload: true
   whole-save byte diff: 0
```
The edit is lost with a success message. The only route that keeps it is to
press A inside the .mib screen first (hint: "A write .arc"), which calls
`request_write_arc`: that does `store_entry()` but, because
`arc_path == None` for a save-sourced arc, writes nothing and changes no
message (hbedit.rs:838-851). Measured with that step added:
```
A in the mib screen: arc now has the edit, take_write() = None, message unchanged
-> B -> X: installed mib == edited buffer: true, whole-save diff: 110 bytes
```
So the feature works only through an undocumented key that announces a file
write it does not perform. The README's step "X でセーブへ書き戻す" taken from
the .mib screen is worse than useless: on that screen X cycles the adjustment
step (hbedit.rs:1157).

### E3 (high, GUI) app.rs:1146-1149 / 386-406 / 354-359 - switching entries discards the other working copy

`select_arc_entry` clears `self.mib` and `self.mib_bytes` and then loads only
the newly selected entry (app.rs:386-406). The entry-list click handler calls
it without storing the previous entry first (app.rs:1146-1149). The write-back
button stores only the currently selected entry (app.rs:359). Consequence, by
code inspection (this is a GUI event flow; I could not drive it headlessly):
edit the .mib field, click the .gmd row to edit the title, press
"セーブに書き戻す" — the .mib edit was dropped from memory and the write reports
success with only the text change. The README explicitly recommends editing
"フィールドやテキスト" in one sitting (README.md:346). The per-entry 反映
buttons (app.rs:1364, 1390) are the only safe workaround.

I verified the successful half by hand instead: the no-op GUI write-back above
went through `store_arc_entry` -> `to_bytes` -> `install_quest` and produced a
byte-identical save, so the plumbing itself is right.

### E4 (high) the payload id is never checked against the record id

`install_quest` (dlc.rs:236-279) takes `quest_id` and `arc_bytes` and never
looks inside the payload. The GUI passes `arc_from_save` (app.rs:355, 372),
not the id inside the edited .mib, while the .mib editor exposes `quest_id`
at offset 0x0C as an editable field (questfields.rs:58, app.rs:1259-1305).
So: open a quest from the save, change its ID field, press write-back — the
record keeps id A while the payload says B, and the tool prints success.
Executed at library level:
```
install B's payload under A's id                    -> Ok(0)
   slot id = 1010005, payload mib id = 1010168
install a payload whose mib id was edited to 9990001 -> Ok(0)
```
On this save all 175 payload ids match their records, so this only bites when
the user edits the id (or the save is already inconsistent). The hbedit path
does not hit it because it reads the id from the payload — which is exactly why
it is invisible to the tests.

### E5 (high) arc.rs:254-257 - the lossless-arc test is disabled by a mangled path, and would fail

The literal is not the intended path. Hex dump of the source bytes:
```
72 22 43 3A 61 6E 79 57 6F 72 6B 73 70 61 63 65 0A 78 6D 68 78 78 ...
r  "  C  :  a  n  y  W  o  r  k  s  p  a  c  e \n  x  m  h  x  x ...
```
i.e. `r"C:anyWorkspace<LF>xmhxxsave_editormhxx_quetoolquestq1010165.arc"`
— the backslashes of the Windows path were consumed as escapes when the file
was written (\a lost, \n became a real newline). `src.exists()` is false on
any machine, so `reparsing_and_repacking_is_byte_identical` always returns at
arc.rs:258-260. This is the only test in the repo that names the repack claim.

It would also fail: with the correct path I measured **2,233 -> 2,217 bytes**,
first difference at 88 (see claim 3). The offsets quoted in the comment above
the test (1216, 1237, 1280, 1301) belong to the *save's* copy in
dist/quests_all, not to the file the test names — I checked both.

### E6 (high, Switch) the extraction folder is not scanned

`OUT_QUEST_DIR = "/switch/mhnxeditor/mhxx/quest/arc"` (switch/editor.rs:63),
but `QUEST_DIRS` (switch/editor.rs:51-56) lists
/switch/mhnxeditor/quest/arc, /switch/mhnxeditor/quest,
/switch/mhnxeditor/quests, /mhnxeditor/quest — not the mhxx folder. After an
extraction, service() calls `ed.set_arc_files(scan_quests())`
(switch/editor.rs:427) with the comment "The folder is new, so refresh the quest
list for the next scan"; the new files are absent from that list, and the
global rescan (Minus, editor.rs:531-533) will not find them either.
README.md:322 states the opposite ("書き出したフォルダは次回のスキャンで
クエスト一覧にも出ます").

### E7 (high, docs) README.md:362 tells the user to press a key that discards the install

"-> セーブ画面で A でファイル保存" (save the file with A on the save screen).
There is no such key. On `Screen::SaveList` A is `Request::ReadSave`
(hbedit.rs:857-861), which re-reads the file and **throws away the in-memory
install**; on `Screen::Slots` A opens the character editor. The save write
keys are Char A (hbedit.rs:934), Items A (972), Charms X (610), Palico X (722)
and Options A (752). From the quest side the only route is
B -> ArcList -> Y -> SaveList -> X (Options) -> A — which the UI never explains,
and which the README recipe sends the user straight past. A user who follows
the README loses the edited quest silently (the save on disk is then the
untouched one).

### E8 (medium, Switch) make_dirs ignores failures and extract_quests reports the wrong cause

`make_dir` returns `mkdir(...) == 0` (switch/editor.rs:120-123); `make_dirs`
calls it for every component and then **always returns true**
(switch/editor.rs:128-141, "Ignore the return value ... The caller checks the
final state by writing"). `extract_quests` ignores that return value entirely
(editor.rs:153) and skips `write_file` failures, returning only a count
(editor.rs:147-168). `apply_extracted` then prints either "wrote N file(s)" or
"nothing written to DIR - no matching quest in this save"
(hbedit.rs:357-363). So a read-only / full SD card, or a path that cannot be
created, is reported to the user as "no matching quest in this save". The same
message overwrites the specific "open a save first" that service() just set
(switch/editor.rs:418-425). Partial failures (some of 175 written) are not
counted at all.

(The mkdir prototype itself is fine: devkitA64's mode_t is __uint32_t, so
`fn mkdir(path, mode: u32)` matches the ABI.)

### E9 (medium) no write-save key on the screens where the install happens

After `install_into_save` (hbedit.rs:1256-1260) the editor is on ArcEntries or
Mib; neither screen has a save-write key (hbedit.rs:1091-1200, 1221-1227), and
the hint line (hbedit.rs:1471, editor.rs:345) does not mention one. The user
must discover B -> ArcList -> Y -> SaveList -> X -> Options -> A. Combined with
E7 this is the most likely way to lose an install. The message after install
("installed quest 1010005 into slot 0") does not say the save is still
unsaved, and nothing marks the editor as dirty on screen.

### E10 (medium, GUI) open_save does not clear the save-quest binding

`open_save` (app.rs:132-145) replaces `self.save` but leaves
`self.arc_from_save` and `self.arc` alone. Open save A, open a quest from it,
then open save B: the Quest tab still says "セーブ内のクエスト X を編集中" and
the write-back writes A's quest into B (if B happens to hold that id).
`open_arc` clears the binding (app.rs:288); `open_save` does not.

### E11 (low) the GUI decodes all 175 titles on every frame

app.rs:1483-1494 runs `Arc::parse` + `Gmd::from_bytes` for every installed
quest each frame. Measured on this machine (175 quests): 6.2 ms per frame in
release, 46 ms per frame in the debug build, versus 0.1 ms for
`list_quests` itself. The comment ("Cheap enough at 175 records") is not true
in debug and eats a third of a 60 Hz budget in release.

### E12 (low) dlc.rs:266 - the refusal message contains runs of ten spaces

The string has "can be          updated" and "free slot          number" where
line breaks were flattened. Valid Rust, ugly in the CLI and GUI.

### E13 (low, latent) SaveQuests cursor after the list shrinks

`input_save_quests` never clamps `save_quest_cursor` against the list length;
entering the screen clamps once (hbedit.rs:999). If the list shrinks while the
screen is open (I simulated it with `dlc::uninstall_quest` on the editor's
save), the render highlights **no** row, A does nothing and keeps the old
"175 quest(s)" message, and Down jumps to index 1. No panic. This is not
reachable through hbedit's own UI (it has no uninstall), so it is latent — but
the state machine offers no defence.

### E14 (low) hbedit.rs:756 test indexes the quest list by slot number

`ed.save_quests()[target.slot]` is only correct for slot 0. The save has gaps
(slots 135..159 and 200..204 are empty), so for any later quest the test would
compare a different record. It passes today only because the test picks the
first quest.

---

## Claim 5 - the Switch sub-questions, one by one

1. **Save absent.** `extract_quests` takes `&Save` so it cannot run without one;
   service() sets "open a save first" and then returns 0, and
   `apply_extracted` immediately overwrites that message with "nothing written
   to DIR - no matching quest in this save" (switch/editor.rs:418-425,
   hbedit.rs:357-363). No crash, wrong explanation.
2. **Folder cannot be created.** `make_dirs` ignores every `mkdir` result and
   returns true unconditionally (switch/editor.rs:128-141); `extract_quests`
   ignores the return value (line 153). Every `write_file` then fails, the
   count is 0, and the user is told there is no matching quest (E8). No panic,
   but also no way to tell "read-only card" from "quest not in the save", and
   a partial write (some of 175) is reported as a plain success count.
3. **Empty payload.** Skipped silently (switch/editor.rs:159-161) and not
   counted; if every id was empty the same misleading message appears. No
   crash. On the save side the editor refuses it first with
   "quest X has no payload (size N)" (hbedit.rs:1028-1031), which I exercised
   with a size = 0x2000 record and with a size = 0 record.
4. **Is make_dirs correct?** It builds each component and creates it; the
   return value is a constant `true`, so "correctness" is not observable from
   its result. The mode 0o777 is irrelevant on FAT. The `mkdir` prototype
   matches devkitA64 (`mode_t` is `__uint32_t`, read from
   C:/devkitPro/devkitA64/aarch64-none-elf/include/sys/_types.h:90). Not
   tested on a real SD card.
5. **Is OUT_QUEST_DIR reachable under stdio?** In principle yes: the path has
   no device prefix, newlib sends such a path to the first mounted devoptab,
   and libnx' init.o calls `fsInitialize` then `fsdevMountSdmc` (verified by
   disassembly). I could not verify the prefix-less resolution without
   hardware, and the same assumption already underpins saving (`SAVE_DIRS` +
   `fopen` in the pre-existing code). The *real* defect is not reachability
   but that scan_quests never looks at this folder (E6).
6. **Does key_from()'s Minus break the global rescan?** No. The rescan is gated
   by `!ed.uses_minus()` (switch/editor.rs:531) and `uses_minus()` is true only
   on ArcEntries/SaveQuests (hbedit.rs:352-354), so it still fires on every
   other screen; on those two, hbedit consumes Minus and the rescan is meant to
   be off. On the other screens the extra `Key::Minus` is ignored by every
   input handler. Two side effects are real: (a) holding Minus on SaveQuests
   auto-repeats (editor.rs:548-553 -> hbedit.rs:1074), so the same quest file is
   rewritten and the folders rescanned every few frames while the button is
   held; (b) the rescan is now unavailable exactly on the entry screen, which
   is where the new output folder (E6) would have to be discovered.

---

## UNSUPPORTED claims

1. **"Edit it in the quest editor, write it back" (console)** — not supported
   by the code. E1 and E2: the A that the list advertises does not open the
   field editor, and the X that installs does not see the edits. The GUI
   version of the loop works for fields and text separately but loses one when
   the user switches entries (E3).
2. **"Repacking an arc is lossless"** — only for the save's own 175 copies and
   for 140 of the 141 shipped files. q1010165.arc (2,233 B) is a counterexample
   (E5/claim 3), and the test that would have caught it never runs.
3. **"The extracted folder appears in the quest list" (README.md:322)** —
   false, E6.
4. **"Save with A on the save screen" (README.md:362)** — false and
   destructive (E7).
5. **"The folder is created automatically" (README.md:319)** — `make_dirs`
   cannot report failure and `extract_quests` ignores it (E8). The behaviour
   may still be fine on FAT; the code cannot tell you.
6. **Claim 6's "the list uses Minus to extract" test** (see V6) — the behaviour
   is real (I checked it), but the test only asserts a predicate.
7. **A key consistent state** — no: `entry` and `cursor` disagree after the A
   that opens a quest, and the next A discards the .mib buffer (E1). The save
   object and the arc are otherwise consistent; `dirty` is not touched by
   opening a quest.

## Not verified (stated, not assumed)

* No run on Switch hardware exists anywhere in the tree:
  docs/switch_build.md:461-462 ("実機での起動は未検証") and
  switch/README.md:5 ("実機は不要"). So the device-less stdio path
  (`/switch/...` without `sdmc:`), `mkdir` on the real SD card, and the
  on-device rendering of the new screens are unverified. I did verify that
  libnx' init calls `fsdevMountSdmc`, so a device is mounted at startup; I
  could not verify newlib resolves a prefix-less path to it without hardware.
* The GUI entry-switch data loss (E3) is code inspection, not an executed UI
  flow; the rest of the GUI path I executed through the env hooks.
* What the game does with a payload whose .mib id differs from the record id
  (E4) is not knowable from here; the tool accepting it is a fact.
* The event half of the installed-bit catalogue is still unknown (pre-existing;
  see rereview_quest.md R1).

---

## Vacuous / non-discriminating tests

| test | why it cannot fail for what it names |
|---|---|
| **V1** arc.rs:252-282 `reparsing_and_repacking_is_byte_identical` | the path literal is mangled (E5); the test returns at line 258 on every machine. With the correct path it **would fail** (2,233 -> 2,217). This is the only test named for claim 3. |
| **V2** hbedit_flow.rs:727-758 `editing_a_quest_from_the_save_writes_back_to_the_same_slot` | it **never edits anything** — it opens quest 0 and presses X. It really tests "an untouched install is a no-op" (claim 1, through the repack). It passes with E2 present, and would pass if `install_into_save` wrote the original bytes forever. The claim in its name is untested. |
| **V3** hbedit_flow.rs:343-352 `quest_flow_installs_a_download_quest_into_the_save` (second half) | after a second X on ArcEntries, `take_write()` is always `None` (X installs, it does not write a file), and the `_ => {}` arm swallows it. The "writing the save must keep the quest" half asserts nothing. |
| **V4** regression_review.rs:162-174 `writing_an_untouched_quest_back_is_a_no_op` | both sides are `to_bytes()` of the same object and the payload is the stored one; it cannot tell "wrote the same bytes" from "wrote nothing", and never compares against the on-disk file. It does catch an install that would zero the slot. |
| **V5** regression_review.rs:177-205 `editing_a_quest_touches_only_its_own_slot` | uses `list_quests()[0]`, i.e. **slot 0** — the slot a broken "write the first record" implementation would also choose, so it cannot show id targeting. It checks the diff window and the other quests, but never that the intended payload landed in the slot (the payload it passes in is re-derived from the same save). My slot-100 run (claim 2) is the discriminating version. |
| **V6** hbedit_flow.rs:772-783 `minus_is_also_swallowed_on_the_save_quest_list` | only asserts `uses_minus()`; it never presses Minus, never checks that the request carries the selected id, and never checks the rescan stays off. |
| **V7** hbedit_flow.rs:760-770 `a_quest_id_that_is_not_in_the_save_is_refused` | passes on a save with **zero** quests: the message is the same "not in this save". It cannot distinguish "absent id" from "no quest data at all". |
| **V8** hbedit_flow.rs:355-376 `cursor_navigation_wraps_on_every_screen` | despite the name, it exercises only Slots and Char; SaveQuests is not touched. |
| **V9** hbedit_flow.rs:574-594 `every_screen_renders_without_panicking` | its fixed key sequence never reaches SaveQuests (no Y then X). |
| **V10** all new hbedit_flow tests | they start with `if !real_save().exists() { return; }` and report nothing; on a clean machine the whole save-quest suite is green having run nothing (regression_review.rs at least panics unless MHNX_SKIP_REAL_SAVE is set). This is the same policy gap rereview_quest.md R6 flagged. |

What *is* really covered: `x_on_the_quest_list_opens_the_saves_own_quests` and
`opening_a_quest_from_the_save_fills_the_editor` can fail and do anchor the
list and the .mib buffer (id and length); the GUI no-op path is covered only by
my manual run; the library's no-op and slot window are covered (see above).

---

## Prioritised fixes

1. **(blocking) Put the working copy into the arc before installing.**
   `install_into_save` (hbedit.rs:1231) must call `store_entry()` (or read
   `mib_bytes` / `gmd` itself), the same way `write_arc_to_save` does on the
   GUI. Otherwise "edit, X" writes the old bytes with a success message (E2).
2. **(blocking) Fix the entry selection after opening from the list.**
   hbedit.rs:1049 `self.cursor = 0` -> `self.cursor = idx`, and add a test
   that presses A twice from SaveQuests and asserts the Mib screen opens with
   the quest's own .mib (E1).
3. **(high) Add a test that edits and writes back through the console flow.**
   Open from SaveQuests, change a .mib field, B, X, then parse the slot payload
   and assert the edited value; this is the test
   `editing_a_quest_from_the_save_writes_back_to_the_same_slot` pretends to be.
4. **(high) Add a save-write key on the quest screens** (or a "unsaved" prompt
   when leaving them) and correct README.md:362 to the actual route; the
   current instruction re-reads the file and drops the install (E7/E9).
5. **(high) Validate the payload id in `install_quest`**: parse the .mib and
   refuse when its id differs from `quest_id`, or make the GUI pass the payload
   id and verify that it equals `arc_from_save`. Closes E4 and the E3
   entry-switch variant (a write-back of a non-mib entry would then be
   refused).
6. **(high) Fix the arc test path and add the 175-file sweep** to the repo
   tests. Decide what "lossless" means for a file whose data area is not at
   align32 (q1010165.arc): preserve the original start offset or document that
   the layout is normalised (E5, claim 3).
7. **(high) Make the extraction folder visible**: add OUT_QUEST_DIR to
   QUEST_DIRS or write into a scanned folder, and correct README.md:322 (E6).
8. **(medium) Report extraction failures**: return an error/status from
   `make_dirs` + `extract_quests` and give `apply_extracted` a reason
   ("cannot create the folder", "write failed"), so a read-only card is not
   reported as "no matching quest" (E8).
9. **(medium) GUI: store the previous entry in `select_arc_entry`** (or make
   the write-back store every touched entry) so editing .mib and .gmd in one
   sitting cannot drop half the edit (E3). Also clear `arc_from_save` in
   `open_save` (E10).
10. **(medium) Make the tests able to fail**: fix V1, V2, V3, V5, V6 above;
    replace the shared "skip if no save" returns in hbedit_flow.rs with the
    env-guarded policy already used in regression_review.rs.
11. **(low) Cache the DLC tab titles** per save/level change instead of
    decoding 175 arcs per frame (E11); fix the dlc.rs:266 message (E12); clamp
    `save_quest_cursor` on every input (E13); fix the slot/list indexing in
    hbedit_flow.rs:756 (E14); update docs/quest_injection_spec.md 264-272
    (five test names there no longer exist).

---

## Reproduction

All probes were temporary integration tests under crates/mhnx-core/tests
(deleted after the run); they only ever mutated in-memory copies.

* No-op: `Save::from_bytes(read(save)).to_bytes()` == the file; GUI:
  `MHNXEDITOR_OPEN_SAVE=<save> MHNXEDITOR_OPEN_QUEST=1010005
  MHNXEDITOR_WRITE_BACK=1 MHNXEDITOR_SAVE_OUT=<tmp>` then compare.
* Dropped edit (E2): drive `Editor` with Y, X, A, (cursor=entry) A, Down,
  Right, B, X and compare `list_quests(save)[0].data` with the original.
* Slot targeting: `dlc::install_quest` with one payload byte flipped for the
  quest in slot 100; diff the serialised saves.
* Arc sweep: for each .arc, `Arc::parse(f).to_bytes() == f`; 175/175 for
  dist/quests_all, 140/141 for save_editor/mhxx_quetool/quest
  (q1010165.arc fails: 2,233 -> 2,217, first diff 88).
* The real save was never written: its SHA-256 and LastWriteTime are unchanged
  from the start of this review.
