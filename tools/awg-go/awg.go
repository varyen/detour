//go:build linux

package main

import (
	"bufio"
	"crypto/ecdh"
	"crypto/rand"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"sort"
	"strings"
	"time"
)

const sockDir = "/var/run/amneziawg"

func awgMain(args []string) int {
	if len(args) == 0 {
		fmt.Fprintln(os.Stderr, "usage: awg {genkey|genpsk|pubkey|setconf IFACE FILE|syncconf IFACE FILE|show IFACE dump}")
		return 2
	}
	var err error
	switch args[0] {
	case "genkey", "genpsk":
		err = genKey(args[0] == "genkey")
	case "pubkey":
		err = pubKey()
	case "setconf", "syncconf":
		if len(args) != 3 {
			err = errors.New(args[0] + " IFACE FILE")
			break
		}
		err = applyConf(args[1], args[2], args[0] == "syncconf")
	case "show":
		if len(args) != 3 || args[2] != "dump" {
			err = errors.New("поддерживается только: show IFACE dump")
			break
		}
		err = showDump(args[1])
	default:
		err = fmt.Errorf("команда %q не поддерживается", args[0])
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "awg:", err)
		return 1
	}
	return 0
}

func genKey(clamp bool) error {
	var k [32]byte
	if _, err := rand.Read(k[:]); err != nil {
		return err
	}
	if clamp {
		k[0] &= 248
		k[31] = (k[31] & 127) | 64
	}
	fmt.Println(base64.StdEncoding.EncodeToString(k[:]))
	return nil
}

func pubKey() error {
	in, err := io.ReadAll(os.Stdin)
	if err != nil {
		return err
	}
	raw, err := base64.StdEncoding.DecodeString(strings.TrimSpace(string(in)))
	if err != nil || len(raw) != 32 {
		return errors.New("pubkey: ждём приватный ключ base64 на stdin")
	}
	priv, err := ecdh.X25519().NewPrivateKey(raw)
	if err != nil {
		return err
	}
	fmt.Println(base64.StdEncoding.EncodeToString(priv.PublicKey().Bytes()))
	return nil
}

func b64hex(s string) (string, error) {
	raw, err := base64.StdEncoding.DecodeString(strings.TrimSpace(s))
	if err != nil || len(raw) != 32 {
		return "", fmt.Errorf("ключ %q: ждём 32 байта base64", s)
	}
	return hex.EncodeToString(raw), nil
}

func hexb64(s string) string {
	raw, err := hex.DecodeString(s)
	if err != nil {
		return s
	}
	return base64.StdEncoding.EncodeToString(raw)
}

type peer struct {
	pub, psk, endpoint, keepalive string
	allowed                       []string
}

type conf struct {
	iface [][2]string // UAPI-ключи интерфейса в порядке файла
	peers []peer
}

// Ключи [Interface] → UAPI. Обфускация AmneziaWG передаётся как есть, в нижнем
// регистре (jc, jmin, …, h4, а у AWG 2.0 — s3, s4, i1…i5).
var ifaceKeys = map[string]string{
	"privatekey": "private_key",
	"listenport": "listen_port",
	"fwmark":     "fwmark",
}

func parseConf(path string) (*conf, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	c := &conf{}
	section := ""
	var cur *peer
	sc := bufio.NewScanner(f)
	for sc.Scan() {
		line := strings.TrimSpace(sc.Text())
		if i := strings.IndexByte(line, '#'); i >= 0 {
			line = strings.TrimSpace(line[:i])
		}
		if line == "" {
			continue
		}
		if strings.HasPrefix(line, "[") {
			section = strings.ToLower(strings.Trim(line, "[]"))
			if section == "peer" {
				c.peers = append(c.peers, peer{})
				cur = &c.peers[len(c.peers)-1]
			}
			continue
		}
		k, v, ok := strings.Cut(line, "=")
		if !ok {
			return nil, fmt.Errorf("строка без «=»: %q", line)
		}
		k = strings.ToLower(strings.TrimSpace(k))
		v = strings.TrimSpace(v)
		switch section {
		case "interface":
			if k == "privatekey" {
				h, err := b64hex(v)
				if err != nil {
					return nil, err
				}
				v = h
			}
			if u, ok := ifaceKeys[k]; ok {
				k = u
			}
			c.iface = append(c.iface, [2]string{k, v})
		case "peer":
			switch k {
			case "publickey":
				h, err := b64hex(v)
				if err != nil {
					return nil, err
				}
				cur.pub = h
			case "presharedkey":
				h, err := b64hex(v)
				if err != nil {
					return nil, err
				}
				cur.psk = h
			case "allowedips":
				for _, a := range strings.Split(v, ",") {
					if a = strings.TrimSpace(a); a != "" {
						cur.allowed = append(cur.allowed, a)
					}
				}
			case "endpoint":
				cur.endpoint = v
			case "persistentkeepalive":
				cur.keepalive = v
			default:
				return nil, fmt.Errorf("неизвестный ключ пира %q", k)
			}
		default:
			return nil, fmt.Errorf("ключ %q вне секции", k)
		}
	}
	return c, sc.Err()
}

func uapi(iface, req string) (string, error) {
	c, err := net.DialTimeout("unix", sockDir+"/"+iface+".sock", 3*time.Second)
	if err != nil {
		return "", err
	}
	defer c.Close()
	c.SetDeadline(time.Now().Add(10 * time.Second))
	if _, err := io.WriteString(c, req); err != nil {
		return "", err
	}
	var out strings.Builder
	r := bufio.NewReader(c)
	for {
		line, err := r.ReadString('\n')
		if err != nil {
			return out.String(), err
		}
		if line == "\n" {
			break
		}
		if strings.HasPrefix(line, "errno=") {
			if line != "errno=0\n" {
				return out.String(), fmt.Errorf("устройство ответило %s", strings.TrimSpace(line))
			}
			continue
		}
		out.WriteString(line)
	}
	return out.String(), nil
}

func peerLines(b *strings.Builder, p peer) {
	fmt.Fprintf(b, "public_key=%s\n", p.pub)
	if p.psk != "" {
		fmt.Fprintf(b, "preshared_key=%s\n", p.psk)
	}
	if p.endpoint != "" {
		fmt.Fprintf(b, "endpoint=%s\n", p.endpoint)
	}
	if p.keepalive != "" {
		fmt.Fprintf(b, "persistent_keepalive_interval=%s\n", p.keepalive)
	}
	b.WriteString("replace_allowed_ips=true\n")
	for _, a := range p.allowed {
		fmt.Fprintf(b, "allowed_ip=%s\n", a)
	}
}

// setconf — всё заново; syncconf — пиры дописываются и снимаются по разнице,
// сессии оставшихся не рвутся (интерфейсные ключи меняются, только если
// отличаются от текущих).
func applyConf(iface, path string, sync bool) error {
	c, err := parseConf(path)
	if err != nil {
		return err
	}
	var b strings.Builder
	b.WriteString("set=1\n")
	if !sync {
		for _, kv := range c.iface {
			fmt.Fprintf(&b, "%s=%s\n", kv[0], kv[1])
		}
		b.WriteString("replace_peers=true\n")
		for _, p := range c.peers {
			peerLines(&b, p)
		}
	} else {
		cur, err := uapi(iface, "get=1\n\n")
		if err != nil {
			return err
		}
		have := map[string]string{}
		var curPeers []string
		for _, l := range strings.Split(cur, "\n") {
			k, v, ok := strings.Cut(l, "=")
			if !ok {
				continue
			}
			if k == "public_key" {
				curPeers = append(curPeers, v)
			} else if len(curPeers) == 0 {
				have[k] = v
			}
		}
		for _, kv := range c.iface {
			if have[kv[0]] != kv[1] {
				fmt.Fprintf(&b, "%s=%s\n", kv[0], kv[1])
			}
		}
		want := map[string]bool{}
		for _, p := range c.peers {
			want[p.pub] = true
		}
		for _, pk := range curPeers {
			if !want[pk] {
				fmt.Fprintf(&b, "public_key=%s\nremove=true\n", pk)
			}
		}
		for _, p := range c.peers {
			peerLines(&b, p)
		}
	}
	b.WriteString("\n")
	_, err = uapi(iface, b.String())
	return err
}

// Формат `wg show IFACE dump`: строка интерфейса, затем по строке на пира —
// ключ, psk, адрес, allowed-ips, последнее рукопожатие, rx, tx, keepalive.
func showDump(iface string) error {
	out, err := uapi(iface, "get=1\n\n")
	if err != nil {
		return err
	}
	dev := map[string]string{}
	type p struct{ m map[string]string; allowed []string }
	var peers []*p
	for _, l := range strings.Split(out, "\n") {
		k, v, ok := strings.Cut(l, "=")
		if !ok {
			continue
		}
		switch {
		case k == "public_key":
			peers = append(peers, &p{m: map[string]string{"public_key": v}})
		case len(peers) == 0:
			dev[k] = v
		case k == "allowed_ip":
			peers[len(peers)-1].allowed = append(peers[len(peers)-1].allowed, v)
		default:
			peers[len(peers)-1].m[k] = v
		}
	}
	or := func(v, d string) string {
		if v == "" || v == "0" && d == "off" {
			return d
		}
		return v
	}
	pub := ""
	if dev["private_key"] != "" {
		if raw, err := hex.DecodeString(dev["private_key"]); err == nil {
			if k, err := ecdh.X25519().NewPrivateKey(raw); err == nil {
				pub = base64.StdEncoding.EncodeToString(k.PublicKey().Bytes())
			}
		}
	}
	fmt.Printf("%s\t%s\t%s\t%s\n", or(hexb64(dev["private_key"]), "(none)"), or(pub, "(none)"), or(dev["listen_port"], "0"), or(dev["fwmark"], "off"))
	for _, x := range peers {
		sort.Strings(x.allowed)
		psk := x.m["preshared_key"]
		if psk == "" || strings.Trim(psk, "0") == "" {
			psk = "(none)"
		} else {
			psk = hexb64(psk)
		}
		allowed := strings.Join(x.allowed, ",")
		if allowed == "" {
			allowed = "(none)"
		}
		fmt.Printf("%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n",
			hexb64(x.m["public_key"]), psk, or(x.m["endpoint"], "(none)"), allowed,
			or(x.m["last_handshake_time_sec"], "0"), or(x.m["rx_bytes"], "0"), or(x.m["tx_bytes"], "0"),
			or(x.m["persistent_keepalive_interval"], "off"))
	}
	return nil
}
