#!/bin/sh
# Стенд «как Keenetic» для detour-server: заглушка RCI + настоящие iptables/ip-full.
# docker run --rm --privileged -v <repo>:/repo:ro python:3.12-alpine sh /repo/tools/keenetic-stand/run.sh
set -u
apk add -q iptables iproute2 lua5.1 curl >/dev/null 2>&1
ln -sf /usr/bin/lua5.1 /usr/bin/lua
mkdir -p /opt/etc/ndm/netfilter.d /opt/etc/detour /opt/etc/sing-box /opt/var/run /opt/var/log /opt/sbin /opt/bin
ln -sf /sbin/ip /opt/sbin/ip
echo '{}' > /opt/etc/sing-box/settings.json
export LUA_PATH="/repo/keenetic/lua/?.lua;;"
SB=1.14.3
curl -fsSL "https://github.com/SagerNet/sing-box/releases/download/v$SB/sing-box-$SB-linux-amd64-musl.tar.gz" | tar xz -C /tmp
cp /tmp/sing-box-$SB-linux-amd64-musl/sing-box /usr/bin/sing-box
cp /repo/router_files/detour-server /opt/sbin/detour-server; chmod +x /opt/sbin/detour-server
python3 /repo/tools/keenetic-stand/rcimock.py & sleep 1
S=/opt/sbin/detour-server
j() { python3 -c 'import json,sys; d=json.load(sys.stdin); print({k: d.get(k) for k in sys.argv[1:]})' "$@"; }
echo "== status (до настройки)"; $S status | j supported installed backend reason note
echo "== set enabled=1"; $S set enabled=1
echo "== client-add phone / laptop"; $S client-add phone; $S client-add laptop; tail -1 /opt/var/log/detour-server.log
echo "== status"; $S status | j enabled running backend iface routed
echo "== rci log"; grep -c . /tmp/rci.log; grep -E "asc|peer .* allow-ips|listen-port" /tmp/rci.log | tail -4
echo "== iptables"; cat /opt/var/run/detour-server.fw
echo "== settings"; cat /opt/etc/sing-box/settings.json
ID=$($S status | python3 -c 'import json,sys; print(json.load(sys.stdin)["clients"][0]["id"])')
echo "== client-conf"; $S client-conf "$ID" | grep -vE "PrivateKey|PresharedKey"
echo "== tick + stats"; $S tick; $S status | python3 -c 'import json,sys; [print(c["name"], c["online"], c["rx"], c["tx"], c["remote"]) for c in json.load(sys.stdin)["clients"]]'
echo "== client-del phone (ожидаю пересоздание)"; : > /tmp/rci.log; $S client-del "$ID"; grep -E "^no interface|peer" /tmp/rci.log | head -4
echo "== route"; $S client-set "$($S status | python3 -c 'import json,sys; print(json.load(sys.stdin)["clients"][0]["id"])')" route=direct
echo "== vless"; $S set vless=1; $S status | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["vless"])'
cat /opt/var/run/detour-server.fw; ip rule | grep 166; ip route show table 166; ip -4 addr show dev dsrv1p | grep inet
tail -5 /opt/var/log/detour-server.log
echo "== fw после «перестройки NDM»"; iptables -F INPUT; iptables -t nat -F POSTROUTING; $S fw; iptables -S INPUT | head -5
echo "== stop"; $S set enabled=0; $S status | j enabled running; ip link show nwg7 2>&1 | head -1; iptables -S INPUT | grep -c ACCEPT
echo "== без компонента (userspace нет) "; touch /tmp/no-wg; $S status | j supported installed backend reason note can_install
