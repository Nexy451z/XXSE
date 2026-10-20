# mhnxeditor

Monster Hunter XX (3DS / Switch) の**セーブ**と**クエスト**を扱う Rust 製エディタ。

| 成果物 | 対象 | 説明 |
|---|---|---|
| **mhnxeditor.exe** | PC (Windows) | egui/eframe による GUI エディタ |
| **mhnxeditor.nro** | Nintendo Switch (Atmosphere homebrew) | libnx 上で動く homebrew |
| **mhnx-core** | 共通 | no_std + alloc で動く解析ライブラリ |

同じコアライブラリを PC と Switch の両方で使うため、解析ロジックの二重実装が無い。

---

## 1. できること

### セーブ
- MHXX の @@system@@ を開く (3DS / Switch / MHGU をサイズで自動判別)
- キャラクター編集: 名前 / プレイ時間 / 所持金 / ハンターランク / HR ポイント / 各拠点ポイント / 外見 / カラー
- アイテムボックス編集 (2300 スロット、アイテム名は内蔵データから解決)
- **3DS ⇄ Switch 変換** (36 バイトのヘッダを計算で合成するので実機の空セーブは不要)
- Switch 用に @@system@@ と @@system_backup@@ の 2 ファイルをまとめて出力

### クエスト
- @@.arc@@ (クエストアーカイブ) の読み書き。中身は zlib 圧縮された複数ファイル
- **@@.mib@@ の 96 項目を名前付きで編集** (Quedixx のコントロール名と対応付け済み)
  - 列挙型のフィールドはドロップダウン (クエスト種類 / ランク / 初期位置 / 参加条件 …)
  - 緑 = 解析で確定したフィールド、黄 = 推定
  - 未確定領域は 249 バイトの 16 進グリッドで直接編集
- @@.gmd@@ (タイトル / 依頼主 / 依頼文 / 目的 / 失敗条件) のテキスト編集

### クエストの検査

実機・PC どちらでも **L ボタン**（PC 版は「このクエストをチェック」）で
そのクエストの問題を検査できます。

| 検査 | 内容 |
|---|---|
| 1人限定 | 参加条件1 が 0x2F のままだとマルチで受注できない。**その場で解除できる** |
| 報酬ファイル | rem_000000（ダミー）だけになっていないか |
| 報酬の中身 | アイテムIDが 0 のまま個数だけ設定されていないか、個数・確率が範囲内か |
| ID の一致 | .mib のクエストIDとファイル名が合っているか |
| 必須ファイル | .qdl / .gmd / .mib がそろっているか |

検査ルールは公式クエスト 141 件すべてで試しています。結果は 139 件が指摘なし、2 件が「1人限定」の警告、エラー 0 件でした。この 2 件は実際に参加条件1が 0x2F のデータなので、誤検出ではありません。
詳しい対処は [docs/quest_troubleshooting.md](docs/quest_troubleshooting.md) にまとめました。

### ダウンロードクエスト
- @@.arc@@ をセーブの DLC 領域 (205 枠) の**既存の枠**へ差し替え / 空にする

  **新しいクエストを増やすことはできません。** ゲームは「導入済みビット列」
  (カタログ順) を見て一覧を作るため、空き枠に書いても表示されません。
  ビット列の並びは公式クエストの一覧からは復元できず、推測で書くと
  別のクエストを壊す恐れがあるので、このツールは触りません。

---

## 2. ビルド

### PC 版

```powershell
cd nx\mhxx\mhnxeditor
cargo build --release -p mhnx-gui
# -> target\release\mhnxeditor.exe
```

依存: Rust 1.75+ (stable)。Windows 以外でもビルドできる (フォント読み込みだけ Windows 前提)。

### Switch 版 (NRO)

devkitPro (devkitA64 + libnx) が必要。詳細は [docs/switch_build.md](docs/switch_build.md) を参照。

```powershell
cd nx\mhxx\mhnxeditor\switch
.\build.ps1
# -> ..\dist\mhnxeditor.nro
```

---

## 2.5 成果物

| ファイル | サイズ | 説明 |
|---|---|---|
| @@dist/mhnxeditor.exe@@ | 約 5.4 MB | PC 版 GUI (単体で動作、追加 DLL 不要) |
| @@dist/mhnxeditor.nro@@ | 約 663 KB | Switch homebrew |
| @@dist/shots/*.png@@ | — | 全タブの実レンダリング結果 (UI スモークテスト) |

---

## 3. 使い方

### セーブを編集して Switch に戻す

```text
1. 先に必ずバックアップを取る (Ryujinx なら 4 ファイル全部)
2. 「セーブを開く」で system を選ぶ
3. 各タブで編集する
4. 「Switch 用 (system 2 ファイル)」で出力フォルダを指定
   → system と system_backup が生成される
5. Checkpoint / JKSV で書き戻す
```

### 3DS のセーブを Switch で使う

```text
1. 3DS の system (4,726,152 バイト) を開く
2. 「3DS から Switch へ変換 (ヘッダを合成)」を押す
3. あとは上と同じ手順で書き出す
```

36 バイトのヘッダは @@crc32(ファイル名) XOR 0xFFFFFFFF@@ で計算できるため、
「自分の Switch で作った空セーブ」を用意する必要は無い。

### Switch 実機でできること

```text
SD カード/
  switch/
    mhnxeditor.nro            <- hbmenu から起動
    mhnxeditor/
      saves/                  <- セーブ（system）を置く
        system
      quest/                  <- クエスト（*.arc）を置く
        q1010165.arc
```

| 画面 | できること |
|---|---|
| SAVE FILES | SD カード内のセーブを探して開く |
| CHARACTER SLOT | 3 つのキャラ枠から選ぶ |
| CHARACTER | 名前・所持金・プレイ時間・HR・HRP・各拠点ポイント・外見を編集 |
| ITEM BOX | 2242 枠の一覧。個数の増減、名前でアイテムを選び直す、1手戻す |
| CHARMS | お守り 2000 枠の一覧・削除・作成（スキル名つき） |
| CHARM EDIT | スキル・ポイント・装飾スロットを編集。**Y でスキルを名前から選ぶ** |
| SKILL PICK | 306 個のスキル名から選ぶ（ソフトウェアキーボードで絞り込み） |
| DOWNLOAD PALICO | 配信オトモの追加・削除（日本語版/英語版を L R で切替） |
| OPTIONS | 明るさ・振動・3D 表示・スライドパッドプロ |
| QUEST FILES | SD カード内の .arc を探して開く |
| QUEST | .arc の中のファイル一覧。A で編集、X でセーブへ書込、Y で中身を見る |
| ENTRY VIEW | .rem / .sup / .sem / .esl / .qdp / .qdl を解読して表示。.rem/.sup は **A でアイテムを差し替え**できる |
| CONVERT SAVE | 3DS と Switch のコンテナを相互変換して書き出す |
| QUEST PARAMETERS (.mib) | 96 項目を名前付きで編集。列挙型は値が切り替わる |
| QUEST TEXT (.gmd) | クエスト名・依頼文などを編集（ソフトウェアキーボードを使用） |
| CHECK | 検査結果の一覧。自動で直せる指摘には「修正」が付く |

**保存**はどちらの画面でも A ボタンです。セーブを保存すると、同じフォルダの
@@system_backup@@ も自動で書き直します（2 つが食い違うと不具合の元になるため）。

```text
ボタン      動き
----------------------------------------------
十字 上下   項目を選ぶ
十字 左右   値をへらす / ふやす
A           決定 / 開く / 保存する
B           ひとつ前の画面に戻る
X           一度に変わる量（1/10/100/1000/10000）、
            項目一覧では「セーブにインストール」、
            .gmd では「.arc に書き込む」
Y           セーブ編集 ⇄ クエスト編集 の切り替え
L / R       キャラクターの枠を切り替え
-           ファイルを探し直す
+           終了
```

補足:
- セーブもクエストも**同じフォルダに置いたものだけ**を読みます（暴発防止のため）。
- @@system_backup@@ は一覧に出しません（誤って単体で書くと壊れるため）。
- 画面は**日本語で表示されます**。Switch 本体に入っているフォントを借りて描いているので、
  クエスト名やアイテム名もそのまま読めます。使えない環境では自動で英数字の画面に切り替わります。
- 名前やクエスト文の入力には **Switch のソフトウェアキーボード**を使います（日本語入力可）。

### SD カードへの配置（自動）

```powershell
# SD カードのドライブを指定するだけ
.\deploy_sdcard.ps1 -Target E:\

# セーブとクエストも一緒に置く（3DS のセーブなら自動で Switch 形式に変換します）
.\deploy_sdcard.ps1 -Target E:\ -Save .\my_system -Quests .\my_quests
```

このスクリプトは消去を一切行いません。フォルダを作ってコピーするだけです。

### コマンド版 (mhnx.exe)

画面を使わずに確認・変換したいときは @@dist\mhnx.exe@@ を使います。

```text
mhnx info <file>                  何のファイルか調べる
mhnx save-info <system>           キャラクターの中身を全部表示
mhnx to-switch <3ds> <dir>        3DS セーブ -> Switch の system 2 ファイル
mhnx to-3ds <switch> <out>        36 バイトの見出しを外す
mhnx arc-list <file.arc>          クエストの中身一覧
mhnx arc-mib <file.arc>           .mib の 96 項目を表示
mhnx arc-gmd <file.arc>           クエストの文章を表示
mhnx arc-repack <in> <out>        展開して再圧縮（往復検証つき）
mhnx install <system> <file.arc>  クエストをセーブに入れる
mhnx dlc-list <system>            入っているクエスト一覧
```

### Switch 実機での使い方

```text
SD カード/
  switch/
    mhnxeditor.nro          <- hbmenu から起動
    mhnxeditor/
      saves/                <- ここに system を置く
        system
```

起動するとコンソール画面でセーブを探す。操作:

画面ごとに割り当てが変わります。各画面の最下行にも同じ案内が出ます。

| 画面 | 操作 |
|---|---|
| SAVE FILES | A 開く / Y クエスト編集 / **L 形式変換** / X オプション / - 再スキャン / + 終了 |
| CONVERT SAVE | A 変換して書き出す / B 戻る |
| CHARACTER SLOT | 上下 スロット / A キャラ / X アイテム / L お守り / R オトモ / Y クエスト / B 戻る |
| CHARACTER | 上下 項目 / 左右 値 / X 刻み / A セーブへ書込 / B 戻る |
| ITEM BOX | 上下 項目 / **LR 個数** / **R 種類を選ぶ** / **L 元に戻す** / X 刻み / Y 空欄表示 / A セーブへ書込 |
| ITEM PICK | A 決定 / Y 名前で絞り込み / X クリア / B 戻る |
| CHARMS | A 削除 / Y 選択中を編集 / L 新規 / X セーブへ書込 / B 戻る |
| CHARM EDIT | 上下 項目 / 左右 値 / **Y スキル名で選ぶ** / X 刻み / A 書込 / B 戻る |
| SKILL PICK | A 決定 / Y 名前で絞り込み / X クリア / B 戻る |
| DOWNLOAD PALICO | A 追加・削除 / LR 言語 / X セーブへ書込 / B 戻る |
| OPTIONS | 上下 項目 / 左右 値 / A セーブへ書込 / B 戻る |
| QUEST FILES | A 開く / X セーブ内のクエスト / Y セーブ編集 / + 終了 |
| QUESTS IN SAVE | A 編集 / - 1件抽出 / R 全件抽出 / **Y 絞り込み / X クリア** / B 戻る |
| QUEST | A 編集 / **Y 中身を見る** / X セーブへ書込 / - 1件抽出 / R 全件抽出 / L チェック / B 戻る |
| ENTRY VIEW | 上下 行 / **A アイテム変更**（.rem .sup のみ）/ X .arc へ書込 / B 戻る |
| QUEST PARAMETERS | 上下 項目 / 左右 値 / X 刻み / A .arc へ書込 / B 戻る |
| QUEST TEXT | A テキストを入力 / X .arc へ書込 / B 戻る |
| CHECK | A 自動修正 / X .arc へ書込 / B 戻る |


Switch 側のコンソールは 8x8 ASCII フォントなので、NRO 版の UI は英語表記にしている
(日本語を出すと豆腐になる)。日本語のフル GUI は PC 版を使う。

---


### 書き込み前のバックアップ

セーブを上書きするとき、**元のファイルを `<name>.bak` に退避してから書きます**。
GUI の「上書き保存」「名前を付けて保存」と、実機の書き込みの両方で同じ動きです。
失敗しても元に戻せます。


## 4. リポジトリ構成

```text
mhnxeditor/
  Cargo.toml                 ワークスペース
  crates/
    mhnx-core/               解析ライブラリ (no_std + alloc)
      src/save.rs            セーブコンテナ / キャラクタ / スロット
      src/itembox.rs         アイテム 19bit パッキング
      src/arc.rs             クエスト .arc コンテナ
      src/quest.rs           .mib / .gmd
      src/dlc.rs             セーブ内 DLC クエスト領域
      src/crc.rs             CRC32 とヘッダ用ハッシュ
      src/gamedata.rs        アイテム / 装備名テーブル (自動生成)
      tests/real_data.rs     実データに対する統合テスト
      src/questfields.rs     .mib 96 項目 (うち 10 は生バイト表示) + 15 enum (docs/quest_fields.json から自動生成)
    mhnx-gui/                PC 版 (egui/eframe) -> mhnxeditor.exe
  switch/                    Switch 版 (libnx) -> mhnxeditor.nro
    src/editor.rs            SD カードのセーブを読み書きする本体 UI
    src/sys.rs               libnx / newlib のバインディング
    build.ps1                再現可能なビルドスクリプト
  tools/                     開発用スクリプト
  docs/                      解析ドキュメント
  dist/                      ビルド成果物
```

---

## 5. 検証

```powershell
cargo test -p mhnx-core
```

このテストは**このマシンにある実データ**を使って以下を検証する
(ファイルが無い環境では自動的にスキップされる):

| テスト | 内容 |
|---|---|
| @@parses_the_real_ryujinx_save@@ | 実セーブを解析し、キャラ名 / 所持金 / プレイ時間 / スロット基点 / DLC セクション位置を照合 |
| @@three_ds_conversion_round_trips@@ | Switch → 3DS → Switch でバイト単位一致すること |
| @@parses_every_shipped_quest_archive@@ | 141 個の実 @@.arc@@ を全部解析し、再構築して中身が一致すること |
| @@quest_mib_ids_match_file_names@@ | 各 @@.mib@@ のクエスト ID がファイル名と一致すること |
| @@gmd_round_trips_for_every_quest@@ | 全 @@.gmd@@ がバイト単位で往復できること |
| @@blank_templates_agree_with_computed_headers@@ | 配布されている空セーブ 4 件のヘッダが計算値と一致すること |

UI のスモークテストはスクリーンショットモードで行う:

```powershell
$env:MHNXEDITOR_SCREENSHOT = "shot.png"
$env:MHNXEDITOR_OPEN_SAVE  = "...\system"
$env:MHNXEDITOR_OPEN_ARC   = "...\q1010165.arc"
$env:MHNXEDITOR_OPEN_TAB   = "quest"
.\dist\mhnxeditor.exe
```

数フレーム描画して PNG を保存し終了する。UI のどこかで panic すれば PNG が生成されないので、
自動テストとして使える。@@dist\shots\@@ に全タブの実レンダリング結果を置いてある。

---

## 6. 解析の根拠

このエディタが前提としている事実は、すべて実データで検証済み。
詳細は [docs/](../save_editor/analysis/MHXX_SAVE_ANALYSIS.md)、[docs/quest_format_spec.md](docs/quest_format_spec.md)、
[docs/talisman_format.md](docs/talisman_format.md) を参照。

お守りのレコード構造と、レア度ごとのスキル許容ポイント表の引き方は
[docs/talisman_format.md](docs/talisman_format.md) に検証結果つきでまとめてあります。

| 事実 | 検証方法 |
|---|---|
| Switch セーブ = 36 B ヘッダ + 4,726,152 B 本体 | 実セーブ 4 ファイル + 配布空セーブ 2 ファイル |
| ヘッダ +0x14 = @@crc32(ファイル名) XOR 0xFFFFFFFF@@ | 6 件のヘッダで合成値が完全一致 |
| ヘッダは内容チェックサムではない | 本体が 100 万バイト違うセーブでもヘッダが 36 B 完全一致 |
| スロット基点 0x126474 / 0x244C34 / 0x3633F4、ストライド 0x11E7C0 | 実セーブの実測値と @@Offsets.cs@@ が一致 |
| DLC 領域が共有ブロックを過不足なく埋める | 205 x 0x1400 + 45 x 0x800 の終端 = 0x126474 |
| @@.mib@@ は 249 バイト固定、ID は +0x0C | 141 ファイル全部で照合 |
| @@.mib@@ = 0x1BBFD18E / @@.gmd@@ = 0x242BB29A | @@quest_open/@@ の実ファイル名と照合 |

---

## 7. 注意

- 編集前に**必ずバックアップ**を取ること。Ryujinx は @@0\@@ と @@1\@@ に同じ内容を二重保持しているので 4 ファイルすべて。
- オンラインプレイでの使用は BAN 対象。**オフライン専用**にすること。
- 本ツールは解析結果に基づく非公式ツール。自己責任で使用すること。

---

## クエストの抽出（実機）

セーブに入っている配信クエストを \`.arc\` として SD カードへ書き出せます。

```text
操作:  クエストを開く -> - (マイナス) でその1件
                       -> R で全件
保存先: /switch/mhnxeditor/mhxx/quest/arc/q<ID>.arc
```

- フォルダは自動作成されます（\`mhxx/quest/arc/\` の階層ごと）
- ファイル名はゲーム本体と同じ \`q<ID>.arc\`
- 中身は**バイト単位のコピー**なので、そのまま編集して書き戻せます
- 書き出したフォルダは次回のスキャンでクエスト一覧にも出ます

**マイナスボタンの役割**: クエストのエントリ画面では「この1件を抽出」、
それ以外の画面では従来どおり「フォルダ再スキャン」です。

## SD カードの配置

```text
sd:/switch/mhnxeditor/
    mhnxeditor.nro
    saves/                 <- セーブを置く場所（任意）
    quest/arc/             <- クエストを置く場所（任意）
    mhxx/quest/arc/        <- 抽出したクエストの出力先
```

---

## セーブ内でクエストを編集する（GUI）

ファイルを経由せず、セーブから直接クエストを開いて書き戻せます。

```text
DLC タブ -> クエスト一覧 -> [編集]
  -> クエストタブに移動し「セーブ内のクエスト <ID> を編集中」と表示
  -> フィールドやテキストを編集
  -> [セーブに書き戻す]
```

- 開いた時点では**メモリ上にコピー**するだけで、書き込みは行われません
- 書き戻しは**同じ ID の枠**へ入るので、導入フラグを壊しません
- 何も編集せずに書き戻した場合、**セーブは 1 バイトも変化しません**

## セーブ内でクエストを編集する（実機）

```text
クエスト一覧 -> X  でセーブ内のクエスト一覧
  -> Up/Down で選ぶ（カーソル行だけタイトルを表示）
  -> A でクエストエディタを開く
  -> .mib / .gmd を編集
  -> X でセーブへ書き戻す
  -> B でクエスト一覧、Y でセーブ一覧、A でファイルへ保存
```

| 画面 | ボタン | 動作 |
|---|---|---|
| クエスト一覧 | **X** | セーブ内のクエスト一覧へ |
| セーブ内一覧 | **A** | そのクエストを編集 |
| セーブ内一覧 | **−** / **R** | 1件抽出 / 全件抽出 |
| セーブ内一覧 | **B** | クエスト一覧へ戻る |

## 書き出しの安全性

- 無編集の書き戻しは**バイト一致**（テストで固定）
- 編集した場合も**そのスロットだけ**が変わり、他のクエストには触れません
- 無編集なら arc は**バイト単位で一致**します（元の圧縮ストリームと元の
  オフセットを保持しているため）。編集した場合はそのエントリだけが
  再圧縮され、arc は数バイト〜数十バイト大きくなります（実測 +8〜+23 B、
  枠の空きは 1659 B）