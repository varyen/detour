#!/opt/bin/sh
# Detour / Keenetic — живая проверка своего VPN-сервера до релиза.
#
#   wget -qO- https://raw.githubusercontent.com/varyen/detour/main/keenetic/test-server-live.sh | sh -s -- setup
#   wget -qO- https://raw.githubusercontent.com/varyen/detour/main/keenetic/test-server-live.sh | sh -s -- cleanup
#
# setup: кладёт новые detour-server, detour-update и netfilter-хук из ветки
# main поверх установленной панели (старые — в /opt/var/detour-srvtest), ставит
# ip-full, включает сервер (AmneziaWG + VLESS) и заводит клиента srvtest.
# Печатает его конфиг и ссылку — их нужно прислать целиком, по ним к роутеру
# подключаются снаружи.
# cleanup: удаляет клиента и сервер, возвращает прежние файлы.

export PATH="/opt/bin:/opt/sbin:/usr/bin:/usr/sbin:/bin:/sbin"
ok()   { printf '  [ OK ] %s\n' "$*"; }
bad()  { printf '  [FAIL] %s\n' "$*"; }
info() { printf '  [info] %s\n' "$*"; }
hdr()  { printf '\n=== %s ===\n' "$*"; }

REF=${2:-main}
BASE="https://raw.githubusercontent.com/varyen/detour/$REF"
BK=/opt/var/detour-srvtest
SRV=/opt/sbin/detour-server
HOOK=/opt/etc/ndm/netfilter.d/50-detour.sh
SET=/opt/etc/sing-box/settings.json
FILES="router_files/detour-server:$SRV router_files/detour-update:/opt/sbin/detour-update keenetic/ndm/netfilter.d/50-detour.sh:$HOOK"

fix_shebang() { sed -i '1s|^#!/bin/sh|#!/opt/bin/sh|' "$1"; }

case "$1" in
setup)
    hdr "1. Файлы из $REF"
    mkdir -p "$BK"
    for f in $FILES; do
        src=${f%%:*}; dst=${f#*:}
        [ -f "$BK/$(basename "$dst")" ] || cp -p "$dst" "$BK/$(basename "$dst")" 2>/dev/null
        if wget -q -T 30 -O "$dst.new" "$BASE/$src" && [ -s "$dst.new" ]; then
            fix_shebang "$dst.new"; chmod 755 "$dst.new"; mv "$dst.new" "$dst"
            ok "$dst"
        else
            rm -f "$dst.new"; bad "$src не скачался"; exit 1
        fi
    done
    [ -f "$BK/settings.json" ] || cp -p "$SET" "$BK/settings.json" 2>/dev/null
    "$HOOK" iptables nat >/dev/null 2>&1

    hdr "2. Компоненты (ip-full для VLESS)"
    /opt/sbin/detour-update server-apply 2>&1 | tail -4 | sed 's/^/  /'

    hdr "3. Сервер"
    "$SRV" status | sed -n 's/.*"backend":"\([^"]*\)".*/  [info] бэкенд: \1/p'
    r=$("$SRV" set enabled=1 awg=1); echo "  set enabled=1 awg=1 → $r"
    r=$("$SRV" set vless=1); echo "  set vless=1 → $r"
    r=$("$SRV" client-add srvtest); echo "  client-add srvtest → $r"
    ID=$(echo "$r" | sed -n 's/.*"id":"\([0-9a-f]*\)".*/\1/p')
    [ -n "$ID" ] || { bad "клиент не заведён"; tail -15 /opt/var/log/detour-server.log | sed 's/^/    /'; exit 1; }
    echo "$ID" > "$BK/client-id"
    st=$("$SRV" status)
    for k in backend running awg_running iface port routed; do
        printf '  [info] %s: %s\n' "$k" "$(echo "$st" | sed -n "s/.*\"$k\":\"\{0,1\}\([^,\"]*\).*/\1/p" | head -1)"
    done
    info "vless: $(echo "$st" | sed -n 's/.*"vless":{\([^}]*\)}.*/\1/p' | sed 's/"pubkey":"[^"]*",//')"
    info "интерфейс: $(ip -o link show "$(echo "$st" | sed -n 's/.*"iface":"\([^"]*\)".*/\1/p')" 2>&1 | cut -c1-70)"
    info "правила: $(wc -l < /opt/var/run/detour-server.fw 2>/dev/null) шт."
    iptables -S INPUT 2>/dev/null | head -6 | sed 's/^/    /'

    hdr "4. Прислать целиком (одноразовый тестовый клиент)"
    echo "AWG-CONF-B64: $("$SRV" client-conf "$ID" | base64 | tr -d '\n')"
    echo "VLESS-LINK: $("$SRV" client-link "$ID")"

    hdr "5. Журнал сервера"
    tail -12 /opt/var/log/detour-server.log | sed 's/^/    /'
    ;;
cleanup)
    hdr "Уборка"
    ID=$(cat "$BK/client-id" 2>/dev/null)
    [ -n "$ID" ] && "$SRV" client-del "$ID" >/dev/null 2>&1
    "$SRV" set enabled=0 >/dev/null 2>&1
    "$SRV" down >/dev/null 2>&1
    rm -rf /opt/etc/detour/server
    for f in $FILES; do
        dst=${f#*:}
        [ -f "$BK/$(basename "$dst")" ] && cp -p "$BK/$(basename "$dst")" "$dst" && ok "вернул $dst"
    done
    # Из vpn_redirect_ifaces — только наши интерфейсы, остальное в settings.json
    # могло измениться с момента setup.
    cur=$(sed -n 's/.*"vpn_redirect_ifaces"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$SET" | head -1)
    new=$(echo "$cur" | tr ', ' '\n\n' | grep -vE '^(nwg[0-9]+|dsrv[0-9]+)$' | grep . | tr '\n' ' ' | sed 's/ $//')
    sed -i "s/\(\"vpn_redirect_ifaces\"[[:space:]]*:[[:space:]]*\"\)[^\"]*\"/\1$new\"/" "$SET"
    info "vpn_redirect_ifaces: $(sed -n 's/.*"vpn_redirect_ifaces"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$SET")"
    "$HOOK" iptables nat >/dev/null 2>&1
    rm -rf "$BK"
    ok "готово"
    ;;
*)
    echo "usage: sh -s -- setup|cleanup [ref]"; exit 1 ;;
esac
