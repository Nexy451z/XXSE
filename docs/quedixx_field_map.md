# Quedixx UI コントロール ⇔ バイナリオフセット対応表

一次根拠: `Quedixx.exe` を ILSpy 9.1 (ilspycmd) で逆コンパイルした C# ソース
(作業ファイル `.work/decomp/Quedixx/Quedixx.decompiled.cs`、行番号はこれ)。
**「推定」ではなく、読み書きメソッド本体の `Array.Copy(bs, <オフセット>, ...)` / `Array.Copy(..., array, <オフセット>, ...)` から機械的に起こした表**である。

* 読み出し: `main_read` (L1414-1562) … `.mib`
* 書き込み: `main_edit` (L1564-1670) … `.mib`
* 値域: `InitializeComponent` の `NumericUpDown.Maximum / .Minimum` と `ComboBox.Items` (Designer 生成コード L2390-5000) + 141 ファイルの実測
* ラベル名: `this.labelXX.Text = "..."` (L2784-4912) と `Location` 座標によるペアリング

---

## 1. フォーム構成

```text
Form1 (Quedixx 本体)
 ├ tabControl1 (ファイル単位のタブ)
 │   ├ tabPage1 BasicInformation  -> tabControl2 を内包
 │   ├ tabPage2 "テキスト"        -> .gmd
 │   ├ tabPage3 "支給品"          -> .sup
 │   ├ tabPage4 "報酬品"          -> .rem
 │   ├ tabPage5 "小型モンスター"   -> .esl   (Form5 が編集)
 │   ├ tabPage6 "大型モンスター"   -> .sem   (Form4 が編集)
 │   └ tabPage14 "ファイル"       -> .qdl (15 スロットのファイル名一覧)
 └ tabControl2 (基本情報の内部タブ)
     ├ tabPage7  "基本"        -> .mib
     ├ tabPage9  "お金/ポイント" -> .mib
     ├ tabPage8  "アイコン/ターゲット" -> .mib
     ├ tabPage11 "モンス"       -> .mib
     ├ tabPage13 "配置変更"     -> .mib
     └ tabPage12 "その他"       -> .mib
Form2 = アイテム選択 (henC.item_)、Form3 = 大型モンスター選択、Form4 = .sem、Form5 = .esl、
Form6 = 保存確認、Form7 = ファイル一覧、Form8 = メッセージ
```

ファイル種別 ↔ タブの対応は `file_read` (L1055-1130) の switch 文。【事実】

---

## 2. `.mib` のコントロール対応 (tabPage7 / 9 / 8 / 11 / 13 / 12)

| # | コントロール | 型 | タブ | ラベル | .mib offset | size | 値域 (Designer) | main_read | main_edit |
|---|---|---|---|---|---|---|---|---|---|
| 1 | `oq_id` | NumericUpDown | 7 | クエストID： | **+12** | 4 (u32) | Min 101 / Max 1120050 / 既定 1000001 | L1420-1422 | L1568 |
| 2 | `oq_syu` | ComboBox | 7 | クエスト種類： | **+16** | 1 | `henC.syu_` 7 項目 | L1431 | L1576 |
| 3 | `oq_va` | NumericUpDown | 7 | 村： | **+17** | 1 | Max 255 | L1435 | L1580 |
| 4 | `oq_lv` | ComboBox | 7 | クエストLv： | **+18** | 1 | 31 項目 (★1..★10 / G★1..G★4 / 二つ名アイコン / 二つ名Lv1-15) | L1424-1425 | L1569-1570 |
| 5 | `oq_ran1` | ComboBox | 7 | ランク | **+19** | 1 | 村,下位,何か1,上位,何か2,G級 (6) | L1426 | L1571 |
| 6 | `oq_map` | ComboBox | 7 | 目的地： | **+20** | 1 | 32 マップ | L1427-1428 | L1572-1573 |
| 7 | `oq_syo` | ComboBox | 7 | 初期位置： | **+21** | 1 | BC,ランダム,特殊 (3) | L1429 | L1574 |
| 8 | `oq_time` | NumericUpDown | 7 | 制限時間： | **+22** | 1 | Max 255 | L1430 | L1575 |
| 9 | `oq_z4` | NumericUpDown | 9 | 復活回数： | **+23** | 1 | Max 255 | L1481 | L1611 |
| 10 | `oq_bgm` | NumericUpDown | 12 | BGM | **+25** | 1 | Max 255 | L1445 | L1590 |
| 11 | `oq_san1` | ComboBox | 7 | 参加条件1： | **+26** | 1 | `henC.san_` 50 項目 | L1446 | L1591 |
| 12 | `oq_san2` | ComboBox | 7 | 参加条件2： | **+27** | 1 | `henC.san_` | L1447 | L1592 |
| 13 | `oq_t0` | ComboBox | 8 | (無ラベル) | **+29** | 1 | 1のみ, &, or (3) | L1453 | L1598 |
| 14 | `oq_oo5` | NumericUpDown | 11 | 撃退HP%： | **+30** | 1 | Max 255 | L1498 | L1622 |
| 15 | `oq_tt1` | ComboBox | 8 | ターゲット1：の「種類」 | **+31** | 1 | `henC.syu_` 20 項目 | L1454 | L1599 |
| 16 | `oq_t12` / `oq_t12_` / `oq_t12__ / `oq_t12n` | ComboBox×3 + NumericUpDown | 8 | ターゲット1 ID | **+32** | 2 (u16) | 3 つの ComboBox は同じ位置に重ねてあり、`comt[0]` で切替 (0=なし / 1=item_ / 2=mons_) | L1457-1464 | L1600 |
| 17 | `oq_t13` | NumericUpDown | 8 | ターゲット1 数 | **+34** | 2 (i16) | Min 32767(as -32768) / Max 32767 | L1465-1466 | L1601 |
| 18 | `oq_tt2` | ComboBox | 8 | ターゲット2 種類 | **+36** | 1 | `henC.syu_` | L1455 | L1602 |
| 19 | ターゲット2 ID | | 8 | | **+37** | 2 | | L1457-1464 | L1603 |
| 20 | `oq_t23` | NumericUpDown | 8 | ターゲット2 数 | **+39** | 2 | i16 | L1467-1468 | L1604 |
| 21 | `oq_tt3` | ComboBox | 8 | サブターゲット 種類 | **+41** | 1 | `henC.syu_` | L1456 | L1605 |
| 22 | サブターゲット ID | | 8 | | **+42** | 2 | | L1457-1464 | L1606 |
| 23 | `oq_t33` | NumericUpDown | 8 | サブターゲット 数 | **+44** | 2 | i16 | L1469-1470 | L1607 |
| 24 | `oq_ran2` | ComboBox | 7 | 剥ぎ取り | **+46** | 1 | 下位,上位,応急薬,G級,不明 (5) ※格納値 = index+1 | L1432 | L1577 |
| 25 | `oq_ran3` | ComboBox | 7 | 採集 | **+47** | 1 | 下位,上位,なし,特産,シメジ,何か1,何か2,何か3,G級,不明 (10) ※index+1 | L1433 | L1578 |
| 26 | `oq_ran4` | ComboBox | 7 | 釣り | **+48** | 1 | 下位,上位,なし,G級,不明 (5) ※index+1 | L1434 | L1579 |
| 27 | `oq_z1` | NumericUpDown | 9 | 契約金： | **+49** | 4 (u32) | i32 全域 / Increment 100 | L1482-1483 | L1608 |
| 28 | `oq_p1` | NumericUpDown | 9 | 村ポイント： | **+53** | 4 | i32 | L1482-1483 | L1612 |
| 29 | `oq_z2` | NumericUpDown | 9 | 報酬金： | **+57** | 4 | i32 | L1477-1478 | L1609 |
| 30 | `oq_z3` | NumericUpDown | 9 | サブ報酬金： | **+61** | 4 | i32 | L1479-1480 | L1610 |
| 31 | `oq_p2` | NumericUpDown | 9 | RRIP： | **+65** | 4 | i32 | L1484-1485 | L1613 |
| 32 | `oq_p3` | NumericUpDown | 9 | 減少RRIP： | **+69** | 4 | i32 | L1486-1487 | L1614 |
| 33 | `oq_p4` | NumericUpDown | 9 | サブRRIP： | **+73** | 4 | i32 | L1488-1489 | L1615 |
| 34 | `oq_p5` | NumericUpDown | 9 | HRP： | **+77** | 4 | i32 | L1490-1491 | L1616 |
| 35 | `oq_p6` | NumericUpDown | 9 | サブHRP： | **+81** | 4 | i32 | L1492-1493 | L1617 |
| 36 | `oq_rem1` | NumericUpDown | 12 | 報酬枠 | **+85** | 1 | Max 255 | L1442 | L1587 |
| 37 | `oq_rem2` | NumericUpDown | 12 | (報酬枠 2) | **+86** | 1 | Max 255 | L1443 | L1588 |
| 38 | `oq_rem3` | NumericUpDown | 12 | (報酬枠 3) | **+87** | 1 | Max 255 | L1444 | L1589 |
| - | *(未使用)* | | | | *+88..+99* | 12 | | **Quedixx は触らない** | |
| 39 | `dataGridView3` (5 行) | DataGridView | 11 | モンスター名/id/状態/獰猛/匹数/体力/攻撃/防御/難易度/サイズ/±/スタミナ | **+100 + 13*i** | 13×5 | | L1500-1519 | L1623-1637 |
| 40 | `oq_ko1` | NumericUpDown | 11 | 小型： | **+165** | 1 | Max 255 (monster id) | L1520 | L1638 |
| 41 | `oq_ko2` | NumericUpDown | 11 | (小型 2) | **+166** | 1 | Max 255 | L1521 | L1639 |
| 42 | `oq_ko3` | NumericUpDown | 11 | (小型 3) | **+167** | 1 | Max 255 | L1522 | L1640 |
| 43 | `oq_tt4` | ComboBox | 13 | 小型モンスター1 種類 | **+168** | 1 | `henC.syu2_` (なし/狩猟討伐/採取/納品) | L1523 | L1641 |
| 44 | `oq_o1` / `oq_o1_` / `oq_o1__` | ComboBox×3 | 13 | 小型モンスター1 ID | **+169** | 2 | `comt[3]` で切替 | L1530-1538 | L1648 |
| 45 | `oq_o31` | NumericUpDown | 13 | 小型モンスター1 数 | **+171** | 2 | i16 | L1539-1540 | L1655 |
| 46 | `oq_tt5` | ComboBox | 13 | 小型モンスター2 種類 | **+173** | 1 | `henC.syu2_` | L1524 | L1642 |
| 47 | `oq_o2` … | ComboBox×3 | 13 | 小型モンスター2 ID | **+174** | 2 | | L1530-1538 | L1649 |
| 48 | `oq_o32` | NumericUpDown | 13 | 小型モンスター2 数 | **+176** | 2 | i16 | L1541-1542 | L1656 |
| 49 | `oq_syutu` | NumericUpDown | 11 | 出現： | **+178** | 1 | Max 255 | L1436 | L1581 |
| 50 | `oq_tt6` | ComboBox | 13 | 大型モンスター1 種類 | **+179** | 1 | `henC.syu2_` | L1525 | L1643 |
| 51 | `oq_o3` … | ComboBox×3 | 13 | 大型モンスター1 ID | **+180** | 2 | | L1530-1538 | L1650 |
| 52 | `oq_o33` | NumericUpDown | 13 | 大型モンスター1 数 | **+182** | 2 | i16 | L1543-1544 | L1657 |
| 53 | `oq_tt7` | ComboBox | 13 | 大型モンスター2 種類 | **+184** | 1 | `henC.syu2_` | L1526 | L1644 |
| 54 | `oq_o4` … | ComboBox×3 | 13 | 大型モンスター2 ID | **+185** | 2 | | L1530-1538 | L1651 |
| 55 | `oq_o34` | NumericUpDown | 13 | 大型モンスター2 数 | **+187** | 2 | i16 | L1545-1546 | L1658 |
| 56 | `oq_tt8` | ComboBox | 13 | 大型モンスター3 種類 | **+189** | 1 | `henC.syu2_` | L1527 | L1645 |
| 57 | `oq_o5` … | ComboBox×3 | 13 | 大型モンスター3 ID | **+190** | 2 | | L1530-1538 | L1652 |
| 58 | `oq_o35` | NumericUpDown | 13 | 大型モンスター3 数 | **+192** | 2 | i16 | L1547-1548 | L1659 |
| 59 | `oq_tt9` | ComboBox | 13 | 大型モンスター4 種類 | **+194** | 1 | `henC.syu2_` | L1528 | L1646 |
| 60 | `oq_o6` … | ComboBox×3 | 13 | 大型モンスター4 ID | **+195** | 2 | | L1530-1538 | L1653 |
| 61 | `oq_o36` | NumericUpDown | 13 | 大型モンスター4 数 | **+197** | 2 | i16 | L1549-1550 | L1660 |
| 62 | `oq_tt10` | ComboBox | 13 | 大型モンスター5 種類 | **+199** | 1 | `henC.syu2_` | L1529 | L1647 |
| 63 | `oq_o7` … | ComboBox×3 | 13 | 大型モンスター5 ID | **+200** | 2 | | L1530-1538 | L1654 |
| 64 | `oq_o37` | NumericUpDown | 13 | 大型モンスター5 数 | **+202** | 2 | i16 | L1551-1552 | L1661 |
| 65 | `oq_oo1` | NumericUpDown | 11 | 乱入： | **+204** | 1 | Max 255 | L1494 | L1618 |
| 66 | `oq_ex1` | NumericUpDown | 12 | 乱入 | **+205** | 1 | Max 255 | L1437 | L1582 |
| 67 | `oq_ex2` | NumericUpDown | 12 | (乱入 2) | **+206** | 1 | Max 255 | L1438 | L1583 |
| 68 | `oq_ex3` | NumericUpDown | 12 | (乱入 3) | **+207** | 1 | Max 255 | L1439 | L1584 |
| 69 | `oq_ex4` | NumericUpDown | 12 | (乱入 4) | **+208** | 1 | Max 255 | L1440 | L1585 |
| 70 | `oq_ex5` | NumericUpDown | 12 | (乱入 5) | **+209** | 1 | Max 255 | L1441 | L1586 |
| 71 | `oq_oo2` | NumericUpDown | 11 | (乱入 2) | **+210** | 1 | Max 255 | L1495 | L1619 |
| 72 | `oq_oo3` | NumericUpDown | 11 | (乱入 3) | **+211** | 1 | Max 255 | L1496 | L1620 |
| 73 | `oq_oo4` | NumericUpDown | 11 | (乱入 4) | **+212** | 1 | Max 255 | L1497 | L1621 |
| 74 | `numericUpDown1` | NumericUpDown | 12 | **mExtraTicket** | **+213** | 1 | Max 255 | L1553 | L1662 |
| 75 | `oq_icn1` | ComboBox | 8 | アイコン： | **+214** | 1 | `henC.icn_` 188 項目 | L1448 | L1593 |
| 76 | `oq_icn2` | ComboBox | 8 | (アイコン 2) | **+215** | 1 | `henC.icn_` | L1449 | L1594 |
| 77 | `oq_icn3` | ComboBox | 8 | (アイコン 3) | **+216** | 1 | `henC.icn_` | L1450 | L1595 |
| 78 | `oq_icn4` | ComboBox | 8 | (アイコン 4) | **+217** | 1 | `henC.icn_` | L1451 | L1596 |
| 79 | `oq_icn5` | ComboBox | 8 | (アイコン 5) | **+218** | 1 | `henC.icn_` | L1452 | L1597 |
| - | *(未使用)* | | | | *+219..+222* | 4 | | 触らない | |
| - | *(固定)* | | | | *+223..+226* | 4 | 常に `0x242BB29A` (= `.gmd` の typeHash) | L1554-1557 は +241 を読む | |
| 80 | `textBox8` | TextBox | 12 | ファイル： | **+227** | 14 | UTF-8。実データは常に `"<id>_jpn"` | L1558-1559 | L1665-1668 |
| 81 | `numericUpDown2` | NumericUpDown | 12 | **mVillagePointG** | **+241** | 4 (u32) | i32 | L1554-1555 | L1663 |
| 82 | `numericUpDown3` | NumericUpDown | 12 | **mFlag** | **+245** | 4 (u32) | i32 | L1556-1557 | L1664 |

合計 249 バイトを完全にカバーする (未使用列はクエスト間で値が変わらない領域)。

### 2.1 ターゲット/配置 ID の 3 重 ComboBox

`comm[n][0..2]` (L999-1038) は **同じ座標に重ねて配置**され、`comt[n]` の値で有効なものが切り替わる:

```csharp
comm[0] = { oq_t12, oq_t12_, oq_t12__ };   // [0]=なし 固定, [1]=henC.item_ (2991 件), [2]=henC.mons_ (131 件)
comm[3] = { oq_o1,  oq_o1_,  oq_o1__  };
...
```
書き込み値は `get_com(n)` (L2030-2046):
```csharp
if (comt[n] == 1) value = comm[n][1].SelectedIndex;             // アイテム ID そのまま
else if (comt[n] == 2) value = henC.mint_[comm[n][2].SelectedIndex]; // モンスター ID (mint_ 空間)
```
`comt[n]` は `com_changed` (L2062-) が、どの ComboBox が操作されたかで決める。

### 2.2 特殊な詰め替え

| フィールド | 読み | 書き |
|---|---|---|
| `oq_lv` (+18) | `bs[18] > 100 ? bs[18]-85 : bs[18]` をインデックスに | `idx < 16 ? idx : idx+85` |
| `oq_map` (+20) | `bs[20] > 100 ? bs[20]-73 : bs[20]` | `idx < 28 ? idx : idx+73` |
| `oq_ran2/3/4` (+46/47/48) | `bs[x] - 1` | `idx + 1` |
| `oq_san1` (+26) | そのまま | `fox_c` が真なら **47 固定** (「ニャンター不可」) |
| モンスター id (5 スロット) | `get_mons(u16)` で `mint_` の位置を引く | `henC.mint_[選択 index]` を u16 で格納 |

---

## 3. 他リソースのコントロール対応

### 3.1 `.gmd` (tabPage2) — `txt_read` L1729 / `txt_edit` L1759

| コントロール | ラベル | 文字列 index | オフセット |
|---|---|---|---|
| `textBox1` | クエスト名 | 0 | 可変長 (58 から) |
| `textBox2` | 依頼主 | 1 | 〃 |
| `textBox3` | 依頼内容 | 2 | 〃 |
| `textBox4` | 主なモンスター | 3 | 〃 |
| `textBox5` | 成功条件 | 4 | 〃 |
| `textBox6` | 失敗条件 | 5 | 〃 |
| `textBox7` | サブ成功条件 | 6 | 〃 |

### 3.2 `.qdl` (tabPage14) — `link_read` L1702 / `link_edit` L1715

`txtb_[0..14]` = `f_o1..f_o15`。オフセット `20*i + 12` (i = 0..14) に **16 バイト**の名前を読み書きする。

| コントロール | ラベル | slot | 実体のフォルダ |
|---|---|---|---|
| `f_o1` | 大型モンスター1 | 0 | `quest\boss\setEmMain` (.sem) |
| `f_o2` | 大型モンスター2 | 1 | 〃 |
| `f_o3` | 大型モンスター3 | 2 | 〃 |
| `f_o4` | 大型モンスター4 | 3 | 〃 |
| `f_o5` | 大型モンスター5 | 4 | 〃 |
| `f_o6` | 小型モンスター1 | 5 | `quest\zako\emSetList` (.esl) |
| `f_o7` | 小型モンスター2 | 6 | 〃 |
| `f_o8` | 小型モンスター3 | 7 | 〃 |
| `f_o9` | メイン報酬品1 | 8 | `quest\rem` (.rem) |
| `f_o10` | メイン報酬品2 | 9 | 〃 |
| `f_o11` | 特別報酬品1 | 10 | 〃 |
| `f_o12` | 特別報酬品2 | 11 | 〃 |
| `f_o13` | サブ報酬品 | 12 | 〃 |
| `f_o14` | 支給品 | 13 | `quest\supp` (.sup) |
| `f_o15` | Plus | 14 | `quest\plus\questPlus_*` (.qdp) |

※ `textBox8` (slot 15 相当) は `.mib` 側 (+227) の `.gmd` 参照名で、`.qdl` の 16 バイトではない。

### 3.3 `.sup` (tabPage3) — `sikyu_read(0)` / `sikyu_edit(0)`

| コントロール | 列 | オフセット | 型 |
|---|---|---|---|
| `dataGridView1` | ID (表示 1..40) | - | - |
| `dataGridView1` | アイテム名 | +8 + 4*i | u16 を `henC.item_` で名前解決 |
| `dataGridView1` | 個数 | +8 + 4*i + 2 | u8 |
| `dataGridView1` | 何か | +8 + 4*i + 3 | u8 |
| (隠し列) | id | 同上 | - |

### 3.4 `.rem` (tabPage4) — `sikyu_read(1)` / `sikyu_edit(1)`

| コントロール | 列 / ラベル | オフセット | 型 |
|---|---|---|---|
| `dataGridView5` | 何か | +8 + 2*i | u8 |
| `dataGridView5` | 枠数 | +8 + 2*i + 1 | u8 |
| `dataGridView2` | アイテム名 | +24 + 4*i | u16 (`item_`) |
| `dataGridView2` | 個数 | +24 + 4*i + 2 | u8 |
| `dataGridView2` | 確率 | +24 + 4*i + 3 | u8 |

### 3.5 `.sem` (tabPage6 / Form4) — `oomons_read` L1839 / `oomons_edit` L1856

| コントロール | ラベル | オフセット | 型 |
|---|---|---|---|
| `o_o0` | エリア： | +12 | u32 (Max 255) |
| `o_o1` | 向き： | +16 | f32 |
| `o_o2` | X： | +20 | f32 |
| `o_o3` | Z： | +24 | f32 |
| `o_o4` | Y： | +28 | f32 |

### 3.6 `.esl` (tabPage5 / Form5)

`komons_read` (L1868) / `komons_edit` (L1874) は **バイト列を保持するだけ**で何も解析しない。
`dataGridView4` の列見出し (モンスター名/id/匹数/CDCDCD/エリア/変化1-5/CDCD/X/Z/Y/座標1/向き/座標2) は
**セーブエディタからのコピペで、実際には使われていない** (`sikyu_read` が `arere` に応じて `datagv[0]`/`datagv[1]` しか埋めないため)。

### 3.7 `.qdp` (plus)

`plus_read` (L1689) / `plus_edit` (L1695) はバイト列を保持するだけ。UI なし。

---

## 4. Designer リソースから得た値域 (`InitializeComponent`)

```text
oq_id          Minimum 101 / Maximum 1120050 / Value 1000001
oq_va          Maximum 255
oq_time        Maximum 255
oq_z4          Maximum 255 / Increment 100
oq_oo1..oo5    Maximum 255
oq_ex1..ex5    Maximum 255
oq_rem1..rem3  Maximum 255
oq_ko1..ko3    Maximum 255
oq_syutu       Maximum 255
numericUpDown1 Maximum 255   (mExtraTicket)
numericUpDown2 i32 全域      (mVillagePointG)
numericUpDown3 i32 全域      (mFlag)
oq_z1,p1,z2,z3,p2..p6   i32 全域 / Increment 100
oq_t13/t23/t33  Minimum 相当 -32768 / Maximum 32767
oq_o31..o37     Minimum 相当 -32768 / Maximum 32767
oq_t12n/t22n/t32n  Maximum 65535 / Hexadecimal = true / Visible = false (隠し 16 進入力欄)
o_o0           Maximum 255
o_o1..o_o4     decimal の符号付き (f32)
```

ComboBox の Items は §2 の表のとおり。すべて `henC` クラスの配列 (`Quedixx.decompiled.cs` L6694-7075) が出所:

| 配列 | 要素数 | 用途 |
|---|---|---|
| `item_` | 2991 | アイテム名 (`.sup` / `.rem` / ターゲット ID) |
| `mons_` | 131 | モンスター表示名 |
| `mint_` | 131 | モンスター内部 ID (`mons_` と同順) |
| `icn_` | 188 | クエストアイコン |
| `san_` | 50 | 参加条件 |
| `syu_` | 20 | ターゲット種類 |
| `syu2_` | 4 | 配置種類 |
| `map_` / `level_` / `ran1_..ran4_` / `syo_` / `t0_` | - | Designer 直書き |

すべて `docs/quest_fields.json` の `enums` に収録済み。

---

## 5. 注記 (推定を含む箇所)

* tabPage13 のラベルは `oq_o1`/`oq_o2` を「小型モンスター1/2」、`oq_o3`〜`oq_o7` を「大型モンスター1〜5」としている。
  `.qdl` 側は slot 5-7 が .esl (小型)、slot 0-4 が .sem (大型) なので、
  **「oq_o1..o2 ↔ .esl、oq_o3..o7 ↔ .sem」という対応が成立する可能性が高い [推定]** が、
  Quedixx のソース上で両者を結び付けるコードは存在しない (`.mib` の 7 スロットと `.qdl` の 8 スロットは独立に編集される)。
* `dataGridView3` (5 スロット) の実際の対応は tabPage11 の「モンス」タブで、列は
  モンスター名 / id / 状態 / 獰猛 / 匹数 / 体力 / 攻撃 / 防御 / 難易度 / サイズ / ± / スタミナ。
* tabPage4 の `dataGridView4` (小型モンスター) の列見出しは Form6/Form7 由来のコピペで、
  `.esl` の解析には使われていない (`komons_read` はバイト列を保持するだけ)。
* ラベル名のペアリングは Designer の `Location` 座標 (同じ行 = Y が近い) から機械的に決めた。
  そのため「ラベルが無いコントロール」(`oq_t0`, `oq_rem2`, `oq_rem3` など) はラベル欄を空にしている。
