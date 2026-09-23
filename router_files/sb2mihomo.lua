-- sb2mihomo — перевод готового конфига sing-box в конфиг mihomo.
--
-- Рендер у проекта один (detour-api строит конфиг sing-box: цепочки, цели
-- маршрутов, перехват, UDP через VPN). В режиме «mihomo» тот же конфиг
-- переводится сюда, и вся логика маршрутизации остаётся в одном месте.
--
--   lua sb2mihomo.lua <in.json> <out.json>     (JSON — подмножество YAML)
--
-- Код выхода: 0 — готово, 2 — в конфиге то, что mihomo не умеет (причина в
-- stderr). Правило «protocol: bittorrent» пропускается молча: у mihomo нет
-- такого сниффера, запрет торрентов держит файрвол (detour-torrent).

if io.open("/opt/etc/ndm") or io.open("/opt/etc/detour/platform") then
    package.path = "/opt/share/lua/5.1/?.lua;/opt/share/lua/5.1/?/init.lua;" .. package.path
end
local cjson = require("cjson.safe")

local M = {}
local errors = {}

local function err(msg) errors[#errors + 1] = msg end

local function list(v)
    if v == nil or v == cjson.null then return {} end
    if type(v) == "table" then return v end
    return { v }
end

local function nonempty(v) return v ~= nil and v ~= cjson.null and v ~= "" end

local function set(t, k, v) if nonempty(v) then t[k] = v end end

-- «a:b» (sing-box) → «a-b» (mihomo); одиночный порт как есть
local function port_range(r)
    r = tostring(r)
    local a, b = r:match("^(%d*):(%d*)$")
    if a then return (a ~= "" and a or "1") .. "-" .. (b ~= "" and b or "65535") end
    return r
end

-- ---------------------------------------------------------------- TLS / транспорт

local function apply_tls(p, ob, sni_key)
    local tls = type(ob.tls) == "table" and ob.tls or nil
    if not tls or tls.enabled == false then return false end
    set(p, sni_key, tls.server_name)
    if tls.insecure then p["skip-cert-verify"] = true end
    if type(tls.alpn) == "table" and #tls.alpn > 0 then p.alpn = tls.alpn end
    local utls = type(tls.utls) == "table" and tls.utls or nil
    if utls and utls.enabled ~= false then set(p, "client-fingerprint", utls.fingerprint) end
    local r = type(tls.reality) == "table" and tls.reality or nil
    if r and r.enabled ~= false then
        p["reality-opts"] = { ["public-key"] = r.public_key, ["short-id"] = r.short_id or "" }
        -- reality без отпечатка mihomo не собирает ClientHello
        if not p["client-fingerprint"] then p["client-fingerprint"] = "chrome" end
    end
    return true
end

local function apply_transport(p, ob, tls_on)
    local tr = type(ob.transport) == "table" and ob.transport or nil
    if not tr then return end
    local t = tr.type
    if t == "ws" or t == "httpupgrade" then
        p.network = "ws"
        local o = {}
        set(o, "path", tr.path)
        local host = (type(tr.headers) == "table" and (tr.headers.Host or tr.headers.host)) or tr.host
        if nonempty(host) then o.headers = { Host = host } end
        if tonumber(tr.max_early_data) then o["max-early-data"] = tonumber(tr.max_early_data) end
        set(o, "early-data-header-name", tr.early_data_header_name)
        if t == "httpupgrade" then o["v2ray-http-upgrade"] = true end
        p["ws-opts"] = o
    elseif t == "grpc" then
        p.network = "grpc"
        p["grpc-opts"] = { ["grpc-service-name"] = tr.service_name or "" }
    elseif t == "http" then
        -- sing-box «http» поверх TLS — это h2; без TLS — http/1.1-обёртка
        local hosts = list(tr.host)
        if tls_on then
            p.network = "h2"
            local o = {}
            if #hosts > 0 then o.host = hosts end
            set(o, "path", tr.path)
            p["h2-opts"] = o
        else
            p.network = "http"
            local o = {}
            if nonempty(tr.path) then o.path = { tr.path } end
            if #hosts > 0 then o.headers = { Host = hosts } end
            p["http-opts"] = o
        end
    elseif t == "xhttp" or t == "splithttp" then
        p.network = "xhttp"
        local o = {}
        set(o, "path", tr.path)
        set(o, "host", tr.host)
        set(o, "mode", tr.mode)
        p["xhttp-opts"] = o
    else
        return "транспорт " .. tostring(t) .. " не поддерживается mihomo"
    end
end

-- ---------------------------------------------------------------- WireGuard / AWG

local AWG_KEYS = {
    "version", "jc", "jmin", "jmax", "s1", "s2", "s3", "s4",
    "h1", "h2", "h3", "h4", "i1", "i2", "i3", "i4", "i5",
    "j1", "j2", "j3", "itime",
    "header-protection-key", "content-padding-addition", "rekey-after-time",
    "rekey-timeout", "reject-after-time", "keepalive-timeout",
    "max-handshake-attempts", "random-trailers", "disable-cookies",
}
local AWG_NUM = { version = 1, jc = 1, jmin = 1, jmax = 1, s1 = 1, s2 = 1, s3 = 1, s4 = 1, itime = 1 }
local AWG_BOOL = { ["random-trailers"] = 1, ["disable-cookies"] = 1 }

function M.awg_option(src)
    if type(src) ~= "table" then return nil end
    local opt, n = {}, 0
    for _, k in ipairs(AWG_KEYS) do
        local v = src[k]
        if v == nil then v = src[k:gsub("-", "_")] end
        if nonempty(v) then
            if AWG_NUM[k] then v = tonumber(v)
            elseif AWG_BOOL[k] then v = (v == true or v == 1 or v == "1" or v == "true")
            else v = tostring(v) end
            if v ~= nil then opt[k] = v; n = n + 1 end
        end
    end
    return n > 0 and opt or nil
end

local function split_addrs(addrs, p)
    for _, a in ipairs(list(addrs)) do
        local host = tostring(a):gsub("/%d+$", "")
        if host:find(":", 1, true) then p.ipv6 = p.ipv6 or host else p.ip = p.ip or host end
    end
end

-- плоская форма (профиль/AWG) и форма endpoint с peers[] (sing-box 1.13)
local function wireguard(p, ob)
    p.type = "wireguard"
    set(p, "private-key", ob.private_key)
    split_addrs(ob.address or ob.local_address, p)
    if not p.ip and not p.ipv6 then return "у WireGuard нет адреса интерфейса" end
    if tonumber(ob.mtu) then p.mtu = tonumber(ob.mtu) end
    local peers = type(ob.peers) == "table" and ob.peers or nil
    if not peers or #peers == 0 then
        peers = { {
            address = ob.server, port = ob.server_port, public_key = ob.peer_public_key,
            pre_shared_key = ob.pre_shared_key, reserved = ob.reserved,
            allowed_ips = ob.allowed_ips,
            persistent_keepalive_interval = ob.persistent_keepalive_interval,
        } }
    end
    local function peer_fields(dst, pe)
        set(dst, "server", pe.address)
        if tonumber(pe.port) then dst.port = tonumber(pe.port) end
        set(dst, "public-key", pe.public_key)
        set(dst, "pre-shared-key", pe.pre_shared_key)
        if type(pe.reserved) == "table" and #pe.reserved > 0 then dst.reserved = pe.reserved end
        local allowed = list(pe.allowed_ips)
        dst["allowed-ips"] = #allowed > 0 and allowed or { "0.0.0.0/0", "::/0" }
    end
    if #peers == 1 then
        peer_fields(p, peers[1])
        local ka = tonumber(peers[1].persistent_keepalive_interval)
        if ka then p["persistent-keepalive"] = ka end
        if not p.server or not p.port or not p["public-key"] then
            return "у WireGuard нет сервера, порта или ключа пира"
        end
    else
        p.peers = {}
        for _, pe in ipairs(peers) do
            local d = {}
            peer_fields(d, pe)
            p.peers[#p.peers + 1] = d
        end
    end
    local awg = M.awg_option(ob.amnezia)
    if awg then p["amnezia-wg-option"] = awg end
end

-- ---------------------------------------------------------------- исходящие

-- sing-box outbound/endpoint → прокси mihomo. nil + причина, если нельзя.
function M.proxy(ob)
    local t = ob.type
    local p = { name = ob.tag, udp = true }
    if t ~= "wireguard" and t ~= "amneziawg" then
        set(p, "server", ob.server)
        if tonumber(ob.server_port) then p.port = tonumber(ob.server_port) end
    end
    if nonempty(ob.detour) and ob.detour ~= "direct" then p["dialer-proxy"] = ob.detour end
    if tonumber(ob.routing_mark) then p["routing-mark"] = tonumber(ob.routing_mark) end
    set(p, "interface-name", ob.bind_interface)
    local e

    if t == "vless" then
        p.type = "vless"
        p.uuid = ob.uuid
        set(p, "flow", ob.flow)
        set(p, "packet-encoding", ob.packet_encoding)
        p.tls = apply_tls(p, ob, "servername")
        e = apply_transport(p, ob, p.tls)
    elseif t == "vmess" then
        p.type = "vmess"
        p.uuid = ob.uuid
        p.alterId = tonumber(ob.alter_id) or 0
        p.cipher = nonempty(ob.security) and ob.security or "auto"
        set(p, "packet-encoding", ob.packet_encoding)
        p.tls = apply_tls(p, ob, "servername")
        e = apply_transport(p, ob, p.tls)
    elseif t == "trojan" then
        p.type = "trojan"
        p.password = ob.password
        apply_tls(p, ob, "sni")
        e = apply_transport(p, ob, true)
    elseif t == "shadowsocks" then
        p.type = "ss"
        p.cipher = ob.method
        p.password = ob.password
        if ob.udp_over_tcp == true or (type(ob.udp_over_tcp) == "table" and ob.udp_over_tcp.enabled) then
            p["udp-over-tcp"] = true
        end
        if nonempty(ob.plugin) then
            local opts = {}
            for kv in tostring(ob.plugin_opts or ""):gmatch("[^;]+") do
                local k, v = kv:match("^([^=]+)=?(.*)$")
                if k then opts[k] = v end
            end
            if ob.plugin == "obfs-local" or ob.plugin == "simple-obfs" then
                p.plugin = "obfs"
                p["plugin-opts"] = { mode = opts.obfs or "http", host = opts["obfs-host"] }
            elseif ob.plugin == "v2ray-plugin" then
                p.plugin = "v2ray-plugin"
                p["plugin-opts"] = { mode = opts.mode or "websocket", tls = opts.tls ~= nil,
                                     host = opts.host, path = opts.path }
            else
                e = "плагин shadowsocks " .. tostring(ob.plugin) .. " не поддерживается"
            end
        end
    elseif t == "hysteria2" then
        p.type = "hysteria2"
        p.password = ob.password
        apply_tls(p, ob, "sni")
        p["client-fingerprint"] = nil
        if type(ob.obfs) == "table" and nonempty(ob.obfs.password) then
            p.obfs = ob.obfs.type or "salamander"
            p["obfs-password"] = ob.obfs.password
        end
        if tonumber(ob.up_mbps) then p.up = ob.up_mbps .. " Mbps" end
        if tonumber(ob.down_mbps) then p.down = ob.down_mbps .. " Mbps" end
        if type(ob.server_ports) == "table" and #ob.server_ports > 0 then
            local r = {}
            for _, x in ipairs(ob.server_ports) do r[#r + 1] = port_range(x) end
            p.ports = table.concat(r, ",")
        end
    elseif t == "hysteria" then
        p.type = "hysteria"
        set(p, "auth-str", ob.auth_str)
        set(p, "obfs", ob.obfs)
        apply_tls(p, ob, "sni")
        p["client-fingerprint"] = nil
        p.up = tostring(tonumber(ob.up_mbps) or 10) .. " Mbps"
        p.down = tostring(tonumber(ob.down_mbps) or 50) .. " Mbps"
    elseif t == "tuic" then
        p.type = "tuic"
        p.uuid = ob.uuid
        p.password = ob.password
        apply_tls(p, ob, "sni")
        p["client-fingerprint"] = nil
        set(p, "congestion-controller", ob.congestion_control)
        set(p, "udp-relay-mode", ob.udp_relay_mode)
        if ob.zero_rtt_handshake then p["reduce-rtt"] = true end
    elseif t == "socks" then
        if ob.version and tostring(ob.version) ~= "5" then
            e = "SOCKS" .. tostring(ob.version) .. " mihomo не умеет (только SOCKS5)"
        end
        p.type = "socks5"
        set(p, "username", ob.username)
        set(p, "password", ob.password)
        if apply_tls(p, ob, "sni") then p.tls = true end
    elseif t == "http" then
        p.type = "http"
        p.udp = nil
        set(p, "username", ob.username)
        set(p, "password", ob.password)
        if apply_tls(p, ob, "sni") then p.tls = true end
    elseif t == "wireguard" or t == "amneziawg" then
        e = wireguard(p, ob)
    else
        e = "тип " .. tostring(t) .. " не поддерживается mihomo"
    end
    if e then return nil, e end
    return p
end

-- ---------------------------------------------------------------- правила

local BUILTIN = { direct = "DIRECT", block = "REJECT" }

local function target(name) return BUILTIN[name] or name end

-- Одно правило sing-box → список строк mihomo. Списки значений
-- разворачиваются в отдельные правила: у каждого правила в нашем рендере
-- один матчер, так что AND здесь не нужен.
local function rule_lines(r, providers)
    if r.action == "sniff" or r.action == "hijack-dns" then return {} end
    if r.protocol then return {} end -- bittorrent: держит файрвол
    local tgt
    if r.action == "reject" then tgt = "REJECT"
    elseif r.outbound then tgt = target(r.outbound)
    else return nil, "правило без outbound: " .. (cjson.encode(r) or "?") end

    local out = {}
    local matchers = 0
    local function add(kind, values, extra)
        matchers = matchers + 1
        values = list(values)
        if #values > 8 and (kind == "DOMAIN-SUFFIX" or kind == "IP-CIDR") then
            -- длинные списки — одним inline-провайдером
            providers.n = (providers.n or 0) + 1
            local name = "rs" .. providers.n
            local behavior = kind == "IP-CIDR" and "ipcidr" or "domain"
            local payload = {}
            for _, v in ipairs(values) do
                v = tostring(v)
                if behavior == "domain" then v = "+." .. v
                elseif not v:find("/", 1, true) then v = v .. (v:find(":", 1, true) and "/128" or "/32") end
                payload[#payload + 1] = v
            end
            providers[name] = { type = "inline", behavior = behavior, payload = payload }
            out[#out + 1] = "RULE-SET," .. name .. "," .. tgt .. (behavior == "ipcidr" and ",no-resolve" or "")
            return
        end
        for _, v in ipairs(values) do
            local k = kind
            if kind == "IP-CIDR" then
                v = tostring(v)
                local v6 = v:find(":", 1, true)
                if v6 then k = "IP-CIDR6" end
                -- mihomo требует маску: «1.2.3.4» → «1.2.3.4/32»
                if not v:find("/", 1, true) then v = v .. (v6 and "/128" or "/32") end
            end
            out[#out + 1] = k .. "," .. tostring(v) .. "," .. tgt .. (extra or "")
        end
    end
    if r.inbound then add("IN-NAME", r.inbound) end
    if r.domain then add("DOMAIN", r.domain) end
    if r.domain_suffix then add("DOMAIN-SUFFIX", r.domain_suffix) end
    if r.ip_cidr then add("IP-CIDR", r.ip_cidr, ",no-resolve") end
    if r.process_name then add("PROCESS-NAME", r.process_name) end
    if r.ip_is_private then matchers = matchers + 1; out[#out + 1] = "GEOIP,lan," .. tgt .. ",no-resolve" end
    if tonumber(r.ip_version) == 6 then matchers = matchers + 1; out[#out + 1] = "IP-CIDR6,::/0," .. tgt .. ",no-resolve" end
    if r.network and not r.port_range and not r.port then
        for _, n in ipairs(list(r.network)) do out[#out + 1] = "NETWORK," .. n:upper() .. "," .. tgt end
        matchers = matchers + 1
    elseif r.port_range or r.port then
        local ports = {}
        for _, x in ipairs(list(r.port_range)) do ports[#ports + 1] = port_range(x) end
        for _, x in ipairs(list(r.port)) do ports[#ports + 1] = tostring(x) end
        local net = list(r.network)[1]
        local pr = "DST-PORT," .. table.concat(ports, "/")
        if net then
            out[#out + 1] = "AND,((NETWORK," .. net:upper() .. "),(" .. pr .. "))," .. tgt
        else
            out[#out + 1] = pr .. "," .. tgt
        end
        matchers = matchers + 1
    end
    if matchers > 1 and not (r.network and (r.port_range or r.port)) then
        return nil, "правило с несколькими матчерами не переводится: " .. (cjson.encode(r) or "?")
    end
    if matchers == 0 then
        return nil, "правило без матчеров: " .. (cjson.encode(r) or "?")
    end
    return out
end

-- ---------------------------------------------------------------- входы

local function listener(ib)
    local t = ib.type
    local l = { name = ib.tag, listen = ib.listen, port = tonumber(ib.listen_port) }
    if l.listen == "::" then l.listen = "0.0.0.0" end
    if t == "redirect" then l.type = "redir"
    elseif t == "tproxy" then l.type = "tproxy"; l.udp = true
    elseif t == "mixed" or t == "socks" or t == "http" then
        l.type = t
        if t ~= "http" then l.udp = true end
        if type(ib.users) == "table" and #ib.users > 0 then
            l.users = {}
            for _, u in ipairs(ib.users) do
                l.users[#l.users + 1] = { username = u.username, password = u.password }
            end
        end
    else
        return nil, "вход " .. tostring(t) .. " не переводится"
    end
    return l
end

-- ---------------------------------------------------------------- весь конфиг

function M.convert(sb)
    errors = {}
    local conf = {
        mode = "rule",
        ["allow-lan"] = true,
        ipv6 = true,
        ["unified-delay"] = true,
        ["tcp-concurrent"] = false,
        profile = { ["store-selected"] = false, ["store-fake-ip"] = false },
        dns = { enable = false },
        proxies = {},
        listeners = {},
        rules = {},
    }
    local log = type(sb.log) == "table" and sb.log or {}
    local lvl = log.level or "warn"
    conf["log-level"] = (lvl == "warn" and "warning") or (lvl == "trace" and "debug")
        or (lvl == "fatal" or lvl == "panic") and "error" or lvl

    for _, ib in ipairs(list(sb.inbounds)) do
        local l, e = listener(ib)
        if l then conf.listeners[#conf.listeners + 1] = l else err(e) end
    end

    local function add_proxy(ob)
        if BUILTIN[ob.tag] and (ob.type == "direct" or ob.type == "block") then return end
        if ob.type == "direct" or ob.type == "block" or ob.type == "dns" then return end
        local p, e = M.proxy(ob)
        if p then conf.proxies[#conf.proxies + 1] = p
        else err("исходящий " .. tostring(ob.tag) .. ": " .. e) end
    end
    for _, ob in ipairs(list(sb.outbounds)) do add_proxy(ob) end
    for _, ob in ipairs(list(sb.endpoints)) do add_proxy(ob) end
    -- dialer-proxy на тег, которого нет среди прокси (direct), убираем
    local names = {}
    for _, p in ipairs(conf.proxies) do names[p.name] = true end
    for _, p in ipairs(conf.proxies) do
        if p["dialer-proxy"] and not names[p["dialer-proxy"]] then p["dialer-proxy"] = nil end
    end

    local route = type(sb.route) == "table" and sb.route or {}
    local providers = {}
    local sniff, override = false, false
    for _, r in ipairs(list(route.rules)) do
        if r.action == "sniff" then sniff = true end
        if nonempty(r.override_address) then override = true end
        local lines, e = rule_lines(r, providers)
        if lines then
            for _, x in ipairs(lines) do conf.rules[#conf.rules + 1] = x end
        else
            err(e)
        end
    end
    conf.rules[#conf.rules + 1] = "MATCH," .. target(route.final or "direct")
    providers.n = nil
    if next(providers) then conf["rule-providers"] = providers end

    if sniff then
        -- parse-pure-ip: на redir/tproxy у соединения есть только IP, домен
        -- берётся из SNI/Host. override-destination — только если конфиг
        -- просил подменить адрес (HTTP/SOCKS-выходам нужно имя хоста).
        conf.sniffer = {
            enable = true,
            ["parse-pure-ip"] = true,
            ["override-destination"] = override,
            sniff = {
                TLS = { ports = { 443, 8443 } },
                HTTP = { ports = { 80, "8080-8880" }, ["override-destination"] = override },
                QUIC = { ports = { 443 } },
            },
        }
    end

    local api = type(sb.experimental) == "table" and type(sb.experimental.clash_api) == "table"
        and sb.experimental.clash_api or nil
    if api then
        conf["external-controller"] = api.external_controller
        set(conf, "secret", api.secret)
    end

    if #errors > 0 then return nil, table.concat(errors, "\n") end
    return conf
end

function M.encode(conf)
    -- пустая Lua-таблица кодируется как {}, а mihomo ждёт на этих ключах список
    for _, k in ipairs({ "proxies", "listeners", "rules" }) do
        if type(conf[k]) == "table" and next(conf[k]) == nil then conf[k] = nil end
    end
    -- lua-cjson пишет «/» как «\/», а YAML-парсер mihomo такой escape отвергает
    return (cjson.encode(conf):gsub("\\/", "/"))
end

-- CLI
if arg and arg[0] and arg[0]:match("sb2mihomo") and arg[1] then
    local f = io.open(arg[1], "rb")
    if not f then io.stderr:write("нет файла " .. arg[1] .. "\n"); os.exit(1) end
    local sb = cjson.decode(f:read("*a"))
    f:close()
    if type(sb) ~= "table" then io.stderr:write("не JSON: " .. arg[1] .. "\n"); os.exit(1) end
    local conf, e = M.convert(sb)
    if not conf then io.stderr:write(e .. "\n"); os.exit(2) end
    local out = arg[2] and io.open(arg[2], "wb") or io.stdout
    out:write(M.encode(conf))
    if arg[2] then out:close() end
    os.exit(0)
end

return M
