#!/usr/bin/env python3
"""Build + publish the self-hosted opkg feed for the detour panel.

The GL.iNet/OpenWrt distro feed is pinned to sing-box 1.8.10, which predates the
1.11 config-schema break and would corrupt the panel's 1.13.x config. zapret's
`tpws` is in no opkg feed on any platform. So the `detour` panel declares
`Depends: sing-box, tpws-zapret` and we serve BOTH from this tiny opkg feed
hosted in the *public* `varyen/detour` repo.

Output (local):
    releases/feed/<arch>/sing-box_<ver>-<rev>_all.ipk
    releases/feed/<arch>/tpws-zapret_<ver>-<rev>_all.ipk
    releases/feed/<arch>/Packages           (one control stanza per .ipk present)
    releases/feed/<arch>/Packages.gz        (what `src/gz` fetches)
    releases/feed/<arch>/Packages.sig       (usign signature of Packages)

The `Packages` index ALWAYS covers every `.ipk` in the feed dir, so rebuilding
just one package (e.g. bump sing-box) keeps the other in the index. Building only
one package requires the other's `.ipk` to already exist in the feed dir — pass
both `--version` (sing-box) and `--tpws-version` on a clean build.

Publish (`--publish`): force-push the feed tree to a dedicated orphan branch
(`feed`) as a single squashed commit, so the binaries never accumulate in history
and `main` stays lean. Served over plain HTTPS via:
    src/gz detour https://raw.githubusercontent.com/varyen/detour/feed/<arch>

Routers get that line in /etc/opkg/customfeeds.conf (deploy_router.py /
detour-update), then `opkg install sing-box tpws-zapret` pulls our builds (our
1.13.x cleanly out-versions the distro's 1.8.10) and `opkg upgrade <pkg>` keeps
them current.

Signatures are not strictly required (opkg here has no `check_signature`), but we
sign Packages with the same usign key already pinned on every router for cheap
integrity + future-proofing.

Usage:
    python3 build_feed.py --version 1.13.2 --tpws-version 72.12            # build both
    python3 build_feed.py --version 1.13.2 --tpws-version 72.12 --publish  # + push feed
    python3 build_feed.py --version 1.13.3                                 # bump sing-box only
    python3 build_feed.py --tpws-version 72.13 --publish                   # bump tpws only
"""
import argparse
import gzip
import io
import os
import subprocess
import sys
import tarfile

# Reuse the ipk tar helpers + GH config loader from the release builder so the
# two packagers never drift on archive format / signing / auth.
from build_release import (
    HERE, KEY_SEC_USIGN, KEY_PUB_USIGN, BACKUP_HOME,
    _add_bytes_to_tar, _add_file_to_tar, _add_dir_to_tar,
    sha256_file, _load_github_config, die,
)
from usign_compat import sign_file, load_public_key

# Сборщик пакетов APKv2 — им же собирается панель в build_release.py.
import apk_pkg

ARCH = "all"  # static binaries, portable across the aarch64 opkg-arch family
              # (the fleet reports aarch64_cortex-a53_neon-vfpv4, etc.). `all`
              # so a single .ipk installs on every aarch64 router.
FEED_ARCH_DIR = "aarch64"  # logical feed sub-dir (one per binary arch family)
MIPSEL_FEED_ARCH_DIR = "mipsel"  # Keenetic/Entware (KeeneticOS, MT7621) sub-dir.
              # Same Architecture: all (path-segregated by sub-dir), but the
              # binaries are 32-bit little-endian MIPS soft-float musl-static
              # (sing-box `-mipsle-softfloat-musl`, tpws `binaries/linux-mipsel`).
FEED_BRANCH = "feed"
DEFAULT_REVISION = "1"

# Формат пакетов текущего фида: "ipk" (opkg) или "apk" (apk-tools 3). Переключает
# select_arch; сам билд и индекс смотрят сюда.
PKG_FORMAT = "ipk"

# ---- apk-фид (OpenWrt 25.12+) -------------------------------------------------
# OpenWrt 25.12 ушёл с opkg на apk-tools 3, и там НЕТ ни tpws, ни nfqws2 — ни в
# родном фиде, ни у нас (наш opkg-фид собран под другой формат). sing-box в
# родном фиде 25.12 уже 1.13.x, поэтому его мы для apk НЕ пересобираем: этот фид
# существует ровно ради движков DPI-обхода.
#
# Почему пакеты объявлены `arch = noarch`, хотя внутри лежит бинарник под
# конкретный CPU: строка архитектуры у apk на OpenWrt конкретна до модели ядра
# (`aarch64_cortex-a53`, `arm_cortex-a7_neon-vfpv4`, …), и попытка совпасть с ней
# точно ломалась бы на каждом новом устройстве. Вместо этого нужный файл
# выбираем МЫ: фид разложен по каталогам `feed/apk-<семейство>`, а роутер
# определяет своё семейство по префиксу `apk --print-arch`. Та же логика, что у
# `Architecture: all` в opkg-ветке, только выбор делает detour-update, а не opkg.
APK_FEED_PREFIX = "apk-"

# семейство → (каталог в релизе zapret/zapret2, ожидаемая ELF-сигнатура)
#   ELF: (EI_CLASS, EI_DATA, e_machine) — 1=32бит/2=64бит, 1=LE/2=BE
APK_TARGETS = {
    "x86_64":  {"zapret": "linux-x86_64", "elf": (2, 1, 0x3E)},
    "x86":     {"zapret": "linux-x86",    "elf": (1, 1, 0x03)},
    "aarch64": {"zapret": "linux-arm64",  "elf": (2, 1, 0xB7)},
    # zapret собирает linux-arm soft-float — на hard-float armv7 такой статический
    # бинарник запускается штатно, обратное неверно, поэтому берём именно его.
    "arm":     {"zapret": "linux-arm",    "elf": (1, 1, 0x28)},
    "mipsel":  {"zapret": "linux-mipsel", "elf": (1, 1, 0x08)},
    "mips":    {"zapret": "linux-mips",   "elf": (1, 2, 0x08)},
}

# Кэш скачанных бинарников: router-backup/apk/<семейство>/<имя>. router-backup
# в .gitignore, поэтому в репозиторий ничего не попадает.
def apk_bin_path(cpu, name):
    return os.path.join(BACKUP_HOME, "apk", cpu, name)

# The binaries we serve. All static and live under router-backup/usr/bin
# (refreshed from the home router by update_backups.py):
#   sing-box    — 1.13.x, musl-free static Go, portable across aarch64
#   tpws-zapret — zapret tpws, aarch64 musl-static (bol-van/zapret prebuilt)
#   nfqws2      — zapret2 nfqws2, aarch64 static (bol-van/zapret2 prebuilt) + its
#                 3 LuaJIT desync scripts. Fetched from the pinned release below
#                 (see fetch_nfqws2_assets) — optional engine for zapret2 mode.
SB_BINARY = os.path.join(BACKUP_HOME, "usr", "bin", "sing-box")
TPWS_BINARY = os.path.join(BACKUP_HOME, "usr", "bin", "tpws-zapret")
NFQWS_BINARY = os.path.join(BACKUP_HOME, "usr", "bin", "nfqws2")
NFQWS_LUA_DIR = os.path.join(BACKUP_HOME, "usr", "share", "detour", "lua")
NFQWS_LUA_FILES = ("zapret-lib.lua", "zapret-antidpi.lua", "zapret-auto.lua")

# mipsel (Keenetic/Entware) binary cache. Distinct paths so --fetch-upstream for
# the mipsel feed never clobbers the aarch64 binaries above. router-backup is
# gitignored. Populated by fetch_singbox_mipsel / fetch_tpws_mipsel.
SB_BINARY_MIPSEL = os.path.join(BACKUP_HOME, "keenetic", "opt", "bin", "sing-box")
TPWS_BINARY_MIPSEL = os.path.join(BACKUP_HOME, "keenetic", "opt", "sbin", "tpws-zapret")

# Upstream source repos for --fetch-upstream (CI auto-publish needs no
# router-backup). sing-box ships the binary in a per-libc tarball; zapret/zapret2
# ship per-arch prebuilts inside the release tarball.
SINGBOX_REPO = "SagerNet/sing-box"
ZAPRET_REPO = "bol-van/zapret"
# zapret2 upstream release used for the nfqws2 binary + lua. Bump together with
# the --nfqws2-version you pass to build_feed.
ZAPRET2_REPO = "bol-van/zapret2"
ZAPRET2_REL = "v1.0.2"
ZAPRET2_EMBEDDED = f"https://github.com/{ZAPRET2_REPO}/releases/download/{ZAPRET2_REL}/zapret2-{ZAPRET2_REL}-openwrt-embedded.tar.gz"
ZAPRET2_SOURCE = f"https://github.com/{ZAPRET2_REPO}/releases/download/{ZAPRET2_REL}/zapret2-{ZAPRET2_REL}.tar.gz"
ZAPRET2_ARM64_BIN = f"zapret2-{ZAPRET2_REL}/binaries/linux-arm64/nfqws2"

FEED_OUT = os.path.join(HERE, "releases", "feed", FEED_ARCH_DIR)

MAINTAINER = "Maintainer <you@example.com>"

# postinst/prerm are deliberately minimal: each package owns ONE binary, chmods
# it and does a best-effort service restart so `opkg upgrade <pkg>` takes effect
# when the detour panel is installed (harmless + silent if it isn't).
_SINGBOX_POSTINST = """#!/bin/sh
set +e
chmod 0755 /usr/bin/sing-box 2>/dev/null
[ -x /etc/init.d/sing-box ] && /etc/init.d/sing-box restart >/dev/null 2>&1
exit 0
"""
_SINGBOX_PRERM = """#!/bin/sh
set +e
# Hold the init's fail-closed flag while we stop the busy daemon for the binary
# swap: stop_service then leaves the REDIRECT rules up, so the gap fails CLOSED
# (LAN -> :12345 -> refused), never direct. postinst's `restart` clears the flag.
# This protects a manual `opkg upgrade sing-box` over SSH; the panel's
# `detour-update bins-apply` also holds it. A stale flag (killed upgrade) is auto-
# expired by stop_service after 2 min, so a leftover can't block a manual «Выкл».
# (Feed pkg is OpenWrt-only; the marker-based Keenetic init never sees this flag.)
: > /tmp/.singbox-keepfw
# Stop the service so the busy binary can be replaced cleanly on upgrade.
[ -x /etc/init.d/sing-box ] && /etc/init.d/sing-box stop >/dev/null 2>&1
exit 0
"""
_TPWS_POSTINST = """#!/bin/sh
set +e
chmod 0755 /usr/bin/tpws-zapret 2>/dev/null
[ -x /etc/init.d/zapret-tpws ] && /etc/init.d/zapret-tpws restart >/dev/null 2>&1
exit 0
"""
_TPWS_PRERM = """#!/bin/sh
set +e
[ -x /etc/init.d/zapret-tpws ] && /etc/init.d/zapret-tpws stop >/dev/null 2>&1
exit 0
"""
# nfqws2 owns its binary + lua. On (re)install, if the operator has zapret2 mode
# selected, re-apply it so the new binary takes effect. detour-bypass lives in the
# panel package; the calls are best-effort + silent when absent.
_NFQWS_POSTINST = """#!/bin/sh
set +e
chmod 0755 /usr/bin/nfqws2 2>/dev/null
if [ -x /usr/sbin/detour-bypass ] && [ "$(cat /etc/detour/bypass.mode 2>/dev/null)" = zapret2 ]; then
    /usr/sbin/detour-bypass set zapret2 >/dev/null 2>&1
fi
exit 0
"""
_NFQWS_PRERM = """#!/bin/sh
set +e
# Stop zapret2 before replacing the busy binary; detour-bypass re-applies in postinst.
# Use `stop` (NOT `set off`): `set off` PERSISTS bypass.mode=off, after which the
# postinst's `if bypass.mode == zapret2` check fails and zapret2 is left disabled
# after every upgrade. `stop` halts the engine without touching the persisted mode,
# so the postinst restores it.
if [ -x /usr/sbin/detour-bypass ] && [ "$(/usr/sbin/detour-bypass mode 2>/dev/null)" = zapret2 ]; then
    /usr/sbin/detour-bypass stop >/dev/null 2>&1
fi
exit 0
"""

# Package specs: a list of (src_path, dest_rel, mode) files + maintainer scripts +
# description. Versions are supplied at build time. Sources are populated locally
# (sing-box/tpws by update_backups.py; nfqws2 by fetch_nfqws2_assets).
PKG_SPECS = {
    "sing-box": {
        "files": [(SB_BINARY, "usr/bin/sing-box", 0o755)],
        "postinst": _SINGBOX_POSTINST,
        "prerm": _SINGBOX_PRERM,
        "description": ("sing-box universal proxy platform. Detour feed build for "
                        "OpenWrt/GL.iNet (the distro feed is stuck on 1.8.x)."),
    },
    "tpws-zapret": {
        "files": [(TPWS_BINARY, "usr/bin/tpws-zapret", 0o755)],
        "postinst": _TPWS_POSTINST,
        "prerm": _TPWS_PRERM,
        "description": ("zapret tpws transparent DPI-bypass proxy (bol-van/zapret). "
                        "Detour feed build — zapret is in no opkg feed."),
    },
    "nfqws2": {
        "files": [(NFQWS_BINARY, "usr/bin/nfqws2", 0o755)]
                 + [(os.path.join(NFQWS_LUA_DIR, n), "usr/share/detour/lua/" + n, 0o644)
                    for n in NFQWS_LUA_FILES],
        "postinst": _NFQWS_POSTINST,
        "prerm": _NFQWS_PRERM,
        "description": ("zapret2 nfqws2 NFQUEUE DPI-bypass engine + LuaJIT desync "
                        "scripts (bol-van/zapret2). Optional — used by zapret2 mode."),
    },
}

# ---- mipsel (Keenetic/Entware) package specs ----------------------------------
# Entware's opkg root is /opt, so the binaries live under opt/* (NOT usr/*) and the
# maintainer scripts drive the detour Keenetic init.d (S52/S53), not the OpenWrt
# procd services. No keepfw flag: opkg replaces the binary by unlink+rename, so the
# running daemon keeps its old inode and only the postinst `restart` swaps it — and
# S52detour-singbox's restart is fail-closed (it keeps the REDIRECT marker up), so
# there is no direct-leak window. nfqws2 is intentionally absent (no NFQUEUE on
# KeeneticOS → the engine can't run; zapret2 stays OpenWrt-only).
_SINGBOX_POSTINST_MIPSEL = """#!/bin/sh
set +e
chmod 0755 /opt/bin/sing-box 2>/dev/null
# Only bounce sing-box if it was RUNNING (enabled marker present). Skipping the
# restart when the operator has it manually «Выкл» means a binary upgrade can't
# silently turn the VPN back on. When it IS running, S52's `restart` keeps the
# REDIRECT marker up, so the swap stays fail-closed (gap refuses, never direct).
if [ -f /opt/etc/detour/singbox.enabled ] && [ -x /opt/etc/init.d/S52detour-singbox ]; then
    /opt/etc/init.d/S52detour-singbox restart >/dev/null 2>&1
fi
exit 0
"""
_SINGBOX_PRERM_MIPSEL = """#!/bin/sh
set +e
# Do NOT stop sing-box here: opkg swaps the binary file underneath the running
# daemon (old inode stays live), and the postinst `restart` brings up the new one
# fail-closed. Stopping would risk a teardown/leak window for no benefit.
exit 0
"""
_TPWS_POSTINST_MIPSEL = """#!/bin/sh
set +e
chmod 0755 /opt/sbin/tpws-zapret 2>/dev/null
# Only cycle tpws if it is currently running (zapret mode active) - a blind restart
# would START tpws even when the DPI switch is off. detour-bypass owns the lifecycle;
# this just lets a live engine pick up the freshly-installed binary.
if pgrep -f /opt/sbin/tpws-zapret >/dev/null 2>&1; then
    [ -x /opt/etc/init.d/S53detour-zapret ] && /opt/etc/init.d/S53detour-zapret restart >/dev/null 2>&1
fi
exit 0
"""
_TPWS_PRERM_MIPSEL = """#!/bin/sh
set +e
exit 0
"""
PKG_SPECS_MIPSEL = {
    "sing-box": {
        "files": [(SB_BINARY_MIPSEL, "opt/bin/sing-box", 0o755)],
        "postinst": _SINGBOX_POSTINST_MIPSEL,
        "prerm": _SINGBOX_PRERM_MIPSEL,
        "description": ("sing-box universal proxy platform. Detour feed build for "
                        "Keenetic/Entware (mipsel soft-float, musl-static)."),
    },
    "tpws-zapret": {
        "files": [(TPWS_BINARY_MIPSEL, "opt/sbin/tpws-zapret", 0o755)],
        "postinst": _TPWS_POSTINST_MIPSEL,
        "prerm": _TPWS_PRERM_MIPSEL,
        "description": ("zapret tpws transparent DPI-bypass proxy (bol-van/zapret). "
                        "Detour feed build for Keenetic/Entware (mipsel)."),
    },
}


# Неизменяемая ссылка на OpenWrt-специи: select_arch перепривязывает PKG_SPECS,
# а apk-специи собираются из этих же (те же пути и maintainer-скрипты, отличается
# только источник бинарника).
PKG_SPECS_OPENWRT = PKG_SPECS


def apk_pkg_specs(cpu):
    """Специи apk-пакетов для одного CPU-семейства.

    Пути на роутере и maintainer-скрипты берём один в один из OpenWrt-ветки:
    apk-прошивка — это тот же OpenWrt с procd и /etc/init.d, отличается только
    менеджер пакетов."""
    lua = [(os.path.join(NFQWS_LUA_DIR, f), "usr/share/detour/lua/" + f, 0o644)
           for f in NFQWS_LUA_FILES]
    return {
        "tpws-zapret": dict(
            PKG_SPECS_OPENWRT["tpws-zapret"],
            files=[(apk_bin_path(cpu, "tpws-zapret"), "usr/bin/tpws-zapret", 0o755)],
        ),
        "nfqws2": dict(
            PKG_SPECS_OPENWRT["nfqws2"],
            files=[(apk_bin_path(cpu, "nfqws2"), "usr/bin/nfqws2", 0o755)] + lua,
        ),
    }


def _control_text(pkg, version, installed_size, description):
    return (
        f"Package: {pkg}\n"
        f"Version: {version}\n"
        f"Source: https://github.com/varyen/detour\n"
        f"License: GPL-3.0-or-later\n"
        f"Section: net\n"
        f"Priority: optional\n"
        f"Maintainer: {MAINTAINER}\n"
        f"Architecture: {ARCH}\n"
        f"Installed-Size: {installed_size}\n"
        f"Description: {description} ({version})\n"
    )


def _build_control_tar_gz(pkg, version, installed_size, spec):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.USTAR_FORMAT) as tar:
        _add_bytes_to_tar(tar, "./control",
                          _control_text(pkg, version, installed_size, spec["description"]).encode(), 0o644)
        _add_bytes_to_tar(tar, "./postinst", spec["postinst"].encode(), 0o755)
        _add_bytes_to_tar(tar, "./prerm", spec["prerm"].encode(), 0o755)
    return buf.getvalue()


def _build_data_tar_gz(files):
    """files: list of (src_path, dest_rel, mode)."""
    dirs = set()
    for _src, dest, _mode in files:
        parts = dest.strip("/").split("/")
        for i in range(1, len(parts)):
            dirs.add("/".join(parts[:i]))
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.USTAR_FORMAT) as tar:
        for d in sorted(dirs):
            _add_dir_to_tar(tar, "./" + d + "/")
        for src, dest, mode in files:
            _add_file_to_tar(tar, src, "./" + dest.lstrip("/"), mode)
    return buf.getvalue()


def build_apk(pkg, version, out_dir):
    """Собрать пакет фида в формате APKv2. Возвращает (путь, installed_size).

    `version` приходит в opkg-виде «72.13-1»: до дефиса — версия апстрима, после
    — наша ревизия. apk хранит ревизию отдельным суффиксом «-rN», и на роутере
    мы её срезаем (панель сравнивает чистый семвер), поэтому в индекс фида уходит
    ТОЛЬКО версия апстрима — иначе установленная «72.13» вечно не совпадала бы с
    доступной «72.13-1» и панель показывала бы несуществующее обновление."""
    spec = PKG_SPECS[pkg]
    files = spec["files"]
    for src, _dest, _mode in files:
        if not os.path.isfile(src):
            die(f"{pkg}: source file not found: {src} (run with --fetch-upstream)")
    upstream, _, rev = version.partition("-")
    os.makedirs(out_dir, exist_ok=True)
    path = os.path.join(out_dir, f"{pkg}_{upstream}_{apk_pkg.NOARCH}.apk")
    return apk_pkg.build_apk(
        path,
        pkgname=pkg,
        version=upstream,
        release=int(rev or 0),
        arch=apk_pkg.NOARCH,
        description=spec["description"],
        url="https://github.com/varyen/detour",
        license_="GPL-3.0-or-later",
        maintainer=MAINTAINER,
        depends=(),
        file_entries=files,
        scripts={
            "post-install": spec["postinst"],
            "post-upgrade": spec["postinst"],
            "pre-upgrade": spec["prerm"],
            "pre-deinstall": spec["prerm"],
        },
    )


def build_ipk(pkg, version, out_dir):
    """Assemble one package's .ipk. Returns (ipk_path, installed_size)."""
    if PKG_FORMAT == "apk":
        return build_apk(pkg, version, out_dir)
    spec = PKG_SPECS[pkg]
    files = spec["files"]
    for src, _dest, _mode in files:
        if not os.path.isfile(src):
            die(f"{pkg}: source file not found: {src} "
                + ("(run fetch_nfqws2_assets / build_feed --nfqws2-version)" if pkg == "nfqws2"
                   else "(run update_backups.py first)"))
    installed_size = sum(os.path.getsize(src) for src, _d, _m in files)
    control_tgz = _build_control_tar_gz(pkg, version, installed_size, spec)
    data_tgz = _build_data_tar_gz(files)
    os.makedirs(out_dir, exist_ok=True)
    ipk_path = os.path.join(out_dir, f"{pkg}_{version}_{ARCH}.ipk")
    with tarfile.open(ipk_path, "w:gz", format=tarfile.USTAR_FORMAT) as tar:
        _add_bytes_to_tar(tar, "./debian-binary", b"2.0\n", 0o644)
        _add_bytes_to_tar(tar, "./control.tar.gz", control_tgz, 0o644)
        _add_bytes_to_tar(tar, "./data.tar.gz", data_tgz, 0o644)
    return ipk_path, installed_size


def _http_get(url):
    import urllib.request
    req = urllib.request.Request(url, headers={"User-Agent": "detour-feed"})
    return urllib.request.urlopen(req, timeout=300).read()


def _assert_static_arm64(data, label):
    """Guard against shipping a binary that won't run on the musl router. Accepts
    an aarch64 ELF with NO PT_INTERP (fully static). sing-box's plain linux-arm64
    build went DYNAMIC/glibc at 1.13.13 — only the `-musl` asset is static — so
    this check is what catches a wrong-variant download before it reaches a router."""
    import struct
    if data[:4] != b"\x7fELF":
        die(f"{label}: not an ELF binary")
    le = data[5] == 1
    e = "<" if le else ">"
    mach = struct.unpack(e + "H", data[0x12:0x14])[0]
    phoff = struct.unpack(e + "Q", data[0x20:0x28])[0]
    phentsize = struct.unpack(e + "H", data[0x36:0x38])[0]
    phnum = struct.unpack(e + "H", data[0x38:0x3a])[0]
    interp = any(
        struct.unpack(e + "I", data[phoff + i * phentsize: phoff + i * phentsize + 4])[0] == 3
        for i in range(phnum)
    )
    if mach != 0xB7:
        die(f"{label}: not aarch64 (machine=0x{mach:x})")
    if interp:
        die(f"{label}: dynamically linked (PT_INTERP present) — needs the musl/static build")
    return True


def _assert_static_mipsel(data, label):
    """Guard the Keenetic/MT7621 target: a 32-bit (ELFCLASS32) little-endian
    (ELFDATA2LSB) MIPS ELF with NO PT_INTERP (fully static). Catches a wrong-variant
    download (glibc/dynamic, big-endian `mips`, or 64-bit `mips64`) before it reaches
    a router. Soft-float is guaranteed by the asset name we fetch (`-softfloat-musl`
    / bol-van's `linux-mipsel`), not parsed here — a reliable FP-ABI read needs the
    .MIPS.abiflags section; the on-device `sing-box version` smoke test is the
    backstop for the float ABI."""
    import struct
    if data[:4] != b"\x7fELF":
        die(f"{label}: not an ELF binary")
    ei_class, ei_data = data[4], data[5]
    if ei_class != 1:
        die(f"{label}: not ELF32 (class={ei_class}) — Keenetic mipsel is 32-bit")
    if ei_data != 1:
        die(f"{label}: not little-endian (data={ei_data}) — need mipsle, not big-endian mips")
    e = "<"
    mach = struct.unpack(e + "H", data[0x12:0x14])[0]
    if mach != 0x08:  # EM_MIPS
        die(f"{label}: not MIPS (machine=0x{mach:x})")
    phoff = struct.unpack(e + "I", data[0x1c:0x20])[0]       # ELF32 e_phoff
    phentsize = struct.unpack(e + "H", data[0x2a:0x2c])[0]   # ELF32 e_phentsize
    phnum = struct.unpack(e + "H", data[0x2c:0x2e])[0]       # ELF32 e_phnum
    interp = any(
        struct.unpack(e + "I", data[phoff + i * phentsize: phoff + i * phentsize + 4])[0] == 3
        for i in range(phnum)
    )
    if interp:
        die(f"{label}: dynamically linked (PT_INTERP present) — needs the static/musl build")
    return True


def _assert_static_elf(data, label, want):
    """Обобщённая версия проверок выше: класс/порядок байт/машина + отсутствие
    PT_INTERP. Нужна для apk-фида, где семейств шесть, а не одно."""
    import struct
    want_class, want_data, want_mach = want
    if data[:4] != b"\x7fELF":
        die(f"{label}: not an ELF binary")
    ei_class, ei_data = data[4], data[5]
    if ei_class != want_class:
        die(f"{label}: ELF class {ei_class}, expected {want_class}")
    if ei_data != want_data:
        die(f"{label}: ELF endianness {ei_data}, expected {want_data}")
    e = "<" if ei_data == 1 else ">"
    mach = struct.unpack(e + "H", data[0x12:0x14])[0]
    if mach != want_mach:
        die(f"{label}: e_machine 0x{mach:x}, expected 0x{want_mach:x}")
    if ei_class == 2:
        phoff = struct.unpack(e + "Q", data[0x20:0x28])[0]
        phentsize = struct.unpack(e + "H", data[0x36:0x38])[0]
        phnum = struct.unpack(e + "H", data[0x38:0x3a])[0]
    else:
        phoff = struct.unpack(e + "I", data[0x1c:0x20])[0]
        phentsize = struct.unpack(e + "H", data[0x2a:0x2c])[0]
        phnum = struct.unpack(e + "H", data[0x2c:0x2e])[0]
    interp = any(
        struct.unpack(e + "I", data[phoff + i * phentsize: phoff + i * phentsize + 4])[0] == 3
        for i in range(phnum)
    )
    if interp:
        die(f"{label}: dynamically linked (PT_INTERP present) — need the static build")
    return True


def fetch_apk_binaries(cpu, tpws_version=None, nfqws2_rel=None):
    """Скачать в кэш бинарники одного CPU-семейства для apk-фида.

    Оба проекта кладут в свой релизный тарбол готовые сборки под все платформы
    (`binaries/<target>/…`), поэтому качаем архив один раз на пакет и достаём из
    него нужный каталог. Каждый файл проверяется на «тот ли это ELF и статический
    ли он» — иначе на роутер уехал бы бинарник чужой архитектуры, и это вскрылось
    бы только в момент запуска движка."""
    import io as _io, tarfile as _tf
    spec = APK_TARGETS[cpu]
    target, elf = spec["zapret"], spec["elf"]

    if tpws_version:
        url = (f"https://github.com/{ZAPRET_REPO}/releases/download/v{tpws_version}/"
               f"zapret-v{tpws_version}.tar.gz")
        member = f"zapret-v{tpws_version}/binaries/{target}/tpws"
        print(f"  [{cpu}] fetching tpws {tpws_version} ({target}) ...")
        with _tf.open(fileobj=_io.BytesIO(_http_get(url)), mode="r:gz") as tf:
            m = next((x for x in tf.getmembers() if x.name == member), None)
            if m is None:
                die(f"tpws {tpws_version}: {member} not found in {url}")
            data = tf.extractfile(m).read()
        _assert_static_elf(data, f"tpws {tpws_version} [{cpu}]", elf)
        dest = apk_bin_path(cpu, "tpws-zapret")
        os.makedirs(os.path.dirname(dest), exist_ok=True)
        with open(dest, "wb") as f:
            f.write(data)
        os.chmod(dest, 0o755)
        print(f"    -> {dest} ({len(data):,} B)")

    if nfqws2_rel:
        url = (f"https://github.com/{ZAPRET2_REPO}/releases/download/{nfqws2_rel}/"
               f"zapret2-{nfqws2_rel}.tar.gz")
        member = f"zapret2-{nfqws2_rel}/binaries/{target}/nfqws2"
        print(f"  [{cpu}] fetching nfqws2 {nfqws2_rel} ({target}) ...")
        with _tf.open(fileobj=_io.BytesIO(_http_get(url)), mode="r:gz") as tf:
            m = next((x for x in tf.getmembers() if x.name == member), None)
            if m is None:
                die(f"nfqws2 {nfqws2_rel}: {member} not found in {url}")
            data = tf.extractfile(m).read()
            _assert_static_elf(data, f"nfqws2 {nfqws2_rel} [{cpu}]", elf)
            dest = apk_bin_path(cpu, "nfqws2")
            os.makedirs(os.path.dirname(dest), exist_ok=True)
            with open(dest, "wb") as f:
                f.write(data)
            os.chmod(dest, 0o755)
            print(f"    -> {dest} ({len(data):,} B)")
            # lua-скрипты общие для всех семейств — кладём в тот же кэш, что и
            # aarch64-ветка, чтобы не качать их шесть раз.
            os.makedirs(NFQWS_LUA_DIR, exist_ok=True)
            for name in NFQWS_LUA_FILES:
                lp = os.path.join(NFQWS_LUA_DIR, name)
                if os.path.isfile(lp):
                    continue
                lm = tf.getmember(f"zapret2-{nfqws2_rel}/lua/{name}")
                with open(lp, "wb") as f:
                    f.write(tf.extractfile(lm).read())


def fetch_singbox(version):
    """Download sing-box <version> (linux-arm64 MUSL = fully static) → SB_BINARY.
    Overwrites any existing copy. The plain `-linux-arm64` asset is glibc-dynamic
    since 1.13.13; we MUST use `-musl` for the musl-based router."""
    import io as _io, tarfile as _tf
    url = (f"https://github.com/{SINGBOX_REPO}/releases/download/v{version}/"
           f"sing-box-{version}-linux-arm64-musl.tar.gz")
    print(f"  fetching sing-box {version} (linux-arm64-musl) ...")
    with _tf.open(fileobj=_io.BytesIO(_http_get(url)), mode="r:gz") as tf:
        member = next((m for m in tf.getmembers() if m.name.endswith("/sing-box")), None)
        if member is None:
            die(f"sing-box {version}: no sing-box binary in {url}")
        data = tf.extractfile(member).read()
    _assert_static_arm64(data, f"sing-box {version}")
    os.makedirs(os.path.dirname(SB_BINARY), exist_ok=True)
    with open(SB_BINARY, "wb") as f:
        f.write(data)
    os.chmod(SB_BINARY, 0o755)
    print(f"    -> {SB_BINARY} ({len(data):,} B, static aarch64)")


def fetch_tpws(version):
    """Download zapret tpws <version> (binaries/linux-arm64/tpws) → TPWS_BINARY."""
    import io as _io, tarfile as _tf
    url = (f"https://github.com/{ZAPRET_REPO}/releases/download/v{version}/"
           f"zapret-v{version}.tar.gz")
    member_name = f"zapret-v{version}/binaries/linux-arm64/tpws"
    print(f"  fetching tpws {version} (linux-arm64) ...")
    with _tf.open(fileobj=_io.BytesIO(_http_get(url)), mode="r:gz") as tf:
        m = next((x for x in tf.getmembers() if x.name == member_name), None)
        if m is None:
            die(f"tpws {version}: {member_name} not found in {url}")
        data = tf.extractfile(m).read()
    _assert_static_arm64(data, f"tpws {version}")
    os.makedirs(os.path.dirname(TPWS_BINARY), exist_ok=True)
    with open(TPWS_BINARY, "wb") as f:
        f.write(data)
    os.chmod(TPWS_BINARY, 0o755)
    print(f"    -> {TPWS_BINARY} ({len(data):,} B, static aarch64)")


def fetch_singbox_mipsel(version):
    """Download sing-box <version> for Keenetic/MT7621 → SB_BINARY_MIPSEL.
    The `-linux-mipsle-softfloat-musl` asset is 32-bit little-endian MIPS,
    soft-float, fully static (musl) — the right ABI for Entware mipselsf and
    immune to the `Error relocating` the dynamic builds would hit."""
    import io as _io, tarfile as _tf
    url = (f"https://github.com/{SINGBOX_REPO}/releases/download/v{version}/"
           f"sing-box-{version}-linux-mipsle-softfloat-musl.tar.gz")
    print(f"  fetching sing-box {version} (linux-mipsle-softfloat-musl) ...")
    with _tf.open(fileobj=_io.BytesIO(_http_get(url)), mode="r:gz") as tf:
        member = next((m for m in tf.getmembers() if m.name.endswith("/sing-box")), None)
        if member is None:
            die(f"sing-box {version}: no sing-box binary in {url}")
        data = tf.extractfile(member).read()
    _assert_static_mipsel(data, f"sing-box {version} (mipsel)")
    os.makedirs(os.path.dirname(SB_BINARY_MIPSEL), exist_ok=True)
    with open(SB_BINARY_MIPSEL, "wb") as f:
        f.write(data)
    os.chmod(SB_BINARY_MIPSEL, 0o755)
    print(f"    -> {SB_BINARY_MIPSEL} ({len(data):,} B, static mipsel)")


def fetch_tpws_mipsel(version):
    """Download zapret tpws <version> (binaries/linux-mipsel/tpws) → TPWS_BINARY_MIPSEL.
    bol-van ships a static mipsel tpws prebuilt inside the release tarball — the same
    one the panel used to bundle, now served from the feed so it upgrades on its own."""
    import io as _io, tarfile as _tf
    url = (f"https://github.com/{ZAPRET_REPO}/releases/download/v{version}/"
           f"zapret-v{version}.tar.gz")
    member_name = f"zapret-v{version}/binaries/linux-mipsel/tpws"
    print(f"  fetching tpws {version} (linux-mipsel) ...")
    with _tf.open(fileobj=_io.BytesIO(_http_get(url)), mode="r:gz") as tf:
        m = next((x for x in tf.getmembers() if x.name == member_name), None)
        if m is None:
            die(f"tpws {version}: {member_name} not found in {url}")
        data = tf.extractfile(m).read()
    _assert_static_mipsel(data, f"tpws {version} (mipsel)")
    os.makedirs(os.path.dirname(TPWS_BINARY_MIPSEL), exist_ok=True)
    with open(TPWS_BINARY_MIPSEL, "wb") as f:
        f.write(data)
    os.chmod(TPWS_BINARY_MIPSEL, 0o755)
    print(f"    -> {TPWS_BINARY_MIPSEL} ({len(data):,} B, static mipsel)")


def fetch_nfqws2_assets(rel=None, force=False):
    """Populate NFQWS_BINARY + the 3 lua files from a zapret2 release.

    Binary comes from the openwrt-embedded bundle (binaries/linux-arm64/nfqws2);
    the lua scripts come from the full-source tarball (the embedded bundle ships
    only the antidpi .gz, which is incomplete — see BYPASS_STRATEGIES.md).
    `rel` overrides the pinned release tag (e.g. "v1.0.2", driven by
    --nfqws2-version under --fetch-upstream). `force` re-downloads even if cached.
    Without force, cached files under router-backup are reused to skip the download."""
    import io as _io, tarfile as _tf
    rel = rel or ZAPRET2_REL
    embedded = (f"https://github.com/{ZAPRET2_REPO}/releases/download/{rel}/"
                f"zapret2-{rel}-openwrt-embedded.tar.gz")
    source = (f"https://github.com/{ZAPRET2_REPO}/releases/download/{rel}/"
              f"zapret2-{rel}.tar.gz")
    arm64_bin = f"zapret2-{rel}/binaries/linux-arm64/nfqws2"

    if force:
        for p in [NFQWS_BINARY] + [os.path.join(NFQWS_LUA_DIR, n) for n in NFQWS_LUA_FILES]:
            try:
                os.remove(p)
            except OSError:
                pass

    os.makedirs(os.path.dirname(NFQWS_BINARY), exist_ok=True)
    os.makedirs(NFQWS_LUA_DIR, exist_ok=True)

    if not os.path.isfile(NFQWS_BINARY):
        print(f"  fetching nfqws2 (arm64) from {rel} ...")
        with _tf.open(fileobj=_io.BytesIO(_http_get(embedded)), mode="r:gz") as tf:
            data = tf.extractfile(arm64_bin).read()
        with open(NFQWS_BINARY, "wb") as f:
            f.write(data)
        os.chmod(NFQWS_BINARY, 0o755)
        print(f"    -> {NFQWS_BINARY} ({len(data):,} B)")

    missing = [n for n in NFQWS_LUA_FILES if not os.path.isfile(os.path.join(NFQWS_LUA_DIR, n))]
    if missing:
        print(f"  fetching nfqws2 lua {missing} from {rel} source ...")
        with _tf.open(fileobj=_io.BytesIO(_http_get(source)), mode="r:gz") as tf:
            for n in missing:
                data = tf.extractfile(f"zapret2-{rel}/lua/{n}").read()
                with open(os.path.join(NFQWS_LUA_DIR, n), "wb") as f:
                    f.write(data)
                print(f"    -> {os.path.join(NFQWS_LUA_DIR, n)} ({len(data):,} B)")


def _md5_file(path):
    import hashlib
    h = hashlib.md5()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def _extract_control(ipk_path):
    """Read the inner control.tar.gz/./control text out of an .ipk."""
    with tarfile.open(ipk_path, "r:gz") as outer:
        member = outer.extractfile("./control.tar.gz") or outer.extractfile("control.tar.gz")
        if member is None:
            die(f"{ipk_path}: no control.tar.gz inside")
        ctrl_bytes = member.read()
    with tarfile.open(fileobj=io.BytesIO(ctrl_bytes), mode="r:gz") as ctl:
        cf = ctl.extractfile("./control") or ctl.extractfile("control")
        if cf is None:
            die(f"{ipk_path}: no ./control in control.tar.gz")
        return cf.read().decode("utf-8")


def _control_from_apk(apk_path):
    """Сделать opkg-подобную строфу из `.PKGINFO` внутри `.apk`.

    Индекс фида у нас один и тот же для обоих форматов: его читает НЕ apk (свой
    фид мы на apk-прошивках не прописываем — см. detour-update), а наш собственный
    feed_curl_install, которому нужны Package/Version/Filename. Держать формат
    индекса общим дешевле, чем городить рядом APKINDEX."""
    info = {}
    with tarfile.open(apk_path, "r:gz") as t:
        for line in t.extractfile(".PKGINFO").read().decode("utf-8").splitlines():
            if " = " in line:
                k, v = line.split(" = ", 1)
                info.setdefault(k, v)
    # «72.13-r1» → «72.13»: ревизия apk наружу не выставляется (см. build_apk).
    ver = info.get("pkgver", "")
    if "-r" in ver:
        ver = ver.rsplit("-r", 1)[0]
    return (
        f"Package: {info.get('pkgname', '')}\n"
        f"Version: {ver}\n"
        f"Architecture: {info.get('arch', apk_pkg.NOARCH)}\n"
        f"Installed-Size: {info.get('size', '0')}\n"
        f"Description: {info.get('pkgdesc', '')}\n"
    )


def build_packages_index(out_dir):
    """Return the `Packages` index covering EVERY package file in out_dir.

    Each stanza is the package's own control text plus the feed-side fields opkg
    needs to fetch + verify: Filename/Size/MD5Sum/SHA256sum. Indexing everything
    present means a one-package rebuild never drops the other from the feed."""
    ext = "." + PKG_FORMAT
    ipks = sorted(n for n in os.listdir(out_dir) if n.endswith(ext))
    if not ipks:
        die(f"no {ext} files in {out_dir} — build at least one package first")
    stanzas = []
    for name in ipks:
        path = os.path.join(out_dir, name)
        control = (_control_from_apk(path) if PKG_FORMAT == "apk"
                   else _extract_control(path)).rstrip("\n")
        size = os.path.getsize(path)
        control += (
            f"\nFilename: {name}\n"
            f"Size: {size}\n"
            f"MD5Sum: {_md5_file(path)}\n"
            f"SHA256sum: {sha256_file(path)}\n"
        )
        stanzas.append(control + "\n")  # trailing blank line terminates the stanza
    return "".join(stanzas), ipks


def write_feed(build_versions, out_dir):
    """Build the requested package .ipk(s), then (re)index everything in out_dir.

    build_versions: {pkg_name: version_string} for packages to (re)build now.
    Packages not in build_versions keep their existing .ipk in out_dir."""
    os.makedirs(out_dir, exist_ok=True)
    for pkg, version in build_versions.items():
        ipk_path, isize = build_ipk(pkg, version, out_dir)
        print(f"  built {os.path.basename(ipk_path)}  "
              f"({os.path.getsize(ipk_path):,} B, sha256 {sha256_file(ipk_path)[:16]}...)")

    # Guard: every package the panel Depends on (sing-box, tpws-zapret) must be in
    # the index. nfqws2 is OPTIONAL (only zapret2 mode uses it) — not required.
    present = {n.split("_", 1)[0] for n in os.listdir(out_dir)
               if n.endswith("." + PKG_FORMAT)}
    # На apk-прошивках sing-box берётся из РОДНОГО фида OpenWrt 25.12 (там уже
    # 1.13.x), поэтому свой apk-фид обязан отдавать только движки DPI-обхода.
    REQUIRED = ("tpws-zapret",) if PKG_FORMAT == "apk" else ("sing-box", "tpws-zapret")
    for required in REQUIRED:
        if required not in present:
            die(f"{required} .{PKG_FORMAT} missing from {out_dir}. Pass its version "
                f"(e.g. --{'tpws-version' if required == 'tpws-zapret' else 'version'}) "
                f"to build it — the feed must serve every package the panel Depends on.")

    packages_txt, ipks = build_packages_index(out_dir)
    packages_path = os.path.join(out_dir, "Packages")
    with open(packages_path, "w", encoding="utf-8", newline="\n") as f:
        f.write(packages_txt)
    print(f"  indexed {len(ipks)} package(s): {', '.join(ipks)}")

    # gzip with no mtime so identical inputs produce identical output (and the
    # `feed` branch only churns when a package actually changes).
    gz_path = packages_path + ".gz"
    with open(packages_path, "rb") as src, open(gz_path, "wb") as dst:
        with gzip.GzipFile(fileobj=dst, mode="wb", mtime=0) as gz:
            gz.write(src.read())
    print(f"  {gz_path}")

    # Sign the *uncompressed* Packages (matches `usign -S -m Packages`).
    if os.path.isfile(KEY_SEC_USIGN):
        sig_path = os.path.join(out_dir, "Packages.sig")
        sign_file(packages_path, KEY_SEC_USIGN, sig_path)
        keynum, _ = load_public_key(KEY_PUB_USIGN)
        print(f"  {sig_path}  (usign key {keynum.hex()})")
    else:
        print(f"  (UNSIGNED — usign secret key missing at {KEY_SEC_USIGN})")
    return out_dir


# ============ publish to the orphan `feed` branch ============

def _run(cmd, cwd=None, check=True, quiet_url=None):
    """Run a git command; scrub a token-bearing URL from any echoed output."""
    printable = " ".join(cmd)
    if quiet_url:
        printable = printable.replace(quiet_url, "https://***@github.com/...")
    print(f"  $ {printable}")
    res = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if res.returncode != 0 and check:
        err = (res.stderr or res.stdout or "").strip()
        if quiet_url:
            err = err.replace(quiet_url, "https://***@github.com/...")
        die(f"git failed ({res.returncode}): {err[:500]}")
    return res


def publish_feed(commit_msg, overlays):
    """Force-push the local feed tree to the orphan `feed` branch as one commit.

    Multi-arch safe: the branch can carry several arch sub-dirs (aarch64, mipsel,
    ...). We fetch the CURRENT branch first and carry its other arch dirs over
    unchanged, then overlay only the arch we just built — so publishing mipsel never
    drops the aarch64 feed the whole OpenWrt fleet depends on (and vice versa).

    Uses a throwaway git repo in releases/feed/.git-publish so the working tree (on
    `main`) is never touched, and a single squashed ORPHAN root commit (no parent)
    so the large blobs do not accumulate across releases."""
    owner, repo, token = _load_github_config()
    remote = f"https://x-access-token:{token}@github.com/{owner}/{repo}.git"

    stage = os.path.join(HERE, "releases", "feed", ".git-publish")
    import shutil
    import stat
    if os.path.isdir(stage):
        # .git objects are read-only on Windows — plain rmtree leaves them behind
        # and the makedirs below dies with FileExistsError.
        def _chmod_retry(func, path, _exc):
            os.chmod(path, stat.S_IWRITE)
            func(path)
        shutil.rmtree(stage, onerror=_chmod_retry)
    os.makedirs(stage)

    _run(["git", "init", "-q", "-b", FEED_BRANCH], cwd=stage)
    _run(["git", "config", "user.name", "detour-feed"], cwd=stage)
    _run(["git", "config", "user.email", "feed@detour.local"], cwd=stage)

    # Seed the stage with the CURRENT feed branch so sibling arch dirs survive.
    # Best-effort: the very first publish has no branch yet (fetch returns non-zero).
    # We load the fetched tree via read-tree+checkout-index (NOT `git checkout`) so
    # HEAD stays the unborn `feed` branch → the commit below is a parentless root.
    fetched = _run(["git", "fetch", "-q", "--depth", "1", remote, FEED_BRANCH],
                   cwd=stage, check=False, quiet_url=remote).returncode == 0
    if fetched:
        _run(["git", "read-tree", "FETCH_HEAD"], cwd=stage)
        _run(["git", "checkout-index", "-a", "-f"], cwd=stage)
        print(f"[publish] carried over existing feed branch ({owner}/{repo}@{FEED_BRANCH})")
    else:
        print(f"[publish] no existing {FEED_BRANCH} branch — creating it fresh")

    # Overlay ONLY the arch dirs we just built (`overlays` = {имя: путь}), всё
    # остальное на ветке остаётся как было.
    for name, src in sorted(overlays.items()):
        dst_arch = os.path.join(stage, name)
        if os.path.isdir(dst_arch):
            shutil.rmtree(dst_arch, onerror=lambda f, p, _e: (os.chmod(p, stat.S_IWRITE), f(p)))
        shutil.copytree(src, dst_arch)

    arch_dirs = sorted(d for d in os.listdir(stage)
                       if os.path.isdir(os.path.join(stage, d)) and not d.startswith("."))
    # A tiny landing file so the branch root isn't empty / 404 on humans — one opkg
    # line per arch sub-dir present.
    with open(os.path.join(stage, "README.md"), "w", encoding="utf-8", newline="\n") as f:
        f.write("# detour opkg feed\n\n"
                "Auto-generated by `build_feed.py`. Serves sing-box (+ tpws-zapret) "
                "for the detour panel.\n\n")
        for a in arch_dirs:
            if a.startswith(APK_FEED_PREFIX):
                # apk-каталоги opkg-строкой не прописываются: на apk-прошивках
                # detour-update качает пакет по этому URL сам (см. feed_curl_install).
                f.write(f"apk ({a[len(APK_FEED_PREFIX):]}): "
                        f"`https://raw.githubusercontent.com/{owner}/{repo}/"
                        f"{FEED_BRANCH}/{a}`\n\n")
            else:
                f.write(f"```\nsrc/gz detour https://raw.githubusercontent.com/{owner}/{repo}/"
                        f"{FEED_BRANCH}/{a}\n```\n\n")

    print(f"[publish] arch dirs on the feed after this push: {', '.join(arch_dirs)}")
    print(f"[publish] force-pushing feed tree -> {owner}/{repo}@{FEED_BRANCH}")
    _run(["git", "add", "-A"], cwd=stage)
    _run(["git", "commit", "-q", "-m", commit_msg], cwd=stage)
    _run(["git", "push", "--force", remote, f"{FEED_BRANCH}:{FEED_BRANCH}"],
         cwd=stage, quiet_url=remote)
    shutil.rmtree(stage, ignore_errors=True)

    base = f"https://raw.githubusercontent.com/{owner}/{repo}/{FEED_BRANCH}"
    for name in sorted(overlays):
        print(f"[publish] feed live at: {base}/{name}")
    return base


def parse_version(v, label):
    if v.startswith("v"):
        v = v[1:]
    parts = v.split(".")
    # До ЧЕТЫРЁХ компонент: zapret2 выпускается тегами вида v1.0.5.1, и на трёх
    # эта проверка роняла автопубликацию фида ровно в тот день, когда апстрим
    # выпускал такую версию.
    if not (1 <= len(parts) <= 4) or not all(p.isdigit() for p in parts):
        die(f"--{label} must be a dotted numeric version, got {v!r}")
    return v


def select_arch(arch):
    """Repoint the module-level feed globals at the requested arch. `aarch64`
    (OpenWrt/GL.iNet) is the default and leaves everything as-is; `mipsel`
    (Keenetic/Entware) swaps in the /opt package specs and the mipsel feed dir.
    The Architecture field stays `all` for both — the feed is path-segregated by
    sub-dir, and `all` side-loads on every router regardless of its exact opkg arch."""
    global FEED_ARCH_DIR, FEED_OUT, PKG_SPECS, PKG_FORMAT
    if arch == "aarch64":
        FEED_ARCH_DIR = "aarch64"
        FEED_OUT = os.path.join(HERE, "releases", "feed", FEED_ARCH_DIR)
        PKG_SPECS = PKG_SPECS_OPENWRT
        PKG_FORMAT = "ipk"
        return
    if arch == "mipsel":
        FEED_ARCH_DIR = MIPSEL_FEED_ARCH_DIR
        FEED_OUT = os.path.join(HERE, "releases", "feed", FEED_ARCH_DIR)
        PKG_SPECS = PKG_SPECS_MIPSEL
        PKG_FORMAT = "ipk"
        return
    if arch.startswith(APK_FEED_PREFIX):
        cpu = arch[len(APK_FEED_PREFIX):]
        if cpu not in APK_TARGETS:
            die(f"unknown apk CPU family {cpu!r} (have: {', '.join(sorted(APK_TARGETS))})")
        FEED_ARCH_DIR = arch
        FEED_OUT = os.path.join(HERE, "releases", "feed", FEED_ARCH_DIR)
        PKG_SPECS = apk_pkg_specs(cpu)
        PKG_FORMAT = "apk"
        return
    die(f"unknown --arch {arch!r}")


ALL_APK_ARCHES = tuple(APK_FEED_PREFIX + c for c in sorted(APK_TARGETS))


def expand_arches(spec):
    """`--arch` → список конкретных каталогов фида.

    Кроме точных имён понимает две группы: `apk-all` (все CPU-семейства
    apk-фида) и `all` (вообще всё, что публикуется). Группы нужны прежде всего
    CI: одна команда собирает и кладёт всё за один force-push ветки."""
    if spec == "apk-all":
        return list(ALL_APK_ARCHES)
    if spec == "all":
        return ["aarch64", "mipsel"] + list(ALL_APK_ARCHES)
    return [spec]


def build_arch(arch, args, sb_ver, tpws_ver, nfqws2_ver):
    """Собрать и проиндексировать фид одной архитектуры. Возвращает (имя, путь)."""
    select_arch(arch)
    is_apk = PKG_FORMAT == "apk"
    cpu = arch[len(APK_FEED_PREFIX):] if is_apk else None

    if nfqws2_ver and arch == "mipsel":
        die("nfqws2 (zapret2) is OpenWrt-only: KeeneticOS has no NFQUEUE, so the engine "
            "cannot run on mipsel. Build it for --arch aarch64 only.")

    build_versions = {}
    if is_apk:
        # sing-box для apk-прошивок НЕ собираем: в родном фиде OpenWrt 25.12 уже
        # 1.13.x — та самая схема конфига, которую ждёт панель. Этот фид нужен
        # только ради движков DPI-обхода, которых там нет ни у кого.
        if sb_ver:
            print(f"  [{arch}] sing-box пропущен — на apk-прошивках он берётся "
                  f"из родного фида OpenWrt")
        if tpws_ver or nfqws2_ver:
            rel = f"v{nfqws2_ver}" if nfqws2_ver else None
            fetch_apk_binaries(cpu, tpws_version=tpws_ver, nfqws2_rel=rel)
        if tpws_ver:
            build_versions["tpws-zapret"] = f"{tpws_ver}-{args.revision}"
        if nfqws2_ver:
            build_versions["nfqws2"] = f"{nfqws2_ver}-{args.revision}"
    else:
        # --fetch-upstream: pull each requested binary straight from its upstream
        # release so a CI runner needs no router-backup checkout. The static-ELF
        # guard in the fetchers refuses a wrong-variant (e.g. glibc sing-box).
        if args.fetch_upstream:
            if sb_ver:
                (fetch_singbox_mipsel if arch == "mipsel" else fetch_singbox)(sb_ver)
            if tpws_ver:
                (fetch_tpws_mipsel if arch == "mipsel" else fetch_tpws)(tpws_ver)
        if sb_ver:
            build_versions["sing-box"] = f"{sb_ver}-{args.revision}"
        if tpws_ver:
            build_versions["tpws-zapret"] = f"{tpws_ver}-{args.revision}"
        if nfqws2_ver:
            # nfqws2 binary+lua always come from upstream; under --fetch-upstream pin
            # the release tag to the requested version and force a fresh download.
            rel = f"v{nfqws2_ver}" if args.fetch_upstream else None
            fetch_nfqws2_assets(rel=rel, force=args.fetch_upstream)
            build_versions["nfqws2"] = f"{nfqws2_ver}-{args.revision}"

    if not build_versions and not os.path.isdir(FEED_OUT):
        die(f"nothing to build for {arch}: pass --version / --tpws-version / --nfqws2-version")

    label = ", ".join(f"{k} {v}" for k, v in build_versions.items()) or "(re-index only)"
    fmt = "apk" if is_apk else "opkg"
    print(f"=== Building {fmt} feed [{FEED_ARCH_DIR}]: {label} ===")
    print(f"Output: {FEED_OUT}")
    write_feed(build_versions, FEED_OUT)
    return FEED_ARCH_DIR, FEED_OUT


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--version", help="sing-box version to build, e.g. 1.13.2")
    ap.add_argument("--tpws-version", help="tpws-zapret (zapret) version to build, e.g. 72.12")
    ap.add_argument("--nfqws2-version", help=f"nfqws2 (zapret2) version to build, e.g. 1.0.1 "
                    f"(fetched from {ZAPRET2_REPO}@{ZAPRET2_REL})")
    ap.add_argument("--revision", default=DEFAULT_REVISION,
                    help=f"opkg package revision suffix (default {DEFAULT_REVISION})")
    ap.add_argument("--publish", action="store_true",
                    help="force-push the feed tree to the `feed` branch")
    ap.add_argument("--fetch-upstream", action="store_true",
                    help="download each binary from its upstream GitHub release "
                         "(sing-box -musl, tpws, nfqws2) instead of using the local "
                         "router-backup copy — needed for headless/CI builds. "
                         "Для apk-архитектур загрузка идёт из апстрима ВСЕГДА.")
    ap.add_argument("--arch", default="aarch64",
                    choices=("aarch64", "mipsel", "apk-all", "all") + ALL_APK_ARCHES,
                    help="target feed dir: aarch64 = OpenWrt/GL.iNet opkg (default), "
                         "mipsel = Keenetic/Entware opkg, apk-<cpu> = OpenWrt 25.12+ "
                         "(apk-tools 3; tpws + nfqws2 only, sing-box там из родного "
                         "фида), apk-all = все apk-семейства, all = вообще всё. "
                         "nfqws2 не собирается для mipsel (нет NFQUEUE на KeeneticOS)")
    args = ap.parse_args()

    sb_ver = parse_version(args.version, "version") if args.version else None
    tpws_ver = parse_version(args.tpws_version, "tpws-version") if args.tpws_version else None
    nfqws2_ver = parse_version(args.nfqws2_version, "nfqws2-version") if args.nfqws2_version else None

    arches = expand_arches(args.arch)
    overlays = {}
    for arch in arches:
        # mipsel не умеет nfqws2 — на групповых сборках просто не передаём его туда,
        # иначе build_arch честно упал бы и увёл с собой весь прогон.
        nf = None if arch == "mipsel" else nfqws2_ver
        name, path = build_arch(arch, args, sb_ver, tpws_ver, nf)
        overlays[name] = path

    if args.publish:
        # Derive a commit message from whatever versions are now in each feed dir.
        parts = []
        for arch in arches:
            select_arch(arch)
            _, files = build_packages_index(FEED_OUT)
            parts.append(f"{FEED_ARCH_DIR}: " + ", ".join(
                n.rsplit("_", 1)[0] for n in files))
        publish_feed("feed — " + " | ".join(parts), overlays)

    print("\n=== DONE ===")
    if not args.publish:
        print("Publish to the feed branch with: --publish")


if __name__ == "__main__":
    main()
