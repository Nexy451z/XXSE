# Switch 実機版 (.nro) レビュー

対象: `switch/src/editor.rs`, `ui.rs`, `text.rs`, `app.rs`, `savedata.rs`, `sys.rs`, `swkbd.rs`, `switch/build.ps1`
手法: 静的解析 + 実ビルド + ホスト側での再現実験 + libnx ヘッダとの突き合わせ。**実機・エミュレータでの動作確認はしていない。**
プロジェクトのコードは一切変更していない（実験用ファイルは `.work/review_nro/` のみ）。
「推測」と明記した箇所以外は、この環境で実際にコマンドを走らせて得た実測値。

---

## 要約

**ビルドと FFI 層の品質は高い。危険なのは NAND 書き込みに至る経路の「入口」と「途中」。**

- ビルドは通る: `.\switch\build.ps1` -> `dist\mhnxeditor.nro` 1,726,153 B、NRO0 / MOD0 / HOMEBREW マーカーすべて検証済み。警告ゼロ。
- `sys.rs` / `text.rs` / `swkbd.rs` の FFI 宣言は **libnx ヘッダと完全一致**（構造体サイズ・オフセット・列挙値・関数シグネチャを devkitA64 gcc の `_Static_assert` で全数検証。詳細は「良い点」1）。
- 一方、**「3DS<->Switch 変換」画面から 4 回のボタン操作で本体セーブが破壊できる**（P0-1、実測で再現）。原因は `editor.rs:735` の `path.starts_with("nand:")` という広すぎる分岐と、`write_console_save` にサイズ・形式の検証が一切ないこと。
- NAND 書き込みは 256 KB ごとに commit する多段コミットで、**失敗時に巻き戻す手段がない**（P0-3）。

---

## 重大度別の問題

### P0（データ破壊・必須機能の不全）

#### P0-1. 変換画面から本体セーブを 3DS 形式で上書きできる（実測で再現）

再現手順をホスト上で実際に走らせた。`mhnx-core` の `Editor` をそのまま駆動し、Switch 形式の実セーブ（`jksv_ready\system`, 4,726,188 B）を開いてボタン 3 回:

    after open              : screen=Slots save_path=Some("nand:/system")
    after B                 : screen=SaveList
    after L                 : screen=Convert
    after A (convert)       : message="writing 3DS (header stripped) -> nand:/system.3ds"
    ---- WRITE REQUEST ----
      path   = "nand:/system.3ds"
      len    = 4726152 bytes
      path.starts_with("nand:") : true

- `crates/mhnx-core/src/hbedit.rs:1502-1537` `convert_now()`: `split_dir("nand:/system")` = `nand:/` なので出力先が **`nand:/system.3ds`** になる（`hbedit.rs:128-133`）。中身は `to_3ds_bytes()` = 36 バイトヘッダを剥いだ 4,726,152 B。
- `switch/src/editor.rs:735`: `if path == CONSOLE_SAVE || path.starts_with("nand:") { write_console_save(&bytes) }` — **`nand:` で始まるパスはすべて NAND への書き込みとして扱われる**。つまり `nand:/system.3ds` は `save:/system` に書かれる。
- `switch/src/savedata.rs:266` は `fopen(path, "r+b")` で開く。**"r+b" は切り詰めない。** ホストで同じ手順（4,726,188 B のファイルに 4,726,152 B を 16 KiB ずつ上書き）を実行した実測:

    file size before  : 4726188
    bytes written     : 4726152
    file size after   : 4726188 (unchanged - "r+b" never truncates)
    stale tail bytes  : 36  first byte = 0xaa

結果、`save:/system` は **「4,726,152 B の 3DS ボディ + 末尾 36 B の旧データ」** というハイブリッドになり、MHXX はロードできない。仮に `r+b` が失敗して `wb` にフォールバックしても（`savedata.rs:268`）、4,726,152 B の 3DS セーブが本体に置かれるだけで同じく読めない。

- UI 側に警告はない。`switch/src/ui.rs:3384-3395` は `出力ファイル名 : system.3ds (元パス: nand:/system)` と表示し、ボタンは `switch/src/ui.rs:3419` の `[A] 変換して書き出す`。**出力が NAND 本体セーブを上書きすることを示す文言がどこにもない。**
- 救いになっている偶然: `savedata.rs:176-187` は `old_bak.len() == bytes.len()` のときしか `system_backup` を更新しない。ここでは 4,726,188 != 4,726,152 なので **ゲーム自身のバックアップは無傷で残る**。ただし MHXX がそこから復旧できるかは未検証。

#### P0-2. NAND 書き込みにサイズ・形式の検証が無い

- `switch/src/editor.rs:353-370` `write_console_save()` は受け取った `bytes` をそのまま書く。**`looks_like_save()` も `SaveKind` も呼んでいない。** `looks_like_save()`（`savedata.rs:350-352`, 4,726,188 固定）は読み込み時（`editor.rs:275`）にしか使われない。
- `write_save_in_place()`（`savedata.rs:256-341`）は `bytes.len()` までしか書かず、ファイルを切り詰めない。したがって **短いバイト列を渡すと必ず「先頭だけ新しい壊れたファイル」が完成する。** P0-1 はこの一般則の一例にすぎない。
- 対策の方向: `write_console_save` の先頭で `bytes.len() == 4_726_188 && bytes[36] == 0xC6` を検証し、不一致なら書かずにエラーを返す。加えて `editor.rs:735` の `starts_with("nand:")` は `== CONSOLE_SAVE` に限定する（将来 `nand:` 系の出力先が増えたときに同じ事故が起きる）。

#### P0-3. NAND 書き込みが非アトミックで、失敗時に巻き戻せない

- `savedata.rs:304-319`: `uncommitted >= 256 KB` ごとに `fclose` -> `fsdevCommitDevice` -> 再 `fopen`。フル書き換え（4,726,188 B）では **18 回の durable point** ができる。途中の commit が失敗するか電源が落ちると、セーブは「先頭 256 KBxn が新、残りが旧」という永久に混ざった状態になる。ロールバック処理は無い（`Err` を返して終わり）。
- 差分的書き込み（数ブロックだけ変更）の場合は commit が 1 回で済むため、**通常の編集操作はこの問題の影響を受けない**。危険なのは「`old` が読めなかった（`editor.rs:359` の `mount.read().ok()` が `None`）」または「長さが変わった」ケース。
- 参考: libnx の fsdev は通常「全部書いてから 1 回 commit」で使われる。256 KB 刻みにした根拠はコメント（`savedata.rs:253-255`）に「journal 溢れ」とあるが、**journal 容量の根拠は不明（推測）**。少なくとも原子性は明確に下がっている。

---

### P1（使い勝手・明確なバグ）

#### P1-1. 安全バックアップの成否を握り潰して「控え: ...」と表示する

`switch/src/editor.rs:362-364`:

    if let Some(old) = old.as_ref() {
        let _ = write_file(&backup_path, old);   // 戻り値 bool を捨てる
        prune_old_backups(10);
    }

`switch/src/editor.rs:739-742` は成否に関係なく `本体のセーブに N バイト書き込みました（控え: {path}）` を出す。**SD が満杯・挿さっていない・書き込み禁止でも、ユーザーにはバックアップが取れたように見える。** さらに `write_file()`（`editor.rs:113-123`）は `fclose()` の戻り値も見ていないため、フラッシュ時のエラーは拾えない。NAND 書き込みはその後で実行されるので、「バックアップが無いまま上書き」が普通に成立する。

#### P1-2. 安全バックアップのファイル名が秒精度の `time()` 依存

`savedata.rs:344-347` の `stamp()` は `time(NULL)` をそのまま 10 進表記にしたもの。`editor.rs:361` で `system_{stamp}.bak` に使う。

- **同じ秒に 2 回保存すると、先のバックアップが上書きされる。** 連続で保存し直したとき、本当に必要な「1 回前の状態」が消える。
- `time()` が有効な値を返さない環境では全バックアップが同名になり 1 個しか残らない。**推測**: 最終 ELF には `libc.a` の `_gettimeofday_r` だけが現れ、`_gettimeofday` 本体はシンボルとして見えない。実機で実時刻が入るかは未確認。名前は連番（`system_0001.bak` 等）にした方が確実。
- `prune_old_backups`（`editor.rs:383`）は名前を文字列ソートする。10 桁固定なら数値順と一致するので実害は無い（推測）。

#### P1-3. panic しても何も表示されず、無限ループで固まる

`switch/src/app.rs:40-50` `print_panic()` は `consoleUpdate(NULL)` + `svcSleepThread` を永久に繰り返す。しかし通常経路（Canvas が取れた場合）は `editor.rs:840-843` の分岐で **`consoleInit()` を呼んでいない**ため、panic メッセージはどこにも描かれない。SD へのログ出力も無い。

- 裏返すと「無限ループで死ぬ」ことは `Mount::drop` を走らせないので、**セーブの観点ではむしろ安全側**（未 commit のバッファが flush されない）。どちらを意図しているか明文化すべき。
- 推奨: panic 時に `/switch/mhnxeditor/crash.log` を書く、もしくは `consoleInit` を遅延初期化してから表示する。

#### P1-4. `system_backup` の更新失敗が握り潰される

`savedata.rs:184`: `let _ = write_save_in_place(SAVE_BACKUP_FILE, &bak_bytes, Some(&old_bak));`
メインの `save:/system` は成功したのに `system_backup` だけ失敗した場合でも `write_diff` は `Ok` を返す。実測で確認した「ペアの実体」から見ると、これは実在する不整合状態:

    hdr system         ... 74 ee b2 36 ...   (name_hash("system")        = 0x36B2EE74)
    hdr system_backup  ... 92 9b 81 d0 ...   (name_hash("system_backup") = 0xD0819B92)
    differences after header (>=36): 0        <- ボディは完全一致

つまり 2 ファイルは **0x14 の 4 バイトだけが違う同一ボディ**でなければならない。片方だけ新しくなるとこの不変条件が崩れる。ゲーム側がどう扱うかは未検証だが、`Err` を返さないのは危険。

---

### P2（改善）

| # | 箇所 | 内容 |
|---|---|---|
| P2-1 | `ui.rs:213` | `let mut entries = [FsDirectoryEntry::ZERO; 32];` = **32 x 784 B = 25,088 B をスタックに置く**（sizeof は実測 784）。呼び出しは `run -> scan_saves/scan_quests/prune_old_backups -> list_dir` の浅い経路なので今すぐ溢れるとは言えないが、プログラム中で最大のスタックオブジェクト。`Vec` にすれば消える。参考: 死んでいる `app.rs:216` は 16 個 = 12,544 B。**スタック上限が 64 KiB 前後という前提は推測**。 |
| P2-2 | `ui.rs:4667` | `...clamp(24, track_h)` は `track_h < 24` で **panic する**（`clamp` は min > max で panic）。現状 `track_h = CONTENT_H - 92 = 432` 固定なので到達しないが、レイアウト定数を変えると描画パスが panic -> P1-3 の無限ループに落ちる。`clamp(24, track_h.max(24))` が安全。 |
| P2-3 | `ui.rs:4750` | `const fn white_or_dark(_c: Color) -> Color { rgb(255,255,255) }` — 引数が完全に無視されている。`ACCENT_GREEN`(46,198,126) のピルに白文字でコントラストが低い。 |
| P2-4 | `ui.rs:4702-4744` | フッターのチップは描画した**後**に `if x > w - 80 { break }` を判定するので、最後の 1 個が右端をはみ出す。`rect`/`text` はクリップするのでクラッシュはしないが、文字が切れる。 |
| P2-5 | `ui.rs:66-75, 1171-1189` | レイアウト定数は 1280x720 固定、キャンバスサイズは `nwindowGetDimensions` 由来。**タッチのヒットテストは安全**（レイアウトも libnx のタッチ座標も同じ 1280x720 空間なのでずれない）が、ウィンドウサイズが変わるとレイアウトはスケールせず見切れるだけ。 |
| P2-6 | `ui.rs:629-642` | OSK を開いていて `mib_picker` が無いとき、コンテンツ領域のドラッグが背面リストをスクロールしてしまう。 |
| P2-7 | `editor.rs:98-108` | `read_file` は 12 MiB を超えると `break` して**切り詰めた内容を `Some` で返す**（エラーにしない）。セーブは 4.7 MiB なので実害は無いが、12 MiB 超の `.arc` を無言で切り詰めてセーブに注入しうる。 |
| P2-8 | `editor.rs:746` | `ed.take_request();` は直前の `write_console_save` と無関係な残骸で、保留中のリクエストを捨てるだけ。 |
| P2-9 | `savedata.rs:325` | `fsdevUnmountDevice` を `-> Result (u32)` と宣言しているが、ヘッダは `int fsdevUnmountDevice(const char*)`。AArch64 では w0 読みなので実害は無いが不一致。戻り値も未チェック。 |
| P2-10 | `savedata.rs:170-173` | `save:/system.new` を削除して commit する処理。このコードは `.new` を作らないので、過去の「リネームで原子的に書く」設計の残骸と思われる。無害だが意図が読めない。 |
| P2-11 | `app.rs:53-95` | `return crate::editor::run();` の後ろに SD プローブ一式（約 140 行）が `#[allow(unreachable_code)]` で残っている。`banner`/`probe_sdcard`/`list_directory` は到達不能。 |
| P2-12 | `editor.rs:831` | `sys::fsInitialize()` の戻り値を捨てている。失敗すると `list_dir` が空を返し、`Mount::open` は無関係なコードで失敗するので原因が追いにくい。 |
| P2-13 | `.cargo/config.toml:2` vs `build.rs:18` | **実測**: この環境は `DEVKITPRO=/opt/devkitpro` が設定済みで、素の `cargo +nightly build --release --target targets/...json` は `build.rs:99` で panic する（`devkitA64 gcc not found at /opt/devkitpro\\devkitA64\\...`）。`$env:DEVKITPRO='C:/devkitPro'` を渡すと EXIT=0。`.cargo/config.toml` の「plain cargo build で十分」というコメントは誤りで、パス変換をしているのは `build.ps1:67-98` だけ。 |
| P2-14 | `crates/mhnx-core/Cargo.toml:9` | **実測**: `default = ["std"]` だけではコンパイルできない（`crate::gamedata` を無条件参照: `dlc.rs:157`, `myset.rs:204`、`bit_for_quest` も feature gate 漏れ）。`.nro` 側は `features = ["gamedata"]` を明示しているので影響なし。 |

---

## 良い点（壊さないでほしい）

1. **FFI 層が実ヘッダと完全一致している（実測）.** `sys.rs` / `text.rs` / `swkbd.rs` が主張する構造体サイズ・オフセット・列挙値・関数シグネチャを、devkitA64 gcc の `_Static_assert` で全部書き写してコンパイルした。**全項目パス**（`Service`=16, `FsDirectoryEntry`=784/offset 772,776, `PadState`=56/16,24,32, `HidTouchScreenState`=656, `PlFontData`=24, `Framebuffer`=80, `SwkbdArgCommon`=980, `SwkbdConfig`=1264/0x408-0x4EC, `SwkbdCustomizedDictionarySet`=206 packed, `SwkbdType_Normal`=0, `AppletType_Application`=0 ほか）。`fsFileRead`, `fsDirRead`, `fsdevMountSaveData`, `framebufferCreate`, `swkbdShow` 等の関数ポインタ代入も通った。`sys.rs:73-110,161-166,236-243` と `swkbd.rs:212-258` の `const _: () = { assert!(...) }` はこの品質を保つ仕組みとして秀逸。**消さないこと。**
2. **36 バイトヘッダの生成が実ファイルとバイト一致する（実測）.** `mhnx-core/src/save.rs:192-200` の `switch_header` を実セーブと突き合わせた: offset 0x08=4, 0x10=0x14, 0x18=0x0C, 0x1C=0x00481D88(=4,726,152), 0x14=`74 EE B2 36`(=`name_hash("system")`), `system_backup` 側 0x14=`92 9B 81 D0`。**全フィールド一致。**
3. **「system / system_backup のペア」不変条件を正しく実装している.** 実測で 2 ファイルは 0x14 の 4 バイト以外完全同一。`savedata.rs:176-187` は本体を書いた後にバックアップのヘッダを生成し直してボディを揃えている。これが無いとペアが壊れるので、**この処理は残すべき**（失敗を握り潰す点だけ直す）。
4. **`Mount` の RAII 化.** `savedata.rs:79-82, 204-213`。早期 return でも `Drop` が `fsdevUnmountDevice` を保証する。`mounted` フラグを落とすので二重 unmount もない。
5. **バックアップはメモリ上の編集前データではなく、書き込み直前に読み直した実データ.** `editor.rs:359` で `mount.read()` し直してから SD に保存している。1 時間前のロード時の状態ではなく「今ディスクにある状態」を控えるので、差分書き込みの基準としても正しい。
6. **エラーに libnx の Result コードとステップ名が入る.** `savedata.rs:34-65`。`IoDetail("fwrite", 0x...)}` のように失敗箇所が特定できる。実機でしか出ない不具合の切り分けに効く。
7. **ホットパスに `unwrap()` が無い.** `switch/src` 全体で `unwrap()` は 1 箇所（`savedata.rs:283`）だけで、しかも `can_diff = old.map(|o| o.len()==bytes.len())` でガード済み。`expect`/`panic!`/`unreachable!` はゼロ。
8. **描画のクリップと飽和演算が徹底されている.** `text.rs:584-620` の `rect` は `x.max(0)`〜`saturating_add(w).min(width)`、`blend_glyph`（`text.rs:994-1036`）はインデックス前に `g.pixels.len() >= w*h` を検証、テキストのペン送りは `saturating_add`。`put_pixel_blend` も毎回境界チェック。フレームバッファへの直接書き込みでここまで守っているのは良い。
9. **フォントハンドルの二重解放を実際に避けている.** `text.rs:310-334, 948-967`。3 サイズの生成に失敗したとき同じハンドルを 3 スロットが指すので、`seen[..i].contains()` で重複を弾く。正しい。
10. **`swkbd` の UTF-16 境界での切り詰め.** `swkbd.rs:342-356`。libnx の `utf8_to_utf16` が「必要な単位数」を返し、設定関数がその位置に NUL を書くため、境界を間違えると固定長配列の外に書かれる。`ch.len_utf16()` で数えて切る実装は正しく、理由もコメントに書かれている。
11. **Applet モードのフォールバック.** `swkbd.rs:374-379` の `is_available()` -> `editor.rs:808-817` で内蔵 OSK に切り替え。アルバム起動でハングしない。
12. **`build.ps1` が devkitPro パスを解決してからリンクする.** `build.ps1:67-98` は `/opt/devkitpro` 形式を全ドライブで試す。リンク後に NRO コンテナ（マジック・mod0・サイズ）まで検証しているのも良い（`build.ps1:183-199`）。
13. **描画は必要なときだけ.** `editor.rs:863, 991-994` の `need_draw` 制御で、待機中は 60 Hz スリープのみ。バッテリーに優しい。
14. **画面ごとに押せるキーをチップで出す.** `ui.rs:4755-4921`。`editor.rs:503-576` と対になっていて、実機でキーを忘れても操作できる。

---

## 優先して直すべき 3 件

### 1. 変換画面からの NAND 上書きを塞ぎ、`write_console_save` に形式検証を入れる（P0-1 / P0-2）

- `editor.rs:735` の `path.starts_with("nand:")` を `path == CONSOLE_SAVE` に限定する。
- `write_console_save`（`editor.rs:353`）の先頭で、`bytes.len() == mhnx_core::save::SWITCH_SIZE`（4,726,188）かつ `bytes[36] == 0xC6` を検証し、外れたら**書かずに**エラーメッセージを返す。
- 併せて `ui.rs` の変換画面に「この操作は本体セーブを上書きしません」と明示する（現状 `nand:/system` を開いていると実際には上書きする）。
- 回帰テストはホスト側で可能: `Editor` を `open_save_bytes("nand:/system", ...)` -> `B` -> `L` -> `A` と駆動し、`take_write()` の `path` が `nand:/system.3ds` にならないことを assert する（本レビューで使った手順がそのままテストになる）。

### 2. NAND 書き込みを「戻せる」ようにする（P0-3 / P1-1 / P1-4）

- SD の安全バックアップは**成否を確認**し、失敗したら NAND に書かずに中断する（少なくともメッセージを「控えの作成に失敗しました」に変える）。`editor.rs:363` の `let _ =` を消す。`write_file` は `fclose()` の戻り値も見る。
- `write_diff` で `system_backup` の更新が失敗したら `Err` を返す（`savedata.rs:184`）。
- 256 KB 刻みの commit をやめ、フル書き換え時も commit を 1 回に寄せられないか検討する。少なくとも「多段 commit 中に失敗したら、SD のバックアップから自動で書き戻す」経路を持つ。
- 検証: `savedata.rs` の書き込みループをホストに移植し、途中で `fwrite` が失敗するスタブを挿して「壊れた状態が残る」ことを再現するテストを 1 本置く。

### 3. 失敗を見えるようにする（P1-2 / P1-3 / P1-4）

- バックアップ名を `time()` 依存から連番に変える（`system_0001.bak` 〜）。同名上書きで「1 回前の状態」が消える事故を防ぐ。
- panic を SD のログに残す（`/switch/mhnxeditor/crash.log`）。少なくとも `print_panic` の無限ループは意図を明記し、CPU を回し続けない形にする。
- `system_backup` 更新失敗・`fclose` 失敗をステータス行に出す。ユーザーが「保存できたつもり」で電源を切るのが最悪のケース。

---

## 付録: 実施した検証

| 内容 | 結果 |
|---|---|
| `.\switch\build.ps1` | 成功。`dist\mhnxeditor.nro` 1,726,153 B、NRO0 / MOD0 検証 OK |
| `cargo +nightly build --release --target targets/aarch64-nintendo-switch-libnx.json`（`cargo clean -p mhnx-switch` 後） | 成功、**警告 0 件** |
| libnx レイアウト全数プローブ（devkitA64 gcc `_Static_assert`） | 全項目一致 |
| `Editor` を駆動した変換パス再現 | `nand:/system.3ds`, 4,726,152 B を生成することを確認 |
| `fopen("r+b")` 上書きの切り詰め挙動 | 4,726,188 B のまま、末尾 36 B が旧データで残る |
| `system` vs `system_backup` の差分 | 全長 4,726,188 B、差分は 0x14 の 4 バイトのみ |
| `switch_header` の実ファイル一致 | 0x08/0x10/0x14/0x18/0x1C すべて一致 |
| 環境 `DEVKITPRO=/opt/devkitpro` での素の `cargo build` | `build.rs:99` で panic（`build.ps1` 経由なら成功） |
| `mhnx-core` を default features のみでビルド | 失敗（`gamedata` 必須） |

実験用ファイルは `C:\anyWorkspace\nx\mhxx\mhnxeditor\.work\review_nro\` に置いた（`layout_probe.c`, `trunc_demo.js`, `fixture.js`, `probe/`）。プロジェクトのソースは未変更。
