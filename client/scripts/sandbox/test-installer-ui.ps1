# Установщик с окнами в сессии пользователя: тексты страниц через UIAutomation
# и снимки. Ключ удаления — в WOW6432Node: makensis 32-битный. Ловит кракозябры — makensis без /INPUTCHARSET UTF8 читал .nsi в ANSI.
$out = 'C:\out'; $st = 'C:\stand'
$log = "$out\installer-ui.txt"
"" | Out-File $log -Encoding utf8
function L($s) { $s | Out-File $log -Append -Encoding utf8 }
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, UIAutomationClient, UIAutomationTypes
function Shot($name) {
  $b = [Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bmp = New-Object Drawing.Bitmap $b.Width, $b.Height
  [Drawing.Graphics]::FromImage($bmp).CopyFromScreen($b.Location, [Drawing.Point]::Empty, $b.Size)
  $bmp.Save("$out\$name.png", [Drawing.Imaging.ImageFormat]::Png)
}
function Win($procId) {
  $c = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ProcessIdProperty, $procId)
  [Windows.Automation.AutomationElement]::RootElement.FindFirst([Windows.Automation.TreeScope]::Children, $c)
}
function Texts($w) {
  $w.FindAll([Windows.Automation.TreeScope]::Descendants, [Windows.Automation.Condition]::TrueCondition) |
    ForEach-Object { $_.Current.Name } | Where-Object { $_ }
}
function Click($w, $label) {
  $c = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::NameProperty, $label)
  $btn = $w.FindFirst([Windows.Automation.TreeScope]::Descendants, $c)
  if (-not $btn) { L "нет кнопки '$label'"; return $false }
  $btn.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern).Invoke(); $true
}
$setup = Get-ChildItem $st -Filter 'detour-setup-*.exe' | Select-Object -First 1
$p = Start-Process $setup.FullName -PassThru
$steps = @(@{ shot = 'ui-1-dir'; click = 'Установить' }, @{ shot = 'ui-2-finish'; click = $null })
foreach ($s in $steps) {
  for ($i = 0; $i -lt 90; $i++) {
    Start-Sleep 2
    $w = Win $p.Id
    if (-not $w) { continue }
    $t = Texts $w
    if ($s.click -and ($t -contains $s.click)) { break }
    if (-not $s.click -and ($t -contains 'Готово')) { break }
  }
  Start-Sleep 1
  Shot $s.shot
  L "[$($s.shot)] окно '$($w.Current.Name)': $($t -join ' / ')"
  if ($s.click) { [void](Click $w $s.click) }
}
$cb = (Texts $w | Where-Object { $_ -match 'Detour' }) -join ' / '
L "строки с Detour на последней странице: $cb"
# Галочку «Открыть Detour» снимаем: окно приложения здесь не проверяем.
$c = New-Object Windows.Automation.PropertyCondition([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::CheckBox)
$chk = $w.FindFirst([Windows.Automation.TreeScope]::Descendants, $c)
if ($chk) { $chk.GetCurrentPattern([Windows.Automation.TogglePattern]::Pattern).Toggle() }
[void](Click $w 'Готово')
Start-Sleep 3
L "ярлыки: $((Get-ChildItem "$env:ProgramData\Microsoft\Windows\Start Menu\Programs\Detour" -ErrorAction SilentlyContinue | ForEach-Object { $_.Name }) -join ' | ')"
L "служба: $((Get-Service DetourSvc -ErrorAction SilentlyContinue).Status)"
$u = Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Detour' -ErrorAction SilentlyContinue
L "в «Программах»: $($u.DisplayName) $($u.DisplayVersion) / $($u.Publisher)"
"" | Out-File "$out\ui-installer.done"
