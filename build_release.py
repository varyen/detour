#!/usr/bin/env python3
"""Build a signed detour release as an OpenWrt .ipk package.

Usage:
    python3 build_release.py --version 1.0.11 [--notes '...'] [--from-git]
    python3 build_release.py --version 1.0.11 --publish
    python3 build_release.py --version 1.0.11 --publish-existing

Output:
    releases/v<version>/detour_<version>_all.ipk            (+ .ipk.sig)
    releases/v<version>/detour-keenetic_<version>_all.ipk   (+ .ipk.sig)
    releases/v<version>/RELEASE_NOTES.md

sing-box is NOT bundled here — the panel `Depends: sing-box`, served by our own
opkg feed (see build_feed.py). The distro feed is stuck on sing-box 1.8.10, which
would break the panel's 1.13.x config schema.

`.ipk` is the standard OpenWrt package format: a gzipped tar containing
`debian-binary`, `control.tar.gz`, and `data.tar.gz`. The `.ipk.sig` file is
a usign(1)-compatible detached signature; the router verifies it before
calling `opkg install`.

Trust chain:
    1. usign -V -p <pinned pubkey> -x <ipk>.sig -m <ipk>
       → validates the package payload against keys/release.usign.pub
         (pinned at /etc/opkg/keys/<keyid> and /etc/detour/release.usign.pub)
    2. opkg install /tmp/<package>.ipk
       → applies the package, runs postinst (which preserves
         /etc/detour.auth + user state if already present).
"""
import argparse
import hashlib
import io
import json
import os
import subprocess
import sys
import tarfile
from datetime import datetime, timezone

# Local module — produces usign-format signatures compatible with the router's
# `usign -V`. Cross-validated end-to-end on the home BE9300.
from usign_compat import sign_file

HERE = os.path.dirname(os.path.abspath(__file__))
KEY_PUB_USIGN = os.path.join(HERE, "keys", "release.usign.pub")
KEY_SEC_USIGN = os.path.join(HERE, "keys", "release.usign.sec")
ROUTER_FILES = os.path.join(HERE, "router_files")
BACKUP_HOME = os.path.join(HERE, "router-backup")
RELEASES_DIR = os.path.join(HERE, "releases")

PACKAGE_NAME = "detour"
# Packaged as `Architecture: all` on purpose. The fleet is mixed but uniformly
# aarch64 (home BE9300 = ipq53xx reports `aarch64_cortex-a53_neon-vfpv4`; router2
# GL-MT6000 = MT7986 reports a DIFFERENT string and used to reject an
# arch-pinned package with "incompatible with the architectures configured").
# The bundled binaries are portable across aarch64 (sing-box = static Go,
# tpws-zapret = static musl), so a single `all` package installs on every router
# via LuCI/opkg AND lets the self-updater (which picks the lone `.ipk` asset,
# not by arch) work everywhere. Override per-arch with DETOUR_ARCH if a
# non-aarch64 device ever joins.
PACKAGE_ARCH = os.environ.get("DETOUR_ARCH", "all")

# Runtime dependencies declared in `control`. opkg refuses to install if any
# is missing. `dnsmasq-full` is needed for ipset= entries. `sing-box` AND
# `tpws-zapret` are both pulled from our self-hosted feed (build_feed.py →
# varyen/detour@feed): the GL.iNet distro feed is stuck on sing-box 1.8.10
# (pre-1.11 schema break) and zapret's tpws is in no opkg feed at all, so the
# feed MUST be configured before installing this package (deploy_router.py /
# detour-update do that). The init.d tolerates a missing binary, so a router
# without the feed still installs and routes directly — it just can't proxy /
# DPI-bypass until the binaries arrive.
DEPENDS = "lua, lua-cjson, curl, openssl-util, dnsmasq-full, kmod-ipt-ipset, ipset, sing-box, tpws-zapret"

MAINTAINER = "Maintainer <you@example.com>"
DESCRIPTION = "Sing-box + zapret-tpws management panel for OpenWrt routers."

# Self-hosted opkg feed serving sing-box (build_feed.py → varyen/detour@feed).
# Keep in sync with build_feed.py and deploy_router.py / detour-update.
FEED_NAME = "detour"
FEED_URL = "https://raw.githubusercontent.com/varyen/detour/feed/aarch64"
FEED_LINE = f"src/gz {FEED_NAME} {FEED_URL}"

# Paths that hold USER state on the router — VPN profiles, panel auth,
# subscription config, generated configs. The package never ships anything
# that lands at one of these paths; postinst is responsible for seeding any
# defaults that aren't already present (e.g. /etc/detour.auth on first
# install). Enforced at build time below.
PROTECTED_PATHS = (
    "etc/sing-box/profiles/",
    "etc/sing-box/profiles",
    "etc/sing-box/settings.json",
    "etc/sing-box/config.json",
    "etc/sing-box/proxy-domains.list",
    "etc/sing-box/whitelist-domains.list",
    "etc/zapret-tpws/domains.list",
    "etc/zapret-tpws.conf",
    "etc/detour.auth",            # panel login (postinst seeds default if absent)
    "etc/detour/update.conf",     # GH token (deployed by deploy_router.py)
    "etc/detour/version",         # set by postinst on install
    "etc/detour/subscription.json",
    "etc/sing-box/route-map.list",     # per-profile routing rules (user state)
)


def _is_protected(path):
    p = path.lstrip("/")
    for prot in PROTECTED_PATHS:
        if p == prot or p.startswith(prot.rstrip("/") + "/"):
            return True
    return False


# (source_parts, archive_relative_path, mode) — archive_relative_path is what
# ends up at the corresponding location on the router after `opkg install`.
#
# The release ships ONE OpenWrt package: `detour` (PANEL_FILES). NEITHER binary
# is bundled — the panel `Depends: sing-box, tpws-zapret`, both pulled from our
# self-hosted opkg feed (build_feed.py). Panel updates stay tiny AND a panel
# upgrade never touches the opkg-owned /usr/bin/{sing-box,tpws-zapret}. The
# Keenetic/Entware package (keenetic/build-ipk.py) still bundles tpws because
# there is no feed there.
PANEL_FILES = [
    (("router_files", "sing-box.initd"), "etc/init.d/sing-box", 0o755),
    (("router_files", "zapret-tpws.initd"), "etc/init.d/zapret-tpws", 0o755),
    (("router_files", "firewall.lan_mark_fallback"), "etc/firewall.lan_mark_fallback", 0o755),
    (
        ("router-backup", "etc", "hotplug.d", "iface", "99-proxy-guard"),
        "etc/hotplug.d/iface/99-proxy-guard",
        0o755,
    ),
    (("router-backup", "etc", "sysctl.d", "99-mptcp.conf"), "etc/sysctl.d/99-mptcp.conf", 0o644),
    (("router_files", "base64-shim.sh"), "usr/bin/base64", 0o755),
    (("router_files", "detour-update"), "usr/sbin/detour-update", 0o755),
    (("router_files", "subscription-refresh"), "usr/sbin/subscription-refresh", 0o755),
    (("router_files", "vpn-keepalive"), "usr/sbin/vpn-keepalive", 0o755),
    (("router_files", "detour-ping"), "usr/sbin/detour-ping", 0o755),
    (("router_files", "detour-health"), "usr/sbin/detour-health", 0o755),
    (("router_files", "detour-hosts"), "usr/sbin/detour-hosts", 0o755),
    (("router_files", "detour-hosts.initd"), "etc/init.d/detour-hosts", 0o755),
    # DPI-bypass engine switch (off|zapret|zapret2) + its boot applier.
    (("router_files", "detour-bypass"), "usr/sbin/detour-bypass", 0o755),
    (("router_files", "detour-bypass.initd"), "etc/init.d/detour-bypass", 0o755),
    (("router_files", "detour-api"), "www/cgi-bin/detour-api", 0o755),
    (("router_files", "index.html"), "www/detour/index.html", 0o644),
    # NOTE: tpws-zapret is NOT bundled here anymore — it comes from the opkg feed
    # (Depends: tpws-zapret), same as sing-box. The Keenetic package still bundles it.
    # Pin our public key in two places: opkg's standard keyring (so future opkg
    # ecosystem tooling sees it) AND a stable path the updater knows about.
    (("keys", "release.usign.pub"), "etc/detour/release.usign.pub", 0o644),
]

# The protected-path sanity check scans the package's full file set.
FILES_IN_PACKAGE = PANEL_FILES

# The opkg-keyring path uses the key fingerprint as the filename. Read it
# from the actual key at build time so the two never drift.
def _opkg_keyring_path():
    from usign_compat import load_public_key
    keynum, _ = load_public_key(KEY_PUB_USIGN)
    return f"etc/opkg/keys/{keynum.hex()}"


def die(msg):
    print(f"ERROR: {msg}", file=sys.stderr)
    sys.exit(1)


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def resolve_source(parts):
    p = os.path.join(HERE, *parts)
    if not os.path.isfile(p):
        die(f"missing source file: {p}")
    return p


def parse_version(v):
    if v.startswith("v"):
        v = v[1:]
    parts = v.split(".")
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        die(f"version must be X.Y.Z, got {v!r}")
    return v


# ============ ipk assembly ============

def _add_bytes_to_tar(tar, archive_path, content, mode):
    info = tarfile.TarInfo(name=archive_path)
    info.size = len(content)
    info.mode = mode
    info.uid = info.gid = 0
    info.uname = info.gname = "root"
    info.mtime = int(datetime.now(timezone.utc).timestamp())
    tar.addfile(info, io.BytesIO(content))


def _add_file_to_tar(tar, src, archive_path, mode):
    info = tar.gettarinfo(src, arcname=archive_path)
    info.mode = mode
    info.uid = info.gid = 0
    info.uname = info.gname = "root"
    with open(src, "rb") as f:
        tar.addfile(info, f)


def build_control_tar_gz(version, installed_size):
    """Return bytes of the control.tar.gz inner archive."""
    control = (
        f"Package: {PACKAGE_NAME}\n"
        f"Version: {version}\n"
        f"Depends: {DEPENDS}\n"
        f"Source: https://github.com/varyen/detour\n"
        f"License: MIT\n"
        f"Section: net\n"
        f"Priority: optional\n"
        f"Maintainer: {MAINTAINER}\n"
        f"Architecture: {PACKAGE_ARCH}\n"
        f"Installed-Size: {installed_size}\n"
        f"Description: {DESCRIPTION}\n"
    )

    # postinst runs after files are placed. It seeds first-install state without
    # ever overwriting state that already exists, so upgrades preserve the user's
    # panel auth, GH token, subscription config, etc.
    postinst = f"""#!/bin/sh
# detour postinst — runs after opkg unpacks the package.
set +e

VERSION="{version}"

# 1) Seed /etc/detour defaults on first install (preserve on upgrade).
mkdir -p /etc/detour
echo "$VERSION" > /etc/detour/version

# Per-group subscription metadata directory. Each subscription is stored as
# /etc/detour/subscriptions/<id>.json (v2 schema). The Lua refresh helper
# falls back to /etc/detour/subscription.json (single, legacy) when this
# directory has no .json files yet, so upgrades from older installs are safe.
mkdir -p /etc/detour/subscriptions
chmod 0700 /etc/detour/subscriptions

# 1a) On a brand-new install the panel auth file is absent. The web UI
# detects that via the panel_setup_status action and shows a one-time
# "set username + password" form instead of the login screen.
# We intentionally do NOT seed credentials here — the operator picks them.

# 1b) Register our state files with sysupgrade so they survive a flash of
# the router firmware. /lib/upgrade/keep.d/ is wiped by the new firmware,
# so we manage a marker-delimited block inside /etc/sysupgrade.conf (which
# is itself preserved by default, being under /etc).
SYSUP=/etc/sysupgrade.conf
BEGIN='# === detour keeplist (managed: do not edit between markers) ==='
END='# === end detour keeplist ==='
mkdir -p /etc
touch "$SYSUP"
# Strip any previous block and re-add a fresh one. POSIX-portable awk.
awk -v B="$BEGIN" -v E="$END" '
    $0 == B {{ skip = 1; next }}
    $0 == E {{ skip = 0; next }}
    !skip {{ print }}
' "$SYSUP" > "$SYSUP.new"
cat >> "$SYSUP.new" <<KEEP
$BEGIN
/etc/detour.auth
/etc/detour/
/etc/sing-box/profiles/
/etc/sing-box/settings.json
/etc/sing-box/proxy-domains.list
/etc/sing-box/whitelist-domains.list
/etc/sing-box/health-urls.list
/etc/sing-box/route-map.list
/etc/zapret-tpws.conf
/etc/zapret-tpws/domains.list
$END
KEEP
mv "$SYSUP.new" "$SYSUP"

# 1c) uci-defaults restore script. Runs at first boot after a sysupgrade
# (OpenWrt processes /etc/uci-defaults/* once on boot then deletes them).
# If sysupgrade preserved /etc/detour/installed.ipk but the package
# database doesn't list us (new firmware == fresh /usr/lib/opkg/status),
# re-install the stashed .ipk in place.
mkdir -p /etc/uci-defaults
cat > /etc/uci-defaults/99-detour-restore <<'UCID'
#!/bin/sh
# Auto-re-install detour after a router firmware sysupgrade. A fresh firmware
# wipes /etc/opkg/customfeeds.conf and the opkg status db, so we re-add the
# sing-box feed, refresh the index, then install the stashed panel .ipk - its
# Depends:sing-box then pulls sing-box from the feed automatically.
STASH=/etc/detour/installed.ipk
[ -f "$STASH" ] || exit 0
if opkg list-installed detour 2>/dev/null | grep -q '^detour '; then
    exit 0   # already installed (regular boot, not a sysupgrade restore)
fi
logger -t detour "post-sysupgrade restore: re-add feed + opkg install panel"
LOG=/var/log/detour-restore.log
grep -qs '^src/gz {FEED_NAME} ' /etc/opkg/customfeeds.conf 2>/dev/null \\
    || echo "{FEED_LINE}" >> /etc/opkg/customfeeds.conf
opkg update >"$LOG" 2>&1
opkg install "$STASH" >>"$LOG" 2>&1
UCID
chmod 0755 /etc/uci-defaults/99-detour-restore

# 1d) update.conf placeholder (deploy_router.py overwrites with real token).
if [ ! -f /etc/detour/update.conf ]; then
    cat > /etc/detour/update.conf <<'CONF'
# Auto-generated placeholder. Public repo — self-update works as-is.
# deploy_router.py overwrites this with a GH_TOKEN for private repos / publishing.
# Owner/repo MUST stay set: an empty GH_OWNER=/GH_REPO= would override the
# detour-update defaults and break self-update.
GH_OWNER=varyen
GH_REPO=detour
GH_TOKEN=
AUTO_CHECK=0
CONF
    chmod 0600 /etc/detour/update.conf
fi

# 2) Make sure shell scripts are executable (opkg honours data.tar.gz mode bits,
# this is defence in depth).
chmod 0755 /etc/init.d/sing-box /etc/init.d/zapret-tpws \\
    /etc/firewall.lan_mark_fallback /etc/hotplug.d/iface/99-proxy-guard \\
    /usr/sbin/detour-update /usr/sbin/subscription-refresh \\
    /usr/sbin/vpn-keepalive /usr/sbin/detour-ping /usr/sbin/detour-health \\
    /usr/sbin/detour-hosts /etc/init.d/detour-hosts \\
    /usr/sbin/detour-bypass /etc/init.d/detour-bypass \\
    /www/cgi-bin/detour-api 2>/dev/null

# 2b) Seed the health-check target list on first install (preserved on upgrade
# via the keeplist + this guard). detour-health/health_urls fall back to built-in
# defaults if it's missing, but seeding makes the panel's target editor non-empty.
if [ ! -f /etc/sing-box/health-urls.list ]; then
    mkdir -p /etc/sing-box
    cat > /etc/sing-box/health-urls.list <<'HURLS'
# Цели проверки работоспособности VPN: "Название|https://адрес" на строку
# ('#'/пустые игнорируются; название необязательно). Профиль «рабочий»,
# только если открылись ВСЕ цели. Список редактируется в панели.
YouTube|https://www.youtube.com/generate_204
YouTube видео|https://redirector.googlevideo.com/generate_204
Google|https://www.google.com/generate_204
HURLS
fi

# 3) Enable + (re)start services, but HONOUR the operator's «Автозапуск» choice
# so a panel REINSTALL never resurrects a service the user turned off. The panel
# writes /etc/detour/autostart.{{singbox,zapret}} (1|0) on toggle; /etc/detour is
# preserved across upgrades, so the flag survives. Absent flag = first install =
# default ON (legacy behaviour). Errors are non-fatal: a fresh OpenWrt might lack
# uci defaults for sing-box; the operator can manually start.
detour_apply_autostart() {{   # $1 init.d path, $2 autostart flag file
    if [ "$(cat "$2" 2>/dev/null)" = "0" ]; then
        "$1" disable >/dev/null 2>&1
        "$1" stop >/dev/null 2>&1
    else
        "$1" enable >/dev/null 2>&1
        "$1" restart >/dev/null 2>&1
    fi
}}
detour_apply_autostart /etc/init.d/sing-box /etc/detour/autostart.singbox
# The DPI-bypass switch (detour-bypass) OWNS the zapret/zapret2 engine lifecycle.
# Only fall back to the legacy standalone zapret-tpws autostart when the switch was
# never used (no bypass.mode persisted) — otherwise it would double-start tpws.
if [ ! -f /etc/detour/bypass.mode ]; then
    detour_apply_autostart /etc/init.d/zapret-tpws /etc/detour/autostart.zapret
fi
# Register the bypass boot applier (always enabled) and re-apply the persisted
# mode now (no-op unless autostart=1) so an upgrade restores the active engine.
/etc/init.d/detour-bypass enable >/dev/null 2>&1
[ -x /usr/sbin/detour-bypass ] && /usr/sbin/detour-bypass boot >/dev/null 2>&1
# detour-hosts: boot hook that re-applies the dnsmasq addn-hosts snippet (tmpfs
# is wiped on reboot). `start` is a no-op when the feature is disabled and only
# touches dnsmasq when its config (preserved in /etc/detour) was left enabled.
/etc/init.d/detour-hosts enable >/dev/null 2>&1
/etc/init.d/detour-hosts start >/dev/null 2>&1

# 4) Install/refresh cron entries for self-update + subscription-refresh.
# subscription-refresh ticks hourly; the script itself decides which subscriptions
# are due based on their per-subscription `interval_hours` (default 24h) + the
# `autoupdate` flag, so running every hour is cheap when nothing is configured.
#
# The 6h auto-check runs `check-all` (panel + sing-box + tpws + nfqws2) so the
# panel can blink any stale version chip. It is ON by default; opt out by setting
# AUTO_CHECK=0 in update.conf. The toggle survives upgrades — prerm strips the
# cron line and we re-add it here unless explicitly disabled.
AUTO_CHECK=$(sed -n 's/^AUTO_CHECK=//p' /etc/detour/update.conf 2>/dev/null | tail -1)
( crontab -l 2>/dev/null | grep -v 'detour-update' | grep -v 'subscription-refresh' | grep -v 'vpn-keepalive' | grep -v 'detour-ping' | grep -v 'detour-health' | grep -v 'detour-hosts'
  [ "$AUTO_CHECK" = "0" ] || echo "0 */6 * * * /usr/sbin/detour-update check-all >/var/log/detour-update.log 2>&1"
  echo "17 * * * * /usr/sbin/subscription-refresh >/var/log/subscription-refresh.log 2>&1"
  echo "*/5 * * * * /usr/sbin/vpn-keepalive >/dev/null 2>&1"
  echo "* * * * * /usr/sbin/detour-ping >/dev/null 2>&1"
  echo "41 * * * * /usr/sbin/detour-health sweep >/var/log/detour-health.log 2>&1"
  echo "23 */12 * * * /usr/sbin/detour-hosts refresh-cron >/var/log/detour-hosts.log 2>&1"
) | crontab -
/etc/init.d/cron enable >/dev/null 2>&1
/etc/init.d/cron restart >/dev/null 2>&1

# 5) Drop our usign public key into opkg's standard keyring directory so the
# OpenWrt opkg ecosystem also trusts it (the file is shipped by data.tar.gz,
# we just make sure the dir mode is correct).
mkdir -p /etc/opkg/keys
chmod 0755 /etc/opkg/keys

echo "detour $VERSION installed."
exit 0
"""

    prerm = """#!/bin/sh
# detour prerm — runs before files are removed.
set +e

# Stop services so opkg can replace the binaries cleanly.
/etc/init.d/sing-box stop >/dev/null 2>&1
# Stop the bypass engine (nfqws2/tpws + its firewall) WITHOUT changing the
# persisted mode — postinst re-applies it. Falls back to a direct tpws stop.
[ -x /usr/sbin/detour-bypass ] && /usr/sbin/detour-bypass stop >/dev/null 2>&1
/etc/init.d/zapret-tpws stop >/dev/null 2>&1
# Drop the dnsmasq addn-hosts snippet (on a full removal it must not linger; on
# upgrade the new postinst's `start` re-applies it from the preserved config).
/etc/init.d/detour-hosts stop >/dev/null 2>&1

# Strip our cron entries (the postinst of the new version will re-add them
# during upgrade; on a full removal they're correctly gone).
crontab -l 2>/dev/null | grep -v 'detour-update' \\
                      | grep -v 'subscription-refresh' \\
                      | grep -v 'vpn-keepalive' \\
                      | grep -v 'detour-ping' \\
                      | grep -v 'detour-health' \\
                      | grep -v 'detour-hosts' \\
                      | crontab - 2>/dev/null
exit 0
"""

    # postrm tidies up trailing state files when the package is fully removed.
    postrm = """#!/bin/sh
# detour postrm — invoked after removal (when $1 == "remove") or upgrade.
set +e
case "$1" in
    remove|abort-install|disappear)
        # User state stays: /etc/detour.auth, /etc/sing-box/profiles/,
        # /etc/detour/subscription.json — operator can wipe manually.
        rm -f /etc/detour/version
        ;;
esac
exit 0
"""

    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.USTAR_FORMAT) as tar:
        # opkg looks for these names at the root of control.tar.gz with `./` prefix.
        _add_bytes_to_tar(tar, "./control", control.encode("utf-8"), 0o644)
        _add_bytes_to_tar(tar, "./postinst", postinst.encode("utf-8"), 0o755)
        _add_bytes_to_tar(tar, "./prerm", prerm.encode("utf-8"), 0o755)
        _add_bytes_to_tar(tar, "./postrm", postrm.encode("utf-8"), 0o755)
        # conffiles is intentionally empty: we never ship files that should be
        # preserved across upgrades (user state lives in PROTECTED_PATHS which we
        # never ship; postinst seeds /etc/detour.auth on first install only).
    return buf.getvalue()


def _add_dir_to_tar(tar, archive_path, mode=0o755):
    info = tarfile.TarInfo(name=archive_path)
    info.type = tarfile.DIRTYPE
    info.mode = mode
    info.uid = info.gid = 0
    info.uname = info.gname = "root"
    info.mtime = int(datetime.now(timezone.utc).timestamp())
    tar.addfile(info)


def build_data_tar_gz(file_entries):
    """Return bytes of data.tar.gz containing the payload tree at root.

    Emits explicit directory entries for every parent dir, shallowest first, so
    opkg can create them on a clean system. Without them, extracting a file into
    a not-yet-existing dir fails with `wfopen: .../file: No such file or
    directory` (e.g. /etc/detour/release.usign.pub on a pristine install)."""
    dirs = set()
    for _, dest_rel, _ in file_entries:
        parts = dest_rel.strip("/").split("/")
        for i in range(1, len(parts)):
            dirs.add("/".join(parts[:i]))
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.USTAR_FORMAT) as tar:
        for d in sorted(dirs):
            _add_dir_to_tar(tar, "./" + d + "/")
        for src_parts, dest_rel, mode in file_entries:
            src = resolve_source(src_parts)
            _add_file_to_tar(tar, src, "./" + dest_rel, mode)
    return buf.getvalue()


def build_ipk(pkg_name, version, file_entries, out_dir, *, inject_keyring):
    """Build the panel .ipk. Returns (ipk_path, installed_size_bytes)."""
    # 0. Sanity: no protected path (user state) may sneak into the payload.
    bad = [d for _, d, _ in file_entries if _is_protected(d)]
    if bad:
        die(f"{pkg_name}: package contains protected paths (user state):\n  "
            + "\n  ".join(bad))

    # 1. Optionally inject the opkg keyring file (path depends on key
    #    fingerprint) — only the panel ships the verification key.
    file_entries = list(file_entries)
    if inject_keyring:
        file_entries.append((("keys", "release.usign.pub"), _opkg_keyring_path(), 0o644))

    # 2. Compute installed size for control metadata.
    installed_size = sum(os.path.getsize(resolve_source(p)) for p, _, _ in file_entries)

    # 3. Assemble control.tar.gz and data.tar.gz.
    control_tgz = build_control_tar_gz(version, installed_size)
    data_tgz = build_data_tar_gz(file_entries)
    debian_binary = b"2.0\n"

    # 4. Wrap into the outer .ipk (a gzipped tar containing those three blobs).
    ipk_name = f"{pkg_name}_{version}_{PACKAGE_ARCH}.ipk"
    ipk_path = os.path.join(out_dir, ipk_name)
    with tarfile.open(ipk_path, "w:gz", format=tarfile.USTAR_FORMAT) as tar:
        _add_bytes_to_tar(tar, "./debian-binary", debian_binary, 0o644)
        _add_bytes_to_tar(tar, "./control.tar.gz", control_tgz, 0o644)
        _add_bytes_to_tar(tar, "./data.tar.gz", data_tgz, 0o644)

    return ipk_path, installed_size


def sign_ipk(ipk_path):
    """Produce <ipk>.sig as a usign(1)-compatible detached signature."""
    if not os.path.isfile(KEY_SEC_USIGN):
        die(f"usign secret key missing at {KEY_SEC_USIGN} — generate it with "
            f"`usign -G -s {KEY_SEC_USIGN} -p {KEY_PUB_USIGN}`")
    sig_path = ipk_path + ".sig"
    sign_file(ipk_path, KEY_SEC_USIGN, sig_path)
    return sig_path


# ============ git/notes ============

def read_git_log_for_notes(version):
    try:
        # encoding=utf-8: git emits UTF-8; without this, text=True decodes with the
        # Windows ANSI codepage (cp1251) and Cyrillic / em-dash commit subjects
        # turn into mojibake in the release notes.
        prev = subprocess.run(
            ["git", "describe", "--tags", "--abbrev=0", "--match", "v*"],
            capture_output=True, text=True, encoding="utf-8", errors="replace", cwd=HERE,
        )
        prev_tag = prev.stdout.strip() if prev.returncode == 0 else None
        rng = f"{prev_tag}..HEAD" if prev_tag else "HEAD"
        log = subprocess.run(
            ["git", "log", "--no-merges", "--pretty=format:- %s", rng],
            capture_output=True, text=True, encoding="utf-8", errors="replace", cwd=HERE,
        )
        return log.stdout.strip()
    except Exception:
        return ""


# ============ GitHub publish ============

def _load_github_config():
    candidates = [
        os.environ.get("ROUTERS_CONFIG") or "",
        os.path.join(HERE, "routers.local.json"),
        os.path.join(HERE, "routers.json"),
    ]
    cfg_path = next((p for p in candidates if p and os.path.isfile(p)), None)
    if not cfg_path:
        die("routers.local.json not found — needed for --publish")
    with open(cfg_path, "r", encoding="utf-8") as f:
        cfg = json.load(f)
    gh = cfg.get("github") or {}
    owner = gh.get("owner") or ""
    repo = gh.get("repo") or ""
    token = gh.get("publish_token") or gh.get("token") or ""
    if not owner or not repo:
        die("routers.local.json: github.owner / .repo must be set for --publish")
    if not token:
        die("routers.local.json: set github.token or .publish_token (PAT with Contents: Read+Write)")
    return owner, repo, token


def _gh_request(method, url, token, *, data=None, content_type=None, expected_status=None):
    import urllib.request, urllib.error, http.client, time
    hdrs = {
        "Authorization": f"Bearer {token}",
        "Accept": "application/vnd.github+json",
        "User-Agent": "detour-release-publisher/3.0",
        "X-GitHub-Api-Version": "2022-11-28",
    }
    body = data
    if data is not None and not isinstance(data, (bytes, bytearray)):
        body = json.dumps(data).encode("utf-8")
        hdrs["Content-Type"] = "application/json"
    if content_type:
        hdrs["Content-Type"] = content_type
    # The GitHub API/CDN intermittently resets TLS on throttled/censored RU links
    # (WinError 10054 mid-handshake/upload). Retry transient network errors —
    # HTTPError is a real HTTP response (carries the status we want), so it is NOT
    # retried. Mirrors detour-update's gh_curl_retry resilience.
    attempts = 5
    for attempt in range(1, attempts + 1):
        req = urllib.request.Request(url, data=body, method=method, headers=hdrs)
        try:
            with urllib.request.urlopen(req, timeout=180) as resp:
                status, raw = resp.status, resp.read()
            break
        except urllib.error.HTTPError as e:
            status, raw = e.code, e.read()
            break
        except (urllib.error.URLError, ConnectionError, TimeoutError,
                http.client.HTTPException, OSError) as e:
            if attempt == attempts:
                die(f"GitHub API {method} {url} failed after {attempts} attempts: {e}")
            print(f"[publish] network error ({e}); retry {attempt}/{attempts - 1}...")
            time.sleep(3)
    try:
        parsed = json.loads(raw.decode("utf-8")) if raw else None
    except Exception:
        parsed = raw
    if expected_status is not None and status not in expected_status:
        msg = parsed if isinstance(parsed, (dict, list)) else (raw[:300] if raw else "")
        die(f"GitHub API {method} {url} → {status}: {msg}")
    return status, parsed


def publish_to_github(version, out_dir):
    owner, repo, token = _load_github_config()
    tag = f"v{version}"
    api = f"https://api.github.com/repos/{owner}/{repo}"
    print(f"\n[publish] target: {owner}/{repo} (tag {tag})")

    status, release = _gh_request("GET", f"{api}/releases/tags/{tag}", token,
                                  expected_status={200, 404})
    # RELEASE_NOTES.md is UTF-8 (Russian) — MUST read it as utf-8, otherwise on a
    # Windows build host Python decodes it with the ANSI codepage (cp1251) and the
    # GitHub release body comes out as mojibake ("Р±РѕР»СЊС€Рµ").
    notes_path = os.path.join(out_dir, "RELEASE_NOTES.md")
    body = open(notes_path, encoding="utf-8").read() if os.path.isfile(notes_path) else ""
    # Guard: never ship a body carrying the Unicode replacement char (U+FFFD) — it's
    # a sure sign the notes got mangled (read with the wrong codepage, or passed via
    # --notes on a non-UTF-8 console). Abort with a clear message instead.
    if chr(0xFFFD) in body:
        die("RELEASE_NOTES.md is corrupted - contains U+FFFD (replacement char). "
            f"Re-write {notes_path} as UTF-8 (or fix the --notes source) and re-run.")
    if status == 404:
        payload = {
            "tag_name": tag, "name": tag, "body": body,
            "draft": False, "prerelease": False, "generate_release_notes": False,
        }
        _, release = _gh_request("POST", f"{api}/releases", token, data=payload,
                                 expected_status={201})
        print(f"[publish] created release {tag}")
    else:
        print(f"[publish] reusing existing release {tag} (id={release['id']})")
        # Refresh the body so re-publishing fixes/updates the description.
        if body:
            _gh_request("PATCH", f"{api}/releases/{release['id']}", token,
                        data={"body": body}, expected_status={200})
            print(f"[publish] updated release notes for {tag}")

    # Post-publish verify: re-read the stored body from GitHub and confirm the notes
    # round-tripped intact. U+FFFD -> hard fail (encoding corruption); a benign
    # reformat -> warning only. Catches any transport/encoding surprise before we
    # treat the release as done.
    if body:
        _, _check = _gh_request("GET", f"{api}/releases/tags/{tag}", token,
                                expected_status={200})
        _stored = _check.get("body") or ""
        if chr(0xFFFD) in _stored:
            die("published release body contains U+FFFD - encoding corruption on "
                f"publish: {_check.get('html_url', tag)}")
        elif _stored.replace("\r\n", "\n").strip() != body.replace("\r\n", "\n").strip():
            print("[publish] WARNING: stored release body differs from RELEASE_NOTES.md "
                  "(GitHub may have normalised it - verify the release page)")
        else:
            print("[publish] release notes verified intact (UTF-8 round-trip OK)")

    upload_url = release["upload_url"].split("{", 1)[0]
    existing = {a["name"]: a["id"] for a in (release.get("assets") or [])}

    # Upload every .ipk (+ .sig) in out_dir: the panel and the Keenetic package.
    # sing-box ships via the opkg feed (build_feed.py), not as a release asset.
    assets_to_upload = sorted(
        os.path.join(out_dir, n) for n in os.listdir(out_dir)
        if n.endswith(".ipk") or n.endswith(".ipk.sig")
    )
    if not assets_to_upload:
        die(f"no .ipk artefacts in {out_dir} — build first")
    upload_names = {os.path.basename(p) for p in assets_to_upload}
    # Wipe legacy artefacts left over from earlier release schemes (tarball+manifest
    # and the detour-bins / detour-full split).
    legacy_names = {
        f"detour-v{version}.tar.gz", "manifest.json", "manifest.json.sig",
        f"detour-full-v{version}.tar.gz",
    }
    for name, aid in existing.items():
        if name in legacy_names or name in upload_names:
            print(f"[publish] removing existing asset {name}")
            _gh_request("DELETE", f"{api}/releases/assets/{aid}", token,
                        expected_status={204})

    for path in assets_to_upload:
        name = os.path.basename(path)
        size = os.path.getsize(path)
        ctype = "application/x-debian-package" if name.endswith(".ipk") else "application/octet-stream"
        print(f"[publish] uploading {name} ({size} bytes)...")
        with open(path, "rb") as f:
            data = f.read()
        _, asset = _gh_request("POST", upload_url + f"?name={name}", token,
                               data=data, content_type=ctype,
                               expected_status={201})
        print(f"[publish]   -> {asset.get('browser_download_url') or asset.get('url')}")

    print(f"\n[publish] release URL: {release['html_url']}")
    return release["html_url"]


# ============ main ============

def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--version", required=True, help="Panel release version, e.g. 1.3.0")
    ap.add_argument("--notes", default=None, help="Release notes (plain text)")
    ap.add_argument("--from-git", action="store_true",
                    help="Derive notes from git log since previous tag")
    ap.add_argument("--publish", action="store_true",
                    help="After building, upload all artefacts to GH release v<version>")
    ap.add_argument("--publish-existing", action="store_true",
                    help="Skip build — only upload existing releases/v<version>/ artefacts")
    ap.add_argument("--allow-existing", action="store_true",
                    help="Don't fail if releases/v<version>/ already exists — overwrite it")
    ap.add_argument("--no-keenetic", action="store_true",
                    help="Skip the Keenetic/Entware .ipk (built into the same release by default)")
    args = ap.parse_args()

    version = parse_version(args.version)
    out_dir = os.path.join(RELEASES_DIR, f"v{version}")

    if args.publish_existing:
        panel_ipk = os.path.join(out_dir, f"{PACKAGE_NAME}_{version}_{PACKAGE_ARCH}.ipk")
        if not os.path.isfile(panel_ipk):
            die(f"missing {panel_ipk} — build first")
        if not os.path.isfile(panel_ipk + ".sig"):
            die(f"missing {panel_ipk}.sig — re-build or run sign step")
        print(f"=== Publishing existing v{version} from {out_dir} ===")
        publish_to_github(version, out_dir)
        print("\n=== DONE ===")
        return

    if os.path.exists(out_dir):
        if args.allow_existing:
            import shutil as _sh
            _sh.rmtree(out_dir)
        else:
            die(f"release dir already exists: {out_dir} — pass --allow-existing or rm -rf")
    os.makedirs(out_dir, exist_ok=False)

    notes = args.notes
    if not notes and args.from_git:
        notes = read_git_log_for_notes(version)

    built = []  # (label, ipk_path, sig_path)
    print(f"=== Building detour v{version} ===")
    print(f"Output: {out_dir}")

    # --- panel (slim; sing-box comes from the opkg feed, tpws is bundled) ---
    print("\n[panel] Assembling .ipk ...")
    panel_ipk, panel_size = build_ipk(PACKAGE_NAME, version, PANEL_FILES, out_dir,
                                      inject_keyring=True)
    print(f"  {panel_ipk}  ({os.path.getsize(panel_ipk):,} B on disk, "
          f"installed {panel_size:,} B, sha256 {sha256_file(panel_ipk)[:16]}...)")
    panel_sig = sign_ipk(panel_ipk)
    print(f"  signed: {panel_sig}")
    built.append(("panel", panel_ipk, panel_sig))

    # --- Keenetic/Entware .ipk (same release, same source: router_files/) ---
    keenetic_ipk = None
    if not args.no_keenetic:
        print("\n[keenetic] Assembling Entware .ipk ...")
        import importlib.util
        _ks = importlib.util.spec_from_file_location(
            "keenetic_build_ipk", os.path.join(HERE, "keenetic", "build-ipk.py"))
        _km = importlib.util.module_from_spec(_ks)
        _ks.loader.exec_module(_km)
        keenetic_ipk, keenetic_sig, keenetic_size = _km.build(version, out_dir)
        print(f"  {keenetic_ipk}  ({os.path.getsize(keenetic_ipk):,} B on disk, "
              f"installed {keenetic_size:,} B)")
        print(f"  signed: {keenetic_sig}" if keenetic_sig else "  (UNSIGNED)")
        built.append(("keenetic", keenetic_ipk, keenetic_sig))

    # --- release notes ---
    print("\n[notes] Writing release notes ...")
    notes_path = os.path.join(out_dir, "RELEASE_NOTES.md")
    with open(notes_path, "w", encoding="utf-8") as f:
        f.write(f"# detour v{version}\n\n")
        f.write(notes or "(no notes)")
        f.write("\n\n## Packages\n\n")
        f.write(f"- `{os.path.basename(panel_ipk)}` — panel for OpenWrt/GL.iNet "
                "(scripts/UI). sing-box AND tpws-zapret are pulled from the opkg feed.\n")
        if keenetic_ipk:
            f.write(f"- `{os.path.basename(keenetic_ipk)}` — Keenetic/Entware (mipsel) package "
                    "(tpws bundled; no feed there).\n")
        f.write("\n## Binaries (sing-box + tpws-zapret)\n\n")
        f.write(
            "Neither binary is bundled — the panel `Depends: sing-box, tpws-zapret`, "
            f"both served by our feed (`{FEED_LINE}`). The feed must be configured "
            "before install (`deploy_router.py` and `detour-update` do this "
            "automatically). Build/publish the feed with "
            "`python3 build_feed.py --version <sb-ver> --tpws-version <zapret-ver> --publish`.\n"
        )
        f.write("\n## Install\n\n")
        f.write(
            "### Fresh (SSH)\n\n"
            f"```\nopkg update && opkg install sing-box tpws-zapret   # from the detour feed\n"
            f"opkg install /tmp/{os.path.basename(panel_ipk)}\n```\n\n"
            "### Panel update (existing install)\n\n"
            f"LuCI → Software → Upload `{os.path.basename(panel_ipk)}`, or the panel's "
            "self-update, or `detour-update apply`.\n\n"
            "### Binary updates\n\n"
            "Panel → version chip → «Обновление». sing-box: `detour-update bins-apply` "
            "(`opkg upgrade sing-box`); tpws-zapret: `detour-update tpws-apply` "
            "(`opkg upgrade tpws-zapret`).\n"
        )
    print(f"  {notes_path}")

    if args.publish:
        publish_to_github(version, out_dir)

    print("\n=== DONE ===")
    for label, ipk, _sig in built:
        print(f"  [{label}] {os.path.basename(ipk)}")
    if not args.publish:
        print(f"\nUpload to GH release v{version}: --publish (or --publish-existing later)")


if __name__ == "__main__":
    main()
