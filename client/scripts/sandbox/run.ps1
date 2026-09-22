# Стенд в Windows Sandbox: TUN поднимается только там, на рабочей машине — никогда.
#   pwsh client/scripts/sandbox/run.ps1 -SingBox <путь к sing-box.exe> [-Only tun|service|dpi|installer]
# -Only installer требует -Setup <detour-setup-X.Y.Z.exe> (его собирает installer/build.ps1).
# Собирает detour-svc со статическим CRT (в чистом образе нет vcruntime140.dll),
# прокидывает stand/ только на чтение и out/ на запись, гоняет сценарии, печатает отчёт.
param(
  [Parameter(Mandatory)] [string] $SingBox,
  [ValidateSet('tun', 'service', 'dpi', 'installer', 'all')] [string] $Only = 'all',
  [string] $Setup,
  [string] $Work = (Join-Path ([IO.Path]::GetTempPath()) "detour-sandbox")
)
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$stand = Join-Path $Work 'stand'; $out = Join-Path $Work 'out'
New-Item -ItemType Directory -Force $stand, $out | Out-Null
Get-ChildItem $out -File -ErrorAction SilentlyContinue | Remove-Item -Force

Push-Location $root
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build -p detour-svc --target-dir target\static
if ($LASTEXITCODE) { throw "сборка не прошла" }
Pop-Location
Copy-Item (Join-Path $root 'target\static\debug\detour-svc.exe') $stand -Force
Copy-Item $SingBox $stand -Force
Copy-Item (Join-Path $PSScriptRoot '*.ps1') $stand -Force
if ($Setup) { Copy-Item $Setup $stand -Force }
if ($Only -eq 'installer' -and -not $Setup) { throw "для -Only installer нужен -Setup <detour-setup-*.exe>" }

$cfg = "<Configuration><Networking>Enable</Networking><MappedFolders>" +
  "<MappedFolder><HostFolder>$stand</HostFolder><SandboxFolder>C:\stand</SandboxFolder><ReadOnly>true</ReadOnly></MappedFolder>" +
  "<MappedFolder><HostFolder>$out</HostFolder><SandboxFolder>C:\out</SandboxFolder><ReadOnly>false</ReadOnly></MappedFolder>" +
  "</MappedFolders></Configuration>"
$id = (wsb start --raw -c $cfg | ConvertFrom-Json).Id
Write-Host "песочница $id"
try {
  # Окно нужно: без сессии пользователя wsb exec -r ExistingLogin не работает.
  Start-Process wsb -ArgumentList 'connect', '--id', $id
  Start-Sleep 25
  if ($Only -in 'tun', 'all') {
    wsb exec --id $id -r System -c "powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\stand\test-tun.ps1" | Out-Null
    Get-Content (Join-Path $out 'result.txt') -Encoding utf8
  }
  if ($Only -in 'service', 'all') {
    $job = Start-Job { wsb exec --id $using:id -r System -c "powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\stand\test-service.ps1" }
    Start-Sleep 20
    wsb exec --id $id -r ExistingLogin -c "powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\stand\pipe-user.ps1" | Out-Null
    $null = Wait-Job $job -Timeout 300; Remove-Job $job -Force
    Get-Content (Join-Path $out 'service.txt') -Encoding utf8
  }
  if ($Only -in 'dpi', 'all') {
    # Архив zapret2 качаем внутри песочницы: на рабочей машине Defender метит его трояном.
    wsb exec --id $id -r System -c "powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\stand\dpi-fetch.ps1" | Out-Null
    wsb exec --id $id -r System -c "powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\stand\test-dpi.ps1" | Out-Null
    Get-Content (Join-Path $out 'dpi-fetch.txt') -Encoding utf8
    Get-Content (Join-Path $out 'dpi.txt') -Encoding utf8
  }
  if ($Only -in 'installer', 'all' -and $Setup) {
    wsb exec --id $id -r System -c "powershell.exe -NoProfile -ExecutionPolicy Bypass -File C:\stand\test-installer.ps1" | Out-Null
    Get-Content (Join-Path $out 'installer.txt') -Encoding utf8
  }
} finally {
  wsb stop --id $id | Out-Null
  Write-Host "песочница остановлена, отчёты в $out"
}
