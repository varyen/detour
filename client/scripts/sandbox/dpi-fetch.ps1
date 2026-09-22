$ErrorActionPreference = 'Continue'
$out = 'C:\out'; $dpi = 'C:\dpi'
$log = "$out\dpi-fetch.txt"
"" | Out-File $log -Encoding utf8
function L($s) { "$(Get-Date -Format HH:mm:ss) $s" | Out-File $log -Append -Encoding utf8 }

L "Defender в песочнице: $(try { (Get-MpComputerStatus).RealTimeProtectionEnabled } catch { 'нет службы' })"
New-Item -ItemType Directory -Force $dpi | Out-Null
$url = 'https://github.com/bol-van/zapret2/releases/download/v1.0.5.2/zapret2-v1.0.5.2.zip'
& curl.exe -sL --max-time 300 -o "$dpi\z.zip" $url
L "скачано: $((Get-Item "$dpi\z.zip" -ErrorAction SilentlyContinue).Length) байт"
Expand-Archive "$dpi\z.zip" "$dpi\x" -Force
$win = Get-ChildItem "$dpi\x" -Recurse -Directory | Where-Object { $_.Name -match 'windows' } | Select-Object -ExpandProperty FullName
L "каталоги windows: $($win -join '; ')"
foreach ($w in $win) {
  L "  $($w): $((Get-ChildItem $w -File | ForEach-Object { "$($_.Name)/$([int]($_.Length/1024))K" }) -join ' ')"
}
$exe = Get-ChildItem "$dpi\x" -Recurse -Filter 'winws2.exe' | Select-Object -First 1
L "winws2: $($exe.FullName)"
if ($exe) {
  Copy-Item (Join-Path $exe.DirectoryName '*') "$dpi\bin\" -Force -Recurse -ErrorAction SilentlyContinue
  New-Item -ItemType Directory -Force "$dpi\bin" | Out-Null
  Copy-Item (Join-Path $exe.DirectoryName '*') "$dpi\bin\" -Force
  $h = & "$dpi\bin\winws2.exe" --help 2>&1 | Out-String
  $h | Out-File "$out\winws2-help.txt" -Encoding utf8
  L "--help: $($h.Length) символов, строк $(($h -split "`n").Count)"
}
L "DONE"
