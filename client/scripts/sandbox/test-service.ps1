$ErrorActionPreference = 'Continue'
$out = 'C:\out'; $svcDir = 'C:\svc'
$log = "$out\service.txt"
"" | Out-File $log -Encoding utf8
function L($s) { "$(Get-Date -Format HH:mm:ss) $s" | Out-File $log -Append -Encoding utf8 }

# Служба работает от SYSTEM, а C:\stand — папка пользователя песочницы: копируем рядом.
New-Item -ItemType Directory -Force $svcDir | Out-Null
Copy-Item C:\stand\detour-svc.exe, C:\stand\sing-box.exe $svcDir -Force

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
    if ($cl.Read($hdr, 0, 4) -ne 4) { return '<нет ответа>' }
    $len = [BitConverter]::ToUInt32($hdr, 0)
    $buf = New-Object byte[] $len; $got = 0
    while ($got -lt $len) { $got += $cl.Read($buf, $got, $len - $got) }
    return [Text.Encoding]::UTF8.GetString($buf)
  } catch { return "<ошибка: $($_.Exception.Message)>" } finally { $cl.Dispose() }
}
function tun($tag) {
  $a = Get-NetAdapter -IncludeHidden -ErrorAction SilentlyContinue | Where-Object { $_.InterfaceDescription -match 'sing-tun' -and $_.Status -ne 'Not Present' }
  L "[$tag] tun: $(($a | ForEach-Object { "$($_.Name)=$($_.Status)" }) -join ', ')"
}

L "install: $(& $svcDir\detour-svc.exe install 2>&1)"
Start-Sleep 3
$s = Get-Service DetourSvc -ErrorAction SilentlyContinue
L "служба: status=$($s.Status) startType=$($s.StartType) путь=$((Get-CimInstance Win32_Service -Filter "Name='DetourSvc'").PathName)"
L "каталог данных: $(Test-Path 'C:\ProgramData\Detour\settings.json') (settings.json)"

$r = Pipe 'status'
L "pipe status: $($r.Substring(0, [Math]::Min(220, $r.Length)))"
L "pipe profile_save: $(Pipe 'profile_save' @{} '{"id":"main","name":"main","outbound":{"type":"socks","server":"127.0.0.1","server_port":18181,"version":"5"}}')"
L "pipe domains: $(Pipe 'domains' @{} "cloudflare.com`n")"
tun 'до'
L "pipe profile_activate: $(Pipe 'profile_activate' @{ name = 'main' })"
Start-Sleep 3
tun 'после активации'
$r = Pipe 'status'
$j = $r | ConvertFrom-Json
L "running=$($j.body.singbox.running) pid=$($j.body.singbox.pid) профиль=$($j.body.singbox.active_profile)"

L "жду клиента из пользовательской сессии…"
for ($i = 0; $i -lt 40; $i++) { if (Test-Path "$out\pipe-user.txt") { break }; Start-Sleep 2 }
L "ответ пользовательской сессии: $(if (Test-Path "$out\pipe-user.txt") { (Get-Content "$out\pipe-user.txt" -Raw).Trim() } else { 'нет' })"

L "stop-service: $(Stop-Service DetourSvc -PassThru -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Status)"
Start-Sleep 3
tun 'после остановки службы'
L "sing-box жив после остановки: $((Get-Process sing-box -ErrorAction SilentlyContinue | Measure-Object).Count)"
L "сеть после остановки: $(& curl.exe -s -o NUL --max-time 15 -w '%{http_code}' https://www.google.com/)"
L "uninstall: $(& $svcDir\detour-svc.exe uninstall 2>&1)"
Copy-Item 'C:\ProgramData\Detour\logs\svc.log' "$out\svc-service.log" -ErrorAction SilentlyContinue
L "DONE"
