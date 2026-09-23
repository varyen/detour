# Detour на iOS — заготовка

**Не собиралось и не запускалось.** Для iOS нужен Mac с полным Xcode (iOS SDK)
и платный аккаунт Apple Developer: без него Network Extension не подписать, а
без Network Extension VPN на iOS не сделать. На нашей macOS-виртуалке стоят
только Command Line Tools.

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
- проверки профилей тоже нет (нужен второй экземпляр sing-box).

## Что осталось сделать на маке

1. `client/scripts/ios/build.sh` — соберёт `Libbox.xcframework`, панель и
   Xcode-проект Tauri, разложит Swift-файлы.
2. В Xcode завести цель Network Extension и подключить файлы — шаги печатает
   тот же скрипт.
3. Прогнать на устройстве: системный запрос на добавление VPN, подъём туннеля,
   трафик через профиль, остановка.

Что наверняка потребует правки при первой сборке: точные Swift-имена методов,
которые gomobile генерирует из Go-интерфейсов libbox (взяты по тем же правилам
перевода, что у официального клиента sing-box для Apple, но компилятором не
проверены).
