//go:build linux

// detour-awg-go — AmneziaWG в userspace для OpenWrt без kmod-amneziawg.
//
// Один статический бинарник на две роли, чтобы не возить по флешу два:
//
//	amneziawg-go [-f] IFACE      — демон (daemon.go — main.go апстрима без правок
//	                               по сути, MIT, WireGuard LLC / Amnezia);
//	amneziawg-go awg <команда>    — то подмножество утилиты awg, которое зовёт
//	                               detour-server: genkey, pubkey, genpsk,
//	                               setconf, syncconf, show IFACE dump.
//
// Пакет (detour-awg-go) кладёт только /usr/bin/amneziawg-go: имя awg занято
// штатным amneziawg-tools, и detour-server зовёт `amneziawg-go awg …` сам, когда
// утилиты awg нет.
package main

import (
	"os"
	"path/filepath"
)

const Version = "1.0.4-detour"

func main() {
	if filepath.Base(os.Args[0]) == "awg" {
		os.Exit(awgMain(os.Args[1:]))
	}
	if len(os.Args) > 1 && os.Args[1] == "awg" {
		os.Exit(awgMain(os.Args[2:]))
	}
	daemonMain()
}
