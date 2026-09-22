# Короткая проверка после установки: ярлык на месте и окно приложения
# открывается (WebView2 в системе есть). Запускается в сессии пользователя.
$out = 'C:\out'
$log = "$out\app.txt"
function L($s) { "$(Get-Date -Format HH:mm:ss) $s" | Out-File $log -Append -Encoding utf8 }
"" | Out-File $log -Encoding utf8

$lnk = "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Detour\Detour.lnk"
L "ярлык в меню «Пуск»: $(Test-Path $lnk)"
L "WebView2 в системе: $(Test-Path 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}')"
$p = Start-Process 'C:\Program Files\Detour\detour-app.exe' -PassThru
Start-Sleep 12
$alive = -not $p.HasExited
$title = (Get-Process -Id $p.Id -ErrorAction SilentlyContinue).MainWindowTitle
L "приложение: живое=$alive окно='$title'"
if ($alive) { Stop-Process -Id $p.Id -Force }
L "DONE"
