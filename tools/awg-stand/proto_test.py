#!/usr/bin/env python3
"""Паритет протоколов: один и тот же профиль через sing-box и через mihomo.

Поднимает в Docker:
  * target  — HTTP-цель (busybox httpd), доступная только из сети стенда;
  * proto   — sing-box-сервер со входами всех протоколов, которые умеет панель;
  * owrt    — OpenWrt с detour-api, sb2mihomo.lua, sing-box и mihomo.

Для каждого протокола кладёт профиль, строит `detour-api render-mixed` и гонит
curl к цели через: (а) sing-box на этом конфиге, (б) mihomo на переводе
sb2mihomo. Отдельно — цепочка из двух хопов (vless → trojan) в обоих движках.

    python tools/awg-stand/proto_test.py [--image openwrt/rootfs:x86-64-23.05.5] [--keep]
"""
import argparse
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run as stand  # noqa: E402
import router_test as rt  # noqa: E402

CT = rt.CT
results = []


def check(name, ok, detail=""):
    results.append(bool(ok))
    print(f"[{'PASS' if ok else 'FAIL'}] {name}")
    if not ok and detail:
        print("   " + str(detail).strip().replace("\n", "\n   ")[:3000])


def sh_c(ct, cmd):
    return stand.sh("docker", "exec", ct, "sh", "-c", cmd, check=False)


def ip_of(ct):
    return stand.sh("docker", "inspect", "-f", "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}", ct)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", default="openwrt/rootfs:x86-64-23.05.5")
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--only")
    a = ap.parse_args()
    os.makedirs(stand.CACHE, exist_ok=True)
    mihomo = stand.fetch_mihomo()
    singbox = rt.fetch_singbox()
    stand.sh("docker", "network", "create", stand.NET, check=False)
    work = os.path.join(stand.CACHE, "proto")
    os.makedirs(work, exist_ok=True)

    # цель
    stand.sh("docker", "rm", "-f", "proto-target", "proto-srv", CT, check=False)
    stand.sh("docker", "run", "-d", "--name", "proto-target", "--network", stand.NET, "alpine:3.20",
             "sh", "-c", "apk add -q busybox-extras >/dev/null 2>&1; mkdir -p /w && echo proto-ok > /w/index.html && httpd -f -p 80 -h /w")
    time.sleep(3)
    target = ip_of("proto-target")

    # сервер: сертификат, ключи reality, конфиг
    srv_cmd = ("apk add -q openssl >/dev/null 2>&1; cd /w; "
               "openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 30 "
               "-subj /CN=proto.example.com -keyout key.pem -out cert.pem >/dev/null 2>&1; "
               "./sing-box generate reality-keypair > reality.txt; "
               "./sing-box generate wg-keypair > wgs.txt; ./sing-box generate wg-keypair > wgc.txt; "
               "./sing-box generate rand --base64 16 > ss2022.txt")
    import shutil
    shutil.copy(singbox, os.path.join(work, "sing-box"))
    stand.sh("docker", "run", "--rm", "-v", f"{work}:/w", "alpine:3.20", "sh", "-c",
             "chmod +x /w/sing-box; " + srv_cmd)

    def kv(fname):
        out = {}
        for line in open(os.path.join(work, fname), encoding="utf-8"):
            if ":" in line:
                k, v = line.split(":", 1)
                out[k.strip()] = v.strip()
        return out
    reality = kv("reality.txt")
    wgs, wgc = kv("wgs.txt"), kv("wgc.txt")
    ss22 = open(os.path.join(work, "ss2022.txt")).read().strip()
    uuid = "b831381d-6324-4d53-ad4f-8cda48b30811"
    tls = {"enabled": True, "server_name": "proto.example.com",
           "certificate_path": "/w/cert.pem", "key_path": "/w/key.pem"}
    inbounds = [
        {"type": "trojan", "tag": "trojan", "listen": "::", "listen_port": 8443,
         "users": [{"password": "pw-trojan"}], "tls": tls},
        {"type": "vless", "tag": "vless-reality", "listen": "::", "listen_port": 8444,
         "users": [{"uuid": uuid, "flow": "xtls-rprx-vision"}],
         "tls": {"enabled": True, "server_name": "www.google.com",
                 "reality": {"enabled": True, "handshake": {"server": "www.google.com", "server_port": 443},
                             "private_key": reality["PrivateKey"], "short_id": ["0123abcd"]}}},
        {"type": "vless", "tag": "vless-ws", "listen": "::", "listen_port": 8445,
         "users": [{"uuid": uuid}], "tls": tls, "transport": {"type": "ws", "path": "/vl"}},
        {"type": "vmess", "tag": "vmess-ws", "listen": "::", "listen_port": 8446,
         "users": [{"uuid": uuid, "alterId": 0}], "transport": {"type": "ws", "path": "/vm"}},
        {"type": "shadowsocks", "tag": "ss", "listen": "::", "listen_port": 8447,
         "method": "aes-256-gcm", "password": "pw-ss"},
        {"type": "shadowsocks", "tag": "ss2022", "listen": "::", "listen_port": 8448,
         "method": "2022-blake3-aes-128-gcm", "password": ss22},
        {"type": "hysteria2", "tag": "hy2", "listen": "::", "listen_port": 8449,
         "users": [{"password": "pw-hy2"}], "obfs": {"type": "salamander", "password": "obfs-pw"},
         "tls": dict(tls, alpn=["h3"])},
        {"type": "tuic", "tag": "tuic", "listen": "::", "listen_port": 8450,
         "users": [{"uuid": uuid, "password": "pw-tuic"}], "congestion_control": "bbr",
         "tls": dict(tls, alpn=["h3"])},
        {"type": "socks", "tag": "socks", "listen": "::", "listen_port": 8451,
         "users": [{"username": "u", "password": "p"}]},
        {"type": "http", "tag": "http", "listen": "::", "listen_port": 8452,
         "users": [{"username": "u", "password": "p"}]},
        {"type": "vless", "tag": "vless-grpc", "listen": "::", "listen_port": 8454,
         "users": [{"uuid": uuid}], "tls": tls, "transport": {"type": "grpc", "service_name": "gsvc"}},
        {"type": "vmess", "tag": "vmess-hu", "listen": "::", "listen_port": 8455,
         "users": [{"uuid": uuid, "alterId": 0}], "transport": {"type": "httpupgrade", "path": "/hu"}},
    ]
    server = {
        "log": {"level": "warn"},
        "inbounds": inbounds,
        "endpoints": [{"type": "wireguard", "tag": "wg-in", "listen_port": 8453, "address": ["10.77.0.1/24"],
                       "private_key": wgs["PrivateKey"],
                       "peers": [{"public_key": wgc["PublicKey"], "allowed_ips": ["10.77.0.2/32"]}]}],
        "outbounds": [{"type": "direct", "tag": "direct"}],
    }
    with open(os.path.join(work, "server.json"), "w", newline="\n") as f:
        json.dump(server, f)
    stand.sh("docker", "run", "-d", "--name", "proto-srv", "--network", stand.NET, "-v", f"{work}:/w",
             "alpine:3.20", "/w/sing-box", "run", "-c", "/w/server.json")
    time.sleep(2)
    srv = ip_of("proto-srv")
    state = stand.sh("docker", "inspect", "-f", "{{.State.Running}}", "proto-srv", check=False)
    check("sing-box-сервер поднялся", state == "true", stand.sh("docker", "logs", "proto-srv", check=False))

    insecure = {"enabled": True, "server_name": "proto.example.com", "insecure": True}
    ob = {
        "trojan": {"type": "trojan", "server": srv, "server_port": 8443, "password": "pw-trojan", "tls": insecure},
        "vless-reality": {"type": "vless", "server": srv, "server_port": 8444, "uuid": uuid, "flow": "xtls-rprx-vision",
                          "tls": {"enabled": True, "server_name": "www.google.com",
                                  "utls": {"enabled": True, "fingerprint": "chrome"},
                                  "reality": {"enabled": True, "public_key": reality["PublicKey"], "short_id": "0123abcd"}}},
        "vless-ws": {"type": "vless", "server": srv, "server_port": 8445, "uuid": uuid, "tls": insecure,
                     "transport": {"type": "ws", "path": "/vl", "headers": {"Host": "proto.example.com"}}},
        "vless-grpc": {"type": "vless", "server": srv, "server_port": 8454, "uuid": uuid, "tls": insecure,
                       "transport": {"type": "grpc", "service_name": "gsvc"}},
        "vmess-ws": {"type": "vmess", "server": srv, "server_port": 8446, "uuid": uuid, "security": "auto",
                     "alter_id": 0, "transport": {"type": "ws", "path": "/vm"}},
        "vmess-hu": {"type": "vmess", "server": srv, "server_port": 8455, "uuid": uuid, "security": "auto",
                     "alter_id": 0, "transport": {"type": "httpupgrade", "path": "/hu"}},
        "ss": {"type": "shadowsocks", "server": srv, "server_port": 8447, "method": "aes-256-gcm", "password": "pw-ss"},
        "ss2022": {"type": "shadowsocks", "server": srv, "server_port": 8448,
                   "method": "2022-blake3-aes-128-gcm", "password": ss22},
        "hy2": {"type": "hysteria2", "server": srv, "server_port": 8449, "password": "pw-hy2",
                "obfs": {"type": "salamander", "password": "obfs-pw"}, "tls": dict(insecure, alpn=["h3"])},
        "tuic": {"type": "tuic", "server": srv, "server_port": 8450, "uuid": uuid, "password": "pw-tuic",
                 "congestion_control": "bbr", "tls": dict(insecure, alpn=["h3"])},
        "socks": {"type": "socks", "server": srv, "server_port": 8451, "version": "5", "username": "u", "password": "p"},
        "http": {"type": "http", "server": srv, "server_port": 8452, "username": "u", "password": "p"},
        "wg": {"type": "wireguard", "server": srv, "server_port": 8453, "private_key": wgc["PrivateKey"],
               "peer_public_key": wgs["PublicKey"], "local_address": ["10.77.0.2/32"], "mtu": 1280},
    }

    # OpenWrt
    stand.sh("docker", "run", "-d", "--name", CT, "--network", stand.NET, a.image, "/sbin/init")
    time.sleep(3)
    rt.sh_in("mkdir -p /var/lock; if command -v apk >/dev/null; then apk update >/dev/null 2>&1; "
             "apk add lua lua-cjson curl >/dev/null 2>&1; else opkg update >/dev/null 2>&1; "
             "opkg install lua lua-cjson curl >/dev/null 2>&1; fi")
    rt.sh_in("mkdir -p /www/cgi-bin /etc/sing-box/profiles /usr/share/detour /etc/detour")
    rt.cp_in(os.path.join(rt.RF, "detour-api"), "/www/cgi-bin/detour-api")
    rt.cp_in(os.path.join(rt.RF, "detour-awg"), "/usr/sbin/detour-awg")
    rt.cp_in(os.path.join(rt.RF, "sb2mihomo.lua"), "/usr/share/detour/sb2mihomo.lua", "0644")
    rt.cp_in(singbox, "/usr/bin/sing-box")
    rt.cp_in(mihomo, "/usr/bin/mihomo")
    for pid, o in ob.items():
        prof = {"id": pid, "name": pid, "type": o["type"], "outbound": dict(o, tag="proxy")}
        path = os.path.join(work, f"p-{pid}.json")
        with open(path, "w", newline="\n") as f:
            json.dump(prof, f)
        rt.cp_in(path, f"/etc/sing-box/profiles/{pid}.json", "0644")

    url = f"http://{target}/"
    runner = """
set -e
CH="$1"; PORT="$2"; ENG="$3"
/www/cgi-bin/detour-api render-mixed "$CH" "$PORT" > /tmp/sb-$PORT.json
if [ "$ENG" = singbox ]; then
  sing-box run -c /tmp/sb-$PORT.json -D /tmp/d-$PORT >/tmp/log-$PORT 2>&1 &
else
  lua /usr/share/detour/sb2mihomo.lua /tmp/sb-$PORT.json /tmp/mh-$PORT.json 2>/tmp/log-$PORT
  mkdir -p /tmp/d-$PORT
  mihomo -d /tmp/d-$PORT -f /tmp/mh-$PORT.json >/tmp/log-$PORT 2>&1 &
fi
P=$!
sleep 2
set +e
R=$(curl -s -m 12 -x socks5h://127.0.0.1:$PORT URL)
kill $P 2>/dev/null
echo "BODY=$R"
""".replace("URL", url)
    with open(os.path.join(work, "runner.sh"), "w", newline="\n") as f:
        f.write(runner)
    rt.cp_in(os.path.join(work, "runner.sh"), "/tmp/runner.sh")

    port = 19500
    cases = [(k, k) for k in ob] + [("цепочка vless-ws → trojan", "vless-ws,trojan"),
                                    ("цепочка socks → hy2", "socks,hy2")]
    for name, chain in cases:
        if a.only and a.only not in name:
            continue
        for eng in ("singbox", "mihomo"):
            port += 1
            out = rt.sh_in(f"sh /tmp/runner.sh '{chain}' {port} {eng}")
            ok = "BODY=proto-ok" in out
            detail = out + "\n" + rt.sh_in(f"tail -5 /tmp/log-{port}")
            if not ok and eng == "mihomo":
                detail += "\n" + rt.sh_in(f"head -c 1500 /tmp/mh-{port}.json")
            check(f"{name:<28} {eng}", ok, detail)

    if not a.keep:
        stand.sh("docker", "rm", "-f", CT, "proto-srv", "proto-target", check=False)
    print(f"\n{sum(results)}/{len(results)} прошло")
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
