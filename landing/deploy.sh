#!/usr/bin/env sh
# Выкладка лендинга detour.varyen.net на varyen.ru (порт 2222 берётся из ~/.ssh/config).
#
#   ./landing/deploy.sh
#
# Отправляются index.html, favicon.svg, img/*.webp и img/og.png (OG-превью) — сырые PNG
# (landing/raw/) на сервер не уезжают.
set -eu

HOST="${DETOUR_LANDING_HOST:-varyen.ru}"
DEST="${DETOUR_LANDING_DEST:-/var/www/varyen/detour.varyen.net}"
DIR="$(cd "$(dirname "$0")" && pwd)"
TGZ="$(mktemp -t detour-landing.XXXXXX.tgz)"

cd "$DIR"
tar czf "$TGZ" index.html favicon.svg img/*.webp img/og.png
scp -q "$TGZ" "$HOST:/tmp/detour-landing.tgz"
rm -f "$TGZ"

ssh "$HOST" "set -e
  mkdir -p '$DEST'
  cd '$DEST'
  tar xzf /tmp/detour-landing.tgz
  rm -f /tmp/detour-landing.tgz
  chown -R varyen:varyen '$DEST'
  du -sh '$DEST'"

echo "→ https://detour.varyen.net/"
