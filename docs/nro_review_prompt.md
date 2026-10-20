# MHXX Switch NRO セーブ＆クエストエディタ (`mhnxeditor.nro`) ロジック・UI・操作性 総合レビュー依頼プロンプト

Nintendo Switch 実機で動作する MHXX（モンスターハンターダブルクロス）用 Homebrew セーブ＆クエストエディタ（`.nro`）について、**ロジックの堅牢性・UIの視認性・タッチ＆コントローラー操作性（UX）** の総合レビューとさらなる改善をお願いします。

---

## 1. ワークスペースと開発環境（すべて絶対パス）

- **プロジェクトルート**: `C:\anyWorkspace\nx\mhxx\mhnxeditor`
- **devkitPro ルート**: `C:\devkitPro`
- **libnx ヘッダ参照先**: `C:\devkitPro\libnx\include\switch\`
- **Rust ツールチェーン**: `nightly` (`rustc 1.101.0-nightly`)
- **ターゲット**: `aarch64-nintendo-switch-libnx`（`#![no_std]` + `alloc` 環境。`std` や `println!` は使用不可）
- **NRO ビルドスクリプト**:
  ```powershell
  Set-Location "C:\anyWorkspace\nx\mhxx\mhnxeditor"
  .\switch\build.ps1
  ```
  - 出力先 NRO: `C:\anyWorkspace\nx\mhxx\mhnxeditor\dist\mhnxeditor.nro`
  - 出力先 ELF: `C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\target\aarch64-nintendo-switch-libnx\release\mhnxeditor.elf`
- **PC ホスト側テストコマンド（121件のテスト）**:
  ```powershell
  Set-Location "C:\anyWorkspace\nx\mhxx\mhnxeditor"
  cargo test -p mhnx-core --features std,gamedata --test hbedit_flow
  ```

---

## 2. 主要ファイル一覧（絶対パスと役割）

### Switch フロントエンド (`C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\`)
| ファイル（絶対パス） | 役割 |
|---|---|
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\ui.rs` | **★ GUI 描画・タッチ入力・内蔵ソフトウェアキーボード (OSK) の主戦場**（約4,335行）。全24画面の2ペインGUI、ヒットボックス判定、ドラッグスクロール、内蔵OSKを実装。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\editor.rs` | **★ Switch メインループ・I/O・入力ブリッジ**（約870行）。パッド入力・長押しリピート・右スティック高速スクロール・タッチ状態取得・ファイル/NANDセーブI/O・キーボード呼び出しを担当。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\savedata.rs` | **★ 本体 NAND セーブデータ (`save:/system`, `save:/system_backup`) のマウント・読み書き・コミット処理**。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\text.rs` | **★ ソフトウェアレンダラ＆日本語フォント描画エンジン**（`1280x720` RGBA8888 フレームバッファ）。3サイズフォント（`Small=17px`, `Medium=22px`, `Large=28px`）、角丸矩形 (`round_rect`)、枠線、円、グラデーション、クリッピング描画を提供。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\swkbd.rs` | Switch OS 標準のシステムキーボード (`swkbd` LibraryApplet) の FFI ラッパー。タイトルオーバーライド起動時 (`AppletType_Application`) に使用。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\sys.rs` | `libnx` / `newlib` の raw FFI 宣言（`PadState`, `HidTouchScreenState`, `AccountUid`, `FsFileSystem` 等）。構造体サイズ・オフセットの `const` アサーション付き。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\c_shim\font_shim.c` | Switch 内蔵共有フォント (`PlSharedFontType_Standard`) を `stb_truetype` でラスタライズする C シム（4スロットキャッシュ）。 |

### コアロジック＆ゲームデータ (`C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\`)
| ファイル（絶対パス） | 役割 |
|---|---|
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\hbedit.rs` | **★ 画面遷移・編集状態マシン (`Editor`, `Screen`, `Key`)**（約3,570行）。24種類の `Screen` と入力ハンドラ・描画用データを管理。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\tests\hbedit_flow.rs` | **★ 124件（121 active + 3 ignored）の網羅的テストスイート**。画面遷移・セーブ編集・クエスト改造・ヒントテーブル整合性を検証。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\save.rs` | MHXX (Switch / 3DS) および MHGU セーブデータコンテナのバイナリ読み書き。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\charm.rs` | 護石（お守り）の読み書き・合法性（レア度別スキルポイント範囲）チェック。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\dlc.rs` | セーブ内ダウンロードクエスト・クエスト装備の展開・追加・上書き。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\equip.rs` | 装備ボックス（武器・防具・お守り）のデコードと名称解決。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\palico.rs` | 雇用オトモ（Roster）のステータス・名前編集。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\guildcard.rs` | ギルドカード・闘技大会記録の読み書き。 |
| `C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\questfields.rs` | クエスト `.mib` の全フィールド定義（日本語名 `ja`・備考 `note`・列挙値テーブル）。 |

---

## 3. 現在までに実装・修正済みの内容

1. **2ペインのリッチGUI (`C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\ui.rs`)**:
   - 上部ヘッダー（キャラスロット1〜3のワンタップ切替タブ、未保存バッジ＆クイック保存ボタン、再読込・終了ボタン）
   - 左サイドバーナビゲーション（14項目、タッチで画面直行、`ZL` ボタンでタブ巡回）
   - コンテキストツールバー（刻み幅 `±1`〜`±10000` のピル切替、検索フィルタバー、画面別アクションボタン）
   - 全24画面の専用カードUI・日本語アイテム名/モンスター名/スキル名表示・`[-]` `[+]` `[MAX]` ステッパー・スクロールバー
2. **タッチスクリーン＆アナログスティック対応 (`C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\editor.rs`, `ui.rs`)**:
   - 全ボタン・行・タブ・スクロールバーのタッチ操作、リスト領域のドラッグスクロール
   - 左スティックでの十字キー同等移動、右スティック上下での高速ページスクロール、`BTN_ZR` ビット修正（`1 << 9`）
3. **アプレットモード対応の本体 NAND セーブ読み書き (`C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\savedata.rs`)**:
   - `accountInitialize` を `Application (0)` → `System (1)` → `Administrator (2)` の順でフォールバック初期化し、アルバム起動（アプレットモード）でもアカウントサービスを初期化可能に修正。
   - `PreselectedUser` → `LastOpenedUser` → `TrySelectUserWithoutInteraction` → `ListAllUsers` の順で本体内の全ユーザーを走査し、MHXXのセーブデータを持つユーザーを自動検出してマウント。
   - 保存時は一時ファイル作成＋`rename`（Horizon OSで `PathAlreadyExists` や容量超過を起こす原因）を廃止し、`save:/system`（および存在すれば `save:/system_backup`）を `"r+b"` で直接開いて **変更された16KBチャンクのみをインプレース差分書き込み＋256KBごとに分割コミット (`fsdevCommitDevice`)** する方式に刷新。
4. **アプレットモード用 内蔵ソフトウェアキーボード (OSK) (`C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\ui.rs`)**:
   - アプレットモードでは Switch OS の `swkbd` アプレットが起動できないため、`!swkbd::is_available()` の場合は自動でキャンバス上の内蔵キーボードモーダル (`OskState`) を起動。
   - 「ひらがな」「カタカナ」「英数字・記号」「よく使う漢字・検索語（モンハン頻出語パレット）」の4タブを搭載し、タッチ・コントローラー双方で文字入力・濁点/半濁点/小文字変換が可能。

---

## 4. 絶対に守るべき制約事項（重要）

1. **`hbedit_flow.rs` の静的検査を壊さないこと**:
   - `C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\tests\hbedit_flow.rs` は `include_str!("../../../switch/src/editor.rs")` で `switch/src/editor.rs` を読み込み、`fn actions(screen: mhnx_core::hbedit::Screen)` から最初の `\n}` までを文字列パースして、全 `Screen` バリアントのキーヒントと実際の入力処理が一致しているかをテストしています。
   - そのため、`C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\editor.rs` の `pub(crate) fn actions(...)` のシグネチャ・マッチアーム構造・終端の `\n}`、および既存のキーバインドを変更するときは、必ず `hbedit_flow` の121件のテストがすべて通ることを確認してください。
2. **Switch ターゲット (`#![no_std]` + `alloc`) で警告・エラーなくビルドできること**:
   - 変更後は必ず `.\switch\build.ps1` と `cargo test -p mhnx-core --features std,gamedata --test hbedit_flow` の両方を実行して検証してください。

---

## 5. レビューしてほしい重点項目

以下の3つの観点からコードを精査し、問題点・エッジケースの洗い出しと改善提案（または直接の修正実装）をお願いします。

### ① ロジック・データ整合性・安全性のレビュー
1. **本体 NAND セーブ書き込み (`C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\savedata.rs`, `editor.rs`)**:
   - `write_save_in_place` の16KB差分書き込み・256KB分割コミットにおいて、ファイル末尾端数や `fseek` / `fwrite` / `fclose` / `fsdevCommitDevice` のエラーハンドリングに抜けがないか？
   - `save:/system` と `save:/system_backup` のヘッダ（36バイト Switch ヘッダ）およびボディの同期ロジックに不整合が生じるケースはないか？
   - バックアップファイル（`/switch/mhnxeditor/backups/system_<stamp>.bak`）が SD カード容量を圧迫しないか（世代管理や古いバックアップの整理が必要か）？
2. **エディタ状態機械 (`C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\hbedit.rs`, `switch\src\editor.rs`)**:
   - 各画面で値を変更した際の `ed.dirty` フラグのセットと、保存成功時の `ed.dirty = false` リセットに漏れがないか？（特にクエスト `.arc` 単体保存時とセーブデータ保存時の `dirty` の区別など）
   - 内蔵 OSK (`OskState`) での入力文字数制限（ハンター名31バイト、オトモ名20バイト、ギルドカードコメント等）が入力中にも分かりやすく制限または表示されているか？
3. **未実装・表示専用機能の拡張性**:
   - `hbedit_flow.rs` にある3件の `ignored` テスト（`spec_a_quest_can_be_added_from_an_arc`, `spec_every_screen_has_its_own_hint_chips`, `spec_the_guild_card_can_be_edited`）や、`gap_*` テストで示されている表示専用画面（装備BOX、マイセット等）について、既存テストを壊さずにどこまで編集機能を強化できるか？

### ② UI / ビジュアル・レイアウトのレビュー (`C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\ui.rs`)
1. **全24画面の描画と座標計算**:
   - 720p (`1280x720`) キャンバス上で、長いアイテム名・クエスト名・スキル名・ファイルパスが表示された際に、隣のバッジやボタンと文字が重なる箇所がないか？（`text_clipped` が適切に使われているか）
   - 空リスト時（お守り0個、セーブ内クエスト0件、検索ヒット0件など）のゼロステート表示が分かりやすいか？
2. **内蔵ソフトウェアキーボード (OSK) の UI**:
   - 現在の文字数 / 最大バイト数のインジケータ表示や、より使いやすい文字配列・変換機能の改善余地はないか？

### ③ 操作性（タッチ＆コントローラー UX）のレビュー (`C:\anyWorkspace\nx\mhxx\mhnxeditor\switch\src\ui.rs`, `editor.rs`)
1. **タッチ操作とドラッグスクロールの兼ね合い**:
   - リスト行内のボタン（`[-]` `[+]` `[MAX]` `[編集]` 等）をタップした際や、リストを上下にドラッグスクロールした際に、意図しない行選択や誤発火が起きないか？
2. **コントローラー操作の快適性**:
   - キャラ名変更（`Screen::Char` の row 0）で、コントローラーの `A` ボタンを押した時に「セーブ保存」が走る仕様（`hbedit.rs` の仕様で名前変更は `</>` キー）が直感に反しないか？UI上のガイド表示やタッチ導線でさらに分かりやすくできるか？
   - 長押しリピート (`REPEAT_DELAY`) の初速・加速バランスは実機操作で快適か？
   - 内蔵 OSK 表示中に長押しリピートでカーソル移動した際、文字入力 (`Key::A`) まで暴発リピートしないか？（移動キーのみリピートさせ、決定/削除キーは単発エッジにするべきではないか？）
