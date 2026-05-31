#!/usr/bin/env python3
"""Build a signed detour release as an OpenWrt .ipk package.

Usage:
    python3 build_release.py --version 1.0.11 [--notes '...'] [--from-git]
    python3 build_release.py --version 1.0.11 --publish
    python3 build_release.py --version 1.0.11 --publish-existing

Output:
    releases/v<version>/detour_<version>_aarch64_cortex-a53.ipk
    releases/v<version>/detour_<version>_aarch64_cortex-a53.ipk.sig
    releases/v<version>/RELEASE_NOTES.md

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
# is missing. `dnsmasq-full` is needed for ipset= entries.
DEPENDS = "lua, lua-cjson, curl, openssl-util, dnsmasq-full, kmod-ipt-ipset, ipset"

MAINTAINER = "Maintainer <you@example.com>"
DESCRIPTION = "Sing-box + zapret-tpws management panel for OpenWrt routers."

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
# The release is split into TWO packages:
#   PANEL_FILES → detour        (scripts/html/lua/init.d, ~0.35 MB)
#   BIN_FILES   → detour-bins   (sing-box + tpws-zapret, ~60 MB)
# Binaries live in their own package so panel updates stay tiny AND a slim-panel
# upgrade never deletes /usr/bin/sing-box (opkg removes files the old package
# owned but the new one doesn't). The two are versioned independently; the
# updater manages them separately (`apply` vs `bins-apply`). A combined
# `detour-full-v<ver>.tar.gz` (both .ipk + install.sh) covers offline/first
# install in one shot.
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
    (("router_files", "detour-api"), "www/cgi-bin/detour-api", 0o755),
    (("router_files", "index.html"), "www/detour/index.html", 0o644),
    # Pin our public key in two places: opkg's standard keyring (so future opkg
    # ecosystem tooling sees it) AND a stable path the updater knows about.
    (("keys", "release.usign.pub"), "etc/detour/release.usign.pub", 0o644),
]

# Heavy binaries — shipped as the separate `detour-bins` package.
BIN_FILES = [
    (("router-backup", "usr", "bin", "sing-box"), "usr/bin/sing-box", 0o755),
    (("router-backup", "usr", "bin", "tpws-zapret"), "usr/bin/tpws-zapret", 0o755),
]

# Back-compat alias: the protected-path sanity check scans the full set.
FILES_IN_PACKAGE = PANEL_FILES + BIN_FILES

BINS_PKG_NAME = "detour-bins"
# detour-bins has no runtime deps of its own (it only drops binaries).
# Built as arch `all` for the same reason the panel is — the fleet is uniformly
# aarch64 but reports differing opkg arch strings, and the static Go/musl
# binaries are portable across them. For a non-aarch64 device, set
# DETOUR_ARCH and publish one bins asset per arch (the updater picks by the
# arch tag in the asset filename).
BINS_DEPENDS = ""

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
# Auto-re-install detour (panel + binaries) after a router firmware
# sysupgrade. Binaries live in the separate detour-bins package, stashed
# alongside the panel .ipk — install bins first so the panel's services start.
STASH=/etc/detour/installed.ipk
BSTASH=/etc/detour/installed-bins.ipk
[ -f "$STASH" ] || exit 0
if opkg list-installed detour 2>/dev/null | grep -q '^detour '; then
    exit 0   # already installed (regular boot, not a sysupgrade restore)
fi
logger -t detour "post-sysupgrade restore: opkg install bins + panel"
[ -f "$BSTASH" ] && opkg install "$BSTASH" >/var/log/detour-restore.log 2>&1
opkg install "$STASH" >>/var/log/detour-restore.log 2>&1
UCID
chmod 0755 /etc/uci-defaults/99-detour-restore

# 1d) update.conf placeholder (deploy_router.py overwrites with real token).
if [ ! -f /etc/detour/update.conf ]; then
    cat > /etc/detour/update.conf <<'CONF'
# Auto-generated placeholder. Populate via deploy_router.py for live updates.
GH_OWNER=
GH_REPO=
GH_TOKEN=
CONF
    chmod 0600 /etc/detour/update.conf
fi

# 2) Make sure shell scripts are executable (opkg honours data.tar.gz mode bits,
# this is defence in depth).
chmod 0755 /etc/init.d/sing-box /etc/init.d/zapret-tpws \\
    /etc/firewall.lan_mark_fallback /etc/hotplug.d/iface/99-proxy-guard \\
    /usr/sbin/detour-update /usr/sbin/subscription-refresh \\
    /usr/sbin/vpn-keepalive \\
    /www/cgi-bin/detour-api 2>/dev/null

# 3) Enable + restart services. Errors here are non-fatal: a fresh OpenWrt
# might lack uci defaults for sing-box; the operator can manually start.
/etc/init.d/sing-box enable >/dev/null 2>&1
/etc/init.d/zapret-tpws enable >/dev/null 2>&1
/etc/init.d/sing-box restart >/dev/null 2>&1
/etc/init.d/zapret-tpws restart >/dev/null 2>&1

# 4) Install/refresh cron entries for self-update + subscription-refresh.
# subscription-refresh ticks hourly; the script itself decides which subscriptions
# are due based on their per-subscription `interval_hours` (default 24h) + the
# `autoupdate` flag, so running every hour is cheap when nothing is configured.
( crontab -l 2>/dev/null | grep -v 'detour-update' | grep -v 'subscription-refresh' | grep -v 'vpn-keepalive'
  echo "0 */6 * * * /usr/sbin/detour-update check >/var/log/detour-update.log 2>&1"
  echo "17 * * * * /usr/sbin/subscription-refresh >/var/log/subscription-refresh.log 2>&1"
  echo "*/5 * * * * /usr/sbin/vpn-keepalive >/dev/null 2>&1"
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
/etc/init.d/zapret-tpws stop >/dev/null 2>&1

# Strip our cron entries (the postinst of the new version will re-add them
# during upgrade; on a full removal they're correctly gone).
crontab -l 2>/dev/null | grep -v 'detour-update' \\
                      | grep -v 'subscription-refresh' \\
                      | grep -v 'vpn-keepalive' \\
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


def build_bins_control_tar_gz(bins_version, installed_size):
    """control.tar.gz for the detour-bins package (sing-box + tpws only).

    Deliberately minimal: it owns ONLY the two binaries. It records the bins
    version (the panel reads it), chmods + restarts the services so a new binary
    takes effect, and on removal nothing of the panel's state is touched."""
    control = (
        f"Package: {BINS_PKG_NAME}\n"
        f"Version: {bins_version}\n"
        + (f"Depends: {BINS_DEPENDS}\n" if BINS_DEPENDS else "")
        + f"Source: https://github.com/varyen/detour\n"
        f"License: MIT\n"
        f"Section: net\n"
        f"Priority: optional\n"
        f"Maintainer: {MAINTAINER}\n"
        f"Architecture: {PACKAGE_ARCH}\n"
        f"Installed-Size: {installed_size}\n"
        f"Description: sing-box + tpws-zapret binaries for detour.\n"
    )

    postinst = f"""#!/bin/sh
# detour-bins postinst — installs the sing-box + tpws-zapret binaries.
set +e
mkdir -p /etc/detour
echo "{bins_version}" > /etc/detour/bins-version
chmod 0755 /usr/bin/sing-box /usr/bin/tpws-zapret 2>/dev/null
# Record a manifest the panel reads (cheap) instead of spawning the 60 MB
# sing-box binary on every status poll just to learn its version.
SBVER=$(/usr/bin/sing-box version 2>/dev/null | sed -n 's/.*version[[:space:]]*\\([0-9][0-9.]*\\).*/\\1/p' | head -1)
[ -z "$SBVER" ] && SBVER="{bins_version}"
cat > /etc/detour/bins-manifest.json <<MAN
{{"bins_version":"{bins_version}","singbox":"$SBVER","tpws":"{bins_version}"}}
MAN
# Restart so the freshly-installed binaries take effect. Non-fatal: on a fresh
# router sing-box may not be configured yet (the panel handles that).
/etc/init.d/sing-box restart >/dev/null 2>&1
/etc/init.d/zapret-tpws restart >/dev/null 2>&1
echo "detour-bins {bins_version} installed."
exit 0
"""

    prerm = """#!/bin/sh
# detour-bins prerm — stop the services so the busy binaries can be
# replaced (opkg can't overwrite a running executable cleanly otherwise).
set +e
/etc/init.d/sing-box stop >/dev/null 2>&1
/etc/init.d/zapret-tpws stop >/dev/null 2>&1
exit 0
"""

    postrm = """#!/bin/sh
# detour-bins postrm — clears the bins markers on full removal.
set +e
case "$1" in
    remove|abort-install|disappear)
        rm -f /etc/detour/bins-version /etc/detour/bins-manifest.json
        ;;
esac
exit 0
"""

    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.USTAR_FORMAT) as tar:
        _add_bytes_to_tar(tar, "./control", control.encode("utf-8"), 0o644)
        _add_bytes_to_tar(tar, "./postinst", postinst.encode("utf-8"), 0o755)
        _add_bytes_to_tar(tar, "./prerm", prerm.encode("utf-8"), 0o755)
        _add_bytes_to_tar(tar, "./postrm", postrm.encode("utf-8"), 0o755)
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


def build_ipk(pkg_name, version, file_entries, out_dir, *, kind, inject_keyring):
    """Build one .ipk. `kind` ∈ {"panel","bins"} selects the maintainer scripts.
    Returns (ipk_path, installed_size_bytes)."""
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
    if kind == "bins":
        control_tgz = build_bins_control_tar_gz(version, installed_size)
    else:
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


# ============ full-bundle (offline one-shot installer) ============

def _full_bundle_install_sh(panel_ipk, panel_sig, bins_ipk, bins_sig):
    """install.sh shipped inside detour-full-v<ver>.tar.gz. Verifies both
    .ipk against the pinned/TOFU usign key, then installs bins THEN panel."""
    return f"""#!/bin/sh
# detour full offline installer. Run from the extracted bundle dir:
#   tar -xzf detour-full-v<ver>.tar.gz && sh detour-full-v<ver>/install.sh
# Installs the binaries package first, then the panel. Pass --skip-verify only
# if you really must bypass the usign signature check.
set -e
cd "$(dirname "$0")"

PANEL_IPK="{panel_ipk}"
PANEL_SIG="{panel_sig}"
BINS_IPK="{bins_ipk}"
BINS_SIG="{bins_sig}"
PUBKEY=etc/detour/release.usign.pub   # TOFU copy shipped in the bundle

verify() {{
    ipk="$1"; sig="$2"
    [ "${{1:-}}" = "--skip-verify" ] && return 0
    case " $* " in *" --skip-verify "*) return 0 ;; esac
    if command -v usign >/dev/null 2>&1; then
        if [ -d /etc/opkg/keys ] && ls /etc/opkg/keys/* >/dev/null 2>&1; then
            usign -V -m "$ipk" -P /etc/opkg/keys -x "$sig" && return 0
        fi
        if [ -f /etc/detour/release.usign.pub ]; then
            usign -V -m "$ipk" -p /etc/detour/release.usign.pub -x "$sig" && return 0
        fi
        usign -V -m "$ipk" -p "$PUBKEY" -x "$sig" && return 0   # TOFU bootstrap
        echo "usign verification FAILED for $ipk" >&2; exit 1
    fi
    echo "usign not found — re-run with --skip-verify to bypass" >&2; exit 1
}}

verify "$BINS_IPK" "$BINS_SIG" "$@"
verify "$PANEL_IPK" "$PANEL_SIG" "$@"

echo "Installing binaries package ..."
opkg install --force-overwrite "$BINS_IPK"
echo "Installing panel package ..."
opkg install --force-overwrite "$PANEL_IPK"
echo "Done. detour installed (panel + binaries)."
"""


def build_full_bundle(version, bins_version, panel_ipk, panel_sig, bins_ipk, bins_sig, out_dir):
    """Bundle both .ipk + .sig + a TOFU pubkey + install.sh into one tarball that
    installs the whole thing offline in a single step. Returns the tarball path."""
    base = f"detour-full-v{version}"
    tgz_path = os.path.join(out_dir, base + ".tar.gz")
    names = {
        "panel_ipk": os.path.basename(panel_ipk), "panel_sig": os.path.basename(panel_sig),
        "bins_ipk": os.path.basename(bins_ipk), "bins_sig": os.path.basename(bins_sig),
    }
    install_sh = _full_bundle_install_sh(
        names["panel_ipk"], names["panel_sig"], names["bins_ipk"], names["bins_sig"])
    with tarfile.open(tgz_path, "w:gz", format=tarfile.USTAR_FORMAT) as tar:
        _add_dir_to_tar(tar, f"./{base}/")
        _add_dir_to_tar(tar, f"./{base}/etc/")
        _add_dir_to_tar(tar, f"./{base}/etc/detour/")
        for p in (panel_ipk, panel_sig, bins_ipk, bins_sig):
            _add_file_to_tar(tar, p, f"./{base}/{os.path.basename(p)}", 0o644)
        # TOFU copy of the pubkey so install.sh can verify on a key-less router.
        _add_file_to_tar(tar, resolve_source(("keys", "release.usign.pub")),
                         f"./{base}/etc/detour/release.usign.pub", 0o644)
        _add_bytes_to_tar(tar, f"./{base}/install.sh", install_sh.encode("utf-8"), 0o755)
    return tgz_path


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
        prev = subprocess.run(
            ["git", "describe", "--tags", "--abbrev=0", "--match", "v*"],
            capture_output=True, text=True, cwd=HERE,
        )
        prev_tag = prev.stdout.strip() if prev.returncode == 0 else None
        rng = f"{prev_tag}..HEAD" if prev_tag else "HEAD"
        log = subprocess.run(
            ["git", "log", "--no-merges", "--pretty=format:- %s", rng],
            capture_output=True, text=True, cwd=HERE,
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
    import urllib.request, urllib.error
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
    req = urllib.request.Request(url, data=body, method=method, headers=hdrs)
    try:
        with urllib.request.urlopen(req, timeout=180) as resp:
            status, raw = resp.status, resp.read()
    except urllib.error.HTTPError as e:
        status, raw = e.code, e.read()
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
    if status == 404:
        notes_path = os.path.join(out_dir, "RELEASE_NOTES.md")
        body = open(notes_path).read() if os.path.isfile(notes_path) else ""
        payload = {
            "tag_name": tag, "name": tag, "body": body,
            "draft": False, "prerelease": False, "generate_release_notes": False,
        }
        _, release = _gh_request("POST", f"{api}/releases", token, data=payload,
                                 expected_status={201})
        print(f"[publish] created release {tag}")
    else:
        print(f"[publish] reusing existing release {tag} (id={release['id']})")

    upload_url = release["upload_url"].split("{", 1)[0]
    existing = {a["name"]: a["id"] for a in (release.get("assets") or [])}

    # Upload every release artefact present in out_dir: panel .ipk+.sig,
    # bins .ipk+.sig (when built) and the full-bundle tarball.
    assets_to_upload = sorted(
        os.path.join(out_dir, n) for n in os.listdir(out_dir)
        if n.endswith(".ipk") or n.endswith(".ipk.sig")
        or (n.startswith("detour-full-v") and n.endswith(".tar.gz"))
    )
    if not assets_to_upload:
        die(f"no .ipk artefacts in {out_dir} — build first")
    upload_names = {os.path.basename(p) for p in assets_to_upload}
    # Wipe any legacy schema-v2 artefacts left over from earlier releases.
    legacy_names = {
        f"detour-v{version}.tar.gz", "manifest.json", "manifest.json.sig",
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
    ap.add_argument("--bins-version", default=None,
                    help="Also build detour-bins (+full bundle) at this version, "
                         "e.g. the sing-box version 1.13.2. Omit for a panel-only release.")
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
    bins_version = parse_version(args.bins_version) if args.bins_version else None
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
    print(f"=== Building detour v{version}"
          + (f" + bins v{bins_version}" if bins_version else " (panel only)") + " ===")
    print(f"Output: {out_dir}")

    # --- panel (slim) ---
    print("\n[panel] Assembling .ipk ...")
    panel_ipk, panel_size = build_ipk(PACKAGE_NAME, version, PANEL_FILES, out_dir,
                                      kind="panel", inject_keyring=True)
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

    bins_ipk = bins_sig = None
    if bins_version:
        # --- binaries ---
        print("\n[bins] Assembling .ipk ...")
        bins_ipk, bins_size = build_ipk(BINS_PKG_NAME, bins_version, BIN_FILES, out_dir,
                                        kind="bins", inject_keyring=False)
        print(f"  {bins_ipk}  ({os.path.getsize(bins_ipk):,} B on disk, "
              f"installed {bins_size:,} B, sha256 {sha256_file(bins_ipk)[:16]}...)")
        bins_sig = sign_ipk(bins_ipk)
        print(f"  signed: {bins_sig}")
        built.append(("bins", bins_ipk, bins_sig))

        # --- full offline bundle ---
        print("\n[full] Building offline one-shot bundle ...")
        full_tgz = build_full_bundle(version, bins_version, panel_ipk, panel_sig,
                                     bins_ipk, bins_sig, out_dir)
        print(f"  {full_tgz}  ({os.path.getsize(full_tgz):,} B)")

    # --- release notes ---
    print("\n[notes] Writing release notes ...")
    notes_path = os.path.join(out_dir, "RELEASE_NOTES.md")
    with open(notes_path, "w", encoding="utf-8") as f:
        f.write(f"# detour v{version}"
                + (f" (bins v{bins_version})" if bins_version else "") + "\n\n")
        f.write(notes or "(no notes)")
        f.write("\n\n## Packages\n\n")
        f.write(f"- `{os.path.basename(panel_ipk)}` — panel for OpenWrt/GL.iNet (scripts/UI).\n")
        if keenetic_ipk:
            f.write(f"- `{os.path.basename(keenetic_ipk)}` — Keenetic/Entware (mipsel) package.\n")
        if bins_version:
            f.write(f"- `{os.path.basename(bins_ipk)}` — sing-box + tpws-zapret binaries.\n")
            f.write(f"- `detour-full-v{version}.tar.gz` — both .ipk + `install.sh` "
                    "for a one-shot offline install.\n")
        f.write("\n## Install\n\n")
        if bins_version:
            f.write(
                "### Fresh / offline (one shot, SSH)\n\n"
                f"```\nscp detour-full-v{version}.tar.gz root@<router>:/tmp/\n"
                f"ssh root@<router> 'cd /tmp && tar -xzf detour-full-v{version}.tar.gz "
                f"&& sh detour-full-v{version}/install.sh'\n```\n\n"
                "### Fresh via LuCI\n\n"
                f"Upload **both** `{os.path.basename(bins_ipk)}` and "
                f"`{os.path.basename(panel_ipk)}` (bins first).\n\n"
            )
        f.write(
            "### Panel update (existing install)\n\n"
            f"LuCI → Software → Upload `{os.path.basename(panel_ipk)}`, or the panel's "
            "self-update, or `detour-update apply`.\n"
        )
        if bins_version:
            f.write("\n### Binaries update\n\n"
                    "Panel → «Обновить бинарники», or `detour-update bins-apply`.\n")
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
