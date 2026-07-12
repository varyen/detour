# Detour

> Веб-панель управления обходом блокировок для роутеров GL.iNet / OpenWrt.
> Два движка под одним SPA-интерфейсом — **sing-box** (Trojan/VLESS-прокси) и
> **zapret-tpws** (DPI-bypass) — с самообновлением по подписанным `.ipk`-релизам.

**Версия:** [`1.26.1`](VERSION) · **История изменений:** [`CHANGELOG.md`](CHANGELOG.md)

---

## Установка

Если вы просто хотите поставить Detour на роутер, а не разрабатывать его,
используйте готовый `.ipk` из [GitHub Releases](https://github.com/varyen/detour/releases).

1. Скачайте нужный пакет из последнего релиза:
  `detour_X.Y.Z_all.ipk` для OpenWrt / GL.iNet или
  `detour-keenetic_X.Y.Z_all.ipk` для Keenetic / Entware.
2. Скопируйте файл на роутер в `/tmp/`.
3. Установите пакет одной командой для своей платформы.
4. Подождите 30-90 секунд: панель сама пропишет нужный `detour`-фид и
   подтянет `sing-box` + `tpws-zapret` в фоне.

### OpenWrt / GL.iNet

```sh
opkg install /tmp/detour_X.Y.Z_all.ipk
```

Логи установки:

- `/var/log/detour-install.log` — сам `postinst/prerm` пакета.
- `/var/log/detour-bootstrap.log` — фоновая подтяжка feed, `sing-box` и `tpws-zapret`.

### Keenetic / Entware

Entware должен быть уже установлен и смонтирован в `/opt`.

```sh
opkg install /tmp/detour-keenetic_X.Y.Z_all.ipk
```

Логи установки на флешке:

- `/opt/var/log/detour-install.log` — сам `postinst/prerm` пакета.
- `/opt/var/log/detour-bootstrap.log` — фоновая подтяжка feed, `sing-box` и `tpws-zapret`.

### Keenetic: вариант через `install/` на флешке

Если у вас уже есть рабочий USB-носитель с Entware и ваш Keenetic умеет
обрабатывать папку `install` при загрузке, можно положить
`detour-keenetic_X.Y.Z_all.ipk` туда и перезагрузить роутер. После установки
сама панель точно так же должна дописать feed и подтянуть `sing-box` +
`tpws-zapret` в фоне.

Этот путь в данном репозитории пока не провалидирован на живом Keenetic в этом
сеансе, поэтому относитесь к нему как к удобной альтернативе, а не как к уже
подтверждённому основному сценарию.

После установки панель доступна по адресу `http://<IP-роутера>:8080/detour/`.
Дальше обновляться проще всего из самой панели или командой `detour-update apply`
на роутере.

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

Панель — это slim-`.ipk` (init.d, CGI, HTML, Lua, updater). Бинарники **`sing-box`
и `tpws-zapret` в пакет не входят** — после установки панель сама добавляет
наш публичный opkg-фид и подтягивает их в фоне:

- **`detour`** — панель для OpenWrt/GL.iNet (фид `feed/aarch64`);
  **`detour-keenetic`** — для Keenetic/Entware (фид `feed/mipsel`: sing-box
  `-mipsle-softfloat-musl` + tpws `linux-mipsel`; Entware `sing-box-go` — только
  фолбэк). _(v1.22.0+ — раньше на Keenetic sing-box брался из Entware, а tpws был
  bundled.)_
- **opkg-фид** (`build_feed.py --arch {aarch64|mipsel}` → ветка `feed` репо
  `varyen/detour`) раздаёт `sing-box` 1.13.x как `Architecture: all`.
  Дистрибутивный фид GL.iNet застрял на 1.8.10 (ломает схему конфига 1.13.x), а
  Entware `sing-box-go` отстаёт — поэтому держим **свой фид на обе платформы**,
  он по версии бьёт оба, так что `opkg install sing-box` ставит именно наш.
- **zapret2 (`nfqws2`)** — только в `feed/aarch64`: на Keenetic нет NFQUEUE,
  движок не запускается, поэтому в mipsel-фид он не входит.

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

Фид прописывается автоматически самой панелью при первой установке, а затем
поддерживается `deploy_router.py` и `detour-update`. Если фоновый bootstrap не
дотянул бинарники, проверьте лог `detour-bootstrap.log` и затем вручную
запустите `detour-update bins-apply` и `detour-update tpws-apply`. Подписи
(usign) проверяются против ключа, запиннингованного на роутере
(`/etc/detour/release.usign.pub` или `/opt/etc/detour/release.usign.pub`);
приватный ключ (`keys/`) — только на build-машине.

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
