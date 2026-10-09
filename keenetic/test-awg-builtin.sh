#!/opt/bin/sh
# Detour / Keenetic — живой тест встроенного WireGuard KeeneticOS с обфускацией
# AmneziaWG (ASC). Пара к test-awg-userspace.sh: тот же сервер, тот же файл,
# тот же замер — чтобы сравнить ядро с userspace.
#
#   wget -qO- https://raw.githubusercontent.com/varyen/detour/main/keenetic/test-awg-builtin.sh \
#     | sh -s -- <адрес тестового сервера>
#
# Нужен компонент «WireGuard VPN». Туннель создаётся через RCI (localhost:79,
# команда parse = строка CLI) под именем Wireguard9 и удаляется в конце, в том
# числе при Ctrl+C. Конфиг не сохраняется (system configuration save не зовём).

export PATH="/opt/bin:/opt/sbin:/usr/bin:/usr/sbin:/bin:/sbin"
ok()   { printf '  [ OK ] %s\n' "$*"; }
bad()  { printf '  [FAIL] %s\n' "$*"; }
info() { printf '  [info] %s\n' "$*"; }
hdr()  { printf '\n=== %s ===\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

HOST="$1"
[ -n "$HOST" ] || { echo "usage: sh -s -- <host>"; exit 1; }
IFN=Wireguard9; TUNIP=10.66.0.9; SRVIP=10.66.0.1

# rci <строка CLI> — выполнить через RCI, ответ в stdout
rci() {
    wget -q -T 20 -O - --header 'Content-Type: application/json' \
        --post-data "{\"parse\":\"$1\"}" http://localhost:79/rci/ 2>&1
}
# run <строка CLI> — выполнить и показать, чем ответил роутер
run() {
    r=$(rci "$1")
    msg=$(echo "$r" | sed -n 's/.*"message": *"\([^"]*\)".*/\1/p' | head -2 | tr '\n' ' ')
    if echo "$r" | grep -q '"error"'; then bad "$1 → $msg"; return 1; fi
    ok "$1${msg:+ → $msg}"
}

cleanup() {
    rci "no ip route $SRVIP 255.255.255.255 $IFN" >/dev/null
    rci "no interface $IFN" >/dev/null
}
trap cleanup EXIT INT TERM

now() { awk '{printf "%d", $1 * 100}' /proc/uptime; }
cpu() { awk '/^cpu / {t=0; for(i=2;i<=NF;i++) t+=$i; print t-$5-$6, t}' /proc/stat; }
fetch() {
    set -- "$1" "$2" "$(cpu)" "$(now)"
    if have curl; then
        r=$(curl -s -o /dev/null -m 60 -w '%{http_code} %{size_download}' "$1")
    else
        wget -q -T 60 -O /tmp/dtst-dl "$1" && r="200 $(wc -c < /tmp/dtst-dl)" || r="000 0"
        rm -f /tmp/dtst-dl
    fi
    t1=$(now); c1=$(cpu)
    echo "$r $3 $c1 $4 $t1" | awk -v lb="$2" '{
        code=$1; size=$2; dt=($8-$7)/100; if (dt <= 0) dt = 0.01
        busy=($5-$3); tot=($6-$4); if (tot <= 0) tot = 1
        printf "  [%s] %s: HTTP %s, %.1f МБ за %.1f с = %.1f Мбит/с; CPU занят %d%%\n",
            (code == "200" ? " OK " : "FAIL"), lb, code, size/1048576, dt, size*8/dt/1000000, busy*100/tot
    }'
}

hdr "1. RCI и компонент"
v=$(rci "show version")
echo "$v" | grep -q '"release"' || { bad "RCI на localhost:79 не отвечает: $(echo "$v" | head -c 200)"; exit 1; }
ok "RCI отвечает, прошивка $(echo "$v" | sed -n 's/.*"title": *"\([^"]*\)".*/\1/p' | head -1)"
comps=$(echo "$v" | sed -n 's/.*"components": *"\([^"]*\)".*/\1/p' | head -1)
case ",$comps," in *,wireguard,*) ok "компонент wireguard установлен";; *) bad "компонента wireguard в списке нет: $(echo "$comps" | tr ',' '\n' | grep -i wire | tr '\n' ' ')";; esac

hdr "2. Конфиг тестового клиента"
CONF=$(wget -q -T 20 -O - "http://$HOST:18080/kn.conf.b64" | base64 -d 2>/dev/null)
[ -n "$CONF" ] || { bad "конфиг с http://$HOST:18080/kn.conf.b64 не скачался"; exit 1; }
val() { echo "$CONF" | sed -n "s/^$1 *= *//p" | head -1; }
PRIV=$(val PrivateKey); PUB=$(val PublicKey); EP=$(val Endpoint)
ASC="$(val Jc) $(val Jmin) $(val Jmax) $(val S1) $(val S2) $(val H1) $(val H2) $(val H3) $(val H4)"
info "сервер $EP, ASC: $ASC"

hdr "3. Туннель $IFN через CLI"
cleanup
run "interface $IFN" || exit 1
run "interface $IFN description detour-test"
run "interface $IFN security-level public"
run "interface $IFN ip address $TUNIP 255.255.255.255"
run "interface $IFN ip mtu 1340"
run "interface $IFN wireguard private-key $PRIV" || exit 1
run "interface $IFN wireguard asc $ASC" || exit 1
run "interface $IFN wireguard peer $PUB" || exit 1
run "interface $IFN wireguard peer $PUB endpoint $EP"
run "interface $IFN wireguard peer $PUB keepalive-interval 25"
run "interface $IFN wireguard peer $PUB allow-ips $SRVIP 255.255.255.255"
run "interface $IFN up"
run "ip route $SRVIP 255.255.255.255 $IFN auto"
sleep 5
ping -c 3 -W 2 "$SRVIP" >/dev/null 2>&1
st=$(rci "show interface $IFN")
echo "$st" | grep -iE '"(state|link|connected|online|last-handshake|rxbytes|txbytes|endpoint)"' | \
    head -12 | sed 's/^ */  [info] /'
info "ядро видит: $(ls /sys/class/net | grep -iE 'nwg|wg' | tr '\n' ' ')"
ping -c 5 -W 2 "$SRVIP" 2>&1 | tail -2 | sed 's/^/  [info] /'

hdr "4. Скорость: один и тот же файл 32 МБ"
fetch "http://$HOST:18080/big.bin" "напрямую   "
fetch "http://$SRVIP:8000/big.bin" "через AWG  "
fetch "http://$HOST:18080/big.bin" "напрямую   "
fetch "http://$SRVIP:8000/big.bin" "через AWG  "

hdr "5. Уборка"
cleanup
rci "show interface $IFN" | grep -q '"error"\|not found\|unable' && ok "$IFN удалён" || info "проверьте вручную: show interface $IFN"
trap - EXIT INT TERM

hdr "Готово — пришлите весь вывод"
