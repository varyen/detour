#!/usr/bin/env python3
"""Эталоны перевода sing-box → mihomo для теста клиента.

Входы — конфиги sing-box той же формы, что строит detour-api (все типы
исходящих, цепочки, входы роутера, правила карты маршрутов). Выходы считает
Lua-транслятор роутера (router_files/sb2mihomo.lua) в OpenWrt-контейнере.
Rust-порт (client/crates/core/src/mihomo.rs) обязан давать то же самое —
это проверяет тест `matches_router_translator`.

    python tools/awg-stand/gen_sb2mihomo_fixtures.py
"""
import base64
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
OUT = os.path.join(ROOT, "client", "crates", "core", "tests", "fixtures", "sb2mihomo")
IMAGE = "detour-owrt-engine:x86-64-23.05.5"   # готовит engine_test.py (lua + lua-cjson)

UUID = "00000000-0000-4000-8000-000000000001"


def key(n, url=False):
    """Ключ правильного формата (32 байта base64): mihomo -t проверяет формат."""
    raw = bytes([n]) * 32
    return base64.urlsafe_b64encode(raw).decode().rstrip("=") if url else base64.b64encode(raw).decode()


PRIV, PEER, PSK, HPK, P1, P2 = key(1), key(2), key(3), key(4), key(5), key(6)
TLS = {"enabled": True, "server_name": "vpn.example.com", "insecure": True, "alpn": ["h2", "http/1.1"],
       "utls": {"enabled": True, "fingerprint": "firefox"}}
OUTBOUNDS = {
    "trojan": {"type": "trojan", "server": "vpn.example.com", "server_port": 443, "password": "pw", "tls": TLS},
    "vless-reality": {"type": "vless", "server": "vpn.example.com", "server_port": 443, "uuid": UUID,
                      "flow": "xtls-rprx-vision",
                      "tls": {"enabled": True, "server_name": "www.example.com",
                              "reality": {"enabled": True, "public_key": key(7, url=True), "short_id": "0123abcd"}}},
    "vless-ws": {"type": "vless", "server": "vpn.example.com", "server_port": 443, "uuid": UUID, "tls": TLS,
                 "transport": {"type": "ws", "path": "/ws", "headers": {"Host": "cdn.example.com"},
                               "max_early_data": 2048, "early_data_header_name": "Sec-WebSocket-Protocol"}},
    "vless-grpc": {"type": "vless", "server": "vpn.example.com", "server_port": 443, "uuid": UUID, "tls": TLS,
                   "transport": {"type": "grpc", "service_name": "svc"}},
    "vless-h2": {"type": "vless", "server": "vpn.example.com", "server_port": 443, "uuid": UUID, "tls": TLS,
                 "transport": {"type": "http", "path": "/h2", "host": ["a.example.com"]}},
    "vmess-http": {"type": "vmess", "server": "vpn.example.com", "server_port": 80, "uuid": UUID,
                   "security": "aes-128-gcm", "alter_id": 0,
                   "transport": {"type": "http", "path": "/p", "host": ["b.example.com"]}},
    "vmess-hu": {"type": "vmess", "server": "vpn.example.com", "server_port": 80, "uuid": UUID,
                 "transport": {"type": "httpupgrade", "path": "/hu", "host": "c.example.com"}},
    "ss": {"type": "shadowsocks", "server": "vpn.example.com", "server_port": 8388,
           "method": "2022-blake3-aes-128-gcm", "password": "AAAAAAAAAAAAAAAAAAAAAA==", "udp_over_tcp": True},
    "ss-obfs": {"type": "shadowsocks", "server": "vpn.example.com", "server_port": 8388, "method": "aes-256-gcm",
                "password": "pw", "plugin": "obfs-local", "plugin_opts": "obfs=tls;obfs-host=www.example.com"},
    "hy2": {"type": "hysteria2", "server": "vpn.example.com", "server_port": 443, "password": "pw",
            "obfs": {"type": "salamander", "password": "ob"}, "up_mbps": 50, "down_mbps": 200,
            "server_ports": ["20000:30000"], "tls": TLS},
    "hy1": {"type": "hysteria", "server": "vpn.example.com", "server_port": 443, "auth_str": "a", "obfs": "o",
            "up_mbps": 20, "down_mbps": 100, "tls": TLS},
    "tuic": {"type": "tuic", "server": "vpn.example.com", "server_port": 443, "uuid": UUID, "password": "pw",
             "congestion_control": "bbr", "udp_relay_mode": "quic", "zero_rtt_handshake": True, "tls": TLS},
    "socks": {"type": "socks", "server": "proxy.example.com", "server_port": 1080, "version": "5",
              "username": "u", "password": "p"},
    "https": {"type": "http", "server": "proxy.example.com", "server_port": 443, "username": "u",
              "password": "p", "tls": {"enabled": True, "server_name": "proxy.example.com"}},
    "wg-flat": {"type": "wireguard", "server": "wg.example.com", "server_port": 51820, "private_key": PRIV,
                "peer_public_key": PEER, "pre_shared_key": PSK, "local_address": ["10.0.0.2/32", "fd00::2/128"],
                "reserved": [1, 2, 3], "mtu": 1280},
    "awg": {"type": "amneziawg", "server": "awg.example.com", "server_port": 51820, "private_key": PRIV,
            "peer_public_key": PEER, "local_address": ["10.8.0.2/32"], "persistent_keepalive_interval": 25,
            "amnezia": {"version": 3, "jc": 4, "jmin": "40", "jmax": 70, "s1": 16, "s2": 24, "s3": 20, "s4": 14,
                        "h1": "100-200", "h2": "300-400", "h3": "500-600", "h4": "700-800",
                        "i1": "<b 0xc2><r 32>", "header-protection-key": HPK, "random-trailers": "1"}},
}


def router_config():
    """Форма detour-api build_config в одиночном режиме: цепочка, цели маршрутов,
    перехват, UDP через VPN, запрет торрентов."""
    big = [f"site{i}.example.com" for i in range(12)]
    return {
        "log": {"level": "warn", "output": "/var/log/sing-box.log", "timestamp": True},
        "inbounds": [
            {"type": "redirect", "tag": "redirect-in", "listen": "::", "listen_port": 12345},
            {"type": "redirect", "tag": "in_rt_1", "listen": "::", "listen_port": 12401},
            {"type": "tproxy", "tag": "in_rt_udp_1", "listen": "::", "listen_port": 12501, "network": "udp"},
            {"type": "socks", "tag": "in_ix_px", "listen": "::", "listen_port": 12371,
             "users": [{"username": "u", "password": "p"}]},
            {"type": "http", "tag": "in_ix_hx", "listen": "::", "listen_port": 12372},
            {"type": "tproxy", "tag": "tproxy-udp", "listen": "::", "listen_port": 12350, "network": "udp"},
        ],
        "endpoints": [{"type": "wireguard", "tag": "out_wg", "address": ["10.0.0.2/32"], "private_key": PRIV,
                       "mtu": 1420, "peers": [
                           {"address": "wg1.example.com", "port": 51820, "public_key": P1,
                            "allowed_ips": ["0.0.0.0/0"], "reserved": [0, 0, 1]},
                           {"address": "wg2.example.com", "port": 51821, "public_key": P2,
                            "allowed_ips": ["10.9.0.0/16"]}]}],
        "outbounds": [
            dict(OUTBOUNDS["vless-ws"], tag="chain_1"),
            dict(OUTBOUNDS["trojan"], tag="proxy", detour="chain_1"),
            dict(OUTBOUNDS["awg"], tag="out_awg"),
            dict(OUTBOUNDS["socks"], tag="out_px"),
            {"type": "direct", "tag": "direct"},
            {"type": "block", "tag": "block"},
        ],
        "route": {
            "rules": [
                {"action": "sniff"},
                {"protocol": ["bittorrent"], "network": ["tcp"], "action": "reject"},
                {"action": "route", "domain": ["api.example.com"], "outbound": "out_px",
                 "override_address": "api.example.com"},
                {"action": "route", "domain_suffix": ["a.example.com", "b.example.com"], "outbound": "out_awg"},
                {"action": "route", "domain_suffix": big, "outbound": "out_wg"},
                {"action": "route", "ip_cidr": ["203.0.113.0/24", "198.51.100.7", "2001:db8::/32"],
                 "outbound": "out_awg"},
                {"action": "route", "ip_cidr": [f"192.0.2.{i}" for i in range(10)], "outbound": "out_wg"},
                {"action": "route", "inbound": ["in_rt_1", "in_rt_udp_1"], "outbound": "out_awg"},
                {"action": "route", "inbound": ["in_ix_px"], "outbound": "direct"},
                {"action": "reject", "domain_suffix": ["blocked.example.com"]},
                {"network": ["udp"], "port_range": ["27000:27100", "3478:"], "outbound": "proxy"},
            ],
            "final": "proxy",
        },
        "experimental": {"clash_api": {"external_controller": "127.0.0.1:19390", "secret": "s3"}},
    }


def main():
    os.makedirs(OUT, exist_ok=True)
    cases = {}
    for name, ob in OUTBOUNDS.items():
        cases[f"out-{name}"] = {
            "log": {"level": "error"},
            "inbounds": [{"type": "mixed", "tag": "mixed-in", "listen": "127.0.0.1", "listen_port": 19482}],
            "outbounds": [dict(ob, tag="proxy"), {"type": "direct", "tag": "direct"}],
            "route": {"final": "proxy"},
        }
    cases["router-full"] = router_config()
    if not subprocess.run(["docker", "images", "-q", IMAGE], capture_output=True, text=True).stdout.strip():
        sys.exit(f"нет образа {IMAGE} — сначала tools/awg-stand/engine_test.py")
    script = ["cd /w"]
    for name, cfg in cases.items():
        with open(os.path.join(OUT, f"{name}.sb.json"), "w", newline="\n", encoding="utf-8") as f:
            json.dump(cfg, f, indent=1, ensure_ascii=False)
        script.append(f"lua /t/sb2mihomo.lua {name}.sb.json {name}.mihomo.json || echo FAIL {name}")
    r = subprocess.run(["docker", "run", "--rm", "-v", f"{OUT}:/w",
                        "-v", f"{os.path.join(ROOT, 'router_files')}:/t:ro", IMAGE, "sh", "-c", "; ".join(script)],
                       capture_output=True, text=True)
    print(r.stdout, r.stderr)
    # выход Lua — одной строкой; для чтения в диффах переформатируем
    for name in cases:
        p = os.path.join(OUT, f"{name}.mihomo.json")
        with open(p, encoding="utf-8") as f:
            v = json.load(f)
        with open(p, "w", newline="\n", encoding="utf-8") as f:
            json.dump(v, f, indent=1, ensure_ascii=False, sort_keys=True)
    print(f"{len(cases)} эталонов в {OUT}")
    sys.exit(1 if "FAIL" in r.stdout else 0)


if __name__ == "__main__":
    main()
