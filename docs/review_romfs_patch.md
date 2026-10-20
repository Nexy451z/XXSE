# MHXX (Nintendo Switch) romfs パッチ（LayeredFS）で村クエを差し替えられるか

対象: `C:\anyWorkspace\nx\mhxx`
作業: 2026-10-20 / 担当: romfs パッチ調査
関連: `docs/review_questloader.md`（クエストローダ解析）, `docs/quest_format_spec.md`（.arc/.mib/.rem/.sup 形式）, `NOTES.md`（タイトル ID・既存 MOD の経緯）

> 表記: **【事実】** = 本調査で実測・一次資料で直接確認した / **【推定】** = 実測からの推論 / **【未確定】** = 判断材料不足
> 本調査は**読み取りのみ**。romfs・セーブ・ソースは変更していない（生成物は `mhnxeditor/.work/romfspatch/` 配下のみ）。

---

## 0. 結論サマリ（依頼 6 項目への回答）

| # | 依頼項目 | 到達点 | 根拠 |
|---|---|---|---|
| 1 | 正しい LayeredFS 配置パスとタイトル ID | **確定**。`sd:/atmosphere/contents/0100C3800049C000/romfs/nativeNX/arc/quest/q0000101.arc`。MHXX はベース `0100C3800049C000` / 更新 `0100C3800049C800` だが、**LayeredFS が使うのはベース側プログラム ID 1 本**（更新 ID のフォルダは参照されない） | §1 |
| 2 | ゲームは村クエを個別弧／集約弧のどちらから読むか | **個別弧が本体、集約弧は一覧用の冗長コピー**（**推定・強**）。決定的な静的命令列は出せず（文字列→コードの静的交差参照は本作では不可能）、**論証は「資源の所在」による**。差し替え対象は**個別 `q%07d.arc` で足りる**見込み。**名称/依頼文/条件まで変えるなら `vq.arc` も要る** | §2 |
| 3 | 集約 .arc の再梱包手順と制約 | **`mhnx arc-repack` は使える**。vq/gq/tq で往復一致（976/1168/602 エントリ、サイズ同一）。**ただし 4 バイトだけ書き換わる**（+0x08 が `0xFFFFFFFE` → `0x00000000`）。サイズ・アラインメントの制約は「データ開始 = `alignUp(12+80n, 0x8000)`」のみで、**エントリ間の境界アラインメントは無い**（実測で非 4 倍数オフセット `0x800F` が現存） | §3 |
| 4 | 個別 .arc のどこを変えれば報酬・支給品・モンスターが変わるか | **確定**。.qdl（15 スロット）が「どの .rem/.sup/.sem/.esl を使うか」の対応表。**報酬 = slots 8-12 の .rem / 支給品 = slot 13 の .sup / 大型 = slots 0-4 の .sem / 小型 = slots 5-7 の .esl** | §4 |
| 5 | 実際に試す手順 | 手順化した（バックアップ → 配置 → 起動確認 → 失敗時の戻し）。**エミュ（Ryujinx）で先に検証 → 実機**の順を推奨 | §5 |
| 6 | 公式アップデートがある状態の優先順位 | 更新 NCA は **section0=ExeFS / section1=Patch RomFS(BKTR)**（実測）。LayeredFS は**両方の上に被さる**ので、更新があっても MOD は勝つ。ただし **exefs MOD（既存 MHNX）は版が合わないと危険** | §6 |

**一行で**: romfs 差し替えは**技術的に成立する**（形式・ツール・配置パスすべて確認済み）。残る不確実性は「**個別弧を読んでいるという直接証明**」と「**差し替え後の実機挙動**」の 2 点で、どちらも**実機/エミュでの 1 クエスト検証**で潰せる。

---

## 1. タイトル ID と LayeredFS の配置パス

### 1.1 MHXX の ID【事実】

| 項目 | 値 | 根拠 |
|---|---|---|
| ベース タイトル ID | `0100C3800049C000` | NSP ファイル名 / ベース NCA の ProgID |
| 更新 タイトル ID | `0100C3800049C800` | NSP ファイル名 / Rights ID `0100C3800049C8000000000000000003`（`nstool` 実測） |
| 更新の版 | `393216 (0x60000) = v1.5.1` | `out_update/manifest.json` |
| **プログラム ID（重要）** | **`0100C3800049C000`** | `nstool` が更新 NCA ヘッダから読んだ `ProgID: 0x0100c3800049c000` |
| 更新 ID の作り方 | ベース ID + `0x800` | 上 2 行の差分（`...C000` → `...C800`）と一致 |

**ここが要点**: 更新タイトル ID（`...C800`）は**タイトル**の ID であって**プログラム**の ID ではない。更新 NCA 自身が `ProgID = 0100C3800049C000` を宣言している（実測）。そして後述の通り Atmosphère の romfs リダイレクトは **プログラム ID** で SD パスを組み立てる。

### 1.2 Atmosphère のパス生成規則【事実】

一次資料: [stratosphere/ams_mitm/source/amsmitm_fs_utils.cpp](https://github.com/Atmosphere-NX/Atmosphere/blob/master/stratosphere/ams_mitm/source/amsmitm_fs_utils.cpp)

```cpp
void FormatAtmosphereSdPath(char *dst_path, size_t dst_path_size, ncm::ProgramId program_id, const char *subdir, const char *src_path) {
    ...
    util::SNPrintf(dst_path, dst_path_size, "/atmosphere/contents/%016lx/%s/%s", static_cast<u64>(program_id), subdir, src_path);
}
void FormatAtmosphereRomfsPath(char *dst, size_t n, ncm::ProgramId program_id, const char *src_path) {
    return FormatAtmosphereSdPath(dst, n, program_id, "romfs", src_path);
}
```

→ 実パスは **`/atmosphere/contents/<プログラムID(16桁hex)>/romfs/<romfs 内の相対パス>`**。

* 代替形式として **`/atmosphere/contents/<ID>/romfs.bin`**（romfs イメージ 1 枚）も読まれる（`HasSdRomfsContent` が `romfs.bin` と `romfs/` の両方を検査）。ディレクトリを触る方が安全なので本件では使わない。
* サブディレクトリ名は `romfs` 固定。フォルダ名は小文字 hex（`%016lx`）。

### 1.3 MHXX の romfs 内パス【事実】

`extract/base_romfs` の実測:

```
nativeNX/arc/quest/q0000101.arc      <- 個別クエスト 1,855 本
nativeNX/arc/quest/vq.arc            <- 集約（村）976 エントリ
nativeNX/arc/quest/gq.arc            <- 集約（集会所）1,168
nativeNX/arc/quest/gq2.arc           <- 集約 460
nativeNX/arc/quest/tq.arc            <- 集約 602
```

（romfs 直下は `nativeNX` ただ 1 つ。ゲーム側の論理パス `arc\quest\q%07d` は実行時にこの `nativeNX/` ルートへ解決される。）

### 1.4 したがって配置パスはこうなる【確定】

```
sd:/atmosphere/contents/0100C3800049C000/romfs/nativeNX/arc/quest/q0000101.arc
```

* **`0100C3800049C800`（更新 ID）側は作らない**。ソース上 `FormatAtmosphereRomfsPath` に渡るのは `m_client_info.program_id`（= 起動プロセスのプログラム ID）1 本だけで、**更新タイトル ID を参照する分岐は存在しない**（`fs_mitm_service.cpp` の全 romfs 系メソッドが `m_client_info.program_id` か引数の `program_id` を使用）。
* MOD の**有効/無効ゲート**も同じ ID で判定される: `override_status.IsProgramSpecific()` と `HasSdRomfsContent(program_id)`。つまりフォルダ名を更新 ID にすると**そもそも mitm が発動しない**（＝無効）。**【推定・強】**
* **【未確定】**: 実機の Atmosphère バージョンによって細部が変わりうる。初回だけ `0100C3800049C800` 側にも同一ツリーを置いて切り分けるのは可（害はない）。

### 1.5 なぜ「更新があると配置先が変わる」と言われるのか【推定】

コミュニティで「更新があるゲームは更新側のタイトル ID に置け」と言われるのは、**exefs MOD（コード差し替え）**の文脈と混同されやすいためと考えられる。romfs については「更新 NCA がパッチ RomFS を持ち、更新側のファイルがベースを上書きする」ことが理由で更新 ID が話題になるが、**Atmosphère の LayeredFS はその合成結果のさらに上に乗る**（§6.2）。本件では**ベース ID 一択**でよい。

---

## 2. ゲームは村クエを「個別弧」と「集約弧」のどちらから読むのか

### 2.1 文字列の実在（生バイナリから再実測）【事実】

`out_update/image/main.bin` を直接走査した結果（VA = ファイルオフセット + `0x100`）:

| VA | 文字列 | 意味 |
|---|---|---|
| `0x0154B0F6` | `quest\q%07d` | **個別弧のパス書式**（例 → `quest\q0000101`） |
| `0x0154B102` | `quest\gq` | 集約（集会所） |
| `0x0154B10B` | `quest\vq` | **集約（村）** |
| `0x0154B114` | `quest\gq2` | 集約 |
| `0x0154B11E` | `quest\tq` | 集約 |
| `0x0154646B` | `arc\` | ルート接頭辞 |
| `0x01546470` | `.arc` | 拡張子（**パス文字列に含まれず、後から付く**） |
| `0x01550C30` | `quest\boss\setEmMain\` | 弧の**内部**パス（大型モンスター） |
| `0x01550C46` | `quest\zako\emSetList\` | 弧の内部パス（小型） |
| `0x01550C5C` | `quest\rem\` | 弧の内部パス（報酬） |
| `0x01550C67` | `quest\supp\` | 弧の内部パス（支給品） |
| `0x01550C73` | `quest\plus\questPlus_` | 弧の内部パス（Plus） |
| `0x01550C15` | `quest\questData\questData_` | 弧の内部パス（.mib/.gmd） |
| `0x01557CA6` | `quest\questLink\questLink_%07d` | 弧の内部パス（.qdl） |
| `0x01546486` | `quest\questData\questData_%07d_%s` | 言語つき書式（`%s` に `jpn`＝`0x015464A8`） |

**重要な観察【事実】**: `quest\q%07d` / `quest\vq` / `quest\gq` / `quest\gq2` / `quest\tq` の **5 本は文字列プール上で連続している**（`0x154B0F6`〜`0x154B11E`）。つまり「コンテナ名を選ぶ 1 つのコード領域」が存在し、**クエスト種別ごとに集約弧を、クエスト ID ごとに個別弧を、同じローダに渡している**。

### 2.2 静的交差参照は不可能（再確認）【事実】

上記 8 文字列＋`.arc`＋`eventQuestList.bin` について、**イメージ内の 4 バイト絶対ポインタ参照を全数走査した結果、ベース・更新の両方で 0 件**だった。

```
quest\q%07d            0x0154B0F6  ptr-refs=0
quest\gq               0x0154B102  ptr-refs=0
quest\vq               0x0154B10B  ptr-refs=0
quest\gq2              0x0154B114  ptr-refs=0
quest\tq               0x0154B11E  ptr-refs=0
.arc                   0x01546470  ptr-refs=0
eventQuestList.bin     0x01546441  ptr-refs=0
```

`docs/review_questloader.md` §2 の結論（MT Framework の文字列はハッシュ表経由で登録され、リテラル参照が無い）と一致する。**「どの関数が `quest\vq` を開くか」を静的に示すことは本作ではできない。** よって以下は**資源の所在からの論証**になる。

### 2.3 集約弧と個別弧の中身【事実】

`mhnx arc-list` の実測:

| ファイル | エントリ | クエスト数 | 中身 | データ開始 |
|---|---|---|---|---|
| `q0000101.arc` | 12 | 1 | boss/zako/rem×4/supp/plus/questLink/questData×2 | `0x8000` |
| `vq.arc` | 976 | 488 | **`quest/questData/questData_*` の .mib と _jpn.gmd のみ** | `0x18000` |
| `gq.arc` | 1,168 | 584 | 同上 | `0x18000` |
| `gq2.arc` | 460 | 230 | 同上 | `0x10000` |
| `tq.arc` | 602 | 301 | 同上 | `0x10000` |

**決定的な非対称**: 集約弧には **`.rem` / `.sup` / `.sem` / `.esl` / `.qdp` / `.qdl` が 1 つも入っていない**。一方、クエストを 1 本プレイするには
`questLink_<id>.qdl`（15 スロットの参照表・`docs/quest_format_spec.md` §4）が**必須**で、これは**個別弧にしか存在しない**。

→ **集約弧だけではクエストは成立しない。** 村クエを受注した時点で、ゲームは必ず個別弧を読まねばならない。

### 2.4 ID 集合の関係【事実】

| 集合 | 件数 |
|---|---|
| 個別弧（`q%07d.arc`）総数 | **1,855** |
| 集約弧のユニーク ID | **1,603**（488 + 584 + 230 + 301、**重複 0**） |
| 集約に**無い**個別弧 | **252** |
| 個別弧に**無い**集約 ID | **0**（＝ 集約 ⊆ 個別） |

集約に無い 252 本の内訳（先頭 3 桁で集計）: `101`:137, `102`:30, `111`:43, `112`:13（配信クエ帯）, `000`:9, `001`:20。

集約ごとの ID 帯:

| 弧 | ID 帯（先頭 3 桁: 件数） |
|---|---|
| `vq`（村） | `000`:335, `003`:105, `010`:40, `013`:8 |
| `gq`（集会所） | `001`:313, `002`:11, `004`:228, `011`:26, `012`:6 |
| `gq2` | `001`:215, `011`:15 |
| `tq` | `009`:301 |

**村クエ（`00001xx`〜`00010xx`）は `vq.arc` に入っている**（個別弧 `q0000101.arc` と対）。なお .mib/.gmd の内容は個別弧と集約弧で**バイト一致**（既存検証 1,603/1,603）。

### 2.5 「既存の実例」の確認結果【事実】

| 場所 | 中身 | 判定 |
|---|---|---|
| `mhxx/mods_disabled/` | `exefs_MHNX_mod/main.npdm`(1,516 B) + `subsdk9`(987,302 B) のみ | **exefs MOD。romfs MOD の実例は無い** |
| `mhxx/existing_mod/` | `subsdk9.bin` + `subsdk9_strings.txt`（解析用コピー） | 同上 |
| Ryujinx `mods/contents/0100c3800049c000/` | 既に削除済み（NOTES §12） | — |

→ **「このゲームの romfs MOD が実機で動いた」というローカル実例は存在しない。** §2 の結論は資源構造からの論証であり、実機裏取りは未実施。

### 2.6 結論（と、その確度）

| 主張 | 確度 | 根拠 |
|---|---|---|
| ゲームは**個別弧を必ず読む** | **推定・強** | .qdl が個別弧にしか無い / 集約弧は questData のみ（§2.3） |
| **報酬・支給品・モンスターの差し替えは個別弧で足りる** | **推定・強** | 当該資源が個別弧にしか存在しない（§2.3） |
| 集約弧 `vq.arc` は**一覧表示用の冗長コピー** | **推定** | 「488 本ぶんの .mib/.gmd だけを持つ弧」が存在する理由が一覧 UI 以外に説明しにくい / 個別弧とバイト一致 / 弧名が種別ごとに 4 本 |
| 集約弧を差し替えても**プレイ内容は変わらない** | **未確定** | 集約にプレイ用資源が無い以上、変わらないと考えるのが自然。ただし実測していない |
| 個別弧だけ差し替えると**一覧の名前・説明は古いまま** | **推定** | 上と表裏。名称を揃えるなら `vq.arc` 側の .gmd も差し替える |

**「決定的証拠」が出せない理由を明記**: 本作は文字列→コードの静的交差参照が成立しない（§2.2、既存レポート §2 と同一結論）。実行時トレース（受注直前にどのファイルが開かれるか）が唯一の直接証明で、これは**エミュレータのファイルアクセスログ**で取れる（§5.2）。

---

## 3. .arc の再梱包：`arc-repack` は使えるか

### 3.1 実測結果【事実】

| 入力 | サイズ | 出力 | 往復検証 |
|---|---|---|---|
| `q0000101.arc` | 34,445 B | 34,445 B | 12/12 エントリ一致 |
| `vq.arc` | 325,588 B | 325,588 B | 976/976 一致 |
| `gq.arc` | 395,156 B | 395,156 B | 1,168/1,168 一致 |
| `tq.arc` | 113,372 B | 113,372 B | 602/602 一致 |

**サイズは 1 バイトも変わらない。** しかし**バイト単位では一致しない**:

```
q0000101.arc : sizes 34445 / 34445   differing bytes: 4
  runs: [0x8..0xb]   A=feffffff   B=00000000
vq.arc       : sizes 325588 / 325588 differing bytes: 4
  runs: [0x8..0xb]   A=feffffff   B=00000000
```

→ **`+0x08` の u32 が `0xFFFFFFFE` → `0x00000000` に落ちる。** romfs の全 1,855 本が `0xFFFFFFFE`、セーブ内 DLC 弧（141 本）は `0`（`quest_format_spec.md` §1.2）なので、**フィールド自体はゲームが無視している可能性が高い**が、**確証は無い**。

**対策（推奨）**: 再梱包後に 4 バイトを書き戻す。実害が無く、原本と同じ値になる。

```powershell
# $p = 再梱包した .arc
$b = [IO.File]::ReadAllBytes($p)
$b[8]=0xFE; $b[9]=0xFF; $b[10]=0xFF; $b[11]=0xFF
[IO.File]::WriteAllBytes($p, $b)
```

（Rust 側を使う場合は `mhnx-core/src/arc.rs` の `to_bytes` が `+0x08` を保存しない実装なので、同じ後処理が要る。将来的には `Arc` に `flags` フィールドを足すのが本筋。）

### 3.2 サイズ・アラインメントの制約【事実】

* **データ開始位置の規則**: `dataStart = alignUp(12 + 80 × entryCount, 0x8000)`
  * 個別弧: 全 1,855 本が `0x8000`（12+80×12=972 → 0x8000 / 17 エントリでも 0x8000）
  * 集約弧: vq/gq が `0x18000`（12+80×976=78,092 → 0x18000）、gq2/tq が `0x10000`
  * **セーブ内 DLC 弧は別規則**（`align32`、`quest_format_spec.md` §1.4）。同じ内容が 2 形態で存在する。
* **エントリ間のアラインメント制約は無い**: オフセットはエントリごとの生 u32 で、実測に `0x8000`（32 B の .sem）→ `0x800F`（= 0x8000 + 15、直前の圧縮サイズ）のような**非 4 倍数**が現存する。つまり「隙間なく積む」だけでよい。
* **合計サイズの上限**: 公式の個別弧は 33,036〜35,924 B（1,855 本）。理論上は 0x8000 ヘッダ＋任意長。**ゲーム側が固定バッファに読む保証は無い**ので、**大幅に増やさない**のが安全（§7）。
* **圧縮**: エントリは zlib。`arc-repack` は「未変更エントリは元の圧縮ストリームをそのまま再利用」するので、無変更部分は 1 バイトも変わらない（`arc.rs` の `original`/`original_offset`）。**変更したエントリだけ再圧縮**される。

### 3.3 集約 .arc を差し替える手順（必要な場合）

1. `mhnx arc-repack vq.arc vq.mod.arc` で往復（この時点で中身は同じ）
2. 差し替えたいエントリ（`quest/questData/questData_0000101_jpn.gmd` 等）のペイロードを編集して再構築
   * **CLI には「弧の中の 1 エントリを書き換える」コマンドが無い**（`arc-list`/`arc-mib`/`arc-gmd`/`arc-repack` のみ）
   * 手段は 2 つ: (a) `crates/mhnx-core`（`Arc::parse` → `entries[i].data` を書換 → `Arc::to_bytes`）を呼ぶ小さな Rust バイナリを書く / (b) Python で zlib を直接扱う（§4.4 に実測済み手順）
3. `+0x08` を `0xFFFFFFFE` に戻す
4. `mhnx arc-list` と `mhnx check` で自己検証（エントリ数・サイズ・支給品サマリが変わること）

---

## 4. 個別 .arc のどこを変えれば内容が変わるか

### 4.1 .qdl が「対応表」【事実】

`questLink_<id>.qdl` は **308 バイト固定 = 8 + 15 × 20**。各レコード = `{u32 typeHash, char[16] name}`（`quest_format_spec.md` §4）:

| slot | 意味 | 参照するエントリ | typeHash |
|---|---|---|---|
| 0-4 | 大型モンスター 1〜5 | `quest/boss/setEmMain/<name>` | `0x2553701D` |
| 5-7 | 小型モンスター 1〜3 | `quest/zako/emSetList/<name>` | `0x32CA92F8` |
| **8-9** | **メイン報酬 1〜2** | `quest/rem/<name>` | `0x5B3C302D` |
| **10-11** | **特別報酬 1〜2** | `quest/rem/<name>` | `0x5B3C302D` |
| **12** | **サブ報酬** | `quest/rem/<name>` | `0x5B3C302D` |
| **13** | **支給品** | `quest/supp/supp_<id>` | `0x54539AEE` |
| 14 | Plus | `quest/plus/questPlus_<n>` | `0x26BEC21C` |

**「どのエントリを変えるか」の答え**:

| 変えたいもの | 触るエントリ | レコード形式 |
|---|---|---|
| **報酬（アイテム・個数・確率）** | `quest/rem/rem_*.rem`（slots 8-12 が指すもの） | 184 B = 8 B ヘッダ + 16 B 表 + **40 × 4 B**（`u16 item_id / u8 count / u8 prob`）、レコード先頭は **+0x18** |
| **支給品** | `quest/supp/supp_<id>.sup` | 168 B = 8 B ヘッダ + **40 × 4 B**（`u16 item_id / u8 count / u8 flag`）、レコード先頭は **+0x08** |
| **大型モンスターの初期配置** | `quest/boss/setEmMain/*.sem`（32 B: area, rot, x, z, y） | 32 B 固定 |
| **小型モンスターの出現** | `quest/zako/emSetList/*.esl`（ヘッダ 64 B + ESD チャンク） | 可変（64 B の空も多い） |
| **クエスト名・依頼文** | `quest/questData/questData_<id>_jpn.gmd` | 文字列テーブル |
| **レベル・制限時間・参加条件・フラグ** | `quest/questData/questData_<id>.mib` | 249 B 固定 |
| **クエストそのものの差し替え**（別クエの資源を使わせる） | `.qdl` の name を書き換え **＋ その名前のエントリを弧に追加** | 名前は 16 B NUL 埋め |

**注意【事実】**: 弧には**ダミー資源**が入る。`q0000101.arc` の実例:

```
0  quest/boss/setEmMain/b_m00em000_00.sem     32 B    <- ダミー（大型なし）
1  quest/zako/emSetList/z_m01d_034.esl      1398 B    <- 実データ（小型）
2  quest/zako/emSetList/z_m00d_000.esl        64 B    <- ダミー
3  quest/rem/rem_210052.rem                  184 B
4  quest/rem/rem_210001.rem                  184 B
5  quest/rem/rem_000000.rem                  184 B    <- ダミー
6  quest/rem/rem_210051.rem                  184 B
7  quest/supp/supp_0000101.sup               168 B    <- 支給品 18 種 / 計 147 個
8  quest/plus/questPlus_0000.qdp              25 B
9  quest/questLink/questLink_0000101.qdl     308 B
10 quest/questData/questData_0000101_jpn.gmd 576 B
11 quest/questData/questData_0000101.mib     249 B
```

`mhnx check` も `rem_000000 はダミーファイルです。ゲームは読み込みません` と明示する。**ダミーを編集しても効果は無い**。どの .rem が「メイン報酬」かは **.qdl の slots 8-12 を読んで決める**（ファイル名から推測しない）。

### 4.2 村クエの実データ例【事実】

`q0000101.arc` の `supp_0000101.sup` の中身（先頭 10 レコード）:

```
(1715, 1), (1716, 3), (1718, 3), (1720, 2), (70, 2),
(101, 30), (106, 10), (213, 15), (207, 10), (1719, 1)   ... 以降 0 埋め
=> 18 種類 / 合計 147 個
```

`mhnx check` の出力と一致（`支給品 18 種類 / 合計 147 個`）。

### 4.3 触ってはいけない／慎重にすべき点

* **エントリ名は変えない**【事実】: 公式データに「名前と中身の ID が食い違う弧」が現存する（`q1011045.arc`、既存レポート §4.1）。ゲームは名前と ID の一致を検証していない。
* **item_id は 0〜2990**【事実】: `itemData.itm` の件数 2,991 とコードの `movw r1,#0xBAE` で確定。**範囲外の値の挙動は未確定**（明示チェックが見つかっていない）。
* **.rem の「個数」は u8 生値**で上限 10 ではない（公式に 99 が存在）。
* **弧を丸ごと別クエと入れ替える**（例: `q0000102.arc` を `q0000101.arc` の場所に置く）のは**動く可能性が高いが非推奨**。理由: (a) 弧内の .mib の `+0x0C` にクエスト ID が入っており、受注 ID と食い違う、(b) §2.6 の通り一覧側（`vq.arc`）の名前と不一致になる。**まずは「同じ弧の中で資源だけ差し替える」**のが安全。

### 4.4 実測した改造手順（Python・動作確認済み）【事実】

`.work/romfspatch/mkmod.py` で実行した内容:

1. `.arc` をパース（`12 + 80×n` のエントリ表、`+0x40` typeHash / `+0x44` 圧縮サイズ / `+0x48` フラグ / `+0x4C` オフセット）
2. `quest/supp/supp_0000101.sup` を zlib 展開し、先頭レコードを `(1715,1,0) → (1,9,0)` に変更
3. **変更したエントリだけ再圧縮**し、他は元の圧縮ストリームを流用して再構築（データ開始は元の `0x8000`、`+0x08` は `0xFFFFFFFE` を維持）
4. 検証結果:

```
wrote q0000101.modded.arc 34445 bytes (was 34445)
reparse ok: count=12 field08=0xFFFFFFFE dataStart=0x8000
sup roundtrip first rec: (1, 9, 0)
mhnx arc-list : 12 entries, version 17       <- 正常
mhnx check    : 支給品 18 種類 / 合計 155 個   <- 147 - 1 + 9 = 155（編集が反映）
```

**サイズを変えずに 1 レコードを差し替えられることを確認した。** これが MOD の最小単位。

---

## 5. 実際に試す手順

### 5.0 前提

* LayeredFS は **Atmosphère 標準機能**（別途 sysmodule 不要。`atmosphere/contents/` を置くだけで有効）。
* 本作は **32bit ARM (AArch32)** ビルド（`NOTES.md` §2）。romfs 差し替えはコードに触らないので ABI の心配は無い。
* **セーブは触らない**。クエスト差し替えは romfs のみで完結する。

### 5.1 バックアップ（最初にやる）

```powershell
cd C:\anyWorkspace\nx\mhxx
# 1) 原本の保全（改造前の基準として固定）
Copy-Item 'extract\base_romfs\nativeNX\arc\quest\q0000101.arc' `
          '.work\romfspatch\backup\q0000101.orig.arc' -Force

# 2) SD の既存 atmosphere/contents を丸ごと退避（他ゲームの MOD を壊さない）
#    $SD = "E:\" など、実際のドライブに読み替える
Copy-Item "$SD\atmosphere\contents" "$SD\atmosphere\contents.bak_20261020" -Recurse
```

### 5.2 先にエミュレータで確かめる（推奨・低リスク）

Ryujinx は Atmosphère と同じ `contents/<tid>/romfs` 規約を使う（**【推定】**。実装差がありうるので、まずログで読み込みパスを確認する）。

```powershell
# MOD ディレクトリ（ベース ID 側）
$mod = "$env:APPDATA\Ryujinx\mods\contents\0100c3800049c000\romfs\nativeNX\arc\quest"
New-Item -ItemType Directory -Force -Path $mod | Out-Null
Copy-Item '.work\romfspatch\out\q0000101.modded.arc' "$mod\q0000101.arc" -Force
```

確認手順:

1. Ryujinx を**再起動**（MOD は起動時に評価される）
2. MHXX を起動 → **村クエ 1★ の該当クエスト**（`q0000101`）を受注
3. **支給品ボックスを開き、先頭が「回復薬 ×9」になっているか**を見る（§4.4 の改造内容）
4. 期待通りなら **romfs 差し替えが効いている**ことが実証される（＝ §2.6 の「推定・強」が確定に変わる）

> エミュで効かなければ、`mods\contents\0100c3800049c800\...`（更新 ID 側）も試して切り分ける。Ryujinx のログに読み込みパスが出る場合はそれで確定させる。

### 5.3 実機（Atmosphère）で試す

```powershell
# $SD = "E:\"（SD カードのドライブ）
$dst = "$SD\atmosphere\contents\0100C3800049C000\romfs\nativeNX\arc\quest"
New-Item -ItemType Directory -Force -Path $dst | Out-Null
Copy-Item '.work\romfspatch\out\q0000101.modded.arc' "$dst\q0000101.arc" -Force
```

1. SD を挿して起動（Atmosphère 起動確認）
2. MHXX 起動 → 該当の村クエを受注 → 支給品を確認
3. 併せて**クエスト一覧の名前・説明が変わっていないこと**を確認（`vq.arc` を触っていないので、§2.6 の予測どおり「一覧は元のまま」になるはず。ここが予測と違えば、集約弧の役割について仮説を修正する材料になる）

### 5.4 失敗時の戻し方

| 症状 | 対処 |
|---|---|
| 起動しない／該当クエでフリーズ | `$dst\q0000101.arc` を**削除**するだけ。LayeredFS は「SD に無ければ元の romfs を使う」ので、削除で完全に元へ戻る（**ゲームファイルは無傷**） |
| 支給品が変わらない | パス誤り（大文字小文字・`nativeNX` の有無・ID 違い）を疑う。`romfs/` の綴り、`0100C3800049C000` か `...C800` かを切り分け |
| 他ゲームの MOD まで壊れた | §5.1 の `contents.bak_*` を戻す |
| 起動時に Atmosphère が落ちる | `atmosphere/contents` を一旦リネームして起動 → 原因フォルダを二分探索 |

**原則**: romfs MOD は**追加ファイル**なので、消せば必ず元に戻る。セーブにも影響しない。バックアップは「他 MOD を壊さない」「改造前の基準を残す」ために取る。

---

## 6. 公式アップデート（v1.5.1）がある状態での優先順位

### 6.1 更新 NCA の構造【事実】

`nstool` による `out_update/program.nca` の実測:

```
Content Type:    Program          Size: 0x3B02800 (61,997,056 B)
ProgID:          0x0100c3800049c000
RightsId:        0100C3800049C8000000000000000003
Key Generation:  3 / SdkAddon 4.4.1
[WARNING] NCA Partition 1 not readable. (Base NCA not supplied. Necessary for update NCA.)
```

* **Partition 0 = ExeFS**（読み取り成功。`main`/`rtld`/`sdk`/`subsdk0` を展開済み）
* **Partition 1 = Patch RomFS (BKTR)**。ベース NCA を渡さないと読めない、という警告がそのまま証拠。**更新は romfs のパッチも配っている**。
* **【未確定】更新パッチ romfs の中身**: ベース NCA（10.5 GB）の title key がローカルに無い（`extract/upd/title.keys` には更新の `0100C3800049C800...0003` の 1 行のみ）。したがって `--basenca` を与えても復号できず列挙できなかった。**村クエの個別弧が更新側にも入っているかは不明**。

### 6.2 LayeredFS と更新 romfs の関係【事実（コード）】

LayeredFS は「ゲームの romfs を丸ごと作り直し、SD のファイルを最優先で差し込む」実装になっている。初期化順序は [fsmitm_layered_romfs_storage.cpp](https://github.com/Atmosphere-NX/Atmosphere/blob/master/stratosphere/ams_mitm/source/fs_mitm/fsmitm_layered_romfs_storage.cpp):

```cpp
void LayeredRomfsStorageImpl::InitializeImpl() {
    romfs::Builder builder(m_program_id);
    if (mitm::IsInitialized())     { builder.AddSdFiles(); }                                    // (1) SD
    if (m_file_romfs)              { builder.AddStorageFiles(m_file_romfs.get(), File); }        // (2) romfs.bin
    if (m_storage_romfs)           { builder.AddStorageFiles(m_storage_romfs.get(), Storage); }  // (3) 実 romfs
    builder.Build(...);
}
```

そして [fsmitm_romfs.cpp](https://github.com/Atmosphere-NX/Atmosphere/blob/master/stratosphere/ams_mitm/source/fs_mitm/fsmitm_romfs.cpp) の `Builder::AddFile`:

```cpp
if (m_files.find(file_ctx) != m_files.end()) { return; }   // 先に入ったものが勝つ
```

→ **(1) SD が最優先**。(2)(3) は「同じパスが既にあるなら無視」される。

**結論**: 更新 NCA がパッチ romfs を持っていても、**SD のファイルはその上に載る**。更新で差し替わった村クエでも、MOD 側が勝つ。逆に言えば「更新で中身が変わったかどうか」を気にせず MOD を作れる（MOD は弧を丸ごと置き換えるため自己完結している）。

### 6.3 更新がある場合の注意点

| 項目 | 内容 | 確度 |
|---|---|---|
| 配置先 | **ベース ID `0100C3800049C000`**。更新 ID 側は参照されない | 【事実（コード）】 |
| 更新 romfs との競合 | SD が勝つので競合しない | 【事実（コード）】 |
| **exefs MOD** | 既存の `mods_disabled/exefs_MHNX_mod`（`main.npdm` 1,516 B = **ベース v0 サイズ**）は**v1.5.1 と不整合**。romfs 検証中は**無効のまま**にしておく（`NOTES.md` §12 の退避状態を維持） | 【事実（サイズ比較）／推定（危険度）】 |
| 更新の再インストール | SD の MOD は消えない。ゲーム内容が v1.5.1 に戻るだけ | 【推定】 |
| 更新パッチ romfs の内容 | 未列挙。**村クエが更新側にも入っている可能性は否定できない**（入っていても §6.2 により影響なし） | 【未確定】 |
| オンライン | 改造クエでのオンラインプレイは**BAN 対象**。オフライン限定にすること | 【運用】 |

### 6.4 「更新側にも同じ弧がある」場合に何が起きるか【推定】

もし更新パッチ romfs に `nativeNX/arc/quest/q0000101.arc` が入っていたとしても:

* MOD 無し → 更新側の弧が使われる（ベース側は無視）
* MOD あり → **SD の弧が使われる**（§6.2）
* したがって「MOD を作る基準にした弧がベース v0 のもの」でも問題ない（弧は自己完結。他ファイルを参照しない）

---

## 7. リスク

| # | リスク | 影響 | 対策 |
|---|---|---|---|
| R1 | **個別弧を読んでいるという直接証明が無い** | 差し替えても無反応の可能性（低いと考えるが 0 ではない） | §5.2 のエミュ検証で最初に潰す。無反応なら集約弧側を疑う |
| R2 | **ゲームが弧を固定サイズバッファに読む可能性** | サイズを大きく増やすと破綻しうる | 改造は「サイズを変えない」範囲で行う（§4.4 の実測はサイズ不変）。増やす場合も公式最大 35,924 B を超えない |
| R3 | **`+0x08` フィールドの書き換わり** | `arc-repack` で `0xFFFFFFFE` → `0`。ゲームが無視する保証が無い | 再梱包後に 4 バイト書き戻す（§3.1） |
| R4 | **名称・説明が一覧と食い違う** | 表示の不整合（クラッシュではない見込み） | §2.6。必要なら `vq.arc` 側の .gmd も差し替える |
| R5 | **item_id 範囲外（>2990）** | 不明（明示チェックが見つかっていない） | 0〜2990 に収める。試すなら 1 件ずつ |
| R6 | **exefs MOD との干渉** | 起動不能・挙動異常 | 検証中は exefs MOD を無効化（現状は既に退避済み） |
| R7 | **オンライン利用** | BAN | オフライン専用 |
| R8 | 弧のエントリを増減させる | ゲームが 15 スロット固定で読む（.qdl）ため、**.qdl に載っていない資源を足しても使われない**。エントリ数自体の増減は未検証 | エントリ数は変えない（資源の中身だけ変える） |
| R9 | セーブの DLC 枠との混同 | セーブ枠は 0x1400 ストライド・上限 0x13F8 で**romfs とは別物** | romfs 改造ではセーブを触らない |

---

## 8. 未確定事項と次の一手

| # | 未確定 | 確かめ方 |
|---|---|---|
| U1 | **村クエが個別弧から読まれるという直接証拠** | Ryujinx のログで `q0000101.arc` が開かれた瞬間を捕捉。または実機で §5.2 の支給品差し替えを確認 |
| U2 | **`vq.arc` の役割**（一覧 UI 専用か） | `vq.arc` の `.gmd` だけを書き換えて、クエスト一覧の名前が変わるかを見る（個別弧を触らない） |
| U3 | **ゲームが弧を読むときのバッファ戦略** | 弧のサイズを意図的に +10 KB して起動・受注できるか |
| U4 | **`+0x08` の意味** | `0xFFFFFFFE` と `0` の両方で受注できるか（セーブ内 DLC 弧は 0 で成立しているので、0 でも通る可能性が高い） |
| U5 | **更新パッチ romfs の中身** | ベース NSP の ticket から title key を取り、`nstool --basenca out_base/program.nca --fstree` で列挙 |
| U6 | 村以外（`gq`/`gq2`）の Group 分割の意味 | ID 帯の対応（§2.4）から `0011xxx` が `gq2` に寄っている等の傾向は見えるが、ゲーム内の呼称は未確認 |
| U7 | Ryujinx の MOD 解決がベース ID か更新 ID か | Ryujinx のログ／`mods` 出力で確認 |

---

## 9. 付録

### 9.1 判定に使った実測値

| 項目 | 値 |
|---|---|
| 個別弧の総数 | 1,855（`000`:344, `001`:548, `002`:11, `003`:105, `004`:228, `009`:301, `010`:40, `011`:41, `012`:6, `013`:8, `101`:137, `102`:30, `111`:43, `112`:13） |
| 個別弧のサイズ | 33,036〜35,924 B（中央値 34,245） |
| 個別弧のエントリ数 | 12〜17 |
| 個別弧のデータ開始 | 全 1,855 本が `0x8000` |
| 個別弧の `+0x08` | 全 1,855 本が `0xFFFFFFFE` |
| 集約の合計 ID | 1,603（vq 488 / gq 584 / gq2 230 / tq 301、重複 0） |
| 集約に無い個別弧 | 252（配信帯 223 + 村/集会所の欠番 29） |

### 9.2 再現に使ったスクリプト（`mhnxeditor/.work/romfspatch/`）

| ファイル | 用途 |
|---|---|
| `scan_strings.py` / `dump_quest_strings.py` | 生バイナリからクエスト関連文字列を VA 付きで抽出 |
| `xref.py` | 文字列 VA の 4 バイト絶対ポインタ参照を全数走査（0 件を確認） |
| `idsets2.py` / `aggcomp.py` | 個別弧と集約弧の ID 集合・ヘッダ統計 |
| `diffarc.py` | 原本と再梱包の差分（`+0x08` の 4 バイトのみと判明） |
| `mkmod.py` | 弧をパース → `.sup` 1 レコード改変 → 再構築 → 自己検証（§4.4） |

### 9.3 結論の要約

1. **配置パスは `sd:/atmosphere/contents/0100C3800049C000/romfs/nativeNX/arc/quest/q0000101.arc`**。更新 ID 側ではない。
2. **差し替えるのは個別弧 `q%07d.arc`**。報酬・支給品・モンスターはすべてその中にあり、集約弧には入っていない。
3. **`arc-repack` は実用**。往復でサイズ不変・エントリ一致。**`+0x08` の 4 バイトだけ戻す**こと。
4. **サイズ・アラインメントの制約は事実上「データ開始 = alignUp(表サイズ, 0x8000)」だけ**。エントリ間は詰めるだけ。
5. **更新があっても MOD は勝つ**（SD が最優先）。危険なのは romfs ではなく **exefs MOD**。
6. 残る不確実性は「個別弧を読んでいる直接証明」と「実機挙動」。**どちらも 1 クエストの差し替えで潰せる**。
