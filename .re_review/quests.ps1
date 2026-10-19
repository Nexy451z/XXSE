
$root = "C:\anyWorkspace\nx\mhxx\mhnxeditor"
$out = "C:\anyWorkspace\nx\mhxx\mhnxeditor\.re_review"
$arc = Join-Path $root "dist\quests_all\q1010001.arc"
function QShot($name, $entry) {
  $env:MHNXEDITOR_SCREENSHOT = Join-Path $out "$name.png"
  Remove-Item env:MHNXEDITOR_OPEN_SAVE -ErrorAction SilentlyContinue
  $env:MHNXEDITOR_OPEN_TAB = "quest"
  $env:MHNXEDITOR_OPEN_ARC = $arc
  if ($entry -ge 0) { $env:MHNXEDITOR_OPEN_ENTRY = "$entry" } else { Remove-Item env:MHNXEDITOR_OPEN_ENTRY -ErrorAction SilentlyContinue }
  & (Join-Path $root "target\release\mhnxeditor.exe") 2>&1 | Out-File (Join-Path $out "$name.log")
  "exit=$LASTEXITCODE png=$((Test-Path (Join-Path $out "$name.png")))"
}
QShot "q_default" -1
QShot "q_mib" 14
QShot "q_gmd" 13
QShot "q_rem" 5
QShot "q_sup" 10
QShot "q_sem" 0
QShot "q_esl" 3
QShot "q_qdp" 11
QShot "q_qdl" 12
Remove-Item env:MHNXEDITOR_OPEN_ARC
