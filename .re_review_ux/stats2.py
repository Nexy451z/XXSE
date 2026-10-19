
import re, sys, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
p = r"C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\gamedata.rs"
s = open(p, encoding="utf-8").read()
i = s.index("pub static ITEM_NAME_LIST"); j = s.index("];", i)
names = re.findall(r'"((?:[^"\\]|\\.)*)"', s[s.index("[", i)+1:j])
qs = {"yaoku": "薬", "tama": "玉", "sozai": "素材", "kaifuku": "回復", "dan": "弾", "mamori": "護符", "tane": "種", "kona": "粉"}
for k,q in qs.items():
    print(k, q, sum(1 for n in names if q in n))
print("digitnames", sum(1 for n in names if any(c.isdigit() for c in n)))
print("first20", names[1:21])
print("dups", [n for n in set(names) if names.count(n)>1][:5])
