-- Полная резервная копия Detour: всё, что нужно, чтобы поднять роутер (или
-- приложение) с нуля, одним JSON. Конверт — надмножество v1
-- (`panel_export_config`): старые ключи на месте, новые рядом, поэтому старый
-- импорт и клиент понимают копию, а новый импорт — старые копии.
--
--   lua detour-backup.lua export <sb_dir> <detour_dir> <zapret_conf> <zapret_domains> <version> <platform>
--   lua detour-backup.lua import <file> <sb_dir> <detour_dir> <zapret_conf> <zapret_domains>
--
-- Пути в `router_files` — относительные к двум корням (`sing-box/`,
-- `detour/`) и пишутся ТОЛЬКО по списку ниже: копия приходит из браузера,
-- и без списка файл вида `../../etc/passwd` был бы записью куда угодно.
--
-- JSON-документы (профили, подписки, настройки, цепочки) переносятся сырым
-- текстом, без decode → encode: lua-cjson 2.1.0 на OpenWrt пишет пустой
-- массив как `{}`, а sing-box на `{}` вместо списка отказывается собирать
-- конфиг. cjson здесь только проверяет, что текст — объект, и достаёт id.

local cjson = require("cjson.safe")

local mode = arg[1]

local function read(path)
  local f = io.open(path, "rb")
  if not f then return nil end
  local s = f:read("*a")
  f:close()
  return s
end

local function encode(v)
  -- «\/» от lua-cjson — тот же JSON, но часть кода роутера читает файлы sed'ом.
  local s = cjson.encode(v)
  return s and (s:gsub("\\/", "/")) or nil
end

local function write(path, body, perm)
  local dir = path:match("^(.*)/[^/]+$")
  if dir then os.execute("mkdir -p '" .. dir .. "'") end
  local tmp = path .. ".restore.tmp"
  local f = io.open(tmp, "wb")
  if not f then return false end
  f:write(body)
  f:close()
  if perm then os.execute("chmod " .. perm .. " '" .. tmp .. "'") end
  return os.rename(tmp, path)
end

local function ls(dir, pattern)
  local out = {}
  local p = io.popen("ls -1 '" .. dir .. "' 2>/dev/null")
  if not p then return out end
  for name in p:lines() do
    if name:match(pattern) then out[#out + 1] = name end
  end
  p:close()
  table.sort(out)
  return out
end

---------------------------------------------------------------- сырой JSON --

local function ws(s, i) return s:find("[^ \t\r\n]", i) or #s + 1 end

-- Индекс сразу за значением, которое начинается в i.
local function value_end(s, i)
  local c = s:sub(i, i)
  if c == '"' then
    local j = i + 1
    while true do
      local p = s:find('["\\]', j)
      if not p then return nil end
      if s:sub(p, p) == "\\" then j = p + 2 else return p + 1 end
    end
  elseif c == "{" or c == "[" then
    local depth, j = 0, i
    while true do
      local p = s:find('[%[%]{}"]', j)
      if not p then return nil end
      local ch = s:sub(p, p)
      if ch == '"' then
        j = value_end(s, p)
        if not j then return nil end
      else
        depth = depth + ((ch == "{" or ch == "[") and 1 or -1)
        if depth == 0 then return p + 1 end
        j = p + 1
      end
    end
  end
  return s:find("[,}%]%s]", i) or #s + 1
end

-- Члены объекта: ключ → исходный текст значения.
local function members(s)
  local out, i = {}, ws(s, 1)
  if s:sub(i, i) ~= "{" then return nil end
  i = ws(s, i + 1)
  while s:sub(i, i) ~= "}" do
    local e = value_end(s, i)
    local key = e and cjson.decode(s:sub(i, e - 1))
    if type(key) ~= "string" then return nil end
    i = ws(s, e)
    if s:sub(i, i) ~= ":" then return nil end
    i = ws(s, i + 1)
    e = value_end(s, i)
    if not e then return nil end
    out[key] = s:sub(i, e - 1)
    i = ws(s, e)
    if s:sub(i, i) == "," then i = ws(s, i + 1) end
    if i > #s then return nil end
  end
  return out
end

-- Элементы массива: исходный текст каждого.
local function elements(s)
  local out, i = {}, ws(s or "", 1)
  if not s or s:sub(i, i) ~= "[" then return {} end
  i = ws(s, i + 1)
  while i <= #s and s:sub(i, i) ~= "]" do
    local e = value_end(s, i)
    if not e then break end
    out[#out + 1] = s:sub(i, e - 1)
    i = ws(s, e)
    if s:sub(i, i) == "," then i = ws(s, i + 1) end
  end
  return out
end

-- Текст JSON-объекта, если это объект; иначе nil.
local function object_text(raw)
  local t = raw and raw:match("^%s*(.-)%s*$")
  return t and t:sub(1, 1) == "{" and type(cjson.decode(t)) == "table" and t or nil
end

-- id документа = имя файла: по нему профиль ищут цепочки и настройки. Нет id —
-- дописать в начало, не тот — пересобрать (редко; тут пустой массив и станет {}).
local function with_id(text, id)
  local d = cjson.decode(text)
  if type(d) ~= "table" or d.id == id then return text end
  if d.id == nil then
    local rest = text:match("^{%s*(.-)$")
    return '{"id":' .. encode(id) .. (rest:sub(1, 1) == "}" and "" or ",") .. rest
  end
  d.id = id
  return encode(d) or text
end

--------------------------------------------------------------- состав копии --

-- Текстовые разделы: ключ конверта → (корень, путь). Ключи совпадают с
-- клиентскими (client/crates/core/src/backend/config.rs).
local TEXTS = {
  { "proxy_domains",      "sb", "proxy-domains.list" },
  { "whitelist_domains",  "sb", "whitelist-domains.list" },
  { "route_map",          "sb", "route-map.list" },
  { "udp_vpn_list",       "sb", "udp-vpn.list" },
  { "health_urls",        "sb", "health-urls.list" },
  { "ru_subnets_exclude", "sb", "ru-subnets-exclude.list" },
  { "autoswitch_exclude", "sb", "autoswitch-exclude.list" },
  { "speedcheck_exclude", "sb", "speedcheck-exclude.list" },
  { "torrent_allow",      "sb", "torrent-allow.list" },
  { "egress_blocklist",   "dd", "blocked-egress-ips.list" },
}

-- Роутерные файлы, у которых нет клиентского аналога: как есть, текстом.
-- Секреты панели (detour.auth, update.conf с токеном GitHub) сюда не входят
-- и не войдут — восстановление не должно менять вход в панель.
local ROUTER_FILES = {
  "sing-box/intercept.map",
  "sing-box/engine",
  "detour/autostart.singbox",
  "detour/autostart.zapret",
  "detour/allvpn.enabled",
  "detour/bypass.autostart",
  "detour/bypass.mode",
  "detour/nfqws2.strategy",
  "detour/offload.conf",
  "detour/portmap.conf",
  "detour/cert.conf",
  "detour/dns-api.conf",
  "detour/hosts-custom.list",
  "detour/hosts.json",
  "detour/rulist.json",
  "detour/wan.conf",
  "detour/wan-watch.conf",
  "detour/push-subs.json",
  "detour/vapid.pem",
  "detour/vapid.pub",
  "detour/detour.conf",
}
-- Каталоги, из которых берутся все файлы по маске имени.
local ROUTER_DIRS = {
  { "detour/portmap.d", "^[%w_.-]+%.htpasswd$" },
}
-- Файлы с ключами — только владельцу.
local PRIVATE = {
  ["detour/cert.conf"] = true, ["detour/dns-api.conf"] = true, ["detour/vapid.pem"] = true,
  ["detour/push-subs.json"] = true,
}

local function allowed(rel)
  for _, r in ipairs(ROUTER_FILES) do
    if r == rel then return true end
  end
  local dir, name = rel:match("^(.*)/([^/]+)$")
  for _, d in ipairs(ROUTER_DIRS) do
    if dir == d[1] and name:match(d[2]) then return true end
  end
  return false
end

local function safe_id(s)
  return type(s) == "string" and s:match("^[%w_.-]+$") and not s:match("^%.") and s or nil
end

local function abs(r, rel)
  local root, rest = rel:match("^([%w-]+)/(.+)$")
  return root and r[root] and (r[root] .. "/" .. rest) or nil
end

-- Документы каталога (профили, подписки): сырые тексты с гарантированным id.
local function dir_docs(dir)
  local out = {}
  for _, name in ipairs(ls(dir, "%.json$")) do
    local t = object_text(read(dir .. "/" .. name))
    if t then out[#out + 1] = with_id(t, (name:gsub("%.json$", ""))) end
  end
  return "[" .. table.concat(out, ",") .. "]"
end

-------------------------------------------------------------------- экспорт --

if mode == "export" then
  local sb, dd, zconf, zdom, version, platform = arg[2], arg[3], arg[4], arg[5], arg[6], arg[7]
  local r = { sb = sb, dd = dd, ["sing-box"] = sb, detour = dd }
  local parts = {}
  local function add(k, text) parts[#parts + 1] = encode(k) .. ":" .. text end

  add("version", "2")
  add("kind", '"full"')
  add("exported_at", encode(os.date("!%Y-%m-%dT%H:%M:%SZ")))
  add("router_version", encode(version or ""))
  add("platform", encode(platform or ""))
  add("settings", object_text(read(sb .. "/settings.json")) or "{}")
  local chains = object_text(read(sb .. "/chains.json"))
  if chains then add("chains", chains) end
  local legacy = object_text(read(dd .. "/subscription.json"))
  if legacy and legacy ~= "{}" then add("subscription", legacy) end
  add("subscriptions", dir_docs(dd .. "/subscriptions"))
  add("profiles", dir_docs(sb .. "/profiles"))
  add("zapret_conf", encode(read(zconf) or ""))
  add("zapret_domains", encode(read(zdom) or ""))
  for _, t in ipairs(TEXTS) do
    add(t[1], encode(read(r[t[2]] .. "/" .. t[3]) or ""))
  end

  local files = {}
  for _, rel in ipairs(ROUTER_FILES) do
    local body = read(abs(r, rel))
    if body then files[#files + 1] = encode(rel) .. ":" .. encode(body) end
  end
  for _, d in ipairs(ROUTER_DIRS) do
    local dir = abs(r, d[1])
    for _, name in ipairs(ls(dir, d[2])) do
      local body = read(dir .. "/" .. name)
      if body then files[#files + 1] = encode(d[1] .. "/" .. name) .. ":" .. encode(body) end
    end
  end
  add("router_files", "{" .. table.concat(files, ",") .. "}")

  io.write("{" .. table.concat(parts, ",") .. "}")
  os.exit(0)
end

-------------------------------------------------------------------- импорт --

if mode == "import" then
  local file, sb, dd, zconf, zdom = arg[2], arg[3], arg[4], arg[5], arg[6]
  local r = { sb = sb, dd = dd, ["sing-box"] = sb, detour = dd }
  local raw = read(file) or ""
  local doc = cjson.decode(raw)
  local m = type(doc) == "table" and members(raw)
  if not m then
    io.write('{"ok":false,"error":"config is not a valid JSON object"}')
    os.exit(2)
  end
  local FORBIDDEN = {
    auth = true, password = true, passwd = true, panel_user = true, panel_password = true,
    update_conf = true, gh_token = true, gh_owner = true, gh_repo = true,
  }
  for k in pairs(doc) do
    if FORBIDDEN[k] then
      io.write(encode({ ok = false, error = "forbidden key in config: " .. k
        .. " — учётные данные панели из файла не переносятся" }))
      os.exit(3)
    end
  end

  local written, skipped = {}, {}
  local function put(key, path, body, perm)
    if write(path, body, perm) then written[#written + 1] = key
    else skipped[#skipped + 1] = key .. ": не удалось записать" end
  end
  -- Каталог документов: каждый элемент — сырым текстом в <dir>/<id>.json.
  local function put_docs(key, dir, list_text)
    local n = 0
    for _, el in ipairs(elements(list_text)) do
      local t = object_text(el)
      local id = t and safe_id(cjson.decode(t).id)
      if id and write(dir .. "/" .. id .. ".json", t, "0600") then
        n = n + 1
      else
        skipped[#skipped + 1] = key .. ": элемент без допустимого id"
      end
    end
    written[#written + 1] = key .. ":" .. n
  end

  -- Профили раньше настроек: settings.active_chain ссылается на них.
  if m.profiles then put_docs("profiles", sb .. "/profiles", m.profiles) end

  local settings = object_text(m.settings)
  if settings and settings ~= "{}" then put("settings", sb .. "/settings.json", settings, "0644") end
  local chains = object_text(m.chains)
  if chains then put("chains", sb .. "/chains.json", chains, "0644") end

  -- v1: пустая строка списка не стирает (так вели себя старые копии); в
  -- полной копии пустой список — тоже значение.
  local full = doc.kind == "full"
  for _, t in ipairs(TEXTS) do
    local v = doc[t[1]]
    if type(v) == "string" and (full or #v > 0) then
      put(t[1], r[t[2]] .. "/" .. t[3], v, "0644")
    end
  end
  if type(doc.zapret_conf) == "string" and #doc.zapret_conf > 0 then put("zapret_conf", zconf, doc.zapret_conf, "0644") end
  if type(doc.zapret_domains) == "string" and (full or #doc.zapret_domains > 0) then
    put("zapret_domains", zdom, doc.zapret_domains, "0644")
  end

  local legacy = object_text(m.subscription)
  if legacy and legacy ~= "{}" then put("subscription", dd .. "/subscription.json", legacy, "0600") end
  if m.subscriptions then
    put_docs("subscriptions", dd .. "/subscriptions", m.subscriptions)
    os.execute("chmod 0700 '" .. dd .. "/subscriptions' 2>/dev/null")
  end

  if type(doc.router_files) == "table" then
    for rel, body in pairs(doc.router_files) do
      local path = type(rel) == "string" and allowed(rel) and abs(r, rel)
      if path and type(body) == "string" then
        put(rel, path, body, PRIVATE[rel] and "0600" or "0644")
      else
        skipped[#skipped + 1] = tostring(rel) .. ": не входит в резервную копию"
      end
    end
  end

  local function list(t)
    local o = {}
    for i, v in ipairs(t) do o[i] = encode(v) end
    return "[" .. table.concat(o, ",") .. "]"
  end
  io.write('{"ok":true,"written":' .. list(written) .. ',"skipped":' .. list(skipped) .. "}")
  os.exit(0)
end

io.stderr:write("usage: detour-backup.lua export|import ...\n")
os.exit(1)
