$ErrorActionPreference = 'Continue'
$st = 'C:\stand'; $out = 'C:\out'; $data = 'C:\detour-data'
$log = "$out\result.txt"
"" | Out-File $log -Encoding utf8
function L($s) { "$(Get-Date -Format HH:mm:ss) $s" | Out-File $log -Append -Encoding utf8 }
function api($a, $q = '', $body = $null) {
  $u = "http://127.0.0.1:18080/cgi-bin/detour-api?action=$a$q"
  try {
    if ($null -ne $body) { return Invoke-RestMethod $u -Method Post -Body $body -ContentType 'application/json' -TimeoutSec 120 }
    return Invoke-RestMethod $u -TimeoutSec 120
  } catch { return @{ ok = $false; error = "$($_.Exception.Message)" } }
}
function J($o) { $o | ConvertTo-Json -Compress -Depth 6 }
function curlt($url, $extra = @()) {
  $r = & curl.exe -s -o NUL --max-time 25 -w "%{http_code} %{remote_ip} %{size_download}B %{time_total}s" @extra $url 2>&1
  return "$r"
}
function net($tag) {
  $ad = Get-NetAdapter -IncludeHidden -ErrorAction SilentlyContinue | Where-Object { $_.InterfaceDescription -match 'sing-tun|Wintun' -and $_.Status -ne 'Not Present' }
  L "[$tag] tun-адаптер: $(($ad | ForEach-Object { "$($_.Name)=$($_.Status)" }) -join ', ')"
  $rt = Get-NetRoute -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object { $_.DestinationPrefix -in '0.0.0.0/0','0.0.0.0/1','128.0.0.0/1' } | ForEach-Object { "$($_.DestinationPrefix) via $($_.InterfaceAlias) m$($_.RouteMetric)" }
  L "[$tag] маршруты: $($rt -join '; ')"
  $dns = Get-DnsClientServerAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object { $_.ServerAddresses } | ForEach-Object { "$($_.InterfaceAlias)=$($_.ServerAddresses -join ',')" }
  L "[$tag] DNS: $($dns -join '; ')"
}

Get-Process sing-box, detour-svc -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 1
Remove-Item -Recurse -Force $data, "$out\up" -ErrorAction SilentlyContinue
L "whoami: $(whoami); ОС: $((Get-CimInstance Win32_OperatingSystem).Caption)"
$alias = (Get-NetRoute -DestinationPrefix 0.0.0.0/0 | Sort-Object RouteMetric | Select-Object -First 1).InterfaceAlias
L "физический адаптер: $alias"
net 'до'
L "напрямую 1.1.1.1/cdn-cgi/trace: $(curlt 'https://1.1.1.1/cdn-cgi/trace')"
L "IPv6 наружу: $(curlt 'https://[2606:4700:4700::1111]/cdn-cgi/trace' @('-6'))"
L "ping 1.1.1.1 до TUN: $((Test-Connection 1.1.1.1 -Count 2 -ErrorAction SilentlyContinue | Measure-Object ResponseTime -Average).Average) мс"

# «VPN-сервер»: отдельный sing-box, его исходящие привязаны к физическому адаптеру, мимо TUN.
$up = @{
  log = @{ level = 'info'; output = "$out\upstream.log"; timestamp = $true }
  dns = @{ servers = @(@{ type = 'https'; tag = 'd'; server = '1.1.1.1'; bind_interface = $alias }) }
  inbounds = @(@{ type = 'mixed'; listen = '127.0.0.1'; listen_port = 18181 })
  outbounds = @(@{ type = 'direct'; tag = 'direct'; bind_interface = $alias })
  route = @{ default_domain_resolver = 'd' }
}
New-Item -ItemType Directory -Force "$out\up" | Out-Null
$up | ConvertTo-Json -Depth 6 | Out-File "$out\up\c.json" -Encoding ascii
$upP = Start-Process "$st\sing-box.exe" -ArgumentList 'run', '-c', "$out\up\c.json", '-D', "$out\up" -WindowStyle Hidden -PassThru

$env:DETOUR_ALLOW_TUN = '1'; $env:DETOUR_SINGBOX = "$st\sing-box.exe"
$svcP = Start-Process "$st\detour-svc.exe" -ArgumentList 'run', '--data', $data, '--dev-http', '127.0.0.1:18080' -WindowStyle Hidden -PassThru -RedirectStandardError "$out\svc.err" -RedirectStandardOutput "$out\svc.out"
Start-Sleep 3
L "статус службы: $(J (api 'status').platform)"

$null = api 'profile_save' '' (J @{ id = 'main'; name = 'main'; outbound = @{ type = 'socks'; server = '127.0.0.1'; server_port = 18181; version = '5' } })
$null = api 'profile_save' '' (J @{ id = 'cf'; name = 'cf'; outbound = @{ type = 'trojan'; server = '1.1.1.1'; server_port = 443; password = 'x'; tls = @{ enabled = $true } } })
$null = api 'domains' '' "cloudflare.com`n"
$null = api 'health_urls' '' "Google|https://www.google.com/generate_204`n"
$null = api 'health_config' '' (J @{ speed = 0; auto_switch = 0 })

$r = api 'profile_activate' '&name=main'
L "profile_activate: $(J $r)"
Start-Sleep 3
$s = api 'status'
L "sing-box: running=$($s.singbox.running) pid=$($s.singbox.pid) режим=$($s.singbox.routing_mode)"
net 'TUN'
L "config.json есть: $(Test-Path "$data\run\config.json")"
Copy-Item "$data\run\config.json" "$out\config.json"

$null = api 'traffic_counters'
Start-Sleep 4
L "система → speed.cloudflare.com (должно через VPN): $(curlt 'https://speed.cloudflare.com/__down?bytes=3000000')"
L "система → www.google.com (напрямую): $(curlt 'https://www.google.com/')"
L "система → 1.1.1.1/cdn-cgi/trace: $(curlt 'https://1.1.1.1/cdn-cgi/trace')"
Start-Sleep 4
L "traffic_counters: $(J (api 'traffic_counters'))"
L "DNS через систему: speed.cloudflare.com → $((Resolve-DnsName speed.cloudflare.com -Type A -ErrorAction SilentlyContinue | Where-Object IPAddress | Select-Object -First 1).IPAddress); example.com → $((Resolve-DnsName example.com -Type A -ErrorAction SilentlyContinue | Where-Object IPAddress | Select-Object -First 1).IPAddress)"
L "DNS мимо (nslookup @8.8.8.8): $((nslookup example.org 8.8.8.8 2>&1 | Select-String 'Address' | Select-Object -Last 1))"
L "IPv6-литерал при TUN (ждём мгновенный отказ): $(curlt 'https://[2606:4700:4700::1111]/cdn-cgi/trace' @('-6'))"
L "системный ping 8.8.8.8 при TUN: $((ping -n 2 -w 2000 8.8.8.8 2>&1 | Select-String 'TTL=|Превышен|timed out|Заданный узел|недоступ' | ForEach-Object { $_.Line.Trim() }) -join ' | ')"
$upHits = (Select-String -Path "$out\upstream.log" -Pattern 'inbound connection to' -ErrorAction SilentlyContinue | ForEach-Object { ($_.Line -split 'inbound connection to ')[1] } | Sort-Object -Unique) -join ', '
L "upstream видел: $upHits"

L "ping_check cf (ICMP мимо туннеля): $(J (api 'ping_check' '&id=cf' ''))"
L "health_check main (проба при TUN): $(J (api 'health_check' '&id=main'))"
L "keepalive_check: $(J (api 'keepalive_check' '' ''))"

$r = api 'settings' '' (J @{ routing_mode = 'all-except' })
L "режим all-except: $(J $r)"
Start-Sleep 3
$null = api 'traffic_counters'
L "all-except: www.google.com: $(curlt 'https://www.google.com/')"
Start-Sleep 4
L "all-except traffic_counters: $(J (api 'traffic_counters'))"
$r = api 'settings' '' (J @{ routing_mode = 'proxy-list' })
Start-Sleep 2

# Падение sing-box без остановки: что остаётся от TUN и есть ли сеть.
$sb = Get-Process sing-box -ErrorAction SilentlyContinue | Where-Object { $_.Id -ne $upP.Id }
L "убиваю sing-box pid=$($sb.Id -join ',')"
$sb | Stop-Process -Force
Start-Sleep 2
net 'после падения'
L "после падения google: $(curlt 'https://www.google.com/')"
L "status после падения: running=$((api 'status').singbox.running)"

$r = api 'profile_activate' '&name=main'
Start-Sleep 3
L "снова поднят: $(J $r) running=$((api 'status').singbox.running)"
$r = api 'singbox_stop'
Start-Sleep 2
L "singbox_stop: $(J $r)"
net 'после стопа'
L "после стопа google: $(curlt 'https://www.google.com/')"
Copy-Item "$data\logs\*" $out -ErrorAction SilentlyContinue
Stop-Process -Id $svcP.Id, $upP.Id -Force -ErrorAction SilentlyContinue
L "DONE"
