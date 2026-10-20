# UI 改修の引き継ぎプロンプト

このファイルは、**別の AI / 開発者に UI の作り直しを依頼するとき**にそのまま渡すためのものです。
下の「--- ここから ---」以降をコピーして使ってください。

--- ここから ---

あなたは Rust 製デスクトップアプリの UI を担当します。既存のロジックは完成しているので、
**UI の改善だけ**を行ってください。

## プロジェクト

```
C:\anyWorkspace\nx\mhxx\mhnxeditor
```

Nintendo Switch 用ゲーム「モンスターハンターダブルクロス（MHXX）」のセーブデータエディタです。
PC 用 GUI（`mhnxeditor.exe`）と、Switch 実機用 homebrew（`mhnxeditor.nro`）の 2 つの入口がありますが、
**UI の改修対象は PC 用 GUI のみ**です。

## 技術スタック

| 項目 | 内容 |
|---|---|
| 言語 | Rust 1.97.1（PC） / nightly 1.101.0（Switch） |
| GUI | eframe / egui **0.32.3**（即時モード GUI） |
| その他 | egui_extras 0.32、rfd 0.15、image 0.25 |

## ディレクトリ構成

```
mhnxeditor/
├─ crates/
│  ├─ mhnx-core/     ← 全ロジック（PC と Switch で共用）
│  ├─ mhnx-gui/      ← ★PC 用 GUI。UI 改修の対象はここ
│  ├─ mhnx-cli/      ← CLI
│  └─ mhnx-nro/      ← Switch 用の薄いラッパ
├─ switch/           ← Switch 実機用（.nro のビルド、UI はテキスト画面）
├─ dist/             ← ビルド成果物
└─ docs/             ← 解析レポート・仕様書
```

## 重要なファイルと役割

### GUI（改修対象）

| ファイル | 行数 | 役割 |
|---|---|---|
| `crates/mhnx-gui/src/app.rs` | 3,232 | **画面のほぼ全部**。タブごとの `tab_*()` メソッド |
| `crates/mhnx-gui/src/main.rs` | 小 | 起動処理、ウィンドウ設定、環境変数フック |

`app.rs` の構造:

```rust
pub struct MhnxEditor {
    pub save: Option<Save>,        // 開いているセーブ
    pub slot: usize,               // 選択中のキャラ枠 (0..3)
    pub tab: Tab,                  // 表示中のタブ
    pub items: Vec<ItemSlot>,      // アイテムボックス
    pub message: String,           // 下部のステータス行
    pub dirty: bool,               // 未保存の変更があるか
    // ... タブごとの作業バッファ
}

pub enum Tab { Save, Items, Quest, Dlc, Charm, Equip, Hunts, Guild, Palico, Sets, Extras, About }

impl MhnxEditor {
    fn update(...)           // egui のエントリポイント
    fn tab_bar(ctx)          // 上部のタブ
    fn side_panel(ctx)       // 左のセーブ情報
    fn tab_save(ui)          // キャラクター編集
    fn tab_items(ui)         // アイテムボックス
    fn tab_quest(ui)         // .arc クエスト編集
    fn tab_dlc(ui)           // 導入済みクエスト
    fn tab_charm(ui)         // お守り
    fn tab_equip(ui)         // 装備
    fn tab_hunts(ui)         // 狩猟記録
    fn tab_guild(ui)         // ギルドカード
    fn tab_palico_detail(ui) // オトモ
    fn tab_sets(ui)          // マイセット
    fn tab_extras(ui)        // 設定・配信オトモ
    fn tab_about(ui)         // 情報
}
```

### ロジック（触らなくてよい）

| ファイル | 役割 |
|---|---|
| `mhnx-core/src/save.rs` | セーブの読み書き。`Save` 構造体、キャラ・アイテム・狩猟記録の API |
| `mhnx-core/src/dlc.rs` | クエストの追加・上書き・削除、クエスト装備 |
| `mhnx-core/src/charm.rs` | お守り（スキル・スロット・`first_free()`） |
| `mhnx-core/src/guildcard.rs` | ギルドカード、闘技記録 |
| `mhnx-core/src/palico.rs` | オトモ |
| `mhnx-core/src/myset.rs` | アイテムセット・装備セット |
| `mhnx-core/src/arc.rs` | クエストの .arc コンテナ |
| `mhnx-core/src/gamedata.rs` | 日本語名テーブル（アイテム・スキル・装備・モンスター） |
| `mhnx-core/src/hbedit.rs` | **Switch 用**テキスト画面のロジック（PC GUI からは未使用） |

**UI 改修では `mhnx-core` を変更しないでください。** 既存の API を呼ぶだけで足ります。

## ビルドと確認

```powershell
cd C:\anyWorkspace\nx\mhxx\mhnxeditor
cargo build --release -p mhnx-gui
.\dist\mhnxeditor.exe
```

### スクリーンショット付きの自動確認

起動時に環境変数で操作を指定できます。**UI の確認に便利**です。

```powershell
$env:MHNXEDITOR_OPEN_SAVE = 'C:\anyWorkspace\nx\jksv_ready\system'
$env:MHNXEDITOR_OPEN_TAB  = 'items'   # save|items|quest|dlc|charm|equip|hunts|guild|palico|sets|extras|about
$env:MHNXEDITOR_SCREENSHOT = "$PWD\shot.png"
Start-Process .\dist\mhnxeditor.exe -PassThru | ForEach-Object { $_.WaitForExit(40000) }
```

### テスト

```powershell
cargo test --workspace -j 1     # 207 件。UI を変えても壊さないこと
```

## テスト用セーブ

| パス | 内容 |
|---|---|
| `C:\anyWorkspace\nx\jksv_ready\system` | Switch 形式、3 キャラ（HR967 / 999 / 999） |
| `C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system` | **実セーブ。書き込まないこと** |

作業用コピーは `.work\` 配下に作ってください。

## 現状の UI と、直してほしいこと

### 直近で直したところ（参考）

- タブ 12 個を**上部バー**に移した（以前はサイドバー内の 1 行で、サイドバーが画面の 55% を占めていた）
- ウィンドウ既定サイズを 1180x740 にした（以前の 1280x820 は 1536x864 の画面からはみ出した）

### 残っている要望（優先度順）

1. **全体的に「どこを押せばいいか分からない」**
   - アイテムの「追加」はボタン 1 つでできるようにしたが、まだ発見しにくい
   - お守りの編集が一覧 + 右側の詳細パネルの 2 箇所に分かれていて分かりにくい
2. **アイテムの「個数」列が見える位置にない**（横スクロールが必要な状態）
3. **エラーメッセージが英語**（`crates/mhnx-core/src/lib.rs` の `Display` 実装）
4. お守りのエディタが 2 箇所にある（`app.rs` 内、片方はスキル ID の生入力）
5. 「設定/オトモ」タブが毎フレーム `save.clone()` していて重い
6. DLC タブが毎フレーム全クエストの .arc を parse していて重い

### 触ってはいけないところ

- `mhnx-core` の公開 API の**意味**（オフセットや型は解析で確定済み）
- 保存の流れ: `apply_to_save()` → `apply_hunts()` → `write_current()`
  （狩猟記録は独自バッファなので、保存前に必ず反映が必要）
- `.arc` 追加時は `store_arc_entry()` で編集中バッファを書き戻してから `to_bytes()`

## 補足：Switch 実機用のビルド環境

PC 用 GUI の改修だけなら不要ですが、参考まで。

```
devkitPro      : C:\devkitPro         （libnx は C:\devkitPro\libnx）
ビルド        : .\switch\build.ps1  → dist\mhnxeditor.nro
ツールチェイン: aarch64-nintendo-switch-libnx（rustup で追加済み）
```

Switch 側の UI は `switch/src/editor.rs`（描画）と `crates/mhnx-core/src/hbedit.rs`
（画面遷移・入力処理）で、egui ではなく**自前のテキスト描画**です。今回は対象外です。

--- ここまで ---