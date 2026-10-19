
import re, json
p = r"C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\gamedata.rs"
s = open(p, encoding="utf-8").read()
i = s.index("pub static ITEM_NAME_LIST")
j = s.index("];", i)
body = s[s.index("[", i)+1:j]
names = re.findall(r'"((?:[^"\\]|\\.)*)"', body)
print("count", len(names))
for q in ["薬","玉","素材","回復","弾","護符","種"]:
    print(q, sum(1 for n in names if q in n))
print("longest", max(len(n) for n in names))
dups = len(names) - len(set(names))
print("dups", dups)
# names that contain digits (id search collisions)
print("digit names", sum(1 for n in names if any(c.isdigit() for c in n)))
print("first 12", names[:12])
# count named entries 1..4095
print("empty/----", sum(1 for n in names if n.strip() in ("-----","")))
