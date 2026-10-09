package main

import (
	"context"
	"fmt"
	"net"
	"net/netip"
	"sync"
	"syscall"

	"github.com/amnezia-vpn/amneziawg-go/conn"
)

// bind — простой (без батчей) UDP-сокет сервера, привязанный к физическому
// интерфейсу через setsockopt (bindToIf — свой на каждую ОС). Без привязки
// ответы клиентам при поднятом TUN Detour ушли бы в туннель.
type bind struct {
	mu      sync.Mutex
	conn    *net.UDPConn
	ifIndex int
}

type endpoint struct{ dst netip.AddrPort }

func (e *endpoint) ClearSrc()           {}
func (e *endpoint) SrcToString() string { return "" }
func (e *endpoint) DstToString() string { return e.dst.String() }
func (e *endpoint) DstToBytes() []byte  { b, _ := e.dst.MarshalBinary(); return b }
func (e *endpoint) DstIP() netip.Addr   { return e.dst.Addr() }
func (e *endpoint) SrcIP() netip.Addr   { return netip.Addr{} }

func newBind(bindIP string) (*bind, error) {
	b := &bind{}
	if bindIP == "" {
		return b, nil
	}
	ip := net.ParseIP(bindIP)
	if ip == nil {
		return nil, fmt.Errorf("-bind-ip: %q не IP", bindIP)
	}
	ifs, err := net.Interfaces()
	if err != nil {
		return nil, err
	}
	for _, i := range ifs {
		addrs, _ := i.Addrs()
		for _, a := range addrs {
			if n, ok := a.(*net.IPNet); ok && n.IP.Equal(ip) {
				b.ifIndex = i.Index
				return b, nil
			}
		}
	}
	return nil, fmt.Errorf("нет интерфейса с адресом %s", bindIP)
}

func (b *bind) Open(port uint16) ([]conn.ReceiveFunc, uint16, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.conn != nil {
		return nil, 0, conn.ErrBindAlreadyOpen
	}
	lc := net.ListenConfig{Control: func(_, _ string, c syscall.RawConn) error {
		if b.ifIndex == 0 {
			return nil
		}
		var serr error
		if err := c.Control(func(fd uintptr) { serr = bindToIf(fd, b.ifIndex) }); err != nil {
			return err
		}
		return serr
	}}
	pc, err := lc.ListenPacket(context.Background(), "udp4", fmt.Sprintf("0.0.0.0:%d", port))
	if err != nil {
		return nil, 0, err
	}
	uc := pc.(*net.UDPConn)
	b.conn = uc
	recv := func(packets [][]byte, sizes []int, eps []conn.Endpoint) (int, error) {
		n, ap, err := uc.ReadFromUDPAddrPort(packets[0])
		if err != nil {
			return 0, err
		}
		sizes[0] = n
		eps[0] = &endpoint{dst: netip.AddrPortFrom(ap.Addr().Unmap(), ap.Port())}
		return 1, nil
	}
	return []conn.ReceiveFunc{recv}, uint16(uc.LocalAddr().(*net.UDPAddr).Port), nil
}

func (b *bind) Close() error {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.conn == nil {
		return nil
	}
	err := b.conn.Close()
	b.conn = nil
	return err
}

func (b *bind) SetMark(uint32) error { return nil }
func (b *bind) BatchSize() int       { return 1 }

func (b *bind) Send(bufs [][]byte, ep conn.Endpoint) error {
	b.mu.Lock()
	c := b.conn
	b.mu.Unlock()
	if c == nil {
		return net.ErrClosed
	}
	e, ok := ep.(*endpoint)
	if !ok {
		return conn.ErrWrongEndpointType
	}
	for _, p := range bufs {
		if _, err := c.WriteToUDPAddrPort(p, e.dst); err != nil {
			return err
		}
	}
	return nil
}

func (b *bind) ParseEndpoint(s string) (conn.Endpoint, error) {
	ap, err := netip.ParseAddrPort(s)
	if err != nil {
		return nil, err
	}
	return &endpoint{dst: ap}, nil
}
