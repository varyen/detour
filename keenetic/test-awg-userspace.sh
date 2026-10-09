#!/opt/bin/sh
# Detour / Keenetic — живой тест userspace AmneziaWG (amneziawg-go) на роутере.
# Роутер на минуту становится клиентом тестового AWG-сервера и качает один и
# тот же файл напрямую и через туннель: так видно, работает ли TUN/AWG на
# этом ядре и сколько вытягивает процессор.
#
#   wget -qO- https://raw.githubusercontent.com/varyen/detour/main/keenetic/test-awg-userspace.sh \
#     | sh -s -- <адрес тестового сервера>
#
# Конфиг клиента скрипт берёт с того же сервера (http://<адрес>:18080/kn.conf.b64):
# длинный аргумент обрезается при вставке в терминал. Можно передать и сам —
# base64 первым аргументом, адрес вторым.
#
# Всё временное: интерфейс dtst0, файлы в /tmp/dtst-*. По завершении (и при
# Ctrl+C) всё снимается. Конфиг KeeneticOS и файрвол не меняются.

export PATH="/opt/bin:/opt/sbin:/usr/bin:/usr/sbin:/bin:/sbin"
ok()   { printf '  [ OK ] %s\n' "$*"; }
bad()  { printf '  [FAIL] %s\n' "$*"; }
info() { printf '  [info] %s\n' "$*"; }
hdr()  { printf '\n=== %s ===\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

if [ -n "$2" ]; then CONF_B64="$1"; HOST="$2"; else HOST="$1"; CONF_B64=""; fi
[ -n "$HOST" ] || { echo "usage: sh -s -- <host>"; exit 1; }
[ -n "$CONF_B64" ] || CONF_B64=$(wget -q -T 20 -O - "http://$HOST:18080/kn.conf.b64") || true
[ -n "$CONF_B64" ] || { echo "конфиг с http://$HOST:18080/kn.conf.b64 не скачался"; exit 1; }

BIN=/tmp/dtst-awg; CONF=/tmp/dtst-conf; IFN=dtst0; TUNIP=10.66.0.9; SRVIP=10.66.0.1
PID=""
cleanup() {
    [ -n "$PID" ] && kill "$PID" 2>/dev/null
    ip link del "$IFN" 2>/dev/null
    rm -f "$BIN" "$CONF" /var/run/amneziawg/"$IFN".sock
}
trap cleanup EXIT INT TERM

# сотые доли секунды с загрузки
now() { awk '{printf "%d", $1 * 100}' /proc/uptime; }
# занятость всех CPU, тики: «busy total»
cpu() { awk '/^cpu / {t=0; for(i=2;i<=NF;i++) t+=$i; print t-$5-$6, t}' /proc/stat; }
ptick() { [ -n "$PID" ] && awk '{print $14 + $15}' /proc/"$PID"/stat 2>/dev/null || echo 0; }

# fetch <url> <метка>: скорость, загрузка CPU всего и amneziawg-go
fetch() {
    set -- "$1" "$2" "$(cpu)" "$(ptick)" "$(now)"
    if have curl; then
        r=$(curl -s -o /dev/null -m 60 -w '%{http_code} %{size_download}' "$1")
    else
        wget -q -T 60 -O /tmp/dtst-dl "$1" && r="200 $(wc -c < /tmp/dtst-dl)" || r="000 0"
        rm -f /tmp/dtst-dl
    fi
    t1=$(now); c1=$(cpu); p1=$(ptick)
    echo "$r $3 $c1 $4 $p1 $5 $t1" | awk -v lb="$2" '{
        code=$1; size=$2; dt=($10-$9)/100; if (dt <= 0) dt = 0.01
        busy=($5-$3); tot=($6-$4); if (tot <= 0) tot = 1
        printf "  [%s] %s: HTTP %s, %.1f МБ за %.1f с = %.1f Мбит/с; CPU занят %d%%, amneziawg-go %d%% одного ядра\n",
            (code == "200" ? " OK " : "FAIL"), lb, code, size/1048576, dt, size*8/dt/1000000,
            busy*100/tot, ($8-$7)/dt
    }'
}

hdr "1. ndmc (CLI KeeneticOS)"
for p in /bin/ndmc /usr/bin/ndmc /sbin/ndmc /usr/sbin/ndmc; do [ -e "$p" ] && info "есть $p"; done
NDMC=$(for p in /bin/ndmc /usr/bin/ndmc /sbin/ndmc /usr/sbin/ndmc; do [ -x "$p" ] && echo "$p" && break; done)
if [ -n "$NDMC" ]; then
    "$NDMC" -c 'show version' 2>&1 | grep -E '^ *(release|title|model|device|hw_id|components):' | cut -c1-600 | sed 's/^ */  /'
else
    bad "ndmc не найден"
fi

hdr "2. Бинарник amneziawg-go (mipsel, сборка Detour)"
wget -q -T 30 -O "$BIN" "http://$HOST:18080/amneziawg-go" || { bad "не скачался"; exit 1; }
chmod 755 "$BIN"
info "md5: $(md5sum "$BIN" | cut -d' ' -f1)"
info "версия: $("$BIN" --version 2>&1 | head -1)"

hdr "3. Туннель"
echo "$CONF_B64" | base64 -d > "$CONF" 2>/dev/null || { bad "конфиг не раскодировался"; exit 1; }
mkdir -p /var/run/amneziawg
"$BIN" -f "$IFN" > /tmp/dtst-log 2>&1 &
PID=$!
i=0; while [ ! -S /var/run/amneziawg/"$IFN".sock ] && [ $i -lt 50 ]; do i=$((i+1)); sleep 1; done
if [ -S /var/run/amneziawg/"$IFN".sock ]; then ok "amneziawg-go запущен, TUN $IFN создан"; else
    bad "amneziawg-go не поднялся:"; sed 's/^/    /' /tmp/dtst-log | tail -10; exit 1; fi
"$BIN" awg setconf "$IFN" "$CONF" && ok "конфиг применён" || { bad "awg setconf не прошёл"; exit 1; }
ip addr add "$TUNIP"/32 dev "$IFN" && ip link set "$IFN" mtu 1340 up && ip route add "$SRVIP"/32 dev "$IFN" \
    && ok "адрес и маршрут назначены" || bad "ip addr/route не прошёл"
info "маршрут к серверу: $(ip route get "$HOST" 2>/dev/null | head -1)"
ping -c 3 -W 2 "$SRVIP" >/dev/null 2>&1
hs=$("$BIN" awg show "$IFN" dump 2>/dev/null | awk 'NR == 2 {print $5}')
if [ -n "$hs" ] && [ "$hs" != 0 ]; then ok "рукопожатие есть"; else bad "рукопожатия нет"; fi
ping -c 5 -W 2 "$SRVIP" 2>&1 | tail -2 | sed 's/^/  [info] /'

hdr "4. Скорость: один и тот же файл 32 МБ"
fetch "http://$HOST:18080/big.bin" "напрямую   "
fetch "http://$SRVIP:8000/big.bin" "через AWG  "
fetch "http://$HOST:18080/big.bin" "напрямую   "
fetch "http://$SRVIP:8000/big.bin" "через AWG  "
info "rx/tx туннеля: $("$BIN" awg show "$IFN" dump 2>/dev/null | awk 'NR == 2 {printf "%.1f / %.1f МБ", $6/1048576, $7/1048576}')"
info "память amneziawg-go: $(awk '/VmRSS/ {print $2 " КБ"}' /proc/"$PID"/status 2>/dev/null)"
[ -s /tmp/dtst-log ] && { info "лог amneziawg-go (хвост):"; tail -5 /tmp/dtst-log | sed 's/^/    /'; }
rm -f /tmp/dtst-log

hdr "Готово, всё временное снято — пришлите весь вывод"
