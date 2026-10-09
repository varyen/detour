package main

import "golang.org/x/sys/unix"

func bindToIf(fd uintptr, index int) error {
	return unix.SetsockoptInt(int(fd), unix.IPPROTO_IP, unix.IP_BOUND_IF, index)
}
