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
# wget-ssl: Entware's default `wget-nossl` can't do HTTPS at all, so any opkg fetch
# of our HTTPS feed fails. Install the SSL build (from Entware's own HTTP repo, which
# wget-nossl CAN reach) so a native `opkg update`/`opkg upgrade` of our feed works.
PKGS="iptables ipset dnsmasq-full lighttpd lighttpd-mod-cgi lighttpd-mod-setenv \
      lua lua-cjson coreutils-base64 openssl-util curl wget-ssl start-stop-daemon"

echo "[bootstrap] opkg install: $PKGS"
opkg install $PKGS

# Force wget/opkg onto IPv4: requests to the RU-throttled raw.githubusercontent.com
# routinely HANG on IPv6. /opt/etc/wgetrc is read by Entware's wget.
if ! grep -qs '^inet4_only' /opt/etc/wgetrc 2>/dev/null; then
    echo "[bootstrap] pinning wget to IPv4 (/opt/etc/wgetrc)"
    printf 'inet4_only = on\ntimeout = 30\ntries = 3\n' >> /opt/etc/wgetrc
fi

# Detour mipsel opkg feed — serves sing-box (latest 1.13.x, the -mipsle-softfloat-musl
# static build) and tpws-zapret, which are the panel's `Depends: sing-box, tpws-zapret`.
# Add it BEFORE installing the panel .ipk, or opkg can't resolve those deps.
DETOUR_BASE="https://raw.githubusercontent.com/varyen/detour/feed/mipsel"
if ! grep -qs '^src/gz detour ' /opt/etc/opkg/customfeeds.conf 2>/dev/null; then
    echo "[bootstrap] adding detour mipsel feed line"
    echo "src/gz detour $DETOUR_BASE" >> /opt/etc/opkg/customfeeds.conf
fi

# Install sing-box + tpws-zapret. Prefer a direct curl -4 download + local opkg
# install: even wget-ssl tends to stall on IPv6 to the throttled GitHub raw host, and
# curl -4 is what reliably works on RU links (opkg's own downloader is the weak link,
# not the feed). Falls back to plain `opkg install` if the curl path can't fetch.
echo "[bootstrap] installing sing-box + tpws-zapret (curl -4 from the feed)"
opkg update 2>/dev/null || true
for pkg in sing-box tpws-zapret; do
    f=$(curl -4 -fsSL --connect-timeout 15 "$DETOUR_BASE/Packages" 2>/dev/null \
        | awk -v p="$pkg" '$1=="Package:"{c=$2} $1=="Filename:" && c==p {print $2; exit}')
    if [ -n "$f" ] && curl -4 -fsSL --connect-timeout 15 -o "/tmp/$f" "$DETOUR_BASE/$f" 2>/dev/null; then
        opkg install --force-overwrite "/tmp/$f" && rm -f "/tmp/$f" \
            || echo "[bootstrap] WARN: opkg install $f failed"
    else
        echo "[bootstrap] curl fetch of $pkg failed — trying plain opkg install"
        opkg install --force-overwrite "$pkg" || echo "[bootstrap] WARN: opkg install $pkg failed"
    fi
done
# Retire Entware's sing-box-go once our feed sing-box is in (it supersedes it).
if opkg list-installed sing-box 2>/dev/null | grep -q '^sing-box ' \
   && opkg list-installed sing-box-go 2>/dev/null | grep -q '^sing-box-go '; then
    echo "[bootstrap] removing Entware sing-box-go (superseded by feed sing-box)"
    opkg remove sing-box-go 2>/dev/null || true
fi

# Directory skeleton on the Entware volume.
mkdir -p /opt/sbin /opt/etc/sing-box/profiles /opt/etc/zapret-tpws \
         /opt/etc/detour /opt/var/log /opt/var/run \
         /opt/share/www/detour /opt/share/www/cgi-bin \
         /opt/etc/ndm/netfilter.d /opt/etc/lighttpd/conf.d

echo "[bootstrap] done. Next: install the panel (it pulls sing-box + tpws from the feed):"
echo "             opkg install ./detour-keenetic_<ver>_all.ipk"
