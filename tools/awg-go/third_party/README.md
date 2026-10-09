# third_party

`amneziawg-go` v1.0.4 (MIT, WireGuard LLC / Amnezia) с правками под MIPS.
Подключается через `replace` в `../go.mod`.

На mips/mipsel невыровненное чтение `uint32` — это SIGBUS. Апстрим читал
поля прямо из байтовых буферов, и под qemu-user демон падал сразу:

- `tun/tun_linux.go`: `getIFIndex`, `setMTU`, `MTU` — поле `ifreq` после
  имени теперь читается и пишется через `binary.NativeEndian`;
- `tun/tun_linux.go`, `device/sticky_linux.go`: 64-килобайтный буфер
  netlink теперь лежит поверх `[]uint64`. Go клал его на стек без
  выравнивания, а разбор `NlMsghdr`/`RtAttr` читал `uint32`.

Живое MIPS-ядро такие чтения обычно эмулирует, поэтому без правок оно, скорее
всего, просто работало бы медленнее. qemu-user не эмулирует. Обновляя апстрим,
перенести правки и прогнать демон под `qemu-mipsel`.
