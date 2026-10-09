#!/bin/sh
# Установка Detour на обычный Linux-сервер (Debian/Ubuntu и любой другой с
# Docker): VPN-сервер (AmneziaWG и VLESS-Reality), клиенты которого выходят в
# интернет через профили и цепочки Detour, с его маршрутами.
#
#   sh install.sh [--port 51820] [--vless-port 443] [--endpoint <IP или домен>]
#                 [--panel-port 8080]
#                 [--panel-public] [--name detour] [--uninstall]
#
# Что делает:
#   1. проверяет Docker и грузит модули netfilter в ядро хоста (контейнер
#      пользуется ядром хоста, а сам модули грузить не может);
#   2. собирает образ из linux/Dockerfile (OpenWrt 25.12 + Detour + AmneziaWG);
#   3. запускает контейнер в своей сети. Файрвол Detour живёт в сетевом
#      пространстве контейнера и файрвол хоста не трогает;
#   4. настройки — в томах <name>-detour, -singbox, -config, -zapret: они
#      переживают пересоздание контейнера и обновление образа.
#
# Панель по умолчанию открыта только на 127.0.0.1 хоста: снаружи к ней ходят
# через SSH-туннель (ssh -L 8080:127.0.0.1:8080 root@сервер). --panel-public
# публикует её на все адреса — тогда сразу смените пароль и выпустите сертификат.

set -eu

NAME=detour
PORT=51820
VPORT=""
PANEL_PORT=8080
ENDPOINT=""
PANEL_PUBLIC=0
UNINSTALL=0
HERE=$(cd "$(dirname "$0")" && pwd)

while [ $# -gt 0 ]; do
    case "$1" in
        --port) PORT=$2; shift 2 ;;
        --vless-port) VPORT=$2; shift 2 ;;
        --endpoint) ENDPOINT=$2; shift 2 ;;
        --panel-port) PANEL_PORT=$2; shift 2 ;;
        --panel-public) PANEL_PUBLIC=1; shift ;;
        --name) NAME=$2; shift 2 ;;
        --uninstall) UNINSTALL=1; shift ;;
        -h|--help) sed -n '2,26p' "$0"; exit 0 ;;
        *) echo "неизвестный параметр: $1" >&2; exit 2 ;;
    esac
done

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
die() { printf 'ОШИБКА: %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" = 0 ] || die "запустите от root"
command -v docker >/dev/null 2>&1 || die "нужен Docker: https://docs.docker.com/engine/install/"

if [ "$UNINSTALL" = 1 ]; then
    say "Удаляю контейнер $NAME (тома с настройками остаются: docker volume rm $NAME-detour $NAME-singbox $NAME-config $NAME-zapret)"
    docker rm -f "$NAME" >/dev/null 2>&1 || true
    docker network rm "$NAME-net" >/dev/null 2>&1 || true
    rm -f /etc/modules-load.d/detour.conf
    exit 0
fi

case "$PORT" in ''|*[!0-9]*) die "--port — число" ;; esac
case "$(uname -m)" in
    x86_64|amd64) OWRT=x86-64-25.12.0 ;;
    aarch64|arm64) OWRT=armsr-armv8-25.12.0 ;;
    *) die "архитектура $(uname -m) не поддерживается (нужна x86_64 или arm64)" ;;
esac

say "Модули ядра для перехвата трафика"
MODS="tun ip_set ip_set_hash_ip ip_set_hash_net xt_set xt_REDIRECT xt_TPROXY nft_tproxy
xt_mark xt_multiport xt_recent xt_comment xt_conntrack nf_nat nft_chain_nat iptable_nat iptable_mangle veth"
missing=""
for m in $MODS; do
    modprobe "$m" 2>/dev/null || missing="$missing $m"
done
printf '%s\n' $MODS > /etc/modules-load.d/detour.conf
[ -z "$missing" ] || echo "   не загрузились:$missing — часть функций может не работать"

if [ -z "$ENDPOINT" ]; then
    ENDPOINT=$(curl -fsS4 --max-time 8 https://api.ipify.org 2>/dev/null || true)
    [ -n "$ENDPOINT" ] || die "не удалось узнать внешний IP — укажите --endpoint"
fi
# VLESS-Reality лучше всего прячется на 443; занят на хосте — 8443.
tcp_busy() {
    if command -v ss >/dev/null 2>&1; then
        ss -Hltn "sport = :$1" 2>/dev/null | grep -q .
    else
        netstat -lnt 2>/dev/null | awk -v p=":$1" '$4 ~ p"$" { f = 1 } END { exit !f }'
    fi
}
if [ -z "$VPORT" ]; then
    VPORT=443
    tcp_busy 443 && VPORT=8443
fi
say "Внешний адрес для клиентов: $ENDPOINT — AmneziaWG $PORT/udp, VLESS $VPORT/tcp"

say "Сборка образа (OpenWrt $OWRT + Detour + AmneziaWG) — несколько минут"
docker build --build-arg OWRT="$OWRT" -t "$NAME-box" "$HERE"

docker network inspect "$NAME-net" >/dev/null 2>&1 || docker network create "$NAME-net" >/dev/null
docker rm -f "$NAME" >/dev/null 2>&1 || true

PANEL_BIND=127.0.0.1:
[ "$PANEL_PUBLIC" = 1 ] && PANEL_BIND=""

say "Запуск контейнера $NAME"
# --privileged: OpenWrt внутри управляет своим файрволом и интерфейсами (tun,
# ipset, iptables) — без него procd/fw4 не поднимутся. Сеть при этом своя.
docker run -d --name "$NAME" --hostname "$NAME" --restart unless-stopped \
    --privileged --network "$NAME-net" \
    -p "${PANEL_BIND}${PANEL_PORT}:80/tcp" -p "$PORT:$PORT/udp" -p "$VPORT:$VPORT/tcp" \
    -e DETOUR_SERVER_PORT="$PORT" -e DETOUR_VLESS_PORT="$VPORT" -e DETOUR_ENDPOINT="$ENDPOINT" -e DETOUR_PANEL_WAN="$PANEL_PUBLIC" \
    -v "$NAME-detour:/etc/detour" -v "$NAME-singbox:/etc/sing-box" -v "$NAME-config:/etc/config" \
    -v "$NAME-zapret:/etc/zapret-tpws" \
    "$NAME-box" >/dev/null

i=0
until docker exec "$NAME" sh -c 'netstat -lnt 2>/dev/null | grep -q ":80 "' 2>/dev/null; do
    i=$((i + 1)); [ "$i" -gt 60 ] && die "панель не поднялась — docker logs $NAME"
    sleep 1
done

cat <<EOF

Detour запущен.

  Панель:   http://${PANEL_BIND:-<IP сервера>:}${PANEL_PORT}/detour/
EOF
[ "$PANEL_PUBLIC" = 1 ] || cat <<EOF
            (доступна только с сервера; со своего компьютера:
             ssh -L ${PANEL_PORT}:127.0.0.1:${PANEL_PORT} root@$ENDPOINT
             и откройте http://127.0.0.1:${PANEL_PORT}/detour/)
EOF
cat <<EOF
  Дальше:   задайте пароль панели, добавьте профиль или подписку, затем
            «Сервисы → Свой VPN-сервер» — включить и завести клиентов.
            Порты сервера уже совпадают с опубликованными ($PORT/udp, $VPORT/tcp).
  Удалить:  sh $0 --uninstall
EOF
