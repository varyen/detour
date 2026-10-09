package main

import (
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"net"
	"net/netip"
	"strconv"
	"time"
)

// Socks — клиент SOCKS5 (RFC 1928/1929) к mixed-входу sing-box: CONNECT для
// TCP и UDP ASSOCIATE для UDP. Своя реализация — ради ASSOCIATE, которого нет
// в golang.org/x/net/proxy.
type Socks struct {
	Addr, User, Pass string
}

func (s *Socks) handshake(c net.Conn, cmd byte, target string) (netip.AddrPort, error) {
	c.SetDeadline(time.Now().Add(10 * time.Second))
	defer c.SetDeadline(time.Time{})
	method := byte(0x00)
	if s.User != "" {
		method = 0x02
	}
	if _, err := c.Write([]byte{5, 1, method}); err != nil {
		return netip.AddrPort{}, err
	}
	rep := make([]byte, 2)
	if _, err := io.ReadFull(c, rep); err != nil {
		return netip.AddrPort{}, err
	}
	if rep[0] != 5 || rep[1] != method {
		return netip.AddrPort{}, errors.New("socks: метод не принят")
	}
	if method == 0x02 {
		msg := []byte{1, byte(len(s.User))}
		msg = append(msg, s.User...)
		msg = append(msg, byte(len(s.Pass)))
		msg = append(msg, s.Pass...)
		if _, err := c.Write(msg); err != nil {
			return netip.AddrPort{}, err
		}
		if _, err := io.ReadFull(c, rep); err != nil {
			return netip.AddrPort{}, err
		}
		if rep[1] != 0 {
			return netip.AddrPort{}, errors.New("socks: неверный логин или пароль")
		}
	}
	req := []byte{5, cmd, 0}
	addr, err := encodeAddr(target)
	if err != nil {
		return netip.AddrPort{}, err
	}
	req = append(req, addr...)
	if _, err := c.Write(req); err != nil {
		return netip.AddrPort{}, err
	}
	head := make([]byte, 4)
	if _, err := io.ReadFull(c, head); err != nil {
		return netip.AddrPort{}, err
	}
	if head[1] != 0 {
		return netip.AddrPort{}, fmt.Errorf("socks: отказ %d", head[1])
	}
	var ip netip.Addr
	switch head[3] {
	case 1:
		b := make([]byte, 4)
		if _, err := io.ReadFull(c, b); err != nil {
			return netip.AddrPort{}, err
		}
		ip = netip.AddrFrom4([4]byte(b))
	case 4:
		b := make([]byte, 16)
		if _, err := io.ReadFull(c, b); err != nil {
			return netip.AddrPort{}, err
		}
		ip = netip.AddrFrom16([16]byte(b))
	case 3:
		l := make([]byte, 1)
		if _, err := io.ReadFull(c, l); err != nil {
			return netip.AddrPort{}, err
		}
		if _, err := io.ReadFull(c, make([]byte, l[0])); err != nil {
			return netip.AddrPort{}, err
		}
	}
	p := make([]byte, 2)
	if _, err := io.ReadFull(c, p); err != nil {
		return netip.AddrPort{}, err
	}
	return netip.AddrPortFrom(ip, binary.BigEndian.Uint16(p)), nil
}

func encodeAddr(target string) ([]byte, error) {
	host, port, err := net.SplitHostPort(target)
	if err != nil {
		return nil, err
	}
	pn, err := strconv.Atoi(port)
	if err != nil {
		return nil, err
	}
	var out []byte
	if ip, err := netip.ParseAddr(host); err == nil {
		if ip.Is4() {
			out = append([]byte{1}, ip.AsSlice()...)
		} else {
			out = append([]byte{4}, ip.AsSlice()...)
		}
	} else {
		out = append([]byte{3, byte(len(host))}, host...)
	}
	return binary.BigEndian.AppendUint16(out, uint16(pn)), nil
}

func (s *Socks) DialTCP(target string) (net.Conn, error) {
	c, err := net.DialTimeout("tcp", s.Addr, 5*time.Second)
	if err != nil {
		return nil, err
	}
	if _, err := s.handshake(c, 1, target); err != nil {
		c.Close()
		return nil, err
	}
	return c, nil
}

// Assoc — один UDP ASSOCIATE: управляющее TCP-соединение держит его живым.
type Assoc struct {
	ctl   net.Conn
	udp   *net.UDPConn
	relay *net.UDPAddr
}

func (s *Socks) Associate() (*Assoc, error) {
	c, err := net.DialTimeout("tcp", s.Addr, 5*time.Second)
	if err != nil {
		return nil, err
	}
	bnd, err := s.handshake(c, 3, "0.0.0.0:0")
	if err != nil {
		c.Close()
		return nil, err
	}
	host, _, _ := net.SplitHostPort(s.Addr)
	relayIP := net.ParseIP(host)
	if bnd.Addr().IsValid() && !bnd.Addr().IsUnspecified() {
		relayIP = net.IP(bnd.Addr().AsSlice())
	}
	u, err := net.ListenUDP("udp", &net.UDPAddr{IP: net.IPv4(127, 0, 0, 1)})
	if err != nil {
		c.Close()
		return nil, err
	}
	return &Assoc{ctl: c, udp: u, relay: &net.UDPAddr{IP: relayIP, Port: int(bnd.Port())}}, nil
}

func (a *Assoc) WriteTo(p []byte, target string) error {
	addr, err := encodeAddr(target)
	if err != nil {
		return err
	}
	pkt := append([]byte{0, 0, 0}, addr...)
	pkt = append(pkt, p...)
	_, err = a.udp.WriteToUDP(pkt, a.relay)
	return err
}

// ReadFrom — полезная нагрузка без заголовка SOCKS5.
func (a *Assoc) ReadFrom(buf []byte) (int, error) {
	raw := make([]byte, len(buf)+262)
	for {
		n, _, err := a.udp.ReadFromUDP(raw)
		if err != nil {
			return 0, err
		}
		if n < 4 || raw[2] != 0 {
			continue // фрагменты не поддерживаем
		}
		off := 4
		switch raw[3] {
		case 1:
			off += 4
		case 4:
			off += 16
		case 3:
			off += 1 + int(raw[4])
		default:
			continue
		}
		off += 2
		if off > n {
			continue
		}
		return copy(buf, raw[off:n]), nil
	}
}

func (a *Assoc) SetReadDeadline(t time.Time) error { return a.udp.SetReadDeadline(t) }

func (a *Assoc) Close() error {
	a.ctl.Close()
	return a.udp.Close()
}
