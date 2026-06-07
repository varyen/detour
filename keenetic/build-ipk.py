#!/usr/bin/env python3
"""Assemble an Entware-installable .ipk for the Keenetic KN-1810 (mipsel).

Output: releases/keenetic/detour-keenetic_<ver>_all.ipk
Install on the router:  opkg update && opkg install ./detour-keenetic_<ver>_all.ipk

⚠ EXPERIMENTAL — never run on hardware. This is the FIRST real-device validation:
  * float ABI of the mipsel bins is unverified (watch for "Error relocating"),
  * the NDM netfilter.d hook contract + LAN bridge name are assumed,
  * DNS+ipset is NOT wired yet → only explicit IPs/CIDRs route, domain-based
    proxying won't populate the ipsets until that's added.

Architecture is `all` on purpose: opkg side-loading a local .ipk accepts `all`
regardless of the host's exact mipselsf arch string.

SLIM build: sing-box is NOT bundled — it comes from the Entware feed via
`Depends: sing-box` (package `sing-box-go` 1.13.x, Provides: sing-box, installs
to /opt/bin/sing-box, ABI guaranteed-correct for mipsel-3.4 soft-float). Only
tpws is bundled (~127 KB) because zapret is not in the Entware feed. Net result:
panel ~90 KB + tpws ~127 KB instead of a 22.5 MB self-contained package, and the
float-ABI risk on sing-box disappears.

Run `keenetic/fetch-bins.py` first to populate keenetic/bins/ (tpws only needed).
"""
import io
import os
import sys
import tarfile
from datetime import datetime, timezone

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
BINS = os.path.join(HERE, "bins")
BACKUP = os.path.join(ROOT, "router-backup")
ROUTER_FILES = os.path.join(ROOT, "router_files")
OUT_DIR = os.path.join(ROOT, "releases", "keenetic")

PKG = "detour-keenetic"
ARCH = "all"
# Entware runtime deps — opkg pulls these from the mipselsf feed on install.
# `sing-box` resolves to the feed's `sing-box-go` (Provides: sing-box).
# NOT listed on purpose (would block opkg resolution — they are not standalone
# packages in the Entware feed):
#   * start-stop-daemon — a busybox applet (busybox is Essential, already present).
#   * lua-cjson — absent from the feed (only the C `cJSON`); shell CGI doesn't need it.
DEPENDS = ("sing-box, iptables, ipset, dnsmasq-full, lighttpd, lighttpd-mod-cgi, "
           "lighttpd-mod-setenv, lua, coreutils-base64, openssl-util, curl")

# (source_path, archive_path_under_opt, mode, fix_shebang)
# sing-box is NOT here — it is pulled from opkg (Depends: sing-box).
FILES = [
    (os.path.join(BINS, "tpws-zapret"), "opt/sbin/tpws-zapret", 0o755, False),
    # Source from router_files/ (canonical) — same source as the OpenWrt build, so
    # the Keenetic package never drifts behind. (router-backup/ was the old source.)
    (os.path.join(ROUTER_FILES, "index.html"), "opt/share/www/detour/index.html", 0o644, False),
    (os.path.join(ROUTER_FILES, "detour-api"), "opt/share/www/cgi-bin/detour-api", 0o755, True),
    # Subscription refresh helper (Lua) + a bundled pure-Lua cjson.safe, since the
    # Entware feed has no lua-cjson. fix_shebang rewrites #!/usr/bin/lua → /opt/bin/lua.
    (os.path.join(ROUTER_FILES, "subscription-refresh"), "opt/sbin/subscription-refresh", 0o755, True),
    (os.path.join(HERE, "lua", "cjson", "safe.lua"), "opt/share/lua/5.1/cjson/safe.lua", 0o644, False),
    # Hosts-DNS manager (shared source, already has a /opt platform shim) — serves
    # addn-hosts via the detour dnsmasq (S50detour-dns). fix_shebang → /opt/bin/sh.
    (os.path.join(ROUTER_FILES, "detour-hosts"), "opt/sbin/detour-hosts", 0o755, True),
    # Self-update (shared source, /opt shim for Keenetic): pulls detour-keenetic_*.ipk.
    (os.path.join(ROUTER_FILES, "detour-update"), "opt/sbin/detour-update", 0o755, True),
    # VPN endpoint health probe (shared source, /opt shim). ⚠ cron scheduling on
    # Entware/KeeneticOS is device-specific — set up a */5 cron manually if wanted.
    (os.path.join(ROUTER_FILES, "vpn-keepalive"), "opt/sbin/vpn-keepalive", 0o755, True),
    # Pinned usign public key (used by detour-update if usign is present on Entware).
    (os.path.join(ROOT, "keys", "release.usign.pub"), "opt/etc/detour/release.usign.pub", 0o644, False),
    (os.path.join(HERE, "init.d", "S51detour-panel"), "opt/etc/init.d/S51detour-panel", 0o755, False),
    # Domain→ipset DNS (Entware dnsmasq + transparent :53 redirect) — domain routing.
    (os.path.join(HERE, "init.d", "S50detour-dns"), "opt/etc/init.d/S50detour-dns", 0o755, False),
    (os.path.join(HERE, "init.d", "S52detour-singbox"), "opt/etc/init.d/S52detour-singbox", 0o755, False),
    (os.path.join(HERE, "init.d", "S53detour-zapret"), "opt/etc/init.d/S53detour-zapret", 0o755, False),
    (os.path.join(HERE, "ndm", "netfilter.d", "50-detour.sh"), "opt/etc/ndm/netfilter.d/50-detour.sh", 0o755, False),
    (os.path.join(HERE, "lighttpd", "detour.conf"), "opt/etc/lighttpd/detour.conf", 0o644, False),
    (os.path.join(HERE, "etc", "detour.conf"), "opt/etc/detour/detour.conf", 0o644, False),
]


def fix_shebang(data):
    txt = data.decode("utf-8")
    nl = txt.find("\n")
    first, rest = txt[:nl], txt[nl:]
    if first.startswith("#!"):
        first = first.replace("/bin/sh", "/opt/bin/sh").replace("/usr/bin/lua", "/opt/bin/lua")
    return (first + rest).encode("utf-8")


def add_bytes(tar, name, content, mode):
    info = tarfile.TarInfo(name)
    info.size = len(content); info.mode = mode
    info.uid = info.gid = 0; info.uname = info.gname = "root"
    info.mtime = int(datetime.now(timezone.utc).timestamp())
    tar.addfile(info, io.BytesIO(content))


def build_data():
    dirs = set()
    for _, dest, _, _ in FILES:
        parts = dest.split("/")
        for i in range(1, len(parts)):
            dirs.add("/".join(parts[:i]))
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.USTAR_FORMAT) as tar:
        for d in sorted(dirs):
            di = tarfile.TarInfo("./" + d + "/"); di.type = tarfile.DIRTYPE
            di.mode = 0o755; di.uid = di.gid = 0; di.uname = di.gname = "root"
            di.mtime = int(datetime.now(timezone.utc).timestamp())
            tar.addfile(di)
        total = 0
        for src, dest, mode, fix in FILES:
            if not os.path.isfile(src):
                sys.exit(f"missing source: {src}" + (
                    "  (run keenetic/fetch-bins.py)" if "/bins/" in src.replace("\\", "/") else ""))
            data = open(src, "rb").read()
            if fix:
                data = fix_shebang(data)
            add_bytes(tar, "./" + dest, data, mode)
            total += len(data)
    return buf.getvalue(), total


def build_control(version, installed_size):
    control = (
        f"Package: {PKG}\nVersion: {version}\nDepends: {DEPENDS}\n"
        f"Source: https://github.com/varyen/detour\nLicense: MIT\nSection: net\n"
        f"Priority: optional\nMaintainer: detour\nArchitecture: {ARCH}\n"
        f"Installed-Size: {installed_size}\n"
        f"Description: Detour panel + sing-box + zapret-tpws for Keenetic (KeeneticOS+Entware, mipsel). EXPERIMENTAL.\n"
    )
    postinst = f"""#!/bin/sh
set +e
mkdir -p /opt/etc/detour/subscriptions /opt/etc/sing-box/profiles /opt/etc/zapret-tpws \\
         /opt/etc/detour/dnsmasq.d /opt/var/log /opt/var/run /opt/var/state \\
         /tmp/detour-sessions /tmp/hosts
echo "{version}" > /opt/etc/detour/version
touch /opt/etc/detour/platform            # the panel CGI's platform shim keys off this
chmod 0755 /opt/sbin/tpws-zapret /opt/sbin/detour-hosts /opt/sbin/detour-update /opt/sbin/vpn-keepalive \\
    /opt/etc/init.d/S50detour-dns /opt/etc/init.d/S51detour-panel \\
    /opt/etc/init.d/S52detour-singbox /opt/etc/init.d/S53detour-zapret \\
    /opt/etc/ndm/netfilter.d/50-detour.sh /opt/share/www/cgi-bin/detour-api 2>/dev/null
# Seed self-update config (public repo; add a GH_TOKEN here to enable update checks).
if [ ! -f /opt/etc/detour/update.conf ]; then
    printf 'GH_OWNER=varyen\\nGH_REPO=detour\\nGH_TOKEN=\\nAUTO_CHECK=0\\n' > /opt/etc/detour/update.conf
    chmod 600 /opt/etc/detour/update.conf
fi
# Keep-alive cron (parity with OpenWrt). Entware crond reads /opt/etc/crontabs/root —
# the dir often doesn't exist yet on KeeneticOS, so create it + add the */5 entry.
mkdir -p /opt/etc/crontabs
if ! grep -qs 'vpn-keepalive' /opt/etc/crontabs/root 2>/dev/null; then
    echo '*/5 * * * * /opt/sbin/vpn-keepalive >/opt/var/log/vpn-keepalive.log 2>&1' >> /opt/etc/crontabs/root
fi
# Make sure Entware crond is running so the entry actually fires (no duplicate).
if ! pgrep crond >/dev/null 2>&1; then
    if [ -x /opt/etc/init.d/S10cron ]; then /opt/etc/init.d/S10cron start 2>/dev/null
    else crond -b -c /opt/etc/crontabs 2>/dev/null; fi
fi
# sing-box comes from the Entware `sing-box-go` package, which ships its own
# auto-start /opt/etc/init.d/S99sing-box (with a default config). Disable it so
# ONLY detour's S52detour-singbox drives the daemon — otherwise two sing-box
# instances fight over the same /opt/etc/sing-box/config.json and ports.
if [ -f /opt/etc/init.d/S99sing-box ]; then
    /opt/etc/init.d/S99sing-box stop 2>/dev/null
    chmod 0644 /opt/etc/init.d/S99sing-box   # drop +x → Entware rc.unslung skips it on boot
fi
# Seed a default panel login if absent. ⚠ CHANGE IT in the panel after first login.
if [ ! -f /opt/etc/detour.auth ]; then
    H=$(printf '%s' 'detour' | openssl passwd -6 -stdin 2>/dev/null)
    [ -n "$H" ] && {{ printf 'admin:%s\\n' "$H" > /opt/etc/detour.auth; chmod 600 /opt/etc/detour.auth; }}
fi
# Start now (Entware rc.unslung auto-starts /opt/etc/init.d/S* on boot).
/opt/etc/init.d/S51detour-panel start 2>/dev/null
/opt/etc/init.d/S52detour-singbox start 2>/dev/null
/opt/etc/init.d/S53detour-zapret start 2>/dev/null
echo ""
echo "detour-keenetic {version} installed."
echo "  Panel:  http://<router-ip>:8080/detour/"
echo "  Login:  admin / detour   (CHANGE the password in the panel!)"
echo "  Note: domain-based routing needs DNS+ipset wiring (not in this build) —"
echo "        for now add explicit IPs/CIDRs, or test with an added VPN profile."
exit 0
"""
    prerm = """#!/bin/sh
set +e
/opt/etc/init.d/S53detour-zapret stop 2>/dev/null
/opt/etc/init.d/S52detour-singbox stop 2>/dev/null
/opt/etc/init.d/S51detour-panel stop 2>/dev/null
exit 0
"""
    postrm = """#!/bin/sh
set +e
case "$1" in remove|purge) rm -f /opt/etc/detour/platform /opt/etc/detour/version ;; esac
exit 0
"""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.USTAR_FORMAT) as tar:
        add_bytes(tar, "./control", control.encode(), 0o644)
        add_bytes(tar, "./postinst", postinst.encode(), 0o755)
        add_bytes(tar, "./prerm", prerm.encode(), 0o755)
        add_bytes(tar, "./postrm", postrm.encode(), 0o755)
    return buf.getvalue()


def build(version=None, out_dir=None):
    """Build (and usign-sign) the Keenetic .ipk. Returns (ipk_path, sig_path,
    installed_size). Importable so `build_release.py` can drop the Keenetic
    package into the same release dir — one release, one source (router_files/)."""
    if version is None:
        version = open(os.path.join(ROOT, "VERSION")).read().strip()
    if out_dir is None:
        out_dir = OUT_DIR
    os.makedirs(out_dir, exist_ok=True)
    data_tgz, installed = build_data()
    control_tgz = build_control(version, installed)
    ipk_path = os.path.join(out_dir, f"{PKG}_{version}_{ARCH}.ipk")
    with tarfile.open(ipk_path, "w:gz", format=tarfile.USTAR_FORMAT) as tar:
        add_bytes(tar, "./debian-binary", b"2.0\n", 0o644)
        add_bytes(tar, "./control.tar.gz", control_tgz, 0o644)
        add_bytes(tar, "./data.tar.gz", data_tgz, 0o644)
    # usign-sign with the same key/scheme as the panel build.
    sig_path = None
    key_sec = os.path.join(ROOT, "keys", "release.usign.sec")
    if os.path.isfile(key_sec):
        sys.path.insert(0, ROOT)
        from usign_compat import sign_file
        sign_file(ipk_path, key_sec, ipk_path + ".sig")
        sig_path = ipk_path + ".sig"
    return ipk_path, sig_path, installed


def main():
    version = open(os.path.join(ROOT, "VERSION")).read().strip()
    ipk_path, sig_path, installed = build(version)
    print(f"built {ipk_path}")
    print(f"  on-disk {os.path.getsize(ipk_path):,} B | installed {installed:,} B | arch {ARCH} | v{version}")
    print("  " + (f"signed: {os.path.basename(sig_path)}" if sig_path
                  else "(UNSIGNED — usign secret key not found)"))
    print("  install: opkg update && opkg install ./" + os.path.basename(ipk_path))


if __name__ == "__main__":
    main()
