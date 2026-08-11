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
# Per-route-target inbound ports (target N → BASE+N). MUST match detour-api's
# ROUTE_PORT_BASE: the CGI writes the listen port into the config, we point the
# target's ipset at it. Literal (not SINGBOX_PORT+N) for exactly that reason.
ROUTE_PORT_BASE=12400
BLOCKED_EGRESS_LIST="${DETOUR_BLOCKED_EGRESS_LIST:-/opt/etc/detour/blocked-egress-ips.list}"

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

# A route target is either a PROFILE or a saved CHAIN (chains.json) — the CGI
# gives both a slot in route-map file order. Skipping chains here would leave
# their sites without an ipset/REDIRECT (→ SNI-sniff only → leak into the active
# VPN) and shift the numbering of every later profile target, pointing its ipset
# at another target's inbound. Parity with sing-box.initd.
chain_ids() {
    _cf="${SINGBOX_CHAINS:-${SINGBOX_CONFIG_DIR:-/opt/etc/sing-box}/chains.json}"
    [ -f "$_cf" ] || return 0
    tr ',' '\n' < "$_cf" 2>/dev/null | \
        sed -n 's/.*"id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p'
}

route_target_exists() {
    [ -f "${SINGBOX_CONFIG_DIR:-/opt/etc/sing-box}/profiles/$1.json" ] && return 0
    chain_ids | grep -qxF "$1"
}

route_map_slots() {
    [ -f "$ROUTE_MAP" ] || return 0
    n=0
    for id in $(route_map_targets); do
        route_map_section "$id" | grep -q . || continue
        route_target_exists "$id" || continue
        n=$((n + 1))
        echo "$n $id $((ROUTE_PORT_BASE + n)) singbox_t$n"
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

# --- egress deny-list: block selected destination IPs globally (router OUTPUT +
# client FORWARD). Source list: /opt/etc/detour/blocked-egress-ips.list, one IPv4
# per line, comments (# or //) allowed.
# IMPORTANT: FORWARD has one exception — destinations from the Detour "white list"
# (singbox_whitelist ipset / "Исключения") are allowed direct and bypass this block.
iptables -t filter -N DETOUR_EGRESS_BLOCK_OUT 2>/dev/null
iptables -t filter -F DETOUR_EGRESS_BLOCK_OUT 2>/dev/null
iptables -t filter -N DETOUR_EGRESS_BLOCK_FWD 2>/dev/null
iptables -t filter -F DETOUR_EGRESS_BLOCK_FWD 2>/dev/null
iptables -t filter -A DETOUR_EGRESS_BLOCK_FWD -m set --match-set "$WL_IPSET" dst -j RETURN 2>/dev/null
if [ -f "$BLOCKED_EGRESS_LIST" ]; then
    awk '
    {
        line=$0
        sub(/#.*/, "", line)
        sub(/\/\/.*/, "", line)
        gsub(/\r/, "", line)
        gsub(/^[ \t]+|[ \t]+$/, "", line)
        if (line ~ /^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$/) print line
    }' "$BLOCKED_EGRESS_LIST" | awk '!seen[$0]++' | while IFS= read -r ip; do
        [ -n "$ip" ] || continue
        add filter DETOUR_EGRESS_BLOCK_OUT -d "$ip" -j REJECT --reject-with icmp-port-unreachable
        add filter DETOUR_EGRESS_BLOCK_FWD -d "$ip" -j REJECT --reject-with icmp-port-unreachable
    done
fi
add filter OUTPUT -j DETOUR_EGRESS_BLOCK_OUT
add filter FORWARD -j DETOUR_EGRESS_BLOCK_FWD

# --- «Проброс сервисов» (detour-portmap) -------------------------------------
# Source: /opt/etc/detour/portmap.conf, one mapping per line, '|'-separated:
#   id|enabled|mode|listen_port|proto|target_ip|target_port|scheme|src|user|sha|name
#
#   mode=https → the reverse proxy runs on THIS router (lighttpd mod_proxy socket
#                written by detour-portmap), so all we owe it is an INPUT accept.
#   mode=dnat  → a real WAN→LAN port forward: nat PREROUTING DNAT + a FORWARD accept
#                (KeeneticOS masquerades the return path itself).
#
# Own chains, flushed on every run, so removing a mapping actually removes its rule
# (the generic `add` helper is idempotent but never deletes).
# ⚠ NOT verified on a live Keenetic yet — see keenetic/README.md.
PORTMAP_CONF="${DETOUR_PORTMAP_CONF:-/opt/etc/detour/portmap.conf}"
LAN_CIDR=$(ip -4 -o addr show "$LAN_IF" 2>/dev/null | awk '{print $4}' | head -1 | \
    awk -F/ '{split($1,a,"."); print a[1]"."a[2]"."a[3]".0/"$2}')

iptables -t filter -N DETOUR_PORTMAP_IN 2>/dev/null
iptables -t filter -F DETOUR_PORTMAP_IN 2>/dev/null
iptables -t filter -N DETOUR_PORTMAP_FWD 2>/dev/null
iptables -t filter -F DETOUR_PORTMAP_FWD 2>/dev/null
iptables -t nat -N DETOUR_PORTMAP_DNAT 2>/dev/null
iptables -t nat -F DETOUR_PORTMAP_DNAT 2>/dev/null

if [ -f "$PORTMAP_CONF" ]; then
    while IFS= read -r pmline; do
        [ -n "$pmline" ] || continue
        case "$pmline" in \#*) continue ;; esac
        pm_en=$(printf '%s' "$pmline"   | cut -d'|' -f2)
        [ "$pm_en" = 1 ] || continue
        pm_mode=$(printf '%s' "$pmline" | cut -d'|' -f3)
        pm_lp=$(printf '%s' "$pmline"   | cut -d'|' -f4)
        pm_pr=$(printf '%s' "$pmline"   | cut -d'|' -f5)
        pm_tip=$(printf '%s' "$pmline"  | cut -d'|' -f6)
        pm_tp=$(printf '%s' "$pmline"   | cut -d'|' -f7)
        pm_src=$(printf '%s' "$pmline"  | cut -d'|' -f9)
        case "$pm_lp" in ''|*[!0-9]*) continue ;; esac
        case "$pm_tp" in ''|*[!0-9]*) continue ;; esac
        # «TCP + UDP» хранится в конфиге одним полем "tcp udp" — раскладываем его
        # по словам ниже (iptables принимает только один -p за правило).
        case "$pm_pr" in tcp|udp|'tcp udp') ;; *) pm_pr=tcp ;; esac
        case "$pm_mode" in
            https)
                if [ "$pm_src" = lan ] && [ -n "$LAN_CIDR" ]; then
                    add filter DETOUR_PORTMAP_IN -s "$LAN_CIDR" -p tcp --dport "$pm_lp" -j ACCEPT
                else
                    add filter DETOUR_PORTMAP_IN -p tcp --dport "$pm_lp" -j ACCEPT
                fi
                ;;
            dnat)
                # src=lan on a WAN forward is a no-op — skip it rather than publish.
                [ "$pm_src" = lan ] && continue
                for pmproto in $pm_pr; do
                    add nat DETOUR_PORTMAP_DNAT ! -s "${LAN_CIDR:-0.0.0.0/0}" -p "$pmproto" \
                        --dport "$pm_lp" -j DNAT --to-destination "$pm_tip:$pm_tp"
                    add filter DETOUR_PORTMAP_FWD -p "$pmproto" -d "$pm_tip" --dport "$pm_tp" -j ACCEPT
                done
                ;;
        esac
    done < "$PORTMAP_CONF"
fi
add filter INPUT -j DETOUR_PORTMAP_IN
add filter FORWARD -j DETOUR_PORTMAP_FWD
add nat PREROUTING -j DETOUR_PORTMAP_DNAT

# Счётчики трафика для схемы потока в панели. NDM пересобирает iptables на
# каждый реконфиг и сносит наши цепочки — возвращаем их, но только если панель
# ими пользуется (есть файл состояния). Правила ничего не решают, лишь считают,
# поэтому их потеря не ломает маршрутизацию, а лишь обнуляет график.
if [ -x /opt/sbin/detour-meter ] && [ -f /tmp/detour-meter.state ]; then
    /opt/sbin/detour-meter install >/dev/null 2>&1
fi

# Reply-routing по аплинкам. В отличие от счётчиков эти цепочки решают, каким
# каналом уйдёт ответ, поэтому их потеря — не косметика: проброс портов и
# HTTPS-панель начнут отвечать не с того адреса. Скрипт сам решает, нужен ли он
# (при одном канале ничего не ставит), так что вызываем безусловно.
if [ -x /opt/sbin/detour-wanpin ]; then
    /opt/sbin/detour-wanpin apply >/dev/null 2>&1
fi

exit 0
