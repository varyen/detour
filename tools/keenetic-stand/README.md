# Стенд «как Keenetic» для detour-server

Живого Keenetic под рукой нет, поэтому `detour-server` гоняется в контейнере,
который отвечает как KeeneticOS там, где это важно:

- `rcimock.py` — заглушка RCI на `:79`: помнит интерфейсы `WireguardN` и их
  пиров, на `up` создаёт dummy-интерфейс `nwgN`, `show interface` отдаёт пиров
  с фиктивными счётчиками; без файла `/tmp/no-wg` компонент wireguard
  «установлен»;
- настоящие `iptables` и `ip-full` (`/opt/sbin/ip`), sing-box 1.14, lua с той
  же чисто-lua `cjson.safe`, что едет на Keenetic.

```sh
docker run --rm --privileged -v <repo>:/repo:ro python:3.12-alpine sh /repo/tools/keenetic-stand/run.sh
```

Не ловит: настоящее поведение KeeneticOS (формат ответов RCI, файрвол NDM,
ядро 4.9-ndm). Для этого — `keenetic/test-*.sh` на устройстве.
