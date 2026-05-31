#!/opt/bin/sh
# Detour / Keenetic (KeeneticOS + Entware) — one-time bootstrap.
# Installs every Entware dependency the detour stack needs on MT7621 (mipselsf).
# Run ON the router after Entware is installed:  sh /opt/etc/detour/entware-bootstrap.sh
#
# ⚠ VALIDATE on device: package names exist in the mipselsf-k3.4 feed (they do as of
#   2026-05 per bin.entware.net/mipselsf-k3.4/Packages.html) and opkg has network.
set -e

echo "[bootstrap] opkg update"
opkg update

# Runtime deps, mirrors the OpenWrt package's Depends line, adapted to Entware:
#   iptables/ipset    — transparent-proxy nat REDIRECT + domain ipsets
#   dnsmasq-full      — ipset= domain population (the OpenWrt approach)
#   lighttpd + mods   — host the panel CGI + HTML on :8080 (no uhttpd on Keenetic)
#   lua + lua-cjson   — the panel CGI's embedded Lua helpers
#   coreutils-base64  — file transfer / hashing (busybox base64 applet may be absent)
#   openssl-util      — password hashing (openssl passwd -6) + usign-less checks
#   curl              — self-update download
#   start-stop-daemon — service supervision in our init.d scripts
PKGS="iptables ipset dnsmasq-full lighttpd lighttpd-mod-cgi lighttpd-mod-setenv \
      lua lua-cjson coreutils-base64 openssl-util curl start-stop-daemon"

echo "[bootstrap] opkg install: $PKGS"
opkg install $PKGS

# Directory skeleton on the Entware volume.
mkdir -p /opt/sbin /opt/etc/sing-box/profiles /opt/etc/zapret-tpws \
         /opt/etc/detour /opt/var/log /opt/var/run \
         /opt/share/www/detour /opt/share/www/cgi-bin \
         /opt/etc/ndm/netfilter.d /opt/etc/lighttpd/conf.d

echo "[bootstrap] done. Next: deploy bins to /opt/sbin, configs to /opt/etc, panel to /opt/share/www."
