# Changelog

Все заметные изменения проекта Detour.

Формат основан на [Keep a Changelog](https://keepachangelog.com/ru/1.0.0/),
версионирование — [SemVer](https://semver.org/lang/ru/).

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

[1.0.0]: https://github.com/varyen/detour/releases/tag/v1.0.0
