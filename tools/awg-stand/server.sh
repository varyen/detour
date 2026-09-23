#!/bin/sh
# UAPI-конфиг приходит готовым в /cfg/uapi (формат wireguard-go: key=value, hex-ключи).
set -e
mkdir -p /var/run/amneziawg
AWG_LOG_LEVEL=verbose amneziawg-go -f awg0 &
i=0
while [ ! -S /var/run/amneziawg/awg0.sock ]; do
    i=$((i + 1)); [ $i -gt 50 ] && { echo "no uapi socket"; exit 1; }
    sleep 0.1
done
{ echo "set=1"; cat /cfg/uapi; echo; } | socat - UNIX-CONNECT:/var/run/amneziawg/awg0.sock
ip addr add 10.66.0.1/24 dev awg0
ip link set awg0 up
mkdir -p /www
echo "awg-ok $AWG_TAG" > /www/index.html
# выход в интернет через туннель — для проверок, которые ходят на внешние адреса
iptables -t nat -A POSTROUTING -s 10.66.0.0/24 ! -o awg0 -j MASQUERADE 2>/dev/null || true
httpd -f -p 10.66.0.1:8000 -h /www &
wait
