# Detour — спецификация бэкенда для порта на Windows (Rust + sing-box.exe TUN)

Источник истины — код репозитория на коммите `2870280` (v1.58.0). Ссылки вида
`router_files/detour-api:1234` указывают на строку, где поведение реализовано.
Пути даны для OpenWrt; на Keenetic тот же набор файлов живёт под `/opt`
(шим путей — `router_files/detour-api:148-227`). Реальные адреса/ключи в документ
не попадали; во всех примерах — `vpn.example.com`, `panel.example.com`, `Example VPN`.

Ссылки на `panel/src/…` даны по коммиту `2870280`. В рабочем дереве сейчас идут
незакоммиченные правки под Tauri-клиент (`panel/src/api/client.ts`, `types.ts`,
`App.vue`, `main.ts`, `env.d.ts`, `panel/vite.config.ts`, новый каталог `client/`):
номера строк в этих файлах уже сдвинуты, часть пунктов §5 (тип `Platform`, подпись
платформы) там уже сделана — см. §0.2.

Пометка **«не проверено»** — вывод сделан чтением кода, на железе не подтверждён.
Пометка **«TUN: …»** — что из поведения теряет смысл или меняется в Windows-порте.

---

## 0. Протокол CGI (общие правила для всех action)

- Один эндпоинт: `GET|POST /cgi-bin/detour-api?action=<name>[&param=…]`.
  `action` вынимается `sed 's/.*action=\([^&]*\).*/\1/p'` — жадно, т. е. берётся
  **последнее** вхождение `action=` в query (`router_files/detour-api:1870`).
- **URL-декодирования параметров нет вообще.** `name=`, `id=`, `mode=`, `on=`,
  `eligible=`, `allow=`, `range=`, `filter=` извлекаются sed-ом как есть
  (`router_files/detour-api:3574`, `:2454`, `:3003`, `:3025`); затем обычно
  прогоняются через `sanitize_name` (`[a-zA-Z0-9_-]`, до 64 символов,
  `router_files/detour-api:399-401`). Исключение — `conntrack` вручную
  раскрывает `%3D`/`%3A` (`router_files/detour-api:3196`). Rust-порт может
  декодировать, но санитайзинг id обязан совпадать, иначе id разъедутся.
- Тело POST читается целиком `head -c $CONTENT_LENGTH` в переменную
  (`router_files/detour-api:1871-1880`); три action пишут тело прямо в файл
  (`panel_update_local`, `hosts_upload`, `hosts_custom_save`).
- JSON-тела разбираются **не парсером**, а sed-регэкспами вида
  `s/.*"key"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p` — берутся только строки
  без `"` внутри, флаги — `0|1` или `true|false` поиском подстроки
  (`router_files/detour-api:2627-2630`, `:2751-2755`, `:3898-3901`). Rust должен
  принимать то же самое (включая `"enabled":1`, `"enabled":"1"`), но ничего не
  потеряет, если будет парсить честно.
- Ответ: всегда `Content-Type: application/json` + `Cache-Control: no-cache`
  (`router_files/detour-api:242-245`), HTTP 200 даже на ошибки, кроме:
  `401` + `{"ok":false,"error":"auth"}` без сессии (`router_files/detour-api:2030-2036`)
  и `429` + `Retry-After` при блокировке логина (`router_files/detour-api:1952-1958`).
- Форма ошибки: `{"ok":false,"error":"<текст>"}` (`router_files/detour-api:241`),
  успех без данных: `{"ok":true}` (`router_files/detour-api:233`).
  Неизвестный action → `{"ok":false,"error":"unknown action: X"}` (`router_files/detour-api:4894`).
- Некоторые GET отдают **сырое содержимое файла** (не обёрнутое): `settings`,
  `chains_list`, `profile_get`, `singbox_config`, `subscriptions_list` (обёртка
  есть), `bypass_strategy` (вообще не JSON — голая строка), `keepalive_status`,
  `*_update_status` (содержимое state-файла).
- Авторизация: cookie `detour_session=<64 hex>`; файл сессии
  `/tmp/detour-sessions/<token>` (содержимое — имя пользователя), `touch` при
  каждом запросе, протухает через 7 суток по mtime (`router_files/detour-api:1684-1696`,
  `:1883`). Без авторизации доступны только `panel_setup_status`,
  `panel_first_setup`, `login`, `logout`, `OPTIONS`, и `push_message` по POST
  с endpoint'ом подписки (`router_files/detour-api:1888-2027`).
- Логин: тело POST = **base64** от `"user\npassword"` (`router_files/detour-api:1961-1963`);
  пароль — `openssl passwd -6` (sha512-crypt) в `/etc/detour.auth` строкой
  `user:$6$salt$hash` (`router_files/detour-api:1698-1710`). Троттлинг по IP: 8
  неудач за 900 с → блок 30/120/600/1800 с (`router_files/detour-api:1725-1788`);
  за loopback-прокси IP берётся из первого `X-Forwarded-For`.
  Cookie: `detour_session=<token>; Path=/; HttpOnly; SameSite=Strict; Max-Age=604800`
  (`router_files/detour-api:1968`). Токен — две склеенные UUID без дефисов
  (`router_files/detour-api:1712-1716`).
- Query-параметры, кроме `action`, извлекаются шаблоном `.*[?&]name=…` — т. е.
  параметр обязан идти **не первым** (`router_files/detour-api:3003`, `:3025`);
  панель всегда ставит `action` первым (`panel/src/api/client.ts:117`). В Rust —
  нормальный парсер query.

### 0.1 Клиентский транспорт панели (`panel/src/api/client.ts`)

Что Rust-сервер обязан соблюдать, чтобы Vue-панель работала без правок:
- URL `"/cgi-bin/detour-api"` абсолютный от origin (`client.ts:17`), панель
  собрана с `base: "/detour/"` (`panel/vite.config.ts:12`, `:46`) — сервер отдаёт
  статику `/detour/…` и API `/cgi-bin/detour-api` с одного origin.
- Метод: `opts.method ?? (body !== undefined ? "POST" : "GET")` (`client.ts:130`) —
  любое тело, даже `""`, означает POST. Строковое тело уходит как есть с
  `Content-Type: text/plain`; объекты — `JSON.stringify` **тоже с `text/plain`**;
  бинарь — `application/octet-stream` (`client.ts:94-110`). form-urlencoded не
  используется. Тело надо разбирать независимо от заголовка.
- `credentials: "same-origin"`, `cache: "no-store"`, своих заголовков нет
  (`client.ts:142-149`). Cookie панель не читает — нужен `Set-Cookie` c `Path=/`
  (страница на `/detour/`, API на `/cgi-bin`).
- 401 на любом action → экран логина (`client.ts:59-62`, `panel/src/stores/session.ts:19-24`);
  429 → блокировка на `retry_after` (JSON) / `Retry-After` / 60 с (`client.ts:166-177`).
  Любой другой не-2xx → «Роутер ответил N (action)», текст тела теряется
  (`client.ts:179-181`) — **ошибки отдавать только как 200 + `{"ok":false,"error"}`**.
- Таймаут по умолчанию 20 с (`AbortController`, `client.ts:20`, `:131-138`); у
  тяжёлых action свои (до 600 с, см. §4.9).
- Разбор (`parseLoose`, `client.ts:188-197`): пустое тело → `null`; первый символ
  не `{`/`[` или невалидный JSON → строка. `Content-Type` ответа не проверяется.
- `requestJson` (`client.ts:217-229`): `null` → `ServerRestartingError` (**пустой
  ответ = «сервер перезапускается»**; допустим только у `logout` и
  `GET bypass_strategy`); строка → «Ожидался JSON, пришёл текст»; ошибка — **только**
  при `ok === false` (`client.ts:200-210`); отсутствие `ok` — норма, поэтому сырые
  ответы (config, профиль, state-файлы) проходят без конверта.
- `requestJsonTolerant` (`client.ts:232-243`) превращает пустое тело, сетевую
  ошибку/таймаут, не-JSON и `{ok:false}` в `null` — это главный механизм «фича
  недоступна»: `{ok:false,error:"unknown action: X"}` → UI рисует прочерк.
- `requestRawText` (`client.ts:246-251`) — только `GET bypass_strategy`.
- `poll()` (`client.ts:267-290`): первый запрос **сразу**, дальше по интервалу
  (дефолт 1500 мс / 180 с), исключения кроме `AuthError` глотаются. Следствие:
  для детач-операций state «в работе» должен быть записан **до** ответа
  `started`, иначе первый опрос увидит «done» прошлого запуска (сейчас
  `detach_bg` стартует воркер через `sleep 1`, `router_files/detour-api:354` —
  у WARP это реальная гонка).
- Все тексты ошибок проходят `translateApiError` (`panel/src/api/messages.ts:8-45`):
  любое сообщение, содержащее `timeout` (без учёта регистра), заменяется на
  «Роутер не ответил вовремя» (`messages.ts:34`) — не пропускать это слово в
  ошибках, которые должен увидеть пользователь (например, вывод `sing-box check`).
- **`status.version` должна совпадать с `VERSION` сборки панели**, иначе панель
  один раз на вкладку делает `location.reload()` (`panel/src/stores/status.ts:62-73`, `:81`).
- Логин: тело — `base64(UTF-8(user + "\n" + password))` строкой (`client.ts:293-298`,
  `panel/src/api/auth.ts:14-19`).

### 0.2 Транспорт приложения (рабочее дерево, не закоммичено)

В незакоммиченной версии `panel/src/api/client.ts` при сборке `--mode client`
(`__CLIENT__`) и наличии `window.__TAURI_INTERNALS__` запрос уходит не в `fetch`, а в
Tauri-команду `api`: `invoke("api", {action, params: Record<string,string>, body})`,
где `body = null | {"kind":"text","data":"…"} | {"kind":"base64","data":"…"}`
(бинарь — base64, объект — `JSON.stringify`, `FormData` не поддерживается); ответ
`{status:number, body:string}` разбирается **той же** логикой, что HTTP (401 →
логин, 429 → блокировка, не-2xx → ошибка, пустое тело → «перезапуск», `ok:false`
→ ошибка). Ошибка самой команды → «Нет связи со службой Detour». Rust-сторона:
`client/app/src/lib.rs:7-21` (команда `api` → `detour_core::ipc::call`),
`client/crates/core/src/ipc/mod.rs:29-75` (`Request{action,params,body}`,
`Body` с `#[serde(tag="kind", content="data")]`, `Response{status,…}`),
`client/crates/core/src/backend.rs` (диспетчер; неизвестный action →
`Response::error("not_supported")` — панель через `requestJsonTolerant` покажет
«недоступно»). Для разработки служба поднимает тот же `/cgi-bin/detour-api` по HTTP
(`client/crates/svc/src/devhttp.rs:1-63`). Статика в client-режиме — с `base: "/"`.
Поэтому всё, что ниже сказано про HTTP-коды и тела, одинаково относится и к
`Response.status`/`Response.body`. Set-Cookie/сессии в Tauri-режиме смысла не
имеют: `check_auth` можно всегда отвечать `{"ok":true,"user":""}` (так уже делает
заглушка `backend.rs`).

---

## 1. Хранилище на диске

### 1.1 Профили — `/etc/sing-box/profiles/<id>.json`

Один файл = один профиль. `<id>` = имя файла без `.json`, санитизованное
`[a-zA-Z0-9_-]{1,64}` (`router_files/detour-api:399-401`, `:3602-3606`;
клиентский двойник — `panel/src/components/profiles/uri.ts:143-145`).

Схема (объединение того, что пишут панель, subscription-refresh и detour-warp):

| Ключ | Тип | Кто пишет | Смысл |
|---|---|---|---|
| `id` | string | панель (`uri.ts:314`), warp (`detour-warp:330`), subscription-refresh | совпадает с именем файла; `profile_save` берёт имя файла из `id`, иначе из `name` (`router_files/detour-api:3602-3604`) |
| `name` | string | все | отображаемое имя (в т. ч. с эмодзи-флагом) |
| `type` | string | **перезаписывается бэкендом** при `profile_save` | выводится из `outbound`: `http`→`http-proxy`/`https-proxy` (по `tls.enabled`), `socks`→`socks`+`version` (дефолт `5`, т. е. `socks5`), иначе `outbound.type`; нет ни того, ни другого → `unknown` (`router_files/detour-api:731-774`; клиент — `uri.ts:187-192`) |
| `group` | string | все | «папка» в UI; WARP-профили кладутся в `"WARP"` (`router_files/detour-warp:330`) |
| `uri` | string | панель/подписка | исходная share-ссылка (для обратного экспорта) |
| `routing_mode` | `""\|"proxy-list"\|"all-except"` | панель/подписка | per-profile override режима маршрутизации при активации (`router_files/detour-api:685-716`) |
| `outbound` | object | все | sing-box outbound, `tag` обычно `"proxy"` — при рендере перезаписывается |
| `warp` | object `{device_id, account_type, client_id, created}` | detour-warp | маркер WARP-профиля (`router_files/detour-warp:333-334`); по наличию ключа `"warp"` WARP-профиль и распознаётся (`router_files/detour-warp:425`, `:455`) |
| метаданные подписки | см. §3 | subscription-refresh | привязка к подписке для синхронизации |

Два формата WireGuard-outbound в профилях:
- **плоский** (пишет форма панели): `server`, `server_port`, `private_key`,
  `peer_public_key`, `pre_shared_key`, `local_address[]`, `allowed_ips[]`, `mtu`,
  `reserved[3]` (`panel/src/components/profiles/uri.ts:244-261`);
- **endpoint-формат 1.13** (пишет detour-warp): `address[]`, `private_key`, `mtu`,
  `peers:[{address, port, public_key, allowed_ips, persistent_keepalive_interval, reserved}]`
  (`router_files/detour-warp:316-339`).

Чтение профилей в detour-api идёт **текстом**, не JSON-парсером:
- `extract_profile_outbound` вырезает первый объект после литерала `"outbound"`
  подсчётом скобок (`router_files/detour-api:486-507`);
- `json_val key file` — `sed 's/.*"key"…"\(…\)".*/'` + `head -1`
  (`router_files/detour-api:404-406`). Из-за жадного `.*` на однострочном JSON
  (а cjson пишет именно одной строкой) возвращается **последнее** вхождение
  ключа в строке. Для `type` это может оказаться `outbound.transport.type`
  (`ws`) или `obfs.type` — отсюда возможный «неправильный» `status.singbox.active_type`
  (не проверено). Rust должен читать верхнеуровневый `type`.

Защита от «обрезанного» сохранения: если файл уже существует и содержит
`"server":`, а в новом теле нет ни `"server":`, ни непустого `"uri"` — отказ
(`router_files/detour-api:3607-3619`).

### 1.2 `settings.json` — `/etc/sing-box/settings.json`

Единственный писатель — `write_settings` (`router_files/detour-api:595-683`)
плюс `panel_import_config` (пишет `cjson.encode(doc.settings)` как есть,
`router_files/detour-api:4310-4312`). `write_settings` каждый раз
**переписывает файл фиксированным набором ключей**: всё, чего нет в списке ниже,
теряется при следующей записи (например, legacy `discord_voice_vpn`).
**Все значения — строки в кавычках**, чтение — `get_settings_val` → `json_val`
(только строковые значения, `router_files/detour-api:408-411`).

| Ключ | Тип (фактически строка) | Дефолт при чтении | Кто меняет |
|---|---|---|---|
| `active_profile` | id | `default` (`router_files/detour-api:1644-1648`) | = последний хоп `active_chain` (`router_files/detour-api:604`) |
| `active_chain` | CSV id профилей (хопы, НЕ id цепочки) | `active_profile`, затем `default` (`router_files/detour-api:1650-1655`) | `profile_activate`, `chain_activate`, `chain_save`, CLI `activate` |
| `routing_mode` | `proxy-list\|all-except` | `proxy-list` (`router_files/detour-api:1657-1661`); поставляемый файл — `all-except` (`router_files/settings.json`) | `settings` POST; при активации — override из профиля-выхода (`router_files/detour-api:705-716`) |
| `upstream_ips` | CSV IPv4 серверов из `config.json` и `config-*.json`, домены резолвятся (`nslookup` c таймаутом ~3 с) | `""` | пересчитывается `collect_upstream_ips` при каждой активации (`router_files/detour-api:576-593`) |
| `singbox_mode` | `single\|multi` | `single` (`router_files/detour-api:718-724`) | `settings` POST |
| `vpn_redirect_ifaces` | строка, интерфейсы через пробел/запятую | `""` | только руками (сохраняется при перезаписи, `:607-610`) |
| `health_check_enabled` | `"1"/"0"` | `"1"` (`:617-618`) | `health_config` |
| `health_auto_switch` | `"1"/"0"` | `"0"` (`:619-620`) | `health_config` |
| `health_speed_enabled` | `"1"/"0"` | `"1"` (`:625-626`) | `health_config` |
| `health_speed_url` | URL | `""` (дефолт внутри detour-health) (`:627-628`) | только руками/импорт |
| `health_speed_bytes` | цифры | `"8000000"` (`:632-634`); клэмп 1e6…3e8 в `health_config` (`:2641-2643`) | `health_config` |
| `log_to_syslog` | `"1"/"0"` | `"0"` (`:639-641`) | `log_config` |
| `log_to_syslog_singbox` | `"1"/"0"` | `"0"` (`:642-643`) | `log_config` |
| `self_intercept` | CSV id целей маршрута | `""` (`:649-651`) | `self_intercept` |
| `self_intercept_full` | CSV | `""` | `self_intercept` |
| `udp_vpn_mode` | `off\|list\|all` | `off`; legacy `discord_voice_vpn=1` → `list` (`:653-661`, `:1283-1288`) | `udp_vpn`; на Keenetic всегда `off` |

Механизм override: вызывающий экспортирует `HEALTH_ENABLED_OVERRIDE`,
`HEALTH_AUTOSWITCH_OVERRIDE`, `HEALTH_SPEED_OVERRIDE`, `HEALTH_SPEED_BYTES_OVERRIDE`,
`LOG_SYSLOG_OVERRIDE`, `LOG_SYSLOG_SB_OVERRIDE`, `SELF_INTERCEPT_OVERRIDE`,
`SELF_INTERCEPT_FULL_OVERRIDE`, `UDP_VPN_MODE_OVERRIDE` перед вызовом
`write_settings`. В Rust это просто «прочитать → изменить поле → записать».

Если файла нет, `settings` GET отдаёт
`{"active_profile":"default","active_chain":"default","routing_mode":"proxy-list","upstream_ips":"","singbox_mode":"single"}`
(`router_files/detour-api:3795`).

### 1.3 Цепочки — `/etc/sing-box/chains.json`

`{"chains":[{"id":"<id>","name":"<имя>","hops":["p1","p2",…],"created":<epoch>}]}`
(`router_files/detour-api:9-13`, `:957-977`).
- Единственный писатель — `chain_store_save`/`chain_store_delete`
  (`router_files/detour-api:942-1001`), detour-warp пишет через внутренний CLI
  `detour-api chain-save <id> <name> <hopsCSV>` (`router_files/detour-api:1812-1820`,
  `router_files/detour-warp:250`).
- Валидация: id `[a-zA-Z0-9._-]+`; id не должен совпадать с id профиля (общее
  пространство имён целей маршрута); хопы нормализуются, каждый должен
  существовать; пустое имя → `name=id` (`router_files/detour-api:944-955`).
- Сохранение с тем же id **заменяет** запись (удалить старую + добавить в конец,
  `router_files/detour-api:964-970`) → порядок в списке меняется.
- Пустой список обязан писаться как `[]`, а не `{}` (cjson-грабля,
  `router_files/detour-api:973-975`).
- Активность цепочки определяется **сравнением CSV**: `active_chain == hops CSV`
  (`router_files/detour-api:3701`, `:3727`). Id цепочки в settings не хранится.
- Грабля: id цепочки допускает `.`, но `route_map_targets` и
  `normalize_profile_list` точку вырезают (`router_files/detour-api:833`, `:426`) —
  цепочка с точкой в id не сработает как цель маршрута (не проверено на живом).

### 1.4 Карта маршрутов — `/etc/sing-box/route-map.list`

Текст секциями (`router_files/detour-api:824-868`; редактор —
`panel/src/components/rules/RouteMapEditor.vue:40-97`):

```
// === route:<targetId> ===
// meta: strict=1 via_chain=0
example.com          # D — точное имя
*.example.org        # W — суффикс
203.0.113.7          # I
198.51.100.0/24      # I
```

- `<targetId>` — id профиля **или** id цепочки; символы вне `[a-zA-Z0-9_-]`
  вырезаются (`router_files/detour-api:833`).
- Строки: комментарии `//` и `#` отрезаются; `*.x` → W, домен → D,
  IPv4/CIDR → I; прочее игнорируется (`router_files/detour-api:854-866`).
  Домен: `^[a-zA-Z0-9]([a-zA-Z0-9._-]*\.)+[a-zA-Z]{2,}$`. IPv6 не поддерживается.
- Опции `// meta: k=v …` внутри секции: `strict` (дефолт **1**), `via_chain`
  (дефолт **0**), значения `1|true|yes|on` / `0|false|no|off`
  (`router_files/detour-api:870-910`, `:1095`, `:1103`).
- **Нумерация целей** (общая для CGI и файрвола): проход по целям в порядке файла,
  цель получает номер N, только если секция непуста **и** цель существует
  (файл профиля или цепочка) (`router_files/detour-api:1082-1093`,
  `router_files/sing-box.initd:447-456`). Порты: TCP redirect-инбаунд
  `12400+N` (`ROUTE_PORT_BASE`, `router_files/detour-api:20-25`), UDP
  tproxy-инбаунд `12500+N` (`ROUTE_UDP_PORT_BASE`, `:26-28`), ipset `singbox_t<N>`.
  Цели, чей профиль/цепочка исчезли, уходят в «мёртвый» ipset `singbox_tdead` →
  REDIRECT на порт `12499`, где никто не слушает (fail-closed,
  `router_files/sing-box.initd:65-71`, `:458-471`, `:831-847`).
  TUN: порты и ipset не нужны; нужна только нумерация/порядок правил и
  fail-closed для пропавших целей (правило `reject` на домены такой секции).

### 1.5 Списки доменов/адресов (текст, формат `proxy-domains.list`)

Общий парсер строки: отрезать `//…` и `#…`, trim, `*.` в начале снять; IPv4
или CIDR → адрес; домен по регэкспу выше → домен (покрывает все поддомены);
прочее (заголовки секций без точки) — пропуск (`router_files/sing-box.initd:87-121`,
`:182-199`).

| Файл | Назначение | Пишет | Применение на роутере |
|---|---|---|---|
| `/etc/sing-box/proxy-domains.list` | что гнать в VPN в режиме `proxy-list` | `domains`, `domains_save_restart`, импорт | ipset `singbox_domains` через dnsmasq `ipset=/d/…` + REDIRECT :12345 (`router_files/sing-box.initd:767`, `:903-908`) |
| `/etc/sing-box/whitelist-domains.list` | что пускать **напрямую** в режиме `all-except` | `whitelist`, `whitelist_save_restart`, импорт | ipset `singbox_whitelist` (maxelem 131072) → RETURN до REDIRECT (`router_files/sing-box.initd:753-765`, `:870-901`) |
| `/etc/sing-box/ru-subnets.list` | управляемый блок RU-подсетей (CIDR на строку) | detour-rulist | в тот же `singbox_whitelist` (`router_files/detour-rulist:51`, `router_files/sing-box.initd:761`) |
| `/etc/sing-box/ru-subnets-exclude.list` | исключения из RU-блока → `nomatch` (т. е. в VPN) | `rulist_exclude` | `hash:net nomatch`, приоритет длинного префикса (`router_files/detour-rulist:20-23`, `:52`) |
| `/etc/detour/rulist.json` | состояние rulist: `{source,url,enabled,auto,count,updated,migrated,error}` | detour-rulist | (`router_files/detour-rulist:53`, `:101-107`) |
| `/etc/sing-box/udp-vpn.list` | какой UDP гнать в VPN (режим `list`; в `all` — force-list поверх whitelist) | `udp_vpn_list` | см. формат ниже |
| `/etc/zapret-tpws/domains.list` | домены для DPI-обхода tpws | `zapret_domains*` | ipset `zapret_domains` → REDIRECT :1081 (см. §6) |
| `/etc/detour/blocked-egress-ips.list` | IPv4 на строку, запрет исходящих | `egress_blocklist` | iptables REJECT в OUTPUT и FORWARD (`router_files/detour-api:292-337`) |

Формат строк `udp-vpn.list` (`router_files/sing-box.initd:542-572`,
пример-сид — `build_release.py:486-495`): `1.2.3.4` / `10.0.0.0/24` / домен →
через ipset `singbox_udp_vpn`; `:27015`, `:p1-p2`, `27015`, `p1-p2`, `p1:p2` —
порт/диапазон с любого адреса; `1.2.3.4:8211`, `10.0.0.0/24:27015` — адрес+порт.
Сид по умолчанию: `19294:19344 // Discord voice`.

### 1.6 Однострочные id-списки (один id на строку)

| Файл | Семантика | Пишет | Читает |
|---|---|---|---|
| `/etc/sing-box/autoswitch-exclude.list` | профили, **исключённые** из автопереключения (нет файла = все участвуют) | `autoswitch_set` (`router_files/detour-api:3443-3474`) | detour-health, `profiles_list` |
| `/etc/sing-box/speedcheck-exclude.list` | исключены из фоновых замеров скорости | `speedcheck_set` (`:3476-3502`) | detour-health, `profiles_list` |
| `/etc/sing-box/torrent-allow.list` | **allow**-список: торренты разрешены (нет файла = запрещено везде) | `torrent_set` (`:3504-3559`) | `build_config` (reject-правило), detour-torrent, `profiles_list` |

Флаги держатся вне JSON профиля намеренно: subscription-refresh пересоздаёт
профили с нуля и затёр бы флаг (`router_files/detour-api:3444-3447`).

### 1.7 Цели health-check — `/etc/sing-box/health-urls.list`

`Название|https://url` на строку; `#`/пустые игнорируются; только `http(s)://`;
без названия — хост из URL. Пустой/нет файла → встроенные три цели
(YouTube `generate_204`, `redirector.googlevideo.com/generate_204`,
Google `generate_204`) (`router_files/detour-api:2045-2073`, сид —
`build_release.py:468-478`). Профиль «рабочий», только если открылись **все** цели.

### 1.8 Кэши и state-файлы

| Путь | Формат | Пишет | Читает |
|---|---|---|---|
| `/tmp/detour-ping.db` | TSV `id \t rtt_ms \t ok(1/0) \t ts \t server` (`router_files/detour-api:2432-2437`) | detour-ping (cron), `ping_check` (upsert, `:2495-2501`) | `ping_status` |
| `/tmp/detour-health.db` | TSV `id \t ok \t ts \t rtt \t delays_csv(ms\|-1) \t [dl_kbit/s]` (`router_files/detour-api:2531-2533`) | detour-health | `health_status`, `health_check` |
| `/tmp/detour-health.switch` | JSON последнего автопереключения (отдаётся как есть в `health_status.switch`) | detour-health | `health_status` (`:2525`) |
| `/tmp/detour-health.unsupported` | маркер: sing-box без clash_api | detour-health | `health_status.supported=false` (`:2527`) |
| `/etc/detour/geo.db` | TSV `id \t ip \t cc \t ts` (cc `??` = неизвестно) | detour-geo | `profiles_list.cc`, `geo_status` |
| `/etc/detour/geo.json` | `{updated,count,known,url,error}` (`router_files/detour-geo:95-99`) | detour-geo | `geo_status` |
| `/tmp/detour-egress-ip.tsv` | `chain \t ip \t ts`, TTL 300 с (`router_files/detour-api:77-81`, `:1863`) | CLI `egress-ip-refresh` | `status.singbox.external_ip*` |
| `/tmp/detour-apply.log` | лог детач-операции + строка-сентинел `===DETOUR_APPLY_DONE rc=<n>` (`router_files/detour-api:378-396`) | `start_apply`, `panel_update_*` | `apply_log`, `logs_view?name=apply` |
| `/var/state/detour-update.json` | `{current_version,available_version,last_check(ISO),status,message,changelog_b64}` (`router_files/detour-update:313-332`) | detour-update | `panel_update_status`, `updates_overview` |
| `/var/state/detour-bins.json` (sing-box), `detour-tpws.json`, `detour-nfqws2.json` | `{"current","available","upstream","upstream_newer":bool,"asset","last_check","changelog_b64"}` (`router_files/detour-update:786-793`) | detour-update `*-check` | `*_update_status`, `updates_overview` |
| `/var/state/detour-torrent.json` | `{enforcing,profile,profile_name,hits,clients[],by_signature{},last_event}` (`router_files/detour-torrent:376-406`) | detour-torrent | `torrent_status` |
| `/var/state/detour-warp.json` | `{ok,state(running\|done\|error),message,id,ip,warp,chain,ts}` (`router_files/detour-warp:81-85`) | detour-warp | `warp_status.last` |
| `/tmp/detour-cpustat` | `total idle` прошлого опроса `/proc/stat` | `status` | `status.system.cpu` (`router_files/detour-api:2134-2152`) |
| `/etc/sing-box/intercept.map` | `ip \t port \t inport` | `compute_intercept_inbounds` | sing-box.initd (`router_files/detour-api:14-19`) |
| `/etc/detour/autostart.{singbox,zapret}` | `1\|0` | `singbox_enable/disable`, `zapret_enable/disable` | `status.*.enabled` (`router_files/detour-api:247-278`) |
| `/etc/detour/nfqws2.strategy` | одна строка аргументов nfqws2 | `bypass_strategy` | detour-bypass |
| `/etc/detour/version` | версия панели | postinst | `status.version` |
| `/etc/detour/subscriptions/<id>.json`, legacy `/etc/detour/subscription.json` | см. §3 | `subscription_save_one`, subscription-refresh | см. §3 |
| `/etc/sing-box/config.json`, `config-<target>.json` | готовые конфиги sing-box | `build_config` | init-скрипт, `collect_upstream_ips` |
| Логи | `/var/log/sing-box.log`, `/var/log/zapret-tpws.log`, `/var/log/detour-health.log`, `/var/log/detour-update.log`, `/tmp/detour-apply.log` (`router_files/detour-api:29`, `:55-57`, `:378`) | — | `logs_view`/`logs_clear` (белый список имён `singbox\|zapret\|health\|update\|apply`, `:3099-3132`) |

---

## 2. Сборка конфига sing-box

### 2.1 Вход: нормализация цепочки

`normalize_profile_list` (`router_files/detour-api:421-430`): CSV → каждый элемент
`sanitize_name` → пустые выкинуть → дедуп с сохранением **первого** вхождения.
Порядок хопов: **первый** элемент CSV набирается напрямую, **последний** —
выход к сайту.

### 2.2 `render_chain_config(chain, extra_ob, extra_ep, rules, port=12345, kind=redirect, extra_in)`

`router_files/detour-api:1289-1492`. Шаги:

1. Для хопа i из N (`:1313-1346`):
   - `payload = extract_profile_outbound` (+`strip_quic_utls`);
   - тег: `chain_<i>` для i<N, `proxy` для последнего (`:1321-1325`);
   - `set_outbound_chain_fields` (`:521-545`): выставить `tag`; `detour` = тег
     предыдущего хопа; у первого хопа `detour` удаляется. Итог:
     `proxy.detour=chain_{N-1}`, …, `chain_2.detour=chain_1`, `chain_1` — без detour;
   - если тип профиля `wireguard` → `translate_wireguard_profile` и в `endpoints[]`,
     иначе в `outbounds[]` (`:1329-1344`).
2. `outbounds = [хопы…, extra_ob…, {"type":"direct","tag":"direct"}, {"type":"block","tag":"block"}]`
   (`:1359-1388`). **`block` — legacy special outbound**: по `docs/deprecated.md`
   sing-box 1.13.14 он deprecated с 1.11.0 и «будет удалён в 1.13.0», заменяется
   действием `reject`. На роутере с 1.13.x конфиг с ним проходит `sing-box check`
   (иначе не прошла бы ни одна активация), т. е. фактически ещё принимается, но
   в Rust его лучше не генерировать — ни одно правило на тег `block` не ссылается.
3. `endpoints = [wg-хопы…, extra_ep…]`, ключ опускается, если пусто (`:1477-1482`).
4. `route.rules`: переданный фрагмент, иначе `[{"action":"sniff"}]` (`:1396-1402`).
   Если `TORRENT_REJECT=1` — сразу после sniff вставляется
   `{"protocol":["bittorrent"],"network":["tcp"],"action":"reject"}` (`:1420-1427`).
5. `inbounds`:
   - `kind=mixed`: `{"type":"mixed","tag":"mixed-in","listen":"127.0.0.1","listen_port":port}`;
   - иначе `{"type":"redirect","tag":"redirect-in","listen":"::","listen_port":port}` (`:1429-1443`);
   - затем `extra_in` (инбаунды целей маршрута и перехвата);
   - затем, только если `port==12345` и `udp_vpn_mode!=off`:
     `{"type":"tproxy","tag":"tproxy-udp","listen":"::","listen_port":12350,"network":"udp"}` (`:1445-1458`).
6. `log = {"level":"warn","output":"<SINGBOX_LOG>","timestamp":true}` — не `info`:
   на info лог растёт ~40-50 МБ/сутки (`:1460-1473`).
7. `route.final = "proxy"` (`:1486-1488`).
8. **Нет** секций `dns`, `experimental`, `route.auto_detect_interface`,
   `route.default_domain_resolver`, `ntp`. DNS на роутере целиком за dnsmasq.
   TUN: всё это придётся добавить (см. §2.8); DNS-серверы писать в новом
   формате 1.12+ (`{"type":"udp"|"https"|"local",…}`) — legacy-формат
   (`address`/`address_resolver`) удаляется в 1.14.0 (`docs/deprecated.md` sing-box 1.13.14).

Пример результата для одиночного профиля без карты маршрутов:

```json
{
  "log": {"level": "warn", "output": "/var/log/sing-box.log", "timestamp": true},
  "inbounds": [{"type": "redirect", "tag": "redirect-in", "listen": "::", "listen_port": 12345}],
  "outbounds": [
    {"tag": "proxy", "type": "vless", "server": "vpn.example.com", "server_port": 443, "uuid": "00000000-0000-0000-0000-000000000000", "tls": {"enabled": true, "server_name": "vpn.example.com"}},
    {"type": "direct", "tag": "direct"},
    {"type": "block", "tag": "block"}
  ],
  "route": {"rules": [{"action": "sniff"}, {"protocol": ["bittorrent"], "network": ["tcp"], "action": "reject"}], "final": "proxy"}
}
```

### 2.3 `build_config(mode, chain)` — `router_files/detour-api:1523-1637`

1. `TORRENT_REJECT=1`, если **хотя бы один** хоп активной цепочки не в
   `torrent-allow.list` (`:1533-1547`). Флаг глобальный: в multi-режиме он
   применяется и к конфигам целей, хотя вычислен по активной цепочке (грабля).
2. **multi** (`:1549-1595`): удалить `intercept.map`; `config.json` = render
   активной цепочки на 12345 без правил (только sniff); для каждой цели маршрута
   N (профиль или цепочка, непустая секция): `target_chain` = хопы цепочки |
   (`via_chain=1` → `route_target_chain` = активная цепочка, если цель в ней уже
   есть, иначе `активная,цель`) | сама цель; render на порт `12400+N` →
   `config-<target>.json`. Каждый процесс — отдельный sing-box (`router_files/sing-box.initd:982-992`).
   TUN: multi-режим смысла не имеет (одна TUN-сессия = один процесс), его надо
   свернуть в single или скрыть переключатель.
3. **single** (`:1596-1620`): `compute_route_extras` + `compute_intercept_inbounds`;
   правила перехвата (`IX_RULES`) вставляются сразу после sniff, перед правилами
   карты; render на 12345 со всеми extras.
4. Каждый собранный конфиг проверяется `sing-box check -c` (`:1557`, `:1587`,
   `:1615`); любая ошибка → откат временных файлов, живые конфиги не тронуты,
   печатается причина (`render_fail_reason` называет пропавшие профили, `:1494-1518`).
5. Коммит: `mv` временных файлов; удаляются `config-*.json`, которых нет в
   новом наборе (`:1622-1635`).

`build_config_for_chain` — шим `build_config "$(get_singbox_mode)" "$1"` (`:1639-1642`).

### 2.4 `compute_route_extras` (single) — `router_files/detour-api:1073-1195`

Для каждой цели (в порядке файла), см. нумерацию §1.4:
- цель-цепочка: `tag="out_<id>"`, `via_chain` игнорируется, тип для
  host_override берётся у **последнего** хопа (`:1096-1101`); хопы
  рендерятся `emit_route_chain` с тегами `rc<N>_1…`, последний — `out_<id>`,
  связка через `detour`, **без** активной цепочки (`:1021-1049`, `:1127-1128`);
- цель-профиль: `tag = route_tag_for(id, active)` — `proxy`, если это последний
  хоп активной цепочки; `chain_<i>`, если i-й хоп; иначе `out_<id>`
  (`:1051-1066`). Если `out_<id>` — добавляется отдельный outbound
  (`detour:"proxy"` при `via_chain=1`, иначе без detour), WG → endpoint (`:1129-1143`);
- `host_override` для типов `socks|socks4|socks4a|socks5|http|http-proxy|https-proxy`
  (`:1107-1110`): точные имена → по правилу на имя
  `{"action":"route","domain":["d"],"outbound":tag,"override_address":"d"}`
  (прокси нужен CONNECT по имени, а REDIRECT отдаёт уже IP), суффиксы →
  `domain_suffix` (`:1113-1120`); для остальных типов D+W → один
  `{"action":"route","domain_suffix":[…],"outbound":tag}` (`:1123`, `:1147-1150`);
- адреса → `{"action":"route","ip_cidr":[…],"outbound":tag}` (`:1151-1154`);
- инбаунд `{"type":"redirect","tag":"in_rt_<N>","listen":"::","listen_port":12400+N}`
  добавляется **всегда** (`:1111-1112`);
- `strict=1` → запасное правило `{"action":"route","inbound":["in_rt_<N>"],"outbound":tag}`
  (`:1155-1162`) — трафик, пришедший на порт цели по ipset, не утекает в `final`,
  даже если сниффинг не дал имени;
- UDP (не Keenetic, и `udp_vpn_mode!=off` **или** strict): инбаунд
  `{"type":"tproxy","tag":"in_rt_udp_<N>","listen":"::","listen_port":12500+N,"network":"udp"}`
  + правило `inbound→tag` (`:1163-1179`);
- итог: `rules = [{"action":"sniff"}, <правила имён/адресов всех целей по порядку>, <правила по инбаундам>]`
  (`:1181-1194`).

### 2.5 `compute_intercept_inbounds` (single) — `router_files/detour-api:1197-1270`

Для целей из `settings.self_intercept` с типом socks/http: инбаунд
`{"type":"socks"|"http","tag":"in_ix_<id>","listen":"::","listen_port":12370+k[,"users":[{username,password}]]}`
с теми же учётными данными, что у апстрим-прокси; для целей из
`self_intercept_full` — правило `{"action":"route","inbound":["in_ix_<id>"],"outbound":<tag>}`.
Пишет `intercept.map` для REDIRECT LAN-клиентов, жёстко прибитых к этому прокси.
TUN: фича целиком роутерная (перехват чужого LAN-трафика), в Windows-порте
не нужна — `self_intercept` можно вернуть пустым и скрыть.

### 2.6 Внутренние CLI detour-api (без HTTP)

`router_files/detour-api:1791-1865`:
- `activate <chainCSV>` — тот же конвейер, что панель: `build_config` →
  `write_settings` (routing_mode из профиля-выхода) → `sing-box.initd reconfigure`
  → `stdout: activate: <chain>`; exit 1 при ошибке сборки. Зовёт detour-health
  при автопереключении.
- `chain-save <id> <name> <hops>` — запись цепочки (для detour-warp).
- `render-mixed [<chain>] [<port>=19482]` — печатает конфиг с mixed-инбаундом
  на `127.0.0.1:<port>` для заданной (или активной) цепочки; без `TORRENT_REJECT`
  (флаг выставляет только build_config) — свои запросы роутера торрентами не
  режутся намеренно (`:1537-1538`). Используется detour-warp (19482),
  subscription-refresh (см. §3), egress-ip.
- `egress-ip-refresh [<chain>]` — поднимает временный sing-box с mixed на
  `127.0.0.1:19480`, 5 попыток `curl -4 -x socks5h://127.0.0.1:19480 https://api.ipify.org`,
  пишет `/tmp/detour-egress-ip.tsv`; single-flight через lock-каталог (`:1834-1865`).

### 2.7 Нормализации и обходы граблей sing-box 1.13 (повторить в Rust)

1. **uTLS на QUIC.** `hysteria`/`hysteria2`/`tuic` с `tls.utls` падают на
   каждом dial «unsupported usage for uTLS» → при рендере удалять `tls.utls`
   (профиль хранит поле ради share-link). detour-api делает это текстовой
   регуляркой, если где-то в outbound встречается `"type":"hysteria2?|tuic"`
   (`router_files/detour-api:472-484`); detour-health — по `outbound.type`
   (`router_files/detour-health:425-428`, `:496-497`). Rust: по `outbound.type`.
2. **WireGuard = endpoint, не outbound.** В 1.11 перенесён в `endpoints[]`, в
   1.13 outbound-форма удалена: один WG-профиль в `outbounds[]` роняет весь
   конфиг «unknown field» (`router_files/detour-health:391-395`). Правильный
   конвертер — `to_endpoint` из detour-health (`router_files/detour-health:396-412`):
   `local_address→address`; если нет `peers` и есть `server` → собрать
   `peers:[{address:server, port:server_port, public_key:peer_public_key,
   pre_shared_key, reserved, allowed_ips:["0.0.0.0/0","::/0"]}]` и удалить
   исходные поля; удалить `system_interface, interface_name, gso, network`;
   без peers — профиль непредставим, пропустить его (а не весь конфиг).
   ⚠ `translate_wireguard_profile` в detour-api (`router_files/detour-api:509-519`)
   лишь переименовывает ключи (`local_address→address`, `server→address`,
   `server_port→port`, убирает `network`) — для плоского профиля из формы это даёт
   два ключа `address` и `peer_public_key` на верхнем уровне, т. е. невалидный
   endpoint (вероятно, WG-профиль, заведённый формой панели, не активируется;
   не проверено). Корректно работают только endpoint-профили (WARP). Также
   `to_endpoint` не убирает верхнеуровневый `allowed_ips` плоского профиля (не
   проверено, будет ли это «unknown field»). В Rust — полный конвертер.
3. **Sniff — только правилом.** Inbound-поля `sniff`/`sniff_override_destination`
   удалены в 1.13.0 (FATAL); с 1.11 inbound-sniff не заполняет домен для правил.
   Первым правилом всегда `{"action":"sniff"}` (`router_files/detour-api:1185-1192`, `:1390-1401`).
4. **Сниффер торрентов** называется только `bittorrent` (`utp`/`udptracker` —
   «unknown sniffer»), reject только для `network:["tcp"]`: UDP-детектор uTP
   ложно срабатывает на handshake WireGuard (`router_files/detour-api:1404-1427`).
5. **HTTP/SOCKS-выход и IP-назначения:** для точных имён — `override_address`
   (`router_files/detour-api:1113-1119`). В TUN c sniff+DNS эта проблема та же:
   если назначение — голый IP, HTTP-прокси получит CONNECT на IP.
6. **xhttp/splithttp/kcp** — транспорты Xray; один такой outbound роняет весь
   конфиг («unknown transport type: xhttp») → subscription-refresh их пропускает
   (`router_files/subscription-refresh:597-600`, подробнее §3).
7. **Порты целей** вынесены из 12345+N (коллизия с 12350 на пятой цели,
   `router_files/detour-api:20-28`). TUN: неактуально.
8. **Пустые массивы**: cjson кодирует `{}` вместо `[]` — везде, где панель ждёт
   массив, ответ собирается конкатенацией или патчится (`router_files/detour-api:973-975`,
   `:3359-3361`; `router_files/detour-warp:443-445`). Rust/serde — просто не
   ломать типы.
9. **`routing_mark` — число**, строку sing-box отвергнет (`router_files/detour-health:382-385`).
10. **clash_api** обязателен для health-check (`experimental.clash_api.external_controller`);
    сборки без `with_clash_api` → маркер unsupported (`router_files/detour-health:881-907`).
    Windows-сборки sing-box с GitHub включают clash_api (не проверено для
    конкретной версии — проверить `sing-box version` → `Tags`).
11. **Валидация до коммита** — `sing-box check -c` на каждый конфиг, живой конфиг
    не трогается при ошибке (`router_files/detour-api:1520-1522`).
12. **Лог `warn`**, не `info` (`router_files/detour-api:1460-1466`).

### 2.8 Что завязано на REDIRECT/ipset и потеряет смысл в TUN

Роутерная схема: трафик LAN попадает в sing-box только если iptables его
REDIRECT'нул; решение «VPN или напрямую» принимает **файрвол** по ipset,
наполняемым dnsmasq при резолве (`router_files/sing-box.initd:169-207`,
`:870-931`). В самом конфиге sing-box режима маршрутизации нет — там всё
`final: proxy`. Следствия для TUN:

| Роутерный механизм | Где | В TUN |
|---|---|---|
| `redirect`-инбаунд 12345, `in_rt_N` 12400+N, tproxy 12350/12500+N | detour-api:1437-1458, :1111, :1175 | один `tun`-инбаунд (`auto_route`, `strict_route`, `stack`) |
| режим `proxy-list`: ipset `singbox_domains` + REDIRECT | sing-box.initd:903-908 | правила `domain_suffix`/`ip_cidr` из `proxy-domains.list` → `proxy`; `final: direct` |
| режим `all-except`: RETURN для private, upstream_ips, `singbox_whitelist` | sing-box.initd:870-901 | `ip_is_private`→direct, whitelist+RU-подсети→direct, `final: proxy`; исключения RU (`nomatch`) — правило `ip_cidr`→proxy **перед** правилом RU→direct |
| `upstream_ips` (не завернуть туннель в себя) | detour-api:576-593 | не нужно: sing-box не маршрутизирует собственные соединения (`auto_detect_interface`) |
| strict fail-closed по инбаунду | detour-api:1155-1162 | правила по домену/IP → тег цели без фолбэка; пропавшая цель → `reject` |
| сниффинг имени как единственный источник домена | — | нужен перехват DNS: правило `{"protocol":"dns","action":"hijack-dns"}` + секция `dns` (fakeip или обычный резолв с кэшем); иначе соединения по IP из системного кэша DNS не сматчатся с доменными правилами |
| `udp_vpn_mode` (TPROXY, mangle, fwmark 0x1e, table 106) | sing-box.initd:537-708 | UDP в TUN приходит сам: `off` → `{"network":"udp","action":"route","outbound":"direct"}` (кроме целей маршрута), `list` → udp-правила по списку → proxy, `all` → udp → proxy кроме whitelist/DNS |
| «Все через VPN» (`SINGBOX_ALLVPN`, не переживает `stop`) | detour-api:3255-3300 | флаг, переключающий генерацию на `final: proxy` без списков (кроме private) |
| `vpn_redirect_ifaces`, `self_intercept`, `intercept.map` | detour-api:413-419, :1197-1270 | неприменимо |
| multi-режим (процесс на цель) | detour-api:1549-1595 | неприменимо |
| KEEPFW/FASTSWITCH (fail-closed на время перезапуска), `reconfigure` | sing-box.initd:29-42, :1160-1187 | TUN при остановке процесса снимает маршруты → трафик пойдёт напрямую; для fail-closed нужен kill-switch (например, WFP-фильтр или удержание TUN-адаптера) — отдельное решение |
| conntrack-флаши, прогрев DNS, dnsmasq restart, MPTCP sysctl, GL-метка 0x8000, `open_port_lan` | sing-box.initd:272-335, :850-864, :897-901 | неприменимо |
| egress blocklist (iptables REJECT) | detour-api:292-337 | правило `{"ip_cidr":[…],"action":"reject"}` первым после sniff |
| слой detour-torrent (nft до REDIRECT) | sing-box.initd:21-25, :995-998 | остаётся только reject-правило sing-box (TCP) |

---

## 3. Подписки

Парсер — `router_files/subscription-refresh` (Lua 5.1 + `cjson.safe`,
`router_files/subscription-refresh:1`, `:42`). Keenetic детектится по
`/opt/etc/detour/platform` → префикс `/opt` и pure-Lua cjson из
`/opt/share/lua/5.1` (`:36-40`). Всё ниже — чтение кода; вживую проверялось
только поведение `openssl base64 -d -A` на мусорном вводе (локальный openssl).

### 3.1 Пути, CLI, расписание

Пути (`router_files/subscription-refresh:43-50`): `SUBS_DIR=/etc/detour/subscriptions`
(подписка = `<id>.json`), `LEGACY_CFG=/etc/detour/subscription.json`,
`PROFILES=/etc/sing-box/profiles`, `WHITELIST=/etc/sing-box/whitelist-domains.list`,
`SETTINGS`/`CHAINS` (только чтение — защита от удаления),
`STATE_FILE=/var/state/subscription-refresh.json`.

CLI (`router_files/subscription-refresh:53-62`, док `:11-19`):

| Аргумент | Действие |
|---|---|
| (нет) | только подписки, у которых подошёл срок (`due_for_refresh`) |
| `--all` | фактически то же + строки `skip <id> (autoupdate disabled)` (`:1018-1026`); комментарий «regardless of interval» неверен — `due_for_refresh` на `CLI.all` не смотрит (`:992-999`) |
| `--force` | игнорировать интервал и `autoupdate` (`:993`) |
| `--id=<id>` | одна подписка, неявно `force` (`:60`); нет такой → warn, exit 1, state не пишется (`:1010-1016`) |
| `--no-vpn` | сразу напрямую (`:197`) |

«Пора обновлять» (`router_files/subscription-refresh:992-999`): `force` → да;
`CLI.id==sub.id` → да; `autoupdate` ложен **или отсутствует** → нет (такую
подписку cron не обновит никогда); иначе `now - last_refresh >= (interval_hours or 24)*3600`
(`interval_hours=0` → каждый тик).

Загрузка (`:966-990`): `ls SUBS_DIR/*.json` (алфавит), только файлы с валидным
JSON и полями `url`+`id`; ни одной → legacy-файл с `id="legacy"` по умолчанию.
Служебный `_path` при записи отбрасывается (все ключи на `_`, `:958-961`).

Запуск по расписанию: OpenWrt — cron `17 * * * * /usr/sbin/subscription-refresh >/var/log/subscription-refresh.log 2>&1`
(`build_release.py:590`); Keenetic — демон `detour-cron`, каждые 12 тиков по 300 с,
первый проход сразу после загрузки (`keenetic/sbin/detour-cron:17`, `:49-50`, `:88-89`).
**Блокировки между параллельными прогонами нет** — защищён только порт VPN-прокси (§3.2).

Коды возврата (`:31`, `:1064-1066`): `0` — что-то обработано без ошибок; `1` —
хотя бы одна ошибка или `--id` не найден; `2` — ничего не обработано (нет
подписок/не подошёл срок). ⚠ `subscription_refresh_all` при отсутствии подписок
получит exit 2 и вернёт ошибку.

State-файл (`:79-82`, `:1006`, `:1030-1062`, атомарно tmp+rename, 0644):
`{"ok":true,"skipped":"no subscriptions","ts":N}` или
`{"ok":true,"ts":N,"processed":N,"errored":N,"runs":[{id,ok:true,saved,removed,skipped,source_kind,via}|{id,ok:false,error}]}`
(`ok` в корне всегда true; пустой `runs` кодируется как `{}`). **Читателей нет.**

Лог: stdout с префиксом `subscription-refresh: `, warn → stderr (`:121-122`).
⚠ Строка `refreshing <id> (group=…, url=<URL>)` пишет полный URL подписки с
токеном (`:1033-1034`) — он попадает и в лог, и в поле `output` ответа CGI.
В Rust — маскировать.

### 3.2 Схема файла подписки `/etc/detour/subscriptions/<id>.json`

Права 0600 (`router_files/detour-api:4196`, `router_files/subscription-refresh:962`),
каталог 0700 (`build_release.py:349-350`). Тип в панели — `panel/src/api/types.ts:332-353`.

| Поле | Тип | Пишет | Смысл |
|---|---|---|---|
| `id` | `[a-zA-Z0-9._-]+` | панель | имя файла (`router_files/detour-api:4187-4189`) |
| `url` | `http(s)://…` | панель | (`router_files/detour-api:4191-4194`) |
| `group` | string | панель | `group` профилей и **ключ синхронизации**; пусто → удаления нет, bypass не пишется (`router_files/subscription-refresh:886`, `:899-900`) |
| `title` | string | панель | группа профилей, если `group` пуст (`:740`), но синхронизацию не включает (`:899`) |
| `user_agent` | string | панель | пусто → `sing-box/1.13.2` (`:848-849`) |
| `user_agent_preset` | `singbox\|happ\|v2rayng\|clash\|custom` | панель | только для UI |
| `interval_hours` | number | панель | дефолт 24 |
| `autoupdate` | bool | панель | нет поля = выключено для cron |
| `apply_routing` | bool | панель | профилям `routing_mode:"all-except"`, direct-домены из фида → whitelist |
| `last_refresh` | epoch | refresh | ставится **и при ошибке** (`:940`) — упавшая подписка не повторяется до конца интервала |
| `last_status` | `ok\|error` | refresh | `:942`, `:954` |
| `last_error` | string | refresh | только при ошибке, при успехе ключ удаляется (`:943`, `:955`) |
| `last_saved`, `last_removed`, `last_kept_stale`, `last_skipped`, `last_bypass_written` | int | refresh | `:944-948`; при ошибке остаются от прошлого прогона |
| `last_source_kind` | `json\|uri-list\|uri-list-b64` | refresh | `:949` |
| `last_fetched_bytes` | int | refresh | `:950` |
| `last_via` | `vpn\|direct` | refresh | `:952` |
| `name`, `enabled`, `interval`, `count` | — | старая панель | Vue только показывает (`count ?? last_saved`) |

Незнакомые поля refresh сохраняет (перекодирует весь объект).

### 3.3 Фетч

**User-Agent**: `sub.user_agent`, иначе `sing-box/1.13.2`
(`router_files/subscription-refresh:848-849`; в CGI-превью — `router_files/detour-api:4089`).
Панели типа Remnawave выбирают формат по UA: на `sing-box/*` отдают полный
sing-box-конфиг (`:696-699`), он детерминирован и без xhttp; Happ-формат
рандомизирует xhttp-узлы. Пресеты UI: `sing-box/1.13.2`, `Happ/1.43.0`,
`v2rayNG/1.8.27`, `ClashMetaForAndroid/2.11.13.Meta`.

**Один запрос** `fetch_body(url, ua, proxy, timeout)` (`:224-256`):
`curl -sS -L --max-time T [-x proxy] -A ua -H 'Accept: */*' -w '\nDETOUR_HTTP:%{http_code}' url`.
Редиректы проходятся; нет `-f`, `--connect-timeout`, `--max-filesize` — **размер
не ограничен**. Статус — последний маркер `DETOUR_HTTP:` (`:241-242`). Ошибки по
порядку: пустой вывод → `empty response (network error?)` (`:237`); код `000` →
`network error — curl could not complete the request` (`:243-245`); ≥400 →
`server returned HTTP <code>, not a subscription feed` (`:246-248`); пустое тело
→ `empty response` (`:249`); `<html` где угодно (регистр не важен) →
`server returned an HTML page, not a subscription feed` (`:252-254`).
Заголовки ответа (`subscription-userinfo`, `profile-title`) refresh **не читает**.

**Порядок «через VPN → напрямую»** (`:856-882`):
1. Попытки: `{via:"vpn", proxy:"socks5h://127.0.0.1:19484", timeout:25}` (если
   прокси поднялся), затем всегда `{via:"direct", timeout:30}`.
2. Успех — только если тело разобралось хотя бы в 1 профиль (`:873-877`); иначе
   ошибка `<via>: no profiles parsed (skipped=N, kind=K)` и следующая попытка.
3. Любой провал `fetch_body` через VPN (включая HTTP ≥400 и HTML) →
   `vpn_proxy_down()`, VPN до конца прогона не используется (`:871`, `:193-195`).
4. Ретраев нет. Итоговая ошибка — склейка через `"; "` (`:882`).

**Временный sing-box** (`:151-220`): порт **19484** (19480 — egress-ip, 19482 —
detour-warp и дефолт `render-mixed`); файлы `/tmp/detour-subfetch.{conf,data,pid,log}`;
поднимается лениво, один раз за прогон. Шаги: без VPN при `--no-vpn` или
отсутствии detour-api/sing-box (`:197-198`); живой pid в pidfile → чужой прогон,
идём напрямую, чужое не трогаем (`:200`); `detour-api render-mixed '' 19484 > conf`
(`:204-205`); `sing-box check` (`:208`); `sing-box run -c conf -D data &`, `sleep 2`,
`kill -0` (`:211-216`) — готовность порта не проверяется. Остановка только своего
процесса: kill → sleep 1 → kill -9 → удалить файлы (`:177-187`); всегда в конце
прогона (`:1058`). `socks5h` — имя резолвит выходной узел. Конфиг `render-mixed`
— §2.6 (без reject торрентов, без UDP tproxy, лог в общий `sing-box.log`).

TUN: на Windows отдельный временный процесс не обязателен — можно добавить в
основной конфиг постоянный `mixed`-инбаунд на `127.0.0.1:19484` с правилом
`inbound → proxy`, либо сохранить временный экземпляр ради изоляции (при
работающем TUN собственный трафик службы и так пойдёт по правилам TUN, если не
исключён процессом).

### 3.4 Детект формата тела — `parse_body_into_profiles` (`:739-843`)

Ничего не пишет на диск (безопасно звать на каждую попытку).

**Шаг 1 — JSON** (`:782-814`): `cjson.decode(body)`; объект с `outbounds`
оборачивается в массив (`:786`); дальше только если непустой массив →
`source_kind="json"`. Одиночный объект без `outbounds` (голый outbound) уходит в
шаг 3. Для каждого `doc`:
1. при `apply_routing` — bypass-домены из `doc.routing.rules[]` с
   `outboundTag=="direct"` и `domain`-массивом (`:790-796`);
2. `pick_proxy_outbound(doc)` — v2ray-outbound: `tag=="proxy"` и
   `protocol ∉ SKIP_PROTOS`, иначе первый подходящий (`:680-689`) →
   `v2ray_to_singbox`; не сконвертировался → `skipped++`; имя
   `doc.remarks‖doc.name‖proxy.tag` (`:797-803`). Один v2ray-документ = один профиль;
3. иначе, если есть `doc.type` — это sing-box outbound, импорт **как есть**,
   имя `doc.tag‖doc.name` (`:804-805`);
4. иначе `singbox_config_outbounds(doc)`: из `doc.outbounds` все с
   `type ∉ {direct,block,dns,selector,urltest}` и с `server`+`server_port`, имя
   `ob.tag` (`:693-711`, `:806-811`). **`endpoints` sing-box-конфига игнорируются**
   (WG из подписки не импортируется).

**Шаг 2 — base64-обёртка** (только если профилей 0, `:816-823`;
`try_decode_base64_blob` `:313-326`): убрать whitespace; длина <16 или символы
вне `[A-Za-z0-9+/_=-]` → не base64; `b64_decode` (`:297-311`): `-→+`, `_→/`,
допаддить `=`, `openssl base64 -d -A` (на мусоре даёт пустую строку). Принимается,
если результат содержит `://`, начинается с `{`/`[` или содержит `[Interface]`
→ `source_kind="uri-list-b64"`, тело заменяется. JSON после декодирования
повторно **не** пробуется.

**Шаг 3 — построчно URI** (`:825-840`): `\r` убрать, trim; пустые и `//`, `#` —
пропуск без `skipped`; остальное — `pcall(parse_uri)`, ошибка/nil → `skipped++`
(поэтому неразобранный JSON-конфиг даёт `skipped` = числу строк, отсюда
исторический `skipped=394`). `source_kind` — прежний или `uri-list`.

Не поддерживается: Clash YAML; base64 поверх JSON; WireGuard `.conf`; `happ://`.

### 3.5 URI → sing-box outbound

Помощники: имя — после **последнего** `#` через `url_decode` (`:351-356`);
query — после **первого** `?` (`:358-362`); `parse_qs` декодирует ключи и
значения (`:329-336`); `url_decode`: **`+`→пробел**, затем `%XX` (`:290-294`);
`split_host_port`: порт после последнего `:` (`tonumber`), IPv6-скобки не
снимаются, `host:443/` → порт nil → профиль пропущен, без порта → пропуск
(`:341-349`). Схемы **регистрозависимы** и только `vless://`, `trojan://`,
`vmess://`, `ss://` (`:521-527`); всё остальное (hysteria2/hy2, hysteria, tuic,
wireguard, socks, http, anytls, ssr) → `unknown scheme`, `skipped++`. У всех
outbound `tag:"proxy"`; профиль создаётся только при наличии `server` и
`server_port` (`:760-763`), пустая строка `server` проверку проходит.

**vless** (`:364-407`):

| URI | outbound |
|---|---|
| userinfo до первого `@` | `uuid` (без декодирования, `:368-370`) |
| `host:port` | `server`, `server_port` |
| `flow` | `flow` (если не пусто) |
| `security=tls` | `tls.enabled=true`; `sni`→`server_name`; `fp`→`utls{enabled,fingerprint}`; `alpn`→массив по `,` (без trim) (`:376-384`) |
| `security=reality` | `tls{enabled, server_name: sni‖"", reality{enabled, public_key: pbk‖"", short_id: sid‖""}}`, `fp`→utls; **alpn не переносится** (`:385-391`) |
| `security` нет/`none`/`xtls` | без `tls` |
| `type=ws` | `transport{type:"ws", path?, headers:{Host}?}` (`:394-397`) |
| `type=grpc` | `transport{type:"grpc", service_name?}` из `serviceName` (`:398-400`) |
| `type=h2`/`http` | `transport{type:"http", path?, host:[host]?}` (`:401-404`) |
| `type=xhttp\|splithttp\|httpupgrade\|kcp\|tcp…` | **transport молча не ставится** → TCP-профиль, мёртвый, но конфиг не роняет (`:393`) |
| игнор | `encryption`, `allowInsecure`/`insecure`, `peer`, `headerType`, `mode`, `spx`, `packetEncoding`, `ed` |

**trojan** (`:409-443`): пароль через `url_decode` (`+`→пробел, `:415`);
`tls.enabled=true` **всегда**, даже `security=none` (`:421`); `sni`, `fp`→utls,
`alpn` при любом security (`:423-429`); `security=reality` → `tls.reality{…}`
(`:430-432`); transport только `ws`/`grpc` (`:433-441`).

**vmess** (`:445-482`): всё после `vmess://` → `b64_decode`; **`#name` не
отрезается** (с ним — `vmess: bad base64/JSON`). `add‖address`→`server`
(дефолт `""`), `port`→число (дефолт 443), `id`→`uuid`, `scy`→`security`
(дефолт `auto`), `aid`→`alter_id` (0). `tls=="tls"` (без регистра) →
`tls{enabled}` + `sni`, `fp`→utls, `alpn` (не-строковый `tls` → исключение →
пропуск). `net‖network` (дефолт tcp): `ws` → `path`, `host`→`headers.Host`;
`grpc` → **`path`→`service_name`**; `h2`/`http` → `path`, `host:[host]`;
прочее → TCP. Имя `ps‖name`.

**ss** (`:484-519`): SIP002 (есть `@`) — userinfo декодируется из base64 в
`method:password`; query отрезается, но `/` перед `?` остаётся →
`host:8388/?plugin=…` пропускается; открытый `method:pass` → `missing method:password`.
Legacy: `ss://base64(method:password@host:port)#name`. Плагины отбрасываются.

**v2ray JSON → sing-box** (`:538-678`): `SKIP_PROTOS = freedom, blackhole, dns,
loopback, balancer, chain` (`:539-540`); объект с `type` без `protocol` → nil (`:608`).

| protocol | поля |
|---|---|
| vless | `vnext[1].address/port`, `users[1].id`→`uuid`, `flow` (даже `""`), `encryption` если ≠ `none` (`:615-627`) ⚠ поля `encryption` у sing-box нет — возможно «unknown field» (не проверено) |
| vmess | + `security` (auto), `alterId`→`alter_id` (`:628-639`) |
| trojan | `servers[1].address/port/password` (`:640-649`) |
| shadowsocks | `servers[1].address/port/method/password` (`:650-654`) |
| hysteria/hysteria2 | версия из `settings.version‖hysteriaSettings.version‖имя`; v2: `hysteria2{server, server_port, password: hs.auth, tls}`; v1: `auth_str, obfs, up_mbps, down_mbps` (`:655-676`) |
| прочие | nil → `skipped++` (`:677`) |

TLS из stream (`tls_from_stream`, `:542-563`): `reality` → `serverName`, `fingerprint`→utls,
`publicKey`, `shortId`; `tls|xtls` → `serverName`, `allowInsecure`→`insecure`,
`alpn`, `fingerprint`→utls. Транспорт (`transport_from_stream`, `:565-604`):
`tcp|raw` → нет; `ws` → `path`, `headers` или `{Host}`; `grpc` → `serviceName`;
`h2|http` → `path`, `host`[]; `quic` → `{type:"quic"}`; `httpupgrade` → `path`,
`host`; **любой другой (xhttp, splithttp, kcp) → весь outbound выбрасывается**
(`:597-603`, `:624`, `:636`, `:646`).

**Расхождения с клиентским `panel/src/components/profiles/uri.ts`** (при порте
лучше взять клиентскую, более полную логику и сделать один парсер):

| | сервер | клиент |
|---|---|---|
| схемы | vless, trojan, vmess, ss (регистр важен) | + hysteria2/hy2, tuic, socks/socks5, http/https (регистр не важен) (`uri.ts:488`, `:587-631`) |
| `+` | → пробел | остаётся (`decodeURIComponent`, `uri.ts:444-450`) |
| `@` | первый | последний (`uri.ts:423`) |
| IPv6 | скобки остаются | снимаются (`uri.ts:428-433`) |
| `host:port/…` | пропуск | хвост отрезается (`uri.ts:441`) |
| без порта | пропуск | 443 (`uri.ts:636`) |
| TLS | только `security=tls/reality`, без `peer`/`insecure` | tls/reality/xtls, `sni‖peer`, `allowInsecure/insecure`, без fp для hysteria2/tuic (`uri.ts:452-464`) |
| vmess | `#` ломает; имя `ps‖name` | `#` отрезается; `ps‖remarks`, `sni‖host` (`uri.ts:494-520`) |
| ss | открытый userinfo и `/?plugin` ломают | оба разбираются (`uri.ts:522-563`) |
| `uri` в профиле | всегда `""` (`:776`) | исходная ссылка (`uri.ts:318`) |
| транслит `ц` | `ts` (`:263`) | `c` (`uri.ts:138`) |

### 3.6 id профиля, имя, файл

`make_profile_id(label)` (`router_files/subscription-refresh:259-287`):
кириллица (UTF-8 `\208`/`\209`) по таблице `TRANSLIT` (ж→zh, ц→ts, щ→sch,
ъ/ь→"", й/ы→y, ю→yu, я→ya; прочие D0/D1 → ""), `lower()` для ASCII, каждый байт
вне `[a-z0-9_-]` → `_`, серии `_` схлопнуть, края обрезать, до 64 байт; пусто
→ `profile_<os.time()>`. `label` = имя из ссылки/JSON, иначе `outbound.server`
(`:764-765`). Коллизии внутри одного разбора (`:766-773`): `base:sub(1,60).."_"..n`, n=2,3…

**Префикса подписки нет**: id глобальные, совпадение с ручным профилем или
профилем другой подписки **перезаписывает чужой файл** и переносит его в свою
группу. Переименование узла у поставщика = новый id + удаление старого →
ломаются ссылки из route-map.

Файл `PROFILES/<id>.json`: `cjson.encode`, 0644, атомарно (`:926`, `:71-77`),
на каждом успешном обновлении все профили перезаписываются:

```json
{
  "id": "example_vpn_nl",
  "name": "Example VPN NL",
  "type": "vless",
  "uri": "",
  "group": "Example VPN",
  "routing_mode": "all-except",
  "outbound": {
    "type": "vless", "tag": "proxy",
    "server": "vpn.example.com", "server_port": 443,
    "uuid": "00000000-0000-0000-0000-000000000000",
    "tls": {"enabled": true, "server_name": "vpn.example.com",
            "utls": {"enabled": true, "fingerprint": "chrome"}}
  }
}
```

`type` = `infer_type` (`:529-536`, совпадает с `uri.ts:187-192`); `routing_mode`
пишется только при `apply_routing` (`:778`). **Ссылки на подписку в профиле нет** —
связь только через строковое равенство `group`. Маркеры есть только в whitelist:
`// === subscription:<group> ===` … `// === end subscription:<group> ===`
(`:714-732`); старая секция удаляется, новая дописывается в конец; пишется только
при `apply_routing`, непустом `group` и непустом списке доменов (`:886-889`).
Очистка доменов: снять `domain:`/`domainSuffix:`, пропустить `geosite:`,
`regexp:`, `full:`, trim, lower, дедуп (`:749-757`).

Флаги autoswitch/speedcheck/torrent/geo живут вне профиля (§1.6) и переживают
обновление; новый профиль: торренты запрещены, autoswitch/speedcheck разрешены.

### 3.7 Синхронизация (удаление устаревших) — `:891-928`

1. Только после успешного разбора (≥1 профиль); пустой/битый фид ничего не удаляет.
2. `sub.group == ""` → удаления нет (профили всё равно пишутся с `group=title`).
3. Защищённые id (`protected_profile_ids`, `:91-117`): все элементы
   `settings.active_chain`, `settings.active_profile`, все `chains.json → hops[]`.
4. Для каждого `PROFILES/*.json` с `prev.group == group` и `prev.id`, которого нет
   в фиде: защищённый → `kept_stale++`, остальные → `os.remove`, `removed++`.
   Сравнивается поле `id` в JSON (профиль без `id` не удаляется никогда).
5. Удаление идёт **до** записи новых (`:897-928`).

Следствия: ручной профиль в папке подписки будет удалён; две подписки с
одинаковым `group` удаляют профили друг друга; ручные правки профилей подписки
затираются. **Перезапуска/reconfigure sing-box после обновления нет** — работающий
конфиг держит старый outbound до следующего `build_config` (не проверено, как
панель это компенсирует). `subscription_delete_one` удаляет только файл
подписки, профили остаются.

### 3.8 API подписок (транспорт — синхронный)

Схемы started+poll нет: CGI ждёт окончания `subscription-refresh`, панель ставит
таймауты 180 с и 600 с (`panel/src/api/profiles.ts:152-162`). Сработает ли раньше
таймаут uhttpd — не проверено. Таблица action — §4.4. Дополнительно:
- `subscription_fetch` ходит **только напрямую** (без VPN), `--max-time 20`, `-L`,
  `-D` для заголовков, берётся последнее значение по цепочке редиректов; без `-f`
  → HTTP 4xx/5xx приходит как `ok:true` со страницей ошибки в `body`.
- `subscription_save_one`: `id`/`url` вынимаются жадным sed (последнее вхождение),
  тело пишется **без проверки JSON**. Vue присылает весь черновик вместе с
  загруженными `last_*` → если refresh прошёл между загрузкой и сохранением,
  свежие `last_*` затираются.
- Кнопка «Сохранить и обновить» = `subscription_save_one` + `subscription_refresh_one`.
- Legacy `subscription_save`/`subscription_get` используют жёсткий
  `/etc/detour`, а не `$DETOUR_DIR` (на Keenetic refresh их не видит).
- `panel_export_config`/`panel_import_config` переносят только legacy-файл.

### 3.9 Грабли sing-box, которые парсер обходит или нет

1. **xhttp/splithttp/kcp** роняют весь конфиг (`unknown transport type: xhttp`) →
   в JSON-ветке outbound выбрасывается (`:597-603`), в URI-ветке транспорт молча
   теряется (`:393`, `:433`).
2. **uTLS на QUIC** парсер не вырезает — вырезает рендер (§2.7 п.1).
3. **inbound sniff** удалён в 1.13 — рендер ставит правило `sniff` (§2.7 п.3).
4. **WireGuard** из подписок не создаётся; `endpoints` игнорируются; плоский
   `type:wireguard` с `server` импортировался бы как есть (не проверено).
5. **Удалённый активный профиль** ломает каждую пересборку — отсюда защита id (§3.7).
6. Не обойдено (не проверено): `reality` без `fp` → возможно
   «uTLS is required by reality client»; `encryption` у vless из v2ray JSON;
   outbound из sing-box-конфига с `domain_resolver`/`detour` на отсутствующие
   теги (`detour` рендер переписывает, `domain_resolver` — нет); IPv6 в скобках.

Что Rust-порту решить сознательно (воспроизвести или исправить): `--all` без
`--force` не игнорирует интервал; нет `autoupdate` → cron не обновляет; `last_refresh`
при ошибке; VPN отключается на HTTP ≥400/HTML; токен в логе и `output`; id без
префикса подписки и `group` как ключ удаления; `+`→пробел, хвост `/`, `#` в
vmess, открытый userinfo в ss; нет перезагрузки sing-box после обновления;
`subscription_fetch` только напрямую.

---

## 4. Контракты action

Обозначения: **Q** — query-параметры (без URL-декодирования, см. §0), **B** —
тело POST, **R** — ответ, **FX** — побочные эффекты, **async** — операция
возвращается сразу, результат опрашивается. «restart» = `sing-box.initd restart`
(полный: DNS-прогрев, ipset, conntrack; ~секунды…100 с), «reconfigure» = быстрый
перезапуск только демона с удержанием файрвола (`router_files/sing-box.initd:1160-1187`).
Ошибка везде — `{"ok":false,"error":"…"}`, если не сказано иное. Где метод
«GET|POST» — ветвление по `REQUEST_METHOD`; где «POST» — GET вернёт
`{"ok":false,"error":"POST required"}`. Ожидания панели по каждому action — §4.9.

### 4.1 Статус, настройки, авторизация

**`check_auth`** — GET. R `{"ok":true,"user":"<имя из файла сессии>"}`
(`router_files/detour-api:2078-2083`). Без сессии — 401 до входа в case.

**`status`** — GET (`router_files/detour-api:2085-2283`). R:
```jsonc
{
  "platform": "openwrt"|"keenetic",           // TUN-порт: "windows"
  "panel_port": 8080,                          // число; на Keenetic из lighttpd-конфига
  "version": "1.58.0",                         // /etc/detour/version, "?" если нет
  "binaries": {
    "bins_version": "1.13.x",                  // = singbox_version (legacy-имя)
    "singbox_version": "1.13.x"|"?",           // из базы opkg/apk, фолбэк `sing-box version`
    "singbox_present": true,                   // -x бинарника
    "tpws_present": true, "tpws_version": "v72.12"|"?",
    "nfqws2_present": false, "nfqws2_version": "?",
    "nfqws2_supported": true                   // false на Keenetic
  },
  "singbox": {
    "running": true,                           // pgrep по пути бинарника
    "pid": "1234"|"null",                      // ⚠ СТРОКА "null", не null (:2242)
    "port": "12345"|"null",                    // ⚠ строка, из netstat
    "enabled": true,                           // автозапуск (§1.8 autostart)
    "allvpn": false,                           // iptables -C … SINGBOX_ALLVPN
    "domains": 12, "ips": 3, "entries": 15,    // подсчёт строк proxy-domains.list
    "ipset_count": 240, "ipset_members": "1.2.3.4,…",  // живой ipset singbox_domains
    "active_profile": "<id>", "active_type": "<type>",
    "external_ip": "203.0.113.5"|"",           // кэш egress-ip, только если chain совпал
    "external_ip_checked": 1726000000,         // epoch, 0 = нет
    "external_ip_refreshing": false,           // true = только что запущен фоновый refresh
    "active_chain": ["p1","p2"],
    "routing_mode": "proxy-list"|"all-except",
    "singbox_mode": "single"|"multi",
    "route_targets": 2,                        // число секций в route-map (без проверки существования)
    "self_intercept": ["id"]
  },
  "zapret": {
    "running": false, "pid": "null", "port": "null", "enabled": false,
    "domains": 0, "ips": 0, "ipset_count": 0,
    "args": "<первая строка zapret-tpws.conf>"
  },
  "system": {
    "mptcp": "0"|"unknown", "uptime": "3d 4h 5m", "memory": "120MB/512MB",
    "disk_free": "40MB", "cpu": "12"|"?",       // ⚠ cpu — строка; "?" на первом опросе
    "cpu_cores": 4
  },
  "wan_link": {"ok":true,"supported":false,"degraded":false,"diagnosis":"","advice":""}
}
```
FX: пишет `/tmp/detour-cpustat`; если sing-box запущен, а кэш внешнего IP устарел
(>300 с) или от другой цепочки — детачит `detour-api egress-ip-refresh <chain>`
(`:2167-2181`). TUN: `ipset_*`, `mptcp`, `allvpn` (как iptables) теряют смысл —
отдавать нули/`false`; `wan_link` отдавать дефолт `supported:false`.

**`settings`** — GET|POST (`router_files/detour-api:3762-3798`).
GET R — сырой `settings.json` (§1.2) или дефолт. POST B
`{"routing_mode"?:"proxy-list"|"all-except","singbox_mode"?:"single"|"multi"}`
(пропущенное — текущее); валидация; `build_config(new_mode, active)` →
`write_settings` → **restart** + `sleep 3` → `{"ok":true}`; ошибка сборки →
`config build failed: …`.

**`updates_overview`** — GET, только читает state-файлы, сеть не трогает
(`router_files/detour-api:2927-2987`). R:
```jsonc
{"panel":  {"update_available":bool,"available_version":"","current_version":"","last_check":"ISO","changelog_b64":""},
 "singbox":{"update_available":bool,"available_version":"","current_version":"","last_check":"","upstream":"","upstream_newer":bool,"changelog_b64":""},
 "tpws":   {…как singbox…}, "nfqws2": {…как singbox…}}
```
`update_available` для фид-пакетов = `available` непуст, не `?`/`n/a` и ≠ `current`
(`:2930-2939`); для панели — `status=="update_available"`. `upstream_newer`
подавляется, если фид уже предлагает обновление (`:2962-2968`).

**`apply_log`** — GET (`router_files/detour-api:2903-2922`). R
`{"ok":true,"done":bool,"rc":"<цифры>"|"","log":"<текст до сентинела>"}`.
`done=true`, когда в логе появилась строка `===DETOUR_APPLY_DONE rc=N`.

**`log_config`** — GET|POST (`router_files/detour-api:2662-2690`). GET R
`{"enabled":bool,"singbox":bool}`. POST B `{"enabled"?:0|1,"singbox"?:0|1}` →
`write_settings` с override → фоновый restart detour-logbridge → R
`{"ok":true,"enabled":bool,"singbox":bool}`. TUN: syslog-мост неприменим
(можно писать в Windows Event Log или игнорировать).

**`logs_clear`** — POST, Q `name=singbox|zapret|health|update|apply` → обнуляет
файл → `{"ok":true}`; иначе `unknown log` (`router_files/detour-api:3119-3132`).
Парный GET `logs_view?name=` → `{"ok":true,"name","path","log":"<tail -300>"[,"missing":true]}`
(`:3099-3117`).

**`secure_dns_set`** — POST B `{"mode":"…","list":"url1\nurl2"}` → `uci set
gl-dns-v2.@dns[0].mode`, `secure_manual_list`, `/etc/init.d/gl_dns boot` →
`{"ok":true}`; пустой mode → `invalid mode` (`router_files/detour-api:3979-3998`).
Чисто GL.iNet-функция; парное чтение — поля `secure_dns_mode`/`secure_dns_list`
в `hosts_status` (`:3960-3977`). TUN: неприменимо.

### 4.2 Профили

**`profiles_list`** — GET (`router_files/detour-api:3303-3441`). R
```jsonc
{"profiles":[{"id":"","type":"","name":"","group":"","routing_mode":"",
              "autoswitch":true,   // НЕ в autoswitch-exclude.list
              "speedcheck":true,   // НЕ в speedcheck-exclude.list
              "torrents":false,    // В torrent-allow.list
              "cc":"NL"|""}],      // страна из geo.db
 "active":"<id последнего хопа>","active_chain":["p1","p2"]}
```
Порядок профилей — порядок glob `*.json` (алфавитный по имени файла). Пустой
список обязан быть `[]` (`:3359-3361`). Секретов (outbound) не отдаёт.

**`profile_get`** — GET, Q `name=<id>` → сырой файл профиля (§1.1), иначе
`name required` / `profile not found` (`router_files/detour-api:3573-3580`).

**`profile_save`** — POST, B = полный JSON профиля (§1.1). Нормализация `type`
(`normalize_profile_json`), имя файла из `id`/`name` → санитайз; защита от
потери параметров подключения; перезапись файла целиком → `{"ok":true}`
(`router_files/detour-api:3596-3622`). **Конфиг не пересобирается** и sing-box не
перезапускается, даже если профиль активен (изменение вступит в силу при
следующей активации) — не проверено, нет ли пересборки со стороны панели.

**`profile_delete`** — POST, Q `name=<id>`. Отказ, если профиль в активной
цепочке (`cannot delete active chain profile`) или в сохранённой цепочке
(`профиль используется в цепочке: <имена>`); иначе `rm` → `{"ok":true}`
(`router_files/detour-api:3624-3641`). route-map, exclude/allow-списки не чистятся.

**`profile_activate`** — POST, Q `name=<id>` → `build_config(mode, id)` →
`routing_mode` из профиля (если задан) → `write_settings` → **reconfigure** +
`sleep 3` → `{"ok":true}` (`router_files/detour-api:3736-3759`).

**`profiles_export`** — GET → `{"version":1,"exported_at":"<ISO UTC>","profiles":[<сырые файлы профилей>]}`
(`router_files/detour-api:3582-3594`). ⚠ Содержит секреты (ключи/пароли) —
отдаётся только авторизованному.

**`autoswitch_set`** / **`speedcheck_set`** — POST, Q `eligible=1|0`, B = id через
`\n` или `,`. `1` → убрать из exclude-списка, `0` → добавить (дедуп). R
`{"ok":true,"excluded":<строк в списке>}` (`router_files/detour-api:3443-3502`).

**`torrent_set`** — POST, Q `allow=1|0`, B = id. ⚠ Направление обратное:
`allow=1` **добавляет** в allow-список. Если затронут профиль активной цепочки —
`build_config` + **reconfigure**; затем `detour-torrent clear && apply`. R
`{"ok":true,"allowed":<n>}`; ошибка сборки → `флаг сохранён, но конфиг не собрался: …`
(`router_files/detour-api:3504-3559`).

**`torrent_status`** — GET → вывод `detour-torrent status` (state §1.8) или
`{"enforcing":false}` (`router_files/detour-api:3561-3571`).

### 4.3 Цепочки

**`chains_list`** — GET → сырой `chains.json` или `{"chains":[]}` (`router_files/detour-api:3669-3675`).

**`chain_save`** — POST, B `{"id":"","name":"","hops":["p1","p2"]}` →
`chain_store_save` (валидация §1.3). Если цепочка была активной (старый CSV хопов
== `active_chain`) — пересборка с НОВЫМИ хопами и `write_settings`; если она
цель в route-map — пересборка с активной цепочкой; в обоих случаях
**reconfigure** + `sleep 3`. R `{"ok":true}` / `{"ok":false,"error":"<текст валидации>"}` /
`цепочка сохранена, но конфиг не собрался: …` (`router_files/detour-api:3677-3720`).

**`chain_delete`** — POST, B `{"id":""}`; отказ, если хопы цепочки == активная
цепочка (`цепочка сейчас активна`) (`router_files/detour-api:3722-3734`). Ссылки
на неё в route-map не чистятся → секция уйдёт в «мёртвую» цель (fail-closed).

**`chain_activate`** — POST, B = **сырой CSV** хопов (`p1,p2`), не JSON →
`build_config` → routing_mode из последнего хопа → `write_settings` →
**reconfigure** + `sleep 3` → `{"ok":true}` (`router_files/detour-api:3643-3664`).

### 4.4 Подписки (детали — §3)

**`subscription_save_one`** — POST, B = объект подписки целиком (схема §3);
обязательны `id` (`[a-zA-Z0-9._-]`) и `url` (`http(s)://`). Сохраняется **как
есть** (сервер схему не проверяет) атомарно в `/etc/detour/subscriptions/<id>.json`,
права 600 → `{"ok":true}`. Фетча не запускает (`router_files/detour-api:4180-4199`).

**`subscription_delete_one`** — POST, Q `id=` → `rm` файла подписки → `{"ok":true}`
(`router_files/detour-api:4201-4211`). Профили подписки **не удаляются** ни этим
action, ни панелью (`panel/src/components/profiles/SubscriptionsPanel.vue:284`).

Смежные (вне списка, но панель их вызывает — см. §3/§4.9):
`subscriptions_list` → `{"ok":true,"subscriptions":[<сырые файлы>]}` (`:4167-4178`);
`subscription_fetch` (превью без сохранения, B `{"url","user_agent"?}`, UA по
умолчанию `sing-box/1.13.2`, curl `--max-time 20`, R
`{"ok":true,"body":"<тело>","headers":{"profile-title","profile-update-interval","subscription-userinfo","support-url","content-type"}}`,
`:4084-4123`); `subscription_refresh_one` (POST Q `id` → синхронно
`subscription-refresh --id=<id> --force`, R `{"ok":true,"output":"…"}` или
`{"ok":false,"error":"refresh failed (exit N)","output":"…"}`, `:4213-4231`);
`subscription_refresh_all` (`--all --force`, та же форма, `:4233-4247`);
legacy `subscription_save/get/refresh` (`:4125-4160`).

### 4.5 Списки маршрутизации

**`route_map`** — GET|POST (`router_files/detour-api:2312-2335`). GET R
`{"routemap":"<текст файла>"}`. POST B = сырой текст (§1.4) → запись →
`build_config` → `write_settings` (upstream_ips) → **restart** + `sleep 3` →
`{"ok":true}` / `config build failed: …` (файл уже перезаписан!).

**`domains`** — GET → `{"domains":"<текст>"}`; POST B текст → только запись
(`echo` добавляет `\n`), `{"ok":true}` (`router_files/detour-api:2285-2297`).
**`domains_save_restart`** — POST → запись + **restart** + `sleep 2` (`:2299-2308`).

**`whitelist`** / **`whitelist_save_restart`** — то же с ключом `"whitelist"` и
файлом `whitelist-domains.list` (`router_files/detour-api:3859-3882`).

**`rulist_status`** — GET → `{"supported":true,"source":"maxmind"|"rir","source_label":"…","url":"…","enabled":bool,"auto":bool,"count":N,"updated":epoch,"migrated":N,"excluded":N,"live_entries":N,"error":"…"}`;
без хелпера `{"supported":false,"error":"detour-rulist not installed"}`
(`router_files/detour-api:3904-3907`, `router_files/detour-rulist:114-123`).
`live_entries` — размер живого ipset (TUN: число правил/CIDR в конфиге).

**`rulist_set`** — POST B `{"source"?:"maxmind"|"rir","auto"?:bool,"enabled"?:bool}`.
Смена источника только запоминается (скачивание — `rulist_update`); `enabled`
сразу применяет список к живому ipset. R — тот же объект статуса **без**
`supported` (`router_files/detour-api:3921-3941`, `router_files/detour-rulist:397-424`).

**`rulist_update`** — POST B `{"source"?:…}` → синхронно скачать (основной URL +
зеркало), провалидировать (не HTML, не слишком мало подсетей), записать, применить
(diff в ipset, без рестарта sing-box) → R — объект статуса (с `error` при
неудаче) (`router_files/detour-api:3909-3919`, `router_files/detour-rulist:318-366`).
Источники: maxmind — `Loyalsoldier/geoip` `text/ru.txt` (+jsDelivr), rir —
ipdeny `ru-aggregated.zone` (+ipverse) (`router_files/detour-rulist:68-79`).

**`rulist_exclude`** — GET → `{"exclude":"<текст>"}`; POST B текст → замена
файла исключений + применение → объект статуса (`router_files/detour-api:3943-3953`).

**`egress_blocklist`** — GET → `{"list":"<текст>"}`; POST B текст → оставить только
валидные IPv4, дедуп, запись, применить (iptables/`firewall.user`) → `{"ok":true}`
(`router_files/detour-api:2607-2620`). TUN: `reject`-правило.

**`udp_vpn`** — GET → `{"mode":"off|list|all","supported":bool,"list":"<текст udp-vpn.list>"}`;
POST B `{"mode":"off|list|all"}` → `build_config` с override → `write_settings` →
**restart** → `{"ok":true}`; на Keenetic POST — ошибка (`router_files/detour-api:3808-3835`).

**`udp_vpn_list`** — GET → `{"list":"…"}`; POST B текст → запись; restart только
если режим ≠ off (`router_files/detour-api:3839-3856`).

**`allvpn_on`** — POST; нужен запущенный sing-box (`sing-box is not running`);
создаёт nat-цепочку `SINGBOX_ALLVPN`: RETURN для 10/8, 172.16/12, 192.168/16,
127/8 и `upstream_ips`, остальной TCP → REDIRECT :12345; прыжок из PREROUTING
для LAN и `vpn_redirect_ifaces`; на Keenetic — маркер-файл → `{"ok":true}`
(`router_files/detour-api:3255-3287`). **`allvpn_off`** — снять цепочку
(`:3289-3300`). На OpenWrt состояние не персистентно (полный `stop` его снимает,
`router_files/sing-box.initd:1021-1024`). TUN: хранить флаг в settings и
генерировать `final: proxy` без списков.

### 4.6 sing-box: сервис и конфиг

**`singbox_config`** — GET → сырой `config.json` (не обёрнут); POST B = JSON
конфига → **сначала запись** в `config.json`, потом `sing-box check` → `{"ok":true}`
или `{"ok":false,"error":"<вывод check>"}` — невалидный конфиг при этом
**остаётся на диске** (`router_files/detour-api:3063-3076`). Рестарта нет.

**`singbox_start`** (`$SVC_START`, sleep 2), **`singbox_stop`** (sleep 1, полный
teardown файрвола → трафик напрямую), **`singbox_restart`** (restart, sleep 3),
**`singbox_enable`** / **`singbox_disable`** (init enable/disable + флаг
`/etc/detour/autostart.singbox`) — все R `{"ok":true}`, метод не проверяется
(`router_files/detour-api:3078-3082`). Результат не проверяется — панель видит
реальное состояние через `status`.

**`bins_update_check`** — POST → синхронно `detour-update bins-check` → R
содержимое `detour-bins.json` (§1.8) или `{"status":"error","message":"проверка не удалась: …"}`
(`router_files/detour-api:2821-2832`). **`bins_update_apply`** — POST, **async**:
`start_apply "detour-update bins-apply"` → R `{"ok":true,"status":"started"}`,
прогресс — `apply_log` (`:2834-2840`). **`bins_update_status`** — GET → state-файл
или `{"status":"unknown","message":"no bins state yet"}` (`:2813-2819`).
Legacy-алиас `singbox_opkg_upgrade` = `bins_update_apply` (`:3051-3061`).
TUN: вместо opkg — скачать `sing-box-<v>-windows-amd64.zip` с GitHub.

`tpws_update_*`, `nfqws2_update_*` — идентично, state-файлы
`detour-tpws.json`/`detour-nfqws2.json`, команды `tpws-check|apply`,
`nfqws2-check|apply` (`router_files/detour-api:2845-2901`). См. §6.

### 4.7 Health, ping, geo, трафик

**`health_status`** — GET (`router_files/detour-api:2515-2545`). R
```jsonc
{"now":epoch,"supported":bool,            // бинарник есть и нет маркера unsupported
 "enabled":bool,"auto_switch":bool,"speed":bool,"speed_bytes":8000000,
 "urls":[{"label":"YouTube","url":"https://…"}],
 "switch": null | {…JSON /tmp/detour-health.switch…},
 "results":{"<id>":{"ok":bool,"ts":epoch,"rtt":ms|-1,"dl":kbit/s|-1,"delays":[ms|-1,…]}}}
```

**`health_check`** — POST. Без `id` → **async**: детач `detour-health sweep`,
R `{"ok":true,"started":true}` — панель опрашивает `health_status`. С Q `id=<p>` →
синхронно `detour-health one <p>` (5-10 с+, со скоростью дольше), R
`{"ok":true,"id":"<p>","result":{"ok":bool,"ts":…,"rtt":…,"dl":…,"delays":[…]}}`
(`router_files/detour-api:2550-2586`).

**`health_config`** — POST B `{"enabled"?:0|1,"auto_switch"?:0|1,"speed"?:0|1,"speed_bytes"?:N}`
(цифры можно в кавычках) → `write_settings` → R
`{"ok":true,"enabled":bool,"auto_switch":bool,"speed":bool,"speed_bytes":N}`;
рестарта нет (`router_files/detour-api:2625-2656`).

**`health_urls`** — GET → `{"list":"<текст>"}`; POST B текст → запись как есть
(`router_files/detour-api:2591-2602`).

**`autocheck_status`** — GET → `{"enabled":bool}` (крон `detour-update check-all`
раз в 6 ч установлен?) (`router_files/detour-api:2738-2745`,
`router_files/detour-update:1394-1417`). **`autocheck_set`** — POST B
`{"enabled":true|false}` (строго булево литералом) → R `{"enabled":bool}`
(`:2747-2762`).

**`ping_check`** — метод не проверяется, Q `id=<p>` (`router_files/detour-api:2453-2505`):
сервер = `server` профиля, у WG — первый `"address"` в файле; ICMP `ping -c1 -W2`
(с одним повтором); не ответил и тип не `wireguard|hysteria2|tuic` →
TCP-handshake к `server:server_port` (luasocket, таймаут 3 с). Upsert строки в
`ping.db`. R `{"id","rtt":ms|-1,"ok":bool,"ts":epoch,"server":"host","method":"icmp"|"tcp"|"none"}`.
⚠ Ищется первый `"address"` в файле — у WG в endpoint-формате это может быть
`outbound.address` (адрес интерфейса), а не адрес пира (не проверено; зависит от
порядка ключей cjson).

**`ping_status`** — GET → `{"now":epoch,"results":{"<id>":{"rtt":N,"ok":bool,"ts":N,"server":"…"}}}`
(`router_files/detour-api:2438-2451`).

**`geo_status`** — GET → `{"supported":true,"updated":epoch,"count":N,"known":N,"url":"…","error":"…","attribution":"IP Geolocation by DB-IP","attribution_url":"https://db-ip.com"}`
(`router_files/detour-api:3888-3891`, `router_files/detour-geo:101-108`).
Ссылка на db-ip.com обязательна (CC-BY).

**`geo_scan`** — POST B `{"force":true}` или пусто → **синхронно** `detour-geo scan
[--force]` (при новой базе — скачивание ~11 МБ) → R тот же объект статуса без
`supported`, или `{"error":"scan already running"}` (lock 30 мин)
(`router_files/detour-api:3893-3902`, `router_files/detour-geo:170-181`, `:300-310`).

**`traffic_counters`** — GET → вывод `detour-meter read` (`router_files/detour-api:3220-3231`):
```jsonc
{"ok":true,"supported":true,"warming":bool,"exact":true,
 "direct":%,"vpn":%,"bypass":%,"total":100,
 "bytes":{"direct":B,"vpn":B,"bypass":B,"total":B,"rx":B,"tx":B},
 "span":seconds,"wan":"eth0"}
```
Нет хелпера → `{"ok":false,"supported":false,"error":"…"}`. ⚠ `read` деструктивен:
отдаёт дельту с прошлого чтения и сдвигает базу (`router_files/detour-meter:225-297`),
первый вызов/сброс счётчиков → `warming:true` с нулями. TUN: считать по
статистике clash_api (`/connections` — байты по outbound-тегам `proxy`/`direct`)
или по счётчикам интерфейса TUN; `bypass` = трафик DPI-обхода (на Windows — 0 или winws).
Смежный `traffic_series?range=minute|hour` — отдаётся `detour-trafficlog series` (`:3237-3253`).

### 4.8 WARP, экспорт/импорт

**`warp_status`** — GET → вывод `detour-warp status` (`router_files/detour-warp:402-448`):
`{"ok":true,"profiles":[{"id","name","created":epoch,"account_type","address":"v4/32","reserved":bool,"chain_id"?,"chain_name"?}],"last":{…state §1.8…}}`.
⚠ Поля `supported` в ответе хелпера **нет** — только в фолбэке без бинарника
(`{"ok":true,"supported":false,"profiles":[],"last":{}}`, `router_files/detour-api:4574-4580`).

**`warp_register`** — POST B `{"via":"<профиль|CSV цепочки>"}` (пусто = активная);
символы только `[a-zA-Z0-9._,-]` → **async**: детач `detour-warp register`,
R `{"ok":true,"started":true}`; прогресс — `warp_status.last.state`
(`running`→`done|error`) (`router_files/detour-api:4582-4594`). Алгоритм
(`router_files/detour-warp:183-262`): `sing-box generate wg-keypair` →
временный sing-box через `render-mixed <via> 19482` → до 3 POST на
`https://api.cloudflareclient.com/v0a2158/reg` через `socks5h://127.0.0.1:19482`
(заголовки `CF-Client-Version: a-6.30-3596`, `User-Agent: okhttp/3.12.1`, тело
`{"key":<pub>,"install_id":"","fcm_token":"","tos":<ISO>,"model":"PC","serial_number":"","locale":"en_US"}`)
→ профиль `warp_<последний хоп>[N]` (группа `WARP`, endpoint-формат, peer
`162.159.192.1:2408`, `mtu 1280`, `reserved` = первые 3 байта base64-декода
`config.client_id`, `persistent_keepalive_interval 25`) → цепочка
`<последний хоп>_warp[N]` = `via,warp_id` через CLI `chain-save` → verify.
Lock `/tmp/detour-warp.lock` (повторный вызов → `регистрация уже идёт`).

**`warp_verify`** — POST B `{"id":"<профиль|цепочка>"}` → **async**, R
`{"ok":true,"started":true}`; verify поднимает цепочку и читает
`https://www.cloudflare.com/cdn-cgi/trace` (`warp=on` → успех)
(`router_files/detour-api:4596-4604`, `router_files/detour-warp:351-399`).

**`warp_delete`** — POST B `{"id":""}` → синхронно: не WARP / в активной цепочке /
в сохранённой цепочке → ошибка; иначе удалить файл → `{"ok":true}`
(`router_files/detour-api:4606-4612`, `router_files/detour-warp:451-477`).

**`panel_export_config`** — GET (`router_files/detour-api:4249-4272`) → 
`{"version":1,"exported_at":"ISO","router_version":"…","settings":{…settings.json…},"subscription":{…legacy subscription.json…},"proxy_domains":"…","whitelist_domains":"…","zapret_conf":"…","zapret_domains":"…"}`.
Не входят: профили (есть `profiles_export`), подписки v2, route-map, chains,
udp-vpn.list, health-urls, exclude/allow-списки, ru-subnets-exclude.

**`panel_import_config`** — POST B = такой же конверт (`router_files/detour-api:4274-4362`).
Ключи-запреты верхнего уровня (`auth, password, passwd, panel_user,
panel_password, update_conf, gh_token, gh_owner, gh_repo`) → отказ
`forbidden key in config: <k> …`; невалидный JSON → `config is not a valid JSON object`.
Пишет атомарно: `settings` (объект), `subscription` (непустой объект),
`proxy_domains`/`whitelist_domains`/`zapret_conf`/`zapret_domains` (непустые
строки). Затем **restart** sing-box в фоне → `{"ok":true}`.

### 4.9 Что ожидает панель (по каждому action)

Все пути — `panel/src/…`. «tolerant» = `requestJsonTolerant` (ошибка → `null` →
«недоступно»), иначе `requestJson` (ошибка → тост). JSON-тело всегда уходит как
`text/plain`.

| action | обёртка → вызовы | метод, Q, B, таймаут | что читается / тип |
|---|---|---|---|
| `check_auth` | `api/auth.ts:10` → `stores/session.ts:29` | GET | `{ok, user?}`; 401 → `panel_setup_status` (`session.ts:31-45`) |
| `status` | `api/overview.ts:18` → `stores/status.ts:78` | GET, 30 с, поллинг 8 с при видимой вкладке (`status.ts:100-113`) | §5 |
| `settings` | `api/rules.ts:72`, `:75` → `views/RulesView.vue:267-290` | GET; POST JSON **с одним ключом** `{routing_mode}` или `{singbox_mode}`, 180 с | GET `{routing_mode?, singbox_mode?}`, при ошибке — из `status.singbox`; после POST — `status` + `self_intercept` |
| `profiles_list` | `api/profiles.ts:43` → `stores/profiles.ts:85` (много вызывающих) | GET, 45 с | `ProfilesListResponse` (`api/types.ts:207-211`): `profiles[]`, `active?`, `active_chain?[]`; `ProfileSummary` (`types.ts:144-164`) — `id, type, name` (**`name` обязателен**: `localeCompare`/`toLowerCase`, `stores/profiles.ts:62-65`, `App.vue:93`), `group?, routing_mode?, autoswitch?, speedcheck?, torrents?, cc?`. `autoswitch/speedcheck === false` → исключён, `undefined` → участвует; `torrents === true` → разрешены (`components/profiles/ProfileList.vue:591`, `:607`, `:628`) |
| `profile_get` | `api/profiles.ts:46` → `ProfilesView.vue:161`, `:312`, `:461` | GET `?name=` | **сырой файл профиля без конверта**; читаемые поля — `components/profiles/uri.ts:326-384` (плоский) и `:713-745` (WG endpoint) |
| `profile_save` | `api/profiles.ts:49` → `ProfilesView.vue:332`, `:614`, `:461` | POST JSON, 20 с | `{ok}`; тело формы `{id,name,type,group,uri,routing_mode,outbound}` (`uri.ts:307-322`); смена папки = `{...profile_get, id, group}` (`api/profiles.ts:134-152`) — **полная перезапись**, не слияние |
| `profile_delete` | `api/profiles.ts:54` → `ProfilesView.vue:354`, `:405` (массово, последовательно) | POST `?name=` без тела | `{ok}` / текст отказа показывается |
| `profile_activate` | `api/profiles.ts:60` → `stores/profiles.ts:113` | POST `?name=`, 90 с | `{ok}`; затем стор сам ставит `active=id`, `activeChain=[id]` и зовёт `status` |
| `profiles_export` | `api/profiles.ts:66` → `ProfilesView.vue:579` | GET | объект сохраняется файлом; импорт (`:602-605`) принимает массив или `{profiles:[…]}` и шлёт `profile_save` поштучно |
| `chains_list` | `api/profiles.ts:111` → `stores/profiles.ts:106` | GET | `{chains:[{id,name,hops[]}]}`, `hops` обязателен (`RulesView.vue:637`, `components/profiles/ChainsPanel.vue:50`) |
| `chain_save` | `api/profiles.ts:113` → `ChainsPanel.vue:77` | POST JSON `{id,name,hops}` (новый id = slugify(name)), 90 с | `{ok}` |
| `chain_delete` | `api/profiles.ts:118` → `ChainsPanel.vue:110` | POST JSON `{id}` | `{ok}` / `error` |
| `chain_activate` | `api/profiles.ts:120` → `stores/profiles.ts:124` | POST **сырой текст** `hop1,hop2` (id хопов, не цепочки), 90 с | `{ok}`; стор ставит `activeChain=ids` |
| `subscription_save_one` | `api/profiles.ts:144` → `SubscriptionsPanel.vue:211` | POST JSON (запись + `id,url,group,title,user_agent,user_agent_preset,interval_hours,autoupdate,apply_routing`) | `{ok}`; незнакомые поля (`last_*`) сохранять; клиентская валидация `:197-208` |
| `subscription_delete_one` | `api/profiles.ts:147` → `SubscriptionsPanel.vue:288` | POST `?id=` | `{ok}` |
| `route_map` | `api/rules.ts:40`, `:42` → `RulesView.vue:142-143` | GET; POST сырой текст, 180 с | `{routemap}`; формат — `components/rules/RouteMapEditor.vue:2-7`, `:85-99` (секции через пустую строку, строки вне секций теряются); счётчик — regex `^\s*//\s*===\s*route:` (`RulesView.vue:560`) |
| `domains` | `api/rules.ts:10`, `:12` | GET 45 с; POST сырой текст 60 с («сохранить без применения», `RulesView.vue:111-112`) | `{domains}` / `{ok}` |
| `domains_save_restart` | `api/rules.ts:14` → `RulesView.vue:113` | POST сырой текст, 180 с | `{ok}`, затем `status` |
| `whitelist`, `whitelist_save_restart` | `api/rules.ts:20-24` → `RulesView.vue:121-123` | как `domains` | `{whitelist}` |
| `rulist_status` | `api/rules.ts:81` (tolerant) → `RulesView.vue:380` | GET | `RulistStatus` (`types.ts:473-481`) + `source_label, excluded, error`; `enabled !== false` → вкл., `auto === true`; `null`/`supported:false` → «не установлено» (`RulesView.vue:1000`, `:1007`) |
| `rulist_set` | `api/rules.ts:88` → `RulesView.vue:393` | POST JSON с одним ключом `{source}\|{auto}\|{enabled}`, 120 с | `RulistStatus`, сливается с прежним (`supported` сохраняется, `:373-376`) |
| `rulist_update` | `api/rules.ts:83` → `RulesView.vue:407` | POST тело `""`, 300 с | `RulistStatus`; читаются `count`, `error` |
| `rulist_exclude` | `api/rules.ts:89`, `:91` → `RulesView.vue:173-174` | GET; POST сырой текст 120 с | `{exclude}` / `{ok}`, затем `rulist_status` |
| `egress_blocklist` | `api/rules.ts:62`, `:64` → `RulesView.vue:164-165` | GET; POST сырой текст 60 с | `{list}` / `{ok}` |
| `singbox_config` | `api/diag.ts:16`, `:18` → `views/JournalView.vue:218-252` | GET 45 с; POST сырой текст 60 с | GET — **сырой объект** (`JSON.stringify(…,2)`); ошибка POST выводится целиком |
| `singbox_start/stop/restart/enable/disable` | `api/diag.ts:22-26` → `OverviewView.vue:320-341`, `JournalView.vue:273-277`, `ProfilesView.vue:205` | POST без тела; 90/60/180/20/20 с | `{ok}`; тумблер автозапуска = `status.singbox.enabled === true` |
| `allvpn_on/off` | `api/overview.ts:65`, `:70` → `OverviewView.vue:279-280` | POST, 120 с | `{ok}`; состояние = `status.singbox.allvpn === true` |
| `health_check` | `api/diag.ts:83`, `:90` | с id: POST `?id=`, 90 с → `{ok,id,result?:{ok,ts,rtt,dl,delays[]}}`, вердикт — `result.ok` (`ProfilesView.vue:238-247`); без id: POST → `{ok,started}` | **отдельного опроса нет** — результаты видны при следующем `health_status` |
| `health_config` | `api/diag.ts:100` → `JournalView.vue:752` | POST JSON, фактически один ключ: `{enabled:0\|1}\|{auto_switch:0\|1}\|{speed:0\|1}\|{speed_bytes:N}` (`api/diag.ts:219-233`) | `{ok}`, затем `health_status` |
| `health_status` | `api/diag.ts:65` → `stores/profiles.ts:96`, `JournalView.vue:738` | GET, 45 с | `HealthStatusResponse` (`types.ts:244-255`); `urls[i].label` — подписи к `delays[i]` (`ProfilesView.vue:115-123`); `switch` — плашка (`OverviewView.vue:233-242`); `enabled/auto_switch/speed === true`; `speed_bytes` сравнивается строкой с пресетами 8e6/3e7/7.5e7/1.5e8 (`JournalView.vue:779-794`) |
| `health_urls` | `api/diag.ts:91`, `:93` → `JournalView.vue:813`, `:826` | GET; POST сырой текст | `{list}` / `{ok}` |
| `autocheck_status` | `api/diag.ts:135` → `JournalView.vue:486` | GET | `{enabled: boolean}` (`=== true`) |
| `autocheck_set` | `api/diag.ts:137` → `JournalView.vue:691` | POST JSON `{enabled: true\|false}` — **булево литералом** (бэкенд ищет подстроку `"enabled":true`) | `{ok}` не требуется (сейчас бэкенд отдаёт `{enabled}` без `ok` — это норма) |
| `autoswitch_set`, `speedcheck_set` | `api/profiles.ts:70`, `:76` → `ProfilesView.vue:269-271`, `:373-375` | POST `?eligible=0\|1`, тело — id через `\n` | `{ok, excluded}` (`excluded` не читается) |
| `torrent_set` | `api/profiles.ts:88` → `ProfilesView.vue:270`, `:374` | POST `?allow=0\|1`, тело — id через `\n` | `{ok, allowed}` |
| `torrent_status` | `api/profiles.ts:94` (tolerant) → `OverviewView.vue:434` | GET, раз в 30 с | только `last_event.{ts, profile_name, clients}`; первый вызов запоминает `ts`, тост — на более новом `ts` (`:433-448`) |
| `ping_check` | `api/diag.ts:59` → `ProfilesView.vue:218` | **GET** `?id=`, 30 с | `{id, rtt?, ok?, ts?, server?, method?}`; читаются `ok`, `rtt` |
| `ping_status` | `api/diag.ts:57` → `stores/profiles.ts:96` | GET | `{now, results}`; `rtt > 400` → «медленный» (`stores/profiles.ts:41-49`). Своего планировщика пингов в новой панели нет — только при заходе на экран; периодика на стороне фонового detour-ping |
| `geo_scan` | `api/profiles.ts:104` → `ProfilesView.vue:296` | POST тело `""` (force не передаётся), 300 с | читаются `known`, `error`; затем `profiles_list` |
| `geo_status` | `api/profiles.ts:97` | — | **панель не вызывает** |
| `traffic_counters` | `api/overview.ts:41` (tolerant) → `OverviewView.vue:382`, раз в 10 с | GET | `direct/vpn/bypass` — **проценты 0..100** (`components/FlowBoard.vue:38-39`), `exact`; `supported:false` → прочерки; `warming:true` → без долей; `bytes.{total,rx,tx}` / `span` → скорость (`OverviewView.vue:391-409`). Читатель должен быть один (`read` деструктивен) |
| `updates_overview` | `api/diag.ts:118` (tolerant) → `stores/updates.ts:91` | GET (кэш 60 с на Обзоре, принудительно в Журнале) | `{panel?,singbox?,tpws?,nfqws2?: UpdateChannelState}` (`types.ts:284-309`); читаются `current_version\|current`, `available_version\|available`, `update_available`, `last_check` (затем `checked`), `changelog_b64`, `upstream_newer`, `upstream`, `error` (`components/journal/UpdateRow.vue:25-64`) |
| `bins/tpws/nfqws2_update_check` | `api/diag.ts:141`, `:146`, `:151` → `JournalView.vue:418-420` | POST, 120 с | сырой state-файл → `normalizeChannel` (`stores/updates.ts:41-58`): нет `update_available` → `cur && avail && cur !== avail`; `status:"error"` → `error = message` |
| `bins/tpws/nfqws2_update_apply` | `api/diag.ts:143`, `:148`, `:153` → `JournalView.vue:424-426` | POST | `{ok}` (пустой/оборванный ответ допустим, `:634-636`), затем опрос `apply_log` |
| `*_update_status` | `api/diag.ts:139`, `:144`, `:149` | — | **панель не вызывает** |
| `apply_log` | `api/diag.ts:54` (tolerant) → `JournalView.vue:567` | GET, раз в 2 с до 420 с (Keenetic-панель — 150 с, `:574`) | конец — `done === true`; успех — `Number(String(rc)) === 0`; `log` целиком заменяет текст |
| `log_config` | GET прямой `requestJson` в `JournalView.vue:167`; POST `api/diag.ts:47` | POST JSON всегда с обоими ключами `{enabled:0\|1, singbox:0\|1}` (`JournalView.vue:182`) | GET `{enabled, singbox}` (`=== true`); ошибка GET → переключатели скрыты (`:171-174`, `:1021`) |
| `logs_clear` | `api/diag.ts:45` → `JournalView.vue:134` | POST `?name=` | `{ok}` |
| `zapret_config` | `api/diag.ts:34`, `:36` → `JournalView.vue:329`, `:342` | GET; POST сырой текст (trim) | `{args}` / `{ok}` |
| `zapret_start/stop/restart/enable/disable` | `api/diag.ts:29-33` → `JournalView.vue:280-284` | POST; 90/60/120/20/20 с | `{ok}` |
| `zapret_domains`, `zapret_domains_save_restart` | `api/rules.ts:30-34` → `RulesView.vue:132-134` | GET; POST сырой текст (20 с / 180 с) | `{domains}` / `{ok}` |
| `bypass_status` | `api/overview.ts:75` (tolerant) → `stores/status.ts:93`, поллинг 8 с | GET | `BypassStatus` (`types.ts:257-276`): `mode`, `autostart` (0\|1\|bool), `running` (**строка**), `zapret2_supported`, `qnum`, `queued`, `strategy`; `platform` не читается |
| `bypass_set` | `api/overview.ts:77` → `components/overview/BypassTile.vue:86-99` | POST `?mode=`, 120 с | `{ok, mode}` (старт/рестарт = тот же set) |
| `bypass_stop` | `api/overview.ts:83` → `BypassTile.vue:103` | POST, 120 с | `{ok}` |
| `bypass_autostart` | `api/overview.ts:88` → `BypassTile.vue:109` | POST `?on=1\|0` | `{ok}` |
| `bypass_strategy` | `api/overview.ts:93`, `:95` → `BypassTile.vue:119-135` | GET `requestRawText` — **голая строка**, может быть пустой (если завернуть в JSON, в редактор уедет JSON-текст); POST сырая строка, 120 с (клиент требует `--lua-desync=`) | `{ok}` |
| `udp_vpn` | `api/overview.ts:100`, `:102` → `stores/status.ts:94` (поллинг 8 с), `OverviewView.vue:293` | GET (не tolerant!); POST JSON `{mode}`, 120 с | `{mode, supported, list?}`; при ошибке GET `udpVpnSupported` становится `true` (`status.ts:42`) — для отключения нужен явный `{mode:"off", supported:false}` |
| `udp_vpn_list` | `api/overview.ts:106`, `:108` → `RulesView.vue:154-155` | GET; POST сырой текст, 120 с | `{list}` / `{ok}` |
| `warp_status` | `api/profiles.ts:166` (tolerant) → `components/profiles/WarpPanel.vue:53` | GET; опрос раз в 3 с до 300 с (register) / 240 с (verify) | `{ok, supported, profiles?:[{id,name?,verified?,address,account_type,chain_name}], last?:{state?,error?,ts?,message?}}`; конец — `last.state ∈ {done,error}`; `null`/`supported:false` → вкладка WARP скрыта (`ProfilesView.vue:88`, `WarpPanel.vue:41`, `:54`). Rust: отдавать `supported:true` явно |
| `warp_register` | `api/profiles.ts:169` → `WarpPanel.vue:65` | POST JSON `{via}` | `{ok, started}` |
| `warp_verify` | `api/profiles.ts:173` → `WarpPanel.vue:98` | POST JSON `{id}` | `{ok, started}` |
| `warp_delete` | `api/profiles.ts:177` → `WarpPanel.vue:128` | POST JSON `{id}`, 45 с | `{ok}` |
| `panel_export_config` | `api/services.ts:100` → `views/ServicesView.vue:646` | GET | объект сохраняется файлом |
| `panel_import_config` | `api/services.ts:102` → `ServicesView.vue:729` | POST JSON (весь файл), 120 с | `{ok}`; превью по ключам `settings, subscription, proxy_domains, whitelist_domains, zapret_conf, zapret_domains` (`:669-686`) |
| `secure_dns_set` | `api/rules.ts:121` → `RulesView.vue:510` | POST JSON `{mode:"secure"\|"auto", list}`, 60 с | `{ok}`; блок виден, только если `hosts_status.secure_dns_mode` непуст (`RulesView.vue:619-622`, `:1146`) |

### 4.10 Прочие action, которые вызывает панель (вне списка)

| action | вызовы | раздел UI | Windows |
|---|---|---|---|
| `login`, `logout`, `panel_setup_status`, `panel_first_setup` | `api/auth.ts:8-25`, `stores/session.ts:40-88` | вход/первичная настройка | нужны |
| `panel_change_password` | `api/auth.ts:33` → `components/services/PasswordSheet.vue:58` | Сервисы → Вход | нужен |
| `subscriptions_list`, `subscription_fetch`, `subscription_refresh_one/_all` | `api/profiles.ts:129-160` → `SubscriptionsPanel.vue:113`, `:168`, `:231-272` | Профили → Подписки | нужны (§3) |
| `self_intercept` | `api/rules.ts:54`, `:57` → `RulesView.vue:325`, `:353` | Правила → перехват | скрыть (отдать `{targets:[],full_targets:[],eligible:[],mode:"single"}`) |
| `logs_view` | `api/diag.ts:41` → `JournalView.vue:118` (раз в 5 с при «авто») | Журнал → Журналы | нужен |
| `traffic_series` | `api/overview.ts:62` → `components/overview/TrafficChart.vue:143` (раз в 60 с, без проверки видимости) | Обзор → Трафик: `{ok,supported?,step?,points:[{ts,direct,vpn,bypass,rx,tx}]}` | нужен (свой сборщик) |
| `lan_clients` | `api/overview.ts:21` → `OverviewView.vue:412`, `ServicesView.vue:217` | «Устройства в сети», форма проброса | скрыть |
| `iptables`, `conntrack` | `api/overview.ts:24`, `:35` → `JournalView.vue:384`, `:395` | Журнал → Файрвол | скрыть |
| `keepalive_status/check` | `api/diag.ts:110`, `:112` → `JournalView.vue:855`, `:864` | Журнал → Проверка | можно оставить (TCP/ICMP-проба активного сервера) |
| `panel_update_check/apply/status/sig/local` | `api/diag.ts:119-131` → `JournalView.vue:417-668`, `OverviewView.vue:353` | обновления панели | скрыть или заменить своим апдейтером |
| `hosts_*` (9 шт.) | `api/rules.ts:97-117` → `RulesView.vue` | Правила → hosts | скрыть (или реализовать через sing-box `dns` hosts) |
| `portmap_*` (8 шт.) | `api/services.ts:16-32` | Сервисы → проброс | скрыть |
| `cert_*` (7 шт.) | `api/services.ts:37-60` | Сервисы → сертификат | скрыть |
| `push_config/subscribe/unsubscribe/test`, `push_message` (SW) | `api/services.ts:65-71`, `panel/src/sw.ts:38-98` | Сервисы → уведомления | скрыть либо оставить (Web Push работает и с Windows, но нужен HTTPS) |
| `offload_*`, `swap_*` | `api/services.ts:77-97` | Сервисы | отдавать `{supported:false}` — панель скроет сама |
| `wanpin_status/set` | `api/services.ts:84-88` → `components/overview/UplinksTile.vue:39-84` | Обзор → Каналы | не вызывается при <2 `wan_link.uplinks` |

---

## 5. Как панель использует `status`, и гейтинг платформы

### 5.1 Поля `status`, которые читает панель

Тип `StatusResponse` — `panel/src/api/types.ts:133-142`
(`{platform, panel_port?, version?, binaries, singbox, zapret, system, wan_link?}`);
в коммите `Platform = "openwrt"|"keenetic"` (`types.ts:4`); в рабочем дереве уже
`"openwrt"|"keenetic"|"windows"|"macos"|"android"`, а `App.vue` получил карту
`PLATFORM_LABEL` (не закоммичено).

- **`platform`**: `stores/status.ts:19` (неизвестное/undefined → `"openwrt"`),
  `status.ts:20` (`isKeenetic`), `App.vue:32` (подзаголовок «Keenetic»/«OpenWrt»).
- **`version`**: сверка с `__PANEL_BUILD__` → reload (`stores/status.ts:62-73`, `:81`);
  показ — `OverviewView.vue:545`, `:808`, `JournalView.vue:456`, `:565`, `:1186`.
- **`panel_port`**: `ServicesView.vue:1222`, `:1231` (подсказки сертификата/Keenetic).
- **`binaries`**: `singbox_version` (`App.vue:33`, `OverviewView.vue:812`,
  `JournalView.vue:1196`); `tpws_version` (`OverviewView.vue:816`, `BypassTile.vue:173`,
  `JournalView.vue:1207`); `nfqws2_version` (`BypassTile.vue:170`, `JournalView.vue:1218`);
  `nfqws2_supported` (`stores/status.ts:33`, `:39`, `JournalView.vue:460`).
- **`singbox`**: `running === true` (`status.ts:51`, `JournalView.vue:312`, `:1068`);
  `active_profile`, `active_chain` (`status.ts:52-53`); `pid`, `port`, `external_ip`,
  `active_type` (`OverviewView.vue:605`, `:620`, `:625-626`); `enabled`
  (`OverviewView.vue:337`, `JournalView.vue:1071`, `:1094`); `allvpn`
  (`OverviewView.vue:154`, `:272`); `domains` (`OverviewView.vue:156`, `:160`,
  `components/overview/RoutingTile.vue:40`, `RulesView.vue:540`); `ips`
  (`RoutingTile.vue:40`); `ipset_count` (`OverviewView.vue:604`, `RoutingTile.vue:50`);
  `routing_mode` (`OverviewView.vue:151`, `RoutingTile.vue:18`, `RulesView.vue:272`);
  `singbox_mode` (`RoutingTile.vue:27`, `RulesView.vue:273`); `route_targets`
  (`RoutingTile.vue:28`, `RulesView.vue:561`).
- **`zapret`**: `running` (`JournalView.vue:313`, `:1113`), `domains`
  (`OverviewView.vue:603`, `BypassTile.vue:179`, `RulesView.vue:555`), `ipset_count`
  (`BypassTile.vue:180`), `port` (`BypassTile.vue:185`, `JournalView.vue:1115`),
  `enabled` (`JournalView.vue:1116`, `:1139`).
- **`system`**: `uptime` (`OverviewView.vue:79`, `:623`), `cpu` (строка, `"?"` → «—»,
  `:86`), `cpu_cores` (`:775`), `memory` (`:777`), `disk_free` (`:778`), `mptcp`
  (`:98-104`, `:560` — числовое ненулевое значение включает красный баннер MPTCP).
- **`wan_link`**: `supported`, `speed_mbps`, `duplex` (`OverviewView.vue:143-146`, `:783`);
  `degraded`, `diagnosis`, `advice` (`:117-130`); `uplinks[]` (`:120`, `:146`;
  `UplinksTile.vue:23-133`: `id, label, iface, ip, up, active, route_dead, carrier,
  degraded, diagnosis, speed_mbps, weight`).

### 5.2 Что обязательно, чтобы панель не упала

Все обращения идут через `?.` (прямые — только под `v-if`, `BypassTile.vue:171`, `:174`),
поэтому отсутствие полей панель переживает. Обязательно:
1. тело — JSON-объект без `ok:false` (иначе красная плашка `App.vue:180`,
   подзаголовок «Соединяюсь с роутером…», `App.vue:30`);
2. `singbox.active_chain`, если есть, — **массив** (`chain.length > 1` → `chain.join`,
   `OverviewView.vue:64-65`);
3. `wan_link.uplinks`, если есть, — массив;
4. `version` == VERSION сборки панели;
5. `singbox.running/enabled/allvpn` — настоящие JSON-булевы (`=== true`);
6. `system.mptcp` на Windows не отдавать или `"unknown"`;
7. `binaries.nfqws2_supported: false` явно, если zapret2 нет — иначе подсказка
   «пакет nfqws2 не установлен» (`stores/status.ts:36-41`, `BypassTile.vue:188-193`) и
   видимая строка обновления nfqws2 (`JournalView.vue:459-461`, `:1215`);
8. `name` у каждого элемента `profiles_list`.

Рекомендованный ответ `status` на Windows: `platform:"windows"`, реальные
`version`/`binaries.singbox_version`, `singbox.{running,pid(строка),port,enabled,
allvpn,domains,ips,entries,active_profile,active_type,external_ip*,active_chain[],
routing_mode,singbox_mode:"single",route_targets,self_intercept:[]}`,
`ipset_count`/`ipset_members` — число правил/пусто, `zapret` — состояние winws или
`running:false`, `system` без `mptcp`, `wan_link:{"supported":false}`.

### 5.3 Все места, где UI ветвится по платформе

`currentPlatform` в `panel/src` нет (это старая `index.html`); ветвление только через
`status.platform` / `status.isKeenetic`:
- `stores/status.ts:19-20` — `platform`, `isKeenetic`;
- `stores/status.ts:31-34` — `zapret2Supported` (без `bypass_status`:
  `!isKeenetic && nfqws2_supported !== false`); опция zapret2 дизейблится в
  `BypassTile.vue:161`;
- `stores/status.ts:36-41` — `nfqws2Missing`;
- `stores/status.ts:42` — `udpVpnSupported = udp.supported !== false && !isKeenetic`;
  дизейбл сегментов UDP — `OverviewView.vue:690-707`, `RulesView.vue:958`, палитра
  `RulesView.vue:691`;
- `App.vue:32` — подзаголовок платформы;
- `JournalView.vue:459-461` — `nfqws2Visible` (строка обновления `:1215`);
- `JournalView.vue:574` — таймаут `apply_log` для Keenetic;
- `ServicesView.vue:928` — подсказка про KeenDNS; `:937`, `:1006` — кнопка
  `KeenHttpsSheet`; `:1221` — проп `keenetic` у `CertSheet` (`CertSheet.vue:41`, `:211`);
- `components/overview/ServicesTile.vue:73` — подсказка Keenetic.

Автогейтинг по ответам бэкенда (без платформы): WARP (`ProfilesView.vue:88`),
offload (`ServicesView.vue:1014`, `supported === true`), swap (`ServicesView.vue:1060`),
uplinks (`UplinksTile.vue:30`, `:137`, ≥2 каналов), rulist (`RulesView.vue:1000`, `:1007`),
hosts (`RulesView.vue:1067`, `:1075`), secure DNS (`RulesView.vue:1146`), мост
syslog (`JournalView.vue:1021`), health (`JournalView.vue:1249-1278`), проброс
(`ServicesView.vue:827`, `:893` — без `https_supported`/`dnat_supported` секция
остаётся, но «Добавить» задизейблена).

### 5.4 Роутерные разделы: где смонтированы и куда ставить гейт `isWindows`

Базовая правка: `isWindows` (или общий `isClient`/`isRouter`) рядом со
`stores/status.ts:20`; тип `Platform` в рабочем дереве уже расширен.

| Раздел | Где смонтирован | Куда ставить гейт |
|---|---|---|
| cert | `ServicesView.vue:904-943` (`svc-cert`), `CertSheet` `:1214`, `loadCert` в `loadAll` `:229`, команда `svc:cert` `:760-769`; плитка «Сервисы и доступ» `OverviewView.vue:766` → `ServicesTile.vue:23-33` (дёргает `cert_status`, `portmap_status`, `push_config` при монтировании) | `v-if` на ServicePanel, пропуск в `loadAll` (`:224-235`), `available` у команды; плитку — через DashSlot |
| portmap | `ServicesView.vue:813-901`, `PortmapSheet` `:1197`, `loadPortmap` `:228`, команды `svc:portmap-add/apply` `:746-759`, `ServicesTile` | то же |
| push | `ServicesView.vue:945-1011`, `loadPush` `:230`, команда `svc:push-test` `:771-777`, `ServicesTile`; `sw.ts` безвреден | `v-if` + пропуск `loadPush` |
| offload / swap | `ServicesView.vue:1013` / `:1059` — уже скрыты при `supported !== true` | бэкенд отдаёт `{supported:false}`; для экономии запросов пропустить `loadOffload`/`loadSwap` (`:231-232`) |
| wanpin | только `UplinksTile.vue` (DashSlot `uplinks`, `OverviewView.vue:756`); запрос только при ≥2 `wan_link.uplinks` | не отдавать `uplinks` или гейт в DashSlot |
| hosts (+`secure_dns_set`) | `RulesView.vue:1062-~1190` (`rule-hosts`), `loadHosts` в `onMounted` `:723`, команда `rules:hosts` `:710-716`, `onExpand` `:651` | `v-if` на секции, пропуск `loadHosts`, `available` |
| lan_clients | `OverviewView.vue:411-416` (`loadExtras`), кнопка `:788-792`, шторка `:854-870`, проп `clients` у FlowBoard `:599`; `ServicesView.vue:215-222` | пропуск вызовов + `v-if` на кнопке |
| iptables / conntrack | `JournalView.vue:1158-1175` (`area-firewall`), шторка `:1371-1420`, команда `jr:firewall` `:918-923` | `v-if` + `available` |
| keepalive | `JournalView.vue:1307-1315`, `loadKeepalive` `:973`, команда `jr:keepalive` `:961-966` | `v-if` + пропуск загрузки (или оставить — фича переносима) |
| panel_update_* | Журнал: UpdateRow «Панель» `JournalView.vue:1183-1192`, «Установить из файла» `:1235`, шторка `:1460-1494`, команды `jr:upd-*` `:925-949`; Обзор: `checkUpdates` `OverviewView.vue:350-366`, кнопка `:827`, команда `ov:updcheck` `:475-480`, баннер `:541-556` (по `updates_overview.panel`) | `v-if`; проще всего не отдавать `panel` в `updates_overview` |
| плитки Обзора централизованно | `components/overview/DashSlot.vue:59` (`v-if="dash.isVisible(id)"`), `stores/dashboard.ts:30-42` (`DASH_TILES`) | флаг «только роутер» для `services` и `uplinks`, фильтр в `tiles`/`visible` (`:113-120`) — скрытая плитка не монтируется и не шлёт запросов |

Тоже роутерное, но не из списка: подпись «Пакет берётся из нашего opkg-фида»
(`JournalView.vue:1199`); MPTCP-баннер (через `system.mptcp`); `singbox_enable/disable`
(`OverviewView.vue:642-651`, `JournalView.vue:1093-1106` — на Windows это автозапуск
службы, смысл сохраняется); сервис zapret целиком (`JournalView.vue:1110-1156`);
`egress_blocklist`, `rulist` (на Windows переносятся как правила sing-box);
`self_intercept`.

### 5.5 Страницы и их запросы

Роуты — `panel/src/router/index.ts:12-51`, `createWebHashHistory`; `?focus=` —
`panel/src/lib/deeplink.ts:36-61`.

- **Глобально** (`App.vue:112-123`): `check_auth` → (401 → `panel_setup_status`);
  после входа `status`, `bypass_status`, `udp_vpn`, `profiles_list`; поллинг раз в 8 с
  при видимой вкладке: `status`, `bypass_status`, `udp_vpn` (`stores/status.ts:100-113`).
- **`#/` Обзор** (`OverviewView.vue:452-504`): `profiles_list` (если не загружен),
  `ping_status` + `health_status`, `lan_clients`, `traffic_counters`, `torrent_status`,
  `updates_overview` (если старше 60 с); плитки: `traffic_series?range=minute|hour`,
  `ServicesTile` → `cert_status`/`portmap_status`/`push_config`, `UplinksTile` →
  `wanpin_status`; таймеры: `traffic_counters` 10 с, `torrent_status` 30 с (оба при
  видимой вкладке), `traffic_series` 60 с (без проверки видимости).
- **`#/profiles`**: вкладки монтируются одновременно (`v-show`) → `profiles_list`,
  `ping_status` + `health_status`, `chains_list`, `subscriptions_list`, `warp_status`;
  таймеров нет.
- **`#/rules`** (`RulesView.vue:719-726`): `settings`, `self_intercept`, `rulist_status`,
  `hosts_status`, `profiles_list`, `chains_list`; при раскрытии секции (`:643-652`) —
  `domains`/`whitelist`/`zapret_domains`/`route_map`/`udp_vpn_list`/`egress_blocklist`/
  `rulist_exclude`/`hosts_custom_get`; таймеров нет.
- **`#/services`** (`ServicesView.vue:224-235`): `portmap_status`, `cert_status` +
  `cert_detect`, `push_config`, `offload_status`, `swap_status`, `lan_clients`.
- **`#/journal`** (`JournalView.vue:969-973`): `logs_view?name=singbox`, `log_config`,
  `updates_overview` + `autocheck_status`, `health_status`, `keepalive_status`; по
  требованию `singbox_config`, `zapret_config`, `iptables`, `conntrack`, `health_urls`;
  таймеры: `logs_view` 5 с (при «авто»), `apply_log` 2 с после установки.

---

## 6. DPI-часть: bypass (nfqws2) и zapret (tpws)

### 6.0 Термины панели

| `bypass.mode` | Движок | Техника | Платформы |
|---|---|---|---|
| `off` | — | домены идут как настроено (напрямую/sing-box) | все |
| `zapret` | `tpws-zapret` | прозрачный TCP-прокси `:1081`, REDIRECT из nat PREROUTING | OpenWrt + Keenetic |
| `zapret2` | `nfqws2` | NFQUEUE, десинк пакетов на лету, соединение идёт **напрямую** | только OpenWrt |

Источник: `router_files/detour-bypass:2-10`. Режимы взаимоисключающие —
`apply_mode` сначала делает `stop_all` (`router_files/detour-bypass:245-253`, `:221-225`).
Оба движка используют **один** ipset `zapret_domains` и **один**
`/etc/zapret-tpws/domains.list` (`:9-10`, `:52`).

В панели два пульта над одним tpws: новый `bypass_*` (плитка «Обход DPI»,
`panel/src/components/overview/BypassTile.vue`) и старый `zapret_*` (служба
zapret-tpws в журнале, `panel/src/views/JournalView.vue:280-284`). Плитка берёт
состояние из `bypass_status`, а не из `status.zapret`: в режиме zapret2 служба
tpws штатно остановлена (`BypassTile.vue:6-8`).

### 6.1 zapret (tpws)

- Запуск: `/usr/bin/tpws-zapret --bind-addr=0.0.0.0 --port=1081 $TPWS_OPT`,
  procd `respawn` (`router_files/zapret-tpws.initd:194-199`); `0.0.0.0` обязателен —
  REDIRECT переписывает dst на IP br-lan (`:191-193`). START=98/STOP=11 (`:8-9`).
  Нет бинарника → `return 1` до установки правил (`:177-181`). Лог
  `/var/log/zapret-tpws.log` (`:17`). Keenetic добавляет `--user=nobody`
  (`keenetic/init.d/S53detour-zapret:39`).
- **`/etc/zapret-tpws.conf`** — читается только первая строка (`head -1`,
  `router_files/zapret-tpws.initd:185`), подставляется без кавычек (word-splitting,
  `:195`); пусто/нет → дефолт
  `--filter-tcp=80 --methodeol --new --filter-tcp=443 --split-pos=1,midsld --disorder`
  (`:186`, он же `router_files/zapret-tpws.conf:1`). Схемы нет — любые опции tpws;
  проверенные варианты — `releases/BYPASS_STRATEGIES.md:26-53`.
- **`domains.list`** (разбор `router_files/zapret-tpws.initd:40-51`,
  `router_files/detour-bypass:108-112`): формат как у `proxy-domains.list` (§1.5);
  IP/CIDR → `add zapret_domains <cidr>` в `ipset restore -exist`; домен →
  `ipset=/<d>/zapret_domains` в `/tmp/dnsmasq.d/zapret-domains.conf`; IPv6 и
  прочее игнорируются.
- Наполнение ipset (`router_files/zapret-tpws.initd:109-119`): `ipset create
  zapret_domains hash:net -exist` → dnsmasq-файл → `dnsmasq restart`, `sleep 1` →
  прогрев `nslookup <d> 127.0.0.1 &` (`:57-66`) → `sleep 1` →
  `flush_ipset_conntrack zapret_domains` (`:73-91`, `:142-147`).
- iptables (`router_files/zapret-tpws.initd:123-140`):
  ```
  -t nat -N ZAPRET_REDIRECT
    -d 10.0.0.0/8|172.16.0.0/12|192.168.0.0/16|127.0.0.0/8 -j RETURN
    -p tcp -m set --match-set zapret_domains dst -j REDIRECT --to-ports 1081
  -t nat -I PREROUTING 1 -i br-lan -j ZAPRET_REDIRECT   (+ vpn_redirect_ifaces)
  ```
  Перенаправляются **все** TCP-порты адресов набора (фильтрует сам tpws через
  `--filter-tcp`). sing-box вставляет свои правила **после** `ZAPRET_REDIRECT`
  (`router_files/sing-box.initd:892-895`) — zapret срабатывает первым. nft: accept
  :1081 в `input_lan` (`router_files/zapret-tpws.initd:93-99`). Keenetic — правило из
  ndm-хука (`keenetic/ndm/netfilter.d/50-detour.sh:200-204`).
- Остановка (`router_files/zapret-tpws.initd:150-171`): снять правила/nft,
  `conntrack -D -p tcp --reply-port-src 1081`, удалить dnsmasq-файл, restart
  dnsmasq, flush+destroy ipset.
- Reload (`:206-219`) — tpws не перезапускается: flush ipset → dnsmasq-файл →
  restart dnsmasq → прогрев → conntrack-flush. Вызывается при сохранении
  списка в обоих режимах (ipset общий, `:209-211`).

### 6.2 zapret2 (nfqws2)

| Файл | Содержимое | Ссылка |
|---|---|---|
| `/etc/detour/bypass.mode` | `off\|zapret\|zapret2` | `router_files/detour-bypass:49` |
| `/etc/detour/bypass.autostart` | `0\|1` | `:50` |
| `/etc/detour/nfqws2.strategy` | одна строка стратегии; нет → дефолт | `:51`, `:202` |
| `/var/run/nfqws2.pid`, `/var/log/nfqws2.log` | pid, лог | `:45-46` |
| `/usr/bin/nfqws2`, `/usr/share/detour/lua/{zapret-lib,zapret-antidpi,zapret-auto}.lua` | пакет `nfqws2` фида | `build_feed.py:121`, `:222-229` |

Встроенная стратегия **одна** (`router_files/detour-bypass:57`):
```
--filter-tcp=443 --filter-l7=tls --payload=tls_client_hello --lua-desync=tcpseg:pos=0,midsld:ip_id=rnd:repeats=2
```
Панель дублирует её как «По умолчанию» (`panel/src/components/overview/BypassTile.vue:24-25`).
Других пресетов в коде нет; альтернативы — `releases/BYPASS_STRATEGIES.md:93-99`
(`hostfakesplit:tcp_md5:repeats=6` работает; `fake…+multidisorder`, `multisplit` — нет).
Hostlist не используется: домены фильтрует ipset в iptables, внутри nfqws2 —
только `--filter-l7=tls`. QUIC (UDP:443) не обрабатывается (`BYPASS_STRATEGIES.md:108-111`).

Запуск (`router_files/detour-bypass:200-209`):
```
nfqws2 --daemon --qnum=200 --pidfile=/var/run/nfqws2.pid --fwmark=0x40000000 \
  --lua-init=@/usr/share/detour/lua/zapret-lib.lua \
  --lua-init=@/usr/share/detour/lua/zapret-antidpi.lua \
  --lua-init=@/usr/share/detour/lua/zapret-auto.lua  <strategy>
```
Lua — строго в этом порядке. Проверка запуска — поиск процесса по
`/proc/<pid>/exe` на `*nfqws2`; не поднялся → снять firewall и `die`
(`:232-243`, `:82-97`).

Firewall (`router_files/detour-bypass:149-181`; `ensure_zapret_domains` — та же
генерация ipset/dnsmasq, что в 6.1):
```
-t nat -N ZAPRET2_NAT   (RETURN для приватных сетей)
  -p tcp -m set --match-set zapret_domains dst -j ACCEPT      # мимо sing-box → напрямую
-t nat -I PREROUTING 1 -i br-lan|<vpn_redirect_ifaces> -j ZAPRET2_NAT
-t mangle -N ZAPRET2_MANGLE
  -p tcp --dport 443 -m set --match-set zapret_domains dst
  -m mark ! --mark 0x40000000/0x40000000
  -m connbytes --connbytes 1:20 --connbytes-dir original --connbytes-mode packets
  -j NFQUEUE --queue-num 200 --queue-bypass
-t mangle -A POSTROUTING -j ZAPRET2_MANGLE
```
Затем `sleep 1` и conntrack-flush; снятие — `:183-198`. Доступность (`:64-70`): не
Keenetic, бинарник исполняемый, `nfnetlink_queue` в lsmod (или modprobe).

Автостарт: init `detour-bypass` всегда enabled, START=99
(`router_files/detour-bypass.initd:11`, `:16-19`) → `detour-bypass boot` применяет
режим только при `autostart=1 && mode!=off` (`router_files/detour-bypass:281-291`).
`cmd_set` делает `zapret-tpws disable` — жизненным циклом tpws владеет
переключатель (`:266-268`). postinst применяет legacy-автостарт tpws, только
если `bypass.mode` не существует (`build_release.py:543-552`).

### 6.3 Actions DPI

Общее: тело — сырой поток; query-параметры читаются `sed 's/.*[?&]mode=…'`, т. е.
параметр должен идти **не первым** (панель всегда ставит `action` первым,
`panel/src/api/client.ts:117`).

**`bypass_status`** — GET (`router_files/detour-api:2992-2998` → `router_files/detour-bypass:293-306`):
```json
{"mode":"off|zapret|zapret2","autostart":0,"running":"zapret2|zapret|none",
 "zapret2_supported":true,"platform":"openwrt|keenetic","qnum":200,"queued":0,"strategy":"<строка>"}
```
`autostart` — **число** (`panel/src/api/types.ts:262`); `queued` — сумма счётчиков
NFQUEUE в `ZAPRET2_MANGLE`; из `strategy` вырезаются `"` и `\`. Нет бинарника →
`{"mode":"off","autostart":0,"running":"none","zapret2_supported":false,"platform":"unknown"}`.

**`bypass_set`** — POST, Q `mode=off|zapret|zapret2`, тела нет (`router_files/detour-api:3000-3011`).
Синхронно: `stop_all` → старт движка → запись `bypass.mode` → `zapret-tpws disable`;
`bypass.autostart` не трогает. R `{"ok":true,"mode":"<m>"}` или ошибка (tail -8
вывода). Панель использует его и для «Старт»/«Рестарт» (`BypassTile.vue:92-100`).

**`bypass_stop`** — POST → `detour-bypass stop` (`stop_all`), режим сохраняется →
`{"ok":true}` (`router_files/detour-api:3013-3020`).

**`bypass_autostart`** — POST, Q `on=1|0|on|off` → пишет только `bypass.autostart`,
ничего не применяет → `{"ok":true}` (`router_files/detour-api:3022-3029`).

**`bypass_strategy`** (`router_files/detour-api:3031-3049`): GET — **голая строка**
(первая строка файла, не JSON; нет файла → `\n`), панель читает `requestRawText`
(`panel/src/api/overview.ts:93`). POST — тело = одна строка (`head -1`, без `\r`),
обязателен `--lua-desync=`, иначе ошибка; запись; если режим `zapret2` —
синхронно `detour-bypass set zapret2` (перезапуск nfqws2 и firewall) → `{"ok":true}`.

**`zapret_start|stop|restart`** — `initd start|stop` (+sleep), restart = stop,
sleep 1, start; **всегда** `{"ok":true}`, результат не проверяется, метод не
проверяется (`router_files/detour-api:3135-3137`).

**`zapret_enable|disable`** — `initd enable|disable` + `/etc/detour/autostart.zapret`
= `1|0` → `{"ok":true}` (`router_files/detour-api:3138-3139`, `:257-263`). Legacy:
после первого `bypass_set` postinst `autostart.zapret` игнорирует
(`build_release.py:546`), но ручной `zapret_enable` вернёт rc.d-симлинк и tpws
поднимется при загрузке в обход detour-bypass. В порте двойственность убрать.

**`zapret_config`** — GET → `{"args":"<первая строка conf>"}`; POST → запись через
`echo` (+`\n`), без валидации и **без рестарта** → `{"ok":true}`
(`router_files/detour-api:3141-3150`). Панель тоже не перезапускает
(`JournalView.vue:339-351`).

**`zapret_domains`** — GET → `{"domains":"<файл>"}` / `{"domains":""}`; POST →
запись (+`\n`) **без применения** → `{"ok":true}` (`router_files/detour-api:3152-3165`).
`mkdir -p /etc/zapret-tpws` захардкожен (на Keenetic — не тот каталог; запись идёт
в правильный `$ZAPRET_DOMAINS`).

**`zapret_domains_save_restart`** — только POST: запись → `initd reload` (§6.1,
tpws не перезапускается) → `sleep 2` → `{"ok":true}` (`router_files/detour-api:3167-3177`).
Поведение procd-`reload` при остановленном сервисе не проверено.

**`tpws_update_status|check|apply`**, **`nfqws2_update_status|check|apply`** —
как `bins_update_*` (§4.6), state `/var/state/detour-tpws.json`,
`/var/state/detour-nfqws2.json` (`router_files/detour-update:74-75`):
```json
{"current":"<ver>","available":"<ver>","upstream":"<gh tag>","upstream_newer":false,
 "asset":"tpws-zapret|nfqws2","last_check":"YYYY-MM-DDTHH:MM:SSZ","changelog_b64":"…"}
```
check: `ensure_feed`; `cur` из пакетного менеджера (нет → `0.0.0`); `avail` —
`opkg update` + `list-upgradable` (Keenetic/apk — из `Packages` фида curl'ом);
`upstream` — последний тег `bol-van/zapret`/`bol-van/zapret2`; нет сборки под
арх./Keenetic для nfqws2 → `"n/a"` (`:966-1007`, `:1064-1104`). Несогласованность:
у tpws `upstream_newer` считается против `avail` (`:998`), у nfqws2 — против
`cur` (`:1095`). apply: lock `/tmp/detour-update.lock`, `opkg update` ×4 →
upgrade/install, затем повторный check (`:1009-1036`, `:1106-1132`); рестарт
движка делает postinst пакета (`build_feed.py:170-173`, `:183-189`). check
синхронный, панель ждёт до 120 с (`panel/src/api/diag.ts:145-151`).

`status.zapret` — `running, pid, port, enabled, domains, ips, ipset_count, args`;
`pid`/`port` — строки или строка `"null"`, хотя панель ждёт `port?: number`
(`router_files/detour-api:2110-2127`, `:2262-2271`; `panel/src/api/types.ts:45-54`).

### 6.4 Перенос на Windows

| Элемент | Статус на Windows |
|---|---|
| режим `zapret` (tpws, :1081, REDIRECT) | **теряет смысл**: tpws на Windows не работает как прозрачный прокси. Теоретически tpws умеет `--socks` (как SOCKS-outbound для sing-box), но рабочая Windows-сборка не проверена |
| `zapret_start/stop/restart/enable/disable`, `status.zapret.pid/port` | смысл только как управление службой winws; иначе убрать/отдавать `running:false` |
| `zapret_config` (аргументы tpws) | к winws не подходят; нужен свой `winws.conf` (`--wf-tcp=443 --wf-udp=443` + опции десинка в стиле nfqws) |
| режим `zapret2` (nfqws2 + NFQUEUE) | аналог — `winws` (zapret v1 на WinDivert). Есть ли Windows-сборка nfqws2 с `--lua-desync` (winws2) — не проверено; строки `--lua-desync` переносимы только с такой сборкой |
| ipset `zapret_domains` + dnsmasq + прогрев | не нужно: winws берёт `--hostlist=<файл>` и сам матчит SNI/Host; `domains.list` → hostlist тем же разбором, IP/CIDR → `--ipset=` |
| iptables nat/mangle, `--queue-bypass`, fwmark, conntrack-flush, nft | нет аналога; WinDivert-фильтр + защита от петли внутри winws |
| связка с sing-box TUN | домены из `domains.list` явно отправлять в `direct` (`domain_suffix`/`ip_cidr`), иначе в режиме all-except они уйдут в VPN; трафик direct-outbound sing-box выходит на физический интерфейс, WinDivert его видит — схема рабочая в теории, **не проверено** |
| `bypass_status/set/stop/autostart/strategy` | сохраняют смысл с режимами `off\|winws[\|winws2]`; `zapret2_supported` → наличие драйвера WinDivert и прав администратора |
| `tpws_update_*`, `nfqws2_update_*` | opkg-фида нет; заменить на обновление бандла winws + WinDivert; формат state-файла сохранить |

---

## 7. Фоновые задачи (cron)

### 7.1 Расписание OpenWrt

Ставит postinst панели (`build_release.py:588-621`): вычищает строки всех
detour-скриптов и добавляет свои; prerm удаляет (`:676-688`). Ключей `autocheck*`
в settings.json нет: тумблер автопроверки обновлений — `AUTO_CHECK` в
`/etc/detour/update.conf` (`router_files/detour-update:102`, `:1382-1390`).

| Команда | Расписание | Что делает | Что пишет |
|---|---|---|---|
| `detour-update check-all` | `0 */6 * * *`, если `AUTO_CHECK≠0` (`build_release.py:589`; тумблер `detour-update autocheck`, `router_files/detour-update:1395-1418`) | проверка панели + sing-box + tpws + nfqws2 в отдельных subshell (`:1138-1144`); пуш о новой версии панели один раз на версию (`:662-672`) | `/var/state/detour-{update,bins,tpws,nfqws2}.json`, `/var/state/detour-update-notified`, `/var/log/detour-update.log` |
| `subscription-refresh` | `17 * * * *` (`:590`) | §3 | профили, файлы подписок, лог |
| `vpn-keepalive` | `*/5 * * * *` (`:591`) | TCP-connect + ICMP до сервера активного профиля, запись; пуши (`router_files/vpn-keepalive:46-47`) | `/var/state/vpn-keepalive.json` |
| `detour-ping` | `* * * * *` (`:592`) | §7.4 | `/tmp/detour-ping.db` |
| `detour-health tick` | `*/2 * * * *` (`:593`) | ступенчатая проверка, 45–60 мин на профиль | `/tmp/detour-health.{db,sched,switch,unsupported}` |
| `detour-health active` | `* * * * *` (`:598`) | цикл 30–60 с по активному профилю | `/tmp/detour-health.{db,active-state,switch}` |
| `detour-hosts refresh-cron` | `23 */12 * * *` (`:599`) | обновление hosts-оверрайда (no-op, если выключен) | dnsmasq addn-hosts |
| `detour-rulist update-cron` | `41 4 * * *` (`:602`; комментарий «дважды в день» неверен) | §7.5 | `ru-subnets.list`, `/etc/detour/rulist.*` |
| `detour-geo update-cron` | `34 5 * * *` (`:606`; первый фоновый scan — `:628-630`) | §7.5 | `/etc/detour/geo.{db,json}` |
| `detour-trafficlog tick` | `* * * * *` (`:609`) | снимок `detour-meter sample` | `/tmp/detour-traffic.tsv` (1440 строк), `/etc/detour/traffic-hour.tsv` (720, сброс раз в час), `/tmp/detour-trafficlog.state` (`router_files/detour-trafficlog:35-43`, `:148-149`) |
| `detour-torrent tick` | `* * * * *` (`:613`) | §7.6 | `/var/state/detour-torrent.{json,tick}` |
| `detour-offload tick` | `* * * * *` (`:617`) | watchdog QCA NSS/PPE | `/etc/detour/offload.conf`, `/var/state/detour-offload.json` |
| `detour-wan-link tick` | `* * * * *` (`:620`) | состояние/скорость WAN, пуш | `/var/state/detour-wan-link.json` |
| acme.sh `--cron` | своё | продление LE (`router_files/detour-cert:555`) | каталог сертификата |

`detour-meter` в cron нет: `read` зовёт CGI `traffic_counters`, `sample` — trafficlog;
цепочки ставятся лениво (`router_files/detour-meter:335-354`). `detour-netmon` в
репозитории нет (по памяти проекта — поставлен на home вручную; не проверено).

### 7.2 Keenetic: `keenetic/sbin/detour-cron`

crond на KeeneticOS нет (`:4-12`) → цикл `TICK=300` с, старт через `BOOT_DELAY=120`,
счётчик `n` по модулю 72 (`:27-28`, `:50`, `:135`):

| n | Задачи |
|---|---|
| каждый тик | vpn-keepalive (`:53`), detour-ping (`:59`), detour-torrent tick (`:66`), detour-wan-link tick (`:71`), detour-health active в фоне с `ACTIVE_LOOP_BUDGET=270` (`:31`, `:82-85`), detour-trafficlog tick (`:133`) |
| `n%2==0` | detour-health tick (`:99-101`) |
| `n%12==0` | subscription-refresh (`:88-90`) |
| `n%72==0` | detour-update check-all при `AUTO_CHECK≠0` (`:104-106`); detour-cert renew (раз в 6 ч, `:110-112`) |
| `n%72==36` | detour-rulist update-cron (`:117-119`) |
| `n%72==54` | detour-geo update-cron (`:124-126`) |

Для Windows-службы это и есть модель: один внутренний планировщик с тиками, а не
cron.

### 7.3 detour-health (функциональная проверка + автопереключение)

Режимы (`router_files/detour-health:181-200`):

| Режим | Назначение | Блокировка | Гейт `health_check_enabled` |
|---|---|---|---|
| `tick` | ступенчатая проверка | `LOCK` | да |
| `sweep` | полная проверка | `LOCK` | да |
| `active` | только активный профиль | свой `ALOCK` | да |
| `one <id>` | ручная проверка одного | нет | нет |

Гейт выключается значениями `0|off|false|no` (`:209-214`).

**Временный sing-box**: clash API `127.0.0.1:$((19390 + $$ % 1000))` (порт на
запуск, чтобы не прилипнуть к осиротевшему процессу, `:83`); конфиг
`log.level=error`, `experimental.clash_api.external_controller`,
`outbounds=[direct, h1..hN]`, WG → `endpoints[]`, `route.final=direct`,
**инбаундов нет** (`:450-456`); тег профиля `h<n>`, map id→тег (`:433-442`);
utls на QUIC вырезается (`:425-428`); `PROBE_MARK>0` → `routing_mark` (`:436`).
Имена конфигов `/tmp/detour-health.conf.$$`, в `active` — `.aconf.$$` (`:72`, `:207`);
данные `-D /tmp/detour-health.sbd.$$`. Готовность — опрос `GET /version` до
`SB_WAIT=20` с (`:462-473`); остановка kill → 5 с → kill -9 (`:475-481`);
`reap_orphans` (tick/sweep) убивает всё с `/tmp/detour-health\.conf\.` по `ps w`
(`:276-280`, `:296`); `active` чистит свои `.aconf.` (`:969-971`).
WireGuard **не исключается** — приводится к endpoint (`:391-412`, `:429`).

**Отказ конфига**: чанков по факту нет (комментарий `:24-29` устарел) — весь
список грузится в один экземпляр; не стартовал → рекурсивное деление пополам;
одиночный отказник → `mark_down` (`:994-1013`, `:586-590`).

**Список профилей** (`:352-364`): все `*.json` с `outbound` и `id`, кроме
содержащих ключ `warp` (WARP в одиночку из РФ не поднимается, `:343-351`;
проверяется `detour-warp verify`).

**URL проверки**: `health-urls.list` (§1.7, `:312-335`).

**Delay**: `GET http://<clash>/proxies/h<n>/delay?timeout=4000&url=<urlenc>`,
`curl -m 6`; успех — в ответе есть `"delay":<число>` (`:550-553`, `:91-92`). HTTP-код
целевого сайта скрипт не проверяет (что sing-box считает успехом — не проверено).

**Вердикт** (`probe_one`, `:557-582`): URL по порядку, неудачный повторяется
**один раз**; после первого провала остальные пишутся `-1` без запроса; `ok=1`
только если прошли **все**; `rtt` — задержка первого успешного URL.
Параллельно `MAXJOBS=4`, пауза `PACE=1` с между пачками (`:84-89`, `:860-878`).

**`/tmp/detour-health.db`** (строка активного профиля первой):
`<id>\t<ok 0|1>\t<ts>\t<rtt|-1>\t<delays csv ms|-1>\t[dl_kbit/s|-1]`
(`:31-35`, `:581`, `:619-627`, `:1074-1081`). Пустой результат не затирает старую
базу (`:1083-1089`); `active` делает upsert с сохранением старого `dl` (`:596-614`).

**Расписание tick**: `/tmp/detour-health.sched`, строки `id\tnext_due`; новый
профиль — случайная фаза `[0,3600)`, после проверки — `now + rand[2700,3600]`
(`:99-100`, `:1021-1045`, `:1096-1110`); ничего не подошло → выход без запуска
sing-box; первый tick после загрузки (пустая база) и `sweep` → все (`FORCE_ALL`,
`:896-900`).

**Поддержка clash API**: раз на загрузку `clash_supported` (конфиг только с
clash API, `:881-907`); нет → `/tmp/detour-health.unsupported`, tick/active выходят.

**Скорость `dl`** (`:217-251`, `:487-539`, `:1112-1142`): свой конфиг — `mixed`
на `127.0.0.1:(18080 + $$ % 1000)`, `route.final=vpn`; `curl -m SPEED_M -x socks5h://…
-w %{speed_download}` → kbit/s = B/s × 8 / 1000; URL
`https://speed.cloudflare.com/__down?bytes=<health_speed_bytes>` (дефолт 8 000 000)
или `health_speed_url`; `SPEED_M` = 5/8/12/15 с при ≤8/≤30/≤75/>75 МБ. Кого
меряют: tick/sweep — **каждый** здоровый проверенный профиль (комментарии про
«только активный» в `:218-221` и `router_files/detour-api:621-622` устарели),
кроме `speedcheck-exclude.list` (`:541-545`); `one` — если ok (exclude не
учитывается); `active` — никогда.

**Режим `active`** (`:947-992`, `:786-850`): условия — нет маркера unsupported,
база не пустая, `ALOCK` (протухает через 600 с), активен **одиночный** профиль
(цепочка с запятой пропускается). Бюджет `ACTIVE_LOOP_BUDGET` (дефолт 55 с; на
Keenetic 0, если не передан env, `:114-115`, `:140`). Цикл: проверка →
`sleep rand[30,60]`, пока следующий шаг влезает в бюджет (`:981-990`).
Подтверждение: провал → сразу повторная проверка (итого два провала подряд плюс
ретрай внутри каждого URL, `:802-807`). Временный sing-box не стартовал — ни
пуша, ни вердикта (`:796-801`). Состояние прошлого прохода —
`/tmp/detour-health.active-state` (`<id>\t<ok>`, `:63`, `:848`). Мультиаплинк
(метки detour-wanpin) влияет только на текст пуша (`:750-784`, `:822-834`).

**Пуши** (`detour-push send`, `:652-655`):

| Когда | Заголовок | Ссылка |
|---|---|---|
| успешное автопереключение (active и хвост tick/sweep) | `VPN: «A» → «B»` | `:726-727`, `:1178-1179` |
| active: провал подтверждён, не переключились, и прошлое состояние не «этот же профиль, 0» (в т. ч. свежее подключение к нерабочему) | `VPN недоступен: «A»` + причина `ASW_REASON` | `:837-843` |

На восстановление пуша нет.

**Автопереключение**:
- условия: `health_auto_switch ∈ 1|on|true|yes`, `detour-api` исполняемый,
  одиночный профиль, подтверждённый провал;
- кандидат (`pick_switch_candidate`, `:662-669`) — из **кэша** db: `ok=1`, не
  текущий, не в `autoswitch-exclude.list` (пробелы/`\r` вырезаются); побеждает
  **минимальный `rtt`** (пустой/отрицательный = 999999), при равенстве — первый
  в файле; скорость не участвует; кандидат перед переключением **не
  перепроверяется**;
- кулдаун `SWITCH_COOLDOWN=120` с по `ts` в `/tmp/detour-health.switch`
  (`{"from":"…","to":"…","ts":N}`, `:119`, `:707-715`, `:724`) — действует
  **только в `active`**; хвост tick/sweep (`:1149-1183`) кулдаун не проверяет и
  переключает, если активный профиль был в раунде с `ok=0` плюс повторная проверка
  (`:1158-1164`; если повторная проверка не стартовала — всё равно переключает);
- вызов `detour-api activate <id>` (`:681`, CLI §2.6); успех проверяется по
  `settings.active_profile == target`, а не по коду возврата (`:679-686`).

Ключи settings (строки): `health_check_enabled` (`"1"`), `health_auto_switch`
(`"0"`), `health_speed_enabled` (`"1"`), `health_speed_url` (`""`),
`health_speed_bytes` (`"8000000"`); парсер `jstr` читает только строковые значения
(`:146`).

### 7.4 detour-ping

`router_files/detour-ping`: lock `/tmp/detour-ping.lock` (протухает через 600 с),
`MAXJOBS=24` (`:35`, `:50-61`). Адрес — `server` профиля, у WG — первый строковый
`address` (адрес пира, `:124-133`). Проба: `ping -c1 -W2`, при пустом ответе
один повтор; нет ответа → TCP-connect на `server_port` (luasocket, таймаут 3 с,
или `nc` + `/proc/uptime`, `:80-114`); для `wireguard|hysteria2|tuic` TCP-фолбэка нет
(`:136-154`). Формат db — §1.8, строка активного профиля первой; пустой
результат старую базу не затирает (`:20-24`, `:176-197`).

### 7.5 detour-geo и detour-rulist

**detour-geo**: `geo.db` TSV `<id>\t<ip>\t<cc>\t<ts>` (пустой cc = ещё не искали,
`router_files/detour-geo:28`, `:286-289`); `geo.json` (`:95`), `status` добавляет
атрибуцию DB-IP (`:107`). Алгоритм: резолв 16 потоками (`nslookup` с убийством
через ~2 с, `:115-162`) → только новые IP → стрим CSV DB-IP
(raw.githubusercontent `sapics/ip-location-db` → зеркало jsDelivr, `curl -4`)
через awk одним слиянием по отсортированным целым (`:59-62`, `:238-263`); <1000
строк → ответ отброшен (`:268`). `update-cron`: полное обновление при возрасте
>30 дней, иначе инкрементальное (`:43`, `:310-318`). Страна — **входного узла**,
не выхода (панель фильтрует по флагу в имени).

**detour-rulist**: источники `maxmind` (дефолт) / `rir` (`router_files/detour-rulist:68-79`);
`ru-subnets.list` — 4 строки заголовка `// …`, затем нормализованные IPv4 CIDR
(/8–/32, IPv6 отбрасывается, `:140-150`, `:351-357`); исключения → тот же ipset с
`nomatch` (`:20-23`, `:280-281`); state `rulist.json` + `rulist.applied`,
`rulist.applied-excl`, `rulist.pending-del`; применение — diff с прошлым
состоянием + conntrack-flush изменённых сетей (`:235-297`); загрузка при старте —
`ipset-load` (`router_files/sing-box.initd:761`, `:1128`); `update-cron` только при
`auto=true && enabled=true` и возрасте ≥6 суток (`:58`, `:391-397`); <1000 подсетей
= отказ (`:339-345`). Миграция (`migrated`) при первом обновлении вычищает старые
RU-подсети из ручного whitelist (подробности не разбирались — не проверено).

### 7.6 detour-torrent

- Allow-список (§1.6); цепочка блокируется, если **любой** хоп не разрешён
  (`router_files/detour-torrent:65-71`, `:130-141`).
- OpenWrt: таблица `inet detour_torrent`, `hook prerouting priority mangle` (раньше
  nat REDIRECT), LAN↔LAN и мультикаст пропускаются (`:170-198`); `drop` + запись
  клиента в `clients4/clients6` на 30 мин. Сигнатуры: `utp-syn` (`udp length 28` +
  `@th,64,16 0x4100` + `@th,128,32 0` + `@th,208,16 0`), `udp-tracker`
  (`@th,64,64 0x0000041727101980`), `dht-query` (`d1:ad2:i`), `dht-reply`
  (`d1:rd2:i`), `bt-handshake` (`\x13BitTorrent protocol` при doff 5/8).
- Keenetic: mangle-цепочки `DETOUR_TORRENT[_D]`, `xt_string`, `xt_recent` (не
  проверено на железе, `:245-293`).
- Второй слой — reject-правило sing-box (TCP, §2.2).
- `apply` строит правила, только если их нет (не обнулять счётчики, `:345-365`).
  `tick`: рост счётчиков → событие + пуш «Detour: торренты заблокированы»; новый
  эпизод после тишины >`EPISODE_GAP=900` с, повторный пуш не чаще
  `PUSH_COOLDOWN=1800` с (`:102-103`, `:416-464`); `/var/state/detour-torrent.tick`
  = `<hits> <last_seen> <last_push>`.

### 7.7 Что переносится на Windows

**Почти без изменений** (планировщик внутри Rust-службы):
- **detour-health** целиком (временный sing-box.exe + clash API delay + замер
  через mixed/socks5h, форматы db/sched/switch, автопереключение, кулдаун, пуши).
  Ловушка: на роутере проба идёт из OUTPUT мимо REDIRECT; на Windows активный TUN
  с `auto_route` захватит соединения второго sing-box.exe → нужен
  `route.auto_detect_interface: true` в конфиге пробы и/или правило
  `process_path`/`process_name → direct` в живом конфиге (не проверено).
  `reap_orphans` через `ps` → Job Object. `routing_mark`/мультиаплинк — убрать.
- **detour-ping**: ICMP через `IcmpSendEcho2Ex` с привязкой к физическому
  интерфейсу (иначе уйдёт в TUN), TCP-connect — тоже с bind. Формат db сохранить.
- **detour-geo**: целиком (HTTP-стрим + разбор).
- **subscription-refresh**, проверка версий в **detour-update** (state-файлы
  сохранить; opkg — нет), **vpn-keepalive**, **Web Push**.

**Меняют реализацию**:
- **detour-rulist**: ipset → sing-box `rule_set` (inline или source-файл) `ip_cidr`
  → `direct`; `nomatch` в sing-box нет — исключения правилом **выше**
  (`ip_cidr` → proxy). conntrack-flush → `DELETE /connections` clash API.
- **detour-torrent**: nft/xt_string → только sniff-reject sing-box (TCP) или
  WinDivert-фильтр с теми же байтовыми шаблонами; «клиенты» → имя процесса из
  `/connections`.
- **detour-meter/trafficlog**: iptables-счётчики → агрегирование `/connections`
  clash API по outbound (direct / proxy / bypass) + счётчики интерфейса
  (`GetIfTable2`); формат рядов `ts direct vpn bypass rx tx` сохранить.
- **detour-hosts**: dnsmasq addn-hosts → `dns` hosts/predefined в sing-box.

**Теряют смысл**: detour-offload, detour-wan-link, detour-wanpin, detour-netmon,
ipset/dnsmasq-прогрев, opkg/apk-фиды, Keenetic-демон detour-cron.

---

## 8. Сводка для Rust-порта

### 8.1 Набросок конфига sing-box для Windows (TUN) — не проверено

Это не извлечение из кода, а перенос роутерной логики (§2.8) в правила sing-box.
Все формы — sing-box 1.12+/1.13; перед использованием прогнать `sing-box check`.

```jsonc
{
  "log": {"level": "warn", "output": "C:\\ProgramData\\Detour\\sing-box.log", "timestamp": true},
  "dns": {
    "servers": [
      {"type": "https", "tag": "remote", "server": "1.1.1.1", "detour": "proxy"},
      {"type": "local", "tag": "local"}
    ],
    "rules": [ {"domain_suffix": ["<whitelist / direct-домены>"], "server": "local"} ],
    "final": "remote"                       // proxy-list: "local"
  },
  "inbounds": [
    {"type": "tun", "tag": "tun-in", "address": ["172.19.0.1/30"],
     "auto_route": true, "strict_route": true, "stack": "mixed"}
  ],
  "endpoints": [ /* WG-хопы, §2.7 п.2 */ ],
  "outbounds": [ /* хопы chain_1…proxy c detour, цели out_<id>/rc<N>_i */,
                 {"type": "direct", "tag": "direct"} ],
  "route": {
    "auto_detect_interface": true,
    "default_domain_resolver": "local",
    "rules": [
      {"action": "sniff"},
      {"protocol": "dns", "action": "hijack-dns"},
      {"ip_is_private": true, "outbound": "direct"},
      {"ip_cidr": ["<egress blocklist>"], "action": "reject"},
      {"protocol": ["bittorrent"], "network": ["tcp"], "action": "reject"},   // если TORRENT_REJECT
      /* карта маршрутов: domain / domain_suffix / ip_cidr → тег цели; пропавшая цель → reject */
      /* домены DPI-обхода (winws) → direct */
      /* all-except: ip_cidr исключений RU → proxy; whitelist + ru-subnets → direct */
      /* proxy-list: proxy-domains → proxy */
      /* udp_vpn_mode=off: {"network":["udp"],"outbound":"direct"}; list: udp-правила по списку → proxy */
    ],
    "final": "proxy"                        // proxy-list: "direct"; allvpn: "proxy" без списков
  },
  "experimental": {"clash_api": {"external_controller": "127.0.0.1:9090", "secret": "<random>"}}
}
```

Большие списки (whitelist ~13 000 имён, ru-subnets ~13 000 CIDR) лучше отдавать
как локальные `rule_set` (source-формат JSON или скомпилированный `.srs`), а не
инлайном.

### 8.2 Главные подводные камни

1. **Решение «VPN или напрямую» на роутере принимает файрвол, а не sing-box.**
   В конфиге sing-box нет ни режима маршрутизации, ни DNS: только `sniff`,
   правила карты маршрутов и `final: proxy` (§2.2). В TUN весь слой ipset/dnsmasq/
   iptables (`proxy-list`/`all-except`, whitelist, RU-подсети с `nomatch`,
   `udp_vpn_mode`, `allvpn`, egress-блоклист, fail-closed пропавших целей) надо
   заново выразить правилами sing-box + перехватом DNS (§2.8, §8.1).
2. **WireGuard-профили из формы панели, вероятно, не активируются**:
   `translate_wireguard_profile` только переименовывает ключи и даёт два `address`
   и `peer_public_key` на верхнем уровне (`router_files/detour-api:509-519`).
   Правильный конвертер — `to_endpoint` из `router_files/detour-health:396-412`.
3. **Текстовое чтение JSON.** `json_val` берёт **последнее** вхождение ключа в
   однострочном файле (`router_files/detour-api:404-406`) — `type` может оказаться
   `transport.type`; `extract_profile_outbound` ищет первый литерал `"outbound"`.
   В Rust — нормальный JSON и верхнеуровневые поля.
4. **settings.json**: все значения — строки; писатель переписывает фиксированный
   набор ключей (всё прочее теряется); `active_chain` хранит CSV хопов, а не id
   цепочки — «активна ли цепочка» определяется сравнением CSV (§1.2, §1.3).
5. **Контракт с панелью**: ошибки только `200 + {"ok":false,"error"}`; JSON-тела
   приходят как `text/plain`; пустой ответ = «сервер перезапускается»; слово
   `timeout` в ошибке подменяется; `status.version` обязана совпадать с VERSION
   сборки, иначе reload; `bypass_strategy` GET — голая строка; `pid`/`port` в
   `status` — строки, `"null"` строкой (§0.1, §4.9).
6. **Детач-операции** (`warp_register/verify`, `health_check` без id, `*_update_apply`)
   отвечают `started` раньше, чем воркер пишет state (`sleep 1` в `detach_bg`,
   `router_files/detour-api:354`), а `poll()` панели делает первый запрос сразу —
   state «running» писать до ответа.
7. **Операции без применения**: `profile_save` не пересобирает конфиг даже для
   активного профиля; `domains`/`whitelist`/`zapret_domains`/`zapret_config` POST
   только пишут файл; `singbox_config` POST пишет файл **до** `sing-box check`;
   subscription-refresh не перезапускает sing-box (§3.7, §4).
8. **Подписки**: серверный парсер понимает только vless/trojan/vmess/ss (клиентский
   `uri.ts` — ещё hysteria2/tuic/socks/http); id профилей без префикса подписки
   (коллизии перезаписывают чужие файлы); `group` — ключ удаления устаревших;
   URL с токеном попадает в лог и в `output` ответа; xhttp в URI молча превращается
   в TCP-профиль (§3).
9. **Грабли sing-box 1.13** (§2.7): utls на QUIC, WG только endpoint, sniff только
   правилом, сниффер `bittorrent` и reject только TCP, xhttp роняет конфиг,
   `block`-outbound deprecated, legacy DNS-формат удаляется в 1.14.
10. **Health-check на Windows**: временный sing-box.exe пробы будет захвачен
    основным TUN — нужен `auto_detect_interface`/bind к физическому интерфейсу или
    исключение по процессу; то же для ICMP/TCP-пинга (§7.7). Автопереключение берёт
    кандидата с минимальным `rtt` **из кэша** без перепроверки, кулдаун 120 с
    действует только в режиме `active` (§7.3).
11. **DPI**: tpws на Windows бесполезен; аналог zapret2 — winws (WinDivert, hostlist
    вместо ipset); строки `--lua-desync` переносимы только при наличии Windows-сборки
    nfqws2 (не проверено); домены DPI-обхода нужно явно отправить в `direct` в TUN (§6.4).
12. **Гейтинг панели**: ветвление только через `status.isKeenetic` — нужен
    `isWindows`/`isRouter` в `stores/status.ts:20` (тип `Platform` в рабочем дереве
    уже расширен); offload/
    swap/WARP/rulist/hosts гейтятся ответами `supported:false`; `udp_vpn` GET не
    tolerant — отдавать явный `{mode:"off",supported:false}`, если фичи нет; для
    остальных разделов — `v-if` в местах из §5.4.
13. **`traffic_counters`** — проценты 0..100 плюс байты за `span`, чтение
    деструктивно (дельта с прошлого вызова) — в Rust держать базу на потребителя (§4.7).
