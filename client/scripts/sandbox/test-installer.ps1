$ErrorActionPreference = 'Continue'
$out = 'C:\out'; $st = 'C:\stand'
$log = "$out\installer.txt"
"" | Out-File $log -Encoding utf8
function L($s) { "$(Get-Date -Format HH:mm:ss) $s" | Out-File $log -Append -Encoding utf8 }
function curlt($url) { "$(& curl.exe -s -o NUL --max-time 20 -w '%{http_code}' $url 2>&1)" }

# Клиент канала службы: панель ходит тем же кадром (u32 LE длина + JSON).
function Pipe($action, $params = @{}, $body = $null) {
  $req = @{ action = $action; params = $params }
  if ($null -ne $body) { $req['body'] = @{ kind = 'text'; data = $body } }
  $json = $req | ConvertTo-Json -Compress -Depth 6
  $cl = New-Object IO.Pipes.NamedPipeClientStream('.', 'detour', [IO.Pipes.PipeDirection]::InOut)
  try {
    $cl.Connect(8000)
    $b = [Text.Encoding]::UTF8.GetBytes($json)
    $cl.Write([BitConverter]::GetBytes([uint32]$b.Length), 0, 4)
    $cl.Write($b, 0, $b.Length); $cl.Flush()
    $hdr = New-Object byte[] 4
    if ($cl.Read($hdr, 0, 4) -ne 4) { return $null }
    $len = [BitConverter]::ToUInt32($hdr, 0)
    $buf = New-Object byte[] $len; $got = 0
    while ($got -lt $len) { $got += $cl.Read($buf, $got, $len - $got) }
    $r = [Text.Encoding]::UTF8.GetString($buf) | ConvertFrom-Json
    if ($r.body -is [string]) { try { return $r.body | ConvertFrom-Json } catch { return $r.body } }
    return $r.body
  } catch { return "<ошибка: $($_.Exception.Message)>" } finally { $cl.Dispose() }
}
function tun() {
  (Get-NetAdapter -IncludeHidden -ErrorAction SilentlyContinue |
    Where-Object { $_.InterfaceDescription -match 'sing-tun' -and $_.Status -ne 'Not Present' } |
    ForEach-Object { $_.Name }) -join ','
}
function rule() { if ((& netsh advfirewall firewall show rule name="Detour kill-switch" 2>&1) -match 'Detour') { 'есть' } else { 'нет' } }

$setup = Get-ChildItem $st -Filter 'detour-setup-*.exe' | Select-Object -First 1
L "установщик: $($setup.Name) ($([int]($setup.Length/1MB)) МБ)"
& $setup.FullName /S | Out-Null
Start-Sleep 8
$svc = Get-Service DetourSvc -ErrorAction SilentlyContinue
L "служба после установки: $($svc.Status)/$($svc.StartType)"
L "файлы: $((Get-ChildItem 'C:\Program Files\Detour' -File -ErrorAction SilentlyContinue | ForEach-Object { $_.Name }) -join ' ')"
$wv = (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue).pv
L "WebView2: $(if ($wv) { $wv } else { 'нет' })"
L "ярлык:$(Test-Path "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Detour\Detour.lnk")"
$s = Pipe 'status'
L "канал отвечает: платформа=$($s.platform) версия=$($s.version) sing-box=$($s.binaries.singbox_version) winws2=$($s.binaries.nfqws2_supported)"

# run.ps1 по этому флагу открывает окно приложения в сессии пользователя.
"" | Out-File "$out\installed.flag"
for ($i = 0; $i -lt 60 -and -not (Test-Path "$out\ui.done"); $i++) { Start-Sleep 2 }
L "окно: $((Get-Content "$out\window.txt" -Encoding utf8 -ErrorAction SilentlyContinue | Where-Object { $_ }) -join ' | ')"

# Движок обхода ставится из самого приложения — установщик его не несёт.
L "ставлю winws2: $((Pipe 'nfqws2_update_apply' @{} '') | ConvertTo-Json -Compress)"
for ($i = 0; $i -lt 60; $i++) {
  Start-Sleep 5
  $a = Pipe 'apply_log'
  if ($a.done) { break }
}
L "установка winws2: rc=$($a.rc) | $(($a.log -split "`n" | Where-Object { $_ }) -join ' | ')"
$s = Pipe 'status'
L "winws2 после установки: поддерживается=$($s.binaries.nfqws2_supported) версия=$($s.binaries.nfqws2_version)"

# Профиль-заглушка: локальный SOCKS, чтобы поднять TUN и проверить связку.
$null = Pipe 'profile_save' @{} '{"id":"main","name":"main","outbound":{"type":"socks","server":"127.0.0.1","server_port":18181,"version":"5"}}'
$null = Pipe 'domains' @{} "speed.cloudflare.com`n"
$null = Pipe 'zapret_domains' @{} "example.com`n"
$r = Pipe 'profile_activate' @{ name = 'main' }
Start-Sleep 3
L "VPN включён: $($r.ok) tun=$(tun)"
$r = Pipe 'bypass_set' @{ mode = 'zapret2' } ''
Start-Sleep 2
$bs = Pipe 'bypass_status'
L "обход DPI: $($r.ok)$($r.error) режим=$($bs.mode) работает=$($bs.running)"
L "сайт из списка обхода: $(curlt 'https://example.com/')"
$dl = Get-Content "$env:ProgramData\Detour\logs\dpi.log" -ErrorAction SilentlyContinue
L "лог winws2: строк $($dl.Count), совпадений hostlist $(($dl | Select-String 'include hostlist check for .* : positive').Count), десинхронизаций $(($dl | Select-String ': desync').Count)"

# Kill-switch: гасим движок руками и смотрим, закрылся ли выход и поднял ли его сторож.
$null = Pipe 'killswitch_set' @{ on = '1' } ''
L "kill-switch включён: $((Pipe 'killswitch_status').enabled), правило сейчас $(rule)"
Get-Process sing-box -ErrorAction SilentlyContinue | Stop-Process -Force
$seen = 'нет'; $sw = [Diagnostics.Stopwatch]::StartNew()
while ($sw.Elapsed.TotalSeconds -lt 8) {
  if ((rule) -eq 'есть') { $seen = "есть через $([int]$sw.Elapsed.TotalMilliseconds) мс"; break }
  Start-Sleep -Milliseconds 300
}
L "после падения выход закрыт: $seen (tun=$(tun))"
Start-Sleep 12
L "через 12 с: tun=$(tun) правило=$(rule) sing-box работает=$((Pipe 'status').singbox.running)"
L "сеть после восстановления: $(curlt 'https://example.com/')"
$null = Pipe 'killswitch_set' @{ on = '0' } ''
$null = Pipe 'singbox_stop' @{} ''
$null = Pipe 'bypass_stop' @{} ''
Start-Sleep 2

& 'C:\Program Files\Detour\uninstall.exe' /S | Out-Null
Start-Sleep 10
L "после удаления: служба=$((Get-Service DetourSvc -ErrorAction SilentlyContinue).Status) каталог=$(Test-Path 'C:\Program Files\Detour') правило=$(rule)"
L "сеть после удаления: $(curlt 'https://example.com/')"
L "DONE"
