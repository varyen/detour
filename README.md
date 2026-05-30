# Detour

> Веб-панель управления обходом блокировок для роутеров GL.iNet / OpenWrt.
> Два движка под одним SPA-интерфейсом — **sing-box** (Trojan/VLESS-прокси) и
> **zapret-tpws** (DPI-bypass) — с самообновлением по подписанным `.ipk`-релизам.

**Версия:** [`1.0.0`](VERSION) · **История изменений:** [`CHANGELOG.md`](CHANGELOG.md)

---

## Что это

Detour — самохостируемая система обхода блокировок для роутеров GL.iNet
(референс-устройство — GL-BE9300 / Flint 3 на OpenWrt 23.05). Весь трафик
выбранных доменов и подсетей прозрачно маршрутизируется одним из двух способов,
а управление вынесено в лёгкую веб-панель прямо на роутере.

- **Веб-панель (SPA)** — одностраничное приложение в `/www/detour/`, отдаётся
  через uhttpd на порту 8080. Режимы маршрутизации, списки доменов, профили
  прокси, статус сервисов, обновления — всё отсюда.
- **Shell CGI API** (`/www/cgi-bin/detour-api`) — бэкенд панели на чистом
  BusyBox ash + Lua (для JSON). Без Python/Node/PHP на роутере.
- **sing-box** (порт 12345) — прозрачный прокси (Trojan/VLESS) для доменов,
  которые нужно гнать через зарубежный сервер.
- **zapret-tpws** (порт 1081) — DPI-bypass: обход блокировок без внешнего
  сервера, прямым соединением с DPI-трюками (фрагментация и т.п.).
- **Самообновление** — подписанные usign `.ipk`, ставятся через opkg. Роутер
  раз в 6 ч проверяет GitHub Releases и показывает плашку в шапке панели.

## Архитектура маршрутизации

```text
Клиент → DNS (dnsmasq + ipset) → iptables nat PREROUTING:
  1. IP в ipset zapret_domains          → REDIRECT :1081 (zapret-tpws, DPI-bypass)
  2. TCP под текущим режимом sing-box    → REDIRECT :12345 (прокси через сервер)
  3. всё остальное                       → напрямую
```

dnsmasq при резолве доменов из списков складывает IP в соответствующие ipset'ы;
nat-правила перенаправляют только нужный трафик, остальное идёт без накладных
расходов. Подробнее — в [`CLAUDE.md`](CLAUDE.md).

## Целевая платформа

| Параметр | Значение |
| --- | --- |
| Референс | GL.iNet GL-BE9300 (Flint 3), SoC IPQ9300 (ipq53xx), Cortex-A53 |
| Прошивка | GL.iNet 4.8.4 (OpenWrt 23.05-SNAPSHOT) |
| Arch (opkg) | `aarch64_cortex-a53` |
| libc | **musl** (не glibc — бинарники должны быть musl) |
| Shell | BusyBox **ash** (не bash) |
| Lua | 5.1 |
| Firewall | **nftables** (`table inet fw4`, `input` policy drop) |

Парк (настраивается в `routers.local.json`):

| Имя | Хост | Железо | Firewall |
| --- | --- | --- | --- |
| `home` (default) | 192.168.8.1 | GL-BE9300 (ipq53xx, OpenWrt 23.05) | nftables fw4 |
| `flint2` | 192.168.9.1 | GL-MT6000 (MT7986, OpenWrt 21.02) | iptables fw3 |

`deploy_router.py` сам определяет особенности устройства (наличие `nft`,
busybox-апплетов и т.п.) и подстраивает деплой.

## Структура репозитория

| Путь | Назначение |
| --- | --- |
| `router_files/` | Скрипты, деплоящиеся на роутер: init.d, CGI, updater, shim'ы. |
| `router-backup/` | Зеркало живого состояния роутера (gitignored). Источник конфигов и бинарников при сборке. |
| `build_release.py` | Сборка подписанных `.ipk` (`detour` + `detour-bins`) и full-бандла. |
| `deploy_router.py` | Унифицированный деплой / синхронизация на роутер по SSH. |
| `deploy_lan_proxy.py` | Деплой отдельного LAN-прокси-сценария. |
| `update_backups.py` | Снять текущее состояние роутера в `router-backup/`. |
| `router_config.py` | Загрузка конфигов роутеров и SSH-хелперы (Paramiko). |
| `usign_compat.py` | Python-реализация usign-подписи (Ed25519). |
| `keys/` | Ключи подписи релизов (приватный — gitignored). |
| `routers.example.json` | Шаблон конфигурации роутеров (копируется в `routers.local.json`). |

## Быстрый старт

### Требования (рабочая машина)

- Python 3.8+ с виртуальным окружением; `pip install paramiko cryptography`
- SSH-доступ к роутеру под `root`

### Первый деплой

```bash
# 1. Конфигурация роутеров (хост, пароли, GitHub-токен для self-update)
cp routers.example.json routers.local.json
$EDITOR routers.local.json

# 2. Полный деплой: init.d, конфиги, панель, бинарники
python3 deploy_router.py --router home --full
```

После этого панель доступна на `http://192.168.8.1:8080/detour/`.

## Релизы и самообновление

Релиз разнесён на **два пакета**, чтобы обновление панели не задевало тяжёлые
бинарники:

- **`detour`** — slim-панель (init.d, CGI, HTML, Lua-скрипты, updater), ~90 КБ.
  Обновляется часто. **Бинарников не содержит.**
- **`detour-bins`** — только `sing-box` + `tpws-zapret` (~22 МБ),
  версионируется отдельно (`/etc/detour/bins-version`).

Плюс `detour-full-vX.Y.Z.tar.gz` — оффлайн-установщик одним файлом (оба `.ipk`
с подписями + `install.sh`).

### Сборка

```bash
# Только панель (обновление панели):
python3 build_release.py --version 1.0.0

# Панель + bins + full-бандл, с публикацией ассетов в GitHub Release:
python3 build_release.py --version 1.0.0 --bins-version 1.0.0 --publish
```

### Установка / обновление

- **Из панели:** плашка «Доступно обновление» → «Установить» (скачивает из GH
  Releases и ставит через opkg). Чип версии bins в шапке → «Обновление
  бинарников».
- **По SSH вручную** — командами `detour-update` на роутере:

  ```sh
  /usr/sbin/detour-update check          # запросить GH, обновить статус
  /usr/sbin/detour-update apply          # скачать + проверить + поставить панель
  /usr/sbin/detour-update bins-apply     # то же для detour-bins
  /usr/sbin/detour-update rollback       # откат к предыдущей версии
  /usr/sbin/detour-update status         # JSON со статусом
  ```

Подписи проверяются usign против публичного ключа, запиннингованного на роутере
(`/etc/detour/release.usign.pub`). Приватный ключ (`keys/`) — только на
build-машине и в репозиторий не попадает.

## Критические ограничения

- **MPTCP должен быть выключен** (`net.mptcp.enabled=0`, `/etc/sysctl.d/99-mptcp.conf`).
  Реализация MPTCP в QSDK-ядре сломана — без этого TCP-серверы зависают в
  `SYN_RECEIVED`.
- **Только nftables для INPUT.** Основной файрвол — `table inet fw4` с
  `input policy drop`; правила iptables INPUT не работают. Порт открывается через
  `nft insert rule inet fw4 input_lan tcp dport <PORT> accept`. iptables
  используется только для nat REDIRECT.
- **Только musl-бинарники** (не glibc).
- **BusyBox ash:** без bashisms (`[[`, массивы, process substitution), без
  `nohup`/`socat`/`tcpdump`, `curl` без `-x`, `nc` только как клиент.

## Лицензия

[MIT](LICENSE) © 2026 varyen.
