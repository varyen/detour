# Плагины Detour

Плагин — отдельный пакет `detour-plugin-<id>`, который ставится поверх панели и
может добавить:

- свои файлы на роутер: скрипты, init, CGI, Lua — что угодно;
- обработчик действий `api`: панель зовёт его через `detour-api` уже с проверкой
  сессии;
- периодические задачи;
- карточку на «Обзоре»;
- секции в разделах «Сервисы», «Правила» и «Профили»: форму настроек, сводку и
  кнопки;
- свой UI-модуль (ES-модуль) для всего, что не описывается данными.

Работает на OpenWrt (opkg и apk), Keenetic/Entware и Linux-сервере (контейнер
OpenWrt 25.12, тот же `.apk`). В приложении на устройстве (Tauri) плагинов нет:
это пакеты роутера.

## Раскладка в репозитории

```
plugins/<id>/
  plugin.json        манифест (обязателен)
  api                обработчик действий (sh или lua, шебанг #!/bin/sh | #!/usr/bin/lua)
  router_files/      файлы для роутера — кладутся по записям files[] манифеста
  keenetic/          файлы только для Keenetic (files[] с "platform": "keenetic")
  cgi-bin/           свои CGI → /www/cgi-bin/ (Keenetic: /opt/share/www/cgi-bin/)
  panel/             UI-модуль и ассеты → /www/detour/plugins/<id>/
                     (есть panel/dist/ — берётся он, исходники не пакуются)
  tools/             стенды и утилиты разработки, в пакет НЕ попадают
```

`id`: `a-z`, `0-9`, `-`, до 32 символов, совпадает с именем каталога.

## Манифест `plugin.json`

```jsonc
{
  "id": "hello",
  "name": "Привет",
  "version": "1.0.0",               // X.Y.Z — версия пакета
  "description": "…",
  "author": "…",
  "icon": "bolt",                   // имя из AreaIcon панели (plug по умолчанию)
  "min_panel": "2.12.0",            // старее — плагин не ставится и не включается
  "platforms": ["openwrt", "keenetic"],
  "catalog": false,                 // не публиковать в каталог (по умолчанию true)
  "depends": { "openwrt": ["ttyd"], "keenetic": [] },   // пакеты штатного фида
  "files": [
    { "src": "router_files/detour-hello", "dst": "/usr/sbin/detour-hello", "mode": "0755" },
    { "src": "keenetic/S80hello", "dst": "/opt/etc/init.d/S80hello", "mode": "0755", "platform": "keenetic" },
    { "src": "router_files/hello.initd", "dst": "/etc/init.d/hello", "mode": "0755", "platform": "openwrt" }
  ],
  "cron": [ { "op": "tick", "every": 300 } ],          // секунды, не меньше 60
  "ui": {
    "module": "ui.js",              // panel/ui.js — необязательно
    "card": {
      "title": "Привет", "hint": "…", "icon": "bolt",
      "span": 2,                    // 2 | 3 | 4 | 6 колонок из шести
      "op": "card",                 // действие, отдающее содержимое (см. «Вид»)
      "poll": 30,                   // период обновления, с; 0 — только при открытии
      "actions": [ { "label": "…", "op": "…", "confirm": "…", "variant": "danger" } ],
      "component": "Card"           // вместо op: компонент из UI-модуля
    },
    "sections": [
      {
        "id": "main", "area": "services",     // services | rules | profiles
        "title": "…", "icon": "…", "summary": "…",
        "op": "card",               // сводка в шапке и метрики в теле
        "fields": [ … ],            // форма настроек
        "actions": [ … ],
        "component": "About"        // компонент из UI-модуля
      }
    ]
  }
}
```

### Пути и Keenetic

`dst` пишется путём OpenWrt, сборщик сам переносит его на Keenetic:
`/etc/` → `/opt/etc/`, `/usr/sbin/`, `/sbin/` → `/opt/sbin/`,
`/usr/bin/`, `/bin/` → `/opt/bin/`, `/usr/share/` → `/opt/share/`,
`/usr/lib/` → `/opt/lib/`, `/www/` → `/opt/share/www/`. Шебанги исполняемых
файлов правятся на `/opt/bin/sh` и `/opt/bin/lua`. Init-скрипт OpenWrt
(`/etc/init.d/…`, procd) на Keenetic не переносится: нужен отдельный файл
`S##имя` с `"platform": "keenetic"`, а у OpenWrt-варианта — `"platform": "openwrt"`.

Нельзя класть файлы в пути самой панели и в пользовательское состояние:
`/etc/detour.auth`, `/etc/detour/plugins/`, профили и `settings.json` sing-box,
конфиги менеджера пакетов. Сборщик такое отвергает.

Различия рантайма Keenetic те же, что у панели: нет nft, нет crond (задачи
плагина зовёт `detour-cron` раз в ~5 мин), web — lighttpd. Плагин, которому
нужна недоступная там подсистема, просто не объявляет `keenetic` в `platforms`.

### Поля настроек

| type | что это | доп. ключи |
|---|---|---|
| `text` | строка | `maxlength` (4096), `pattern` (Lua-шаблон целиком), `placeholder` |
| `password` | строка, в панель не возвращается | пустое при сохранении = оставить прежнее |
| `number` | число | `min`, `max` |
| `bool` | переключатель | |
| `select` | выбор | `options: [{ "value", "label" }]` или `["a", "b"]` |
| `textarea` | многострочный текст | `maxlength` |

Общие: `key` (`[A-Za-z0-9_]`, уникален в плагине), `label`, `hint`,
`default`. Проверку и запись делает роутер (`detour-plugin settings-set`):
ключи вне схемы отбрасываются, типы приводятся. Результат лежит в
`$PLUGIN_SETTINGS` — плоский JSON-объект.

## Обработчик `api`

`detour-plugin` зовёт `api <op>` с телом запроса на stdin. Ответ — JSON в
stdout (пустой вывод с кодом 0 → `{"ok":true}`). Окружение:

| переменная | значение |
|---|---|
| `PLUGIN_ID` | id плагина |
| `PLUGIN_DIR` | каталог манифеста и `api` (только чтение, перезаписывается обновлением) |
| `PLUGIN_DATA` | `/etc/detour/plugins/<id>` — своё состояние; переживает обновления, прошивку и попадает в keeplist |
| `PLUGIN_SETTINGS` | `$PLUGIN_DATA/settings.json` |
| `PLUGIN_PLATFORM` | `openwrt` или `keenetic` |
| `PLUGIN_METHOD` | `GET`, `POST` (из панели) или `CLI` (cron, хуки) |

Служебные op зовёт только рантайм, из панели они недоступны:

| op | когда |
|---|---|
| `install` | после установки и обновления пакета (postinst) |
| `remove` | перед удалением пакета (не при обновлении) |
| `enable` / `disable` | переключатель в панели |
| `settings` | после сохранения настроек; ответ уходит в панель как ответ сохранения (`{"ok":false,"error":"…"}` — отказ, `message` — тост) |

Выключенный плагин не получает ни вызовов из панели, ни задач `cron`. Свои
службы ему стоит гасить в `disable`, а скриптам — проверять
`detour-plugin enabled <id>`.

Действия, меняющие состояние, стоит пускать только по `PLUGIN_METHOD=POST`:
кнопки панели шлют POST, а GET — это опрос карточки.

### Вид: ответ для карточки и секции

```json
{
  "ok": true,
  "status": { "text": "работает", "tone": "ok" },
  "lead": "короткая фраза",
  "metrics": [ { "label": "Тиков", "value": "12", "hint": "…", "tone": "warn" } ],
  "actions": [ { "label": "Сбросить", "op": "reset", "confirm": "Точно?", "variant": "danger" } ]
}
```

`tone`: `ok` | `warn` | `bad`. В секции `status` — чип в шапке, `lead` —
строка под заголовком. Ответ кнопки — `{"ok":true,"message":"…"}` (тост) или
`{"ok":false,"error":"…"}`.

## UI-модуль

`panel/<module>` — ES-модуль. Панель грузит его через `import()` и вызывает
`default(host)`; ждёт `{ components: { Имя: Компонент } }`. Компилятора
шаблонов в сборке панели нет, поэтому компоненты пишутся render-функциями:

```js
export default function setup(host) {
  const { h, ref, defineComponent } = host.vue;
  const About = defineComponent({
    setup() {
      const out = ref("");
      const run = async () => (out.value = JSON.stringify(await host.call("echo", "hi")));
      return () => h("div", [h(host.components.UiButton, { onClick: run }, () => "echo"), out.value]);
    },
  });
  return { components: { About } };
}
```

`host` (`apiVersion: 1`, поля только добавляются):

| поле | что |
|---|---|
| `vue` | тот же экземпляр Vue, что у панели |
| `plugin` | `{ id, name, version, manifest }` |
| `settings`, `secrets` | реактивные настройки; для паролей — только «задан ли» |
| `call(op, body?)` | действие `api`; с телом — POST |
| `saveSettings(patch)` | сохранить настройки (с проверкой по схеме) |
| `toast.ok/info/error(text)` | уведомления панели |
| `components` | `UiButton`, `SwitchToggle`, `TileCard`, `FormField`, `SegmentedControl`, `AreaIcon` |

Стили — CSS-переменные панели (`--ink`, `--dim`, `--line`, `--accent`,
`--ok`, `--warn`, `--bad`, `--mono`, `--radius-sm`), тогда плагин сам следует
теме.

## Сборка и публикация

```sh
python build_plugin.py --check          # манифесты и раскладка
python build_plugin.py hello --no-sign  # локальная сборка без ключа
python build_plugin.py                  # все, с подписью → releases/plugins/
python build_plugin.py --publish        # все + каталог в ветку feed (plugins/)
```

На плагин три пакета: `_all.ipk` (opkg), `_noarch.apk` (apk-tools 3),
`_keenetic.ipk`, у каждого `.sig`. Каталог `feed/plugins/index.json` (+`.sig`)
описывает версии, файлы и sha256. `--publish` заменяет каталог плагинов в ветке
целиком, поэтому публикует всегда все плагины репозитория.

## На роутере

```sh
detour-update plugins-catalog              # скачать и проверить каталог
detour-update plugin-install <id>          # из каталога (и обновление тоже)
detour-update plugin-inspect <pkg> [sig]   # что за пакет, подписан ли
detour-update plugin-install-local <pkg> [1]   # 1 — разрешить без подписи
detour-update plugin-remove <id> [purge]   # purge — стереть и настройки
detour-update plugins-restore              # вернуть плагины после прошивки
detour-plugin list | call <id> <op> | enable <id> | disable <id> | tick
```

Пакет из каталога обязан быть подписан ключом релизов (на Entware без `usign`
подпись не проверить — там доверие держится на HTTPS и sha256 из
подписанного индекса). Загруженный в панель пакет без подписи ставится только
после явного подтверждения. Копия каждого установленного пакета лежит в
`/etc/detour/plugins/.pkg/` — по ней `plugins-restore` возвращает плагины
после прошивки (через `restore.list` и `detour-bootstrap-install`). Настройки
плагинов входят в полную резервную копию панели.
