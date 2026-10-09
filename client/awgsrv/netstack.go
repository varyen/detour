package main

import (
	"context"
	"fmt"
	"io"
	"net"
	"net/netip"
	"os"
	"reflect"
	"strings"
	"sync"
	"time"

	"github.com/amnezia-vpn/amneziawg-go/tun"
	"gvisor.dev/gvisor/pkg/buffer"
	"gvisor.dev/gvisor/pkg/tcpip"
	"gvisor.dev/gvisor/pkg/tcpip/adapters/gonet"
	"gvisor.dev/gvisor/pkg/tcpip/header"
	"gvisor.dev/gvisor/pkg/tcpip/link/channel"
	"gvisor.dev/gvisor/pkg/tcpip/network/ipv4"
	"gvisor.dev/gvisor/pkg/tcpip/stack"
	"gvisor.dev/gvisor/pkg/tcpip/transport/icmp"
	"gvisor.dev/gvisor/pkg/tcpip/transport/tcp"
	"gvisor.dev/gvisor/pkg/tcpip/transport/udp"
	"gvisor.dev/gvisor/pkg/waiter"
)

// netTun — tun.Device на стеке gVisor, который принимает пакеты на ЛЮБОЙ
// адрес (promiscuous) и отвечает от имени любого адреса (spoofing): сервер
// работает как шлюз, а не как хост. Пойманные соединения уходят в SOCKS5.
type netTun struct {
	ep       *channel.Endpoint
	stack    *stack.Stack
	events   chan tun.Event
	incoming chan *buffer.View
	notify   *channel.NotificationHandle
	mtu      int
	self     netip.Addr
	socks    *Socks
}

const nicID = 1

func newNetTun(prefix netip.Prefix, mtu int, socks *Socks) (*netTun, error) {
	s := stack.New(stack.Options{
		NetworkProtocols:   []stack.NetworkProtocolFactory{ipv4.NewProtocol},
		TransportProtocols: []stack.TransportProtocolFactory{tcp.NewProtocol, udp.NewProtocol, icmp.NewProtocol4},
		// HandleLocal сверяет адрес источника со «своими», а в promiscuous-режиме
		// своим считается любой — и стек отбрасывал каждый пакет клиента.
		HandleLocal: false,
	})
	t := &netTun{
		ep:       channel.New(1024, uint32(mtu), ""),
		stack:    s,
		events:   make(chan tun.Event, 4),
		incoming: make(chan *buffer.View),
		mtu:      mtu,
		self:     prefix.Addr(),
		socks:    socks,
	}
	sack := tcpip.TCPSACKEnabled(true)
	s.SetTransportProtocolOption(tcp.ProtocolNumber, &sack)
	t.notify = t.ep.AddNotify(t)
	if err := s.CreateNIC(nicID, t.ep); err != nil {
		return nil, fmt.Errorf("CreateNIC: %v", err)
	}
	pa := tcpip.ProtocolAddress{
		Protocol:          ipv4.ProtocolNumber,
		AddressWithPrefix: tcpip.AddrFromSlice(prefix.Addr().AsSlice()).WithPrefix(),
	}
	if err := s.AddProtocolAddress(nicID, pa, stack.AddressProperties{}); err != nil {
		return nil, fmt.Errorf("AddProtocolAddress: %v", err)
	}
	s.SetPromiscuousMode(nicID, true)
	s.SetSpoofing(nicID, true)
	s.SetRouteTable([]tcpip.Route{{Destination: header.IPv4EmptySubnet, NIC: nicID}})

	tcpFwd := tcp.NewForwarder(s, 0, 2048, t.handleTCP)
	s.SetTransportProtocolHandler(tcp.ProtocolNumber, tcpFwd.HandlePacket)
	udpFwd := udp.NewForwarder(s, t.handleUDP)
	s.SetTransportProtocolHandler(udp.ProtocolNumber, udpFwd.HandlePacket)

	t.events <- tun.EventUp
	if verbose {
		go func() {
			for range time.Tick(5 * time.Second) {
				var out []string
				dumpStats(reflect.ValueOf(s.Stats()), "", &out)
				debugf("stats: %s", strings.Join(out, " "))
			}
		}()
	}
	return t, nil
}

func (t *netTun) handleTCP(r *tcp.ForwarderRequest) {
	id := r.ID()
	dst := netip.AddrPortFrom(addrOf(id.LocalAddress), id.LocalPort)
	debugf("tcp %s -> %s", addrOf(id.RemoteAddress), dst)
	upstream, err := t.socks.DialTCP(dst.String())
	if err != nil {
		r.Complete(true) // RST — клиент сразу узнает, что не вышло
		return
	}
	var wq waiter.Queue
	ep, terr := r.CreateEndpoint(&wq)
	if terr != nil {
		r.Complete(true)
		upstream.Close()
		return
	}
	r.Complete(false)
	ep.SocketOptions().SetKeepAlive(true)
	local := gonet.NewTCPConn(&wq, ep)
	go pipe(local, upstream)
}

func (t *netTun) handleUDP(r *udp.ForwarderRequest) {
	id := r.ID()
	dst := netip.AddrPortFrom(addrOf(id.LocalAddress), id.LocalPort)
	debugf("udp %s -> %s", addrOf(id.RemoteAddress), dst)
	var wq waiter.Queue
	ep, terr := r.CreateEndpoint(&wq)
	if terr != nil {
		return
	}
	local := gonet.NewUDPConn(t.stack, &wq, ep)
	go func() {
		defer local.Close()
		assoc, err := t.socks.Associate()
		if err != nil {
			return
		}
		defer assoc.Close()
		idle := 2 * time.Minute
		if dst.Port() == 53 {
			idle = 20 * time.Second
		}
		var wg sync.WaitGroup
		wg.Add(1)
		go func() {
			defer wg.Done()
			buf := make([]byte, 65535)
			for {
				assoc.SetReadDeadline(time.Now().Add(idle))
				n, err := assoc.ReadFrom(buf)
				if err != nil {
					local.Close()
					return
				}
				local.SetWriteDeadline(time.Now().Add(5 * time.Second))
				if _, err := local.Write(buf[:n]); err != nil {
					return
				}
			}
		}()
		buf := make([]byte, 65535)
		for {
			local.SetReadDeadline(time.Now().Add(idle))
			n, err := local.Read(buf)
			if err != nil {
				break
			}
			if err := assoc.WriteTo(buf[:n], dst.String()); err != nil {
				break
			}
		}
		assoc.Close()
		wg.Wait()
	}()
}

func addrOf(a tcpip.Address) netip.Addr {
	ip, _ := netip.AddrFromSlice(a.AsSlice())
	return ip
}

func pipe(a, b net.Conn) {
	defer a.Close()
	defer b.Close()
	ctx, cancel := context.WithCancel(context.Background())
	go func() {
		_, _ = io.Copy(a, b)
		cancel()
	}()
	_, _ = io.Copy(b, a)
	if cw, ok := b.(interface{ CloseWrite() error }); ok {
		_ = cw.CloseWrite()
	}
	select {
	case <-ctx.Done():
	case <-time.After(30 * time.Second):
	}
}

// ---- tun.Device ----

func (t *netTun) Name() (string, error)    { return "awgsrv", nil }
func (t *netTun) File() *os.File           { return nil }
func (t *netTun) Events() <-chan tun.Event { return t.events }
func (t *netTun) MTU() (int, error)        { return t.mtu, nil }
func (t *netTun) BatchSize() int           { return 1 }

func (t *netTun) Read(bufs [][]byte, sizes []int, offset int) (int, error) {
	v, ok := <-t.incoming
	if !ok {
		return 0, os.ErrClosed
	}
	n, err := v.Read(bufs[0][offset:])
	if err != nil {
		return 0, err
	}
	sizes[0] = n
	return 1, nil
}

func (t *netTun) Write(bufs [][]byte, offset int) (int, error) {
	for _, b := range bufs {
		p := b[offset:]
		if len(p) == 0 {
			continue
		}
		if p[0]>>4 != 4 {
			continue // только IPv4: клиентам выдаётся только IPv4
		}
		debugf("in %d bytes proto %d", len(p), p[9])
		pkb := stack.NewPacketBuffer(stack.PacketBufferOptions{Payload: buffer.MakeWithData(p)})
		t.ep.InjectInbound(header.IPv4ProtocolNumber, pkb)
	}
	return len(bufs), nil
}

func (t *netTun) WriteNotify() {
	pkt := t.ep.Read()
	if pkt == nil {
		return
	}
	v := pkt.ToView()
	pkt.DecRef()
	debugf("out %d bytes", v.Size())
	t.incoming <- v
}

func (t *netTun) Close() error {
	t.stack.RemoveNIC(nicID)
	t.stack.Close()
	t.ep.RemoveNotify(t.notify)
	t.ep.Close()
	close(t.events)
	close(t.incoming)
	return nil
}

// dumpStats — все ненулевые счётчики gVisor (только для -v).
func dumpStats(v reflect.Value, prefix string, out *[]string) {
	if v.Kind() == reflect.Ptr {
		if v.IsNil() {
			return
		}
		if c, ok := v.Interface().(*tcpip.StatCounter); ok {
			if n := c.Value(); n != 0 {
				*out = append(*out, fmt.Sprintf("%s=%d", prefix, n))
			}
			return
		}
		v = v.Elem()
	}
	if v.Kind() != reflect.Struct {
		return
	}
	for i := 0; i < v.NumField(); i++ {
		f := v.Type().Field(i)
		if !f.IsExported() {
			continue
		}
		name := f.Name
		if prefix != "" {
			name = prefix + "." + f.Name
		}
		dumpStats(v.Field(i), name, out)
	}
}
