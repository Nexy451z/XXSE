# mhnxeditor GUI 動作監査

- 監査日: 2026-10-20
- 対象: `dist\mhnxeditor.exe` (6,095,360 B / 2026-10-20 20:07:05 / SHA-256 `68EA028F73B585D5D9DC2FFDE19E99B088F2945C81599313DDFDE47DA0D4BE92`)
- ソース: `crates/mhnx-gui/src/app.rs` (137,003 B, 19:39:23) / `crates/mhnx-core/src/` (exe より古い = ビルドは最新ソース)
- セーブ: `C:\anyWorkspace\nx\jksv_ready\system` の作業コピーのみ使用 (`.work\gui_audit\work2\`、SHA-256 `D06B7252…C112A`)。実セーブ (Ryujinx) には一切触れていない。
- 画面: 1536x864 / DPI 125% (egui のポイント数は 1600x1025 相当のフレームバッファで撮影)

---

## 0. 判定サマリ

| # | 項目 | 判定 | ひとこと |
|---|------|------|----------|
| 1 | アイテムの追加 (空きスロットへ新規) | **できる（分かりにくい）** | 空きは既定で非表示。「空きスロットも表示」を入れて末尾までスクロールし、詳細パネルの名前検索で選ぶ。専用の「追加」ボタンは無い |
| 1b | アイテムの種類変更 | **できる** | 詳細パネルの ID 直接入力 or 名前検索。表の行クリックで選択できる |
| 1c | アイテムの個数変更 | **できる** | 表の個数セル / 詳細パネルの DragValue のどちらでも即時反映 |
| 2 | お守りの編集 (新規/削除/スキル変更) | **できる（新規と削除が分かりにくい）** | 新規は「枠番号を手入力」して作る。空き番号は一覧に出ない。削除は確認なしの 1 クリック。スキル変更は可能だがポイント欄が既定幅で画面外 |
| 3 | クエストの追加 (DLC タブ) | **できる（条件付き・実機未検証）** | .arc を開いていないとボタンは無効。カタログ外 ID は拒否。空き枠 135 に追加され、導入ビットも立つ。ただし直前に .mib を編集していても未反映で追加される |
| 4 | クエストの編集 (.mib / .gmd) | **できる** | 反映ボタン、または「セーブに書き戻す」で .arc に flush される |
| 5 | オトモの変更 (名前/レベル/なつみ) | **できる（ラベル要確認）** | 即時反映 + 保存で永続。ただし「なつき」の実体はコード上 `enthusiasm` (0x26, テンション) |
| 6 | ギルドカードの変更 (ランク/時間/コメント) | **できる（一部・分かりにくい）** | 3 項目とも可。キャラ本体の HR / プレイ時間とは別フィールドで同期しない。闘技タイムとカード ID は読み取り専用 |
| 7 | 狩猟記録の変更 | **できるが、既定では保存されない（危険）** | 「変更を反映」を押さないと `上書き保存` に入らない。スロット切替でも消える |
| 8 | マイセットの変更 | **名前のみ（中身はできない）** | アイテムセット / 装備セットとも名前だけ編集可。構成は読み取り専用 |
| 9 | 保存が実際にファイルへ反映されるか | **できる** | `上書き保存` で .bak を作って書き込み。無変更保存はバイト単位で一致、編集内容も再読込で一致 |

**総合**: 9 項目のうち 8 項目は「データとしては保存できる」。ただし **P0 が 2 件**（狩猟記録が保存に載らない / .arc 追加時に .mib 編集が落ちる）と、**レイアウト起因で触れない UI**（サイドバーが画面の 55% を占め、アイテムの「元に戻す」・お守りのポイント欄・DLC の追加ボタンが画面外）がある。

---

## 1. 方法

UI を操作できないため、次の 3 つを突き合わせた。

1. **実 exe の起動 + 環境変数フック**（`main.rs:66-145`）
   `MHNXEDITOR_OPEN_SAVE / MHNXEDITOR_OPEN_TAB / MHNXEDITOR_SCREENSHOT`、および
   `MHNXEDITOR_ITEM_SELECT / MHNXEDITOR_ITEM_FILTER / MHNXEDITOR_CHARM_SELECT / MHNXEDITOR_OPEN_ARC / MHNXEDITOR_OPEN_QUEST / MHNXEDITOR_WRITE_BACK / MHNXEDITOR_SAVE_OUT`。
   3 フレーム描画 → PNG 保存 → 自動終了 (`app.rs:731-758`)。全 12 タブでパニックなく PNG が生成された。
2. **スクリーンショット** 21 枚（`.work\gui_audit\shots2\`、一覧は §6）。
3. **コア API + GUI メソッドの直接呼び出しテスト**（20 本、全合格）
   - `crates/mhnx-core/tests/audit_flows.rs` … 10 本（各機能の書込→再パース）
   - `crates/mhnx-gui/tests/gui_flow.rs` … 10 本（`#[path = "../src/app.rs"]` で GUI 本体を組み込み、`open_save / apply_to_save / write_current / add_open_arc_to_save / open_arc_from_save / store_arc_entry / write_arc_to_save` を実 exe と同じコードで実行）
   - テストソースは `.work\gui_audit\tests_src\` に退避（§5 の手順で戻して再実行できる）

   **限界**: egui のウィジェットのクリック自体は自動化していない。「クリックで呼ばれる関数」を直接叩き、スクリーンショットで導線を確認する形にしている。


---

## 2. 各項目の根拠

### 2.1 アイテムの追加 / 種類 / 個数 — できる（追加は分かりにくい）

**個数変更（できる）**
- 表の個数セルは `DragValue`（`app.rs:1220-1231`）。`changed()` のたびに `apply_to_save()` を呼ぶので、保存を押す前からセーブ（メモリ）に載る。
- 詳細パネル側も同じ（`app.rs:1282-1289`）。

**種類変更（できる）**
- 詳細パネルの ID DragValue（`app.rs:1274-1281`）と、名前検索の一覧（`app.rs:1298-1360`）。検索結果は最大 40 件・高さ 160px 固定（`app.rs:1326, 1339-1342`）。

**追加（できるが導線が悪い）**
- 空きスロットは既定で一覧に出ない: `item_show_empty: false`（`app.rs:131`）＋ フィルタで `id == 0` を除外（`app.rs:1163-1175`）。
- 追加するには ①「空きスロットも表示」を ON（`app.rs:1111`）② 末尾（箱 #2243 以降）までスクロール ③ 行クリック ④ 名前検索して候補クリック、が必要。専用の「追加」ボタンは無い。
- クリック時の処理は「id を入れ、個数 0 なら 1 にする」（`app.rs:1349-1357`）。妥当。

**証跡**
- `x1_items_picker.png`: 箱 #2243（空き）を選択した状態。`ID 0 / 個数 0`、検索語「回復薬」で 9/10 の 2 件が出ている。表側は「2242 件」で空き行は出ていない。
- `z1_reopen_item.png`: テストで書き込んだ `item_test.bin` を実 exe で開いた図。**2243 件**に増え、`選択中: 箱 #2243 ID 46 個数 3`、1 行目が「調合書②」に変わっている（元は①）。
- テスト `item_add_change_and_persist` / `gui_item_add_and_write_current_persists` が全合格。

### 2.2 お守り — できる（新規・削除が分かりにくい／危険）

**スキル変更（できる）**
- 左リストのクリック（`app.rs:2735-2745`）でフィールドに読み込み、スキルは検索付き ComboBox（`skill_combo, app.rs:2631-2683`）、ポイントは符号付き DragValue（`app.rs:2771-2792`）。
- レア度ごとの許容範囲の注意表示もある（`app.rs:2798-2846`）。実測: 枠 224 で「種別 0x0A / レア度 100 / 属性会心 +2〜+6 / 達人 -7〜+10」。

**新規（分かりにくい）**
- 「空き枠に新規作成」（`app.rs:2869-2889`）は `self.charm_index`（＝左の「枠」DragValue、`app.rs:2759`）の番号にそのまま作る。
- 空き番号は一覧に出ない（一覧は `charms()` の結果のみ）。**`charm::first_free` がコアにあるのに GUI からは使っていない**（`charm.rs:392-397`）。空きでない番号を指定すると「その枠は空きではないか、保護されています」で失敗するまで分からない。
- 実測の空きは装備ボックス 23 番（`first_free = Some(23)`）。

**削除（危険）**
- 一覧の「削除」（`app.rs:2746-2748`）→ 即 `clear_charm`（`app.rs:2912-2919`）。確認ダイアログも Undo も無い。1 クリックで 1 個消える。

**レイアウト**
- `x2_charm_selected.png` の通り、既定幅では **スキル 1/2 のコンボの右側にあるポイント DragValue と装飾スロットの 4 個目の●が画面外 (x>1600)**。ポイントを変えるにはウィンドウを広げるかサイドバーを狭める必要がある（§3-A）。

**証跡 / テスト**
- `z2_reopen_charm.png`: テスト出力 `charm_test.bin` を実 exe で開いた図。先頭に **23 [2 slots] 属性会心 +6 / 達人 +10** が現れ、224 が +5 に変更され、225 が消えている（188 個のまま = +1 −1）。
- テスト `charm_create_edit_delete_and_persist` では、武器レコード (index 0) が 36 バイト完全一致、新規お守りの +0x02/+0x12（レア度ペア）がテンプレート由来であることも確認。

### 2.3 クエストの追加 — できる（条件付き・実機未検証）

- ボタンは DLC タブ（`app.rs:2490-2497`）で、`self.arc.is_some()` のときだけ有効。先にトップバーの「.arc を開く」（`app.rs:806-814`）で開く必要がある。
- 実体は `add_open_arc_to_save`（`app.rs:448-480`）→ `dlc::add_quest`（`dlc.rs:540-593`）。
  - .mib から ID を読み（`hbedit::quest_id_of_arc`）、**DLC_CATALOG にある ID** でなければ拒否（`dlc.rs:547-552`）。
  - 空きスロットは size==0 の最初の枠（`dlc.rs:565-578`）。
  - 導入ビットを立てる（`dlc.rs:591`）。
- 実測: 175 件 → **176 件**、クエスト 1020020 が **スロット 135**（空き枠の先頭）に入り、再読込後も payload がバイト一致、ビット数も +1。
- GUI レベルでも `gui_open_arc_then_add_persists` が合格（1010116 をスロット 135 に追加、書き出し→再読込で一致）。
- **注意 1**: 追加直前に .mib を編集していても、その編集は追加に載らない（§3-C、P0）。
- **注意 2**: 実機で一覧に出るかは未検証（`dlc.rs:530-539` のコメントも「experiment」）。作業前に .bak を取るべき。
- **注意 3**: カタログ外 ID の .arc は `Error::OutOfRange` の英語メッセージがそのまま出る（§3-E）。

### 2.4 クエストの編集 (.mib / .gmd) — できる

- .mib: フィールド表（`app.rs:1499-1618`）と 16 進グリッド（`app.rs:1632-1690`）を編集し、**「.mib の変更を .arc に反映」**（`app.rs:1627-1630`）で `store_arc_entry`。
- .gmd: 7 行のテキスト（`app.rs:1691-1716`）→「.gmd の変更を .arc に反映」。
- セーブへは「セーブに書き戻す」（`app.rs:1391-1397`）→ `write_arc_to_save`（`app.rs:486-516`）が **`store_arc_entry` を先に呼ぶ**（`app.rs:491`）ので、反映ボタンを押し忘れていても書き戻し時に flush される。
- 実測: 制限時間 50 → 3、タイトル「皐月に竜と泳ぐ」→「監査で書き換えたタイトル」が、保存→再読込後も一致。GUI 経由でも同様（`gui_mib_edit_and_write_back_persists` / `gui_gmd_edit_and_write_back_persists`）。
- 実 exe で「開く→書き戻す→保存」しても **ファイルはバイト一致**（無変更なら no-op が保証されている）。


### 2.5 オトモ — できる（ラベル要確認）

- 名前 `TextEdit` → `palico::set_name`（`app.rs:2136-2148`）、レベル `DragValue 1..=99` → `set_level`（`app.rs:2149-2163`）、なつき `DragValue 0..=100` → `set_enthusiasm`（`app.rs:2164-2178`）。すべて即時反映。
- 保存→再読込で一致（`palico_edits_persist / gui_palico_and_guild_persist`）。
- **要確認**: ラベルは「なつき」だが、コアの定義は `ENTHUSIASM = 0x26`「Enthusiasm, which the game calls tension」（`palico.rs:28, 262-276`）。仕様書と突き合わせて呼称を決めるべき。
- 系統・修得スキルは読み取り専用（`app.rs:2179-2186`）。

### 2.6 ギルドカード — できる（一部・分かりにくい）

- HR・カード側プレイ時間・コメントの 3 つを編集可（`app.rs:2316-2360`）。変更は即 `guildcard::set_*`（`app.rs:2361-2372`）。
- 保存→再読込で一致（HR 967→321、時間 2409621→123456、コメント変更）。
- **分かりにくい点**
  - 「カード側プレイ時間」はキャラ本体のプレイ時間（`OFF_PLAY_TIME`）とは**別物**。カードだけ変えてもセーブ選択画面の時間は変わらない。
  - HR も同様にキャラ本体の HR（`OFF_HUNTER_RANK`）と独立。両方変えないとゲーム内表示と食い違う可能性がある。
  - カード ID は表示のみ（`app.rs:2345-2346`）。
  - 闘技大会のタイムは一覧表示のみ（`app.rs:2378-2404`）。`guildcard::set_arena_time` はコアにあるが GUI 未使用。

### 2.7 狩猟記録 — できるが、既定では保存されない（危険）

- 表の DragValue は `self.hunt_slay / hunt_capture / hunt_size` という**タブ専用バッファ**を編集するだけ（`app.rs:2243-2278`）。セーブに書くのは「変更を反映」を押したときだけ（`app.rs:2219-2221` → `apply_hunts`, `app.rs:211-230`）。
- `write_current`（`app.rs:272-291`）は `apply_to_save`（キャラ＋アイテムボックスのみ、`app.rs:233-251`）しか呼ばない。**狩猟記録は保存に載らない。**
- スロット切替（`app.rs:942-952`）も `apply_to_save` だけなので、**dirty でも狩猟記録は捨てられる**。
- 実証（GUI コードで）: `hunt_slay[0] = 4321` にして `write_current` → ファイルは元の 24 のまま。同じ値を `set_hunt_count` で書いてから保存すると 4321 になる（`gui_hunt_edits_are_lost_unless_the_apply_button_is_pressed`）。
- なお「変更を反映」自体は正しく動く（`hunt_records_persist`: 137 体すべてを書き戻し、再読込で一致）。

### 2.8 マイセット — 名前のみ

- アイテムセット / 装備セットとも **名前の `TextEdit` だけ**（`app.rs:2059-2104`）。中身は `used_items()` / `parts` をラベル表示するのみ。
- 名前は保存され、再読込で一致（`my_set_names_persist`）。
- コアには `set_item_set_entry`（`myset.rs:193-217`）があるが GUI 未使用。装備セットの構成を書く API は無い。

### 2.9 保存が実際にファイルへ反映されるか — できる

- `上書き保存`（`app.rs:777-781`）→ `write_current`（`app.rs:272-291`）: `apply_to_save` → `.bak` 作成（`backup_before_write`, `app.rs:257-270`）→ `std::fs::write(path, s.to_bytes())` → `dirty=false` と `save_path` 更新。
- **実 exe で確認**
  - 開くだけ → `MHNXEDITOR_SAVE_OUT` で書き出したファイルは元と **SHA-256 完全一致**（`D06B7252…`）。＝開いて保存しても壊れない。
  - セーブ内クエストを書き戻して保存しても **完全一致**（無変更なら真の no-op）。
- **編集の永続**（テスト + 実 exe 再表示）
  - アイテム: `item_test.bin` → 実 exe で 2243 件・箱 #2243 ID 46 個数 3 を確認
  - お守り: `charm_test.bin` → 実 exe で 23 番の新規・224 の +5・225 の消滅を確認
  - クエスト: `quest_add_test.bin` → 実 exe で「セーブ内のクエスト 1020020」を開ける（12 エントリ、1827 バイト）
  - ギルド / 狩猟 / オトモ / マイセット: 再読込で一致
- **残る穴**
  - 狩猟記録（§2.7）は保存に載らない。
  - `.bak` は 1 世代のみ（同名に上書き）。
  - `backup_before_write` はコピー失敗を握り潰して None を返す（`app.rs:266-269`）。バックアップが取れなくても「書き出しました」と出る。
  - 書込後の検証（読み直して比較）はしていない。`to_bytes` が正しい前提。


---

## 3. 横断的に見つかった不具合

### A. サイドバーが画面の 55% を占め、中央ペインが切れる（P1 / 全タブ）

- `side_panel` は `default_width(340.0)`（`app.rs:864`）だが、実測では境界が **1600px 中の x=889**（≒711pt、既定の 2 倍以上）。
- 原因はタブ列: 11 個の `selectable_value` を 1 行の `ui.horizontal` に並べている（`app.rs:868-881`）。実測のタブ列幅 864px（≒691pt）と一致する。egui の `SidePanel` はパネル幅を「内容の矩形」で保存し直す（egui 0.32.3 `panel.rs:298,343`）ため、内容幅に引き上げられたまま固定される。
- 結果、中央ペインは約 569pt しかなく、以下が**画面外**になる:
  - アイテム: 「全アイテムを 99 個にする」の右半分、「重複を除去」「**元に戻す**」（`tab_items.png`, `app.rs:1108-1154`）
  - お守り: スキル 1/2 の**ポイント欄**と装飾スロット 4 個目（`x2_charm_selected.png`）
  - DLC: 「開いている .arc を追加」の右側（`x4_dlc_arc_open.png`）
  - クエスト: フィールド値の右端（`x3_quest_from_save.png`）
- 実画面は 1536x864 / DPI 125%（論理 1229x691）なので、最大化しても改善しない。

### B. 狩猟記録が保存に載らない（P0 / §2.7）

### C. 「開いている .arc を追加」が .mib / .gmd バッファを flush しない（P0）

- `add_open_arc_to_save` は `arc.to_bytes()` を直接呼ぶ（`app.rs:453`）。`write_arc_to_save` が呼んでいる `self.store_arc_entry(self.arc_selected)`（`app.rs:491`）が無い。
- 実証: クエストタブで制限時間を 3 に変え（反映ボタンは押さず）、DLC タブで追加 → 追加された payload は **50 のまま**（`gui_add_arc_drops_uncommitted_mib_edit`）。
- 影響: 「開いて直して追加」という一番自然な使い方で編集が消える。しかもエラーにならない。

### D. お守りエディタが 2 つある（P2）

- お守りタブ（`app.rs:2686-2920`、スキル名検索・レア度警告あり）と、設定/オトモタブ内の「お守り」（`app.rs:3024-3075`、**スキル ID を生の数値で入れる**・検証なし）。
- 同じデータを別 UI で編集でき、片方は ID を直接打てるため不正な値（スキル表 306 件超）も書ける。どちらが正なのか分からない。

### E. エラーメッセージが英語（P2）

- `mhnx_core::Error` の `Display` が英語（`lib.rs:68-78`）。UI は `format!("追加できません: {}", e)`（`app.rs:478`）のようにそのまま出すため、「追加できません: quest id is not in the DLC catalogue: value … out of range」のような混在表示になる。

### F. 毎フレームの重い処理（P2）

- 設定/オトモタブは **セーブ全体を clone** している（`app.rs:2930`）。`Save` は 4.7MB の `Vec<u8>` を持つので、フレームごとに 4.7MB のコピー。
- DLC タブは一覧を作るたびに、**全 175 件の .arc を parse して .gmd を取り出している**（`app.rs:2576-2583`）。フィルタ適用はその後。スクロール中は zlib 展開が毎フレーム走る。

### G. `mhnx-core` が既定の feature ではビルドできない（P2 / 付随）

- `cargo check -p mhnx-core` が失敗する。`dlc.rs` が `#[cfg(feature = "gamedata")]` の `bit_for_quest / position_for_quest` を無条件で呼んでいるため（`dlc.rs:236, 256, 547`）。
- ワークスペース全体では GUI/CLI が `gamedata` を有効にするので通るが、`cargo test -p mhnx-core` 単体は失敗する（監査では `--features gamedata` を付けて実行した）。


---

## 4. 修正提案（優先度付き）

### P0 — データが黙って消える

| # | 対象 | 修正 |
|---|------|------|
| P0-1 | 狩猟記録 | `write_current`（`app.rs:272`）の先頭で、編集があれば `apply_hunts()` を呼ぶ。少なくとも `self.dirty && !self.hunt_slay.is_empty()` のとき。スロット切替（`app.rs:946-948`）も同じ。あるいは狩猟記録も即時反映にして「変更を反映」を廃止する（他のタブと揃う）。 |
| P0-2 | .arc 追加 | `add_open_arc_to_save`（`app.rs:448`）の冒頭に `self.store_arc_entry(self.arc_selected);` を追加。`write_arc_to_save` と同じ 1 行。 |

### P1 — 触れない・分からない

| # | 対象 | 修正 |
|---|------|------|
| P1-1 | サイドバー幅 | タブ列を `ui.horizontal_wrapped` にする、またはタブをトップバーへ移す。`SidePanel` の幅を `width_range(300.0..=360.0)` で固定するのも有効。これでアイテムの「元に戻す」・お守りのポイント欄・DLC 追加ボタンの clipping も同時に解消する。 |
| P1-2 | アイテム追加 | `item_show_empty` の既定を true にする、または詳細パネルに「空き枠へ追加」ボタン（`items.iter().position(|s| s.id == 0)` を選ぶ）を置く。少なくとも「空きスロットも表示」に気付けるヒントを出す。 |
| P1-3 | お守り新規 | 「空き枠に新規作成」の既定枠を `charm::first_free()` にする（無ければメッセージ）。`charm_index` の手入力は最後の手段にする。 |
| P1-4 | お守り削除 | 削除を `ui.menu_button` で「削除」確定にする、または Undo を用意する。少なくとも確認 1 回。 |

### P2 — 品質

| # | 対象 | 修正 |
|---|------|------|
| P2-1 | 二重お守りエディタ | 設定/オトモ側の「お守り」を削除するか、「生の数値で編集（上級者向け）」と明示してスキル ID の範囲検査を入れる。 |
| P2-2 | エラー表示 | UI 側で `Error` を日本語に写像する小さな関数を用意し、`追加できません: {}` を「追加できません: このクエスト ID は配信カタログにありません」等にする。 |
| P2-3 | ギルドカード | キャラ本体の HR / プレイ時間との関係を注記するか、カード側を編集したら本体側も同期するチェックボックスを置く。 |
| P2-4 | マイセット | 「名前のみ編集できます」を明記。可能なら `set_item_set_entry` を使った構成編集を追加。 |
| P2-5 | 性能 | 設定/オトモの `self.save.clone()`（`app.rs:2930`）をやめ、必要な値だけ先に取り出す。DLC 一覧の .arc/.gmd 解析結果を `HashMap<i32, String>` などでキャッシュする（`app.rs:2576`）。 |
| P2-6 | core の feature | `dlc.rs` の `bit_for_quest` 呼び出しを `#[cfg(feature = "gamedata")]` で囲むか、カタログを feature 非依存にする。 |
| P2-7 | .bak | 世代番号（`.bak1`, `.bak2`）か日時付きにする。バックアップ失敗時は警告を出す（`app.rs:257-270`）。 |


---

## 5. 再現手順

### 5.1 画面（実 exe）

```powershell
$env:MHNXEDITOR_OPEN_SAVE = 'C:\anyWorkspace\nx\mhxx\mhnxeditor\.work\gui_audit\work2\system'
$env:MHNXEDITOR_OPEN_TAB    = 'items'      # save|items|quest|dlc|charm|equip|hunts|guild|palico|sets|extras|about
$env:MHNXEDITOR_SCREENSHOT  = 'C:\anyWorkspace\nx\mhxx\mhnxeditor\.work\gui_audit\shots2\out.png'
Start-Process 'C:\anyWorkspace\nx\mhxx\mhnxeditor\dist\mhnxeditor.exe' -PassThru | Wait-Process
```

補助フック: `MHNXEDITOR_ITEM_SELECT=<index>`（0 始まり）/ `MHNXEDITOR_ITEM_FILTER=<語>` / `MHNXEDITOR_CHARM_SELECT=<index>` / `MHNXEDITOR_OPEN_ARC=<file.arc>` / `MHNXEDITOR_OPEN_QUEST=<id>` / `MHNXEDITOR_WRITE_BACK=1` / `MHNXEDITOR_SAVE_OUT=<file>`。

### 5.2 テスト（20 本）

```powershell
cd C:\anyWorkspace\nx\mhxx\mhnxeditor
New-Item -ItemType Directory -Force crates\mhnx-core\tests, crates\mhnx-gui\tests | Out-Null
Copy-Item .work\gui_audit\tests_src\mhnx-core\audit_flows.rs crates\mhnx-core\tests\
Copy-Item .work\gui_audit\tests_src\mhnx-gui\gui_flow.rs   crates\mhnx-gui\tests\
cargo test -p mhnx-core --features gamedata --test audit_flows -- --nocapture --test-threads=1
cargo test -p mhnx-gui  --test gui_flow   -- --nocapture --test-threads=1
```

テストは `.work\gui_audit\work2\baseline_system.bin`（原本の複製）だけを読み、出力は `.work\gui_audit\tests\` に書く。原本・実セーブは変更しない。


---

## 6. 証跡

| ファイル | 内容 |
|----------|------|
| `shots2/tab_save.png` … `shots2/tab_about.png` | 全 12 タブの描画（パニックなし） |
| `shots2/x1_items_picker.png` | 空きスロット #2243 を選び、名前検索で回復薬を出した状態 |
| `shots2/x2_charm_selected.png` | お守り 224 の編集画面（ポイント欄が画面外） |
| `shots2/x3_quest_from_save.png` | セーブ内クエスト 1010165 の .mib 編集画面 |
| `shots2/x4_dlc_arc_open.png` | .arc を開いた DLC タブ（追加ボタンが右端で切れている） |
| `shots2/z1_reopen_item.png` | item_test.bin を実 exe で再表示（2243 件 / #2243 ID46 x3） |
| `shots2/z2_reopen_charm.png` | charm_test.bin を再表示（23 新規 / 224 編集 / 225 削除） |
| `shots2/z3_reopen_added_quest.png` | quest_add_test.bin を再表示（1020020 がセーブ内にある） |
| `shots2/y1_saveout.png` / `shots2/y2_writeback.png` | 実 exe の保存フック（無変更保存がバイト一致） |
| `.work/gui_audit/work2/exe_saveout_nocng.bin` | 実 exe 出力（元と SHA-256 一致） |
| `.work/gui_audit/work2/exe_writeback.bin` | 実 exe のクエスト書き戻し出力（元と SHA-256 一致） |
| `.work/gui_audit/tests/*.bin` | 各テストの出力セーブ（再表示用） |
| `.work/gui_audit/tests_src/` | 監査テスト 2 本のソース |
| `.work/gui_audit/probe/` | セーブ内容の調査用ワンオフ（空き枠・カタログ・チャーム生バイト） |

### 実測値

- セーブ: Switch MHXX / 4,726,188 B / DLC セクション 0xB280 / スロット基点 0x126474
- クエスト: 175 件導入済み（導入ビット 175）、空き枠 30、カタログ未導入 26
- アイテム: 2242 / 2300 使用（空き 58、先頭 2242）
- お守り: 188 個、最初の空きレコード 23
- オトモ: 17 匹 / 狩猟記録: 137 体中 129 体に記録あり
- アイテムセット 24（使用 2）/ 装備セット 40（使用 9）

---

## 7. 付記

- 監査中、`crates/mhnx-core/src/hbedit.rs` が別作業で変更されていた（`git status` に `M`）。本監査はこの変更に依存していない（読み取りのみ）。
- `dist/mhnxeditor.exe` はソースより新しい（20:07:05 > 19:39:23）ため、判定は現行ソースに対応する。
- 実セーブ `C:/Users/haruk/AppData/Roaming/Ryujinx/...` には読み書きしていない。

---

## 8. 追試: 実際のマウス操作で確認した結果（別セッション）

上の監査は「ウィジェットのクリック自体は自動化していない」と明記しているため、実ウィンドウをマウスで操作する追試を別セッションで行った。詳細・スクリーンショット・再現手順は **[review_gui_audit_clicks.md](review_gui_audit_clicks.md)** を参照。

- 実行ビルド: 6,087,680 B / 15:19:58（アイテム・お守り）、6,095,360 B / 20:07:05（クエスト）、6,095,360 B / 20:34:12（オトモ表示）。監査中に exe が再ビルドされたため、#3 / #6 は当時のビルドの結論で、現行では本文 §2.3 / §2.6 のとおり。
- 操作: `SetProcessDpiAwarenessContext(-4)` + `SetCursorPos` + `mouse_event`、クライアント領域を `CopyFromScreen` で撮影。

| # | 項目 | 実クリックでの結果 |
|---|------|--------------------|
| 1 | アイテム追加 | **できる**。空き #2243 に「秘薬」を追加 → 上書き保存 → デコードで `(id 28, count 1)`、再起動後も 2243 件。ただし空きは既定で非表示・末尾 2243〜2300 番・チェックボックスが右端で切れる |
| 2 | お守り編集 | **できる**。枠 224 の装飾スロットを `[3 slots]→[---]` に変更 → 適用 → 保存 → CLI で確認。**スキル 1/2 のポイント欄は 260 pt コンボの右で画面外**（`app.rs:2794-2814`） |
| 3 | DLC 追加 | 当時のビルドは **不可**（UI 無し・`install_dlc` は死コード）。現行は `app.rs:2519` に追加ボタンあり |
| 4 | クエスト編集 | **できる**。制限時間 50→70 → 書き戻す → 保存 → 抽出 .arc の `.mib` `0x16 = 70` を確認（差分 111 B、DLC セクション内） |
| 5 | オトモ | UI は編集可。クリック自動化が効かず、**保存までの実証は未完了**（ウィジェットは `app.rs:2159-2209`） |
| 6 | ギルドカード | 当時のビルドは表示のみ。現行は `app.rs:2339-2382` で HR / 時間 / コメント編集可 |
| 7 | 保存 | **できる**。3 系統で「保存 → 終了 → 再読込」を確認。`.bak` も元と SHA-256 一致 |

追試で新たに分かった点:

- **.mib を編集しても下部が「○ 変更なし」のまま**（`dirty` を立てるのは「セーブに書き戻す」だけ）。編集したつもりで閉じると失われる。
- 既定 1280x820 pt は 125% で 1600x1025 px。1536x864 の画面では右下がはみ出す（`main.rs:47-50`）。
- クエストタブは外側ページと 420 px の .mib 一覧の二重スクロールで、既定サイズでは「.mib の変更を .arc に反映」が画面外。
- `PostMessage` による背面・画面外クリックは egui/winit に無視される（検証済み）。今後の自動化は前面操作が必要。

追試からの追加修正提案:

| 優先 | 対象 | 修正 |
|------|------|------|
| P1 | .mib 編集の dirty | フィールド / 16 進グリッドの書き込み時に `self.dirty = true` を立てる。今は「セーブに書き戻す」まで未保存表示が出ない |
| P1 | お守りのポイント欄 | スキル名コンボとポイントを別行にする、またはポイントをコンボの左へ移す（`app.rs:2794-2814`） |
| P2 | 「空き枠に新規作成」 | 現在の入力値で作る（`app.rs:2892-2900` の 0 固定をやめる）。空き枠の既定は `charm::first_free` に |
| P2 | 既定ウィンドウ | `main.rs:47-50` の 1280x820 pt を画面の作業領域に収める（例: `min(1280, work_area)`） |



