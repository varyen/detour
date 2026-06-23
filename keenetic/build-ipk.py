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
    (os.path.join(ROUTER_FILES, "sw.js"), "opt/share/www/detour/sw.js", 0o644, False),
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
    # Unified DPI-bypass engine switch (off|zapret|zapret2). The panel's single DPI
    # toggle calls this via the CGI bypass_* endpoints; absent here, enabling zapret
    # returned "detour-bypass not installed". zapret2 (NFQUEUE) is OpenWrt-only — on
    # Keenetic the shared source drives only off/zapret. fix_shebang → /opt/bin/sh.
    (os.path.join(ROUTER_FILES, "detour-bypass"), "opt/sbin/detour-bypass", 0o755, True),
    # VPN endpoint health probe (shared source, /opt shim). Driven by the
    # S90detour-cron loop below (KeeneticOS kills crond's job shell — see below).
    (os.path.join(ROUTER_FILES, "vpn-keepalive"), "opt/sbin/vpn-keepalive", 0o755, True),
    # Per-profile latency sweep (shared source, /opt shim). Also driven by the
    # S90detour-cron loop — replaces the old browser-driven ping probing.
    (os.path.join(ROUTER_FILES, "detour-ping"), "opt/sbin/detour-ping", 0o755, True),
    # Functional health check (shared source, /opt shim). Driven hourly by the
    # S90detour-cron loop. Self-no-ops if Entware sing-box lacks clash_api.
    (os.path.join(ROUTER_FILES, "detour-health"), "opt/sbin/detour-health", 0o755, True),
    # Web Push (VAPID) sender (shared source, /opt shim). Backs the panel's push
    # settings + detour-health down/auto-switch alerts. fix_shebang → /opt/bin/sh.
    (os.path.join(ROUTER_FILES, "detour-push"), "opt/sbin/detour-push", 0o755, True),
    # Let's Encrypt helper (acme.sh, /opt shim). Best-effort on Keenetic: :80 is the
    # stock router UI, so HTTP-01 needs :80 forwarded to lighttpd. fix_shebang → /opt/bin/sh.
    (os.path.join(ROUTER_FILES, "detour-cert"), "opt/sbin/detour-cert", 0o755, True),
    # Read-only preflight for the cert flow (diagnose env + print next steps). Keenetic-only.
    (os.path.join(HERE, "detour-cert-preflight.sh"), "opt/sbin/detour-cert-preflight", 0o755, True),
    # Standalone scheduler daemon (Keenetic-only): replaces the broken crond for
    # detour's periodic jobs — keep-alive, subscription-refresh, update auto-check.
    (os.path.join(HERE, "sbin", "detour-cron"), "opt/sbin/detour-cron", 0o755, False),
    # Syslog log-bridge (shared source, /opt shim): tails Detour's log files →
    # `logger` so KeeneticOS remote-log forwarding picks them up. Gated by the
    # log_to_syslog setting (off by default). fix_shebang → /opt/bin/sh.
    (os.path.join(ROUTER_FILES, "detour-logbridge"), "opt/sbin/detour-logbridge", 0o755, True),
    # Pinned usign public key (used by detour-update if usign is present on Entware).
    (os.path.join(ROOT, "keys", "release.usign.pub"), "opt/etc/detour/release.usign.pub", 0o644, False),
    (os.path.join(HERE, "init.d", "S51detour-panel"), "opt/etc/init.d/S51detour-panel", 0o755, False),
    # Domain→ipset DNS (Entware dnsmasq + transparent :53 redirect) — domain routing.
    (os.path.join(HERE, "init.d", "S50detour-dns"), "opt/etc/init.d/S50detour-dns", 0o755, False),
    (os.path.join(HERE, "init.d", "S52detour-singbox"), "opt/etc/init.d/S52detour-singbox", 0o755, False),
    (os.path.join(HERE, "init.d", "S53detour-zapret"), "opt/etc/init.d/S53detour-zapret", 0o755, False),
    # Boot applier for the persisted bypass mode (runs `detour-bypass boot`); S54 so
    # it runs after the sing-box (S52) and zapret (S53) init scripts exist.
    (os.path.join(HERE, "init.d", "S54detour-bypass"), "opt/etc/init.d/S54detour-bypass", 0o755, False),
    # Scheduler daemon launcher (rc.unslung boot-start). S90 = last, after the
    # panel/proxies are up. Drives /opt/sbin/detour-cron — the crond replacement.
    (os.path.join(HERE, "init.d", "S90detour-cron"), "opt/etc/init.d/S90detour-cron", 0o755, False),
    # Syslog log-bridge launcher (rc.unslung boot-start). S91 = after the proxies/
    # cron so the log files it tails exist. Self-gates on the log_to_syslog setting.
    (os.path.join(HERE, "init.d", "S91detour-logbridge"), "opt/etc/init.d/S91detour-logbridge", 0o755, False),
    (os.path.join(HERE, "ndm", "netfilter.d", "50-detour.sh"), "opt/etc/ndm/netfilter.d/50-detour.sh", 0o755, False),
    (os.path.join(HERE, "lighttpd", "detour.conf"), "opt/etc/lighttpd/detour.conf", 0o644, False),
    # TLS-overlay shim for detour.conf's include_shell. MUST stay 0755 and keep its
    # #!/opt/bin/sh shebang (fix=False — fix_shebang would turn it into /opt/opt/bin/sh).
    # See lighttpd/detour-ssl-helper.sh for why the inline `cat … 2>/dev/null` crashed lighttpd.
    (os.path.join(HERE, "lighttpd", "detour-ssl-helper.sh"), "opt/etc/lighttpd/conf.d/detour-ssl-helper.sh", 0o755, False),
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
    /opt/sbin/detour-ping /opt/sbin/detour-health /opt/sbin/detour-bypass /opt/sbin/detour-cron \\
    /opt/etc/init.d/S50detour-dns /opt/etc/init.d/S51detour-panel \\
    /opt/etc/init.d/S52detour-singbox /opt/etc/init.d/S53detour-zapret /opt/etc/init.d/S54detour-bypass \\
    /opt/etc/init.d/S90detour-cron /opt/sbin/detour-logbridge /opt/etc/init.d/S91detour-logbridge \\
    /opt/etc/lighttpd/conf.d/detour-ssl-helper.sh \\
    /opt/etc/ndm/netfilter.d/50-detour.sh /opt/share/www/cgi-bin/detour-api 2>/dev/null
# Seed the health-check target list on first install (preserved on upgrade).
if [ ! -f /opt/etc/sing-box/health-urls.list ]; then
    printf '# Цели проверки: "Название|https://адрес" на строку. Профиль рабочий, только если открылись ВСЕ.\\nYouTube|https://www.youtube.com/generate_204\\nYouTube видео|https://redirector.googlevideo.com/generate_204\\nGoogle|https://www.google.com/generate_204\\n' > /opt/etc/sing-box/health-urls.list
fi
# Seed self-update config (public repo; add a GH_TOKEN here to enable update checks).
# AUTO_CHECK=1 → S90detour-cron runs the 6h `check-all` by default (opt out with =0).
if [ ! -f /opt/etc/detour/update.conf ]; then
    printf 'GH_OWNER=varyen\\nGH_REPO=detour\\nGH_TOKEN=\\nAUTO_CHECK=1\\n' > /opt/etc/detour/update.conf
    chmod 600 /opt/etc/detour/update.conf
fi
# Scheduled tasks. On KeeneticOS the firmware sandbox kills the shell crond spawns
# to run a job, so cron silently never fires here — instead we run the schedule
# (vpn-keepalive / subscription-refresh / detour-update check-all) from an
# init.d-launched daemon (S90detour-cron), the same session model the panel and
# proxy daemons use, which DO work on KeeneticOS. Strip any detour crontab lines a
# pre-1.8.4 install left in /opt/etc/crontabs/root so the work can't double-run on
# devices where crond happens to work.
if [ -f /opt/etc/crontabs/root ]; then
    grep -v -e 'vpn-keepalive' -e 'subscription-refresh' -e 'detour-update check' \\
        /opt/etc/crontabs/root > /opt/etc/crontabs/root.detour.tmp 2>/dev/null \\
        && mv /opt/etc/crontabs/root.detour.tmp /opt/etc/crontabs/root \\
        || rm -f /opt/etc/crontabs/root.detour.tmp
fi
# (Re)start the scheduler daemon now; rc.unslung also boot-starts it via S90.
/opt/etc/init.d/S90detour-cron restart 2>/dev/null
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
# Start now (Entware rc.unslung auto-starts /opt/etc/init.d/S* on boot). On a panel
# SELF-UPDATE the old lighttpd is still serving (prerm kept it up to stream this install's
# log) — skip the early start here and restart it at the very END of this postinst (below),
# so apply_log stays reachable for as long as possible. Fresh/manual install (no marker) →
# start it now, before the proxies, so the UI is up to control them.
[ -f /tmp/detour-panel-selfupdate ] || /opt/etc/init.d/S51detour-panel start 2>/dev/null
/opt/etc/init.d/S52detour-singbox start 2>/dev/null
# The DPI-bypass switch (detour-bypass) OWNS the zapret engine lifecycle. Only fall
# back to the standalone zapret autostart when the switch was never used (no
# bypass.mode persisted) — otherwise it would double-start tpws against the switch.
if [ ! -f /opt/etc/detour/bypass.mode ]; then
    /opt/etc/init.d/S53detour-zapret start 2>/dev/null
fi
# Re-apply the persisted bypass mode now (no-op unless bypass.autostart=1) so an
# upgrade restores the active engine.
[ -x /opt/sbin/detour-bypass ] && /opt/sbin/detour-bypass boot 2>/dev/null
# Syslog log-bridge: (re)start it. It self-gates on the log_to_syslog setting, so
# this is a no-op until the operator enables it in the panel; rc.unslung also
# boot-starts it via S91. On upgrade restart re-loads the new script.
/opt/etc/init.d/S91detour-logbridge restart 2>/dev/null
echo ""
echo "detour-keenetic {version} installed."
echo "  Panel:  http://<router-ip>:8080/detour/"
echo "  Login:  admin / detour   (CHANGE the password in the panel!)"
echo "  Note: domain-based routing needs DNS+ipset wiring (not in this build) —"
echo "        for now add explicit IPs/CIDRs, or test with an added VPN profile."
# Panel SELF-UPDATE: everything is in place — restart the kept-alive lighttpd LAST so it
# loads any new config/CGI, then drop the marker. This is the only moment the live log
# blips; the detached worker writes the apply_log completion sentinel right after opkg
# returns, so the browser reconnects, shows the final log and reloads.
if [ -f /tmp/detour-panel-selfupdate ]; then
    rm -f /tmp/detour-panel-selfupdate
    /opt/etc/init.d/S51detour-panel restart 2>/dev/null
fi
exit 0
"""
    prerm = """#!/bin/sh
set +e
# Stop the scheduler daemon first so a periodic task can't fire mid-upgrade.
[ -x /opt/etc/init.d/S90detour-cron ] && /opt/etc/init.d/S90detour-cron stop 2>/dev/null
# Stop the syslog log-bridge so its tail|logger followers don't linger across the swap.
[ -x /opt/etc/init.d/S91detour-logbridge ] && /opt/etc/init.d/S91detour-logbridge stop 2>/dev/null
# Stop the bypass-managed engine (tpws + its rules) WITHOUT changing the persisted
# mode — the new postinst's `detour-bypass boot` re-applies it. Falls back to S53.
[ -x /opt/sbin/detour-bypass ] && /opt/sbin/detour-bypass stop 2>/dev/null
/opt/etc/init.d/S53detour-zapret stop 2>/dev/null
/opt/etc/init.d/S52detour-singbox stop 2>/dev/null
# Panel web server: on a panel-driven SELF-UPDATE (marker set by the panel CGI) KEEP it
# running so the browser can stream this install's log live (apply_log) — the new postinst
# restarts it at the very end. The CGI/HTML are re-read per request, so serving from the
# old process during the swap is safe; only a changed lighttpd config needs the restart.
# On a real removal / manual op (no marker) stop it normally so we don't orphan it.
if [ ! -f /tmp/detour-panel-selfupdate ]; then
    /opt/etc/init.d/S51detour-panel stop 2>/dev/null
fi
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
