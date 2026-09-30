# Detour на iOS

**Собирается и запускается, но VPN-туннель не проверен.** Сборка — `client/scripts/ios/build.sh`
(Mac с полным Xcode), результат — неподписанный `releases/client/Detour-<версия>-unsigned.ipa`.
Проверено 30.09.2026 на ВМ (Intel, macOS 26, Xcode 26.6) и на iPad Air 2 (iPadOS 15.7.2):
приложение ставится, стартует, панель рисуется («ios · sing-box 1.13.21 · панель 2.3.0»),
профили добавляются. Туннель не поднимается: кнопка «Запустить» ничего не делает, журнал
sing-box пуст. Бинарники собраны без подписи, поэтому entitlement'ов (в том числе Network
Extension) в них нет; что причина именно в этом — предположение, не проверено.

Чтобы VPN на iOS заработал, нужна подпись с entitlement'ом `packet-tunnel-provider`: его выдаёт
Apple по платному аккаунту Developer (99 $ в год). Без аккаунта неподписанный `.ipa`
годится только показать панель.

## Как устроено

Как на Android, ядро (`detour-core`) работает в процессе приложения, а «канал»
до него — прямой вызов функции (`client/app/src/lib.rs`, модуль `inproc`).
Разница в туннеле:

- на Android его поднимает `VpnService` в том же процессе;
- на iOS туннель живёт в **отдельном процессе** — расширении
  `NEPacketTunnelProvider` (`Extension/PacketTunnelProvider.swift`) с libbox
  внутри. Приложение только включает и выключает его через
  `NETunnelProviderManager` (`App/DetourTunnel.swift`).

```
панель ─invoke→ detour-core (Rust, в приложении)
                    │ engine::tunnel (client/app/src/ios.rs)
                    ▼ extern "C" ⇄ @_cdecl
              DetourTunnel.swift ── NETunnelProviderManager ──▶ расширение
                                                                 PacketTunnelProvider
                                                                 └ libbox (sing-box)
```

Конфиг передаётся параметром запуска (`startVPNTunnel(options:)`), а не путём
к файлу: у расширения своя песочница, файлы приложения ему не видны. Clash API
sing-box слушает 127.0.0.1 внутри расширения — ядро в приложении ходит к нему
по loopback так же, как на остальных платформах (счётчики трафика, задержки).

Отличия от Android, которые учтены в коде:

- дескриптор TUN libbox находит сам (`LibboxGetTunnelFileDescriptor`), от
  `openTun` нужно только применить `NEPacketTunnelNetworkSettings`;
- сокеты расширения идут мимо собственного туннеля сами — «защищать» их, как
  `VpnService.protect()`, не нужно;
- обхода DPI нет: iOS не даёт запускать сторонние процессы, tpws негде
  поднять. Панель это видит — движок обхода «не установлен»;
- проверок профилей тоже нет (нужен второй экземпляр sing-box).

## Что лежит в каталоге

- `App/`, `Extension/` — Swift-файлы, entitlements и Info.plist расширения;
- `project.yml` — спецификация xcodegen: заменяет сгенерированную tauri-cli, добавляет цель
  `DetourTunnel`, Libbox, `libresolv`, отключает подпись (`@VERSION@` подставляет build.sh);
- `Libbox-Info.plist` — настоящий Info.plist для Libbox.framework;
- `../.cargo/config.toml` и `../../.cargo/config.toml` — флаг линковки для Rust (см. ниже).

## Подпись (не проверялось)

Нужна команда разработчика с включённой возможностью Network Extension. Предположительно:
задать `APPLE_DEVELOPMENT_TEAM`, убрать из `project.yml` четыре настройки подписи
(`CODE_SIGNING_ALLOWED`, `CODE_SIGNING_REQUIRED`, `CODE_SIGN_IDENTITY`, `CODE_SIGN_STYLE`) —
тогда `cargo tauri ios build` экспортирует подписанный `.ipa` сам. Ни разу не запускалось.

## Грабли (каждая съела цикл сборки)

- **Имена методов libbox в Swift.** Сгенерированный модуль переименовывает их API notes'ами
  независимо от ObjC-заголовка: в Swift это `autoDetectControl(_:)` и
  `usePlatformAutoDetectControl()`, а не `…InterfaceControl`. Подсказка — в тексте ошибки
  компилятора («has been renamed to»).
- **`libresolv.tbd`.** Go-резолвер (cgo) без неё не линкуется: `_res_9_ninit` и другие.
- **`-undefined dynamic_lookup`.** У крейта `crate-type = ["staticlib", "cdylib", "rlib"]`
  (cdylib нужен Android); cdylib линкуется сразу целиком, а `detour_tunnel_*` определены в
  Swift и появляются только при сборке приложения. Флаг разрешает их на этапе cargo и откладывает
  на загрузку; staticlib он не касается.
- **Пустой Info.plist в Libbox.framework.** gomobile кладёт `<dict/>` без ключей, Xcode 26
  отказывается его внедрять («was empty», «expected Info.plist at the root level»).
  `build.sh` подменяет его файлом `Libbox-Info.plist` в обоих срезах.
- **Голый `xcodebuild` не работает.** Фаза «Build Rust Code» подключается обратно к
  родительскому `cargo tauri` по WebSocket; без него падает на чтении адреса сервера.
  Собирать только через `cargo tauri ios build`.
- **Экспорт падает** («exportArchive No Team Found»): без команды разработчика `.ipa`
  экспортировать нельзя. Нужен лишь `.xcarchive`, `.ipa` из него собирает build.sh.
- **brew на ВМ.** `cargo tauri ios init` пытается доставить через brew `xcodegen`,
  `libimobiledevice` и `cocoapods`. Для нашей macOS готовых бутылок нет, и brew собирает всё из
  исходников (openssl — 17 минут, Python с LTO — часы). Ставить заранее: `xcodegen` — из
  исходников через SPM (~11 минут), CocoaPods — `gem install` под Ruby >= 3.0 (системный
  2.6 не подходит для `ffi`; годится portable Ruby из каталога brew), `libimobiledevice` — бутылка.
- **Диск.** Контейнер ВМ почти полон; Rust-кэш, DerivedData и Homebrew вместе съедают
  несколько ГБ («database or disk is full» в xcodebuild). Перед сборкой смотреть `df -h /`.
