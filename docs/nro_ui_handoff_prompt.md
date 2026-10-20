# NRO（Switch 実機）UI 改修の引き継ぎプロンプト

このファイルは、**別の AI / 開発者に Switch 実機版の UI を作り直してもらう**ときに
そのまま渡すためのものです。下の「--- ここから ---」以降をコピーしてください。

--- ここから ---

あなたは Nintendo Switch の homebrew（.nro）の UI を担当します。
セーブを読む・書く・解析する処理は**すべて完成済み**なので、**画面の描画と操作感だけ**を改善してください。

## プロジェクト

```
C:\anyWorkspace\nx\mhxx\mhnxeditor
```

ゲーム「モンスターハンターダブルクロス（MHXX）」のセーブエディタです。
PC 用 GUI（egui）と Switch 用 .nro の 2 つがあり、**今回は .nro 側だけ**が対象です。

## 環境（重要）

| 項目 | パス / 値 |
|---|---|
| devkitPro | `C:\devkitPro` |
| **libnx** | `C:\devkitPro\libnx`（ヘッダは `C:\devkitPro\libnx\include\switch\`） |
| Rust | nightly 1.101.0 |
| ターゲット | `aarch64-nintendo-switch-libnx`（rustup 済み） |
| ビルド | `cd <プロジェクト>; .\switch\build.ps1` |
| 出力 | `dist\mhnxeditor.nro` |
| 実行環境 | Atmosphère 1.12.1 / ファーム 23.0.1 / hbmenu |

`no_std` + `alloc` です。**std はありません。** `println!` も `String` の一部機能も使えません。
（`alloc::string::String` / `alloc::vec::Vec` は使えます）

## ファイル構成と役割

### UI（改修対象）

| ファイル | 行数 | 役割 |
|---|---|---|
| `switch/src/editor.rs` | 773行 | **★ 画面の描画と入力処理。主戦場はここ** |
| `switch/src/text.rs` | 793行 | テキスト描画エンジン（自前。egui ではない） |
| `switch/src/app.rs` | 8KB | メインループ、パッド入力の取得 |
| `switch/src/swkbd.rs` | 402行 | ソフトウェアキーボード（文字入力） |

### ロジック（触らなくてよい）

| ファイル | 行数 | 役割 |
|---|---|---|
| `crates/mhnx-core/src/hbedit.rs` | 3,467行 | **画面の状態機械**。`Screen` enum、`Editor` 構造体、`input_*()`、`lines()` |
| `crates/mhnx-core/src/save.rs` | 992行 | セーブの読み書き |
| `crates/mhnx-core/src/dlc.rs` | 868行 | クエストの追加・上書き・削除 |
| `crates/mhnx-core/src/charm.rs` | 557行 | お守り |
| `crates/mhnx-core/src/guildcard.rs` | 小 | ギルドカード・闘技記録 |
| `crates/mhnx-core/src/palico.rs` | 小 | オトモ |
| `crates/mhnx-core/src/myset.rs` | 小 | マイセット |
| `switch/src/savedata.rs` | 6.8KB | 本体セーブのマウント（NAND 直編集） |
| `switch/src/sys.rs` | 10KB | libnx の FFI 宣言（`extern "C"`） |

## 現在の UI の作り

### 描画は完全に自前

`text.rs` の `Canvas` に用意されているのはこの 6 つだけです。

```rust
impl Canvas {
    pub fn width(&self) -> i32;              // 画面幅（1280 or 854 など）
    pub fn height(&self) -> i32;
    pub fn clear(&mut self, color: Color);   // 背景
    pub fn rect(&self, x, y, w, h, color);   // 矩形塗りつぶし
    pub fn text(&self, x, y, s: &str, color); // 文字（UTF-8 / 日本語OK）
    pub fn string_width(&mut self, s: &str) -> i32;
    pub fn present(&mut self);               // 画面に反映
}
```

`Color` は `u32`（0xAARRGGBB）。**線を引く関数・角丸・画像表示はありません。**
必要なら `rect` を並べて作ってください（`text.rs` に追加実装してもよい）。

フォントは本体の**共有フォント**（`plGetSharedFontByType`）を stb_truetype でラスタライズしています。
日本語は問題なく出ます。

### 画面遷移は `hbedit.rs` の `Screen` enum

```rust
pub enum Screen {
    SaveList, Slots, Char, Items, ItemPick, Charms, CharmEdit, SkillPick,
    Equip, QuestEquip, SaveQuests, ArcList, ArcEntries, EntryView, Mib, Gmd,
    Check, Hunts, Guild, Roster, Sets, Palico, Options, Convert,
}
```

`Editor::lines()` が**表示する行の一覧**（`Vec<Line>`）を返します。
`Line { text: String, selectable: bool, cursor: bool }` の 3 つだけです。

**今の UI はこれをそのまま縦に並べているだけ**なので、そこが「分かりにくい」原因です。

### 今の描画（`editor.rs` の `draw()`）

```
┌──────────────────────────────────────────┐
│ mhnxeditor  キャラクター編集              │ ← ヘッダ
├──────────────────────────────────────────┤
│ > 名前        ナナ                        │ ← lines() をそのまま縦に
│   所持金      9997199                     │
│   ハンターランク 967                      │
│   ...                                     │
├──────────────────────────────────────────┤
│ 変更なし                                  │ ← メッセージ
│ [A 保存] [<> 値を変更] [X 刻み] ...       │ ← キーのチップ
└──────────────────────────────────────────┘
```

### 入力

`app.rs` がパッドを読み、`editor.rs` の `key_from()` が `Key` enum に変換します。

```rust
pub enum Key { Up, Down, Left, Right, A, B, X, Y, L, R, Minus, Plus, Z }
```

`Z` は **ZR ボタン**です。

## 直してほしいこと

### 最優先：全体的に分かりにくい

ユーザーの言葉：「UI がほんとうに意味わからない」「使い悪すぎる」

具体的な問題:

1. **文字だけの羅列**で、どこが選べるのか・どこが値なのか区別しにくい
2. **一覧が縦に並ぶだけ**で、項目の区切りがない
3. 選択中の行が `> ` だけなので**目立たない**
4. **値の変更方法が分からない**（`←→` で変わるが、押すまで気づけない）
5. 画面が変わっても**レイアウトが同じ**なので、今どこにいるか分からない
6. キーのチップは出るが、**押せる感がない**

### 提案（自由に設計してよい）

- 選択行を**背景色でハイライト**する
- 項目名と値を**左右に分けて**配置する（今は 1 本の文字列）
- 値を `[ 967 ]` のように**囲んで**、変更可能だと分かるようにする
- 画面ごとに**色やヘッダの見た目を変える**
- 一覧に**スクロールバー**や「12 / 137」のような**位置表示**を出す
- 下部のチップを**押せそうな見た目**にする（枠線・背景）
- 余白を整える（今は行間 22px 固定）

### 制約

- `hbedit.rs` は**PC と共用**です。`Screen` / `Editor` / `Key` の**意味を変えない**でください。
  - 表示用の情報を増やしたい場合は `lines()` の `Line` にフィールドを足すのは OK
  - ただし `hbedit_flow.rs` のテスト（120 件）が通ること
- `no_std` なので `std::` は使えません
- 描画は 60fps で毎フレーム呼ばれます。重い処理は避けてください

## ビルドと確認

```powershell
cd C:\anyWorkspace\nx\mhxx\mhnxeditor
.\switch\build.ps1          # → dist\mhnxeditor.nro
```

実機が無くても、**PC 側のテストで画面遷移は検証できます**。

```powershell
cargo test -p mhnx-core --features std,gamedata --test hbedit_flow
```

`Editor` を直接操作して、キー入力と画面の関係を確認できます。

```rust
let mut ed = Editor::new();
ed.open_save_bytes("system", &bytes);
ed.input(Key::A);          // キャラ画面へ
let lines = ed.lines();    // 表示される行
```

## 参考：画面の一覧と到達キー

| 画面 | 到達方法 | 内容 |
|---|---|---|
| SaveList | 起動直後 | セーブを開く（先頭が本体のセーブ） |
| Slots | セーブを開く | キャラ枠 3 つ |
| Char | Slots で A | 名前・所持金・HR など |
| Items | Slots で X | アイテムボックス |
| ItemPick | Items で R | アイテム選択（名前で絞り込み可） |
| Charms | Slots で L | お守り一覧 |
| CharmEdit | Charms で Y | お守り編集 |
| SkillPick | CharmEdit で Y | スキル選択 |
| Equip | Slots で - | 装備一覧 |
| Hunts | Char で - | 狩猟記録（137 体） |
| Guild | Equip で L | ギルドカード・闘技記録 |
| Roster | Equip で R | オトモ |
| Sets | Char で Y | マイセット |
| SaveQuests | ArcList で X | セーブ内のクエスト |
| QuestEquip | SaveQuests で L | クエスト装備 |
| ArcList | Slots で Y | .arc 一覧（ZR で追加） |

## 補足：PC 用 GUI は別物

`crates/mhnx-gui/` は egui 製の PC アプリで、**今回の対象外**です。
参考にしたい場合はそちらの UI を見てもらっても構いませんが、
Switch 側は egui が使えない（GPU も Window システムも無い）ため、
**同じ見た目を手描きで再現する**必要があります。

--- ここまで ---