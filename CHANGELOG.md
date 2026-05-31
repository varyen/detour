# Changelog

Все заметные изменения проекта Detour.

Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.0.0/),
версионирование — [SemVer](https://semver.org/lang/ru/).

## [1.2.0] — 2026-06-01

### Порт на Keenetic / Entware (KeeneticOS, mipsel)

Первый рабочий порт всего стека на **KeeneticOS + Entware** (MT7621, mipsel),
проверен на реальном железе (sing-box 1.13.3, ABI в порядке).

- **Slim-пакет.** `detour-keenetic` больше не бандлит sing-box — он ставится из
  фида Entware по `Depends: sing-box` (пакет `sing-box-go` → `/opt/bin/sing-box`,
  ABI гарантирован сборкой). Размер пакета упал с **22.5 МБ до ~200 КБ**; в пакете
  остаётся только `tpws` (zapret в фиде Entware нет).
- **Обновление sing-box из OPKG по кнопке** — `opkg update && opkg upgrade
  sing-box-go`, отключение собственного автозапуска пакета и перезапуск сервиса
  (только Keenetic).
- **`all-except` маршрутизация** в firewall-хуке Keenetic: REDIRECT всего LAN TCP
  на sing-box, кроме приватных/loopback/CGNAT, апстрим-сервера и whitelist.
- **Подписки** портированы: встроенный pure-Lua `cjson.safe` (C-шного `lua-cjson`
  в фиде нет; проверен против системного lua-cjson) + шим путей под `/opt`.
- Панель отдаётся на `/detour/`; `detour-api` ремапит все пути под `/opt` на
  Entware (на OpenWrt — инертно).

### Изменено (обе платформы)

- **Подпись `.ipk` опциональна** — обновление панели и `detour-bins` больше не
  требует `.ipk.sig`. С подписью — проверка + бэкап, без неё — установка напрямую
  через opkg. Это же включает обновление панели на Keenetic.
- **Одна кнопка Старт/Стоп** для sing-box и zapret (переключается по состоянию).
- **«Kill Switch» → «Все через VPN»** — понятнее.
- **Логотип в шапке** заменён на favicon Detour.
- `status` теперь отдаёт поле `platform` (`openwrt` | `keenetic`).

### Исправлено

- **PATH в CGI/хуках/init.d** — lighttpd и NDM дают минимальный PATH, а инструменты
  Entware лежат в `/opt/bin:/opt/sbin`. Без экспорта PATH `sed`/`openssl`/`iptables`
  молча не находились → ложные `auth`-401 и отсутствие правил файрвола.
- Зависимости пакета: убраны `start-stop-daemon` (апплет busybox, не отдельный
  пакет) и `lua-cjson` (нет в фиде), из-за которых opkg отказывался ставить пакет.

## [1.1.0] — 2026-05-31

### Добавлено / Изменено

- Переработанный UI панели; профили **HTTP/SOCKS-прокси**; пер-сайт **route-map**
  (разные сайты через разные профили/VPN); **подписки** с автообновлением; цепочки
  прокси/VPN; режимы «только список» и «всё кроме whitelist».

## [1.0.0] — 2026-05-30

Первый публичный релиз Detour — веб-панель управления обходом блокировок для
роутеров GL.iNet / OpenWrt.

### Добавлено

- **Веб-панель (SPA)** на `/www/detour/` (uhttpd, порт 8080): режимы
  маршрутизации, редакторы списков доменов, управление профилями прокси, статус
  сервисов и обновления.
- **Shell CGI API** (`/www/cgi-bin/detour-api`) — бэкенд панели на BusyBox ash
  и Lua, без внешних рантаймов на роутере.
- **sing-box** (порт 12345) — прозрачный прокси Trojan/VLESS для маршрутизации
  выбранных доменов и подсетей через зарубежный сервер (режим redirect).
- **zapret-tpws** (порт 1081) — DPI-bypass прозрачный прокси без внешнего
  сервера.
- **Маршрутизация** через dnsmasq + ipset и iptables nat PREROUTING:
  zapret-домены → `:1081`, домены под режимом sing-box → `:12345`, остальное —
  напрямую.
- **Самообновление**: подписанные usign `.ipk`, установка через opkg, проверка
  GitHub Releases по cron (раз в 6 ч) с плашкой в шапке панели.
- **Двухпакетная схема релиза:** slim-панель `detour` (~90 КБ) и отдельно
  версионируемые бинарники `detour-bins` (`sing-box` + `tpws-zapret`, ~22 МБ),
  плюс оффлайн-установщик `detour-full-vX.Y.Z.tar.gz`.
- **Утилита `detour-update`** на роутере: `check` / `apply` / `bins-apply` /
  `rollback` / `status` / `selftest` и установка из локальных файлов.
- **Инструменты на рабочей машине:** `deploy_router.py` (унифицированный деплой
  по SSH с автоопределением особенностей устройства), `build_release.py`
  (сборка и публикация подписанных пакетов), `update_backups.py` (снятие
  состояния роутера), `usign_compat.py` (usign-подпись на Python).
- **Поддержка парка из нескольких роутеров** через `routers.local.json`
  (референс: GL-BE9300 на nftables fw4 и GL-MT6000 на iptables fw3).
- **Защитные механизмы платформы:** отключение сломанного MPTCP
  (`net.mptcp.enabled=0`), открытие портов через nftables `fw4`, hotplug-guard
  для восстановления правил после смены интерфейсов.

[1.2.0]: https://github.com/varyen/detour/releases/tag/v1.2.0
[1.1.0]: https://github.com/varyen/detour/releases/tag/v1.1.0
[1.0.0]: https://github.com/varyen/detour/releases/tag/v1.0.0
