# MHXX コンソール UI（テキスト画面）機能監査

対象: `crates/mhnx-core/src/hbedit.rs`（`Editor` / `Screen` / `input_*` / `lines()`）と、
それを描画する Switch 版 `switch/src/editor.rs`（`title()` / `actions()` / メインループ）。

実セーブ: `C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system`
（読み取り専用。作業コピーは `.work/console_audit/system.base`、4,726,188 B）

**このセーブで確定した前提数値（実測）**

| 項目 | 値 |
|---|---|
| コンテナ | 36 B ヘッダ + 本体 4,726,152 B（Switch） |
| クエスト（DLC セクション） | **175 件インストール済み / 205 スロット / 空き 30** |
| クエスト 1 件の最大サイズ | 5,112 B（`dlc::QUEST_MAX_PAYLOAD`） |
| お守り | 188 個（slot 0 の装備 BOX 内） |
| アイテム BOX | 2,300 スロット |
| オトモ | 17 匹（slot 0） |
| 装備 BOX | 2,000 × 36 B |
| ギルドカード | 4,986 B + アリーナ 17 戦 × 5 ランク |

テストは既存 `crates/mhnx-core/tests/hbedit_flow.rs` に **追記のみ**（先頭 113,128 バイトが
追記前と完全一致することを確認済み）。`hbedit.rs` は 1 行も変更していない。

---

## 0. 結論サマリ

| # | 結論 | 確度 |
|---|---|---|
| 1 | `Screen` の全 24 バリアントは**すべてキー操作で到達できる**（表 4-1）。到達不能な画面は無い | 確定（テスト） |
| 2 | **アイテムの種類変更はできる**（Items → R → ItemPick → A）。個数は ←→、刻みは X、戻すは L | 確定（テスト） |
| 3 | **お守りは新規・編集・削除・スキル名選択までできる**（Slots → L → Charms → Y/L/A、スキル行で Y） | 確定（テスト） |
| 4 | **クエスト編集（セーブ内の既存クエスト）はできる**（SaveQuests → A → Mib/Gmd → X で書き戻し） | 確定（テスト） |
| 5 | **クエスト追加は UI からできない**。`install_into_save()` は `dlc::install_quest` 固定で、これは「既にあるクエストの上書き」専用。`dlc::add_quest`（新規追加）は実装済みなのに `Editor` から一度も呼ばれない | 確定（コード + テスト） |
| 6 | **クエスト削除もできない**。`dlc::uninstall_quest` / `dlc::set_installed` も未配線 | 確定（テスト） |
| 7 | **ギルドカードは表示専用**。`input_guild()` は B と + だけ。`guildcard.rs` に書き込み API 自体が無い | 確定（テスト） |
| 8 | **オトモ（Roster）は表示専用**。`input_roster()` は Up/Down/B/+ だけ。`palico::set_name/set_exp/set_level/set_enthusiasm` は未使用 | 確定（テスト） |
| 9 | 狩猟記録・マイセット・装備 BOX・クエスト装備も**表示専用**（Up/Down でカーソルが動くので編集できそうに見える） | 確定（テスト） |
| 10 | **最大の不具合はボタン案内**。`switch/src/editor.rs` の `title()`/`actions()` が古い `Charm` を綴っているため `Charm =>` が**全マッチの束縛パターン**になり、**17 画面でタイトルが「お守り」・チップがお守りの 4 キー**になる | 確定（実測コンパイル） |
| 11 | `EntryView` の **X（.arc 保存）は死んでいる**。画面下の案内にもチップにも「X .arc 保存」と出るのに、`input()` に X の腕が無い | 確定（テスト） |
| 12 | 案内の**抜け**も多い（Charm の L、ArcEntries の -/R、SkillPick と Mib の X、Palico の L/R、CharmEdit の Up/Down/X）。`lines()` 側のキー行も Items/ArcList/Mib には存在しない | 確定（テスト） |

> ユーザーの「アイテム編集・お守り編集・クエスト追加・クエスト編集が一切ない」という不満は、
> **機能が無いのではなく「案内が嘘だから操作に辿り着けない」** ケースが大半。
> たとえばクエスト一覧のチップは「A 削除」と表示するが、実際の A は**クエストを開く**。
> お守り編集画面のチップは「A 削除 / X 保存」だが、実際は **A = 書き込み / X = 刻み**。
> ギルドカード・オトモ・狩猟記録・マイセットでは、お守りのチップ（Y 新規/編集・A 削除・X 保存）が
> そのまま表示されるが、どのキーも何もしない。

---

## 1. 方法

- 既存の統合テスト `crates/mhnx-core/tests/hbedit_flow.rs` に **20 本を追記**（既存 103 本は無変更）。
- `Editor` を直接操作し、`ed.input(Key::X)` で遷移と値の変化を検証。実セーブは読み取りのみ。
- Switch 版 `actions()`/`title()` は *Nintendo SDK が無いとビルドできない* ため、
  **ソースからその 2 関数だけを切り出して `mhnx-core` に対してコンパイル**し、
  実際に表示されるタイトルとチップを実測（`.work/console_audit/hints_probe`）。
- 実行コマンド:

```powershell
cargo test -p mhnx-core --features std,gamedata --test hbedit_flow
# => test result: ok. 120 passed; 0 failed; 3 ignored

cargo test -p mhnx-core --features std,gamedata --test hbedit_flow -- --ignored
# => 3 failed（未実装機能の spec テスト。下記 7 章）
```

テストの 3 分類:

| 接頭辞 | 意味 | 現在 |
|---|---|---|
| `audit_*` | できている機能の固定（回帰防止） | 4 本 緑 |
| `gap_*` | **今の欠落・誤動作の記録**。直すと落ちるので、直した人は test を `audit_*` に書き換える | 13 本 緑 |
| `spec_*` | あるべき姿。`#[ignore]` 中で、`--ignored` で回すと失敗＝未実装リスト | 3 本 失敗 |

---

## 2. 要求 8 項目への直答

| # | 要求 | 判定 | 実際の操作・根拠 |
|---|---|---|---|
| 1 | 各画面に到達できるか | **○ 24/24** | 表 4-1。`audit_every_screen_is_reachable_by_keys` |
| 2 | アイテムの種類を変えられるか | **○** | Items で **R** → ItemPick（Y 名前絞り / X クリア / ←→ ±10）→ **A** 決定。個数は ←→、X 刻み、L で 1 手戻し、A でセーブ書き込み。`audit_the_item_type_can_be_changed_through_the_picker` |
| 3 | お守りを編集できるか | **○** | Slots **L** → Charms（**Y** 編集 / **L** 新規 / **A** 削除 / **X** 保存）→ CharmEdit（↑↓ 行 / ←→ 値 / **Y** スキル選択 / **X** 刻み / **A** 書き込み）→ SkillPick。`audit_a_charm_can_be_edited_through_the_skill_picker` |
| 4 | クエストを追加できるか | **× 不可** | ArcList → A で .arc を開き ArcEntries で **X** を押しても `dlc::install_quest` が「このセーブにはまだ無い」で拒否する。`dlc::add_quest` は未配線。`gap_a_quest_the_save_does_not_have_cannot_be_added` |
| 5 | クエストを編集できるか | **○（既存分のみ）** | ArcList **X** → SaveQuests → **A** → ArcEntries → **A**（.mib/.gmd）→ 編集 → ArcEntries で **X** で同じスロットへ書き戻し。`audit_a_quest_inside_the_save_can_be_edited_and_written_back` |
| 6 | ギルドカードを編集できるか | **× 表示のみ** | Guild は **B だけ**。ランク・コメント・アリーナタイムを表示するが書き込み API 無し。`gap_the_guild_card_is_display_only` |
| 7 | オトモを編集できるか | **× 表示のみ** | Roster は **B だけ**（↑↓ でカーソルは動く）。`gap_the_palico_roster_is_display_only` |
| 8 | 案内と実キーは一致するか | **× 17 画面で不一致** | 5 章。`gap_the_switch_hint_table_uses_a_catch_all_arm` |

参考（今回の監査で追加で分かった可否）:

| 機能 | 判定 | 備考 |
|---|---|---|
| 配信オトモの追加/削除 | ○ | Slots **R** → Palico（**A** 追加/削除、L/R 言語、X 保存） |
| 設定（明るさ・振動・3D・サークルパッド） | ○ | SaveList **X** → Options |
| 3DS ⇔ Switch 変換 | ○ | SaveList **L** → Convert（A 変換 / Y 名前） |
| クエスト検査と一括修正 | ○（1 種のみ） | ArcEntries **L** → Check。A は **one-player-only 固定**（fixable が増えると誤爆する構造） |
| クエスト書き出し | ○ | ArcEntries **-**（1 件）/ **R**（全部）、SaveQuests も同じ |
| 狩猟記録の編集 | × | 表示のみ（`Save::set_hunt_count` 等は存在） |
| マイセットの編集 | × | 表示のみ（`myset::set_*` は存在） |
| 装備 BOX の編集 | × | 表示のみ（書き込み API 自体が無い） |
| クエスト装備（45 枠）の編集 | × | 表示のみ（`dlc::set_quest_equipment_item` は存在） |
| お守りの検索・絞り込み | × | 188 件を ↑↓ だけで辿る（アイテム/スキルには名前絞り込みがあるのに） |
| アイテム BOX の名前表示 | × | 行は `#1234 id 5678 x3` で **名前が出ない**（`item_name_of` はピッカーと報酬ビューだけ） |

---

## 3. 画面到達表（全 24 画面）

<a id="table-4-1"></a>

| # | 画面 | 到達キー | 実装 | B の戻り先 | 備考 |
|---|---|---|---|---|---|
| 1 | SaveList | 起動時 | hbedit.rs:1104 | — | ルート。A で I/O に `ReadSave` を要求 |
| 2 | Slots | SaveList **A**（I/O 応答後） | :1133 | SaveList | セーブ読込の唯一の窓口 |
| 3 | Char | Slots **A** | :1180 | Slots | L/R でスロット切替（案内に無い） |
| 4 | Items | Slots **X** | :1222 | Slots | キー案内行が無い |
| 5 | ItemPick | Items **R** | :1659 | Items | 空き枠は Y で表示してから R |
| 6 | Charms | Slots **L** | :715 / :789 | Slots | お守り |
| 7 | CharmEdit | Charms **Y**（既存）/ **L**（新規） | :731 / :830 | Charms | 初期カーソルはスキル 1 行 |
| 8 | SkillPick | CharmEdit のスキル行（2 or 4）で **Y** | :1617 | CharmEdit | それ以外の行ではメッセージだけ |
| 9 | Equip | Slots **-** | :1569 | Slots | Y 絞る / X クリア / L ギルド / R オトモ |
| 10 | Guild | Equip **L** | :1511 | **Slots**（Equip ではない） | 表示専用 |
| 11 | Roster | Equip **R** | :1522 | **Slots**（Equip ではない） | 表示専用 |
| 12 | Hunts | Char **-** | :1493 | **Slots**（Char ではない） | 表示専用 |
| 13 | Sets | Char **Y** | :1199 | Slots | 表示専用 |
| 14 | Palico | Slots **R** | :901 | Slots | 配信オトモ。L/R 言語 |
| 15 | Convert | SaveList **L** | :1601 | SaveList | |
| 16 | Options | SaveList **X** | :963 | SaveList | |
| 17 | ArcList | SaveList **Y** / Slots **Y** | :1721 | **B が無い**（Y で SaveList） | .arc 一覧 |
| 18 | ArcEntries | ArcList **A** / SaveQuests **A** | :1902 | ArcList | .arc の中身 |
| 19 | EntryView | ArcEntries **Y** | :668-701 | ArcEntries | X が死んでいる |
| 20 | Mib | ArcEntries **A**（.mib 行） | :1969 | ArcEntries | |
| 21 | Gmd | ArcEntries **A**（.gmd 行） | :2026 | ArcEntries | |
| 22 | Check | ArcEntries **L** | :1007 | ArcEntries | |
| 23 | SaveQuests | ArcList **X**（要セーブ） | :1854 | ArcList | セーブ内クエスト一覧 |
| 24 | QuestEquip | SaveQuests **L** | :1551 | SaveQuests | 表示専用 |

**遷移で分かりにくい点**

- **お守りは Slots の L**、**オトモ（Roster）は Equip の R**、**狩猟記録は Char の -**、**ギルドカードは Equip の L**。
  同じ「お守り」でも Slots のチップは L なのでまだ良いが、**Slots の R は「オトモ」と表示して配信オトモを開く**（実体が違う）。
- **Guild / Roster / Hunts の B は元の画面ではなく Slots に戻る**（Equip や Char に戻れない）。
- **ArcList には B が無い**。戻るには「Y セーブ編集」を押すしかない（他の全画面は B で戻る）。

---

## 4. ボタン案内（`actions()` / `title()`）と実キーの突き合わせ

### 4.1 実測: いま画面に出ているタイトルとチップ

`switch/src/editor.rs` の `title()`（399-427 行）と `actions()`（429-504 行）を
**そのまま切り出してコンパイル**した結果（`.work/console_audit/hints_probe`）。

| 画面 | 実測タイトル | 実測チップ |
|---|---|---|
| SaveList | セーブを開く | A 開く / Y クエスト / L 変換 / X 設定 / - 再読込 / + 終了 |
| Slots | キャラクター選択 | A キャラ / X アイテム / - 装備 / L お守り / R オトモ / Y クエスト / B 戻る |
| Char | キャラクター編集 | A 保存 / </> 値を変更 / X 刻み / - 狩猟記録 / Y マイセット / B 戻る |
| Items | アイテムボックス | R 種類を変える / </> 個数 / X 刻み / Y 空き表示 / L 元に戻す / A 保存 / B 戻る |
| ItemPick | アイテムを選ぶ | A 決定 / Y 名前で絞る / X クリア / B 戻る |
| Convert | 3DS <-> Switch 変換 | A 変換して保存 / Y 名前 / B 戻る |
| Charms | お守り | Y 新規/編集 / A 削除 / X 保存 / B 戻る |
| **残り 17 画面** | **お守り** | **Y 新規/編集 / A 削除 / X 保存 / B 戻る** |

17 画面 = CharmEdit, SkillPick, Equip, QuestEquip, SaveQuests, ArcList, ArcEntries, EntryView,
Mib, Gmd, Check, Hunts, Guild, Roster, Sets, Palico, Options。

### 4.2 原因: `Charm =>` が「全マッチ」の束縛パターンになっている

`Screen` のバリアントは **`Charms`**（hbedit.rs:58）だが、Switch 版は **`Charm`** と書いている
（`title()` 408 行、`actions()` 472 行）。`Charm` という名前はどこからも import されていないので、
これは**バリアント参照ではなく単なる変数束縛**になり、**すべての値にマッチ**する。
それより下の腕はすべて到達不能になる。

実際に同じ 2 関数をコンパイルすると、次の警告が 17 個出る（抜粋・行番号は切り出し後）:

```
warning: unreachable pattern
  --> src/main.rs:13:9
   |
12 |         Charm => "お守り",
   |         ----- matches any value
13 |         CharmEdit => "お守り編集",
   |         ^^^^^^^^^ no value can reach this

warning: unused variable: `Charm`
  --> src/main.rs:12:9
   |
12 |         Charm => "お守り",
   |         ^^^^^
help: you might have meant to pattern match on the similarly named variant `Char`
```

**コンパイルは通ってしまう**（警告のみ）ため、この不整合はこれまで表に出ていない。
帰結:

- ArcList / SaveQuests / ArcEntries / Mib / Gmd / Check / EntryView / Equip / Palico / Options などで
  **見出しが「お守り」**、**フッタのチップがお守りの 4 キー**になる。
- そのためユーザーは「クエスト一覧で A を押すと削除される」「お守り編集で X が保存」と誤解する。
  実際は **A = 開く / X = 刻み**（お守り編集）で、**A = 開く / X = セーブへインストール**（クエスト一覧）。
- ギルドカード・オトモ・狩猟記録・マイセットでは「Y 新規/編集・A 削除・X 保存」が出るが、
  どれを押しても何も起きない → **「編集機能が無い」と見える最大の理由**。

### 4.3 案内漏れ（効くのにチップに無いキー）

| 画面 | 抜けているキー | 実際の動作 | 根拠 |
|---|---|---|---|
| Charms | **L** | 空き枠に新規お守りを作る | :819 / `gap_the_hint_table_omits_keys_that_work` |
| CharmEdit | **↑↓ / X** | 行移動 / 刻み | :836-864 |
| SkillPick | **X** | 絞り込み解除 | :1633 |
| Mib | **X** | 刻み切替 | :1981 |
| Palico | **L / R** | 日本語/英語名の切替 | :914 |
| ArcEntries | **- / R** | 1 件書き出し / 全件書き出し | :1943 / :1957 |
| ItemPick / SkillPick | **← →** | ±10 行 | :1664 / :1624（`lines()` の行には有る） |
| Slots | **L / R / - / Y** | お守り / 配信オトモ / 装備 / クエスト | `lines()` のフッタは「Up/Down slot A char edit X items」だけ |

さらに **`lines()` 側のキー行が存在しない画面**がある: Items（:2181-2218）、ArcList（:2460-2473）、
Mib（:2506-2542）。チップが壊れている現状では、これらは**キー案内がゼロ**になる。

### 4.4 死んでいるキー

| 画面 | キー | 表示 | 実態 |
|---|---|---|---|
| EntryView | **X** | チップ「X .arc 保存」+ 画面下「A change item  X write .arc  B back」 | `input()` の EntryView に X の腕が無い（hbedit.rs:668-701）。何も起きず、メッセージすら出ない。`gap_the_entry_view_advertises_a_dead_x_key` |
| SaveList | **-** | チップ「- 再読込」 | core には無いが、Switch のメインループ（editor.rs:741）が再スキャンするので**実機では生きている**。PC/テストからは再現できない |
| ArcList | **B** | チップに無い | 他画面の習慣どおり押しても何も起きない。`gap_the_arc_list_has_no_back_key` |

### 4.5 ラベルの意味ズレ

| 画面 | チップ | 実際 |
|---|---|---|
| Slots | **R オトモ** | **配信オトモ**（`Screen::Palico`）。本当のオトモ一覧は Equip → R |
| ArcEntries | **X 保存** | `.arc` の保存ではなく **セーブへのインストール**（`install_into_save`）。`.arc` を書くのは Mib の A / Gmd の X / Check の X のみ |
| SaveQuests | **- 書き出す / R 全部** | 正しい（`ExtractQuests`）。ただし**追加・削除のキーは無い** |
| ArcEntries | **A 編集** | .mib/.gmd 以外のエントリでは「this entry type cannot be edited」 |

---

## 5. 不足している機能（優先度順）

| 優先 | 不足 | 影響 | 既にある部品 |
|---|---|---|---|
| **P0** | `Charm` → `Charms` の修正（17 画面の案内が嘘） | 全クエスト画面・各表示画面で操作不能に見える | 1 行修正 ×2 |
| **P0** | EntryView の X を実装 or 表示から消す | 「保存したのに何も起きない」 | `request_write_arc()` |
| **P1** | **クエスト追加（.arc → セーブ）** | ユーザー要望の中心。現状は既存クエストの上書きのみ | `dlc::add_quest`（テスト済み・実セーブで成功確認） |
| **P1** | クエスト削除 | 配信クエストを消せない | `dlc::uninstall_quest` |
| **P1** | クエストの配信フラグ切替 | 一覧に出る/出ないを制御できない | `dlc::set_installed` |
| **P2** | ギルドカード編集（コメント 28 字・HR・アリーナタイム） | 表示のみ | `guildcard` の読み取り定数のみ。**書き込み API が無い** |
| **P2** | オトモ編集（名前・Lv・経験値・なつき） | 表示のみ | `palico::set_name/set_exp/set_level/set_enthusiasm` |
| **P2** | 狩猟記録編集（討伐/捕獲/サイズ） | 表示のみ | `Save::set_hunt_count/set_capture_count/set_monster_size` |
| **P2** | マイセット編集（名前・中身） | 表示のみ | `myset::set_item_set_name/set_item_set_entry/set_equip_set_name` |
| **P2** | クエスト装備（45 枠）編集 | 表示のみ | `dlc::set_quest_equipment_item` |
| **P3** | 装備 BOX 編集 | 表示のみ | `Save::equipment_box_mut` のみ（**エンコーダが無い**） |
| **P3** | 一覧のページ送り（175 クエスト / 2,300 アイテム / 188 お守り / 84 オトモ） | ↑↓ だけで現実的でない | 長押しリピートは実装済み（editor.rs:758-763） |
| **P3** | アイテム BOX に名前を出す | ID だけでは探せない | `item_name_of` |
| **P3** | ArcList の B | 戻り方が他画面と違う | — |
| **P3** | 戻り先の記憶（Guild→Equip、Hunts→Char、Roster→Equip） | 文脈を失う | — |
| **P3** | Items/ArcList/Mib のキー案内行 | 案内ゼロ | — |
| **P3** | Check の A を「選択した指摘」に紐付ける | 将来 fixable が増えると誤爆 | `Finding.code` |

---

## 6. 実装提案

### 6.1 P0: 案内テーブルを core に移し、コンパイラに網羅性を担保させる

いまの `actions()`/`title()` は Switch クレート内の private 関数なので、
**core 側のテストから呼べず、名前の変更にも気付けない**。これが今回のバグの温床。

`crates/mhnx-core/src/hbedit.rs` に移動する（`Screen` と同じファイル＝同じ crate なので、
新しいバリアントを足すと **match の網羅性エラーで必ず落ちる**）:

```rust
/// Header title for a screen.
pub fn screen_title(screen: Screen) -> &'static str {
    match screen {  // ワイルドカード禁止
        Screen::SaveList => "セーブを開く",
        ...
        Screen::Charms => "お守り",
        ...
    }
}

/// (key, label) chips for the footer. Keep in sync with the input handlers;
/// the audit tests drive every chip and fail if it does nothing.
pub fn screen_actions(screen: Screen) -> &'static [(&'static str, &'static str)] {
    match screen {
        Screen::SaveList => &[("A", "開く"), ("Y", "クエスト"), ("L", "変換"), ("X", "設定"), ("-", "再読込")],
        ...
        Screen::Charms => &[("Y", "新規/編集"), ("A", "削除"), ("L", "新規作成"), ("X", "保存"), ("B", "戻る")],
        ...
    }
}
```

Switch 側は `use mhnx_core::hbedit::{screen_actions, screen_title};` に置き換えるだけ。
PC GUI でも同じチップを出せるようになる。
（`+` はメインループが先に拾って終了するので、チップからは外すのが正しい。）

### 6.2 P0: EntryView の X

`hbedit.rs:668-701` の EntryView の match に 1 行足す:

```rust
Key::X => self.request_write_arc(),
```

（`request_write_arc()` は arc_path が無いときに「press X on the entry list to write this quest back into the save」と
案内するので、セーブから開いたクエストでも誤解を生まない。）

### 6.3 P1: クエスト追加 — `install_into_save()` を「更新 or 追加」に

`install_into_save()`（hbedit.rs:2055-2091）の `dlc::install_quest(s, id, &raw)` を分岐にする:

```rust
let id_i = id as i32;
let exists = dlc::find_quest(s, id_i).is_some();
let result = if exists {
    dlc::install_quest(s, id_i, &raw)
} else {
    dlc::add_quest(s, id_i, &raw)   // 空きスロットを探してカタログ bit も立てる
};
match result {
    Ok(slot) => {
        self.dirty = true;
        self.message = if exists {
            format!("updated quest {} in slot {}", id, slot)
        } else {
            format!("added quest {} to slot {} (new)", id, slot)
        };
    }
    Err(e) => self.message = format!("install failed: {}", e),
}
```

実測では、このセーブに無い **1010002**（カタログ bit 65）を `add_quest` すると
**slot 135 に入り 175 → 176 件**になることを確認済み（`gap_a_quest_the_save_does_not_have_cannot_be_added`）。
ただしカタログに無い ID と 5,112 B 超は拒否される（既存テストどおり）。

- 追加先を選ばせたい場合は、`SaveQuests` に「追加メニュー」を足す（6.4）。
- `add_quest` は「未検証」と doc に明記されているので、UI には
  「実験的: バックアップを取ってください」を併記する。

### 6.4 P1: クエスト削除・フラグ・追加を 1 つのメニューに

`SaveQuests` は全キーが埋まっている（A 編集 / B 戻る / X 絞り解除 / Y 絞り / L 装備 / R 全書き出し / - 1 件書き出し）。
**← → だけが空いている**ので、ここを「アクション」に使う案:

- `X` を「絞り込み解除」から **「クエスト操作メニュー」** に変更（解除はメニュー内 or Y 入力で空文字）。
  メニュー項目: 「このクエストを削除」「配信フラグを切る」「.arc から追加」「絞り込み解除」。
- 追加は「.arc 一覧 → 選ぶ」で、`install_into_save()` を呼ぶだけ（6.3 の分岐が効く）。
- 削除は `dlc::uninstall_quest`。**カタログ bit は消せない**（doc コメント参照）ので、
  「一覧に残るかもしれません」とメッセージに出す。

### 6.5 P2: ギルドカード編集

`guildcard.rs` に書き込みを追加してから画面を作る:

```rust
pub fn set_comment(save: &mut Save, slot: usize, text: &str) -> bool; // GC_COMMENT, 28 文字
pub fn set_arena_time(save: &mut Save, slot: usize, arena: usize, rank: usize, cs: u16) -> bool;
```

画面は `Screen::GuildEdit`（`CharmEdit` と同じ「行リスト + ←→ で値」）。
コメントは `TEXT_GUILD_COMMENT` sentinel を足してソフトキーボードへ（既存の `TEXT_EQUIP_FILTER` と同じ経路）。
HR はキャラ側の値と二重管理になるので、まずは**表示のみ**にしてコメントとアリーナタイムから。

### 6.6 P2: オトモ編集

`Screen::PalicoEdit`（`CharmEdit` のコピー）で 4 行:
名前（`TEXT_PALICO_NAME`）/ Lv（`set_level`）/ 経験値（`set_exp`）/ なつき（`set_enthusiasm`）。
Roster の **A** で開く。保存は **X**。

### 6.7 P2: 狩猟記録・マイセット・クエスト装備

- Hunts: 選択行で **← →** が討伐数、**Y** で列（討伐/捕獲/最小/最大）切替、**A** で保存。
  `Save::set_hunt_count/set_capture_count/set_monster_size` をそのまま呼ぶ。
- Sets: **A** で選択セットの編集（名前はソフトキーボード → `myset::set_*_name`、中身は一覧）。
- QuestEquip: **A** で `ItemPick`（`item_pick_mode = 2` を追加）→ `dlc::set_quest_equipment_item`。

### 6.8 P3: 一覧の使い勝手

- SaveQuests / Charms / Hunts / Roster / ArcList の **← → を ±10 行**に（SkillPick/ItemPick と同じ）。
  Items は ←→ が個数なので、**長押しリピート**（既に main loop にある）を前提にする。
- Items の行に `item_name_of(s.id)` を足す（:2203）。コンソール幅に合わせて 12 文字で切る。
- ArcList に **B → SaveList**。
- Guild/Roster/Hunts に `return_to: Screen` を持たせ、B で元の画面へ。
- Items / ArcList / Mib にも `lines()` のキー行を追加（他画面と同じ体裁）。

### 6.9 再発防止

- `screen_actions()` を core に置く（6.1）。
- `hbedit_flow.rs` の `gap_the_switch_hint_table_uses_a_catch_all_arm` は
  **修正すると落ちる**ように書いてある。直したらこのテストを消し、
  `spec_every_screen_has_its_own_hint_chips`（`#[ignore]`）の `#[ignore]` を外して緑にすること。
- 「チップに書いたキーは必ず何かする」を機械的に検査するテストを足す
  （各画面の代表状態でチップのキーを押し、`screen/message/dirty/request/write/text` のどれかが動くことを確認）。

---

## 7. 追加したテスト（20 本）

`crates/mhnx-core/tests/hbedit_flow.rs` の末尾に追記。既存 103 本は無変更。

| # | テスト名 | 種別 | 何を固定するか | 結果 |
|---|---|---|---|---|
| 1 | `audit_every_screen_is_reachable_by_keys` | audit | 24 画面すべてがキー操作で到達できること | ok |
| 2 | `audit_the_item_type_can_be_changed_through_the_picker` | audit | Items→R→ItemPick→A で ID が変わり、A で保存され、L で戻せること | ok |
| 3 | `audit_a_charm_can_be_edited_through_the_skill_picker` | audit | Slots L→Charms Y→CharmEdit（↑↓/←→/Y）→SkillPick→A 書き込み→X 保存 | ok |
| 4 | `audit_a_quest_inside_the_save_can_be_edited_and_written_back` | audit | SaveQuests A→Mib←→→X で同じスロットへ書き戻り、payload が repack と一致 | ok |
| 5 | `gap_a_quest_the_save_does_not_have_cannot_be_added` | gap | 未所持クエストは X で拒否。`dlc::add_quest` なら成功（配線だけが欠落） | ok |
| 6 | `gap_the_guild_card_is_display_only` | gap | Guild の A/X/Y/L/R/←→ がセーブを 1 バイトも動かさない | ok |
| 7 | `gap_the_palico_roster_is_display_only` | gap | Roster も同様（↑↓ だけ動く） | ok |
| 8 | `gap_the_hunt_records_are_display_only` | gap | Hunts も同様 | ok |
| 9 | `gap_the_quest_equipment_list_is_display_only` | gap | QuestEquip も同様 | ok |
| 10 | `gap_the_my_sets_screen_is_display_only` | gap | Sets も同様 | ok |
| 11 | `gap_the_equipment_box_is_display_only` | gap | Equip の A/←→/↑↓ が何もしない | ok |
| 12 | `gap_the_entry_view_advertises_a_dead_x_key` | gap | 画面が「X write .arc」と出すのに X が無反応 | ok |
| 13 | `gap_the_arc_list_has_no_back_key` | gap | ArcList の B が効かない | ok |
| 14 | `gap_the_slots_screen_calls_the_download_palico_list_the_roster` | gap | Slots の R は配信オトモ。Roster は Equip R | ok |
| 15 | `gap_the_switch_hint_table_uses_a_catch_all_arm` | gap | `Charm` が束縛パターンで 17 腕が到達不能 | ok |
| 16 | `gap_the_hint_table_omits_keys_that_work` | gap | 効くのに載っていないキー（L / - / R / X / L/R） | ok |
| 17 | `gap_the_quest_list_cannot_add_or_remove_a_quest` | gap | SaveQuests でどのキーを押しても件数とバイト列が変わらない | ok |
| 18 | `spec_a_quest_can_be_added_from_an_arc` | spec | 未所持クエストの追加（**未実装**） | ignored→失敗 |
| 19 | `spec_the_guild_card_can_be_edited` | spec | ギルドカード編集（**未実装**） | ignored→失敗 |
| 20 | `spec_every_screen_has_its_own_hint_chips` | spec | 全画面に正しいチップ（**未実装**） | ignored→失敗 |

`--ignored` 実行時の失敗メッセージ:

```
spec_a_quest_can_be_added_from_an_arc ... FAILED
  X on an .arc must add a quest the save does not have yet
spec_the_guild_card_can_be_edited ... FAILED
  A on the guild card must open an editor (comment / rank / arena log)
spec_every_screen_has_its_own_hint_chips ... FAILED
  actions() has an arm that is not a Screen variant: Charm (a bare identifier is a catch-all)
```

---

## 8. 再現手順

```powershell
# 1) テスト（既存 + 追記 20 本）
cd C:\anyWorkspace\nx\mhxx\mhnxeditor
cargo test -p mhnx-core --features std,gamedata --test hbedit_flow
#    -> ok. 120 passed; 0 failed; 3 ignored

# 2) 未実装リスト（spec テストだけ回す）
cargo test -p mhnx-core --features std,gamedata --test hbedit_flow -- --ignored
#    -> 3 failed

# 3) 実機が出しているチップの実測（Switch 版の title/actions を切り出して実行）
cd .work\console_audit\hints_probe
cargo run --quiet 2>$null
```

補足:

- Switch クレートは Nintendo SDK が必要で、この環境ではビルドできない。
  そのため案内テーブルは **ソースから 2 関数だけ抽出して**検証した（4.1 / 4.2）。
  抽出元はビルド対象と同一ファイル・同一テキスト。
- PC 版（`crates/mhnx-gui`）は egui の独自 UI で、このテキスト画面は使っていない
  （`hbedit` からは `item_name_of` と `quest_id_of_arc` だけ利用）。
  本レポートの対象は **共通状態機械 + Switch のテキスト描画**。
- 実セーブは読み取りのみ。作業コピー `.work/console_audit/system.base` と、
  追記前のテストファイル `.work/console_audit/hbedit_flow.rs.orig` を残してある。

---

## 付録 A: 実測チップ生成に使ったプローブ（probe）のビルド警告（要約）

```
warning: unreachable pattern  ... CharmEdit / SkillPick / Equip / ... （17 個）
warning: unused variable: `Charm`
```

＝ **ビルドは成功する**。だから今まで気付かれていない。

## 付録 B: 変更ファイル

| ファイル | 変更 |
|---|---|
| `crates/mhnx-core/src/hbedit.rs` | なし |
| `switch/src/editor.rs` | なし（提案のみ） |
| `crates/mhnx-core/tests/hbedit_flow.rs` | **追記のみ**（+35,242 バイト / 20 テスト） |
| `.work/console_audit/system.base` | 実セーブの作業コピー（4,726,188 B） |
| `.work/console_audit/hints_probe/` | 案内テーブル実測用の一時クレート |

## 付録 C: 監査で使った主要な行番号

| 内容 | 位置 |
|---|---|
| `Screen` 定義（`Charms` は :58） | hbedit.rs:36-85 |
| `input()` ディスパッチ | hbedit.rs:644-712 |
| EntryView（X の腕が無い） | hbedit.rs:668-701 |
| お守り一覧 / 編集フォーム | hbedit.rs:789-899 |
| Items / ItemPick | hbedit.rs:1222-1273 / 1659-1719 |
| SkillPick | hbedit.rs:1617-1657 |
| ArcList / SaveQuests / ArcEntries | hbedit.rs:1721-1967 |
| Mib / Gmd / Check | hbedit.rs:1969-2052 / 1007-1056 |
| `install_into_save()` | hbedit.rs:2055-2091 |
| Guild / Roster / Hunts / QuestEquip | hbedit.rs:1511 / 1522 / 1493 / 1551 |
| `lines()` | hbedit.rs:2095-3108 |
| `title()` / `actions()`（`Charm` は :408 / :472） | switch/src/editor.rs:399-504 |
| チップ描画 | switch/src/editor.rs:384-391 |
| Minus の再スキャン | switch/src/editor.rs:741-750 |
