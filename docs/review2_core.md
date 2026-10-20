# mhnx-core ロジックレビュー (review2_core)

対象: `crates/mhnx-core/src/` の save / dlc / charm / guildcard / palico / myset / arc / quest / check
（`mib.rs` は存在せず、`Mib` は `quest.rs` にある）

レビュー日: 2026-10-21 / コードは一切変更していない（読み取りと検証のみ）。

---

## 要約

実セーブと実 .arc を使って検算した。**レイアウトの数値はほぼ全て実測で裏が取れた**。
特に `to_bytes_compact()` は、セーブに実際に入っている 175 本のクエストを
**175/175 バイト完全一致で再現**し、romfs の 223 本も同じレイアウトに落ちる。バイト単位の設計は信頼できる。

一方で、**(1) MHGU セーブに対する書き込みガードの抜け**、**(2) 名前フィールドの UTF-8 途中切断**、
**(3) カタログ由来の「ビットは立つが装備が無い」クエスト** の 3 点は実害が出る可能性が高い。
`mhnx-core` が単体ではコンパイルできない（既定 feature で 5 エラー）点も、
ライブラリとして致命的ではないが確実な欠陥。

| 検証 | 実測値 |
|---|---|
| セーブサイズ | 4,726,188 = 36 + 4,726,152 |
| スロット基底 | 0x126474 / 0x244C34 / 0x3633F4（差はどちらも 0x11E7C0） |
| スロット使用フラグ | body[0x04..0x07] = 1,1,1 |
| DLC セクション | 0xB280。0xB280+0x45F4+205*0x1400+45*0x800 = **0x126474**（slot0 と厳密一致） |
| 導入済みクエスト | 175 本 / マスクのセットビット 175 / 装備エントリ 40 |
| テスト | `cargo test --workspace -j 1` = 205 passed / 0 failed / 3 ignored |
| 元ファイル | jksv_ready/system と Ryujinx 実セーブの SHA256 は開始時と終了時で不変（D06B7252…C112A） |

---

## 検証方法

`.work\review_core\` にセーブをコピーし（SHA256 記録済み）、
書き込み系はすべて **メモリ上の `Save` に対してのみ**実行して、
最後に元 2 ファイルのハッシュが変わっていないことを確認した。
検算用のプログラムは `.work\review_core\probecrate\`（本体ワークスペース外の独立クレート）に置き、
**作業ツリーの現在のソース**を path 依存でリンクしてビルドした（古い target 成果物ではない）。

---

## 重大度別の問題

### P0 — データ破壊

#### P0-1. `dlc::add_quest` と `dlc::set_installed` に MHGU ガードが無い

**根拠**: `dlc.rs:255-275`(set_installed)、`dlc.rs:540-593`(add_quest) には `check_kind(save)?` が無い。
`dlc.rs:371-383` のモジュール doc は「Everything that writes refuses to touch an MHGU save」と明言しているが、実際は
`install_quest`(595) / `uninstall_quest`(670) / `install_dlc_quests_by_category`(722) / `toggle_bundled_quest`(856) /
`remove_quest_by_id`(843) だけがガードされている。

**実測**（実セーブの body を MHGU サイズまで伸ばし、36 バイトヘッダを付けて `SaveKind::Mhgu` にした検体）:

    list_quests -> 0 本（ガードが効いている）
    install_quest   -> Err("download quests are not supported for MHGU saves")   ← 正しい
    uninstall_quest -> Err(...)                                                  ← 正しい
    set_installed   -> Ok                                                        ← 素通り
    add_quest       -> Ok(slot 135)                                              ← 素通り
    この 2 呼び出しで body の 1,358 バイトが変化

**到達経路**:
- GUI は `app.rs:2546-2552` で MHGU を早期 return するため、DLC タブ経由の `add_open_arc_to_save`(app.rs:2568) は守られている（実測で確認）。
- **CLI は守られていない**: `crates/mhnx-cli/src/main.rs:450-465 cmd_add_quest` は MHGU チェック無しで `add_quest` を呼ぶ。
  出力は `out` 引数のファイルなので入力は壊れないが、**書き出したファイルは MHXX オフセットで書かれた壊れた MHGU セーブ**になる。
- ライブラリ利用者から見れば公開 API の契約違反。

**注意（推測ではなく実測との区別）**: キャラクタ内のオフセットは MHXX/MHGU で共通らしく
（`save.rs:110` の `SLOT_DATA_SIZE = 0x11D088` が両者共通で、stride だけ 0x11E7C0 / 0x11F8C4 と違う）、
`write_character` の MHGU 書き込みは今回の検体では元と同一位置に書かれた。危険なのは **DLC 系のオフセット**だけ。
ただし `write_character` にも kind ガードは無い（`save.rs:415`）ので、少なくとも「MHGU では書くな」を一箇所で表明すべき。

---

### P1 — 明確なバグ・使い勝手

#### P1-1. 名前フィールドを UTF-8 の途中で切る（3 箇所）

**根拠**:
- `save.rs:420-425` — `let n = raw.len().min(31);` でバイト数切り詰め → ハンター名
- `palico.rs:224-226` — `bytes.len().min(NAME_LEN - 1)` → オトモ名
- `myset.rs:107-112 write_name` — `bytes.len().min(dst.len() - 1)` → アイテムセット／装備セット名

**実測**（実セーブに対し「あいうえおかきくけこさ」= 11 文字 33 バイトを書き込み）:

    save.rs  : 31 バイト保存 -> valid UTF-8 = false、読み戻しは "あいうえおかきくけこ" + 置換文字
    palico.rs: 31 バイト保存 -> valid UTF-8 = false
    myset.rs : 31 バイト保存 -> valid UTF-8 = false

**なぜ実害か**: ハンター名のフィールドは 32 バイトで、ゲーム内の名前上限は全角 10 文字（=30 バイト）。
エディタは 11 文字を受け付けてしまい、**壊れた UTF-8 を 1 バイト残す**。読み出し側は
`from_utf8_lossy`（save.rs:367, 371）なので panic しないが、ユーザーには文字化けとして見え、
そのまま書き戻すと保存ファイルに不正なバイト列が残る。
`guildcard::set_comment`(guildcard.rs:180) は `text.chars().take(28)` と **文字単位で正しく切っている**ので、
同じ流儀に揃えればよい。

#### P1-2. `install_official_dlc_quests` が「マスクのビットだけ立って装備が無い」クエストを作る

**根拠**: `dlc.rs:748-768` はバンドルの 40 件の装備を書き、`dlc.rs:800/831` で全クエストのビットを立てる。
しかし装備バンドルには **slot 25 が存在しない**（`OFFICIAL_EQUIPS` の slot = 0..24, 26..40）。
一方 `DLC_CATALOG`(gamedata.rs:603) の position 25 には 1020020 が入っており、バンドルにも 1020020 の本体がある。

**実測**:

    実セーブ  : 装備エントリ [0..24, 26..40] = 40件 ↔ 装備側ビット [0..24, 26..40] = 40ビット（完全に 1:1）
    install_official_dlc_quests 後 : 装備 40件 のまま ↔ ビット [0..40] = 41ビット（bit 25 だけ浮く）
    1020020 に装備データがあるか -> false / クエスト本体はあるか -> true
    実セーブへの適用結果: 175 -> 189 本、変化したビット = [25, 65, 83, 84, 93, 99, 103, 106, 129, 140, 147, 150, 155, 207]

つまり **実セーブには絶対に存在しない組み合わせ（ビットあり／装備なし）をツールが作る**。
ゲーム側の挙動（一覧に出るが装備が読めず落ちる、等）は**未検証＝推測**だが、
「マスクに無い＝ゲームが出さない」という前提でこの機能全体が設計されている以上、ここだけ前提が崩れている。

#### P1-3. `mhnx-core` が既定 feature でコンパイルできない

**根拠**:
- `myset.rs:204` — `crate::gamedata::LAST_ITEM_ID` を無条件参照
- `dlc.rs:157` — 同上
- `dlc.rs:236, 256, 547` — `bit_for_quest` は `#[cfg(feature = "gamedata")]`(dlc.rs:229) なのに無条件呼び出し
- 一方 `lib.rs:45-46` で `pub mod gamedata;` は gamedata feature の下。
- `crates/mhnx-core/Cargo.toml:9` の default は `["std"]` のみ（gamedata は GUI/CLI 側が付ける）。

**実測**:

    cargo check -p mhnx-core                 -> 5 errors (E0433 x2, E0425 x3)
    cargo check -p mhnx-core --no-default-features -> 同じ 5 errors
    cargo test -p mhnx-core -j 1             -> 同じ 5 errors（テストが走らない）
    cargo test --workspace -j 1              -> 205 passed（GUI/CLI の feature 統合で gamedata が入るため）

`crates/mhnx-core/Cargo.toml:11-12` は「game data tables — large, can be disabled」と書いているが、実際は外せない。
コアだけを触る作業ができないので、レビューや修正のコストに直結する。

---

### P2 — 改善

#### P2-1. `to_bytes_compact()` と `arcs\all_event\*` が 4 バイトだけ食い違う

**根拠**: `arc.rs:215-225` は `out[8..12]` を 0 で埋める（新規追加分）。
**実測**:

    romfs   arcs\event\*.arc                : +0x08 = 0xFFFFFFFE (223/223)
    compact arcs\all_event\{playable,dead}  : +0x08 = 0xFFFFFFFE (223/223)
    実セーブにゲームが書いた 175 本          : +0x08 = 0x00000000 (175/175)
    to_bytes_compact() の出力                : +0x08 = 0x00000000
    223 本すべて、サイズ一致・最初の差分は offset 8 のみ

つまり **現在の `to_bytes_compact()` は `arcs\all_event\*` を再現しない**（当時の版は 0xFFFFFFFE を残していた）。
そして `dlc_bundle::arc_bytes` はその旧成果物由来なので、`install_official_dlc_quests` で 189 本入れると
**189 本すべてが +0x08 = 0xFFFFFFFE** で保存される（実測）。セーブ内のクエストは 175/175 が 0。
ゲームが romfs の 0xFFFFFFFE を読んでいる実績があるので**実害は無いと推測**するが、
「セーブに入る形」を名乗る関数と、実際に入るデータが食い違っているのは直すべき。

なお `to_bytes_compact()` 自体のレイアウトは正しい（下の「良い点」参照）。

#### P2-2. バンドル .arc のサイズ検査が無く、将来パニックしうる

`dlc.rs:796, 829, 924` は `dst.fill(0)` のあと `dst[8..8 + arc_data.len()]` を無検査で切る。
同じ関数の装備側(761-765)は `src.len().min(EQUIP_SLOT_SIZE)` と守っているのに非対称。
**実測**: 現在のバンドル最大は 3,859 バイト（上限 5,112）なので今は安全。ただし上限を超える .arc を混ぜた瞬間に
`range end index out of range` でプロセスごと落ちる（GUI なら保存前なので実害は小さいが、CLI では診断不能な失敗になる）。

#### P2-3. `add_quest` だけペイロード検証が無い

`install_quest`(dlc.rs:607-627) は「.arc として読めること」「.mib の ID が一致すること」を検査するが、
`add_quest`(540-593) はサイズとカタログ有無しか見ない。**実測**: `.mib` の ID を 0 に書き換えた .arc を
`install_quest` に渡すと **通ってしまう**（`dlc.rs:618` の `payload_id != 0 &&` のため）。
`check::check_quest` は同じ状態を `mib-id-zero` の **Error** として報告する(check.rs:125-131)ので、検査と書き込みで判定が食い違っている。

#### P2-4. `DLC_CATALOG` に重複 ID があり、ビットが一意に決まらない

**実測**: 重複は 2 件。

    1020001 : positions [0, 41, 42, 43, 44] -> bit 0（41..44 は bit_for_position が None なので無害）
    1110113 : positions [188, 202]           -> bit 207（本来 221 かもしれない）

1110113 は実セーブに入っていない（bit 207 も 221 も 0）ので**どちらが正しいか実測では決められない＝不明**。
`position_for_quest`(dlc.rs:222-226) は先頭一致なので 207 を選ぶ。もしゲームが 221 を見るなら、
`add_quest` で入れた 1110113 は「記録はあるが一覧に出ない」状態になる。

#### P2-5. `BlankCharacter` の公開フィールドがサイズ検証を迂回する

`save.rs:976` の `pub struct BlankCharacter(pub Vec<u8>)` は外から自由に作れる。
`save.rs:996-1010 erase_slot` は `blank.0.len()` をそのまま書き込み量にするので、**実測**: 8 バイトの
`BlankCharacter(vec![0;8])` を渡すと `erase_slot` が `Ok` を返し、スロットを「未使用」にしつつ 8 バイトしか消さない
（中途半端なキャラが残る）。`from_bytes` は検証しているのに、フィールド公開で意味が無くなっている。

#### P2-6. `name_preview` は読むだけで書かれない

`Character.name_preview`(save.rs:908) は `read_character` が埋めるが、`write_character`(save.rs:424-425) は
常に `c.name` を両方のコピーに書く。フィールドを書き換えても無視される。
実セーブでは両者は一致している（実測: slot0/1/2 すべて name == name_preview）ので挙動自体は正しいが、
構造体が「別々に持てる」と嘘をついている。

#### P2-7. 未使用・重複・不整合な定数

- `save.rs:137-141` の `OFF_GUILD_CARD_ID` / `OFF_ARENA_LOG` / `ARENA_LOG_SIZE` は
  `guildcard.rs:14-39` の `OFF_GUILD_CARD + GC_ID` / `OFF_ARENA` と二重定義。
  しかも `ARENA_LOG_SIZE = 342` に対し実データは 17*5*4 = **340**（実測で 85 エントリが読める）。
- `save.rs:881-883` の「3D disabled (1 on Switch, 0 on 3DS)」は実セーブで **0**（実測: disable_3d: 0, circle_pad_pro: 0）。
  読み書きはそのままなので害は無いが、コメントが事実と違う。
- `save.rs:213-223 from_bytes` はサイズ不一致時に常に `want: SWITCH_SIZE` を返すので、
  3DS セーブを 1 バイト間違えた人には「4726188 が正しい」と表示される。

#### P2-8. テストが「通るだけ」になっている箇所

1. `crates/mhnx-core/tests/real_data.rs` — 冒頭(3-4行目)に「参照ファイルが無ければ **失敗ではなくスキップ**」と明記され、
   7 テスト中 6 テストが 21-24 / 68-70 行などで `return` する。クリーンチェックアウトでは**中身が空でも緑**になる。
   同じディレクトリの `quest_injection.rs:15-29` と `regression_review.rs:18-33` は
   「無いなら panic（`MHNX_SKIP_REAL_SAVE` で明示スキップ）」という正しい設計なので、方針がバラバラ。
2. `tests/quest_injection.rs:133-146 adding_a_quest_is_refused_and_writes_nothing` は
   「adding a quest is not supported」と主張するが、`dlc::add_quest`(dlc.rs:540) は追加を実装済み。
   実際に検証できているのは「install_quest は未導入クエストを拒否する」だけ。**テスト名と意図が古い**
   （`hbedit_flow.rs` の ignore 理由「missing: install_into_save only overwrites an installed quest」も同じ古さ）。
3. `to_bytes_compact()` を実データで検証するテストが 1 つも無い。今回の実測（223 本で offset 8 だけ不一致）は
   テストが 1 本あれば即座に検出できた類のもの。
4. `dlc.rs:947-959 arrays_end_at_first_slot` は、`empty_switch_save()` が自分で入れた定数
   （0xB280 と FIRST_SLOT_BASE）同士を比較している。算術の確認にはなるが、実データの裏付けにはならない
   （実際の裏付けは実測で取れている: 0x126474 で厳密一致）。

---

## 良い点（壊してはいけない）

1. **`slot_base()` の一元境界チェック（save.rs:323-337）** が効いている。実測: スロット基底を
   `body.len()-16` に壊すと `read_character` / `read_item_box` / `export_slot` / `write_character` が
   **すべて None / Err を返し panic しない**。0 を入れても同じ。`slot_field()`(727-735) 経由の全アクセサがこの恩恵を受けている。

2. **`section_start()` の DLC セクション検証（dlc.rs:358-369）**。実測: 実セーブで
   `0xB280 + 0x45F4 + 205*0x1400 + 45*0x800 = 0x126474` が slot0 と**厳密に一致**。
   壊れたポインタ（slot0 の後ろ）を入れると全読み書きが拒否される。テストも実データも通っている。

3. **`to_bytes_compact()` のレイアウトが正しい**。実測: セーブにゲームが書いた 175 本のクエストを
   パースして `to_bytes_compact()` すると **175/175 バイト完全一致**。
   最初のペイロードのオフセットも 175/175 が `align32(12 + 80*n)` に一致。
   romfs 223 本でも同じオフセットになる。つまり「セーブに入る形」の定義は実測で裏付けられている。
   圧縮ストリーム再利用（`original`）も安全: `decoded == e.data` のときだけ再利用し、
   編集した 1 エントリだけ再圧縮される（実測: 1 バイト編集で変化は 4 バイト、うち slot 内のみ）。

4. **「開いて保存しても何も変わらない」性質が実測で成立**。実セーブに対して:
   - 3 スロット全部 `read_character` -> `write_character` -> **0 バイト変化**
   - 188 個の全お守りを `set_charm` で書き戻す -> **0 バイト変化**
   - 3 スロット `export_slot` -> `import_slot` -> **0 バイト変化**
   - アイテムボックス `decode` -> `encode` -> **3/3 完全一致**（19bit ストリームが全ビット使い切るため情報落ちが無い）
   - `to_bytes()` == 入力（4,726,188 バイト完全一致）
   これは「ユーザーが開いただけでセーブが書き換わる」事故を防ぐ最重要の性質。

5. **お守りの書き込み規律（charm.rs）**。装備ボックスは武器/防具/お守りの共有配列で、
   書き込みは 5 フィールドだけ（+0x0C..+0x10）。実測: 装備ボックスは
   **お守り 188 / 武器・防具 601 / 空 1211** で、モジュール doc の数値と完全一致。
   武器レコードへの書き込みは `is_talisman` で拒否され、`template_from_box` は
   rarity と +0x02 の組をコピーする（実測: template の rarity=100 / +0x02=0x0A / tail は全て 0）。
   実セーブのギルドカードの外見コピー（+0x19）と色コピー（+0x38）は 3 スロットすべて
   キャラ側と**バイト一致**しており、`sync_guild_card_appearance`(save.rs:464-481) の前提が正しい。

6. **本番コードに `unwrap()` / `expect()` / `panic!` が無い**。
   全ヒットは `#[cfg(test)]` モジュール内（dlc.rs:939+, charm.rs:524+, arc.rs:320+, quest.rs:425+, entries.rs:315+, dlcats.rs:1850+）。
   `#![forbid(unsafe_code)]`(lib.rs:21) も維持されている。

7. **`install_quest` の防御**。「.arc として読めないペイロード」「.mib が無い」「ID 不一致」「サイズ超過」を
   すべて拒否し、拒否時に 1 バイトも書かない（実測: 0xAB×777、64 バイト、途中で切った .arc すべて Err かつ body 不変）。
   実データでの書き戻しは **同一スロットに 4 バイトだけ変化**（+0x08 のヘッダ語のみ）で、他は無傷。

8. **マスクとカタログの対応モデルが実データで成立**。実測: 175 セットビット ↔ 175 レコードで
   「ビットはあるがレコードが無い」= 0 件、「レコードはあるがビットが無い」= 0 件。
   ビット 45..63 にセットは 1 つも無く、`bit_for_position`(dlc.rs:210-218) の
   「45..63 は使わない」という設計と一致。

9. **その他の実測一致**: アイテムボックスは 2242/2295/2295 スロット使用・最大 ID 2972（`LAST_ITEM_ID=2990` と整合）・
   個数は全て 99 以下。オトモは 17/84 体、`level = rec[0x24]+1` が 73 と一致。
   アイテムセット 24 件（先頭 "剣士(汎用)"、16 個）×170 バイトがアイテムボックス直後 0x17CF から隙間なく始まり、
   装備セット 40 件（先頭 "大剣"、部位 [18,40,41,42,43,44,1967]）も 0x208CE で矛盾なし。
   闘技場は 85 エントリで先頭が 57544 = **9:35.44**（guildcard.rs の doc と完全一致）。

---

## 優先して直すべき 3 件

### 1. `add_quest` / `set_installed` に `check_kind(save)?` を入れる（P0）

`dlc.rs:540` の直後と `dlc.rs:255` の直後に 1 行ずつ。あわせて CLI の
`cmd_add_quest`(main.rs:450) に MHGU の明示エラーを出す。
現状「Everything that writes refuses to touch an MHGU save」という doc が嘘になっているので、
doc を直すのではなくコードを doc に合わせるべき（設計意図は正しい）。

### 2. 名前系 3 箇所を文字境界で切る（P1）

`save.rs:420-425` / `palico.rs:224-226` / `myset.rs:107-112` を
「バイト長 .min(31)」から「UTF-8 境界まで戻す」に。`guildcard.rs:180` の
`text.chars().take(n)` 方式に揃えるのが一番簡単。全角 11 文字を入れた瞬間に
保存ファイルが不正 UTF-8 になるのは、セーブエディタとして避けるべき挙動。

### 3. `mhnx-core` 単体ビルドを直す（P1・数分）

`dlc.rs:157` / `myset.rs:204` の `LAST_ITEM_ID` 参照を feature ガードするか、
`LAST_ITEM_ID` を gamedata の外（lib.rs）へ移す。`bit_for_quest` 呼び出し 3 箇所も同様。
これを直すと `cargo test -p mhnx-core` が通り、コアだけを触る作業と CI が成立する。
（次点: P1-2 の「bit 25 だけ立って装備が無い」問題。装備バンドルに slot 25 を足すか、
`DLC_CATALOG` の position 25 を 41..44 と同じプレースホルダ扱いにするかのどちらか。）

---

## 補足: 再現手順

    # 検体（元ファイルは変更していない。SHA256 は開始時と同一）
    .work\review_core\system_base.bin   # jksv_ready\system のコピー

    # 検算プログラム（ワークスペース外の独立クレート。作業ツリーのソースを直接リンク）
    cd .work\review_core\probecrate
    cargo run

書き込み系の検証はすべてメモリ上の `Save` に対して行い、ファイルへは書いていない。
最終確認: `jksv_ready\system` と Ryujinx 実セーブの SHA256 は
`D06B72520E379F462D49098E55C270BA48B203C10A9DBF39A1F8EE7E410C112A` のまま（開始時と一致）。

## 注記

本レビュー中、リポジトリは他担当のエージェントによって並行して編集されていた
（`git status` の差分が 08:05 時点より増えている: hbedit.rs / hbedit_flow.rs / cli main.rs / switch/*）。
**本レポートの行番号は 2026-10-21 08:05〜08:20 時点の作業ツリーに対するもの**で、
指摘そのものは 08:30 時点で再確認済み（`add_quest`(dlc.rs:540) と `set_installed`(dlc.rs:255) に `check_kind` が無いこと、
`dlc.rs:157` / `myset.rs:204` の無条件 `crate::gamedata` 参照、`save.rs:422` / `palico.rs:225` のバイト長切り詰めは、
いずれも現在も未修正）。
