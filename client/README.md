# Detour — нативный клиент

Та же панель, что на роутере, только маршрутизирует не роутер, а само
устройство: sing-box поднимает TUN, и трафик уходит в него вместо iptables и
ipset. Панель (`panel/`, сборка `npm run build:client`) живёт внутри
приложения и ходит в ядро не по HTTP, а через IPC.

## Раскладка

| Каталог | Что |
| --- | --- |
| `crates/core` | ядро: хранилище, профили, цепочки, рендер конфига sing-box, подписки, проверки, счётчики, обход DPI |
| `crates/svc` | привилегированная служба (Windows) и демон launchd (macOS) |
| `app` | Tauri-приложение: окно с панелью и одна команда `api` |
| `installer` | NSIS для Windows, сборка `.app`/`.dmg` для macOS |
| `scripts` | e2e-наборы, стенд в Windows Sandbox, сборка Android |

## Как это собрано на каждой платформе

**Windows.** Ядро работает в службе, UI — обычным процессом без прав; между
ними именованный канал `\\.\pipe\detour`. Обход DPI — winws2 из zapret2
(WinDivert), домены обхода идут напрямую, а winws2 правит их на проводе.
Защита от утечки — правило брандмауэра, включённое на время, пока VPN должен
работать, но не работает.

**macOS.** То же, но демон launchd и unix-сокет `/var/run/detour.sock`;
перехватывать пакеты без своего kext нельзя, поэтому tpws поднимается
SOCKS-прокси, и домены обхода маршрутизируются в него. Защита от утечки —
якорь pf. На живом маке не проверено: машины нет.

**Android.** Службы нет — система не даст держать демона, а дескриптор TUN
выдаёт только `VpnService`. Поэтому ядро работает в процессе приложения, а
sing-box — библиотекой libbox рядом с ним; «канал» вырождается в вызов
функции. tpws лежит в APK как `jniLibs/*/libtpws.so`: запускать файлы из
каталога данных Android не разрешает. Своё приложение исключено из туннеля —
иначе соединения самого tpws возвращались бы в sing-box тем же правилом
обхода. Защиты от утечки нет: её роль играет системный переключатель
«блокировать соединения без VPN».

## Сборка

```sh
cd panel && npm run build:client      # панель для клиента
cd ../client
cargo test -p detour-core             # юнит-тесты ядра
npx tauri build                       # Windows/macOS
pwsh scripts/android/build.ps1 -Abi arm64 -Release
pwsh installer/build.ps1              # NSIS со службой и WebView2
bash installer/macos/build.sh         # Detour.app + .dmg
```

`scripts/android/build.ps1` сначала собирает `libbox.aar` из исходников
sing-box (готового артефакта нет) — это долго и разово, дальше берётся готовый.
Подробности про мины gomobile — в шапке самого скрипта.

## Проверки

```sh
node scripts/e2e-backend.mjs          # действия ядра через IPC
node scripts/e2e-subscriptions.mjs
node scripts/e2e-health.mjs
node scripts/e2e-live.mjs             # живой sing-box в dev-режиме
pwsh scripts/e2e-all.ps1              # всё сразу
pwsh scripts/sandbox/run.ps1          # чистая Windows в Sandbox: установка, TUN, kill-switch
```

TUN на рабочей машине не поднимается: служба с `--dev-http` его запрещает,
снимает запрет только `DETOUR_ALLOW_TUN=1` — и только на стенде.
