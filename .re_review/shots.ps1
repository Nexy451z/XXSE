
$ErrorActionPreference = "Continue"
$root = "C:\anyWorkspace\nx\mhxx\mhnxeditor"
$out = "C:\anyWorkspace\nx\mhxx\mhnxeditor\.re_review"
New-Item -ItemType Directory -Force -Path $out | Out-Null
$save = "C:\Users\haruk\AppData\Roaming\Ryujinx\bis\user\save\0000000000000001\0\system"
$arc = Join-Path $root "dist\quests_all\q1010001.arc"
function Shot($name, $tab, $extra) {
  $env:MHNXEDITOR_SCREENSHOT = Join-Path $out "$name.png"
  $env:MHNXEDITOR_OPEN_SAVE = $save
  $env:MHNXEDITOR_OPEN_TAB = $tab
  if ($extra) { foreach ($k in $extra.Keys) { Set-Item -Path "env:$k" -Value $extra[$k] } }
  & (Join-Path $root "target\release\mhnxeditor.exe") 2>&1 | Out-File (Join-Path $out "$name.log")
  "exit=$LASTEXITCODE png=$((Test-Path (Join-Path $out "$name.png")))"
  Remove-Item env:MHNXEDITOR_OPEN_ARC -ErrorAction SilentlyContinue
  Remove-Item env:MHNXEDITOR_OPEN_ENTRY -ErrorAction SilentlyContinue
  Remove-Item env:MHNXEDITOR_ITEM_SELECT -ErrorAction SilentlyContinue
  Remove-Item env:MHNXEDITOR_ITEM_FILTER -ErrorAction SilentlyContinue
}
Shot "a_save" "save" $null
Shot "b_items" "items" $null
Shot "c_items_sel" "items" @{ MHNXEDITOR_ITEM_SELECT = "3"; MHNXEDITOR_ITEM_FILTER = "回復" }
Shot "d_extras" "extras" $null
Shot "e_dlc" "dlc" @{ }
Shot "f_about" "about" $null
