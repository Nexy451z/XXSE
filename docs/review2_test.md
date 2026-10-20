# レビュー2 — テスト・CLI・ドキュメントの整合性

担当: テスト/CLI/ドキュメント整合性レビュー
作業ディレクトリ: `C:\anyWorkspace\nx\mhxx\mhnxeditor`
日付: 2026-10-21
**コードは変更していない。** 作業用コピーは `.work\review_test\` に作り、実セーブ
(`C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system`) は読むだけにした。
数値はすべて実測。**推測**はその都度そう書く。

---

## 0. 要約

| 項目 | 実測 |
|---|---|
| `cargo test --workspace -j 1` | **205 passed / 0 failed / 3 ignored**（11 バイナリ、`hbedit_flow` が 37.35 s）。この機械では全通過 |
| 内訳 | クレート内ユニットテスト **44**、統合テスト **161**（+ ignored 3） |
| 統合テストのうち、データが無いと**落ちる**もの | **17 件**（`entry_types` 3 / `quest_injection` 6 / `regression_review` 8） |
| 統合テストのうち、データが無いと**黙って成功する**もの | **117 件以上**（`hbedit_flow` 96、`webapp_features` 9、`real_data` 7、`check_quests` 4、`talisman_ranges` 1） |
| クリーンな checkout で `cargo test --workspace` | **通らない**（上記 17 件が panic / assert 失敗。`dist/quests_all` は `.gitignore:7` で除外） |
| CLI のテスト | **0 件**（`crates/mhnx-cli/src/main.rs` に `#[test]` が 1 つも無い。`cargo test` の当該バイナリは "running 0 tests"） |
| CLI のコマンド数 / `usage()` に載っている数 | **36 / 25**（11 コマンドがヘルプから発見できない） |
| 統合テストから一度も呼ばれない公開関数 | **77 / 254**（うち 5 つはプロダクションからも未使用＝死んだ API） |

**結論**: テストの中身の質は高い（バイト一致・実データ照合・ファズがある）。問題は**「データが無いときに静かに成功する」運用**で、
このリポジトリの「121 件 + 他、全通過」は**この機械のデータがあるから成り立つ**数字である。CLI は 1,033 行 36 コマンドで
テストゼロ、かつ**入力ファイルを直接上書きするコマンドが 4 つ**あり、そのうち 1 つは
「入力セーブは変更していません」と**嘘を表示する**（実測）。ドキュメントは `quest_injection_spec.md` が
実装と真逆のことを書いたまま残っている。

---

## 1. P0 — データ破壊につながる / 明示的な嘘

### P0-1 `add-quest` は「入力セーブは変更していません」と表示しながら入力セーブを書き換える

* 実装: `crates/mhnx-cli/src/main.rs:450-465`。`out` に書く（`main.rs:461`）だけで、
  **`save_path` と `out` が同一ファイルかどうかを検査していない**。メッセージは `main.rs:463`。
* 実測（作業コピーで実行）:

```
$ mhnx add-quest .work\review_test\same out\group4\compact\q1111009_flags0.arc .work\review_test\same
added quest 1111009 to slot 135 (mask bit 223), wrote .work\review_test\same
the input save was not modified
input changed by out==in ? True        <- SHA256 が変化
$ mhnx dlc-list .work\review_test\same | head -1
176 installed download quest(s)        <- 175 から増えている
```

  同じ操作で `out` を別ファイルにすると入力は不変（SHA256 一致）なので、
  **「別ファイルを指定したときだけ成り立つ保証」を無条件の断定で表示している**のが問題。
* 影響: 利用者はこの 1 行を根拠にバックアップを省く。レビュー依頼でも
  「add-quest が入力セーブを変更しないことの保証」が論点になっている。
* 直し方（提案）: `out` と入力を `std::fs::canonicalize` で比較して同一ならエラーにする。
  もしくは成功メッセージを「`<out>` に書いた（入力は変更していない）」に変え、同一パスのときは何も言わない。

---

## 2. P1 — 明確なバグ / 使い勝手を損なう / 再現性が無い

### P1-1 入力セーブを直接上書きする CLI コマンドが 4 つあり、どれもバックアップを作らない

| コマンド | 書き込み | 実測 |
|---|---|---|
| `install` | `main.rs:215` `fs::write(save_path, ..)` | 入力置換。`.bak` は作られない（実測: `inst.bak` 無し） |
| `cat-add` | `main.rs:773` | 実測: 入力の SHA256 が変化、`.bak` 無し |
| `cat-del` | `main.rs:793` | 同上（実測: スロット 3 を削除） |
| `slot-import` | `main.rs:689` | 実測: 入力の SHA256 が変化（1,173,440 B をスロット 2 へ） |

* 一方 `add-quest / equip-set / fix-oneplayer / to-3ds / arc-repack / arc-compact / slot-export` は
  明示的な出力パスを取る。**「入力を壊すコマンドだけが出力引数を持たない」**という一貫性の無い設計。
* `install / cat-add / slot-import` は最後に「NOTE: write system_backup too」と表示するが、
  自分では書かない（`main.rs:217`）。NOTE を読み飛ばすと `system` と `system_backup` が食い違う。
* `slot-import` がサイズ違いを弾くのは正しい（実測: `error: bad size: got 1234 want 1173440` で入力は無傷）。
  `install` も「セーブに無いクエスト」を拒否するので安全側（実測）。危険なのは**正常系**で無警告に上書きする点。
* 直し方（提案）: 上書き系は (a) 同一ディレクトリに `<name>.bak` を必ず作る、(b) `--in-place` を
  明示しない限り拒否する、(c) `system` を書くときは `system_backup` も同時に書く、のどれか。

### P1-2 CLI の数値引数が `unwrap_or(0)` で、タイプミスが「0 番」として実行される

* `main.rs:984-993`（`equip-set`）は slot/record/index/item をすべて `unwrap_or(0)` で受ける。
* 実測:

```
$ mhnx equip-set base_orig x y supply z w 3 out
slot 0 rec 0 supply[0] = item 0 x3, wrote out      <- exit 0、書き込み成功
```

  4 つのタイプミスが全部 0 になり、**意図していないレコードにアイテム 0 を書いた**。
  `set_quest_equipment_item` は item=0 を許すので素通りする。
* 読み取り系では「全スロット」に化ける（実測 `mhnx charms base abc` → exit 0 で 3 スロット全部を列挙。
  `items / mysets / guild-card / hunts` も同じ。`main.rs:976-980, 1003-1004, 1012-1013`）。
  読み取りなので P2 だが、同じ書き方なので一緒に直すべき。
* 直し方: `parse().map_err(|_| format!("slot must be a number: {}", s))?` に統一する。
  `cmd_slot_export / cmd_hunts / cmd_charms` は本体側で範囲検査しているので、parse 段で失敗を返すだけで足りる。

### P1-3 統合テストの 7 割が「データが無ければ黙って成功」で、テスト方針が 3 種類に分裂している

| 方針 | ファイル | 件数 | 実装 |
|---|---|---|---|
| 黙って `return` | `hbedit_flow.rs:61` ほか、`check_quests.rs:20`、`real_data.rs:21`、`webapp_features.rs:19`、`talisman_ranges.rs:10` | **117+** | データ欠落時に 0 アサートで pass |
| `panic!`（`MHNX_SKIP_REAL_SAVE` で任意スキップ） | `quest_injection.rs:15-29`、`regression_review.rs:18-33` | 14 | データ欠落時に**落ちる** |
| `assert!(dir.is_dir())` | `entry_types.rs:19-21` | 3 | `dist/quests_all` 必須 |

* `real_data.rs:1-4` のコメントは「so the suite still passes on a clean checkout」と書くが、
  **同じクレートの `quest_injection.rs / regression_review.rs / entry_types.rs` が clean checkout で落ちる**ので、
  クレート全体としては成り立っていない。
* 正しい方針は既に `regression_review.rs:14-17` に書いてある
  （「A missing save is a hard failure rather than a quiet pass: six tests used to go green on a machine
  that had no dump at all」）。**このヘルパを他 5 ファイルへコピーするだけ**で解決する。
* 併せて `hbedit_flow.rs:16-21` のパスを直したい。`real_arc()` は
  `C:\anyWorkspace\nx\mhxx\save_editor\mhxx_quetool\quest\q1010165.arc`（**このリポジトリの外**、実測 2,233 B）を
  指している。素材は既に `C:\anyWorkspace\nx\mhxx\arcs\`（event 223 / all_event/playable 189 / dead 34 /
  village 1,632 / save_event 175、すべて実測）に揃っているので、そちらへ移せるはず。
  外のディレクトリが消えた瞬間、`hbedit_flow` の 3 件が静かに消える。

### P1-4 テストが無い（だけでなく呼び出し元も無い）公開関数が 5 つ、テストが無い重要機能が 2 つ

* 死んでいる公開 API（参照は定義のみ、実測）:
  `save::erase_slot`（`save.rs:996`、セーブ枠消去）、`Save::palico_equipment_box`（`save.rs:758`）、
  `Save::palico_data`（`save.rs:763`）、`Save::guild_card`（`save.rs:768`）、
  `guildcard::set_arena_time`（`guildcard.rs:190`）。
  消すか、テストを書くか、どちらかにすべき（`erase_slot` は破壊的 API なので特に）。
* `arc::to_bytes_compact`（`arc.rs:215`）は **CLI の `cmd_arc_compact`（`main.rs:234`）以外に
  呼び出し元が無く、テストもゼロ**。これは 0x8000 パディング除去（`docs/uninstalled_1..4.md`）の
  実装そのもので、**直近でいちばん重要なデータ経路が自動検証されていない**。
  手で確認した結果: `q1010002.arc` 35,258 B → 3,642 B、サイズは
  `arcs/all_event/playable/q1010002.arc`（3,642 B）と一致。ただしバイトは **+0x08 の 4 バイトだけ違う**
  （compact は `00 00 00 00`、playable は `fe ff ff ff`）。セーブ内のレコード
  （`arcs/save_event/*`、5 件抜き取り実測）は flags = 0 なので、**セーブ互換なのは compact 側**。
* ギルドカードの**書き込み側**はクレートテストでは未検証（読み取りだけ）。
  `set_hunter_rank / set_play_time / set_comment` は `hbedit.rs:1677/1687` と
  `mhnx-gui/src/app.rs:2436-2442` から呼ばれるが、Switch 画面側のテストは `#[ignore]`
  （`hbedit_flow.rs:4190`「the guild card screen has no editor」）。
* クエストの**削除**（`dlc::remove_quest_by_id` / `hbedit::remove_save_quest`）に統合テストが無い。
  追加（`dlc::add_quest`）は `hbedit_flow.rs:2534` 付近の 4 件で検証されている。

### P1-5 ビルドに必要なファイルが git 管理外（`official_dlc.bin` 527,555 B）

* `crates/mhnx-core/src/dlc_bundle.rs:7` が `include_bytes!("official_dlc.bin")` しており、
  `lib.rs` の未コミット差分が `pub mod dlc_bundle;` を追加している。
* 実測 `git status`: `dlc_bundle.rs` と `official_dlc.bin` は **untracked**。生成元の
  `tools/gen_official_dlc.py` も **untracked**。
* したがって `git clean -fd`（`-x` 無し）を打つと、追跡済みの `lib.rs` は残り
  `dlc_bundle.rs` だけ消えて **クレートがコンパイルできなくなる**。今の作業ツリーは
  「(a) `official_dlc.bin` を commit する、(b) `build.rs` で生成する、(c) `include_bytes` をやめる、
  のどれかを選ぶまで再現性が無い。
* 関連: 現在 **未コミットの変更が 7 ファイル**（`main.rs / arc.rs / dlc.rs / hbedit.rs / lib.rs /
  switch/editor.rs / switch/ui.rs`）あるため、`dist\mhnx.exe` が現在のソースと一致する保証は無い。実測でも
  `dist\mhnx.exe` と `cargo build --release -p mhnx-cli` の出力は SHA256 が違う
  （サイズは同じ 685,568 B、試した範囲の挙動は同じ）。**配布物に世代を刻む仕組みが無い**。

### P1-6 `docs/quest_injection_spec.md` が実装と真逆

* `3.3（`quest_injection_spec.md:158-164`）「したがって本ツールは**ビットマスクを書き換えません**。
  空きスロットへの新規追加は、必要なビットが分からないため拒否します」
  `5.1（`:202-213`）「ビットマスクを一切書かない方式に変更しました」「enable_all / clear_all /
  set_quest_bit / quest_bit は削除」
* 実装: `dlc::set_installed`（`dlc.rs:255-275`）と `dlc::add_quest`（`dlc.rs:540-593`）が
  マスクを書く。実測: `add-quest` で **ビット 65**（1010002）と **ビット 223**（1111009）が立ち、
  セーブの導入数が 175 → 176 になった。
* `7（`:259-273`）のテスト名 7 つのうち **5 つが現存しない**
  （`the_installed_bit_set_lines_up_with_the_records` → 実際は `the_installed_set_is_not_indexed_by_slot`、
  `replacing_a_quest_keeps_the_slot_and_the_payload` → `..._the_id_and_the_mask`、
  `adding_a_new_quest_sets_only_its_own_bit` → `adding_a_quest_is_refused_and_writes_nothing`、
  dlc モジュールの 2 件 `installing_touches_only_its_own_bit / installing_an_existing_id_replaces_in_place` は
  `replacing_keeps_the_slot_the_id_and_the_mask / a_quest_that_is_not_installed_is_refused` に変わっている）。
  「実セーブに対して 5 件」も実際は **6 件**。
* この 1 ファイルだけを読んだ人は「新規追加は不可能」と理解する。**冒頭に「`3.3/`5.1 は
  `review_catalogue.md` で覆された（現在は `dlc::add_quest` がある）」と書くか、該当節を書き換えるべき。**

### P1-7 README の数値・手順が実物と違う

| 記述 | 実測 |
|---|---|
| `README.md:86` `dist/mhnxeditor.exe` 約 5.4 MB | **6,099,960 B**（5.8 MiB） |
| `README.md:87` `dist/mhnxeditor.nro` 約 663 KB | **1,726,153 B**（1.65 MiB）— 2.6 倍違う |
| `README.md:134`「ITEM BOX 2242 枠の一覧」 | 枠数は `itembox::TOTAL_ITEM_SLOTS = 2300`（`itembox.rs:13`）。2242 は**この 1 セーブの使用数**（実測 `mhnx items` → "2242 / 2300 slots used"） |
| `README.md:64` `cargo build --release -p mhnx-gui` | CLI（`dist\mhnx.exe`、README 自身が案内）が出来ない。`-p mhnx-cli` が要る。実測で両方指定すると 18.3 s で成功 |
| `README.md:287` `cargo test -p mhnx-core` | クリーンな checkout では落ちる（P1-3） |
| `README.md:129-149` の表 | 145-146 行の散文で表が分断され、147 行以降の 3 行（`.mib` / `.gmd` / CHECK）が表として描画されない |

* `README.md:151-152` の「セーブを保存すると同じフォルダの `system_backup` も自動で書き直します」は
  **Switch 版では正しい**（`switch/src/savedata.rs:161-184`）。ただし `savedata.rs:184` で
  `let _ = write_save_in_place(..)` と**エラーを捨てている**ので、バックアップだけ失敗しても
  成功扱いになる（P2）。

---

## 3. P2 — 改善

### P2-1 `usage()` に 11 コマンドが載っていない

`main.rs:14-46` は 25 コマンドしか列挙しない。実装済み 36 コマンドのうち
`arc-compact / add-quest / charm-check / mysets / guild-card / hunts / equip-items / equip-set /
gmd-dump / equip-list / equip-export` がヘルプに無い（実測: `mhnx` の出力に 1 つも現れない）。
**直近で追加された機能が全部ここに含まれる。** `docs/uninstalled_*.md` が案内する `arc-compact` も、
ヘルプからは辿れない。

### P2-2 引数個数が違うときのメッセージが「未知のコマンド」になる

`main.rs:1020-1024` の catch-all。実測:

```
$ mhnx install <save>            -> unknown or incomplete command: install   (exit 1)
```

存在するコマンドの引数不足を「unknown」と表示するので、利用者はコマンド名を疑う。
「install needs 2 arguments (got 1)」と usage の該当行を出すべき。

### P2-3 エラーメッセージが行動につながらない / 読みにくい

* `add-quest` に romfs のままの .arc を渡したとき（実測）:
  `error: quest payload larger than a slot: value 35258 out of range`
  → 正解は `mhnx arc-compact`（実測 35,258 → 3,642 B）。メッセージからは辿れない。
* `install` で未導入クエストを指したとき、内部の複行文字列が 1 行に潰れて表示される（実測）:
  `error: parse error: this save does not have that quest yet. ... can be          updated: ...`
  空白が 10 個以上連続する。`dlc.rs` 側の文字列を 1 行に整えるか、CLI 側で折り返す。
* `dlc-extract` は存在しない ID を渡すと**ファイルを書いたあとで** exit 1 になる（実測: 1 ファイル書き込み +
  `error: not in this save: 9990001`）。スクリプトから「何もしていない」と区別できない。

### P2-4 `docs/review_catalogue.md `4.3` の分類が古い（依頼の指摘どおり）

* **ビット式そのものは実装と一致している**（良い点）。`bit_for_position`（`dlc.rs:210-218`）は
  position < 45 → bit = pos、それ以外 → bit = 64 + pos - 45 で、実測 `1010002` → ビット **65**、
  `1111009` → ビット **223**。`4.3` の表の bit 列と一致。
* 古いのは「配信版 .arc が手元にあるか」の列。`4.3（`:428-439`）は素材を
  `save_editor\mhxx_quetool\quest`（141 個）に限定しているが、現在は
  `arcs\uninstalled`（48）・`arcs\all_event\playable`（189）・`out\group4\compact`（3）・
  `.work\uninst2\rebuilt`（12）が存在する（いずれも実測カウント）。
* さらに `2.8（`:363-364`）は「プレースホルダ 9 件のうち配信版 .arc があるのは位置 198 の 1 件だけ」と書くが、
  `out\group4\compact\q1111009_flags0.arc` が存在し、**それを `add-quest` で導入できてしまった**
  （位置 204 = ニャンターＧ級予備２、ビット 223、実測）。「入れても意味が無い枠」という**分類はコードで一切
  強制されていない**（`dlc::add_quest` の検査は `dlc.rs:541-578` の 4 つだけ: サイズ ≤ 5,112 /
  カタログ在籍 / 未導入 / 空き枠あり）。`4.2` が定めた「41..44 を拒否」は `bit_for_position` が
  `None` を返すことで偶然満たされているが、155・196〜204 のプレースホルダは素通りする。
* `4.3` は位置 202（1110113）を「追加できる」に入れているが、`2.6（`:289-303`）と `4.2` 手順 3 は
  「188 に正規化するので 202 は触らない」と言っている。表の中で矛盾している
  （実装は `position_for_quest` が最初の一致を返すので 188 に正規化される＝実装は `2.6` 側）。
* 提案: `4.3` に「この表は `out/group4`・`arcs/uninstalled` 追加前のもの」と注記し、
  「プレースホルダを拒否するのはコアの責任かフロントエンドの責任か」を決めて `4.2` に書く。

### P2-5 `docs/review_romfs_patch.md:18` の「4 バイトだけ書き換わる」が現在の実装と合わない

* 同書は `mhnx arc-repack` が `+0x08` を `0xFFFFFFFE` → `0x00000000` に変えると書く（集約 .arc の話）。
* 実測（個別クエスト .arc）: `arc-repack q1010002.arc` の出力は**入力と 35,258 バイトすべて一致**、
  ヘッダは `fe ff ff ff` のまま。`uninstalled_1.md:472` の「パディングを保持したまま出力
  （35,258 B のまま）」は今も正しい。
* 一方 `arc-compact` は `+0x08` を 0 にする。**どちらの writer が何を 0 にするのか**を
  docs に 1 行ずつ書かないと、`arcs/all_event/playable`（flags 0xFFFFFFFE）と
  `arc-compact` の出力（flags 0）を取り違える。セーブ側は flags 0 なので、
  「セーブに入れるなら `arc-compact`」を明記したい。

### P2-6 弱いアサーションが 3 か所残っている（うち 1 つは過去レビューが指摘済み）

* `hbedit_flow.rs:1849-1851` — `let above = total - below;` の直後に
  `assert_eq!(below + above, total);`。**恒真**で絶対に落ちない。
* `hbedit_flow.rs:1794` — `if mhnx_core::dlc::install_quest(&mut s, id, &raw).is_ok() { ... assert ... }`。
  `install_quest` が失敗するとアサートが 1 つも走らずに pass する。
* `webapp_features.rs:86` — `assert_eq!(0x62EE + charm::CHARM_BOX_SIZE, 0x17C2E);`（定数 2 つの言い換え）。
  `docs/review_save_layout.md:259` が同じアサーションを名指しで批判しているが、**未修正のまま残っている**。
  実データに対する検証に置き換えるべき。

### P2-7 `#[ignore]` の spec テスト 3 件は良いが、腐る仕組みしかない

`hbedit_flow.rs:4167 / 4190 / 4208` は「未実装の機能」を仕様として固定していて有用。
ただし理由文は既に古くなりかけている（`:4167` は「`install_into_save` only overwrites an installed quest」
だが、今は `dlc::add_quest` が存在し、CLI からは追加できる）。`#[ignore]` の理由に
「どのレポートで未実装と確認したか」を書くか、`docs/review_verify_plan.md` 側に索引を作るべき。

### P2-8 `switch/build.ps1` の脆さ

* `build.ps1:222` の `-Probe` は `cargo build ... 2>&1 | Write-Host` で
  **`$LASTEXITCODE` を検査しない**。失敗しても「probe ELF not found」（`:227`）という
  見当違いのメッセージで落ちる。また本体（`:134`）は `--target $targetJson` を明示するのに、
  `-Probe` は指定しない（`switch/.cargo/config.toml` の `[build] target` に依存）。同じ
  スクリプト内で 2 通りの渡し方があり、片方だけ壊れても気づけない。
* `:209-211` は `switch/sdcard/*` を `dist` へ丸ごとコピーする。実測で
  `switch/sdcard/switch/mhnxeditor/hello.txt`（プレースホルダ）が `dist` に配られる。
* `$appVersion = '0.1.0'`（`:49`）が `Cargo.toml` の `workspace.package.version`
  （`Cargo.toml:7`）と二重管理。バージョンを上げると .nro の NACP だけ古いまま配られる。
* devkitPro 探索（`:71-87`）は `@($candidates)` を走査しながら `$candidates += ...` する
  変則的なコードで、候補が重複する。動くが、失敗時のメッセージ（`:89`）に候補一覧が
  重複して並ぶ。
* `:205-211` で `dist` を作るが、**PC 側のバイナリは作り直さない**ので `dist` は
  世代混在になる（P1-5 の実測を参照）。

### P2-9 マニフェストまわり

* `crates/mhnx-cli/Cargo.toml:13` は `features = ["std", "gamedata"]` 固定。CLI が
  `gamedata` を外せないのは妥当だが、`mhnx-core` の「no_std 対応」（`lib.rs:5`）を
  検証するテスト/CI が無い（`--no-default-features` のビルドは誰も回していない）。
* `mhnx-core` に `[dev-dependencies]` が無く、テストデータは**すべて絶対パス**。
  `save::empty_switch_save` でダミーセーブを生成できるのに、それ自体が未テスト（P1-4）で
  フィクスチャに使われていない。統合テストの脆さの根本原因はここ。
* `dist/` はワークスペース外で、CI 設定も無い（`.github` 等が無い）。「全通過」を機械的に
  担保する仕組みが存在しない。

---

## 4. 良い点（壊さないでほしい）

1. **バイト一致を最終判定にしているテスト群**。これはこのリポジトリの最大の資産。
   * `real_data.rs:105-145`: 出荷 .arc 全件で `arc.to_bytes()` が**元ファイルとバイト一致**。
   * `regression_review.rs:176-187`: 何も編集せずに書き戻すと**変化バイト 0**。
   * `check_quests.rs:44-52`: 1 人限定フラグの修正が**その 1 バイトだけ**を変えることを、
     他エントリの全バイト比較で確認。
   * `hbedit_flow.rs:1589-1618`: アーカイブへのランダムキー入力後、毎回 `Arc::parse` が通る。
2. **テストはディスクに一切書かない**。`crates/mhnx-core/tests/` 全体を grep して
   `fs::write / File::create / copy / remove_file` は 0 件。書き込みは
   `Editor::take_write()` でメモリ上に取り出して検証している（例 `hbedit_flow.rs:101-110`）。
   実セーブを読むだけ、という約束がコードで守られている。
3. **`regression_review.rs` の「無ければ落とす」方針**（`:14-33`）。理由まで書いてある。
   他の 5 ファイルはこれをコピーするだけでよくなる。
4. **実データ由来の定数を実データで検証している**。`the_name_hash_matches_the_files_ryujinx_wrote`
   （`hbedit_flow.rs:1681-1707`）は `crc32("system") ^ 0xFFFFFFFF = 0x36B2EE74` を
   Ryujinx が書いた実ファイルと突き合わせている。定数の言い換えになっていない。
5. **`docs/review_save_layout.md` のオフセットは現在のコードと一致**。スポットチェックした
   `0x23B7D`（名前）/ `0x2248B`（プレイ時間）/ `0x280F`（所持金）/ `0x62EE`（共有装備ボックス）/
   `0x17C2E`（オトモ装備）/ `0x23BB6`（オトモ）/ DLC `0xB280` はすべて `save.rs` の定数と同じ。
   ドリフト無し。
6. **0x8000 パディングの知見は docs に十分反映されている**。`uninstalled_1.md:40-60`、
   `uninstalled_2.md:204-215`、`uninstalled_3.md:142-157`、`uninstalled_4.md:280-286`、
   `review_romfs_patch.md:230-235` の 5 か所に「データ開始 = alignUp(12+80n, 0x8000)」が実測付きで書かれ、
   実装（`arc.rs:215`）と CLI（`arc-compact`）が存在する。**足りないのは可視性だけ**
   （ヘルプ・README・テスト）。
7. **`review_catalogue.md` の position → bit の式はコードと一致**（実測: 1010002 → 65、1111009 → 223）。
   カタログ 205 件の並びを復元した上で「ビットはカタログ位置」と結論したのは正しく、`dlc.rs:204-232` の
   コメントもその結論を保っている。
8. **`dlc.rs:1159` の「公式 DLC 189 件 + 装備 40 枠を空セーブに入れる」ユニットテスト**は、
   外部ファイルに依存しない**唯一のヘルメスティックな統合テスト**。この形を増やすのが最も効く
   （ただし P1-5 のとおり素材が untracked）。

---

## 5. 優先して直すべき 3 件

1. **`add-quest` の同一パス上書きを禁止し、成功メッセージを実態に合わせる**
   （`main.rs:450-465`）。`in` と `out` が同一ならエラーにする。あわせて
   `install / cat-add / cat-del / slot-import` に `.bak` 作成か `--in-place` 必須化を入れる。
   *根拠*: 実測で「the input save was not modified」を表示しつつ SHA256 が変化し、導入数が 175→176 になった。

2. **統合テストのデータ欠落時の挙動を `regression_review.rs` 方式に統一する**
   （`quest_injection.rs:15-29` のヘルパを他 5 ファイルへ複製し、`hbedit_flow.rs:16-21` の
   `real_arc()` をリポジトリ内の `arcs\` へ移す）。これで「121 件通過」が
   どの機械でも意味を持つ数字になる。
   *根拠*: 161 件中 117 件以上がデータ欠落時に 0 アサートで pass、17 件は clean checkout で落ちる。
   さらに `dist/quests_all` は `.gitignore:7` で追跡外。`dist/*.exe` は `.gitignore:13` で除外（配布物がリポジトリに残らない）。

3. **`official_dlc.bin`（527,555 B）と `dlc_bundle.rs`、生成スクリプトを git に入れる**
   （または `build.rs` で生成する）。加えて `crates/mhnx-core/tests/` に
   `arc::to_bytes_compact` のテストを 1 本足す（romfs arc → 3,642 B、flags 0、`Arc::parse` が通り、
   セーブの `save_event` と同じ flags になること）。
   *根拠*: `include_bytes!` しているファイルが untracked で、`git clean -fd` でビルドが壊れる。
   `to_bytes_compact` は CLI 以外に呼び出し元が無くテストもゼロ。

---

## 付録: 実測に使ったコマンド

```powershell
# テスト（この機械: 205 passed / 0 failed / 3 ignored）
cargo test --workspace -j 1

# ビルド（依頼票のコマンドはそのまま通る。GUI は warning 3 件）
cargo build --release -p mhnx-gui -p mhnx-cli

# 作業コピー（実セーブには書かない）
New-Item -ItemType Directory -Force .work\review_test
Copy-Item C:\anyWorkspace\nx\jksv_ready\system .work\review_test\base

# P0 の再現
Copy-Item .work\review_test\base .work\review_test\same
target\release\mhnx.exe add-quest .work\review_test\same out\group4\compact\q1111009_flags0.arc .work\review_test\same
# -> "the input save was not modified" と表示されるが SHA256 が変化

# P1-2 の再現
target\release\mhnx.exe equip-set .work\review_test\base x y supply z w 3 .work\review_test\eq
# -> slot 0 rec 0 supply[0] = item 0 x3  (exit 0)

# パディング経路の確認
target\release\mhnx.exe arc-repack  C:\anyWorkspace\nx\mhxx\arcs\event\q1010002.arc out\repack.arc   # 35,258 B バイト一致
target\release\mhnx.exe arc-compact C:\anyWorkspace\nx\mhxx\arcs\event\q1010002.arc out\compact.arc  # 3,642 B
target\release\mhnx.exe add-quest   .work\review_test\base out\compact.arc out\added                 # mask bit 65
```

GUI のビルド警告（実測、参考）:

```
warning: unused imports: `BODY_SIZE` and `SWITCH_SIZE`   --> crates\mhnx-gui\src\app.rs:15:50
warning: fields `dlc_files` and `dlc_log` are never read --> crates\mhnx-gui\src\app.rs:116:9
warning: methods `scan_quest_dir`, `install_dlc`, and `remove_dlc` are never used --> crates\mhnx-gui\src\app.rs:597:12
```

（最後の 1 件は「GUI からクエストを導入する経路が配線されていない」ことの傍証。
`hbedit_flow.rs:4167` の `#[ignore]` と同じ欠落を指している。）
