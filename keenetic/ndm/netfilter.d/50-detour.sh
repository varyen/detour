#!/opt/bin/sh
# Detour / Keenetic — transparent-proxy firewall hook.
# KeeneticOS (NDM) rebuilds iptables on every reconfig and runs the scripts in
# /opt/etc/ndm/netfilter.d/. This re-asserts our rules each time so they survive.
# Also invoked directly by the init.d start/stop to apply immediately.
#
# Two sing-box routing modes (read live from settings.json):
#   proxy-list : REDIRECT only destinations in the singbox_domains ipset.
#   all-except : REDIRECT ALL LAN TCP to :12345 EXCEPT private nets, the upstream
#                server IP(s) (loop guard) and the singbox_whitelist ipset.
#
# ⚠ VALIDATE on device:
#   * the NDM hook contract — type comes as $1 or env $type, table as $2 or env $table.
#   * LAN bridge name (detour.conf LAN_IF, assumed br0).
#   * that `-m set --match-set` (xt_set/ipset) is available in KeeneticOS iptables.

# NDM runs netfilter.d hooks with a minimal PATH — but iptables/ipset/sed live
# under /opt on Entware. Without this they're not found and NO rules get applied.
export PATH="/opt/bin:/opt/sbin:/usr/bin:/usr/sbin:/bin:/sbin"

. /opt/etc/detour/detour.conf 2>/dev/null
: "${LAN_IF:=br0}" "${SINGBOX_PORT:=12345}" "${ZAPRET_PORT:=1081}" "${PANEL_PORT:=8080}"
: "${SINGBOX_IPSET:=singbox_domains}" "${ZAPRET_IPSET:=zapret_domains}"
SETTINGS="${SINGBOX_SETTINGS:-/opt/etc/sing-box/settings.json}"
WL_IPSET="singbox_whitelist"

TYPE="${1:-$type}"     # iptables | ip6tables
# Only touch IPv4. IPv6 transparent-proxy is out of scope for the port.
[ "$TYPE" = "ip6tables" ] && exit 0

# Routing mode + upstream server IPs (loop guard for all-except) from settings.json.
ROUTING_MODE=$(sed -n 's/.*"routing_mode"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$SETTINGS" 2>/dev/null | head -1)
[ -z "$ROUTING_MODE" ] && ROUTING_MODE="proxy-list"
UPSTREAM_IPS=$(sed -n 's/.*"upstream_ips"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$SETTINGS" 2>/dev/null | head -1)

# Make sure the ipsets exist before any --match-set rule references them.
ipset create "$SINGBOX_IPSET" hash:net -exist 2>/dev/null
ipset create "$ZAPRET_IPSET"  hash:net -exist 2>/dev/null
ipset create "$WL_IPSET"      hash:net -exist 2>/dev/null

# add <table> <chain> <rule...> — insert once (idempotent via -C).
add() {
    t="$1"; c="$2"; shift 2
    iptables -t "$t" -C "$c" "$@" 2>/dev/null || iptables -t "$t" -A "$c" "$@"
}
del() {
    t="$1"; c="$2"; shift 2
    while iptables -t "$t" -C "$c" "$@" 2>/dev/null; do iptables -t "$t" -D "$c" "$@"; done
}

# --- nat PREROUTING: zapret domain-set → REDIRECT (zapret first = higher priority) ---
if [ -f /opt/etc/detour/zapret.enabled ]; then
    add nat PREROUTING -i "$LAN_IF" -p tcp -m set --match-set "$ZAPRET_IPSET" dst \
        -j REDIRECT --to-ports "$ZAPRET_PORT"
else
    del nat PREROUTING -i "$LAN_IF" -p tcp -m set --match-set "$ZAPRET_IPSET" dst \
        -j REDIRECT --to-ports "$ZAPRET_PORT"
fi

# --- sing-box: tear down BOTH modes' rules, then apply the active one ---
del nat PREROUTING -i "$LAN_IF" -p tcp -m set --match-set "$SINGBOX_IPSET" dst \
    -j REDIRECT --to-ports "$SINGBOX_PORT"
del nat PREROUTING -i "$LAN_IF" -j SINGBOX_ALL

if [ -f /opt/etc/detour/singbox.enabled ]; then
    if [ "$ROUTING_MODE" = "all-except" ]; then
        # Proxy EVERYTHING except private/loopback/CGNAT, the upstream server(s),
        # and the whitelist ipset. sing-box itself also sends whitelisted domains
        # direct (SNI sniff), so the ipset is only an optimisation / IP-whitelist.
        iptables -t nat -N SINGBOX_ALL 2>/dev/null
        iptables -t nat -F SINGBOX_ALL
        iptables -t nat -A SINGBOX_ALL -d 10.0.0.0/8 -j RETURN
        iptables -t nat -A SINGBOX_ALL -d 172.16.0.0/12 -j RETURN
        iptables -t nat -A SINGBOX_ALL -d 192.168.0.0/16 -j RETURN
        iptables -t nat -A SINGBOX_ALL -d 127.0.0.0/8 -j RETURN
        iptables -t nat -A SINGBOX_ALL -d 100.64.0.0/10 -j RETURN
        if [ -n "$UPSTREAM_IPS" ]; then
            OLD_IFS="$IFS"; IFS=','; set -- $UPSTREAM_IPS; IFS="$OLD_IFS"
            for ip in "$@"; do
                [ -n "$ip" ] && iptables -t nat -A SINGBOX_ALL -d "$ip" -j RETURN
            done
        fi
        # Whitelist ipset bypass (tolerated if xt_set is unavailable — sing-box still
        # routes whitelist domains direct internally).
        iptables -t nat -A SINGBOX_ALL -p tcp -m set --match-set "$WL_IPSET" dst -j RETURN 2>/dev/null
        iptables -t nat -A SINGBOX_ALL -p tcp -j REDIRECT --to-ports "$SINGBOX_PORT"
        add nat PREROUTING -i "$LAN_IF" -j SINGBOX_ALL
    else
        add nat PREROUTING -i "$LAN_IF" -p tcp -m set --match-set "$SINGBOX_IPSET" dst \
            -j REDIRECT --to-ports "$SINGBOX_PORT"
    fi
else
    # sing-box disabled → make sure the all-except chain is gone.
    iptables -t nat -F SINGBOX_ALL 2>/dev/null
    iptables -t nat -X SINGBOX_ALL 2>/dev/null
fi

# --- filter INPUT: let the LAN reach the panel (lighttpd :PANEL_PORT) ---
add filter INPUT -i "$LAN_IF" -p tcp --dport "$PANEL_PORT" -j ACCEPT

exit 0
