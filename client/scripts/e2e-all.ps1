# Все e2e-наборы подряд, каждый на свежем каталоге данных и своей службе.
#   cargo build; pwsh client/scripts/e2e-all.ps1 -SingBox <путь к sing-box.exe> [-Work <каталог>] [-Only health,live]
param(
  [Parameter(Mandatory)] [string] $SingBox,
  [string] $Work = (Join-Path ([IO.Path]::GetTempPath()) "detour-e2e"),
  [string[]] $Only = @()
)
$svc = Join-Path $PSScriptRoot "..\target\debug\detour-svc.exe"
$api = "http://127.0.0.1:18080/cgi-bin/detour-api"
$suites = @(
  @{ name = "backend"; args = @(); live = $false },
  @{ name = "subscriptions"; args = @(); live = $false },
  @{ name = "health"; args = @($SingBox); live = $false },
  @{ name = "live"; args = @($SingBox); live = $true }
)
$failed = 0
foreach ($s in $suites) {
  if ($Only.Count -and $s.name -notin $Only) { continue }
  $data = Join-Path $Work $s.name
  if (Test-Path $data) { Remove-Item -Recurse -Force $data }
  New-Item -ItemType Directory -Force (Join-Path $data "bin") | Out-Null
  Copy-Item $SingBox (Join-Path $data "bin\sing-box.exe")
  $env:DETOUR_SINGBOX = $SingBox
  if ($s.live) { $env:DETOUR_DEV_NO_TUN = "1" } else { Remove-Item Env:DETOUR_DEV_NO_TUN -ErrorAction SilentlyContinue }
  $p = Start-Process -FilePath $svc -ArgumentList "run", "--data", $data, "--dev-http", "127.0.0.1:18080" -WindowStyle Hidden -PassThru `
    -RedirectStandardError (Join-Path $Work "$($s.name).svc.err") -RedirectStandardOutput (Join-Path $Work "$($s.name).svc.out")
  Start-Sleep 2
  Write-Host "=== $($s.name)"
  & node (Join-Path $PSScriptRoot "e2e-$($s.name).mjs") @($s.args)
  if ($LASTEXITCODE -ne 0) { $failed++ }
  try { Invoke-RestMethod "$api`?action=singbox_stop" -TimeoutSec 10 | Out-Null } catch {}
  Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
  Start-Sleep 1
}
Remove-Item Env:DETOUR_DEV_NO_TUN -ErrorAction SilentlyContinue
if ($failed) { Write-Host "`nнаборов с ошибками: $failed"; exit 1 }
Write-Host "`nвсе наборы зелёные"
