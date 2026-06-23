#!/opt/bin/sh
# Detour / Keenetic — RUNTIME state snapshot (read-only).
# Run on the router shell (SSH locally → `exec sh`), then paste the output back.
#   wget -qO- https://raw.githubusercontent.com/varyen/detour/main/keenetic/diagnose-runtime.sh | sh
# or scp it over and: sh diagnose-runtime.sh
#
# READ-ONLY: it never changes firewall/markers/config. It only reads state so we
# can confirm which released-but-never-on-device-validated features actually work:
#   * NDM netfilter.d hook fired (nat chains/rules present)
#   * domain-mode plumbing (:53->5354 redirect, detour dnsmasq, ipset membership)
#   * «Все через VPN» chain
#   * auto-switch / health-active loop / scheduler daemon
#   * current installed version + running daemons

export PATH="/opt/bin:/opt/sbin:/usr/bin:/usr/sbin:/bin:/sbin"
ok()   { printf '  [ OK ] %s\n' "$*"; }
bad()  { printf '  [FAIL] %s\n' "$*"; }
info() { printf '  [info] %s\n' "$*"; }
hdr()  { printf '\n=== %s ===\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

DETC=/opt/etc/detour
SETTINGS=/opt/etc/sing-box/settings.json
[ -f "$SETTINGS" ] || SETTINGS=/opt/etc/detour/settings.json

hdr "0. Version / install state"
info "panel version file: $(cat $DETC/version 2>/dev/null || echo '(none)')"
info "opkg detour-keenetic: $(opkg list-installed 2>/dev/null | grep -i '^detour' | tr '\n' '; ')"
if [ -x /opt/bin/sing-box ]; then
    info "sing-box: $(/opt/bin/sing-box version 2>&1 | head -1)"
else
    info "sing-box: /opt/bin/sing-box MISSING"
fi

hdr "1. Daemons running"
chk() { # chk <label> <pgrep-pattern>
    if ps w 2>/dev/null | grep -v grep | grep -q "$2"; then ok "$1 running"; else bad "$1 NOT running"; fi
}
chk "lighttpd (panel)"      "lighttpd"
chk "sing-box"              "[s]ing-box run"
chk "tpws (zapret)"         "[t]pws"
chk "detour dnsmasq (:5354)" "dnsmasq.*5354"
if [ -f /opt/var/run/detour-cron.pid ] && kill -0 "$(cat /opt/var/run/detour-cron.pid 2>/dev/null)" 2>/dev/null; then
    ok "detour-cron running (pid $(cat /opt/var/run/detour-cron.pid))"
else
    bad "detour-cron NOT running (schedule: update-check/keepalive/health/cert-renew won't fire)"
fi
info "listeners: $(netstat -tlnp 2>/dev/null | grep -oE ':(8080|12345|1081|5354|443|8443) ' | sort -u | tr '\n' ' ')"

hdr "2. State markers + routing mode"
for m in singbox.enabled zapret.enabled allvpn.enabled dns.enabled; do
    [ -f "$DETC/$m" ] && ok "marker $m present" || info "marker $m absent"
done
RM=$(sed -n 's/.*"routing_mode"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$SETTINGS" 2>/dev/null | head -1)
info "routing_mode: ${RM:-'(unset → proxy-list default)'}"
info "vpn_redirect_ifaces: $(sed -n 's/.*"vpn_redirect_ifaces"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$SETTINGS" 2>/dev/null | head -1)"

hdr "3. NDM netfilter.d hook (did it fire? rules present?)"
H=/opt/etc/ndm/netfilter.d/50-detour.sh
[ -x "$H" ] && ok "hook installed+exec: $H" || bad "hook missing/not exec: $H"
info "nat PREROUTING jumps to our chains:"
iptables -t nat -S PREROUTING 2>/dev/null | grep -E 'SINGBOX_ALL|SINGBOX_ALLVPN|--to-ports (12345|1081|5354)|dport 53' | sed 's/^/    /' || info "    (none)"
info "SINGBOX_ALL chain:";    iptables -t nat -S SINGBOX_ALL    2>/dev/null | sed 's/^/    /' | head -20 || info "    (absent)"
info "SINGBOX_ALLVPN chain:"; iptables -t nat -S SINGBOX_ALLVPN 2>/dev/null | sed 's/^/    /' | head -20 || info "    (absent)"

hdr "4. Domain-mode plumbing (:53->5354 + ipset membership)"
DNSRED=$(iptables -t nat -S PREROUTING 2>/dev/null | grep -E 'dport 53 .*REDIRECT.*5354')
[ -n "$DNSRED" ] && { ok ":53->5354 redirect active:"; printf '%s\n' "$DNSRED" | sed 's/^/    /'; } \
    || info ":53->5354 redirect NOT present (proxy-list/domain routing needs it; ok if all-except mode)"
for s in singbox_domains zapret_domains singbox_whitelist; do
    if ipset list "$s" >/dev/null 2>&1; then
        n=$(ipset list "$s" 2>/dev/null | grep -cE '^[0-9]')
        info "ipset $s: $n members"
    else
        info "ipset $s: (does not exist)"
    fi
done

hdr "5. Auto-switch / health-active / push (v1.10–1.16 features)"
[ -f $DETC/autoswitch-exclude.list ] && info "autoswitch-exclude: $(grep -cvE '^\s*(#|$)' $DETC/autoswitch-exclude.list 2>/dev/null) profiles excluded" || info "autoswitch-exclude.list: none (all profiles eligible)"
info "health db tail:"; tail -3 /tmp/detour-health.db 2>/dev/null | sed 's/^/    /' || info "    (no health db yet)"
info "push subscriptions: $(grep -c 'endpoint' $DETC/push-subs.json 2>/dev/null || echo 0)"
info "detour-cron wiring:"; grep -nE 'detour-health|detour-cert|subscription-refresh|check-all|keepalive' /opt/sbin/detour-cron 2>/dev/null | sed 's/^/    /' | head -8

hdr "6. Recent logs"
info "update log:"; tail -3 /opt/var/log/detour-update.log 2>/dev/null | sed 's/^/    /' || info "    (none)"
info "sing-box log:"; tail -3 /opt/var/log/sing-box.log 2>/dev/null | sed 's/^/    /' || info "    (none)"
info "cert status: $(cat /opt/var/state/detour-cert.json 2>/dev/null | head -c 300 || echo '(no cert issued)')"

printf '\n=== DONE — paste everything above back ===\n'
