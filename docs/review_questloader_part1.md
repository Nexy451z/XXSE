# MHXX (Nintendo Switch) クエストローダ解析レビュー

対象: `out_update/image/main.bin` (25,604,158 B) / `out_base/image/main.bin` (26,184,744 B)
作業: 2026-10-20 / 担当: exefs 解析サブエージェント
関連: `docs/quest_format_spec.md`, `docs/quedixx_field_map.md`, `docs/mp_ghidra.md`, `docs/mp_reverse_engineering.md`, `docs/dlc_layout.md`

> 表記: **【事実】** = 本解析で実測・逆アセンブルで直接確認した / **【推定】** = 実測からの推論 / **【未確定】** = 判断材料不足

---

## 0. 結論サマリ（依頼された 6 項目への回答）

| # | 依頼項目 | 到達点 | 根拠 |
|---|---|---|---|
| 1 | .arc / .mib / .gmd / .rem / .sup / .sem / .esl / .qdp / .qdl を読む処理の特定 | **一部確定**。DLC クエストの「在処探索 → ロード要求」を行う状態機械 (VA 0x37A6C) と、その中の 0x1400 ストライド探索・size フィールド操作を特定。**コンテナ (.arc) のパーサ本体は特定できなかった** | §3.3–3.7 |
| 2 | ゲームが実際に検証・参照しているフィールド | **未確定**。ただし「エントリ名とクエスト ID の一致」は**検証していない**と断定できる | §4.1 |
| 3 | 値を変えると起動しなくなる条件 | **未確定**。明示的な範囲チェック / マジック検証は**見つからなかった**（否定された候補は §5） | §5, §6 |
| 4 | .mib の未確定フィールド候補 | **部分的**。文字列プールから `sQuest` のフィールド名一覧を回収（§3.8）。ただし +0x58〜+0x63 等に対応する名前は見つからず | §3.8 |
| 5 | 「1人限定」等の参加条件の判定箇所 | **候補特定**。DL クエスト一覧側の `mQuestNoSingle` / `mQuestNoMulti`（`cGuestQuestDataXML`）と、`sQuest` の `mJoinMax` / `mJoinNum` | §3.8 |
| 6 | クエスト ID とファイル名 / 内部 ID の整合要求 | **要求されていない**（公式データに反例あり） | §4.1 |

**最重要**: セーブ内の DLC クエストは **.arc の生バイトそのまま**であることを 134/135 件で実証した（§3.1）。したがって「改造 .arc を作ってセーブに入れる」経路は実在し、ゲームはそれを .arc として解釈している。

---

## 1. 解析対象・手法・アドレス規約

### 1.1 アドレス規約【事実】

`~
VA（仮想アドレス） = ファイルオフセット + 0x100
`~

根拠:

| 検証 | 結果 |
|---|---|
| イメージ先頭 8 バイト | `00 00 00 00 08 00 00 00` → MOD0 ヘッダがオフセット 8 に存在 |
| `main.bin` サイズ | 25,604,158 B = 0x186B03E（update） |
| 本解析の逆アセンブル | 全アドレスで妥当な ARM 命令列・リテラルプールが解決する |
| リテラルプールの実例 | VA 0x0306530 の語 `0x01557C04` → VA 0x1557C04 に文字列 `mpQuestDataDL[2]` が実在 |

先行レポート `mp_ghidra.md` の結論（+0x100）と一致。**本レポートのアドレスはすべて VA**。

### 1.2 ツール

| 種別 | 内容 |
|---|---|
| 逆アセンブラ | capstone 5.0.7 (CS_ARCH_ARM / CS_MODE_ARM) |
| Python | 3.12.14 |
| 解析スクリプト | `mhnxeditor/.work/qload/*.py`（本書 §8.3 に一覧） |
| 実セーブ | Ryujinx `bis/user/save/0000000000000001/0/system` (4,726,188 B) 読み取りのみ |

### 1.3 base と update の関係【事実】

クエスト関連の文字列は **両イメージに同一数だけ存在**する（`mpQuestArchiveFixDL` / `quest\questData\questData_%07d` / `mQuestNoSingle` / `mQuestNoMulti` / `quest\q%07d` / `QDLoader` = 各 1 件ずつ両方に存在）。以降は update イメージで解析した。

---

## 2. 【最初に潰した誤り】文字列 → コードの逆引きは不可能

先行レポート `mp_ghidra.md` §2.2 が「15 個の手掛かり文字列に xref が 0 件」と報告している。本解析は**対象を 26 文字列に広げ、参照方法も 5 通りに増やして再検証**した。

### 2.1 やったこと【事実】

| 手法 | 内容 |
|---|---|
| ① 4 バイトアライン全語走査 | イメージ全体の全 32bit 語を走査し、対象 VA と一致する語を列挙 |
| ② `add rX, pc, #imm` 全走査 | 全命令の ARM ADD Rd,PC,#imm12 をデコードして実効アドレスを計算 |
| ③ `ldr rX, [pc, #imm]` 全走査 | 全命令の PC 相対 LDR をデコードして実効アドレスを計算 |
| ④ 文字列内部へのポインタ | 「文字列の途中を指すポインタ」（サフィックス併合）を検出するため、対象文字列の格納範囲 [start, start+len) に入るポインタ語を全走査 |
| ⑤ 相対オフセット表 | ファイルオフセット値・下位 16bit 値での参照も走査 |

### 2.2 結果【事実】

文字列プール（VA 0x1530000〜0x1580000）を指す静的ポインタは **1,454 本**存在する。しかし:

| 対象文字列 | VA | プール内の格納数 | それを指すポインタ |
|---|---|---|---|
| `mpQuestArchiveFixDL` | 0x1557BCE | 1 | **0** |
| `mQuestSizeMin` | 0x1546A03 | 1 | **0** |
| `mQuestSizeMax` | 0x1546A11 | 1 | **0** |
| `mpArcData` | 0x15579B3 | 1 | **0** |
| `quest\questData\questData_` | 0x1550C15 | 1 | **0** |
| `quest\questData\questData_%07d` | 0x1557C87 | 1 | **0** |
| `quest\questLink\questLink_%07d` | 0x1557CA6 | 1 | **0** |
| `quest\q%07d` | 0x154B1F6 | 1 | **0** |
| `quest\rem\` / `quest\supp\` / `quest\plus\questPlus_` | 0x1550C5C / 67 / 73 | 各 1 | **0** |
| `quest\boss\setEmMain\` / `quest\zako\emSetList\` | 0x1550C30 / 46 | 各 1 | **0** |
| `mQuestNoSingle` / `mQuestNoMulti` | 0x155D537 / 46 | 各 1 | **0** |
| `eventQuestList.bin` / `challengeQuestList.bin` | 0x1546441 / 54 | 各 1 | **0** |

→ **クエスト系の文字列は 1 本も静的参照を持たない。**

### 2.3 理由【推定】

- プールは **NUL 終端＋サフィックス併合**で密に連結されている（実例: VA 0x1557BC0 付近 = `...tArchiveFixDL 0 mpQuestDataDL[0] 0 mpQuestDataDL[1] 0 mpQuestDataDL[2] 0 ...`）。名前の重複は「共通の末尾」を共有して 1 箇所に格納される。
- ポインタが存在する 1,454 本は、主にコード中のリテラルプール（例: VA 0x0306530 → `mpQuestDataDL[2]`）と、2 箇所の名前配列（例: VA 0x03075F4〜 に 15 語連続）。
- クエスト系は **実行時に索引 / ハッシュで引かれる**ため静的参照が無い。

**⇒ 「文字列から関数へ辿る」方針は、このタイトルでは成立しない。**（`mp_ghidra.md` の結論を独立に追認）

### 2.4 副産物: クエスト関連の文字列一覧【事実】

プール内で確認できたクエスト関連文字列（すべて無参照）:

`~
quest\questData\questData_          0x1550C15
quest\boss\setEmMain\               0x1550C30
quest\zako\emSetList\               0x1550C46
quest\rem\                          0x1550C5C
quest\supp\                         0x1550C67
quest\plus\questPlus_               0x1550C73
questData_  questPlus_  b_  z_m  rem_  supp_      0x1550BED..0x1550C0A
quest\q%07d                         0x154B1F6
quest\questData\questData_%07d      0x1557C87   <- DLC 用テンプレート
quest\questLink\questLink_%07d      0x1557CA6   <- DLC 用テンプレート
quest\questData\questData_%07d_%s   0x1546486   <- %s = 言語サフィックス
arc\    .arc                        0x154646B / 0x1546470
`~

**この並び（0x1550C15〜0x1550C73 の 6 プレフィクス）は .qdl のスロット 0-14 の解決規則と完全に一致する**（boss 5 / zako 3 / rem 5 / supp 1 / plus 1）。

---

## 3. 判明した事実

### 3.1 セーブの DLC クエストは .arc の生バイト【事実・最重要】

実セーブを走査した結果（読み取りのみ）:

`~
DLC セクション基点      0x000B2A4   （§3.2 で導出）
クエスト枠 0            0x000F898
  +0x00 u32 id          = 0x000F6955 = 1010005
  +0x04 u32 size        = 0x0000071D = 1821
  +0x08 payload         = "ARC\0 11 00 0C 00 00 00 00 00 ..."
                          = magic 0x00435241 / version 17 / entryCount 12 / flags 0
`~

| 実測項目 | 値 |
|---|---|
| 枠数 | 205（0x1400 × 205 = 0x100400） |
| id != 0 の枠 | **175** |
| id の範囲 | 1010001 〜 1120012 |
| size の範囲 | 1,733 〜 3,453（上限 0x13F8 = 5,112 に対して余裕） |
| 空き枠 | 30（例: 枠 135〜164 が連続で空き） |
| サンプル .arc 141 本との照合 | **134 件がバイト完全一致** / 1 件だけ不一致 / 40 件はサンプルに無い |

不一致の 1 件:

| id | セーブ内 size | サンプル `q1010165.arc` |
|---|---|---|
| 1010165 | 2,222 | 2,233 |

→ これは `quest_format_spec.md` §1.4 が「唯一の例外（dataOffset = 1232、Quest_Construct の式と一致）」と書いた**ツールで作り直されたファイル**であり、セーブ側は Capcom 原本。**整合が取れている。**

**結論**: ゲームはセーブの `{id, size, arc 生バイト}` をそのまま .arc として解釈する。改造 .arc をこの枠に入れる経路は仕様上そのまま成立する。

### 3.2 セーブ内 DLC セクションの実測オフセット【事実】

`dlc_layout.md` は「本体 +0xB280 から始まる」とするが、実測では **0xB2A4** だった（0x24 の差）。

導出方法（3 つの独立した手掛かりが一致）:

| 手掛かり | 実測位置 | 逆算した DLC 基点 |
|---|---|---|
| `+0x34` のマスク 24 B（`FF FF FF FF FF 7F 80 07 ... 1F`）の位置 | 0xB2D8 | 0xB2D8 − 0x34 = **0xB2A4** |
| クエスト枠 0 の `{id,size}` = 1010005 | 0xF898 | 0xF898 − 0x45F4 = **0xB2A4** |
| クエスト装備枠 0 の先頭 u32 = 0x000F9061 = 1020001 | 0x10FC98 | 0x10FC98 − 0x1049F4 = **0xB2A4** |

| オフセット（DLC 基点から） | 実測値 |
|---|---|
| `+0x34` | 24 B マスク。立っているビット **112**（`dlc_layout.md` と一致） |
| `+0x4C` | 28 B マスク。立っているビット **175** ＝ 導入済みクエスト数と一致 |
| `+0x45F4` | 205 × 0x1400 クエスト枠（§3.1） |
| `+0x1049F4` | 45 × 0x800 クエスト装備枠。枠 0 の先頭 u32 = 1020001（`dlc_layout.md` と一致） |

> `dlc_layout.md` の 0xB280 との差 0x24 は、同書が「第1スロット基点」を基準にしているためと思われる【推定】。**絶対オフセットを使う実装は 0xB2A4 を基準にすべき**（本書の値は実測）。

### 3.3 クエストローダの本体は「状態機械」【事実】

DLC クエスト処理を含む関数の先頭は **VA 0x37A6C**。

`~
0037A6C  push {r4,r5,r6,r7,r8,sb,sl,fp,lr}
0037A70  sub  sp, sp, #0x3C
0037A74  sub  sp, sp, #0x800          ; フレーム合計 0x83C
0037A78  mov  sl, r0                  ; sl = this
0037A7C  bl   #0x2E5A8
0037A80  ldr  r0, [sl, #0xDF4]        ; r0 = state
0037A84  sub  r1, r0, #1
0037A88  cmp  r1, #0x12               ; state-1 <= 18 ?
0037A8C  bhi  #0x39198                ; 範囲外は共通出口へ
0037A90  lsl  r1, r1, #2
0037A94  add  r2, pc, #0x14
0037A98  movw fp, #0x6B44
0037A9C  movw r5, #0xFD8D
0037AA0  ldr  r1, [r1, r2]            ; ジャンプテーブル
0037AA4  movt fp, #0xF                ; fp = 0x000F6B44  (後述 §3.6)
0037AA8  movt r5, #0xFFFF             ; r5 = 0xFFFFFD8D
0037AAC  add  pc, r1, r2              ; switch(state)
`~

- `state` は **1〜19** の 19 状態。
- 状態遷移は `str rX, [sl, #0xDF4]` の形で行われる（例: 0x37B30 → 0x11、0x39018 → 0x13、0x37B4C → 0）。
- したがって「クエスト 1 件の読み込み」は **1 回の関数呼び出しではなく、19 状態のステップ実行**で進む。**1 フレームで完結しないため、値を壊したときにどの状態で止まるかが観測点になる。**

この `this` が持つフィールド（逆アセンブルから読み取れたもの）:

| フィールド | 使われ方（実測） |
|---|---|
| `+0x3C` | 別の状態変数（`cmp r0,#2` / `mov r1,#3` 等） |
| `+0xDF4` | **ロード状態**（switch 対象） |
| `+0xDF8` | 0 クリアされる作業値 |
| `+0xDFC` | **ループ上限**（`ldr r0,[sl,#0xDFC]` / `cmp r7,r0` / `blo`） |
| `+0xE00` | `+0xDFC` と比較される値 |
| `+0xE10` | ポインタ（0 チェックあり） |
| `+0xE60` | ポインタ（0 チェックあり） |
| `+0xEA8` | **コンテナ A**（`[+0x64]` = 件数、要素は `+4` にキー） |
| `+0xEB4` | **コンテナ B**（同上、別系統） |
| `+0xFA8` | **クエスト枠配列 B**（160 件 × 0x1400） |
| `+0xFAC` | **クエスト枠配列 A**（45 件 × 0x1400） |
| `+0xFB0` | クエスト装備枠配列（idx × 0x800、先頭 u32 = ID） |

> 45 + 160 = **205**（セーブのクエスト枠数と一致）。`dlc_layout.md` の「205 枠」と整合する【事実＋推定】。

### 3.4 DLC クエスト枠の探索【事実】

`state` の一つ（VA 0x389A0 付近）で、ID からスロットを探している:

`~
0038994  ldr  r0, [r0, #0xFAC]       ; 配列 A（45 件）
0038998  cmp  r0, #0
003899C  beq  #0x38AFC
00389A0  add  r6, r0, #4
00389A4  mov  r3, #0
00389A8  ldr  r4, [r6, #-4]          ; slot[0] = id
00389AC  cmp  r4, r2                 ; 探しているキーと比較
00389B0  bne  #0x389C0
00389B4  ldr  r4, [r6]               ; slot[1] = size
00389B8  cmp  r4, #0
00389BC  bne  #0x38AF8               ; id 一致 かつ size != 0 -> 発見
00389C0  add  r3, r3, #1
00389C4  add  r6, r6, #0x1400        ; * ストライド 0x1400
00389C8  cmp  r3, #0x2D              ; * 45 件
00389CC  blo  #0x389A8
`~

同型のループが 0x38A44〜 にもう 1 本あり、こちらは `cmp r3, #0xA0`（**160 件**）。

**⇒ ゲームは「id が一致し、かつ size != 0」の枠を有効とみなす。** 逆に言えば **size を 0 にすると、その枠は「無い」ものとして扱われる**（実測コードの分岐）。これは改造時の重要な挙動。

発見後の処理（VA 0x38A74）:

`~
0038A74  add  r1, r1, r1, lsl #2     ; r1 = 添字 * 5
0038A78  add  r4, r0, r1, lsl #10    ; r4 = 配列先頭 + 添字 * 0x1400  *
0038A7C  mov  r1, #0x100
0038A80  add  sb, r4, #0x1300        ; sb = slot + 0x1300
0038A84  mov  r0, sb
0038A88  bl   #0x13E46E4             ; 0x100 バイトのゼロクリア相当（import サンク）
`~

→ 3DS 版の `add r1,r1,r1,lsl #2` + `add r0,r0,r1,lsl #10`（`mp_reverse_engineering.md` §2）と**同一の ×5 → ×0x400 命令列**が Switch 版にも存在する。先行レポート `mp_ghidra.md` が 0x00038974 / 0x000389FC で見つけた箇所の正体はこれ。

### 3.5 ロード時の「Fix」処理 — size を書き換える【事実】

状態の一つが、枠の中身を**その場で書き換える**:

`~
0038A80  add  sb, r4, #0x1300        ; sb = slot + 0x1300
0038A84  mov  r0, sb
0038A88  bl   #0x13E46E4             ; クリア(buf, 0x100)
0038A8C  mov  r6, r4
0038A90  ldr  r5, [r6, #4]!          ; r6 = slot+4 ; r5 = size
0038A94  cmp  r5, #0x100
0038A98  bls  #0x38AB8               ; size <= 0x100 なら何もしない
0038A9C  add  r0, r4, r5             ; r0 = slot + size
0038AA0  mov  r2, #0x100
0038AA4  sub  r1, r0, #0xF8          ; r1 = slot + size - 0xF8 = ペイロード末尾 - 0x100
0038AA8  mov  r0, sb                 ; 転送先 = slot + 0x1300
0038AAC  bl   #0x13E4870             ; memcpy(dst, src, 0x100) 相当
0038AB0  sub  r0, r5, #0x100
0038AB4  str  r0, [r6]               ; * size フィールドを 0x100 減らして書き戻す
`~

読み取れること:

| 事実 | 内容 |
|---|---|
| ペイロード位置 | `slot+8` から（§3.1 のセーブ実測と一致） |
| size の意味 | **ペイロードのバイト数**（8 バイトのヘッダは含まない） |
| 上限 | `slot + size` が 0x1400 を超えないこと（0xF8 = 0x100 − 8 の補正がその証拠） |
| 副作用 | 条件を満たすと **size を 0x100 減らして書き戻す** |

**【未確定】** この「末尾 0x100 バイトを +0x1300 へ移し、size を 0x100 減らす」処理の目的。
実セーブの 175 件はすべて `size == .arc 実サイズ` のまま（§3.1 でバイト一致を確認）なので、
**この状態は通常のクエストでは通らない**（別の状態を通る）と考えられる【推定】。
考えられる解釈（いずれも未検証）:
- 旧レイアウト（末尾 0x100 B を別領域に置いていた世代）からの移行処理
- ローダが枠末尾 0x100 B を自前の作業領域として確保するための退避

**改造時の注意（推奨）**: この処理が走る条件を踏むと、**セーブ上の size が書き換わる**可能性がある。size を「.arc 実サイズ」以外の値にした改造は避けるべき。

### 3.6 クエスト ID → 内部キーの変換表【事実（表）／未確定（意味）】

別関数（VA 0x3B58C / 0x3B8C4）で、数値から検索キーを作っている:

`~
003B5A8  sub  r3, r2, #0xB           ; r2 = 入力値
003B5AC  mvn  r7, #0xA               ; バイアス = -0x0B
003B5B0  cmp  r3, #0x32              ; 11..60
003B5B4  blo  #0x3B5F0
003B5B8  sub  r3, r2, #0x47          ; 71..80
003B5BC  mvn  r7, #0x46              ; バイアス = -0x47
...
003B5F0  add  r3, r7, r2             ; idx = 入力 + バイアス
003B5F4  movw r2, #0x6B44
003B5F8  movt r2, #0xF               ; r2 = 0x000F6B44
003B5FC  add  r7, r3, r2             ; * キー = (入力+バイアス) + 0xF6B44
`~

| 入力値の範囲 | バイアス |
|---|---|
| 11..60 | −0x0B |
| 71..80 | −0x47 |
| 80..159 | −0x50 |
| 160..169 | −0xA0 |
| 170..469 | −0xAA |
| 507..557 | フォールバック節 0x3B684 |
| 577..621 | 同 |
| 637..796 | 同 |

- キーは `(入力 + バイアス) + 0xF6B44`。`0xF6B44 = 1,010,500`。
- **【未確定】** 入力 `r2` が何か。7 桁のクエスト ID（1010005 等）を入れると、どの範囲にも入らず「見つからない」経路に落ちるため、**7 桁 ID ではない**。11〜796 の小さな通し番号（DL クエスト一覧上の番号）と推定。

### 3.7 ローダは 2 系統【事実】

| 関数 | 探索するコンテナ | 見つけた後の転送関数 |
|---|---|---|
| VA 0x3B58C | `this+0xEA8` | VA 0x34F2D0 |
| VA 0x3B8C4 | `this+0xEB4` | VA 0x3308A0 |

両方とも「`キー == entry[+4]` の要素を線形探索し、見つかれば `転送(dst, entry)` を呼んで 1 を返す / 見つからなければ 0」。

転送関数は **フィールド単位のコピー**（構造体の詰め替え）:

`~
0034F2D0  ldr r2, [r1, #4]
0034F2D4  str r2, [r0, #4]
0034F2D8  ldrb r2, [r1, #8]
0034F2DC  strb r2, [r0, #8]
... (+0x0C, +0x10, +0x14, +0x18, +0x1C, +0x20, +0x24, +0x29, +0x2E,
     +0x38, +0x42, +0x4C, +0x56, +0x5B, +0x60, +0x6A, +0x74, +0x7E ...)
`~

コピーされるオフセット（0x34F2D0 系）と、もう一方（0x3308A0 系）は**別のレイアウト**を持つ（前者は +0x6A/+0x74/+0x7E まで、後者は +0x24〜+0x62 を中心に u8/u16 が混在）。

**【推定】** 前者は「クエスト定義（大きい構造体）」、後者は「クエスト一覧エントリ（小さい構造体）」への詰め替え。**ここでどのフィールドが実際に使われるかは追えていない。**

### 3.8 クエストクラスのフィールド名（文字列プールから回収）【事実】

静的参照は無いが、**文字列そのものはクラス定義の証拠**になる。VA 0x1557A87〜0x1557C26 に `sQuest` / `cQuestDataEx` 系の名前が連続している:

`~
sQuest                       0x155784A
sQuest::SupplyBox            0x1557851
sQuest::ShareBox             0x1557863
sQuest::BossLoader           0x1557874
mQuestTimer  mFlgReturnTime20  mFlgTimeUpSuccess  mFlgSubClear
mClearCameraSetFlag  mLeaveLongCarcass  mFlgStartSecret
mUseCatInsurance  mQuestFaildBecauseKilled  mReciveQuestDone
mClearDemoExeFlag  mHuntDailyTarget  mAddBossRno  mBossRushLv
mRushSubCtr  mAppearPermitBit  mAppearRequestIdx
mSupplyBox  mShareBox
mpArcData                    0x15579B3
mpQuestDataRes               0x15579BD
mpQuestData                  0x15579CC
mpQuestLinkRes               0x15579D8
mpQuestLink                  0x15579E7
mpBossSet[QUEST_BOSS_1..5]   0x15579F3 - 0x1557A53
mpQuestGroupRes              0x1557A6B
mBossLoader                  0x1557A7B
cQuestDataEx                 0x1557A87
sQuestSelector               0x1557A94
mOrderState  q_join_stat  quest_no
mQuestListNum  mQuestLvTblNum
mpQB  mDailyData
mHostID(sGameControl)  mMasterID(sGameControl)  mJoinNum(sGameControl)
mSessionIndx(sNetControl)
local_host_id  mPastTime  mJoinMax  mJoinNum  mIsHost
mMemberIndex  mIsEntry  mIsGame  mOption  join_order_idx
mTicketColor  qb_result
mpQuestArchiveFixDL          0x1557BCE
mpQuestDataDL[0] .. [3]      0x1557BE2 - 0x1557C15
mEnablePoolAcEquip  mPoolAcPlayerEquip  mPoolAcNyanterEquip
mPoolPlayerParam  mUseEquipSet
QDLoader                     0x1557C7E
quest\questData\questData_%07d   0x1557C87
quest\questLink\questLink_%07d   0x1557CA6
`~

**ここから読み取れる設計【推定】**:

1. クエストは **`mpQuestDataRes` / `mpQuestLinkRes` / `mpArcData`** という 3 つの「資源ハンドル」を持つ → .mib（questData）と .qdl（questLink）と .arc 全体を別々に保持。
2. `mpQuestDataDL[0..3]` が 4 本 → **DLC 用に 4 スロット**（プレイヤー人数ぶん？）。**【未確定】**
3. `mpQuestArchiveFixDL` → **DL クエスト専用の「Fix」処理を持つ**（§3.5 の処理の名前と推定）。
4. `QDLoader` → quest data loader。`mQuestListNum` / `mQuestLvTblNum` は一覧とレベル表の件数。
5. `mJoinMax` / `mJoinNum` / `mIsHost` / `mMemberIndex` → **参加人数の管理**。ここが「何人まで参加できるか」を持ちうる。

### 3.9 リソース種別名と拡張子【事実】

VA 0x1557734〜0x1557846 に「クラス名 + 拡張子」の並びがある:

| クラス名 | 拡張子 | 対応する実ファイル |
|---|---|---|
| rEmSetData | esd | .esl 内 ESD チャンク |
| rAcNyanterEquip | ane | dlc_ac_ny_equip.ane |
| rAcPlayerEquip | ape | dlc_ac_pl_equip.ape |
| rBui | bui | - |
| rEmSetList | esl | .esl |
| rFishData | fsh | - |
| rFreeHuntData | fht | - |
| rHagi + mData | hgi | - |
| rQuestDaily | qdm | - |
| **rQuestData** | **ext** | .mib ??? |
| rQuestGroup | qsg | - |
| **rQuestLink** | **qdl** | .qdl |
| **rQuestPlus** | **qdp** | .qdp |
| rRem | （なし） | .rem |
| rResourceSample | rspl | - |
| rSetEmMain | sem | .sem |
| rSupplyList | sup | .sup |

**注意**: イメージ全体を走査しても、**単独文字列 `mib` は 0 件**（`supp`, `rem`, `gmd` も単独では 0 件、`rem` は別文脈で 1 件のみ）。つまり
**「.mib / .sup / .rem / .gmd」という拡張子はゲーム側に存在しない**。これらはツール（Quest_Construct）側の命名規則であり、.arc は拡張子を格納しない（`quest_format_spec.md` §1.3 の「拡張子は格納されない」と整合）。

→ **ゲームは「名前 + typeHash」で資源を引いており、拡張子では判定していない**【推定】。

### 3.10 アイテム表の件数は 2991【事実】

`nativeNX/table/itemData.itm` の実測:

`~
+0x00 u32 magic  = 0x41AA6666
+0x04 u32 count  = 2991
+0x08 ...        レコード 44 バイト x 2991
                 8 + 2991*44 = 131,612 = ファイルサイズ（厳密一致）
record +0x08 は 0,1,2,3,... の単調増加 u16 -> レコード番号 = アイテム ID
`~

`itemData_jpn.gmd` の `stringCount = 5982 = 2 x 2991`（名前＋説明の 2 本）。

コード側の裏取り:

| VA | 命令 | 意味 |
|---|---|---|
| 0x380974 | `movw r1, #0xBAE` (2990) | 配列長チェックの上限 |
| 0x380998 | `movw r1, #0xBAF` (2991) | 確保する要素数 |
| 0x380A38 | `movw r0, #0xBAF` → `str r0,[r4,#0x168]` | 長さを 2991 に正規化 |
| 0x3809FC | `movw r2,#0xBAF` → `sub r1,r2,r1` → `bl 0x13E46E4` | 残りをゼロ埋め |

**⇒ アイテム ID の有効範囲は 0〜2990。**（公式クエストの実測最大は .rem の 2976）

### 3.11 公式 .arc の実測（141 ファイル / 1,950 エントリ）【事実】

| 項目 | 実測 |
|---|---|
| ファイルサイズ | 1,811 〜 3,881 B（**最大でも枠上限 0x13F8 = 5,112 の 76%**） |
| エントリ数 | 12〜16（12:14件, 13:33件, 14:66件, 15:19件, 16:9件） |
| 展開後サイズの不一致 | 0 / 1,950 |
| 最大ペイロード | 2,010 B（`q1110104.arc` の `z_m01d_053`） |
| .rem レコード（非ゼロ） | 0〜24 / 40 |
| .sup レコード（非ゼロ） | 0〜40 / 40（平均 24.2） |

.mib のフィールド実測（141 件）:

| フィールド | 観測値（件数） |
|---|---|
| quest_kind (+0x10) | 0:66, 1:36, 2:1, 3:8, 4:4, 5:26 |
| rank1 (+0x13) | 0:1, 1:11, 3:80, 5:49 |
| level (+0x12) | 2〜14（7 が 64 件、14 が 34 件） |
| time_limit (+0x16) | **35:1, 50:140** |
| revive_count (+0x17) | **3 のみ（141/141）** |
| cond1 (+0x1A) | 0:10, 3:57, 7:22, 8:25, 12:24, 45:1, 47:2 |
| cond2 (+0x1B) | 0:139, 45:1, 46:1 |
| mFlag (+0xF5) | 0x0:39, 0x20000:9, 0x80000:1, 0x420000:3, 0x3000000:59, 0x3020000:5, 0x3080000:3, 0x30A0000:7, 0x4000000:13, 0x4020000:1, 0x4100000:1（11 種、下位 16bit は常に 0） |
| rem1/rem2/rem3 (+0x55..57) | 14 パターン（例 1,0,0 / 2,2,0 / 0,0,255） |

.rem / .sup のレコード実測:

| 項目 | .rem（報酬） | .sup（支給品） |
|---|---|---|
| item_id 範囲 | **26 〜 2976**（968 種） | **13 〜 1735**（52 種） |
| count の値 | 1〜12 が大半、最大 **99**（90/95/99 が各 1 件） | 1,2,3,5,10,15,30,**60** |
| 確率/フラグ | prob = 1〜100 | flag = 0〜4 |

> **仕様書との差**: `quest_format_spec.md` §7 は .rem の個数を「0..10」と記載しているが、141 件実測では **1〜99** が出現する。個数は u8 の生値であって上限 10 ではない【事実】。仕様書の記述は観測の一部のみを見ている可能性がある。
