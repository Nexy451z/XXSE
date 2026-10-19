
import sys
from PIL import Image
p = r"C:\anyWorkspace\nx\mhxx\mhnxeditor\.re_review_ux"
img = Image.open(p + r"\dlc.png")
print(img.size)
# crop header area
img.crop((440, 155, 1060, 215)).resize((1240, 120), Image.LANCZOS).save(p + r"\z_dlc_header.png")
img2 = Image.open(p + r"\extras.png")
img2.crop((440, 660, 1000, 780)).resize((1120, 240), Image.LANCZOS).save(p + r"\z_charm_form.png")
img3 = Image.open(p + r"\items_sel_hit.png")
img3.crop((440, 70, 1000, 330)).resize((1120, 520), Image.LANCZOS).save(p + r"\z_item_panel.png")
