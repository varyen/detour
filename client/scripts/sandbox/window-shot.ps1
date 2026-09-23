# Окно приложения в сессии пользователя: должна открыться встроенная панель,
# а не devUrl (так было, когда detour-app собирали без custom-protocol).
$out = 'C:\out'
$log = "$out\window.txt"
"" | Out-File $log -Encoding utf8
$p = Start-Process 'C:\Program Files\Detour\detour-app.exe' -PassThru
Start-Sleep 20
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$b = [Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object Drawing.Bitmap $b.Width, $b.Height
$g = [Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($b.Location, [Drawing.Point]::Empty, $b.Size)
$bmp.Save("$out\window.png", [Drawing.Imaging.ImageFormat]::Png)
$p.Refresh()
"процесс жив: $(-not $p.HasExited), окно: '$($p.MainWindowTitle)'" | Out-File $log -Append -Encoding utf8
try {
  Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
  $cond = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ProcessIdProperty, $p.Id)
  $wins = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, $cond)
  foreach ($w in $wins) {
    $texts = $w.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition) |
      ForEach-Object { $_.Current.Name } | Where-Object { $_ } | Select-Object -First 12
    "окно '$($w.Current.Name)': $($texts -join ' / ')" | Out-File $log -Append -Encoding utf8
  }
} catch { "uia: $($_.Exception.Message)" | Out-File $log -Append -Encoding utf8 }
$wv = Get-Process msedgewebview2 -ErrorAction SilentlyContinue
"webview2: процессов $($wv.Count)" | Out-File $log -Append -Encoding utf8
$c = (Get-NetTCPConnection -State SynSent, Established -RemotePort 5199 -ErrorAction SilentlyContinue).Count
"обращений к localhost:5199: $c" | Out-File $log -Append -Encoding utf8
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
