#!/usr/bin/env sh
# Выкладка лендинга detour.varyen.net на varyen.ru (порт 2222 берётся из ~/.ssh/config).
#
#   ./landing/deploy.sh
#
# Отправляются index.html, favicon.svg, img/*.webp и img/og.png (OG-превью),
# сгенерированные фоны gen/, шрифты fonts/ и ролики video/ (mp4 + webm + постеры).
# Сырьё — landing/raw/, landing/raw-video/, landing/gen-src/ — на сервер не уезжает.
#
# Адреса картинок и роликов в index.html несут ?v=<дата>: имена файлов постоянные,
# при пересъёмке файл подменяется под тем же адресом, и без версии посетитель
# месяц видит старый кадр. При пересъёмке — обновить ?v= в index.html.
set -eu

HOST="${DETOUR_LANDING_HOST:-varyen.ru}"
DEST="${DETOUR_LANDING_DEST:-/var/www/varyen/detour.varyen.net}"
DIR="$(cd "$(dirname "$0")" && pwd)"
TGZ="$(mktemp -t detour-landing.XXXXXX.tgz)"

cd "$DIR"
tar czf "$TGZ" index.html favicon.svg img/*.webp img/og.png \
  gen/*.webp fonts/*.woff2 fonts/fonts.css \
  video/*.mp4 video/*.webm video/*.jpg
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
