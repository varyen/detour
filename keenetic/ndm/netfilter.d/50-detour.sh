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
WL_IPSET="${SINGBOX_WL_IPSET:-singbox_whitelist}"
DNS_PORT="${DETOUR_DNS_PORT:-5354}"
ALLVPN_MARK="/opt/etc/detour/allvpn.enabled"   # «Все через VPN» (set by the panel)
DNS_MARK="/opt/etc/detour/dns.enabled"         # detour dnsmasq up (set by S50detour-dns)
ROUTE_MAP="${SINGBOX_ROUTEMAP_LIST:-/opt/etc/sing-box/route-map.list}"

# Extra inbound ifaces (besides LAN_IF) that get the same redirect — VPN
# road-warriors. From settings.json "vpn_redirect_ifaces" (space/comma list);
# empty = none. Lets WireGuard/OpenVPN-server clients route like LAN clients.
vpn_ifaces() {
    sed -n 's/.*"vpn_redirect_ifaces"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
        "$SETTINGS" 2>/dev/null | head -1 | tr ',' ' '
}

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

route_map_targets() {
    [ -f "$ROUTE_MAP" ] || return 0
    awk '
    /^[[:space:]]*\/\/[[:space:]]*===[[:space:]]*route:/ {
        line=$0
        sub(/^[[:space:]]*\/\/[[:space:]]*===[[:space:]]*route:[[:space:]]*/, "", line)
        sub(/[[:space:]]*===.*$/, "", line)
        gsub(/[^a-zA-Z0-9_-]/, "", line)
        if (line != "" && !seen[line]++) print line
    }' "$ROUTE_MAP"
}

route_map_section() {
    id="$1"
    [ -f "$ROUTE_MAP" ] || return 0
    awk -v want="$id" '
    /^[[:space:]]*\/\/[[:space:]]*===[[:space:]]*route:/ {
        t=$0
        sub(/^[[:space:]]*\/\/[[:space:]]*===[[:space:]]*route:[[:space:]]*/, "", t)
        sub(/[[:space:]]*===.*$/, "", t)
        gsub(/[^a-zA-Z0-9_-]/, "", t)
        inblk = (t == want) ? 1 : 0
        next
    }
    {
        if (!inblk) next
        sub(/\/\/.*/, ""); sub(/#.*/, ""); gsub(/\r/, ""); gsub(/^[ \t]+|[ \t]+$/, "")
        sub(/^\*\./, "")
        if ($0 == "") next
        if ($0 ~ /^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+(\/[0-9]+)?$/) { print; next }
        if ($0 ~ /^[a-zA-Z0-9]([a-zA-Z0-9._-]*\.)+[a-zA-Z]{2,}$/) { print; next }
    }' "$ROUTE_MAP"
}

route_map_option() {
    id="$1" key="$2" def="$3"
    [ -f "$ROUTE_MAP" ] || {
        printf '%s' "$def"
        return 0
    }
    awk -v want="$id" -v opt="$key" -v def="$def" '
    BEGIN { inblk=0; found=0 }
    /^[[:space:]]*\/\/[[:space:]]*===[[:space:]]*route:/ {
        t=$0
        sub(/^[[:space:]]*\/\/[[:space:]]*===[[:space:]]*route:[[:space:]]*/, "", t)
        sub(/[[:space:]]*===.*$/, "", t)
        gsub(/[^a-zA-Z0-9_-]/, "", t)
        inblk = (t == want) ? 1 : 0
        next
    }
    inblk && /^[[:space:]]*\/\/[[:space:]]*meta:/ {
        line=$0
        sub(/^[[:space:]]*\/\/[[:space:]]*meta:[[:space:]]*/, "", line)
        n=split(line, parts, /[[:space:]]+/)
        for (i=1; i<=n; i++) {
            split(parts[i], kv, "=")
            if (kv[1] == opt && kv[2] != "") {
                print kv[2]
                found=1
                exit
            }
        }
    }
    END { if (!found) print def }' "$ROUTE_MAP"
}

route_map_bool_option() {
    id="$1" key="$2" def="$3"
    v=$(route_map_option "$id" "$key" "$def")
    case "$v" in
        1|true|yes|on) printf '1' ;;
        0|false|no|off) printf '0' ;;
        *) printf '%s' "$def" ;;
    esac
}

route_map_slots() {
    [ -f "$ROUTE_MAP" ] || return 0
    n=0
    for id in $(route_map_targets); do
        route_map_section "$id" | grep -q . || continue
        [ -f "${SINGBOX_CONFIG_DIR:-/opt/etc/sing-box}/profiles/$id.json" ] || continue
        n=$((n + 1))
        echo "$n $id $((SINGBOX_PORT + n)) singbox_t$n"
    done
}

route_map_strict_slots() {
    route_map_slots | while read -r n id port ipset; do
        [ "$(route_map_bool_option "$id" strict 1)" = "1" ] || continue
        echo "$n $id $port $ipset"
    done
}

# All inbound interfaces that receive transparent-proxy rules: LAN + opt-in VPN.
IFACES="$LAN_IF $(vpn_ifaces)"

# --- transparent DNS: send LAN/VPN :53 to the detour dnsmasq (it tags ipsets) ---
# Without this the singbox_domains/zapret_domains ipsets never fill on Keenetic
# (KeeneticOS owns :53). S50detour-dns drops $DNS_MARK while its dnsmasq is up.
for IF in $IFACES; do
    [ -n "$IF" ] || continue
    if [ -f "$DNS_MARK" ]; then
        add nat PREROUTING -i "$IF" -p udp --dport 53 -j REDIRECT --to-ports "$DNS_PORT"
        add nat PREROUTING -i "$IF" -p tcp --dport 53 -j REDIRECT --to-ports "$DNS_PORT"
    else
        del nat PREROUTING -i "$IF" -p udp --dport 53 -j REDIRECT --to-ports "$DNS_PORT"
        del nat PREROUTING -i "$IF" -p tcp --dport 53 -j REDIRECT --to-ports "$DNS_PORT"
    fi
done

# --- nat PREROUTING: zapret domain-set → REDIRECT (zapret first = higher priority) ---
for IF in $IFACES; do
    [ -n "$IF" ] || continue
    if [ -f /opt/etc/detour/zapret.enabled ]; then
        add nat PREROUTING -i "$IF" -p tcp -m set --match-set "$ZAPRET_IPSET" dst \
            -j REDIRECT --to-ports "$ZAPRET_PORT"
    else
        del nat PREROUTING -i "$IF" -p tcp -m set --match-set "$ZAPRET_IPSET" dst \
            -j REDIRECT --to-ports "$ZAPRET_PORT"
    fi
done

# --- sing-box: tear down BOTH modes' rules on every iface, then apply active ---
for IF in $IFACES; do
    [ -n "$IF" ] || continue
    del nat PREROUTING -i "$IF" -p tcp -m set --match-set "$SINGBOX_IPSET" dst \
        -j REDIRECT --to-ports "$SINGBOX_PORT"
    del nat PREROUTING -i "$IF" -j SINGBOX_ALL
done
if command -v iptables-save >/dev/null 2>&1; then
    iptables-save -t nat 2>/dev/null | grep -- '--match-set singbox_t' | sed 's/^-A/-D/' | while IFS= read -r rule; do
        [ -n "$rule" ] && iptables -t nat $rule 2>/dev/null
    done
fi

if [ -f /opt/etc/detour/singbox.enabled ]; then
    route_map_slots | while read -r n id port ipset; do
        ipset create "$ipset" hash:net -exist 2>/dev/null
        for IF in $IFACES; do
            [ -n "$IF" ] || continue
            add nat PREROUTING -i "$IF" -p tcp -m set --match-set "$ipset" dst \
                -j REDIRECT --to-ports "$port"
        done
    done
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
        for IF in $IFACES; do
            [ -n "$IF" ] || continue
            add nat PREROUTING -i "$IF" -j SINGBOX_ALL
        done
    else
        for IF in $IFACES; do
            [ -n "$IF" ] || continue
            add nat PREROUTING -i "$IF" -p tcp -m set --match-set "$SINGBOX_IPSET" dst \
                -j REDIRECT --to-ports "$SINGBOX_PORT"
        done
    fi
else
    # sing-box disabled: keep only strict route sections fail-closed by redirecting
    # them into closed local ports; non-strict sections are allowed to go direct.
    route_map_strict_slots | while read -r n id port ipset; do
        ipset create "$ipset" hash:net -exist 2>/dev/null
        for IF in $IFACES; do
            [ -n "$IF" ] || continue
            add nat PREROUTING -i "$IF" -p tcp -m set --match-set "$ipset" dst \
                -j REDIRECT --to-ports "$port"
        done
    done
    # sing-box disabled → make sure the all-except chain is gone.
    iptables -t nat -F SINGBOX_ALL 2>/dev/null
    iptables -t nat -X SINGBOX_ALL 2>/dev/null
fi

# --- «Все через VPN» (force ALL TCP through sing-box) — survives NDM rebuilds ---
# The panel drops $ALLVPN_MARK; here we (re)assert the chain so it persists. Built
# at PREROUTING top (before zapret/singbox) so it captures everything.
for IF in $IFACES; do
    [ -n "$IF" ] || continue
    while iptables -t nat -C PREROUTING -i "$IF" -j SINGBOX_ALLVPN 2>/dev/null; do
        iptables -t nat -D PREROUTING -i "$IF" -j SINGBOX_ALLVPN
    done
done
if [ -f "$ALLVPN_MARK" ] && [ -f /opt/etc/detour/singbox.enabled ]; then
    iptables -t nat -N SINGBOX_ALLVPN 2>/dev/null
    iptables -t nat -F SINGBOX_ALLVPN
    iptables -t nat -A SINGBOX_ALLVPN -d 10.0.0.0/8 -j RETURN
    iptables -t nat -A SINGBOX_ALLVPN -d 172.16.0.0/12 -j RETURN
    iptables -t nat -A SINGBOX_ALLVPN -d 192.168.0.0/16 -j RETURN
    iptables -t nat -A SINGBOX_ALLVPN -d 127.0.0.0/8 -j RETURN
    iptables -t nat -A SINGBOX_ALLVPN -d 100.64.0.0/10 -j RETURN
    if [ -n "$UPSTREAM_IPS" ]; then
        OLD_IFS="$IFS"; IFS=','; set -- $UPSTREAM_IPS; IFS="$OLD_IFS"
        for ip in "$@"; do
            [ -n "$ip" ] && iptables -t nat -A SINGBOX_ALLVPN -d "$ip" -j RETURN
        done
    fi
    iptables -t nat -A SINGBOX_ALLVPN -p tcp -j REDIRECT --to-ports "$SINGBOX_PORT"
    for IF in $IFACES; do
        [ -n "$IF" ] || continue
        iptables -t nat -I PREROUTING 1 -i "$IF" -j SINGBOX_ALLVPN
    done
else
    iptables -t nat -F SINGBOX_ALLVPN 2>/dev/null
    iptables -t nat -X SINGBOX_ALLVPN 2>/dev/null
fi

# --- filter INPUT: let the LAN reach the panel (lighttpd :PANEL_PORT) ---
add filter INPUT -i "$LAN_IF" -p tcp --dport "$PANEL_PORT" -j ACCEPT

exit 0
