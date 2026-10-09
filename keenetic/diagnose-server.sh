#!/opt/bin/sh
# Detour / Keenetic — что есть на роутере для своего VPN-сервера (AmneziaWG + VLESS-Reality).
# Запуск на роутере (SSH → `exec sh`, если попали в CLI KeeneticOS):
#   wget -qO- https://raw.githubusercontent.com/varyen/detour/main/keenetic/diagnose-server.sh | sh
# Вывод целиком прислать обратно.
#
# Почти только чтение. Единственное, что скрипт создаёт, — пробные TUN-интерфейс,
# сетевое пространство и пара veth с именами dtst*; каждое удаляется сразу
# после проверки. Конфиг KeeneticOS и файрвол не меняются.

export PATH="/opt/bin:/opt/sbin:/usr/bin:/usr/sbin:/bin:/sbin"
ok()   { printf '  [ OK ] %s\n' "$*"; }
bad()  { printf '  [FAIL] %s\n' "$*"; }
info() { printf '  [info] %s\n' "$*"; }
hdr()  { printf '\n=== %s ===\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }
ndm()  { have ndmc && ndmc -c "$1" 2>/dev/null; }
# У busybox ip нет tuntap и netns — берём ip-full из Entware, если он есть.
IP=ip; [ -x /opt/sbin/ip ] && IP=/opt/sbin/ip

hdr "1. Модель и прошивка"
ndm 'show version' | grep -E '^ *(release|title|model|device|hw_version|arch|ndm|sandbox):' | sed 's/^ */  /'
info "ядро: $(uname -r) $(uname -m)"
grep -m1 -E 'system type|cpu model' /proc/cpuinfo | sed 's/^/  [info] /'
info "ядер CPU: $(grep -c '^processor' /proc/cpuinfo)"
awk '/MemTotal|MemAvailable/ {printf "  [info] %s %d МБ\n", $1, $2/1024}' /proc/meminfo
df -h /opt 2>/dev/null | tail -1 | awk '{print "  [info] /opt: всего " $2 ", свободно " $4}'

hdr "2. Компоненты KeeneticOS (WireGuard, OpenVPN, Netfilter)"
ndm 'show version' | grep -E '^ *components:' | tr ',' '\n' | \
    grep -iE 'wireguard|openvpn|netfilter|tun|kernel|ipsec' | sed 's/^ */  [info] /'

hdr "3. Встроенный WireGuard"
for n in $(ls /sys/class/net 2>/dev/null); do
    case "$n" in nwg*|wg*|Wireguard*) info "интерфейс WireGuard: $n";; esac
done
ndm 'show interface' | grep -iE '^ *(id|type|description|link|state): ' | \
    awk '/id: Wireguard/ {p=1} /id: / && !/Wireguard/ {p=0} p' | sed 's/^ */  /'
lsmod 2>/dev/null | grep -E '^(wireguard|amneziawg) ' | sed 's/^/  [info] lsmod: /'

hdr "4. TUN (для userspace amneziawg-go)"
[ -c /dev/net/tun ] && ok "/dev/net/tun есть" || bad "/dev/net/tun нет"
lsmod 2>/dev/null | grep -q '^tun ' && ok "модуль tun загружен" || info "модуль tun не загружен (может быть встроен в ядро)"
find /lib/modules/"$(uname -r)" -name 'tun.ko*' 2>/dev/null | head -1 | sed 's/^/  [info] /'
if have "$IP"; then
    info "ip: $(readlink -f "$(command -v "$IP")") ($($IP -V 2>&1 | head -1))"
    if $IP tuntap add dev dtst0 mode tun 2>/dev/null; then
        ok "TUN создаётся (ip tuntap)"; $IP link del dtst0 2>/dev/null
    else
        bad "ip tuntap add не сработал: $($IP tuntap add dev dtst0 mode tun 2>&1 | tail -1)"
    fi
else
    bad "команды ip нет"
fi

hdr "5. Сетевые пространства и veth (для VLESS-Reality)"
[ -e /proc/self/ns/net ] && ok "ядро с NET_NS" || bad "/proc/self/ns/net нет — ядро без сетевых пространств"
find /lib/modules/"$(uname -r)" -name 'veth.ko*' 2>/dev/null | head -1 | sed 's/^/  [info] /'
if have "$IP" && $IP netns add dtst 2>/dev/null; then
    ok "ip netns add работает"
    if $IP link add dtsta type veth peer name dtstb 2>/dev/null; then
        ok "veth создаётся"
        $IP link set dtstb netns dtst 2>/dev/null && ok "veth переносится в netns" || bad "veth не переносится в netns"
        $IP link del dtsta 2>/dev/null
    else
        bad "veth не создаётся: $($IP link add dtsta type veth peer name dtstb 2>&1 | tail -1)"
    fi
    $IP netns del dtst 2>/dev/null
else
    bad "ip netns add не работает: $($IP netns add dtst 2>&1 | tail -1)"
fi
have unshare && info "unshare: $(command -v unshare)" || info "unshare нет"

hdr "6. WAN: белый ли адрес"
ip -4 route show default 2>/dev/null | head -2 | sed 's/^/  [info] default: /'
WANDEV=$(ip -4 route show default 2>/dev/null | awk '{for(i=1;i<NF;i++) if($i=="dev") print $(i+1)}' | head -1)
WANIP=$(ip -4 -o addr show dev "$WANDEV" 2>/dev/null | awk '{print $4}' | cut -d/ -f1 | head -1)
info "WAN-интерфейс: ${WANDEV:-?}"
case "$WANIP" in
    10.*|192.168.*|172.1[6-9].*|172.2[0-9].*|172.3[01].*|100.6[4-9].*|100.[7-9][0-9].*|100.1[01][0-9].*|100.12[0-7].*)
        bad "адрес на WAN серый ($WANIP) — снаружи сервер не достать без проброса у провайдера";;
    "") info "адрес WAN не определился";;
    *) ok "адрес на WAN — белый (последний октет скрыт: ${WANIP%.*}.x)";;
esac

hdr "7. Файрвол: можно ли открыть порт с WAN"
iptables -S INPUT 2>/dev/null | head -8 | sed 's/^/  [info] /'
[ -d /opt/etc/ndm/netfilter.d ] && ok "netfilter.d-хуки есть" || bad "/opt/etc/ndm/netfilter.d нет"
ls /opt/etc/ndm/netfilter.d 2>/dev/null | sed 's/^/  [info] хук: /'

hdr "8. Detour и движки"
info "панель: $(cat /opt/etc/detour/version 2>/dev/null || echo нет)"
opkg list-installed 2>/dev/null | grep -E '^(detour|sing-box|tpws-zapret|mihomo|ip-full|ip |kmod-)' | sed 's/^/  [info] /'
for b in /opt/bin/sing-box /opt/bin/sing-box-vless; do
    [ -x "$b" ] && info "$b: $($b version 2>&1 | head -1)"
done
SET=/opt/etc/sing-box/settings.json
[ -f "$SET" ] && info "vpn_redirect_ifaces: $(sed -n 's/.*"vpn_redirect_ifaces"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$SET" | head -1)"
opkg list 2>/dev/null | grep -E '^(ip-full|kmod-tun|kmod-veth|wireguard-tools|amneziawg) ' | sed 's/^/  [info] в фиде Entware: /'

hdr "Готово — пришлите весь вывод"
