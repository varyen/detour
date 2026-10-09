package main

import (
	"encoding/binary"

	"golang.org/x/sys/windows"
)

// IP_UNICAST_IF ждёт номер интерфейса в сетевом порядке байт (для IPv4).
func bindToIf(fd uintptr, index int) error {
	var be [4]byte
	binary.BigEndian.PutUint32(be[:], uint32(index))
	return windows.SetsockoptInt(windows.Handle(fd), windows.IPPROTO_IP, 31 /* IP_UNICAST_IF */, int(binary.NativeEndian.Uint32(be[:])))
}
