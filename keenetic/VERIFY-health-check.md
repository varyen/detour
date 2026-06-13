# Keenetic: что проверить владельцу после обновления до 1.10.0

В 1.10.0 появилась **функциональная проверка ВПН** — раз в час роутер реально
открывает целевые сайты (YouTube, видео-CDN, Google) **через каждый VPN-профиль**
и показывает в панели бейдж «Работает» (отдельно от пинга). Пинг говорит лишь
«сервер отвечает», а эта проверка — «трафик реально идёт».

На **OpenWrt/GL.iNet** всё проверено вживую. На **Keenetic** (Entware, mipsel)
есть один неизвестный момент, который надо подтвердить на вашем железе. Этот файл —
короткий чек-лист (нужен SSH-доступ к роутеру; всё под `/opt`).

> Если что-то не так — фича сама себя выключает (панель прячет блок «Работает»),
> она **не** ломает ни ВПН, ни панель. Худший случай — просто нет бейджа «Работает».

---

## Главный вопрос

Проверка работает через **clash-API** внутри sing-box. На Keenetic sing-box — это
Entware-пакет `sing-box-go` (`/opt/bin/sing-box`), а не наш бинарник. **Надо
подтвердить, что он собран с `with_clash_api`.** Скорее всего да (официальные сборки
его включают), но это и есть то, что стоит проверить.

## Шаг 0 — обновитесь до 1.10.0

```sh
opkg list-installed | grep -i detour          # detour-keenetic — версия 1.10.0?
ls -la /opt/sbin/detour-health                # должен существовать
```

Если `detour-health` нет — поставьте свежий `detour-keenetic_1.10.0_all.ipk`
(через панель «обновить из файла» или `opkg install`).

## Шаг 1 — РЕШАЮЩАЯ проба: есть ли clash-API

Скопируйте блок целиком в SSH на роутере. Он не трогает боевой sing-box
(слушает свой порт на loopback) и сам за собой убирает.

```sh
SB=/opt/bin/sing-box; PORT=127.0.0.1:19390
echo "== sing-box =="; "$SB" version 2>&1 | head -2
echo "== with_clash_api в бинарнике (>0 = есть) =="; grep -aoc with_clash_api "$SB" 2>/dev/null
echo "== lua/cjson =="; command -v lua >/dev/null && lua -e 'print(require("cjson.safe") and "cjson-ok" or "NO-CJSON")' 2>&1 | tail -1 || echo "NO-LUA"
echo "== curl =="; command -v curl >/dev/null && echo curl-ok || echo "NO-CURL"
echo "== ЖИВАЯ clash-проба =="
rm -rf /tmp/cltest.d /tmp/cltest.json; mkdir -p /tmp/cltest.d
printf '{"log":{"level":"error"},"experimental":{"clash_api":{"external_controller":"%s"}},"outbounds":[{"type":"direct","tag":"direct"}],"route":{"final":"direct"}}' "$PORT" > /tmp/cltest.json
"$SB" run -c /tmp/cltest.json -D /tmp/cltest.d >/tmp/cltest.log 2>&1 &
P=$!; i=0
while [ $i -lt 15 ]; do curl -s -o /dev/null "http://$PORT/version" 2>/dev/null && break; sleep 1; i=$((i+1)); done
echo "clash /version → $(curl -s -m 3 "http://$PORT/version" 2>/dev/null || echo '(нет ответа)')"
kill $P 2>/dev/null; sleep 1; kill -9 $P 2>/dev/null
echo "sing-box log:"; tail -3 /tmp/cltest.log
rm -rf /tmp/cltest.d /tmp/cltest.json /tmp/cltest.log
```

**Как читать результат:**

- `clash /version → {"meta":true,...,"version":"sing-box ..."}` → **clash-API есть,
  всё заработает.** Переходите к Шагу 2.
- `clash /version → (нет ответа)` + в логе ошибка про `clash_api`/`experimental` →
  **сборка без clash-API.** Функц. проверка на этом устройстве работать не будет
  (деградирует штатно — Шаг 4). Варианты: поставить sing-box с clash-API того же
  arch, либо просто пользоваться пингом.
- `NO-LUA` / `NO-CJSON` → `opkg install lua lua-cjson` (их использует и панель, так
  что обычно уже стоят). `NO-CURL` → `opkg install curl`.

## Шаг 2 — реальный прогон

```sh
# Быстрая проверка ОДНОГО профиля (активного) — несколько секунд:
ACT=$(sed -n 's/.*"active_profile"[^"]*"\([^"]*\)".*/\1/p' /opt/etc/sing-box/settings.json | head -1)
echo "active=$ACT"; /opt/sbin/detour-health one "$ACT"
#   ↑ ждём строку вида:  <id>\t1\t<время>\t<мс>\t<задержки>   (1 = работает, 0 = нет)

# Полный прогон всех профилей. Идёт АККУРАТНО (не более 4 проверок одновременно,
# с паузами) — это нагружает слабый роутер не залпом. Поэтому на Keenetic это
# несколько минут, иногда дольше, чем на OpenWrt — это нормально.
time /opt/sbin/detour-health sweep
echo "всего=$(wc -l < /tmp/detour-health.db)  работает=$(awk -F'\t' '$2==1' /tmp/detour-health.db | wc -l)"
ls -la /tmp/detour-health.unsupported 2>/dev/null && echo "^ маркер 'не поддерживается' — clash-API нет" || echo "маркера нет — хорошо"
tail -5 /opt/var/log/detour-health.log 2>/dev/null
```

**Хорошо:** `detour-health one` печатает строку с `1`/`0` во втором поле; полный
прогон наполняет `/tmp/detour-health.db` (часть профилей `работает`); маркера
`/tmp/detour-health.unsupported` нет; после прогона **не остаётся** лишних
процессов sing-box (`ps w | grep -c '[d]etour-health.conf'` → `0`).

**Плохо:** есть маркер `unsupported`, или все профили `0` И живая clash-проба из
Шага 1 провалилась → см. ремедиацию в Шаге 1.

## Шаг 3 — расписание (раз в час)

На KeeneticOS системный `crond` не работает, поэтому задачи гоняет демон
`detour-cron`. Проверьте, что health в него попал:

```sh
grep -n detour-health /opt/sbin/detour-cron     # должна быть строка ~раз в час
ps w | grep -c '[d]etour-cron'                   # 1 = демон жив
```

## Шаг 4 — панель

Откройте «Статусы VPN»: у каждого профиля колонка **«Работает»** (✓/✗) и в колонке
«Проверен» две строки — когда мерился пинг и когда гонялась проверка ВПН. Если
clash-API нет (Шаг 1), панель эти элементы **прячет** автоматически — это и есть
штатная деградация, ничего чинить не нужно.

## Если что-то пошло не так — что прислать

1. Вывод блока из **Шага 1** (есть ли `with_clash_api` и ответил ли clash `/version`).
2. Из **Шага 2**: сколько `работает` из всех, есть ли маркер `unsupported`.
3. Есть ли строка `detour-health` в `detour-cron` (**Шаг 3**).
4. При ошибках — хвост `/opt/var/log/detour-health.log` и `/tmp/cltest.log`.
