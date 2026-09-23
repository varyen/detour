#!/bin/sh
# Сборка macOS-версии Detour. Запускать на самом маке: Tauri собирает .app
# только там, а демон ставится в /Library/LaunchDaemons.
#
#   client/installer/macos/build.sh <путь к sing-box> [идентификатор сертификата]
#
# Получается Detour.app с интерфейсом и detour-svc рядом; демон регистрирует
# сам себя (`detour-svc install`), он же дописывает якорь pf для защиты от
# утечки. Движок обхода (tpws) приложение ставит само — как на Windows.
set -eu

SINGBOX=${1:?нужен путь к sing-box}
SIGN_ID=${2:-}
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
VERSION=$(tr -d '\n\r' < "$ROOT/VERSION")
OUT="$ROOT/releases/client"
APP="$OUT/Detour.app"

echo "== панель"
( cd "$ROOT/panel" && npm run build:client )

echo "== служба и интерфейс"
# custom-protocol — иначе Tauri считает сборку отладочной и открывает в окне
# devUrl (localhost:5199) вместо встроенной панели.
( cd "$ROOT/client" && cargo build --release -p detour-svc \
    && cargo build --release -p detour-app --features tauri/custom-protocol )

echo "== сборка Detour.app $VERSION"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$ROOT/client/target/release/detour-app" "$APP/Contents/MacOS/Detour"
cp "$ROOT/client/target/release/detour-svc" "$APP/Contents/MacOS/detour-svc"
cp "$SINGBOX" "$APP/Contents/MacOS/sing-box"
# mihomo — сайдкар AmneziaWG (sing-box этот протокол не умеет)
MIHOMO_VERSION=${MIHOMO_VERSION:-1.19.31}
case "$(uname -m)" in arm64) MARCH=arm64 ;; *) MARCH=amd64-v1 ;; esac
curl -sfL --max-time 600 \
    "https://github.com/MetaCubeX/mihomo/releases/download/v$MIHOMO_VERSION/mihomo-darwin-$MARCH-v$MIHOMO_VERSION.gz" \
    | gunzip > "$APP/Contents/MacOS/mihomo"
chmod 0755 "$APP/Contents/MacOS/mihomo"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Detour</string>
    <key>CFBundleIdentifier</key><string>com.detour.app</string>
    <key>CFBundleExecutable</key><string>Detour</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

if [ -n "$SIGN_ID" ]; then
    echo "== подпись"
    # Демон и движки — отдельные исполняемые файлы, подписывать надо каждый.
    for f in "$APP/Contents/MacOS/"*; do codesign --force --options runtime --sign "$SIGN_ID" "$f"; done
    codesign --force --options runtime --sign "$SIGN_ID" "$APP"
else
    echo "ВНИМАНИЕ: без подписи Gatekeeper не пустит приложение на чужой машине"
fi

echo "== образ"
DMG="$OUT/Detour-$VERSION.dmg"
rm -f "$DMG"
hdiutil create -volname "Detour" -srcfolder "$APP" -ov -format ULFO "$DMG" >/dev/null
echo "готово: $DMG"

cat <<'NOTE'

Первый запуск на целевой машине:
  sudo /Applications/Detour.app/Contents/MacOS/detour-svc install
Демон поднимется сам и будет подниматься после перезагрузки; интерфейс
запускается обычным способом и ходит к нему по /var/run/detour.sock.
Удаление: sudo /Applications/Detour.app/Contents/MacOS/detour-svc uninstall
NOTE
