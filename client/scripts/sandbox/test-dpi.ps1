$ErrorActionPreference = 'Continue'
$out = 'C:\out'; $dpi = 'C:\dpi'; $st = 'C:\stand'; $data = 'C:\detour-data'
$log = "$out\dpi.txt"
"" | Out-File $log -Encoding utf8
function L($s) { "$(Get-Date -Format HH:mm:ss) $s" | Out-File $log -Append -Encoding utf8 }
function curlt($url) { "$(& curl.exe -s -o NUL --max-time 20 -w '%{http_code} %{remote_ip}' $url 2>&1)" }

Get-Process winws2, sing-box, detour-svc -ErrorAction SilentlyContinue | Stop-Process -Force
Remove-Item -Recurse -Force $data -ErrorAction SilentlyContinue
Remove-Item "$out\winws2.log" -ErrorAction SilentlyContinue

# Адреса TUN фиксированы в рендере (172.19.0.1/30). Исключаем их, иначе winws2
# обрабатывает пакет дважды: на стыке с туннелем и на физическом интерфейсе.
# Фильтр подаём файлом: в нём пробелы, и одним аргументом он не доезжает.
$bin = Join-Path $dpi 'bin'
$wf = Join-Path $bin 'wf.txt'
'ip.SrcAddr != 172.19.0.1 and ip.DstAddr != 172.19.0.1' | Set-Content $wf -Encoding ascii
$strategy = @(
  '--wf-tcp-out=80,443', '--wf-udp-out=443',
  "--wf-raw-filter=@$wf",
  "--lua-init=@$bin\zapret-lib.lua", "--lua-init=@$bin\zapret-antidpi.lua", "--lua-init=@$bin\zapret-auto.lua",
  "--hostlist=$bin\hostlist.txt",
  '--filter-tcp=443', '--filter-l7=tls', '--payload=tls_client_hello',
  '--lua-desync=tcpseg:pos=0,midsld:ip_id=rnd:repeats=2'
)
L "dry-run: $(& "$bin\winws2.exe" @strategy --dry-run 2>&1 | Select-Object -Last 1)"
$p = Start-Process "$bin\winws2.exe" -ArgumentList ($strategy + @("--debug=@$out\winws2.log")) -WindowStyle Hidden -PassThru
Start-Sleep 3
L "winws2 живой: $(-not $p.HasExited)"
L "фильтр с исключением TUN попал в windivert: $((Get-Content "$out\winws2.log" -ErrorAction SilentlyContinue | Select-String 'SrcAddr != 172').Count) строк"

$env:DETOUR_ALLOW_TUN = '1'; $env:DETOUR_SINGBOX = "$st\sing-box.exe"
$svc = Start-Process "$st\detour-svc.exe" -ArgumentList 'run', '--data', $data, '--dev-http', '127.0.0.1:18080' -WindowStyle Hidden -PassThru -RedirectStandardError "$out\svc-dpi.err"
Start-Sleep 3
$api = 'http://127.0.0.1:18080/cgi-bin/detour-api?action='
Invoke-RestMethod "${api}profile_save" -Method Post -Body '{"id":"main","name":"main","outbound":{"type":"socks","server":"127.0.0.1","server_port":18181,"version":"5"}}' -ContentType 'application/json' | Out-Null
Invoke-RestMethod "${api}domains" -Method Post -Body "speed.cloudflare.com`n" | Out-Null
$r = Invoke-RestMethod "${api}profile_activate&name=main"
Start-Sleep 3
L "TUN: $((Get-NetAdapter -IncludeHidden | Where-Object { $_.InterfaceDescription -match 'sing-tun' -and $_.Status -ne 'Not Present' } | ForEach-Object { $_.Name }) -join ',') activate=$($r.ok)"
L "example.com при TUN: $(curlt 'https://example.com/')"
L "www.google.com при TUN: $(curlt 'https://www.google.com/')"
Start-Sleep 3
$lg = Get-Content "$out\winws2.log" -ErrorAction SilentlyContinue
L "строк в логе: $($lg.Count)"
L "пакетов со стороны TUN (172.19.0.1): $(($lg | Select-String 'src=172\.19\.0\.1|dst=172\.19\.0\.1').Count)"
L "десинхронизаций: $(($lg | Select-String ': desync').Count); совпадений hostlist: $(($lg | Select-String 'include hostlist check for .* : positive').Count)"

Invoke-RestMethod "${api}singbox_stop" | Out-Null
Start-Sleep 2
Stop-Process -Id $svc.Id, $p.Id -Force -ErrorAction SilentlyContinue
Start-Sleep 2
L "после остановки: сеть $(curlt 'https://example.com/')"
L "DONE"
