#!/opt/bin/sh
# Detour / Keenetic — targeted validation for recent feature parity (v1.24.0+).
# Run on the router shell (SSH locally -> `exec sh`), then paste the output back.
#   wget -qO- https://raw.githubusercontent.com/varyen/detour/main/keenetic/validate-recent-features.sh | sh
# or scp it over and: sh validate-recent-features.sh
#
# Read-only: does not change firewall, settings, profiles or services.

export PATH="/opt/bin:/opt/sbin:/usr/bin:/usr/sbin:/bin:/sbin"

ok()   { printf '  [ OK ] %s\n' "$*"; }
bad()  { printf '  [FAIL] %s\n' "$*"; }
warn() { printf '  [WARN] %s\n' "$*"; }
info() { printf '  [info] %s\n' "$*"; }
hdr()  { printf '\n=== %s ===\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }

DETOUR_DIR=/opt/etc/detour
SB_DIR=/opt/etc/sing-box
SETTINGS="$SB_DIR/settings.json"
CONFIG="$SB_DIR/config.json"
INTERCEPT_MAP="$SB_DIR/intercept.map"
WANLINK_BIN=/opt/sbin/detour-wan-link
CRON_BIN=/opt/sbin/detour-cron
PUSH_SUBS="$DETOUR_DIR/push-subs.json"

if [ ! -f "$SETTINGS" ] && [ -f "$DETOUR_DIR/settings.json" ]; then
    SETTINGS="$DETOUR_DIR/settings.json"
fi

json_setting_csv() {
    _key="$1"
    sed -n "s/.*\"$_key\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" "$SETTINGS" 2>/dev/null | head -1
}

profile_meta() {
    lua - "$1" 2>/dev/null <<'LUA'
package.path = "/opt/share/lua/5.1/?.lua;/opt/share/lua/5.1/?/init.lua;" .. package.path
local path = arg[1] or ""
local ok, cjson = pcall(require, "cjson.safe")
if not ok or not cjson then os.exit(1) end
local f = io.open(path, "rb")
if not f then os.exit(1) end
local obj = cjson.decode(f:read("*a"))
f:close()
if type(obj) ~= "table" then os.exit(1) end

local outbound = obj.outbound or {}
local inferred = "unknown"
if outbound.type == "http" then
    inferred = (type(outbound.tls) == "table" and outbound.tls.enabled) and "https-proxy" or "http-proxy"
elseif outbound.type == "socks" then
    local version = tostring(outbound.version or "5")
    if version == "4" then
        inferred = "socks4"
    elseif version == "4a" then
        inferred = "socks4a"
    else
        inferred = "socks5"
    end
elseif type(outbound.type) == "string" and outbound.type ~= "" then
    inferred = outbound.type
end

local stored = type(obj.type) == "string" and obj.type or ""
local id = obj.id or ""
local name = obj.name or ""
local server = outbound.server or ""
local port = tostring(outbound.server_port or "")
io.write(id, "\t", name, "\t", stored, "\t", inferred, "\t", server, "\t", port)
LUA
}

wan_status_tsv() {
    [ -x "$WANLINK_BIN" ] || return 1
    lua - "$WANLINK_BIN" 2>/dev/null <<'LUA'
package.path = "/opt/share/lua/5.1/?.lua;/opt/share/lua/5.1/?/init.lua;" .. package.path
local ok, cjson = pcall(require, "cjson.safe")
if not ok or not cjson then os.exit(1) end
local bin = arg[1]
if type(bin) ~= "string" or bin == "" then os.exit(1) end
local f = io.popen(bin .. " status 2>/dev/null")
if not f then os.exit(1) end
local raw = f:read("*a")
f:close()
local obj = cjson.decode(raw)
if type(obj) ~= "table" then os.exit(1) end
local fields = {
  tostring(obj.supported),
  tostring(obj.degraded),
  tostring(obj.wan_if or ""),
  tostring(obj.phy_if or ""),
  tostring(obj.speed_mbps or 0),
  tostring(obj.partner_max_mbps or 0),
  tostring(obj.duplex or ""),
  tostring(obj.diagnosis or ""),
  tostring(obj.advice or "")
}
io.write(table.concat(fields, "\t"))
LUA
}

hdr "0. Version / install state"
info "panel version file: $(cat $DETOUR_DIR/version 2>/dev/null || echo '(none)')"
info "detour package: $(opkg list-installed 2>/dev/null | grep -i '^detour' | tr '\n' '; ')"
info "sing-box package: $(opkg list-installed 2>/dev/null | grep -E '^sing-box(-go)? ' | tr '\n' '; ')"
info "tpws package: $(opkg list-installed tpws-zapret 2>/dev/null | tr '\n' '; ')"

hdr "1. Proxy profile type normalization (v1.25.3)"
if [ ! -d "$SB_DIR/profiles" ]; then
    bad "profiles dir missing: $SB_DIR/profiles"
else
    PROXY_TOTAL=0
    PROXY_BAD=0
    for f in "$SB_DIR"/profiles/*.json; do
        [ -f "$f" ] || continue
        meta=$(profile_meta "$f") || {
            warn "cannot parse $(basename "$f")"
            continue
        }
        IFS='	' read -r pid pname stored inferred server port <<EOF
$meta
EOF
        case "$inferred" in
            http-proxy|https-proxy|socks4|socks4a|socks5)
                PROXY_TOTAL=$((PROXY_TOTAL + 1))
                if [ "$stored" = "$inferred" ]; then
                    ok "$pid ($pname): stored type $stored"
                else
                    PROXY_BAD=$((PROXY_BAD + 1))
                    bad "$pid ($pname): stored type '$stored', inferred '$inferred'"
                fi
                ;;
        esac
    done
    info "proxy profiles checked: $PROXY_TOTAL"
    if [ "$PROXY_TOTAL" -eq 0 ]; then
        warn "no HTTP/SOCKS proxy profiles found"
    elif [ "$PROXY_BAD" -eq 0 ]; then
        ok "all proxy profiles have normalized type"
    else
        warn "$PROXY_BAD proxy profile(s) still have stale type metadata — re-save them from the panel on v1.25.3 and rerun"
    fi
fi

hdr "2. Smart intercept for HTTP/SOCKS proxy targets (v1.25.2)"
SELF_INTERCEPT=$(json_setting_csv self_intercept)
SELF_INTERCEPT_FULL=$(json_setting_csv self_intercept_full)
info "self_intercept: ${SELF_INTERCEPT:-'(empty)'}"
info "self_intercept_full: ${SELF_INTERCEPT_FULL:-'(empty)'}"
if [ ! -f "$CONFIG" ]; then
    bad "config missing: $CONFIG"
elif [ -z "$SELF_INTERCEPT" ]; then
    warn "smart intercept is not configured — enable at least one HTTP/SOCKS proxy target in the panel, then rerun"
else
    OLDIFS=$IFS
    IFS=,
    for target in $SELF_INTERCEPT; do
        IFS=$OLDIFS
        [ -n "$target" ] || continue
        pfile="$SB_DIR/profiles/$target.json"
        if [ ! -f "$pfile" ]; then
            bad "$target: profile missing ($pfile)"
            IFS=,
            continue
        fi
        meta=$(profile_meta "$pfile") || {
            bad "$target: profile parse failed"
            IFS=,
            continue
        }
        IFS='	' read -r pid pname stored inferred server port <<EOF
$meta
EOF
        case "$inferred" in
            http-proxy|https-proxy) expected=http ;;
            socks4|socks4a|socks5) expected=socks ;;
            *) expected=unknown ;;
        esac
        inbound=$(sed -n "s/.*{\"type\":\"\([^\"]*\)\",\"tag\":\"in_ix_${target}\".*/\1/p" "$CONFIG" 2>/dev/null | head -1)
        if [ "$expected" = unknown ]; then
            warn "$target: intercept selected for non-proxy type '$inferred'"
        elif [ "$inbound" = "$expected" ]; then
            ok "$target: inbound type '$inbound' matches expected '$expected'"
        else
            bad "$target: inbound type '$inbound' but expected '$expected'"
        fi
        if [ -n "$server" ] && [ -n "$port" ] && grep -qE "^${server}[[:space:]]+${port}[[:space:]]+" "$INTERCEPT_MAP" 2>/dev/null; then
            ok "$target: intercept.map contains ${server}:${port}"
        else
            bad "$target: intercept.map missing ${server}:${port}"
        fi
        IFS=,
    done
    IFS=$OLDIFS
fi

hdr "3. WAN link watchdog / physical-link diagnostics (v1.25.3)"
if [ ! -x "$WANLINK_BIN" ]; then
    bad "$WANLINK_BIN missing"
else
    ok "$WANLINK_BIN present"
    wan=$(wan_status_tsv)
    if [ -z "$wan" ]; then
        bad "detour-wan-link status returned nothing / unreadable JSON"
    else
        IFS='	' read -r WL_SUPPORTED WL_DEGRADED WL_WAN_IF WL_PHY_IF WL_SPEED WL_PARTNER WL_DUPLEX WL_DIAG WL_ADVICE <<EOF
$wan
EOF
        info "wan_if=$WL_WAN_IF phy_if=$WL_PHY_IF speed=${WL_SPEED}Mbps partner=${WL_PARTNER}Mbps duplex=${WL_DUPLEX:-?}"
        info "diagnosis: ${WL_DIAG:-'(empty)'}"
        if [ "$WL_SUPPORTED" = true ]; then
            ok "speed detection supported on this Keenetic"
        else
            warn "speed detection not supported here — unresolved: need to inspect /sys/class/net/<if>/speed and ethtool availability"
        fi
        if [ "$WL_DEGRADED" = true ]; then
            warn "WAN link currently degraded below 1 Gbps"
            info "advice: ${WL_ADVICE:-'(empty)'}"
        else
            ok "WAN link not currently degraded"
        fi
    fi
fi
if [ -x "$CRON_BIN" ] && grep -q 'detour-wan-link tick' "$CRON_BIN" 2>/dev/null; then
    ok "detour-cron includes detour-wan-link tick"
else
    bad "detour-cron does not include detour-wan-link tick"
fi
if [ -f /opt/var/log/detour-wan-link.log ]; then
    info "detour-wan-link log tail:"
    tail -3 /opt/var/log/detour-wan-link.log 2>/dev/null | sed 's/^/    /'
else
    info "detour-wan-link log: (not created yet)"
fi

hdr "4. Push path for recent diagnostics"
if [ -x /opt/sbin/detour-push ]; then
    ok "detour-push present"
else
    bad "detour-push missing"
fi
if [ -f "$PUSH_SUBS" ]; then
    subs=$(grep -c 'endpoint' "$PUSH_SUBS" 2>/dev/null)
    info "push subscriptions: ${subs:-0}"
else
    info "push subscriptions file missing"
fi

hdr "5. What is still unresolved after this script"
if [ -z "$SELF_INTERCEPT" ]; then
    warn "smart intercept was not configured, so v1.25.2 datapath could not be exercised on this device"
fi
warn "UI-side proxy type editing still needs a quick manual check: open one HTTP or SOCKS profile in the panel, verify the type dropdown is correct, save it unchanged, then rerun this script"
if [ -n "$wan" ] && [ "$WL_SUPPORTED" != true ]; then
    warn "WAN speed support is unresolved on this hardware/runtime"
fi
warn "If push subscriptions are 0, WAN-link alert delivery cannot be validated from shell output alone"

printf '\n=== DONE — paste everything above back ===\n'