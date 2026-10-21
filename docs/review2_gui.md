# PC 用 GUI (egui) 再レビュー — `crates/mhnx-gui/src/{app.rs,main.rs}`

対象: `mhnx-editor` の PC 版 GUI。`app.rs` 3,326 行 + `main.rs` 150 行。
レビュー中に HEAD は `33918de` → `efc7303` (core/CLI/Switch のみ) に進んだが、`app.rs` は `33918de` のまま (サイズ 141,785 B / 3,326 行 / `git log -- crates/mhnx-gui` いずれも不変)。以下の行番号は現在の working tree に対応する。画面撮影は dist のビルド、状態機械とフレーム費用の実測は現在の core ソースで行い、GUI の挙動に関する限り両者は一致した。
コードは **一切変更していない**。作業用コピーは `C:\anyWorkspace\nx\mhxx\mhnxeditor\.work\review_gui2\jksv\system`、
実セーブ (`%APPDATA%\Ryujinx\bis\...\system`) は読み書きしていない。

## 検証方法 (実測と推測の区別)

| # | 方法 | 実体 |
|---|---|---|
| 1 | 画面の実測 | `dist\mhnxeditor.exe` + `MHNXEDITOR_OPEN_SAVE/OPEN_TAB/SCREENSHOT` で 12 タブを撮影。画像は `.work\review_gui2\shots\`。レビュー中に別セッションが dist を再ビルドしたが、撮り直した items/palico の PNG は SHA256 まで同一だった |
| 2 | 状態機械の実測 | `.work\review_gui2\harness` が `#[path]` で **本物の `app.rs`** を読み込み、ウィンドウなしで `open_save` / `select_arc_entry` / `write_current` 等を呼ぶ。以下の [A]〜[G] はこの出力 |
| 3 | フレーム費用の実測 | `.work\review_gui2\bench` (release) が 175 クエスト入り実セーブで各処理を反復計測 |
| 4 | 保存経路の実測 | 本物の exe を `MHNXEDITOR_SAVE_OUT` で走らせ、出力の SHA256・`.bak` を比較 |
| 5 | コード読解 | 行番号つき。推測は「推測」と明記 |

再現コマンド:

```
# 本物の app.rs を駆動する (実測 A-G)
target\debug\guiharness.exe .work\review_gui2\jksv\system dist\quests_all\q1010001.arc .work\review_gui2\harness_tmp

# フレーム費用 (実測)
.work\review_gui2\bench\target\release\guibench.exe C:\anyWorkspace\nx\jksv_ready\system

# 画面
$env:MHNXEDITOR_OPEN_SAVE='...\jksv\system'; $env:MHNXEDITOR_OPEN_TAB='items'; $env:MHNXEDITOR_SCREENSHOT='...\items.png'; Start-Process .\dist\mhnxeditor.exe -PassThru | %{ $_.WaitForExit(40000) }
```

---

## 要約

- **P0 が 2 件**。どちらも「既存データが silently 置き換わる」型。
  1) スロットを切り替えたあと「元に戻す」を押すと、**別スロットのアイテム箱 186 枠**が現在のスロットへ書き込まれる (実測 [C])。
  2) 「Switch 用 (system 2 ファイル)」は、選んだフォルダに既にある `system`/`system_backup` を **確認もバックアップもなく上書き**する (実測 [D1] で `.bak` なし)。実セーブのフォルダを選ぶと両方の控えが同時に消える。
- **P1 が 13 件**。中心は「編集バッファと dirty の不一致」。
  - キャラ欄 (名前・所持金…) の編集は dirty に乗らず、スロット切替で無言消失 ([B])。
  - .arc のエントリを切り替えると .mib/.gmd の編集が無言消失 ([A])。切替時のフラッシュ処理は **到達不能なデッドコード** (4 呼び出し全部が先に `arc_selected` を書き換える)。
  - .mib のフィールド編集は保存に載らないのに、16 進欄だけが dirty を立て、上書き保存で「未保存」が消える ([E])。
  - 狩猟記録は上書き保存にだけ載り、Switch 2 ファイル / 3DS 書き出しでは落ちる ([D1][D2])。
- 性能は **DLC タブが 175 件で 8.7 ms/フレーム** (実測)。それ以外は軽い。ただしサイドバーの `to_bytes()` が**全タブで毎フレーム 0.66 ms**、設定/オトモは 4.7 MB の clone が毎フレーム 0.67 ms。
- レイアウトは 3 箇所が破綻: アイテムのツールバー右端で「アイテムを追加」が切れる、オトモの表が右端で切れて修得スキルが読めず、名前欄が 3 文字幅、設定/オトモとマイセットには外側スクロールが無い。
- 良い面も多い: 上書き保存は必ず `.bak` を作り (実測)、無変更なら**バイト一致**で書き戻す (SHA256 一致)。セーブ内クエストの書き戻しループは正しく動く ([F])。デバッグビルドのクエストタブはパニックしなくなった。アイテム表は仮想化済みで 2,300 行でも軽い。

---

## 重大度別の問題

### P0 — データ破壊

#### P0-1 「元に戻す」がスロットをまたいで別キャラのアイテム箱を上書きする
- 根拠: `app.rs:200-218` (`reload_from_save` は `items` を消すが `item_undo` を消さない)、`app.rs:1295-1303` (`undo_item` は pop した箱をそのまま `apply_to_save`)、`app.rs:1180-1186` (ボタンは `!item_undo.is_empty()` だけで有効化)。
- 実測 [C] (本物の app.rs):
  - slot0 で「アイテムを追加」→ undo 段数 1。slot1 へ切替 → 段数は **1 のまま**。
  - そこで「元に戻す」相当を実行 → **slot1 のアイテム箱 186 枠が slot0 の内容に置換** (例: 枠 2109 `{id:2200, count:99}` → `{id:2381, count:99}`)。
- 影響: そのまま上書き保存すると別キャラの箱が恒久的に入れ替わる。`.bak` があるので直前 1 世代だけは救えるが、UI 上は何の警告も出ない。
- 修正案: `reload_from_save()` で `item_undo.clear()` と `charm_*` のリセットを行う。undo はスロット番号を持たせ、スロットが変わった履歴は捨てる。

#### P0-2 「Switch 用 (system 2 ファイル)」が既存セーブを確認・退避なしで上書きする
- 根拠: `app.rs:308-328`。`backup_before_write` を呼ばず、`std::fs::write(dir.join("system"))` / `system_backup` を直接書く。フォルダ選択 (`pick_folder`) に上書き確認はない。
- 実測 [D1]: `system`/`system_backup` が既にあるフォルダへ出力 → **`.bak` は作られず**、両ファイルとも置換 (`.bak made=false`)。
- 影響: 出力先に実機セーブのフォルダ (= Ryujinx/実機 SD の `.../0/system`) を選ぶと、`system` と `system_backup` の**両方**が上書きされ、このツールには復元手段がない。上書き保存だけが `.bak` を作る設計なので、危険度の非対称がそのまま事故になる。
- 修正案: 出力前に「既存の system / system_backup を上書きします」と対象パスを出して確認し、既存ファイルには必ず `.bak` を付ける。可能なら一時ファイル + rename でアトミックに。
- 関連 (P1-13): 片方だけ失敗しても「書き出しに失敗しました」としか出さず (`app.rs:326`)、**どちらが失敗したか・片方だけ新しくなったか**が分からない。

### P1 — 明確なバグ / 使い勝手

#### P1-1 キャラ欄の編集が dirty に乗らず、スロット切替で無言消失する
- 根拠: `app.rs:1041` (名前)、`app.rs:1054` (所持金)、`app.rs:1058-1071` (HR/ポイント) 等は `self.edit` を直接書き換えるだけで `dirty` を立てない。切替側は `app.rs:979-981` の `if self.dirty { self.apply_to_save(); }` なので、dirty=false のときはフラッシュされない。
- 実測 [B]: 所持金を 9,997,199 → 4,242,424 に変更 → `dirty = false`。そのまま slot1 → slot0 と切替 → 保存側の所持金は **9,997,199 のまま**。画面下は「○ 変更なし」。
- 影響: 最頻の操作 (キャラ編集) が、タブ移動では残るがスロット切替・`import_3ds_body`・`import_slot` で消える。UI は「変更なし」と言っているので気づけない。
- 修正案: キャラ欄のどのウィジェットでも `changed()` で dirty を立てる。もしくは切替時に常に `apply_to_save()` を呼び、dirty は「保存内容と差分があるか」で計算する (二重適用は無害)。

#### P1-2 .arc のエントリ切替で .mib / .gmd の編集が無言消失する
- 根拠: `app.rs:537-544`。`let prev = self.arc_selected;` のあと `if self.arc.is_some() && prev != index` でしかフラッシュしない。しかし呼び出し側 4 箇所 (`app.rs:397-398`, `439-440`, `1531-1532`, `main.rs:123-124`) は **すべて先に `self.arc_selected = i` してから** `select_arc_entry(i)` を呼ぶため、`prev == index` になりフラッシュは **一度も実行されない**。続く `self.mib_bytes.clear()` (`app.rs:547`) で編集は破棄される。
- 実測 [A] (本物の app.rs): .mib の 0x11 を 0x0A→0x11 に編集 → 別エントリへ切替 → .arc 側は **0x0A のまま (LOST)**。.gmd の文字列も同様に LOST。
- 影響: クエストのフィールドを直して別エントリ (テキスト等) を確認し、戻って「反映」→ 修正が消えている。UI は何も言わない。
- 修正案: `select_arc_entry` 側で切替前の index を保持できるよう、UI からは「`arc_selected` を書き換えずに `select_arc_entry(i)` だけ呼ぶ」に統一する (メソッド内で `arc_selected = index` を設定)。または切替前に明示的に `store_arc_entry(prev)` を呼ぶ。

#### P1-3 .mib のフィールド編集は保存に載らないのに dirty が立ち、上書き保存で「未保存」が消える
- 根拠: `app.rs:1577-1685` のフィールド編集は `mib_bytes` を書くだけで dirty を立てない。一方 16 進欄だけは `app.rs:1745` で `dirty = true`。`write_current` は `.arc` を保存しないのに成功時に `dirty = false` (`app.rs:292`)。
- 実測 [E]: セーブ内クエスト 1010005 を開き `mib_bytes[0x11]` を +3、`dirty=true` にして上書き保存 → 保存後 `dirty=false`、しかし書き出したファイルの該当 .mib バイトは **0x0A のまま (変化なし)**。
- 影響: 「未保存」が消えるので保存されたと誤解する。クエスト編集は `write_arc_to_save` (「セーブに書き戻す」) を押さない限りファイルに載らない。dirty の意味が「セーブ」と「.arc バッファ」で混ざっている。
- 修正案: dirty をセーブ用と .arc 用に分ける (`.arc 未反映` 表示)。少なくとも 16 進欄で dirty を立てるのはやめ、クエスト側に独立したインジケータを出す。

#### P1-4 狩猟記録が Switch 2 ファイル / 3DS 書き出しに載らない + 出力後も dirty が残る
- 根拠: `write_current` だけが `apply_hunts()` を呼ぶ (`app.rs:287`)。`write_switch_pair` (`app.rs:308-328`) と `export_3ds` (`app.rs:331-338`) は `apply_to_save()` のみで、`dirty` も落とさない。
- 実測 [D1][D2]: hunt[0] を 24→35 に編集 → Switch 2 ファイル出力は **24 (DROPPED)**、3DS 書き出しも **24 (DROPPED)**。どちらも出力後の `dirty=true`。上書き保存 (`write_current`) だけは 35 で kept、dirty=false ([D3])。
- 影響: 「狩猟記録を直して Switch 用に出力」が無言で失敗する。直前の 1 コミットで直したのと同じ種類のバグが別経路に残っている。
- 修正案: 3 経路すべてで `apply_hunts()` を呼び、成功時に dirty を落とす。

#### P1-5 終了・別セーブの読み込みに未保存の確認がない
- 根拠: `app.rs:184-197` (`open_save` は確認なしで置換し `dirty=false`)、`app.rs:718-745` の `update` に `close_requested` の処理なし (grep で `close_requested|on_exit` は 0 件、`ViewportCommand::Close` はスクショ用の 1 件のみ)。
- 影響: 未適用の編集 (キャラ・クエスト) を持ったまま×で閉じると全部消える。上の P1-1/P1-3 と合わせて「気づかない消失」の温床。
- 修正案: `dirty` または「バッファに未反映あり」のとき、閉じる/開き直す前に確認 (egui の modal)。

#### P1-6 クエストチェッカの「修正」ボタンが自分の修正を打ち消す
- 根拠: `app.rs:2069-2098`。`check::clear_one_player_only(a, 0x00)` で `entry.data[0x1A]` を 0 にした直後に `self.store_arc_entry(self.arc_selected)` を呼ぶ。`store_arc_entry` (`app.rs:566-582`) は `mib_bytes` (古い 0x2F のまま) を `entry.data` にコピーするので、`e.data` が元に戻る。直後の `run_quest_check` は「直っていない」arc を再検査する。
- 実測: 0x2F を持つ .arc は今回の素材に無かったため、`q1010001.mib` の 0x1A を 0x2F にした合成 arc で同じ順序を再現 (`.work\review_gui2\bench\src\bin\repro.rs`) → `old=0x2F, after_clear=0x00, after_store=0x2F, findings 3 -> 3`、メッセージは「0x2F から 0x00 に直しました」。実セーブ 175 件 / romfs 223 件には 0x2F は無かった (到達頻度は低いが、Quedixx 産クエストでは起きる)。
- 修正案: 修正の前に `store_arc_entry` でバッファを流し、修正後に `select_arc_entry(self.arc_selected)` で `mib_bytes` を作り直す (または core 側で修正後の raw を返して `mib_bytes` にコピーする)。

#### P1-7 取り返しのつかない削除に確認がない (お守りタブとは非対称)
- 根拠: DLC タブの「空にする」は 1 クリックで `dlc::uninstall_quest` (`app.rs:2672-2674`, `2688-2699`)。`remove_dlc` (`app.rs:701-713`) には「元に戻せない」旨のメッセージがあるのに **UI から呼ばれていない** (デッドコード)。設定/オトモのお守り「削除」も 1 クリック (`app.rs:3149-3151`)。
- 対照: お守りタブは 2 段階確認 `削除 → 削除する?` (`app.rs:2820-2829`)、重複除去はメニューで「実行」 (`app.rs:1159-1179`) と、作者はこのパターンを知っている。
- 影響: DLC は「枠を空にすると導入フラグが残り、ツールからは戻せない」と自分で書いている (`app.rs:695-713`)。1 クリックの誤操作が恒久損失。お守り削除も undo なし。
- 修正案: どちらも 2 段階確認に。少なくとも DLC は `remove_dlc` 経由に統一する。

#### P1-8 レイアウト破綻 (画面外・はみ出し・スクロール不能)
実測スクリーンショット (既定 1180×740 logical / 125% DPI):
1. **アイテムのツールバー右端で「アイテムを追加」が切れる** (`app.rs:1141-1198` は `ui.horizontal` で折り返さない)。`items_toolbar_zoom.png` では右端に「アイテ」だけ見え、ボタンは押せない。既定サイズで既に切れており、最小 900 幅ではさらに悪化する。→ `horizontal_wrapped` にするか、頻度の低いボタンをメニューへ。
2. **オトモの表が右端で切れる** (`app.rs:2195-2261`)。修得スキル列が途中で切れて残りは読めず、`ScrollArea::vertical` は横スクロールしないので到達不能。副作用として**名前欄が 3 文字幅**しかない (`app.rs:2212` の `desired_width(140)` が効いていない; `Grid` の列幅計算と推測)。また、ヘッダを含め 6 列なのに `num_columns(4)` (`app.rs:2199`) と不一致。→ 修得スキルは詳細ペイン/折り返しへ逃がし、`TableBuilder` か `add_sized` で列幅を固定。
3. **設定/オトモ (Extras) とマイセット (Sets) に外側スクロールが無い** (`app.rs:3025-3296`, `app.rs:2120-2180`)。既定サイズでは Extras の内容が約 622 logical px で空き 638 px に辛うじて収まるが、高さ 600 に縮めると「キャラクタをまるごと入出力」が丸ごと画面外 (スクロール手段なし)。Sets は非空セットが 20+20 になると確実に溢れる。→ `ScrollArea::vertical` で包む。

#### P1-9 「名前を付けて保存」でヘッダのファイル名ハッシュが古いまま
- 根拠: `app.rs:282-305` は `s.to_bytes()` を書くだけ。`Save::refresh_header` / `header_name` (`crates/mhnx-core/src/save.rs:849,859`) はどこからも呼ばれない (grep 0 件)。
- 実測 [G]: 元ファイル (名前 `system`) を `renamed_system_backup` として保存 → ヘッダ +0x14 は `0x36B2EE74` (crc32("system")) のまま。新しい名前なら `0x3A804A5C` であるべき。`write_switch_pair` は system=`0x36B2EE74` / system_backup=`0xD0819B92` と正しく作り分けている。
- 影響: ヘッダ +0x14 はファイル名の CRC。実機/エミュが名前で照合する場合、`system_backup` として置いたファイルが不正になる可能性 (ゲーム側の挙動は未検証 = 推測)。少なくともツール自身のモデルと矛盾。
- 修正案: 保存先のファイル名で `refresh_header(name)` を呼ぶ。

#### P1-10 抽出先が空だとカレントディレクトリへ書き出す
- 根拠: `dlc_outdir` の初期値は空 (`app.rs:155`)、`app.rs:2614-2621` はそのまま `extract_dlc(&dir, &[])` へ。`app.rs:661-665` は `create_dir_all(outdir)` を無条件に呼ぶ。
- 実測: `std::fs::create_dir_all("")` は `Ok(())`、`Path::new("").join("q.arc")` は相対 `q.arc` (rustc で確認)。つまり 175 ファイルがプロセスの CWD (Explorer 起動なら exe のある `dist\`、ショートカットなら任意) に落ち、ステータスは「175 件を  に書き出しました」。
- 影響: 意図しない場所への大量書き込み。前回レビュー (`docs/re_review_gui.md` E2) の指摘が未修正。
- 修正案: 空ならフォルダ選択ダイアログを出し、キャンセルなら何もしない。

#### P1-11 DLC タブが 175 件で 8.7 ms/フレーム (スクロールが重い)
- 根拠: `app.rs:2641-2678` は**全クエスト行**について毎フレーム `Arc::parse` + `Gmd::from_bytes` でタイトルを復元する (仮想化なしの `Grid`)。さらに `app.rs:2600-2610` のクエスト装備表が行ごとに `quest_title_of` (`app.rs:2103-2118`) を呼び、その中で `dlc::list_quests` をやり直す (O(行×クエスト)、データのコピー込み)。
- 実測 (release, 175 件): `list_quests + 全件タイトル復元` = **6.3–6.6 ms**、`quest_title_of × 40 行` = **2.3–2.6 ms**、`list_quests` 単体 = 0.13 ms。合計 ~8.7 ms/フレーム。60 fps の予算 16.7 ms の半分以上をデータ処理が占める。
- 影響: DLC タブのスクロールがカクつく (データ処理だけで 16.7 ms の半分以上; 描画を足すと 60 fps を割り得る)。件数が増えるほど線形に悪化。
- 修正案: タイトルを (save の同一性, quest_id) でキャッシュし、save が変わったときだけ再計算。装備表は最初に 1 回だけ id→title の Map を作る。

#### P1-12 MHGU セーブを警告なしで編集・保存できてしまう
- 根拠: サイドバーは `SaveKind::Mhgu` を「Switch (MHGU)」と表示する (`app.rs:926-930`) が、警告は DLC タブだけ (`app.rs:2546-2551`)。キャラ/アイテム/お守り/装備タブと上書き保存・Switch 出力はすべて有効なまま (`app.rs:799-827` は `has_save` だけで有効化)。`write_switch_pair` は 3DS だけを拒否 (`app.rs:311`)。MHGU のレイアウトは MHXX と異なるため、書き込みはそのまま破損になり得る (推測ではなく、DLC タブ自ら「MHGU はセクション配置が違うため扱わない」と書いている)。
- 影響: MHGU ユーザーが開いた瞬間に誘導される。読み取り専用にするか、強い警告 + 保存の無効化が必要。
- 修正案: MHGU では編集系を無効化し、少なくとも上部に赤い警告を常時表示。

#### P1-13 書き込み経路の安全性 (バックアップ失敗が無言 / 非アトミック)
- 根拠: `backup_before_write` (`app.rs:267-280`) は `Err(_) => None` で失敗を捨て、呼び出し側 (`app.rs:289-301`) は成功メッセージしか出さない。書き込みは `std::fs::write` (`app.rs:290`) で、失敗時は切り詰め後の部分ファイルが残り得る。
- 影響: 空き容量不足・権限・書き込み禁止媒体で「.bak なし + 中途半端な本体」になり得る。.bak がある通常ケースでは 1 世代戻せる (実測 [D3]) が、失敗が黙って通るのは危険。
- 修正案: バックアップ失敗時は書き込みを中止して明示。一時ファイル + rename で置換する。

### P2 — 改善

1. **サイドバーが全タブで毎フレーム 4.7 MB をコピー**: `app.rs:936` の `s.to_bytes().len()` はサイズ表示のためだけに毎フレーム 4,726,188 バイトを確保・コピーする。実測 **0.66–0.74 ms/フレーム** (全タブ共通)。長さは `header.len() + body.len()` で足りる。
2. **設定/オトモが 4.7 MB を毎フレーム clone**: `app.rs:3032` `self.save.as_ref().unwrap().clone()` 実測 **0.67–0.74 ms/フレーム**。借用分割か、必要な値だけコピーする。
3. **エラーが英語のまま/原因が消える**: core の `Error` Display は英語 (`crates/mhnx-core/src/lib.rs:85-95`) で「読み込み失敗: bad size: got 123 want 4726188」のように混在。`write_switch_pair` の失敗は `app.rs:326` で `io::Error` を捨て「書き出しに失敗しました」だけ。エラー色も無く、直後のメッセージで上書きされる。
4. **アイテム名検索が 40 件で打ち切り** (`app.rs:1394`)。「の」で 1,876 件ヒットする語でも 40 件しか選べず、打ち切りの表示もない。前回レビュー E10 の未修正。
5. **エントリ件数表示が逆** (`app.rs:1497-1501`): `{エントリ数} / {選択+1}` の順で出るため、先頭を選ぶと「15 / 1」=「1 件中 15 件目」とも読める。前回 E7 の未修正。
6. **デッドコード**: `scan_quest_dir` / `install_dlc` / `remove_dlc` (`app.rs:597,613,701`) は未使用でコンパイラ警告 3 件。`dlc_files`/`dlc_log` も UI に出ない。「フォルダ内の .arc を一括インストール」という UI は無く、`install_dlc` は全件を無条件に入れる実装なので、そのままボタン化すると選択と確認が要る。`remove_dlc` は P1-7 の通り削除確認の受け皿として使うべき。
7. **装備タブは非仮想化 Grid**: `app.rs:2518-2537` は使用中 789 行 × 4 セルを毎フレーム生成 (実セーブ)。アイテム表 (`TableBuilder`) と違い全行を構築する。2,000 枠まで埋まると重い。アイテム表と同じ `TableBuilder` に寄せる。
8. **英語のままのモンスター名**: 狩猟記録 (`app.rs:2278-2314`) は `MONSTER_HUNT_NAMES` をそのまま出し「Rathian / Gold Rathian / id」と英語。UI 全体は日本語で、アイテム名も日本語なので浮く。
9. **アイテム個数の上限が 127** (`app.rs:1266, 1352, 1864`)。ゲームの実上限は 99 (`全アイテムを 99 個にする` ボタンもそう書いている)。127 を書けるので不正値を作れる。
10. **タブ名「設定/オトモ」が紛らわしい** (`app.rs:899`)。中身は本体オプション・配信オトモ・お守り・スロット入出力で、別タブの「オトモ」と名前がほぼ同じ。
11. **保存先の永続化・ショートカットなし**: ウィンドウサイズ/位置は毎回 1180×740 (`main.rs:46-53`)、Ctrl+O/Ctrl+S 等なし、ドラッグ&ドロップ非対応。
12. **マイセットの装備が生の ID 列** (`app.rs:2161-2172`): 「大剣 18 40 41 42 43 44 1967」で何の装備か分からない。アイテムセット側は名前を出しているので不統一。
13. **ギルドカードのプレイ時間がキャラ側と 7 秒ずれる** (実測: キャラ 2,409,628 秒 / カード 2,409,621 秒)。ゲームが別フィールドを持つのなら仕様、そうでなければどちらかの読み出しが誤り。データ担当への申し送り。
14. **お守りタブの一覧にスクロールバーが見えない** (188 件・460 px 固定)。動作はするが、スクロールできることが分かりにくい。
15. **本番バイナリに検証用 env フックが残る** (`main.rs:67-146`, 例 `MHNXEDITOR_SAVE_OUT` は起動時に書き込む)。テスト資産としては有用だが、リリースでは「明示フラグ」に切るのが望ましい。

---

## 良い点 (壊さないでほしい)

- **上書き保存のバックアップが実際に機能する** (実測): 既存ファイルへ保存すると `system.bak` が作られ、内容は直前のファイルと一致。2 回目の保存では `.bak` = 1 回目の出力 (1 世代前) になる。メッセージにも退避先が出る (`app.rs:294-301`)。
- **無変更の保存がバイト一致** (実測): `write_current` で書き出したファイルの SHA256 は元ファイルと同一。`apply_to_save`/`apply_hunts` を経由しても壊れない。
- **セーブ内クエストの編集ループが正しい** (実測 [F]): `open_arc_from_save` → .mib 1 バイト編集 → `write_arc_to_save` → 上書き保存で、書き出したセーブの該当 .mib だけが変わり、クエスト数 175 は不変。
- **デバッグビルドのクエストタブがパニックしない** (実測): `target\debug\mhnxeditor.exe` でディスクの .arc とセーブ内クエストの両方を撮影、exit 0 / PNG 生成。前回レビューの E1 (shift overflow) は修正済み。
- **クエストタブがスクロール可能になり、249 バイト 16 進欄と 2 つの反映ボタンに到達できる** (`app.rs:1435-1441`)。前回の U1 は解消。
- **アイテム表の仮想化** (`TableBuilder::body(.rows)`, `app.rs:1246-1280`)。2,242 行でもフレーム費用は軽い (実測 0.03 ms のデコード + 仮想化描画)。
- **アイテム undo (32 段) と重複除去の確認**: `push_item_undo` / `undo_item` (`app.rs:1287-1303`)、重複除去はメニューで「実行」 (`app.rs:1159-1179`)。「全アイテムを 99 個」も undo 可能。
- **お守りの 2 段階削除確認と、レア度・許容ポイントの検証表示** (`app.rs:2820-2829`, `2883-2931`)。「この種別では付けられません」まで出るのは親切。
- **日本語フォントのフォールバック**: 候補を順に試し、見つかったパスをステータスに出す (`main.rs:17-43`)。スクショで豆腐なしを確認。
- **ヘッドレス・スクリーンショット機構** (`main.rs:67-146`, `app.rs:754-781`): 全タブのスモークテストとしてそのまま資産になる。今回のレビューもこれで撮った。
- **DLC クエスト装備のカタログ表示 (40/45)** とタイトル復元、抽出・削除の導線。データの見える化として価値がある。
- **タブバーを上部へ分離し、サイドパネルをスクロール可能にした** (`app.rs:885-1012`)。前回の「サイドバーが画面を覆う」問題は解消。
- **スロット切替時に dirty なら apply する改善** (`app.rs:975-985`) と、`write_current` の `apply_hunts` (`app.rs:287`)。方向は正しい (網羅が足りないだけ)。
- **.arc 再構築が元の zlib ストリームと元オフセットを保持** (core `arc.rs:61-85, 227-259`)。未編集エントリはバイト一致で書き戻る。この設計は変えない方がよい。

---

## 優先して直すべき 3 件

1. **「元に戻す」のスコープをスロット内に閉じる (P0-1)**
   `reload_from_save` で `item_undo.clear()` / `charm_*` / `item_selected` をリセットし、undo に slot 番号を持たせて不一致なら捨てる。あわせてスロット切替時は常に `apply_to_save()` を通し、P1-1 の「dirty でないので流さない」経路を無くす。実測 186 枠の破壊が消える。

2. **Switch 用 2 ファイル出力の安全化 (P0-2 + P1-4 + P1-13)**
   書き出す前に既存 `system`/`system_backup` の有無を確認して対象パスを提示し、必ず `.bak` を作る。`apply_hunts()` を呼び、成功時に `dirty=false`。片方失敗時はどちらが失敗したかを出す。実セーブのフォルダを選んだ場合の全損リスクが消える。

3. **dirty / 未反映モデルの統一 (P1-1 / P1-2 / P1-3)**
   キャラ欄と .mib/.gmd の編集を dirty (または「未反映」) に乗せ、`.arc` のエントリ切替では必ず `store_arc_entry(prev)` が走るよう呼び出し順を直す (`select_arc_entry` 内で `arc_selected` を更新する)。上書き保存で「セーブ」と「クエスト」の未保存が混ざらないよう、インジケータを 2 本にする。ここが「編集が消える」系の共通根。

---

## 付録: 実測値一覧

```
[A] entries=15 mib=14 gmd=13
    mib 0x11 edited 0x0A -> 0x11; arc entry after switching away = 0x0A (LOST)
    gmd[0] 'USJ...' + "XYZ"; entry after switching = original (LOST)
[B] dirty after editing funds = false; funds 9997199 -> 9997199 (EDIT LOST)
[C] undo depth after add = 1 / after slot switch = 1
    slot1 item slots changed by undo = 186 (CROSS-SLOT OVERWRITE)
[D1] hunt[0]=35 write_switch_pair wrote 24 (DROPPED); dirty after=true; .bak made=false
[D2] hunt[0]=35 export_3ds wrote 24 (DROPPED); dirty after=true
[D3] hunt[0]=35 write_current wrote 35 (kept); dirty after=false
     .bak created on overwrite; after 2nd save .bak == 1st output
[E]  dirty after 上書き保存 = false; save .mib 0x11 10 -> 10 (QUEST EDIT NOT IN SAVE)
[F]  write_arc_to_save: mib 0x11 0x0A -> 0x0D in the written save (OK); quests 175 -> 175
[G]  save-as header 0x36B2EE74 (stale, crc32("system")); expected 0x3A804A5C
     pair headers: system 0x36B2EE74 / system_backup 0xD0819B92 (correct)

フレーム費用 (release, 175 クエスト, 実セーブ):
  side panel to_bytes().len()        0.66–0.74 ms
  extras save.clone()                0.67–0.74 ms
  DLC list_quests + title (175)      6.30–6.62 ms
  DLC quest_title_of x 40 rows       2.33–2.57 ms
  items filter 2300 names            0.22 ms (文字入力中のみ)
  charm summary + lowercase x188     0.086 ms
  sets / palico / equip              0.004 / 0.011 / 0.003 ms

保存経路:
  既存ファイルへ write_current → system.bak 生成、出力 SHA256 == 入力
  名前を付けて保存 → ヘッダ +0x14 は旧名のまま
  create_dir_all("") = Ok(()), Path::new("").join("q.arc") = 相対パス
```

スクリーンショット: `.work\review_gui2\shots\`
(`save/items/quest/dlc/charm/equip/hunts/guild/palico/sets/extras/about.png`、
`debug_quest_save.png`、`items_toolbar_zoom.png`、`palico_zoom.png`、`re_items.png`、`re_palico.png`)