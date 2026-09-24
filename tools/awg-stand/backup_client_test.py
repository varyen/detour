"""Полная резервная копия между роутером и приложением.

1. OpenWrt в Docker: засеять состояние, `detour-backup.lua export`.
2. Windows dev-служба (`detour-svc run --dev-http`): импорт этой копии.
3. Проверить профили, цепочки, маршруты, цели проверки, подписки.
4. Экспорт полной копии из приложения → импорт обратно на OpenWrt.

    python tools/awg-stand/backup_client_test.py [--svc client/target/debug/detour-svc.exe]
"""
import argparse, json, os, shutil, subprocess, sys, tempfile, time, urllib.parse, urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
API = "http://127.0.0.1:18091/cgi-bin/detour-api"
IMAGE = "detour-owrt-engine:x86-64-23.05.5"
fails = 0


def check(name, ok, detail=""):
    global fails
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" — {detail}" if detail and not ok else ""))
    fails += 0 if ok else 1


def call(action, body=None):
    data = None if body is None else (body if isinstance(body, bytes) else json.dumps(body).encode())
    req = urllib.request.Request(f"{API}?{urllib.parse.urlencode({'action': action})}", data=data,
                                 method="POST" if data is not None else "GET")
    try:
        text = urllib.request.urlopen(req, timeout=60).read().decode()
    except urllib.error.HTTPError as e:
        text = e.read().decode()
    try:
        return json.loads(text)
    except ValueError:
        return text


SEED = r"""
set -e
SB=/tmp/bk/sb; DD=/tmp/bk/dd
mkdir -p $SB/profiles $DD/subscriptions /tmp/bk/z
printf '{"id":"nl1","name":"NL","type":"vless","uri":"vless://00000000-0000-0000-0000-000000000000@vpn.example.com:443?security=tls#NL","outbound":{"type":"vless","server":"vpn.example.com","server_port":443,"uuid":"00000000-0000-0000-0000-000000000000","tls":{"enabled":true,"server_name":"vpn.example.com","alpn":[]}}}' > $SB/profiles/nl1.json
printf '{"id":"de1","name":"DE","type":"trojan","outbound":{"type":"trojan","server":"de.example.com","server_port":443,"password":"x"}}' > $SB/profiles/de1.json
printf '{"active_chain":"nl1","routing_mode":"list"}' > $SB/settings.json
printf '{"chains":[{"id":"c1","name":"NL→DE","hops":["nl1","de1"],"created":1}]}' > $SB/chains.json
printf 'example.com\n' > $SB/proxy-domains.list
printf 'site.example.com de1\n' > $SB/route-map.list
printf 'YT|https://www.youtube.com/generate_204\n' > $SB/health-urls.list
printf 'de1\n' > $SB/torrent-allow.list
printf '{"id":"s1","name":"Example VPN","url":"https://sub.example.com/x"}' > $DD/subscriptions/s1.json
printf '1|1|https|8443|tcp|192.168.1.5|8123|http||||||1\n' > $DD/portmap.conf
lua /rf/detour-backup.lua export $SB $DD /tmp/bk/z/tpws.conf /tmp/bk/z/domains.list 9.9.9 openwrt
"""

REIMPORT = r"""
set -e
mkdir -p /tmp/r/sb /tmp/r/dd /tmp/r/z
lua /rf/detour-backup.lua import /io/client.json /tmp/r/sb /tmp/r/dd /tmp/r/z/tpws.conf /tmp/r/z/domains.list
echo
ls /tmp/r/sb/profiles
cat /tmp/r/sb/route-map.list
grep -c '"alpn":\[\]' /tmp/r/sb/profiles/nl1.json || true
"""


def docker(script, io_dir=None):
    args = ["docker", "run", "--rm", "--entrypoint", "/bin/sh",
            "-v", f"{os.path.join(ROOT, 'router_files')}:/rf:ro"]
    if io_dir:
        args += ["-v", f"{io_dir}:/io"]
    r = subprocess.run(args + [IMAGE, "-c", script], capture_output=True, text=True, encoding="utf-8",
                       env={**os.environ, "MSYS_NO_PATHCONV": "1"})
    return r.stdout, r.stderr


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--svc", default=os.path.join(ROOT, "client", "target", "debug", "detour-svc.exe"))
    a = ap.parse_args()

    out, err = docker(SEED)
    try:
        router_doc = json.loads(out)
    except ValueError:
        sys.exit(f"роутерный экспорт не JSON:\n{out[:400]}\n{err[-400:]}")
    check("router export", router_doc.get("kind") == "full" and len(router_doc.get("profiles", [])) == 2)

    data = tempfile.mkdtemp(prefix="detour-bk-")
    svc = subprocess.Popen([a.svc, "run", "--data", data, "--dev-http", "127.0.0.1:18091"],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(60):
            try:
                urllib.request.urlopen(f"{API}?action=status", timeout=2)
                break
            except Exception:
                time.sleep(0.5)
        r = call("panel_import_config", router_doc)
        check("client import ok", isinstance(r, dict) and r.get("ok"), str(r)[:300])
        check("client restored 2 profiles", isinstance(r, dict) and r.get("profiles") == 2, str(r)[:300])

        pl = call("profiles_list")
        ids = sorted(p.get("id") for p in (pl.get("profiles") if isinstance(pl, dict) else pl) or [])
        check("profiles listed", ids == ["de1", "nl1"], str(ids))
        full = call("backup_export")
        check("client export kind", isinstance(full, dict) and full.get("kind") == "full")
        check("client chains", (full.get("chains") or {}).get("chains", [{}])[0].get("hops") == ["nl1", "de1"],
              json.dumps(full.get("chains"))[:200])
        check("client route_map", "site.example.com de1" in full.get("route_map", ""))
        check("client health_urls", "youtube" in full.get("health_urls", ""))
        check("client torrent_allow", "de1" in full.get("torrent_allow", ""))
        check("client subscriptions", any(s.get("id") == "s1" for s in full.get("subscriptions", [])))
        check("client active chain", full.get("settings", {}).get("active_chain") in ("nl1", ["nl1"]),
              str(full.get("settings", {}).get("active_chain")))
        check("client skips router_files", "router_files" not in full)

        io_dir = tempfile.mkdtemp(prefix="detour-bk-io-")
        with open(os.path.join(io_dir, "client.json"), "w", encoding="utf-8") as f:
            json.dump(full, f, ensure_ascii=False)
        out, err = docker(REIMPORT, io_dir)
        check("router re-import", '"ok":true' in out, (out + err)[-400:])
        check("router got profiles back", "nl1.json" in out and "de1.json" in out, out[-300:])
        check("router got route map back", "site.example.com de1" in out)
        shutil.rmtree(io_dir, ignore_errors=True)
    finally:
        svc.terminate()
        svc.wait(10)
        shutil.rmtree(data, ignore_errors=True)
    print("ALL PASS" if not fails else f"{fails} FAILED")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
