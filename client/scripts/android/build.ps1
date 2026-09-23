# Сборка Android-версии Detour.
#   pwsh client/scripts/android/build.ps1 [-Abi arm64|x86_64|both] [-Release]
#                                         [-Keystore keys/android-release.jks]
# С -Keystore release-APK подписывается (пароль — в $env:DETOUR_KS_PASS) и
# кладётся в releases/client как detour-client-android_<версия>_<abi>.apk.
# Ключ менять нельзя: APK с другой подписью не встанет поверх старой версии.
#
# Что происходит:
#  1. libbox.aar — ядро sing-box для Android. Собирается gomobile из исходников
#     sing-box: готового артефакта нет. Две мины по пути:
#       * `with_naive_outbound` тянет prebuilt cronet, который новый NDK не
#         линкует («unknown relocation») — тег выкинут;
#       * `with_tailscale` требует Go >= 1.25 (reflect.TypeAssert), а
#         `badlinkname` ломается на Go 1.27 — поэтому Tailscale выкинут, а Go
#         берётся ровно 1.24 отдельной распаковкой, не трогая системный.
#  2. tpws — движок обхода DPI, кладётся в jniLibs как `libtpws.so`: только так
#     Android разрешает исполнять файл из APK.
#  3. Панель и Tauri-проект собираются обычным `tauri android build`.
param(
  [ValidateSet('arm64', 'x86_64', 'both')] [string] $Abi = 'arm64',
  [switch] $Release,
  [string] $Keystore,
  [string] $Work = (Join-Path ([IO.Path]::GetTempPath()) "detour-android"),
  [string] $SingBoxVersion = "1.13.21",
  [string] $GoVersion = "1.24.7",
  [string] $ZapretVersion = "72.13",
  [string] $MihomoVersion = "1.19.31"
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..\..")).Path
$client = Join-Path $root 'client'
$sdk = "$env:LOCALAPPDATA\Android\Sdk"
$ndk = Get-ChildItem "$sdk\ndk" -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -Last 1
if (-not $ndk) { throw "нет NDK: sdkmanager --install 'ndk;27.2.12479018'" }
$jdk = Get-ChildItem "C:\Program Files\Microsoft\jdk-17*" -ErrorAction SilentlyContinue | Select-Object -Last 1
if (-not $jdk) { throw "нужен OpenJDK 17: winget install Microsoft.OpenJDK.17" }
New-Item -ItemType Directory -Force $Work | Out-Null

$libs = Join-Path $client 'app\gen\android\app\libs'
$jni = Join-Path $client 'app\gen\android\app\src\main\jniLibs'
New-Item -ItemType Directory -Force $libs, "$jni\arm64-v8a", "$jni\x86_64" | Out-Null

# --- 1. libbox.aar ---
if (-not (Test-Path "$libs\libbox.aar")) {
  $src = Join-Path $Work 'sing-box'
  if (-not (Test-Path $src)) {
    git clone --depth 1 --branch "v$SingBoxVersion" https://github.com/SagerNet/sing-box.git $src
  }
  $tags = 'with_gvisor,with_quic,with_wireguard,with_utls,with_clash_api,badlinkname,tfogo_checklinkname0,with_low_memory'
  $targets = switch ($Abi) { 'arm64' { 'android/arm64' } 'x86_64' { 'android/amd64' } default { 'android/arm64,android/amd64' } }
  # Свой Go нужной версии: системный не трогаем.
  $goRoot = Join-Path $Work "go$($GoVersion.Replace('.',''))\go"
  if (-not (Test-Path "$goRoot\bin\go.exe")) {
    $goZip = Join-Path $Work "go$GoVersion.zip"
    if (-not (Test-Path $goZip)) { & curl.exe -sL --max-time 900 -o $goZip "https://go.dev/dl/go$GoVersion.windows-amd64.zip" }
    Expand-Archive $goZip (Split-Path $goRoot) -Force
  }
  $clean = Join-Path $PSScriptRoot 'run-clean.ps1'
  foreach ($cmd in 'gomobile', 'gobind') {
    & $clean -File "$goRoot\bin\go.exe" -Args "install github.com/sagernet/gomobile/cmd/$cmd@v0.1.12" -WorkDir $Work -OutLog (Join-Path $Work "$cmd.log") -GoRoot $goRoot
  }
  # -checklinkname=0 обязателен: libbox подменяет приватную os.checkPidfdOnce
  # через //go:linkname, а с Go 1.23 линкер такие ссылки отвергает
  # («invalid reference to os.checkPidfdOnce»). multipathtcp=0 — по той же
  # причине, что и на роутере: MPTCP ломает приём соединений.
  $ld = "-X github.com/sagernet/sing-box/constant.Version=$SingBoxVersion" +
        " -X internal/godebug.defaultGODEBUG=multipathtcp=0 -checklinkname=0 -s -w -buildid="
  $bindArgs = "bind -v -o libbox.aar -target $targets -androidapi 24 -javapkg=io.nekohasekai" +
              " -libname=box -trimpath -buildvcs=false -ldflags `"$ld`" -tags $tags ./experimental/libbox"
  Write-Host "== libbox.aar ($targets), это надолго"
  & $clean -File "$env:USERPROFILE\go\bin\gomobile.exe" -Args $bindArgs -WorkDir $src -OutLog (Join-Path $Work 'libbox.log') -GoRoot $goRoot
  if (-not (Test-Path "$src\libbox.aar")) { throw "libbox не собрался, смотрите $Work\libbox.log" }
  Copy-Item "$src\libbox.aar" $libs -Force
} else {
  Write-Host "== libbox.aar уже собран"
}

# --- 2. tpws ---
if (-not (Test-Path "$jni\arm64-v8a\libtpws.so")) {
  Write-Host "== tpws из zapret $ZapretVersion"
  $zip = Join-Path $Work "zapret.zip"
  if (-not (Test-Path $zip)) {
    & curl.exe -sL --max-time 600 -o $zip "https://github.com/bol-van/zapret/releases/download/v$ZapretVersion/zapret-v$ZapretVersion.zip"
  }
  $x = Join-Path $Work 'zapret'
  Expand-Archive $zip $x -Force
  foreach ($pair in @(@('android-arm64', 'arm64-v8a'), @('android-x86_64', 'x86_64'))) {
    $bin = Get-ChildItem $x -Recurse -Directory | Where-Object { $_.Name -eq $pair[0] } | Select-Object -First 1
    if ($bin) { Copy-Item (Join-Path $bin.FullName 'tpws') "$jni\$($pair[1])\libtpws.so" -Force }
  }
}

# --- 2b. mihomo — сайдкар AmneziaWG, тоже из jniLibs (`libmihomo.so`) ---
if (-not (Test-Path "$jni\arm64-v8a\libmihomo.so")) {
  Write-Host "== mihomo $MihomoVersion"
  foreach ($pair in @(@('android-arm64-v8', 'arm64-v8a'), @('android-amd64', 'x86_64'))) {
    $gz = Join-Path $Work "mihomo-$($pair[0]).gz"
    & curl.exe -sfL --max-time 600 -o $gz "https://github.com/MetaCubeX/mihomo/releases/download/v$MihomoVersion/mihomo-$($pair[0])-v$MihomoVersion.gz"
    if ($LASTEXITCODE) { throw "не скачался mihomo $($pair[0])" }
    $in = [IO.File]::OpenRead($gz)
    $out = [IO.File]::Create("$jni\$($pair[1])\libmihomo.so")
    $z = New-Object IO.Compression.GZipStream($in, [IO.Compression.CompressionMode]::Decompress)
    $z.CopyTo($out)
    $z.Dispose(); $out.Dispose(); $in.Dispose()
  }
}

# --- 3. приложение ---
Push-Location (Join-Path $root 'panel')
npm run build:client
Pop-Location
$env:JAVA_HOME = $jdk.FullName
$env:ANDROID_HOME = $sdk
$env:NDK_HOME = $ndk.FullName
$env:Path = "$($jdk.FullName)\bin;$env:Path"
Push-Location $client
$targetArg = switch ($Abi) { 'arm64' { @('--target', 'aarch64') } 'x86_64' { @('--target', 'x86_64') } default { @() } }
$mode = if ($Release) { @() } else { @('--debug') }
npx tauri android build @mode @targetArg
if ($LASTEXITCODE) { Pop-Location; throw "сборка приложения не прошла" }
Pop-Location

$apk = Get-ChildItem (Join-Path $client 'app\gen\android\app\build\outputs\apk') -Recurse -Filter *.apk -ErrorAction SilentlyContinue |
  Sort-Object LastWriteTime | Select-Object -Last 1
if ($apk) { Write-Host "готово: $($apk.FullName) ($([int]($apk.Length/1MB)) МБ)" }

if ($Release -and $Keystore) {
  if (-not $env:DETOUR_KS_PASS) { throw "нет пароля ключа: `$env:DETOUR_KS_PASS" }
  $bt = Get-ChildItem "$sdk\build-tools" | Sort-Object { [version]$_.Name } | Select-Object -Last 1
  $version = (Get-Content (Join-Path $root 'VERSION') -Raw).Trim()
  $outDir = Join-Path $root 'releases\client'
  New-Item -ItemType Directory -Force $outDir | Out-Null
  $signed = Join-Path $outDir "detour-client-android_${version}_$Abi.apk"
  $aligned = Join-Path $Work 'aligned.apk'
  & (Join-Path $bt.FullName 'zipalign.exe') -p -f 4 $apk.FullName $aligned
  if ($LASTEXITCODE) { throw "zipalign не прошёл" }
  & (Join-Path $bt.FullName 'apksigner.bat') sign --ks (Resolve-Path $Keystore) --ks-key-alias detour `
    --ks-pass env:DETOUR_KS_PASS --out $signed $aligned
  if ($LASTEXITCODE) { throw "подпись не прошла" }
  & (Join-Path $bt.FullName 'apksigner.bat') verify --print-certs $signed | Select-Object -First 2
  Write-Host "подписано: $signed"
}
