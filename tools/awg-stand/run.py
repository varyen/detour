#!/usr/bin/env python3
"""Стенд AmneziaWG: три сервера amneziawg-go (AWG 1.5 / 2.0 / 3.x) и mihomo-клиент.

Для каждого случая поднимает сервер в Docker, клиент mihomo с wireguard +
amnezia-wg-option и проверяет, что http://10.66.0.1:8000 (доступен только
внутри туннеля) отвечает через socks5 mihomo. Плюс отрицательный случай —
несовпадающий H4 обязан НЕ пройти.

    python tools/awg-stand/run.py [--mihomo PATH] [--keep]

--mihomo — готовый linux-amd64 бинарник (по умолчанию качается релиз из
MetaCubeX/mihomo в tools/awg-stand/.cache).
"""
import argparse
import base64
import gzip
import json
import os
import subprocess
import sys
import tempfile
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
CACHE = os.path.join(HERE, ".cache")
NET = "detour-awg"
MIHOMO_VER = "v1.19.31"

SERVERS = {
    "v1.5": "v0.2.13",
    "v2": "v0.2.19",
    "v3": "v3.1.20260828",
}


def sh(*cmd, check=True, capture=True, inp=None):
    r = subprocess.run(cmd, capture_output=capture, text=True, input=inp)
    if check and r.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)}\n{r.stdout}\n{r.stderr}")
    return (r.stdout or "").strip()


def b64_to_hex(k):
    return base64.b64decode(k).hex()


def gen_keys():
    out = sh("docker", "run", "--rm", "alpine:3.20", "sh", "-c",
             "apk add -q wireguard-tools-wg >/dev/null 2>&1; "
             "for n in s c h; do k=$(wg genkey); echo $n $k $(echo $k | wg pubkey); done")
    keys = {}
    for line in out.splitlines():
        n, priv, pub = line.split()
        keys[n] = (priv, pub)
    return keys


def fetch_mihomo():
    os.makedirs(CACHE, exist_ok=True)
    dst = os.path.join(CACHE, f"mihomo-{MIHOMO_VER}")
    if os.path.exists(dst):
        return dst
    url = (f"https://github.com/MetaCubeX/mihomo/releases/download/{MIHOMO_VER}/"
           f"mihomo-linux-amd64-v1-{MIHOMO_VER}.gz")
    print(f"качаю {url}")
    with urllib.request.urlopen(url) as r:
        data = gzip.decompress(r.read())
    with open(dst, "wb") as f:
        f.write(data)
    return dst


# Параметры обфускации: общие (обязаны совпасть на обеих сторонах) и
# клиентские (junk/сигнатурные пакеты перед рукопожатием, серверу не нужны).
CASES = [
    {
        "name": "awg-1.5",
        "server": "v1.5",
        "shared": {"s1": 15, "s2": 21, "h1": "1234567", "h2": "2345678",
                   "h3": "3456789", "h4": "4567890"},
        "client": {"jc": 4, "jmin": 40, "jmax": 70,
                   "i1": "<b 0xc2000000011088><r 32><t>", "i2": "<r 64>",
                   "j1": "<r 40>", "j2": "<b 0xdeadbeef><r 10>", "itime": 60},
        "mihomo_extra": {},
        "expect": True,
    },
    {
        "name": "awg-2.0",
        "server": "v2",
        "shared": {"s1": 15, "s2": 21, "s3": 19, "s4": 23,
                   "h1": "100000000-100100000", "h2": "200000000-200100000",
                   "h3": "300000000-300100000", "h4": "400000000-400100000"},
        "client": {"jc": 5, "jmin": 50, "jmax": 900,
                   "i1": "<b 0xc2000000011088><r 32><c><t>"},
        "mihomo_extra": {},
        "expect": True,
    },
    {
        "name": "awg-2.0-wrong-h4",
        "server": "v2",
        "shared": {"s1": 15, "s2": 21, "s3": 19, "s4": 23,
                   "h1": "100000000-100100000", "h2": "200000000-200100000",
                   "h3": "300000000-300100000", "h4": "400000000-400100000"},
        "client_override": {"h4": "500000000-500100000"},
        "client": {"jc": 3, "jmin": 50, "jmax": 100},
        "mihomo_extra": {},
        "expect": False,
    },
    {
        "name": "awg-3",
        "server": "v3",
        "shared": {"s1": 16, "s2": 24, "s3": 20, "s4": 14,
                   "h1": "100000000-100100000", "h2": "200000000-200100000",
                   "h3": "300000000-300100000", "h4": "400000000-400100000",
                   "content_padding_addition": "0-32"},
        "client": {"jc": 4, "jmin": 40, "jmax": 300, "i1": "<r 48>"},
        "hpk": True,
        "mihomo_extra": {"version": 3},
        "expect": True,
    },
    {
        "name": "awg-3-as-legacy",
        "server": "v3",
        "shared": {"s1": 16, "s2": 24, "s3": 20, "s4": 14,
                   "h1": "100000000-100100000", "h2": "200000000-200100000",
                   "h3": "300000000-300100000", "h4": "400000000-400100000",
                   "content_padding_addition": "0-32"},
        "client": {"jc": 4, "jmin": 40, "jmax": 300},
        "hpk": True,
        "hpk_client_off": True,
        "mihomo_extra": {},
        "expect": False,
    },
]


def server_uapi(case, keys):
    lines = [f"private_key={b64_to_hex(keys['s'][0])}", "listen_port=51820"]
    for k, v in case["shared"].items():
        lines.append(f"{k}={v}")
    if case.get("hpk"):
        lines.append(f"header_protection_key={b64_to_hex(keys['h'][0])}")
    lines += [f"public_key={b64_to_hex(keys['c'][1])}", "allowed_ip=10.66.0.2/32"]
    return "\n".join(lines) + "\n"


def mihomo_yaml(case, keys, server_ip):
    opt = dict(case["shared"])
    opt.update(case["client"])
    opt.update(case.get("client_override", {}))
    opt.update(case["mihomo_extra"])
    if "content_padding_addition" in opt:
        opt["content-padding-addition"] = opt.pop("content_padding_addition")
    if case.get("hpk_client_off"):
        opt.pop("content-padding-addition", None)
    elif case.get("hpk"):
        opt["header-protection-key"] = keys["h"][0]
    proxy = {
        "name": "awg", "type": "wireguard", "server": server_ip, "port": 51820,
        "ip": "10.66.0.2", "private-key": keys["c"][0], "public-key": keys["s"][1],
        "allowed-ips": ["0.0.0.0/0"], "udp": True, "mtu": 1280,
        "amnezia-wg-option": opt,
    }
    cfg = {
        "mixed-port": 7890, "bind-address": "127.0.0.1", "allow-lan": False,
        "mode": "rule", "log-level": "info", "ipv6": False,
        "proxies": [proxy], "rules": ["MATCH,awg"],
    }
    return json.dumps(cfg, indent=1)


def run_case(case, keys, mihomo, tmp, keep):
    tag = SERVERS[case["server"]]
    sname = f"awg-srv-{case['name']}"
    cdir = os.path.join(tmp, case["name"])
    os.makedirs(cdir, exist_ok=True)
    with open(os.path.join(cdir, "uapi"), "w", newline="\n") as f:
        f.write(server_uapi(case, keys))
    sh("docker", "rm", "-f", sname, check=False)
    sh("docker", "run", "-d", "--name", sname, "--network", NET, "--cap-add", "NET_ADMIN",
       "--device", "/dev/net/tun", "-v", f"{cdir}:/cfg:ro", f"detour-awg-srv:{tag}")
    time.sleep(1.5)
    ip = sh("docker", "inspect", "-f", "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}", sname)
    with open(os.path.join(cdir, "config.yaml"), "w", newline="\n") as f:
        f.write(mihomo_yaml(case, keys, ip))
    script = ("apk add -q curl >/dev/null 2>&1; "
              "/m/mihomo -d /tmp/m -f /c/config.yaml >/tmp/m.log 2>&1 & "
              "sleep 2; "
              "out=$(curl -s -m 8 -x socks5h://127.0.0.1:7890 http://10.66.0.1:8000/); rc=$?; "
              "echo \"RESULT rc=$rc body=$out\"; "
              "grep -iE 'error|warn|amnezia' /tmp/m.log | head -5")
    out = sh("docker", "run", "--rm", "--network", NET,
             "-v", f"{os.path.dirname(mihomo)}:/m:ro", "-v", f"{cdir}:/c:ro",
             "alpine:3.20", "sh", "-c",
             f"cp /m/{os.path.basename(mihomo)} /tmp/mh && chmod +x /tmp/mh && "
             + script.replace("/m/mihomo", "/tmp/mh"), check=False)
    ok = "body=awg-ok" in out
    passed = ok == case["expect"]
    print(f"[{'PASS' if passed else 'FAIL'}] {case['name']:<18} server {tag:<14} "
          f"tunnel={'up' if ok else 'down'} (ожидалось {'up' if case['expect'] else 'down'})")
    if not passed:
        print("   " + out.replace("\n", "\n   "))
        print("   server log:\n   " + sh("docker", "logs", "--tail", "15", sname, check=False).replace("\n", "\n   "))
    if not keep:
        sh("docker", "rm", "-f", sname, check=False)
    return passed


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--mihomo")
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--only")
    a = ap.parse_args()
    mihomo = os.path.abspath(a.mihomo) if a.mihomo else fetch_mihomo()
    sh("docker", "network", "create", NET, check=False)
    keys = gen_keys()
    results = []
    with tempfile.TemporaryDirectory(dir=CACHE if os.path.isdir(CACHE) else None) as tmp:
        for case in CASES:
            if a.only and a.only not in case["name"]:
                continue
            results.append(run_case(case, keys, mihomo, tmp, a.keep))
    print(f"\n{sum(results)}/{len(results)} прошло")
    sys.exit(0 if all(results) else 1)


if __name__ == "__main__":
    main()
