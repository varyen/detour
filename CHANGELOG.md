# Changelog

Все заметные изменения проекта Detour.

Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.0.0/),
версионирование — [SemVer](https://semver.org/lang/ru/).

## [1.4.0] — 2026-06-04

### Добавлено — VPN-сервер: входящие клиенты через sing-box/zapret

- **Маршрутизация road-warrior VPN-клиентов через обход.** Новая опция
  `vpn_redirect_ifaces` в `settings.json` (например `"wgserver"`): трафик
  клиентов, подключённых к WireGuard-серверу роутера, проходит те же правила
  `sing-box` и `zapret-tpws`, что и LAN — split-routing по `proxy-domains`,
  DPI-bypass, оба режима (`proxy-list`/`all-except`), multi-instance. Зеркалит
  nat PREROUTING правила `br-lan` на VPN-интерфейс. **По умолчанию выключено**
  (пустой ключ) — другие роутеры не затрагиваются.
- Реализовано в `sing-box.initd`, `zapret-tpws.initd` (start/stop) и
  `detour-api`; режим «Все через VPN» теперь покрывает и VPN-интерфейсы, а
  `write_settings` сохраняет ключ при перезаписи settings.json.
- Живая валидация на home (GL-BE9300, `wgserver` 10.1.0.0/24): WG-клиент
  получает выход через VPN-сервер и доступ к хостам LAN `192.168.8.x`.

### Добавлено — Hosts-DNS (приоритетный hosts-файл)

- **Вкладка «Hosts-DNS»** в панели: загрузка/редактирование приоритетного
  hosts-файла через dnsmasq `addn-hosts`, фильтрация пересечений с
  `proxy-domains.list` / `whitelist-domains.list`, пользовательские записи.
- `/usr/sbin/detour-hosts` + `/etc/init.d/detour-hosts` (переприменение при
  загрузке), API-обработчики в `detour-api`, cron обновления раз в 12 ч.
  Платформы OpenWrt и KeeneticOS/Entware.

### Изменено — «killswitch» → «Все через VPN»

- Внутреннее переименование во всех идентификаторах: chain
  `SINGBOX_KS` → `SINGBOX_ALLVPN`, эндпоинты `killswitch_on/off` →
  `allvpn_on/off`, JSON-поле `killswitch` → `allvpn`, CSS/JS
  (`ks-*`/`toggleKillswitch` → `allvpn-*`/`toggleAllvpn`). Видимая надпись в
  UI («Все через VPN») не менялась.

## [1.3.2] — 2026-06-03

### Изменено — автопроверка обновлений теперь опциональна и выключена по умолчанию

Фоновая автопроверка (cron раз в 6 ч, `detour-update check`) была только
**проверкой-уведомлением** — показывала плашку «доступно обновление» в шапке,
ничего не устанавливая (установка всегда ручная). Тем не менее это фоновый
запрос к GitHub каждые 6 ч. Теперь это **opt-in и по умолчанию выключено**.

- **Тумблер «Автопроверка обновлений (раз в 6 ч)»** в модале обновления панели —
  по умолчанию выключен; включается/выключается из UI.
- **`detour-update autocheck on|off|status`** — управляет 6h-cron и пишет флаг
  `AUTO_CHECK` в `update.conf`. Ручная кнопка «Проверить» работает всегда.
- **postinst `.ipk`** ставит cron только при `AUTO_CHECK=1` — выбор сохраняется
  при апгрейде; на свежей установке (и при апгрейде со снятым флагом) cron не
  добавляется.
- **`deploy_router.py --enable-autocheck`** (по умолчанию off) + `AUTO_CHECK` в
  `update.conf`.

## [1.3.1] — 2026-06-03

### Добавлено — экспорт профилей в виде ссылок

- **Экспорт профилей как отдельные ссылки.** Кнопка «Экспорт профилей» теперь
  открывает модал с выбором формата: **🔗 Ссылки** (по одной на строку) или
  **JSON** (прежний полный экспорт). Поддерживаются `vless://`, `trojan://`,
  `vmess://`, `ss://` и прокси `https://` / `http://` / `socks5://` /
  `socks4://` / `socks4a://`. Любую ссылку можно скопировать или скачать `.txt`.
- **Просмотр ссылки одного профиля.** Кнопка «🔗 Ссылка» рядом с
  «Редактировать» показывает ссылку выбранного профиля; в модале редактирования
  — кнопка «🔗 Показать ссылку» строит ссылку из введённых данных (URI / JSON /
  подписка / прокси).
- Конвертер `outbound → URI` — точная инверсия существующих парсеров импорта
  (round-trip проверен на всех протоколах). WireGuard и неизвестные типы
  помечаются как «без ссылки» (для них стандартного URI нет — используйте JSON).
- Копирование работает и по HTTP в LAN (где `navigator.clipboard` недоступен) —
  через `execCommand`-фолбэк; UTF-8-имена и IPv6-хосты обрабатываются корректно.

### Исправлено

- **`deploy_router.py`: панель деплоится из канонического `router_files/`.**
  `step_panel` грузил `index.html` и `detour-api` из устаревшего снапшота
  `router-backup/` — та же ловушка, что ломала релиз 1.2.0. Теперь источник —
  `router_files/` (как и в `build_release.py`).

## [1.3.0] — 2026-06-03

### Изменено — sing-box из собственного opkg-фида (GL.iNet/OpenWrt)

Дистрибутивный фид GL.iNet застрял на **sing-box 1.8.10** (ломает схему конфига
1.13.x), поэтому раньше бинарник бандлился пакетом `detour-bins` (22 МБ). Теперь
GL.iNet приходит к той же модели, что и Keenetic: **бинарник из opkg**.

- **`detour-bins` и `detour-full`-бандл удалены.** Панель `detour` теперь
  slim-`.ipk` (~200 КБ) с `Depends: sing-box` и **bundled `tpws-zapret`**
  (~110 КБ; zapret нет ни в одном фиде).
- **Собственный публичный opkg-фид** (`build_feed.py` → ветка `feed` репо
  `varyen/detour`) раздаёт `sing-box` 1.13.x как `Architecture: all`; по версии
  бьёт дистрибутивный 1.8.10, поэтому `opkg install sing-box` ставит наш.
  Раздаётся по HTTPS (`raw.githubusercontent.com/.../feed/aarch64`), `Packages`
  подписан usign.
- **`deploy_router.py` / `detour-update`** прописывают фид в
  `/etc/opkg/customfeeds.conf` и ставят sing-box через opkg
  (`ensure_feed`/`ensure_singbox`); порядок: фид → sing-box → панель. Восстановление
  после sysupgrade пере-добавляет фид и переустанавливает панель.
- **`detour-update bins-*`** теперь работают через opkg (`opkg upgrade sing-box`),
  а не качают `detour-bins` с GitHub; `bins-apply-local` убран. Панель читает
  версию из `opkg list-installed sing-box`. Модал «Обновление sing-box» — без
  загрузки `.ipk` из файла.

## [1.2.1] — 2026-06-01

### Исправлено

- **Релизный пакет панели теперь содержит актуальный UI.** `build_release.py`
  собирал `index.html` и `detour-api` из устаревшего снапшота `router-backup/`,
  а не из `router_files/`, поэтому панель 1.2.0 ставилась со старым интерфейсом
  (без единой кнопки Старт/Стоп, логотипа и т.д.). Теперь сборка читает единый
  источник `router_files/`.

### Изменено

- **Единый релиз для всех платформ.** `build_release.py --version X --publish`
  собирает, подписывает (usign) и публикует **оба** пакета в один GitHub-релиз:
  `detour_*.ipk` (OpenWrt/GL.iNet/Flint) и `detour-keenetic_*.ipk`
  (Keenetic/Entware). Один источник `router_files/` — UI правится один раз.
  `keenetic/build-ipk.py` тоже подписывает свой `.ipk`.

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

[1.2.1]: https://github.com/varyen/detour/releases/tag/v1.2.1
[1.2.0]: https://github.com/varyen/detour/releases/tag/v1.2.0
[1.1.0]: https://github.com/varyen/detour/releases/tag/v1.1.0
[1.0.0]: https://github.com/varyen/detour/releases/tag/v1.0.0
