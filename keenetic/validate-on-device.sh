#!/opt/bin/sh
# Detour / Keenetic — on-device Phase-0 validation.
# Run on the router shell (SSH locally → `exec sh`), then paste the output back.
#   wget -qO- https://raw.githubusercontent.com/varyen/detour/main/keenetic/validate-on-device.sh | sh
# or scp it over and: sh validate-on-device.sh
#
# Read-only: it does NOT change anything (the one ipset test is created+destroyed).

export PATH="/opt/bin:/opt/sbin:/usr/bin:/usr/sbin:/bin:/sbin"
ok()   { printf '  [ OK ] %s\n' "$*"; }
bad()  { printf '  [FAIL] %s\n' "$*"; }
info() { printf '  [info] %s\n' "$*"; }
hdr()  { printf '\n=== %s ===\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

hdr "1. Platform / ABI"
info "uname:  $(uname -a 2>/dev/null)"
info "opkg arch: $(opkg print-architecture 2>/dev/null | awk '{print $2"("$3")"}' | tr '\n' ' ')"

hdr "2. LAN bridge (detour.conf assumes br0)"
BR=$(ip -o link 2>/dev/null | sed -n 's/[0-9]*: \([a-z0-9._-]*\):.*/\1/p' | grep -i '^br' | tr '\n' ' ')
[ -n "$BR" ] && ok "bridge(s): $BR" || bad "no br* interface found"
info "v4 addrs: $(ip -o -4 addr show 2>/dev/null | awk '{print $2"="$4}' | tr '\n' ' ')"

hdr "3. Entware + storage"
[ -x /opt/bin/sh ] && ok "/opt/bin/sh present" || bad "/opt/bin/sh missing (Entware?)"
have opkg && ok "opkg: $(opkg --version 2>&1 | head -1)" || bad "opkg missing"
info "/opt space: $(df -h /opt 2>/dev/null | awk 'NR==2{print $4" free of "$2}')"

hdr "4. dnsmasq (domain->ipset DNS needs ipset= support)"
if [ -x /opt/sbin/dnsmasq ]; then
    ok "/opt/sbin/dnsmasq present: $(/opt/sbin/dnsmasq --version 2>/dev/null | head -1)"
    if /opt/sbin/dnsmasq --version 2>/dev/null | grep -qi 'no-ipset'; then
        bad "compiled with NO-ipset — domain routing WON'T tag ipsets (need dnsmasq-full)"
    elif /opt/sbin/dnsmasq --version 2>/dev/null | grep -qi ipset; then
        ok "ipset support compiled in"
    else
        info "could not confirm ipset support from --version (check opkg: dnsmasq-full)"
    fi
else
    bad "/opt/sbin/dnsmasq missing — run: opkg install dnsmasq-full"
fi

hdr "5. Required tools"
for t in iptables ipset start-stop-daemon usign lua curl lighttpd; do
    p=$(command -v "$t" 2>/dev/null)
    [ -n "$p" ] && ok "$t -> $p" || bad "$t MISSING"
done

hdr "6. ipset / xt_set kernel support (transparent redirect needs it)"
if have ipset; then
    if ipset create detour_probe hash:net -exist 2>/dev/null; then
        ipset add detour_probe 1.2.3.4 2>/dev/null && ok "ipset create+add works"
        ipset destroy detour_probe 2>/dev/null
    else
        bad "ipset create failed (kernel ip_set module?)"
    fi
    iptables -t nat -m set --help >/dev/null 2>&1 && ok "iptables -m set (xt_set) available" \
        || info "could not confirm iptables -m set (may still work at rule time)"
fi
info "ip_set modules: $(lsmod 2>/dev/null | grep -i ip_set | awk '{print $1}' | tr '\n' ' ')"

hdr "7. Who owns :53 (KeeneticOS resolver) + does NDM already redirect DNS?"
info "udp :53 listeners:"
netstat -lnup 2>/dev/null | grep ':53 ' | sed 's/^/    /' || info "    (none via netstat)"
DNSRED=$(iptables -t nat -S 2>/dev/null | grep -E 'dpt:53|--dport 53')
[ -n "$DNSRED" ] && { bad "KeeneticOS ALREADY has :53 nat rules (may collide with our redirect):"; printf '%s\n' "$DNSRED" | sed 's/^/    /'; } \
    || ok "no pre-existing :53 nat redirect — our transparent :53->5354 should be clear"

hdr "8. scheduler (keep-alive / sub-refresh / 6h auto-check)"
# KeeneticOS kills the shell crond spawns for a job → we run the schedule from a
# daemon (S90detour-cron → /opt/sbin/detour-cron), not crontab.
if [ -f /opt/var/run/detour-cron.pid ] && kill -0 "$(cat /opt/var/run/detour-cron.pid 2>/dev/null)" 2>/dev/null; then
    ok "detour-cron running (pid $(cat /opt/var/run/detour-cron.pid))"
else
    bad "detour-cron NOT running — scheduled update-check/keep-alive won't fire (start: /opt/etc/init.d/S90detour-cron start)"
fi
[ -x /opt/sbin/detour-cron ] && ok "/opt/sbin/detour-cron present" || bad "/opt/sbin/detour-cron missing"
info "last update-check log: $(tail -1 /opt/var/log/detour-update.log 2>/dev/null || echo '(none yet)')"

hdr "9. Detour install state"
info "version file: $(cat /opt/etc/detour/version 2>/dev/null || echo '(detour not installed)')"
info "opkg: $(opkg list-installed 2>/dev/null | grep -iE '^detour|sing-box|dnsmasq|ipset|lighttpd' | tr '\n' '; ')"
if [ -x /opt/bin/sing-box ]; then
    info "sing-box: $(/opt/bin/sing-box version 2>&1 | head -1)"
else
    info "sing-box: not installed (comes from Entware sing-box-go)"
fi

printf '\n=== DONE — paste everything above back ===\n'
