# Запуск команды с вручную собранным окружением: gomobile падает на служебных
# переменных Windows вида "=C:", а они наследуются от любого родителя, который
# менял каталог. ProcessStartInfo даёт очистить блок целиком.
param(
  [Parameter(Mandatory)] [string] $File,
  [Parameter(Mandatory)] [string] $Args,
  [Parameter(Mandatory)] [string] $WorkDir,
  [Parameter(Mandatory)] [string] $OutLog,
  # sing-box 1.13 линкуется только старым Go: новый переименовал внутренние
  # символы, на которые опирается его `badlinkname`.
  [string] $GoRoot
)
$psi = [Diagnostics.ProcessStartInfo]::new($File, $Args)
$psi.UseShellExecute = $false
$psi.WorkingDirectory = $WorkDir
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.Environment.Clear()
$jdk = "C:\Program Files\Microsoft\jdk-17.0.20.101-hotspot"
$sdk = "$env:LOCALAPPDATA\Android\Sdk"
$keep = @{
  SystemRoot       = $env:SystemRoot
  windir           = $env:windir
  TEMP             = $env:TEMP
  TMP              = $env:TEMP
  USERPROFILE      = $env:USERPROFILE
  LOCALAPPDATA     = $env:LOCALAPPDATA
  APPDATA          = $env:APPDATA
  HOMEDRIVE        = $env:HOMEDRIVE
  HOMEPATH         = $env:HOMEPATH
  NUMBER_OF_PROCESSORS = $env:NUMBER_OF_PROCESSORS
  JAVA_HOME        = $jdk
  ANDROID_HOME     = $sdk
  ANDROID_NDK_HOME = "$sdk\ndk\27.2.12479018"
  NDK_HOME         = "$sdk\ndk\27.2.12479018"
  PATH             = "$jdk\bin;$(if ($GoRoot) { "$GoRoot\bin" } else { 'C:\Program Files\Go\bin' });$env:USERPROFILE\go\bin;$env:SystemRoot\system32;$env:SystemRoot"
}
if ($GoRoot) { $keep.GOROOT = $GoRoot }
foreach ($k in $keep.Keys) { $psi.Environment[$k] = $keep[$k] }
$p = [Diagnostics.Process]::Start($psi)
$out = $p.StandardOutput.ReadToEndAsync()
$err = $p.StandardError.ReadToEndAsync()
$p.WaitForExit()
Set-Content -Path $OutLog -Value ($out.Result + "`n--- stderr ---`n" + $err.Result) -Encoding utf8
"rc=$($p.ExitCode)"
