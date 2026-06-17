# Keenetic: выпуск HTTPS-сертификата для панели Detour (HTTP-01 + проброс :80)

> Это инструкция для **LLM-агента с SSH-доступом к Keenetic** (Entware под `/opt`).
> Цель — выпустить Let's Encrypt сертификат для домена и отдавать панель Detour по
> **HTTPS** (без этого не работают push-уведомления — браузер требует защищённый
> контекст). Версия панели — **detour-keenetic 1.12.0+** (есть `/opt/sbin/detour-cert`).

## Контекст и в чём загвоздка Keenetic

- Панель Detour живёт в **отдельном Entware-lighttpd на :8080** (не в веб-сервере
  KeeneticOS).
- **:80 и :443 занимает сам KeeneticOS** (его веб-интерфейс) — туда ACME-challenge не
  положить.
- Поэтому используем **HTTP-01 webroot через наш lighttpd** + **проброс порта**:
  внешний **WAN :80 → LAN-IP роутера : 8080** (lighttpd). Тогда challenge
  `http://домен/.well-known/acme-challenge/...` доходит до нашего lighttpd, который его
  отдаёт. На устройстве НЕТ `socat`, поэтому standalone-режим acme.sh невозможен —
  только webroot.
- Для доступа к панели по HTTPS поднимаем TLS-сокет на самом lighttpd (по умолчанию
  **:443**; если :443 занят KeeneticOS — берём **:8443**) и пробрасываем на него WAN.

`detour-cert` это уже умеет: пробу webroot он шлёт на порт lighttpd (`server.port` из
`/opt/etc/lighttpd/detour.conf`, обычно 8080), а не на :80. Твоя задача — подтвердить
окружение, настроить пробросы и прогнать выпуск (сначала на стейджинге).

> Безопасность: сначала всё гоняем на **staging LE** (`DETOUR_CERT_STAGING=1`) — это
> ненастоящий серт без лимитов; он ничего не ломает. Боевой выпуск — только после того,
> как стейджинг прошёл.

---

## Шаг 1 — прогони preflight-скрипт (он делает ВСЮ диагностику)

Вместо ручных проверок просто запусти скрипт — он read-only (ничего не меняет, только
кладёт и сразу удаляет тестовый токен), и в конце печатает вердикт + точные следующие
команды с уже подставленными значениями (LAN-IP, порт lighttpd, домен).

```sh
# Если панель уже обновлена до 1.12.0+ — скрипт уже на устройстве:
sh /opt/sbin/detour-cert-preflight ВАШ.ДОМЕН ВАШ@EMAIL

# Если файла нет (старая панель) — скопируй keenetic/detour-cert-preflight.sh на роутер и:
sh /tmp/detour-cert-preflight.sh ВАШ.ДОМЕН ВАШ@EMAIL
```

Скрипт проверяет: платформу, наличие `detour-cert`/`acme.sh`/`curl`/`openssl`/
`lighttpd-mod-openssl`, порт lighttpd, кто занимает :80/:443/:8080/:8443, жива ли панель,
**отдаёт ли lighttpd challenge-статику** (главное для HTTP-01), публичный IP, A-запись
домена и уже настроенные пробросы. В конце — секция `ИТОГО` (FAIL/WARN) и `ДЕЙСТВИЕ`.

**Как читать вывод:**

- `FAIL=0` → локально всё готово, переходи к пробросам (Шаг 2) и стейджинг-выпуску (Шаг 3).
- `[FAIL] lighttpd НЕ отдаёт challenge-статику` → панель не отдаёт `.well-known`. Проверь,
  что панель запущена (`/opt/etc/init.d/S51detour-panel status`); **пришли эту строку**.
- `[FAIL] A-запись … НЕ совпадает с публичным IP` → поправь A-запись домена.
- `[WARN] СЕРЫЙ IP (NAT/CGNAT)` → HTTP-01 снаружи не достучится; нужен DNS-01 (сообщи).
- `[WARN] НЕТ lighttpd-mod-openssl` → `opkg install lighttpd-mod-openssl` (для HTTPS-сокета).

**Если есть FAIL — пришли весь вывод скрипта.** Если `FAIL=0` — продолжай.

## Шаг 2 — A-запись домена

A-запись домена должна указывать на **публичный IP роутера** (тот, что виден из
интернета на WAN). Скрипт из Шага 1 уже показывает этот IP и сам сверяет A-запись
(`[OK]/[FAIL] A-запись …`). Если у провайдера «серый» IP (CGNAT) — HTTP-01 снаружи не
сработает; тогда нужен DNS-01 (другой путь, сообщите).

## Шаг 3 — пробросы портов в KeeneticOS

(Скрипт в секции `ДЕЙСТВИЕ` уже напечатал эти команды с подставленным LAN-IP.)

Нужно два правила переадресации (NAT) на WAN-интерфейсе:

| Внешний порт | → | Куда (LAN-IP роутера) | Зачем |
| --- | --- | --- | --- |
| TCP **80** | → | `127.0.0.1` / LAN-IP : **8080** | ACME HTTP-01 challenge (наш lighttpd) |
| TCP **443** | → | LAN-IP : **443** (или **8443**, см. Шаг 1) | доступ к панели по HTTPS |

**Вариант A — веб-интерфейс KeeneticOS (надёжнее):** «Сетевые правила» →
«Переадресация портов» → добавить два правила: вход TCP/80 → этот роутер, порт
назначения 8080; вход TCP/443 → этот роутер, порт 443 (или 8443).

**Вариант B — ndmc CLI** (подставьте имя WAN-интерфейса из Шага 1 вместо `ISP`):

```sh
# LAN-IP роутера (приватный адрес моста, обычно 192.168.1.1) — его печатает preflight-скрипт.
LANIP=$(ip -4 addr show 2>/dev/null | grep -oE 'inet (192\.168\.[0-9.]+|10\.[0-9.]+|172\.(1[6-9]|2[0-9]|3[01])\.[0-9.]+)' | awk '{print $2}' | head -1)
echo "LAN-IP роутера: $LANIP   (на него шлём challenge-порт)"
ndmc -c "ip static tcp ISP 80 $LANIP 8080 !\"detour-acme\""
ndmc -c "ip static tcp ISP 443 $LANIP 443 !\"detour-https\""
ndmc -c "system configuration save"
ndmc -c "show ip static"          # проверить, что правила появились
```

> ⚠️ Если KeeneticOS сам слушает WAN :80/:443 (например, удалённый доступ/KeenDNS в
> «прямом» режиме) — правило может не примениться или конфликтовать. Тогда:
> отключите удалённый веб-доступ KeeneticOS по этим портам, **или** используйте :8443
> для панели и WAN :443 → :8443.

## Шаг 4 — СТЕЙДЖИНГ-выпуск (главная проверка, без риска)

Это и есть сквозной тест проброса + challenge. Серт ненастоящий (staging), лимитов нет.

```sh
DETOUR_CERT_STAGING=1 /opt/sbin/detour-cert issue ВАШ.ДОМЕН ВАШ@EMAIL
echo "---- status ----"; /opt/sbin/detour-cert status
echo "---- log ----"; tail -25 /opt/var/log/detour-cert.log
```

**Хорошо:** `status` → `"last_result":"ok"`, появился `"expiry"`. В логе —
`issue OK`. Файлы серта: `ls -la /opt/etc/detour/ssl/` (fullchain.pem, privkey.pem,
combined.pem).

**Плохо и как читать лог:**

- `webroot prep failed … lighttpd not serving the challenge on :8080` → панель не
  отдаёт challenge локально. Вернитесь к Шагу 1 (строка `ACME-STATIC-OK`), убедитесь,
  что lighttpd запущен (`/opt/etc/init.d/S51detour-panel status`).
- `ACME issuance failed` + в логе acme.sh `Verify error … 404/timeout` → LE не достучался
  снаружи. Значит **проброс WAN :80 → :8080 не работает** (не то имя интерфейса, серый
  IP, провайдер режет :80, или KeeneticOS перехватывает :80). Проверьте Шаги 2–3.
- `acme.sh unavailable` → поставьте вручную: `opkg install acme.sh` и повторите.

> Перед боевым выпуском уберите staging-данные домена, чтобы боевой `--issue` создал
> чистый прод-серт: `rm -rf /opt/.acme.sh/ВАШ.ДОМЕН_ecc 2>/dev/null` (или
> `/root/.acme.sh/ВАШ.ДОМЕН_ecc`, смотря где acme.sh).

## Шаг 5 — БОЕВОЙ выпуск

```sh
/opt/sbin/detour-cert issue ВАШ.ДОМЕН ВАШ@EMAIL
/opt/sbin/detour-cert status
tail -15 /opt/var/log/detour-cert.log
```

Должно быть `"last_result":"ok"`. После этого `detour-cert` пишет
`/opt/etc/lighttpd/conf.d/detour-ssl.conf` (TLS-сокет) и перезапускает панель.

## Шаг 6 — проверка HTTPS-панели

```sh
echo "== что слушает lighttpd сейчас =="; netstat -tlnp 2>/dev/null | grep -E ':(443|8443|8080) '
echo "== конфиг lighttpd валиден? =="; LBIN=/opt/sbin/lighttpd; [ -x "$LBIN" ] || LBIN=/opt/bin/lighttpd; "$LBIN" -tt -f /opt/etc/lighttpd/detour.conf 2>&1 | tail -5
echo "== TLS локально (серт совпал с доменом?) =="
echo | openssl s_client -connect 127.0.0.1:443 -servername ВАШ.ДОМЕН 2>/dev/null | openssl x509 -noout -subject -dates 2>/dev/null
```

Если `:443` занят KeeneticOS и lighttpd не смог подняться на нём — отредактируйте
`/opt/etc/lighttpd/conf.d/detour-ssl.conf`, замените `:443` на `:8443`, перезапустите
`/opt/etc/init.d/S51detour-panel restart`, и пробросьте WAN :443 → :8443.

Снаружи откройте **https://ВАШ.ДОМЕН/detour/** в браузере — должна открыться панель с
валидным замком (для боевого серта; для staging браузер будет ругаться — это ожидаемо).

## Шаг 7 — push-уведомления

В панели по HTTPS: ⚙ → «Push-уведомления» → включить тумблер (браузер спросит
разрешение) → «Отправить тестовый push». Должно прийти уведомление. Push работает
**только** по этому HTTPS-домену.

## Шаг 8 — автопродление

```sh
grep -n 'detour-cert' /opt/sbin/detour-cron     # должна быть строка `detour-cert renew` (раз в сутки)
ps w | grep -c '[d]etour-cron'                   # 1 = демон-планировщик жив
```

Продление делает `detour-cert renew` (acme.sh `--cron`) из `detour-cron`; при продлении
серт переустанавливается и панель перезапускается автоматически. Проброс :80 должен
оставаться включённым (renew тоже идёт через HTTP-01).

## Что прислать, если не вышло

1. Вывод **Шага 1** целиком (особенно `ACME-STATIC-OK` и кто слушает :443).
2. `nslookup` домена (**Шаг 2**) — совпал ли IP с WAN роутера.
3. Какие правила проброса добавили (**Шаг 3**) и вывод `ndmc -c "show ip static"`.
4. Хвост `/opt/var/log/detour-cert.log` и вывод `/opt/sbin/detour-cert status` после
   стейджинг-попытки (**Шаг 4**).
5. `netstat` по портам и `lighttpd -tt` (**Шаг 6**), если HTTPS не поднялся.
