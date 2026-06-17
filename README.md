# Detour

> Веб-панель управления обходом блокировок для роутеров GL.iNet / OpenWrt.
> Два движка под одним SPA-интерфейсом — **sing-box** (Trojan/VLESS-прокси) и
> **zapret-tpws** (DPI-bypass) — с самообновлением по подписанным `.ipk`-релизам.

**Версия:** [`1.14.0`](VERSION) · **История изменений:** [`CHANGELOG.md`](CHANGELOG.md)

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
расходов.

## Целевая платформа

| Параметр    | Значение                                                       |
| ----------- | -------------------------------------------------------------- |
| Референс    | GL.iNet GL-BE9300 (Flint 3), SoC IPQ9300 (ipq53xx), Cortex-A53 |
| Прошивка    | GL.iNet 4.8.4 (OpenWrt 23.05-SNAPSHOT)                         |
| Arch (opkg) | `aarch64_cortex-a53`                                           |
| libc        | **musl** (не glibc — бинарники должны быть musl)               |
| Shell       | BusyBox **ash** (не bash)                                      |
| Lua         | 5.1                                                            |
| Firewall    | **nftables** (`table inet fw4`, `input` policy drop)           |

Парк (настраивается в `routers.local.json`):

| Имя              | Хост        | Железо                             | Firewall     |
| ---------------- | ----------- | ---------------------------------- | ------------ |
| `home` (default) | 192.168.8.1 | GL-BE9300 (ipq53xx, OpenWrt 23.05) | nftables fw4 |
| `flint2`         | 192.168.9.1 | GL-MT6000 (MT7986, OpenWrt 21.02)  | iptables fw3 |

`deploy_router.py` сам определяет особенности устройства (наличие `nft`,
busybox-апплетов и т.п.) и подстраивает деплой.

## Структура репозитория

| Путь                   | Назначение                                                                                |
| ---------------------- | ----------------------------------------------------------------------------------------- |
| `router_files/`        | Скрипты, деплоящиеся на роутер: init.d, CGI, updater, shim'ы.                             |
| `router-backup/`       | Зеркало живого состояния роутера (gitignored). Источник конфигов и бинарников при сборке. |
| `build_release.py`     | Сборка подписанных `.ipk` панели (`detour` + `detour-keenetic`).                          |
| `build_feed.py`        | Сборка/публикация opkg-фида с `sing-box` (ветка `feed`).                                  |
| `deploy_router.py`     | Унифицированный деплой / синхронизация на роутер по SSH.                                  |
| `deploy_lan_proxy.py`  | Деплой отдельного LAN-прокси-сценария.                                                    |
| `update_backups.py`    | Снять текущее состояние роутера в `router-backup/`.                                       |
| `router_config.py`     | Загрузка конфигов роутеров и SSH-хелперы (Paramiko).                                      |
| `usign_compat.py`      | Python-реализация usign-подписи (Ed25519).                                                |
| `keys/`                | Ключи подписи релизов (приватный — gitignored).                                           |
| `routers.example.json` | Шаблон конфигурации роутеров (копируется в `routers.local.json`).                         |

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

Панель — это slim-`.ipk` (init.d, CGI, HTML, Lua, updater) **+ bundled
`tpws-zapret`** (~110 КБ; zapret нет ни в одном opkg-фиде). Бинарник **`sing-box`
не входит в пакет** — панель объявляет `Depends: sing-box`, а сам бинарник
приходит из **нашего публичного opkg-фида**:

- **`detour`** — панель для OpenWrt/GL.iNet; **`detour-keenetic`** — для
  Keenetic/Entware (там sing-box берётся из Entware `sing-box-go`).
- **opkg-фид** (`build_feed.py` → ветка `feed` репо `varyen/detour`) раздаёт
  `sing-box` 1.13.x как `Architecture: all`. Дистрибутивный фид GL.iNet застрял
  на 1.8.10 (ломает схему конфига 1.13.x), поэтому держим свой. Фид по версии
  бьёт дистрибутивный, так что `opkg install sing-box` ставит именно наш.

### Сборка

```bash
# Панель (+ Keenetic) и публикация ассетов в GitHub Release:
python3 build_release.py --version 1.0.0 --publish

# Фид sing-box (при бампе версии sing-box):
python3 build_feed.py --version 1.13.2 --publish
```

### Установка / обновление

- **Из панели:** плашка «Доступно обновление» → «Установить» (скачивает панель
  из GH Releases и ставит через opkg). Чип версии в шапке → «Обновление
  sing-box» (`opkg update && opkg upgrade sing-box`).
- **По SSH вручную** — командами `detour-update` на роутере:

  ```sh
  /usr/sbin/detour-update check          # запросить GH, обновить статус
  /usr/sbin/detour-update apply          # ensure feed+sing-box, поставить панель
  /usr/sbin/detour-update bins-apply     # opkg update && opkg upgrade sing-box
  /usr/sbin/detour-update rollback       # откат к предыдущей версии
  /usr/sbin/detour-update status         # JSON со статусом
  ```

Фид прописывается в `/etc/opkg/customfeeds.conf` автоматически (`deploy_router.py`
и `detour-update`); он должен быть прописан до установки панели, иначе
`Depends: sing-box` не разрешится. Подписи (usign) проверяются против ключа,
запиннингованного на роутере (`/etc/detour/release.usign.pub`); приватный ключ
(`keys/`) — только на build-машине.

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
