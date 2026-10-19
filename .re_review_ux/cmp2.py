
import re, sys, io
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
g = open(r"C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\gamedata.rs", encoding="utf-8").read()
i = g.index("pub static SKILL_NAMES"); j = g.index("];", i)
en = re.findall(r'"((?:[^"\\]|\\.)*)"', g[g.index("[", i)+1:j])
s = open(r"C:\anyWorkspace\nx\mhxx\mhnxeditor\crates\mhnx-core\src\skills.rs", encoding="utf-8").read()
i2 = s.index("pub static SKILL_NAMES"); j2 = s.index("];", i2)
ja = re.findall(r'"((?:[^"\\]|\\.)*)"', s[s.index("[", i2)+1:j2])
for k in range(196, 210):
    print("id", k, "EN:", en[k] if k < len(en) else "-", "JA:", ja[k-1] if 0 < k <= len(ja) else "-")
print("id 0 JA none")
