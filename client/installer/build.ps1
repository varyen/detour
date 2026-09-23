# Сборка установщика Detour для Windows.
#   pwsh client/installer/build.ps1 -SingBox <sing-box.exe> [-Zapret <zapret2.zip|каталог>]
#                                   [-CertThumbprint <отпечаток>] [-Out <каталог>]
# Собирает панель, службу и интерфейс, складывает всё в staging и зовёт makensis.
# Без -Zapret архив zapret2 скачивается с GitHub (Defender метит его трояном —
# на рабочей машине лучше передавать уже скачанный файл).
param(
  [Parameter(Mandatory)] [string] $SingBox,
  [string] $Zapret,
  [string] $ZapretVersion = "1.0.5.2",
  [string] $CertThumbprint,
  # Без движка обхода DPI: на машине с Defender архив zapret2 не распаковать,
  # а поставить winws2 можно потом из самого приложения.
  [switch] $NoDpi,
  [string] $Out = (Join-Path $PSScriptRoot "..\..\releases\client")
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$client = Join-Path $root 'client'
$version = (Get-Content (Join-Path $root 'VERSION') -Raw).Trim()
$stage = Join-Path ([IO.Path]::GetTempPath()) "detour-stage"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage, $Out | Out-Null

Write-Host "== панель (клиентская сборка)"
Push-Location (Join-Path $root 'panel')
npm run build:client
if ($LASTEXITCODE) { throw "сборка панели не прошла" }
Pop-Location

Write-Host "== служба и интерфейс (release, статический CRT)"
Push-Location $client
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --release -p detour-svc
if ($LASTEXITCODE) { throw "сборка службы не прошла" }
# Без custom-protocol Tauri считает сборку отладочной и открывает в окне
# devUrl (localhost:5199) вместо встроенной панели — это делает tauri build,
# а мы собираем cargo напрямую.
cargo build --release -p detour-app --features tauri/custom-protocol
if ($LASTEXITCODE) { throw "сборка интерфейса не прошла" }
Pop-Location
Copy-Item (Join-Path $client 'target\release\detour-svc.exe') $stage
Copy-Item (Join-Path $client 'target\release\detour-app.exe') $stage
Copy-Item $SingBox (Join-Path $stage 'sing-box.exe')
# Загрузчик WebView2 (~2 МБ) едет внутри установщика: без среды окно не откроется,
# а в песочнице и на урезанных сборках Windows её нет.
& curl.exe -sfL --max-time 120 -o (Join-Path $stage 'MicrosoftEdgeWebview2Setup.exe') 'https://go.microsoft.com/fwlink/p/?LinkId=2124703'
if ($LASTEXITCODE) { throw "не скачался загрузчик WebView2" }
$wv = Get-AuthenticodeSignature (Join-Path $stage 'MicrosoftEdgeWebview2Setup.exe')
if ($wv.Status -ne 'Valid' -or $wv.SignerCertificate.Subject -notmatch 'Microsoft Corporation') { throw "загрузчик WebView2 без подписи Microsoft" }

if ($NoDpi) {
  Write-Host "== winws2 не кладём (-NoDpi)"
} else {
Write-Host "== winws2 и lua из zapret2"
$zdir = Join-Path $stage '_zapret'
New-Item -ItemType Directory -Force $zdir | Out-Null
if (-not $Zapret) {
  $Zapret = Join-Path $zdir 'zapret2.zip'
  $url = "https://github.com/bol-van/zapret2/releases/download/v$ZapretVersion/zapret2-v$ZapretVersion.zip"
  Write-Host "   качаю $url"
  & curl.exe -sL --max-time 600 -o $Zapret $url
}
if ((Get-Item $Zapret).PSIsContainer) {
  $src = $Zapret
} else {
  Expand-Archive $Zapret (Join-Path $zdir 'x') -Force
  $src = Join-Path $zdir 'x'
}
$bin = Get-ChildItem $src -Recurse -Directory | Where-Object { $_.Name -eq 'windows-x86_64' } | Select-Object -First 1
if (-not $bin) { throw "в $src нет каталога binaries/windows-x86_64" }
foreach ($f in 'winws2.exe', 'cygwin1.dll', 'WinDivert.dll', 'WinDivert64.sys') {
  Copy-Item (Join-Path $bin.FullName $f) $stage
}
foreach ($f in 'zapret-lib.lua', 'zapret-antidpi.lua', 'zapret-auto.lua') {
  $lua = Get-ChildItem $src -Recurse -Filter $f | Select-Object -First 1
  if (-not $lua) { throw "в архиве zapret2 нет $f" }
  Copy-Item $lua.FullName $stage
}
Remove-Item -Recurse -Force $zdir
}

# Подпись: без сертификата SmartScreen ругается на каждый запуск, а Defender
# придирается к WinDivert. Отпечаток берётся из личного хранилища.
function Sign([string[]] $files) {
  if (-not $CertThumbprint) { return }
  $tool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match 'x64' } | Select-Object -Last 1
  if (-not $tool) { throw "signtool.exe не найден — поставьте Windows SDK" }
  & $tool.FullName sign /sha1 $CertThumbprint /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 @files
  if ($LASTEXITCODE) { throw "подпись не прошла" }
}
Sign @((Join-Path $stage 'detour-svc.exe'), (Join-Path $stage 'detour-app.exe'))

$outfile = Join-Path $Out "detour-setup-$version.exe"
$makensis = "${env:ProgramFiles(x86)}\NSIS\makensis.exe"
if (-not (Test-Path $makensis)) { throw "NSIS не установлен: winget install NSIS.NSIS" }
& $makensis /V2 "/DVERSION=$version" "/DSTAGE=$stage" "/DOUTFILE=$outfile" (Join-Path $PSScriptRoot 'detour.nsi')
if ($LASTEXITCODE) { throw "makensis не собрал установщик" }
Sign @($outfile)

$size = [int]((Get-Item $outfile).Length / 1MB)
Write-Host "готово: $outfile ($size МБ)"
if (-not $CertThumbprint) {
  Write-Host "ВНИМАНИЕ: установщик не подписан — SmartScreen покажет предупреждение, а Defender может придраться к WinDivert."
}
