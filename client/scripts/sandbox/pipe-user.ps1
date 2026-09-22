# Клиент панели работает в пользовательской сессии: проверяем доступ к каналу.
$r = 'нет ответа'
try {
  $c = New-Object IO.Pipes.NamedPipeClientStream('.', 'detour', [IO.Pipes.PipeDirection]::InOut)
  $c.Connect(8000)
  $j = [Text.Encoding]::UTF8.GetBytes('{"action":"status"}')
  $c.Write([BitConverter]::GetBytes([uint32]$j.Length), 0, 4)
  $c.Write($j, 0, $j.Length); $c.Flush()
  $h = New-Object byte[] 4; $null = $c.Read($h, 0, 4)
  $l = [BitConverter]::ToUInt32($h, 0)
  $b = New-Object byte[] $l; $n = 0
  while ($n -lt $l) { $n += $c.Read($b, $n, $l - $n) }
  $s = [Text.Encoding]::UTF8.GetString($b)
  $r = "$(whoami) admin=$(([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole('Administrators')) → $($s.Substring(0, [Math]::Min(120, $s.Length)))"
} catch { $r = "ошибка: $($_.Exception.Message)" }
[IO.File]::WriteAllText('C:\out\pipe-user.txt', $r)
