#Requires -Version 5.1
<#
  extract-titles.ps1 - .arc からクエストタイトルを抜き出して一覧にする。

  使い方:
    pwsh -File .\tools\extract-titles.ps1 -Dir C:\anyWorkspace\nx\mhxx\arcs\event
    pwsh -File .\tools\extract-titles.ps1 -Dir <フォルダ> -PerFile   # 1件ずつ .txt も

  出力:
    <Dir>\クエストタイトル.txt        ID / サイズ / ファイル / タイトル
    <Dir>\q1010002_<タイトル>.txt      -PerFile 指定時
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Dir,
    [string]$Mhnx = 'C:\anyWorkspace\nx\mhxx\mhnxeditor\dist\mhnx.exe',
    [switch]$PerFile,
    [string]$OutName = 'クエストタイトル.txt'
)

$ErrorActionPreference = 'Stop'
if (-not (Test-Path $Mhnx)) { throw "mhnx.exe が見つかりません: $Mhnx" }
if (-not (Test-Path $Dir)) { throw "フォルダがありません: $Dir" }

$files = Get-ChildItem $Dir -Filter '*.arc' -File | Sort-Object Name
if ($files.Count -eq 0) { throw "$Dir に .arc がありません" }

# arc-gmd は「  クエスト名 : <タイトル>」の形で出す。漢字を含むので UTF-8 で読む。
function Get-QuestTitle([string]$path) {
    $lines = & $Mhnx arc-gmd $path 2>&1
    foreach ($line in $lines) {
        if ($line -match '^\s*クエスト名\s*[:：]\s*(.+)$') { return $Matches[1].Trim() }
    }
    return ''
}

function Get-QuestId([string]$name) {
    if ($name -match '^q0*(\d+)\.arc$') { return [int]$Matches[1] }
    return 0
}

$rows = New-Object System.Collections.Generic.List[object]
$i = 0
foreach ($f in $files) {
    $i++
    Write-Progress -Activity 'クエストタイトル抽出' -Status $f.Name -PercentComplete (($i / $files.Count) * 100)
    $title = Get-QuestTitle $f.FullName
    $id = Get-QuestId $f.Name
    $rows.Add([PSCustomObject]@{ Id = $id; File = $f.Name; Size = $f.Length; Title = $title })
    if ($PerFile -and $title) {
        $safe = ($title -replace '[\\/:*?"<>|]', '_')
        if ($safe.Length -gt 60) { $safe = $safe.Substring(0, 60) }
        $txt = Join-Path $Dir ("$($f.BaseName)_$safe.txt")
        "ID   : $id`r`nFILE : $($f.Name)`r`nSIZE : $($f.Length)`r`nNAME : $title" | Set-Content $txt -Encoding utf8
    }
}
Write-Progress -Activity 'クエストタイトル抽出' -Completed

$lines = New-Object System.Collections.Generic.List[string]
$lines.Add('# クエストタイトル一覧')
$lines.Add("# 生成   : $(Get-Date -Format 'yyyy/MM/dd HH:mm:ss')")
$lines.Add("# 対象   : $Dir")
$lines.Add("# 件数   : $($rows.Count)")
$lines.Add('#')
$lines.Add(("{0,-10} {1,7}  {2,-16} {3}" -f 'ID', 'サイズ', 'ファイル', 'タイトル'))
$lines.Add(('-' * 90))
foreach ($r in $rows) {
    $lines.Add(("{0,-10} {1,7}  {2,-16} {3}" -f $r.Id, $r.Size, $r.File, $r.Title))
}

$path = Join-Path $Dir $OutName
$lines | Set-Content $path -Encoding utf8

$named = ($rows | Where-Object { $_.Title }).Count
Write-Output "$($rows.Count) 件（タイトル取得 $named 件）-> $path"
if ($PerFile) { Write-Output '1 件ずつの .txt も作成しました' }