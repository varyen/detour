#!/usr/bin/env python3
"""Режимы движка на OpenWrt в Docker: singbox / hybrid / mihomo через init.

Использует серверы proto_test (sing-box со всеми протоколами) и AWG-сервер
(случай awg-2.0). Внутри OpenWrt стоят настоящие /etc/init.d/sing-box,
detour-api, detour-awg (+init), sb2mihomo.lua, sing-box и mihomo.

Трафик роутера в init'е ловится только с br-lan, поэтому для проверки
локальный curl заворачивается в те же порты правилами nat OUTPUT:
  цель proto-target:80   → :12345 (основная цепочка)
  10.66.0.1:8000 (AWG)   → :12401 (вход цели маршрута «awgtest»)

    python tools/awg-stand/engine_test.py [--image …] [--keep]
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
TOK = "c" * 64


def check(name, ok, detail=""):
    results.append(bool(ok))
    print(f"[{'PASS' if ok else 'FAIL'}] {name}")
    if not ok and detail:
        print("   " + str(detail).strip().replace("\n", "\n   ")[:2500])


def cgi(action, body=None):
    if body is None:
        cmd = (f"HTTP_COOKIE=detour_session={TOK} REQUEST_METHOD=GET QUERY_STRING='action={action}' "
               "/www/cgi-bin/detour-api")
    else:
        data = json.dumps(body)
        rt.sh_in(f"cat > /tmp/cgi-body <<'EOF'\n{data}\nEOF")
        n = len(data.encode()) + 1
        cmd = (f"HTTP_COOKIE=detour_session={TOK} REQUEST_METHOD=POST QUERY_STRING='action={action}' "
               f"CONTENT_LENGTH={n} /www/cgi-bin/detour-api < /tmp/cgi-body")
    out = rt.sh_in(cmd)
    js = out[out.find("{"):] if "{" in out else out
    try:
        return json.loads(js)
    except ValueError:
        return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", default="openwrt/rootfs:x86-64-23.05.5")
    ap.add_argument("--keep", action="store_true")
    a = ap.parse_args()

    # серверы протоколов: переиспользуем proto_test (--keep оставит их)
    import subprocess
    r = subprocess.run([sys.executable, os.path.join(stand.HERE, "proto_test.py"), "--image", a.image,
                        "--keep", "--only", "trojan"], capture_output=True, text=True,
                       env=dict(os.environ, PYTHONUTF8="1"))
    if " прошло" not in r.stdout or "FAIL" in r.stdout:
        print(r.stdout[-2000:], r.stderr[-2000:])
        sys.exit("proto_test не поднял серверы")
    target = stand.sh("docker", "inspect", "-f", "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}",
                      "proto-target")

    # AWG-сервер
    keys = stand.gen_keys()
    case = next(c for c in stand.CASES if c["name"] == "awg-2.0")
    sname = "awg-srv-engine"
    cdir = os.path.join(stand.CACHE, "engine-case")
    os.makedirs(cdir, exist_ok=True)
    with open(os.path.join(cdir, "uapi"), "w", newline="\n") as f:
        f.write(stand.server_uapi(case, keys))
    stand.sh("docker", "rm", "-f", sname, check=False)
    stand.sh("docker", "run", "-d", "--name", sname, "--network", stand.NET, "--cap-add", "NET_ADMIN",
             "--device", "/dev/net/tun", "--sysctl", "net.ipv4.ip_forward=1", "-v", f"{cdir}:/cfg:ro",
             f"detour-awg-srv:{stand.SERVERS[case['server']]}")
    time.sleep(1.5)
    awg_ip = stand.sh("docker", "inspect", "-f", "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}", sname)

    # OpenWrt с NET_ADMIN: netifd перенастроил бы eth0 и контейнер остался бы без
    # сети, поэтому пакеты ставятся в образ заранее, а network/firewall выключены.
    img = "detour-owrt-engine:" + a.image.split(":")[-1]
    if not stand.sh("docker", "images", "-q", img, check=False):
        stand.sh("docker", "rm", "-f", "owrt-prep", check=False)
        stand.sh("docker", "run", "-d", "--name", "owrt-prep", a.image, "sleep", "3600")
        stand.sh("docker", "exec", "owrt-prep", "sh", "-c",
                 "mkdir -p /var/lock; if command -v apk >/dev/null; then apk update >/dev/null 2>&1; "
                 "for k in lua lua-cjson curl ipset iptables-nft iptables-mod-tproxy; do apk add $k >/dev/null 2>&1; done; "
                 "else opkg update >/dev/null 2>&1; for k in lua lua-cjson curl ipset iptables-nft "
                 "iptables-mod-tproxy; do opkg install $k >/dev/null 2>&1; done; fi; "
                 "for sv in network firewall odhcpd; do /etc/init.d/$sv disable 2>/dev/null; done; true", check=False)
        stand.sh("docker", "commit", "owrt-prep", img)
        stand.sh("docker", "rm", "-f", "owrt-prep", check=False)
    stand.sh("docker", "rm", "-f", CT, check=False)
    stand.sh("docker", "run", "-d", "--name", CT, "--network", stand.NET, "--cap-add", "NET_ADMIN",
             img, "/sbin/init")
    time.sleep(4)
    # загрузка OpenWrt опускает eth0 — возвращаем адрес, который выдал Docker
    net = json.loads(stand.sh("docker", "inspect", "-f", "{{json .NetworkSettings.Networks}}", CT))
    n0 = next(iter(net.values()))
    rt.sh_in(f"ip link set eth0 up; ip addr add {n0['IPAddress']}/{n0['IPPrefixLen']} dev eth0 2>/dev/null; "
             f"ip route add default via {n0['Gateway']} 2>/dev/null; true")
    check("lua/iptables на месте", "ok" in rt.sh_in("command -v lua && command -v iptables && echo ok"),
          rt.sh_in("command -v lua iptables ipset; ip -4 addr"))
    rt.sh_in(f"mkdir -p /www/cgi-bin /etc/sing-box/profiles /usr/share/detour /etc/detour /tmp/detour-sessions; "
             f"touch /tmp/detour-sessions/{TOK}")
    rt.cp_in(os.path.join(rt.RF, "detour-api"), "/www/cgi-bin/detour-api")
    rt.cp_in(os.path.join(rt.RF, "detour-awg"), "/usr/sbin/detour-awg")
    rt.cp_in(os.path.join(rt.RF, "detour-awg.initd"), "/etc/init.d/detour-awg")
    rt.cp_in(os.path.join(rt.RF, "sing-box.initd"), "/etc/init.d/sing-box")
    rt.cp_in(os.path.join(rt.RF, "sb2mihomo.lua"), "/usr/share/detour/sb2mihomo.lua", "0644")
    rt.cp_in(os.path.join(rt.RF, "detour-health"), "/usr/sbin/detour-health")
    rt.sh_in("echo 'gstatic|https://www.gstatic.com/generate_204' > /etc/sing-box/health-urls.list")
    rt.cp_in(rt.fetch_singbox(), "/usr/bin/sing-box")
    rt.cp_in(stand.fetch_mihomo(), "/usr/bin/mihomo")
    rt.sh_in("/etc/init.d/detour-awg enable; touch /etc/sing-box/proxy-domains.list")

    # профили: берём из proto_test (они уже в /etc/sing-box/profiles старого контейнера — пересоздаём)
    work = os.path.join(stand.CACHE, "proto")
    for pid in ("trojan", "vless-ws", "ss"):
        rt.cp_in(os.path.join(work, f"p-{pid}.json"), f"/etc/sing-box/profiles/{pid}.json", "0644")
    amnezia = dict(case["shared"]); amnezia.update(case["client"])
    awgp = {"id": "awgtest", "name": "AWG", "type": "amneziawg", "outbound": {
        "type": "amneziawg", "server": awg_ip, "server_port": 51820, "private_key": keys["c"][0],
        "peer_public_key": keys["s"][1], "local_address": ["10.66.0.2/32"], "mtu": 1280, "amnezia": amnezia}}
    p = os.path.join(cdir, "awgtest.json")
    with open(p, "w", newline="\n") as f:
        json.dump(awgp, f)
    rt.cp_in(p, "/etc/sing-box/profiles/awgtest.json", "0644")
    route_map = "// === route:awgtest ===\n10.66.0.1\n"
    rt.sh_in(f"printf '%s' '{route_map}' > /etc/sing-box/route-map.list")
    # торренты разрешены на всех профилях — иначе mihomo-режим всегда откатывается в гибрид
    rt.sh_in("printf 'trojan\\nvless-ws\\nss\\nawgtest\\n' > /etc/sing-box/torrent-allow.list")

    rt.sh_in(f"iptables -t nat -A OUTPUT -p tcp -d {target} --dport 80 -j REDIRECT --to-ports 12345; "
             f"iptables -t nat -A OUTPUT -p tcp -d 10.66.0.1 --dport 8000 -j REDIRECT --to-ports 12401")

    def traffic(label, main_ok=True, route_ok=True):
        time.sleep(3)
        m = rt.sh_in(f"curl -s -m 10 http://{target}/")
        check(f"{label}: основная цепочка", ("proto-ok" in m) == main_ok, m)
        rr = rt.sh_in("curl -s -m 10 http://10.66.0.1:8000/")
        check(f"{label}: цель маршрута → AWG", ("awg-ok" in rr) == route_ok, rr)

    def procs():
        return rt.sh_in("ps w | grep -E 'sing-box run|mihomo -d' | grep -v grep")

    # --- гибрид (по умолчанию)
    out = rt.sh_in("/www/cgi-bin/detour-api activate vless-ws,trojan 2>&1")
    check("activate в гибриде", "activate:" in out, out)
    ec = cgi("engine_config")
    check("engine_config: гибрид, движок sing-box", isinstance(ec, dict) and ec.get("mode") == "hybrid"
          and ec.get("effective") == "hybrid", ec)
    pr = procs()
    check("гибрид: sing-box + сайдкар", "sing-box run" in pr and "detour-awg" in pr, pr)
    traffic("гибрид")

    # --- mihomo
    r = cgi("engine_config", {"mode": "mihomo", "torrent_singbox": True})
    check("переключение в mihomo", isinstance(r, dict) and r.get("ok"), r)
    ec = cgi("engine_config")
    check("engine_config: mihomo в работе", isinstance(ec, dict) and ec.get("effective") == "mihomo", ec)
    pr = procs()
    check("mihomo: один процесс mihomo, sing-box и сайдкар не работают",
          "detour-engine" in pr and "sing-box run" not in pr and "detour-awg" not in pr, pr)
    traffic("mihomo")

    # AWG вторым звеном — только в mihomo
    out = rt.sh_in("/www/cgi-bin/detour-api activate ss,awgtest 2>&1")
    check("mihomo: цепочка ss → AWG собирается", "activate:" in out, out)
    traffic("mihomo, цепочка ss → AWG")

    # health и временные экземпляры (подписки/WARP/внешний IP) — тоже через mihomo
    for pid in ("awgtest", "trojan"):
        out = rt.sh_in(f"rm -f /tmp/detour-health.db; /usr/sbin/detour-health one {pid} 2>&1; "
                       "cat /tmp/detour-health.db 2>/dev/null")
        line = next((l for l in out.splitlines() if l.startswith(pid + "	")), "")
        check(f"mihomo: detour-health one {pid}", line.split("	")[1:2] == ["1"], out)
    out = rt.sh_in("/www/cgi-bin/detour-api render-mixed ss,awgtest 19601 > /tmp/em.json && "
                   "(/www/cgi-bin/detour-api engine-run /tmp/em.json /tmp/em.d >/tmp/em.log 2>&1 &) ; sleep 2; "
                   "curl -s -m 10 -x socks5h://127.0.0.1:19601 http://10.66.0.1:8000/; "
                   "ps w | grep '[e]m.json.mihomo' | awk '{print $1}' | xargs kill 2>/dev/null; true")
    check("mihomo: engine-run (подписки/WARP) через ss → AWG", "awg-ok" in out, out + rt.sh_in("tail -3 /tmp/em.log"))

    # запрет торрентов на цепочке → откат в гибрид
    rt.sh_in("printf 'trojan\\nss\\nawgtest\\n' > /etc/sing-box/torrent-allow.list")
    out = rt.sh_in("/www/cgi-bin/detour-api activate vless-ws,trojan 2>&1")
    ec = cgi("engine_config")
    check("mihomo + запрет торрентов на цепочке → гибрид", isinstance(ec, dict)
          and ec.get("effective") == "hybrid", [out, ec])
    traffic("откат в гибрид")
    r = cgi("engine_config", {"mode": "mihomo", "torrent_singbox": False})
    ec = cgi("engine_config")
    check("флажок выключен → mihomo даже с запретом", isinstance(ec, dict) and ec.get("effective") == "mihomo", [r, ec])
    traffic("mihomo без отката")

    # --- только sing-box: AWG в карте маршрутов — отказ
    r = cgi("engine_config", {"mode": "singbox"})
    check("singbox + AWG-цель маршрута → понятный отказ", isinstance(r, dict) and not r.get("ok")
          and "AmneziaWG" in str(r.get("error", r)), r)
    rt.sh_in("rm -f /etc/sing-box/route-map.list")
    r = cgi("engine_config", {"mode": "singbox"})
    check("singbox без AWG применяется", isinstance(r, dict) and r.get("ok"), r)
    pr = procs()
    check("singbox: только sing-box", "sing-box run" in pr and "mihomo" not in pr, pr)
    traffic("singbox", route_ok=False)

    if not a.keep:
        stand.sh("docker", "rm", "-f", CT, sname, "proto-srv", "proto-target", check=False)
    print(f"\n{sum(results)}/{len(results)} прошло")
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
