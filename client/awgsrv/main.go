// detour-awgsrv — VPN-сервер AmneziaWG для клиента Detour на компьютере.
//
// На роутере сервер — интерфейс ядра, а его трафик ловит файрвол. На
// компьютере ни того ни другого нет: Windows не умеет NAT без Hyper-V, а TUN
// Detour занят своим туннелем. Поэтому сервер целиком в userspace: устройство
// amneziawg-go работает поверх сетевого стека gVisor, и каждое TCP-соединение
// и UDP-поток клиента уходит в sing-box через локальный SOCKS5-вход (`-socks`).
// Там к нему применяются те же правила маршрутизации, что к трафику самого
// компьютера.
//
// UDP-сокет сервера привязывается к физическому интерфейсу (`-bind-ip`): при
// поднятом TUN ответы клиентам иначе ушли бы в туннель, sing-box отправил бы их
// с другого порта, и NAT клиента такие ответы отбросил бы.
//
// Конфиг устройства — файл в формате UAPI (`-config`), перечитывается при
// изменении. Состояние (handshake, байты по пирам) пишется в `-status` раз в
// две секунды. Процесс завершается, когда закрывается stdin: родитель (служба
// Detour) держит его открытым, пока жив.
package main

import (
	"bytes"
	"flag"
	"fmt"
	"io"
	"log"
	"net/netip"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"time"

	"github.com/amnezia-vpn/amneziawg-go/device"
)

func main() {
	if len(os.Args) > 1 {
		switch os.Args[1] {
		case "genkey", "genpsk", "pubkey":
			keys(os.Args[1])
			return
		}
	}
	cfgPath := flag.String("config", "", "UAPI-конфиг устройства (перечитывается при изменении)")
	statusPath := flag.String("status", "", "куда писать состояние (IpcGet)")
	addr := flag.String("addr", "10.66.0.1/24", "адрес сервера в сети клиентов")
	mtu := flag.Int("mtu", 1376, "MTU туннеля")
	socksAddr := flag.String("socks", "127.0.0.1:19485", "SOCKS5-вход sing-box")
	socksUser := flag.String("socks-user", "", "логин SOCKS5")
	socksPass := flag.String("socks-pass", "", "пароль SOCKS5")
	perClient := flag.Bool("socks-per-client", false, "логин SOCKS5 по клиенту: <socks-user>-<последний октет его адреса>")
	bindIP := flag.String("bind-ip", "", "IPv4 физического интерфейса для UDP-сокета сервера")
	watchStdin := flag.Bool("watch-stdin", true, "выйти, когда закроется stdin")
	flag.BoolVar(&verbose, "v", false, "подробный журнал")
	flag.Parse()

	if *cfgPath == "" || *statusPath == "" {
		log.Fatal("нужны -config и -status")
	}
	prefix, err := netip.ParsePrefix(*addr)
	if err != nil || !prefix.Addr().Is4() {
		log.Fatalf("-addr: нужен IPv4/маска, получено %q", *addr)
	}

	socks := &Socks{Addr: *socksAddr, User: *socksUser, Pass: *socksPass, PerClient: *perClient}
	tunDev, err := newNetTun(prefix, *mtu, socks)
	if err != nil {
		log.Fatalf("сетевой стек: %v", err)
	}
	b, err := newBind(*bindIP)
	if err != nil {
		log.Fatalf("привязка к интерфейсу: %v", err)
	}
	level := device.LogLevelError
	if verbose {
		level = device.LogLevelVerbose
	}
	logger := device.NewLogger(level, "awgsrv: ")
	dev := device.NewDevice(tunDev, b, logger)

	var lastCfg []byte
	apply := func() {
		data, err := os.ReadFile(*cfgPath)
		if err != nil || bytes.Equal(data, lastCfg) {
			return
		}
		if err := dev.IpcSet(string(data)); err != nil {
			log.Printf("конфиг не применён: %v", err)
			return
		}
		lastCfg = data
		if err := dev.Up(); err != nil {
			log.Printf("устройство не поднялось: %v", err)
		}
	}
	apply()
	if lastCfg == nil {
		log.Fatal("конфиг не применён — выхожу")
	}

	done := make(chan struct{})
	if *watchStdin {
		go func() {
			_, _ = io.Copy(io.Discard, os.Stdin)
			close(done)
		}()
	}
	sig := make(chan os.Signal, 1)
	signal.Notify(sig, os.Interrupt)

	t := time.NewTicker(2 * time.Second)
	defer t.Stop()
	writeStatus(dev, *statusPath)
	for {
		select {
		case <-t.C:
			apply()
			writeStatus(dev, *statusPath)
		case <-done:
			dev.Close()
			return
		case <-sig:
			dev.Close()
			return
		}
	}
}

func writeStatus(dev *device.Device, path string) {
	s, err := dev.IpcGet()
	if err != nil {
		return
	}
	// Приватный ключ сервера в файл состояния не нужен.
	var out strings.Builder
	for _, l := range strings.Split(s, "\n") {
		if strings.HasPrefix(l, "private_key=") || strings.HasPrefix(l, "preshared_key=") {
			continue
		}
		out.WriteString(l)
		out.WriteByte('\n')
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, []byte(out.String()), 0o600); err != nil {
		return
	}
	_ = os.Rename(tmp, path)
}

func init() {
	log.SetFlags(log.LstdFlags)
	log.SetPrefix(fmt.Sprintf("%s: ", filepath.Base(os.Args[0])))
}

var verbose bool

func debugf(format string, a ...any) {
	if verbose {
		log.Printf(format, a...)
	}
}
