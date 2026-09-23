#!/usr/bin/env python3
"""Роутерная часть AmneziaWG на стенде: OpenWrt в Docker вместо живого роутера.

Поднимает AWG-сервер (amneziawg-go, случай awg-2.0 из run.py) и контейнер
openwrt/rootfs с procd, кладёт туда detour-api / detour-awg / detour-health из
router_files/, sing-box и mihomo под x86_64 — и проверяет:

  1. detour-awg sync: порт выдан, конфиг mihomo собран, сайдкар запущен init'ом;
  2. render-mixed + sing-box: трафик идёт sing-box → socks → mihomo → AWG;
  3. AWG вторым звеном цепочки — рендер отказывает с понятной причиной;
  4. CGI profile_save / awg_status / profile_delete с сессией панели;
  5. detour-health one: функциональная проверка AWG-профиля через сайдкар.

    python tools/awg-stand/router_test.py [--image openwrt/rootfs:x86-64-23.05.5] [--keep]
"""
import argparse
import io
import json
import os
import sys
import tarfile
import time
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run as stand  # noqa: E402

ROOT = os.path.abspath(os.path.join(stand.HERE, "..", ".."))
RF = os.path.join(ROOT, "router_files")
SB_VER = "1.14.1"
CT = "detour-awg-owrt"

results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(f"[{'PASS' if ok else 'FAIL'}] {name}")
    if not ok and detail:
        print("   " + detail.strip().replace("\n", "\n   "))


def fetch_singbox():
    dst = os.path.join(stand.CACHE, f"sing-box-{SB_VER}")
    if os.path.exists(dst):
        return dst
    url = (f"https://github.com/SagerNet/sing-box/releases/download/v{SB_VER}/"
           f"sing-box-{SB_VER}-linux-amd64-musl.tar.gz")
    print(f"качаю {url}")
    with urllib.request.urlopen(url) as r:
        tf = tarfile.open(fileobj=io.BytesIO(r.read()), mode="r:gz")
        m = next(x for x in tf.getmembers() if x.name.endswith("/sing-box"))
        with open(dst, "wb") as f:
            f.write(tf.extractfile(m).read())
    return dst


def sh_in(cmd, check_rc=False):
    out = stand.sh("docker", "exec", CT, "sh", "-c", cmd, check=check_rc)
    return out


def cp_in(src, dst, mode="0755"):
    stand.sh("docker", "cp", src, f"{CT}:{dst}")
    sh_in(f"chmod {mode} {dst}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", default="openwrt/rootfs:x86-64-23.05.5")
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--ui", type=int, metavar="PORT",
                    help="оставить стенд с панелью на localhost:PORT (для Playwright)")
    a = ap.parse_args()

    os.makedirs(stand.CACHE, exist_ok=True)
    mihomo = stand.fetch_mihomo()
    singbox = fetch_singbox()
    stand.sh("docker", "network", "create", stand.NET, check=False)
    keys = stand.gen_keys()
    case = next(c for c in stand.CASES if c["name"] == "awg-2.0")

    # AWG-сервер
    sname = "awg-srv-router"
    cdir = os.path.join(stand.CACHE, "router-case")
    os.makedirs(cdir, exist_ok=True)
    with open(os.path.join(cdir, "uapi"), "w", newline="\n") as f:
        f.write(stand.server_uapi(case, keys))
    stand.sh("docker", "rm", "-f", sname, check=False)
    stand.sh("docker", "run", "-d", "--name", sname, "--network", stand.NET, "--cap-add", "NET_ADMIN",
             "--device", "/dev/net/tun", "--sysctl", "net.ipv4.ip_forward=1", "-v", f"{cdir}:/cfg:ro",
             f"detour-awg-srv:{stand.SERVERS[case['server']]}")
    time.sleep(1.5)
    srv_ip = stand.sh("docker", "inspect", "-f",
                      "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}", sname)

    # OpenWrt
    stand.sh("docker", "rm", "-f", CT, check=False)
    publish = ["-p", f"{a.ui}:8080"] if a.ui else []
    stand.sh("docker", "run", "-d", "--name", CT, "--network", stand.NET, *publish, a.image, "/sbin/init")
    time.sleep(3)
    print("opkg: lua-cjson, curl …")
    sh_in("mkdir -p /var/lock; if command -v apk >/dev/null; then "
          "apk update >/dev/null 2>&1; apk add lua lua-cjson curl >/dev/null 2>&1; else "
          "opkg update >/dev/null 2>&1; opkg install lua lua-cjson curl >/dev/null 2>&1; fi", check_rc=False)
    check("lua-cjson на месте", "ok" in sh_in("lua -e 'require(\"cjson.safe\") print(\"ok\")'"))

    sh_in("mkdir -p /www/cgi-bin /etc/sing-box/profiles /etc/detour /var/state /tmp/detour-sessions")
    cp_in(os.path.join(RF, "detour-api"), "/www/cgi-bin/detour-api")
    cp_in(os.path.join(RF, "detour-awg"), "/usr/sbin/detour-awg")
    cp_in(os.path.join(RF, "detour-awg.initd"), "/etc/init.d/detour-awg")
    cp_in(os.path.join(RF, "detour-health"), "/usr/sbin/detour-health")
    cp_in(singbox, "/usr/bin/sing-box")
    cp_in(mihomo, "/usr/bin/mihomo")
    sh_in("/etc/init.d/detour-awg enable")
    for f in ("detour-api", "detour-awg", "detour-awg.initd", "detour-health"):
        rc = sh_in(f"sh -n {'/www/cgi-bin/' if f == 'detour-api' else '/usr/sbin/'}{f} 2>&1 && echo OK"
                   if f != "detour-awg.initd" else "sh -n /etc/init.d/detour-awg 2>&1 && echo OK")
        check(f"busybox sh -n {f}", rc.endswith("OK"), rc)

    amnezia = dict(case["shared"])
    amnezia.update(case["client"])
    profile = {
        "id": "awgtest", "name": "AWG test", "type": "amneziawg", "group": "", "uri": "",
        "outbound": {
            "type": "amneziawg", "tag": "proxy", "server": srv_ip, "server_port": 51820,
            "private_key": keys["c"][0], "peer_public_key": keys["s"][1],
            "local_address": ["10.66.0.2/32"], "mtu": 1280, "amnezia": amnezia,
        },
    }
    vless = {"id": "vl", "name": "vl", "type": "vless", "outbound": {
        "type": "vless", "tag": "proxy", "server": "192.0.2.1", "server_port": 443,
        "uuid": "00000000-0000-0000-0000-000000000000"}}
    for p in (profile, vless):
        path = os.path.join(cdir, f"{p['id']}.json")
        with open(path, "w", newline="\n") as f:
            json.dump(p, f)
        cp_in(path, f"/etc/sing-box/profiles/{p['id']}.json", "0644")

    # 1. sync
    out = sh_in("/usr/sbin/detour-awg sync; echo rc=$?")
    check("detour-awg sync", out.endswith("rc=0"), out)
    ports = sh_in("cat /etc/sing-box/awg-ports")
    check("порт выдан (12600)", ports.strip() == "awgtest 12600", ports)
    time.sleep(2)
    st = sh_in("/usr/sbin/detour-awg status")
    try:
        stj = json.loads(st)
    except ValueError:
        stj = {}
    check("status: установлен и работает (procd)", stj.get("installed") and stj.get("running"), st)
    check("mihomo -t принимает конфиг", "successful" in sh_in("/usr/sbin/detour-awg check 2>&1").lower()
          or "test is successful" in sh_in("/usr/sbin/detour-awg check 2>&1"),
          sh_in("/usr/sbin/detour-awg check 2>&1"))

    # 2. сквозной путь sing-box → mihomo → AWG
    r = sh_in("/www/cgi-bin/detour-api render-mixed awgtest 19482 > /tmp/m.json; echo rc=$?; "
              "sing-box check -c /tmp/m.json 2>&1 && echo CHECK_OK; "
              "(sing-box run -c /tmp/m.json -D /tmp/sbd >/tmp/sb.log 2>&1 &); sleep 2; "
              "curl -s -m 10 -x socks5h://127.0.0.1:19482 http://10.66.0.1:8000/; echo; "
              "killall sing-box 2>/dev/null")
    check("render-mixed отдаёт socks на сайдкар", '"server_port":12600' in sh_in("tr -d ' \\n' < /tmp/m.json"),
          sh_in("cat /tmp/m.json"))
    check("sing-box check", "CHECK_OK" in r, r)
    check("трафик sing-box → mihomo → AWG", "awg-ok" in r, r + "\n" + sh_in("tail -5 /tmp/sb.log"))

    # 3. цепочки
    bad = sh_in("/www/cgi-bin/detour-api render-mixed vl,awgtest 19483 >/dev/null; echo rc=$?")
    check("AWG вторым звеном — отказ", bad.endswith("rc=1"), bad)
    good = sh_in("/www/cgi-bin/detour-api render-mixed awgtest,vl 19483 | tr -d ' \\n'")
    check("AWG первым звеном — vless через него (detour chain_1)",
          '"detour":"chain_1"' in good and '"server_port":12600' in good, good[:600])

    # 4. CGI
    tok = "a" * 64
    sh_in(f"touch /tmp/detour-sessions/{tok}")
    cgi = (f"HTTP_COOKIE=detour_session={tok} REQUEST_METHOD={{m}} QUERY_STRING='action={{a}}' "
           "CONTENT_LENGTH={n} /www/cgi-bin/detour-api")
    second = dict(profile, id="awgtest2", name="AWG test 2")
    body = json.dumps(second)
    path = os.path.join(cdir, "body.json")
    with open(path, "w", newline="\n") as f:
        f.write(body)
    cp_in(path, "/tmp/body.json", "0644")
    out = sh_in(cgi.format(n=len(body.encode()), m="POST", a="profile_save") + " < /tmp/body.json")
    check("CGI profile_save AWG", '"ok":true' in out.replace(" ", ""), out)
    ports = sh_in("cat /etc/sing-box/awg-ports")
    check("второй профиль получил 12601, первый остался на 12600",
          "awgtest 12600" in ports and "awgtest2 12601" in ports, ports)
    time.sleep(2)
    out = sh_in(cgi.format(n=0, m="GET", a="awg_status"))
    js = out[out.find("{"):] if "{" in out else "{}"
    try:
        stj = json.loads(js)
    except ValueError:
        stj = {}
    check("CGI awg_status: 2 профиля, работает", len(stj.get("profiles", [])) == 2 and stj.get("running"), out)
    out = sh_in(cgi.format(n=0, m="POST", a="profile_delete&name=awgtest2"))
    ports = sh_in("cat /etc/sing-box/awg-ports")
    check("profile_delete освобождает порт", "awgtest2" not in ports and "awgtest 12600" in ports, out + ports)

    # 5. detour-health
    sh_in("echo 'gstatic|https://www.gstatic.com/generate_204' > /etc/sing-box/health-urls.list; "
          "echo '{\"health_check_enabled\":\"1\",\"health_speed_enabled\":\"0\"}' > /etc/sing-box/settings.json")
    out = sh_in("/usr/sbin/detour-health one awgtest 2>&1; cat /tmp/detour-health.db 2>/dev/null")
    line = next((l for l in out.splitlines() if l.startswith("awgtest\t")), "")
    check("detour-health one: AWG-профиль живой", line.split("\t")[1:2] == ["1"] if line else False, out)

    # 6. профилей AWG не осталось — сайдкар гаснет
    sh_in("rm -f /etc/sing-box/profiles/awgtest.json; /usr/sbin/detour-awg sync; true")
    time.sleep(1)
    st = sh_in("/usr/sbin/detour-awg status")
    check("без AWG-профилей сайдкар остановлен", '"running":false' in st, st)

    if a.ui:
        # Стенд для панели: uhttpd с CGI и собранной Vue-панелью, AWG-профиль
        # возвращён на место, сессия — фиксированная.
        dist = os.path.join(ROOT, "panel", "dist")
        stand.sh("docker", "cp", dist + os.sep + ".", f"{CT}:/www/detour/")
        cp_in(os.path.join(cdir, "awgtest.json"), "/etc/sing-box/profiles/awgtest.json", "0644")
        sh_in("/usr/sbin/detour-awg sync; true")
        sh_in("uhttpd -p 0.0.0.0:8080 -h /www -x /cgi-bin -t 120")
        print(f"\nпанель: http://localhost:{a.ui}/detour/  cookie detour_session={tok}")
    elif not a.keep:
        stand.sh("docker", "rm", "-f", CT, sname, check=False)
    print(f"\n{sum(results)}/{len(results)} прошло")
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
