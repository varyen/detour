#!/bin/sh
# Сборка iOS-версии Detour. НЕ ЗАПУСКАЛОСЬ: нужен Mac с полным Xcode (iOS SDK),
# а на нашей ВМ стоят только Command Line Tools. Подробности —
# client/app/ios/README.md.
#
#   client/scripts/ios/build.sh [рабочий каталог]
#
# 1. Libbox.xcframework — sing-box для Apple, gomobile из исходников (как на
#    Android: -checklinkname=0, Go 1.24, без naive и tailscale).
# 2. Xcode-проект Tauri (`cargo tauri ios init`, один раз).
# 3. Swift-файлы и настройки расширения кладутся рядом — добавить их в проект
#    и завести цель расширения пока нужно руками (см. README).
set -eu

ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
WORK=${1:-$HOME/detour-ios}
SINGBOX_VERSION=1.13.21
TAGS=with_gvisor,with_quic,with_wireguard,with_utls,with_clash_api,badlinkname,tfogo_checklinkname0
mkdir -p "$WORK"

command -v xcodebuild >/dev/null && xcodebuild -version >/dev/null 2>&1 \
    || { echo "нужен полный Xcode (xcode-select -s /Applications/Xcode.app)"; exit 1; }

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

echo "== панель"
( cd "$ROOT/panel" && npm run build:client )

echo "== проект Xcode"
( cd "$ROOT/client/app" && [ -d gen/apple ] || cargo tauri ios init )

APPLE="$ROOT/client/app/gen/apple"
mkdir -p "$APPLE/Frameworks" "$APPLE/DetourTunnel"
rm -rf "$APPLE/Frameworks/Libbox.xcframework"
cp -R "$WORK/Libbox.xcframework" "$APPLE/Frameworks/"
cp "$ROOT/client/app/ios/App/DetourTunnel.swift" "$ROOT/client/app/ios/App/Detour.entitlements" "$APPLE/Sources/" 2>/dev/null \
    || cp "$ROOT/client/app/ios/App/"* "$APPLE/"
cp "$ROOT/client/app/ios/Extension/"* "$APPLE/DetourTunnel/"

cat <<'NOTE'

Файлы на месте. В Xcode (gen/apple/*.xcodeproj) один раз:
  1. File → New → Target → Network Extension, имя DetourTunnel,
     bundle id io.github.varyen.detour.tunnel; заменить сгенерированные файлы
     теми, что лежат в gen/apple/DetourTunnel/.
  2. Libbox.xcframework (gen/apple/Frameworks) — в обе цели: приложению Embed,
     расширению Do Not Embed.
  3. DetourTunnel.swift — в цель приложения; Detour.entitlements и
     DetourTunnel.entitlements — в Code Signing Entitlements своих целей.
  4. Signing & Capabilities: команда разработчика, Network Extensions →
     Packet Tunnel у обеих целей (нужен платный аккаунт Apple Developer).
Дальше обычная сборка: cargo tauri ios build (или Run в Xcode на устройстве).
NOTE
