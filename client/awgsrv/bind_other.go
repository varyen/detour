//go:build !windows && !darwin

package main

import (
	"net"

	"golang.org/x/sys/unix"
)

func bindToIf(fd uintptr, index int) error {
	i, err := net.InterfaceByIndex(index)
	if err != nil {
		return err
	}
	return unix.BindToDevice(int(fd), i.Name)
}
