#!/bin/sh
# Сборка iOS-версии Detour: от sing-box до неподписанного .ipa. Нужен Mac с полным
# Xcode. Проверено на ВМ (Intel, macOS 26, Xcode 26.6, Go 1.27, tauri-cli 2.11.4).
#
#   client/scripts/ios/build.sh [рабочий каталог]
#
# Результат — releases/client/Detour-<VERSION>-unsigned.ipa. Без подписи его можно
# поставить только установщиком, который подпись не проверяет; а VPN-туннель без
# entitlement'а Network Extension на iOS не поднимется — см. client/app/ios/README.md.
#
# Нужно в PATH: xcodebuild, go, cargo (+ цели aarch64-apple-ios), cargo-tauri,
# xcodegen, pod (CocoaPods, нужен Ruby >= 3.0), ideviceinfo (libimobiledevice).
# Последние три tauri-cli пытается доставить сам через brew — на нестандартной macOS
# brew собирает всё из исходников часами, проще поставить заранее.
set -eu

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
WORK=${1:-$HOME/detour-ios}
VERSION=$(tr -d '\n\r' < "$ROOT/VERSION")
SINGBOX_VERSION=1.13.21
TAGS=with_gvisor,with_quic,with_wireguard,with_utls,with_clash_api,badlinkname,tfogo_checklinkname0
IOS="$ROOT/client/app/ios"
APPLE="$ROOT/client/app/gen/apple"
OUT="$ROOT/releases/client"
mkdir -p "$WORK" "$OUT"

command -v xcodebuild >/dev/null && xcodebuild -version >/dev/null 2>&1 \
    || { echo "нужен полный Xcode (xcode-select -s /Applications/Xcode.app)"; exit 1; }
for tool in go cargo xcodegen pod ideviceinfo; do
    command -v "$tool" >/dev/null || { echo "нет $tool в PATH (см. шапку скрипта)"; exit 1; }
done
cargo tauri --version >/dev/null 2>&1 || { echo "нет cargo-tauri (cargo install tauri-cli)"; exit 1; }
# CocoaPods без UTF-8 в окружении ругается и иногда падает.
export LANG=${LANG:-en_US.UTF-8}

echo "== Libbox.xcframework"
if [ ! -d "$WORK/Libbox.xcframework" ]; then
    [ -d "$WORK/sing-box" ] || git clone --depth 1 --branch "v$SINGBOX_VERSION" \
        https://github.com/SagerNet/sing-box.git "$WORK/sing-box"
    GOBIN="$WORK/bin" go install github.com/sagernet/gomobile/cmd/gomobile@v0.1.12
    GOBIN="$WORK/bin" go install github.com/sagernet/gomobile/cmd/gobind@v0.1.12
    ( cd "$WORK/sing-box" && PATH="$WORK/bin:$PATH" gomobile bind -v \
        -target ios,iossimulator -iosversion 15.0 -libname=box -trimpath -buildvcs=false \
        -ldflags "-X github.com/sagernet/sing-box/constant.Version=$SINGBOX_VERSION -X internal/godebug.defaultGODEBUG=multipathtcp=0 -checklinkname=0 -s -w -buildid=" \
        -tags "$TAGS" -o "$WORK/Libbox.xcframework" ./experimental/libbox )
fi
# gomobile кладёт в framework пустой Info.plist (<dict/> без ключей), а Xcode 26 такой
# внедрять в приложение отказывается.
for plist in "$WORK"/Libbox.xcframework/*/Libbox.framework; do
    cp "$IOS/Libbox-Info.plist" "$plist/Info.plist"
done

echo "== панель"
( cd "$ROOT/panel" && npm run build:client )

echo "== проект Xcode"
( cd "$ROOT/client/app" && [ -d gen/apple ] || cargo tauri ios init )
mkdir -p "$APPLE/Frameworks" "$APPLE/DetourTunnel"
rm -rf "$APPLE/Frameworks/Libbox.xcframework"
cp -R "$WORK/Libbox.xcframework" "$APPLE/Frameworks/"
cp "$IOS/App/DetourTunnel.swift" "$APPLE/Sources/"
cp "$IOS/App/Detour.entitlements" "$APPLE/detour-app_iOS/detour-app_iOS.entitlements"
cp "$IOS/Extension/"* "$APPLE/DetourTunnel/"
sed "s/@VERSION@/$VERSION/g" "$IOS/project.yml" > "$APPLE/project.yml"
( cd "$APPLE" && xcodegen generate )

echo "== сборка"
# `cargo tauri ios build` сам поднимает канал, по которому фаза «Build Rust Code»
# получает параметры: голый xcodebuild падает на ней. Завершается он ошибкой
# «exportArchive: No Team Found» — экспорт без команды разработчика невозможен,
# а нужен нам только архив, .ipa собираем из него сами.
ARCHIVE="$APPLE/build/detour-app_iOS.xcarchive"
rm -rf "$ARCHIVE"
( cd "$ROOT/client/app" && cargo tauri ios build --ci -t aarch64 ) \
    || echo "(ошибка экспорта ожидаема — проверяю, что архив есть)"
APP="$ARCHIVE/Products/Applications/Detour.app"
[ -d "$APP/PlugIns/DetourTunnel.appex" ] \
    || { echo "архив не собрался: нет Detour.app с расширением DetourTunnel"; exit 1; }

echo "== .ipa"
IPA="$OUT/Detour-$VERSION-unsigned.ipa"
TMP=$(mktemp -d)
mkdir "$TMP/Payload"
cp -R "$APP" "$TMP/Payload/"
rm -f "$IPA"
( cd "$TMP" && zip -qry "$IPA" Payload )
rm -rf "$TMP"
echo "готово: $IPA"
