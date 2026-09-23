#!/usr/bin/env python3
"""Клиент (Windows) с AmneziaWG: detour-svc в режиме разработки без TUN.

AWG-сервер (случай awg-3 из run.py, с NAT наружу) публикуется на
127.0.0.1:51899/udp; служба получает sing-box.exe и mihomo.exe в свой bin/,
профиль заводится через тот же HTTP-API, что у панели. Проверки:

  1. профиль сохраняется, активируется — сайдкар и sing-box запущены;
  2. с «Все через VPN» запрос через dev-in (127.0.0.1:18282) проходит;
  3. сервер остановлен — тот же запрос падает (трафик шёл именно через AWG);
  4. стоп VPN гасит и сайдкар.

    python tools/awg-stand/client_test.py [--svc client/target/debug/detour-svc.exe]
"""
import argparse
import io
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request
import zipfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run as stand  # noqa: E402

ROOT = os.path.abspath(os.path.join(stand.HERE, "..", ".."))
SB_VER = "1.14.1"
API = "http://127.0.0.1:18090/cgi-bin/detour-api"
results = []


def check(name, ok, detail=""):
    results.append(bool(ok))
    print(f"[{'PASS' if ok else 'FAIL'}] {name}")
    if not ok and detail:
        print("   " + str(detail).strip().replace("\n", "\n   "))


def fetch_zip_exe(url, name, dst):
    if os.path.exists(dst):
        return dst
    print(f"качаю {url}")
    with urllib.request.urlopen(url) as r:
        z = zipfile.ZipFile(io.BytesIO(r.read()))
    member = next(n for n in z.namelist() if n.endswith(".exe") and name in os.path.basename(n))
    with open(dst, "wb") as f:
        f.write(z.read(member))
    return dst


def call(action, params=None, body=None):
    q = {"action": action, **(params or {})}
    data = None if body is None else (body if isinstance(body, bytes) else json.dumps(body).encode())
    req = urllib.request.Request(f"{API}?{urllib.parse.urlencode(q)}", data=data,
                                 method="POST" if data is not None else "GET")
    try:
        text = urllib.request.urlopen(req, timeout=90).read().decode()
    except urllib.error.HTTPError as e:
        text = e.read().decode()
    try:
        return json.loads(text)
    except ValueError:
        return text


def via_vpn(url="https://www.gstatic.com/generate_204"):
    r = subprocess.run(["curl.exe", "-s", "-o", "NUL", "-w", "%{http_code}", "--max-time", "15",
                        "-x", "socks5h://127.0.0.1:18282", url], capture_output=True, text=True)
    return r.stdout.strip()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--svc", default=os.path.join(ROOT, "client", "target", "debug", "detour-svc.exe"))
    a = ap.parse_args()
    os.makedirs(stand.CACHE, exist_ok=True)
    sb = fetch_zip_exe(f"https://github.com/SagerNet/sing-box/releases/download/v{SB_VER}/"
                       f"sing-box-{SB_VER}-windows-amd64.zip", "sing-box",
                       os.path.join(stand.CACHE, f"sing-box-{SB_VER}.exe"))
    mh = fetch_zip_exe(f"https://github.com/MetaCubeX/mihomo/releases/download/{stand.MIHOMO_VER}/"
                       f"mihomo-windows-amd64-v1-{stand.MIHOMO_VER}.zip", "mihomo",
                       os.path.join(stand.CACHE, f"mihomo-{stand.MIHOMO_VER}.exe"))

    stand.sh("docker", "network", "create", stand.NET, check=False)
    keys = stand.gen_keys()
    case = next(c for c in stand.CASES if c["name"] == "awg-3")
    sname = "awg-srv-client"
    cdir = os.path.join(stand.CACHE, "client-case")
    os.makedirs(cdir, exist_ok=True)
    with open(os.path.join(cdir, "uapi"), "w", newline="\n") as f:
        f.write(stand.server_uapi(case, keys))
    stand.sh("docker", "rm", "-f", sname, check=False)
    stand.sh("docker", "run", "-d", "--name", sname, "--network", stand.NET, "--cap-add", "NET_ADMIN",
             "--device", "/dev/net/tun", "--sysctl", "net.ipv4.ip_forward=1", "-p", "51899:51820/udp",
             "-v", f"{cdir}:/cfg:ro", f"detour-awg-srv:{stand.SERVERS[case['server']]}")
    time.sleep(1.5)

    data = tempfile.mkdtemp(prefix="detour-awg-client-")
    os.makedirs(os.path.join(data, "bin"))
    import shutil
    shutil.copy(sb, os.path.join(data, "bin", "sing-box.exe"))
    shutil.copy(mh, os.path.join(data, "bin", "mihomo.exe"))
    env = dict(os.environ, DETOUR_DEV_NO_TUN="1")
    svc = subprocess.Popen([a.svc, "run", "--data", data, "--dev-http", "127.0.0.1:18090"],
                           env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(30):
            try:
                call("status")
                break
            except OSError:
                time.sleep(0.5)

        st = call("awg_status")
        check("awg_status: mihomo найден", isinstance(st, dict) and st.get("installed"), st)

        opt = dict(case["shared"])
        opt.update(case["client"])
        opt["content-padding-addition"] = opt.pop("content_padding_addition")
        opt["header-protection-key"] = keys["h"][0]
        opt["version"] = 3
        prof = {
            "id": "awg3", "name": "AWG 3", "type": "amneziawg", "group": "", "uri": "",
            "outbound": {
                "type": "amneziawg", "server": "127.0.0.1", "server_port": 51899,
                "private_key": keys["c"][0], "peer_public_key": keys["s"][1],
                "local_address": ["10.66.0.2/32"], "mtu": 1280, "amnezia": opt,
            },
        }
        r = call("profile_save", body=prof)
        check("profile_save", isinstance(r, dict) and r.get("ok"), r)
        r = call("profile_activate", {"name": "awg3"})
        check("profile_activate", isinstance(r, dict) and r.get("ok"), r)
        st = call("awg_status")
        check("сайдкар работает, профиль на 19600",
              st.get("running") and st.get("profiles") == [{"id": "awg3", "port": 19600}], st)
        r = call("allvpn_on")
        check("allvpn_on", isinstance(r, dict) and r.get("ok"), r)
        time.sleep(1)
        code = via_vpn()
        check("запрос через VPN (AWG 3) проходит", code == "204", code)

        stand.sh("docker", "stop", sname, check=False)
        time.sleep(1)
        code = via_vpn("https://www.google.com/generate_204")
        check("сервер остановлен — запрос падает (шёл через AWG)", code not in ("204", "200"), code)

        call("singbox_stop")
        time.sleep(1)
        st = call("awg_status")
        check("стоп VPN гасит сайдкар", not st.get("running"), st)
    finally:
        svc.terminate()
        stand.sh("docker", "rm", "-f", sname, check=False)
    print(f"\n{sum(results)}/{len(results)} прошло")
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
