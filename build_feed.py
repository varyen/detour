#!/usr/bin/env python3
"""Build + publish the self-hosted opkg feed that serves sing-box to GL.iNet.

The GL.iNet/OpenWrt distro feed is pinned to sing-box 1.8.10, which predates the
1.11 config-schema break and would corrupt the panel's 1.13.x config. So instead
of bundling a 22 MB `detour-bins` package, the `detour` panel declares
`Depends: sing-box` and we serve sing-box 1.13.x ourselves from a tiny opkg feed
hosted in the *public* `varyen/detour` repo.

Output (local):
    releases/feed/<arch>/sing-box_<ver>-<rev>_all.ipk
    releases/feed/<arch>/Packages           (concatenated control stanzas)
    releases/feed/<arch>/Packages.gz        (what `src/gz` fetches)
    releases/feed/<arch>/Packages.sig       (usign signature of Packages)

Publish (`--publish`): force-push the feed tree to a dedicated orphan branch
(`feed`) as a single squashed commit, so the 63 MB binary never accumulates in
history and `main` stays lean. Served over plain HTTPS via:
    src/gz detour https://raw.githubusercontent.com/varyen/detour/feed/<arch>

Routers get that line in /etc/opkg/customfeeds.conf (deploy_router.py /
detour-update), then `opkg install sing-box` pulls 1.13.x (our 1.13.2-1 cleanly
out-versions the distro's 1.8.10-1) and `opkg upgrade sing-box` keeps it current.

Signatures are not strictly required (opkg here has no `check_signature`), but we
sign Packages with the same usign key already pinned on every router for cheap
integrity + future-proofing.

Usage:
    python3 build_feed.py --version 1.13.2            # build only
    python3 build_feed.py --version 1.13.2 --publish  # build + push feed branch
"""
import argparse
import gzip
import io
import os
import subprocess
import sys
import tarfile
from datetime import datetime, timezone

# Reuse the ipk tar helpers + GH config loader from the release builder so the
# two packagers never drift on archive format / signing / auth.
from build_release import (
    HERE, KEY_SEC_USIGN, KEY_PUB_USIGN, BACKUP_HOME,
    _add_bytes_to_tar, _add_file_to_tar, _add_dir_to_tar,
    sha256_file, _load_github_config, die,
)
from usign_compat import sign_file, load_public_key

PKG = "sing-box"
ARCH = "all"  # static Go binary; `all` so it installs on any aarch64 opkg-arch
              # string (the fleet reports aarch64_cortex-a53_neon-vfpv4, etc.).
FEED_ARCH_DIR = "aarch64"  # logical feed sub-dir (one per binary arch family)
FEED_BRANCH = "feed"
DEFAULT_REVISION = "1"

# The 1.13.x sing-box we ship today (musl-free static Go, portable across aarch64).
SB_BINARY = os.path.join(BACKUP_HOME, "usr", "bin", "sing-box")
SB_INSTALL_PATH = "usr/bin/sing-box"   # MUST match the OpenWrt init.d expectation

FEED_OUT = os.path.join(HERE, "releases", "feed", FEED_ARCH_DIR)

MAINTAINER = "Maintainer <you@example.com>"


def _control_text(version, installed_size):
    return (
        f"Package: {PKG}\n"
        f"Version: {version}\n"
        f"Source: https://github.com/varyen/detour\n"
        f"License: GPL-3.0-or-later\n"
        f"Section: net\n"
        f"Priority: optional\n"
        f"Maintainer: {MAINTAINER}\n"
        f"Architecture: {ARCH}\n"
        f"Installed-Size: {installed_size}\n"
        f"Description: sing-box universal proxy platform ({version}). "
        f"Detour feed build for OpenWrt/GL.iNet (the distro feed is stuck on 1.8.x).\n"
    )


def _build_control_tar_gz(version, installed_size):
    # postinst is deliberately minimal: it only owns the binary. chmod + a
    # best-effort service restart so `opkg upgrade sing-box` takes effect when
    # the detour panel is installed; harmless (and silent) if it isn't.
    postinst = """#!/bin/sh
set +e
chmod 0755 /usr/bin/sing-box 2>/dev/null
[ -x /etc/init.d/sing-box ] && /etc/init.d/sing-box restart >/dev/null 2>&1
exit 0
"""
    prerm = """#!/bin/sh
set +e
# Stop the service so the busy binary can be replaced cleanly on upgrade.
[ -x /etc/init.d/sing-box ] && /etc/init.d/sing-box stop >/dev/null 2>&1
exit 0
"""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.USTAR_FORMAT) as tar:
        _add_bytes_to_tar(tar, "./control", _control_text(version, installed_size).encode(), 0o644)
        _add_bytes_to_tar(tar, "./postinst", postinst.encode(), 0o755)
        _add_bytes_to_tar(tar, "./prerm", prerm.encode(), 0o755)
    return buf.getvalue()


def _build_data_tar_gz():
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.USTAR_FORMAT) as tar:
        _add_dir_to_tar(tar, "./usr/")
        _add_dir_to_tar(tar, "./usr/bin/")
        _add_file_to_tar(tar, SB_BINARY, "./" + SB_INSTALL_PATH, 0o755)
    return buf.getvalue()


def build_ipk(version, out_dir):
    """Assemble the sing-box .ipk. Returns (ipk_path, installed_size)."""
    if not os.path.isfile(SB_BINARY):
        die(f"sing-box binary not found at {SB_BINARY} — run update_backups.py first")
    installed_size = os.path.getsize(SB_BINARY)
    control_tgz = _build_control_tar_gz(version, installed_size)
    data_tgz = _build_data_tar_gz()
    os.makedirs(out_dir, exist_ok=True)
    ipk_path = os.path.join(out_dir, f"{PKG}_{version}_{ARCH}.ipk")
    with tarfile.open(ipk_path, "w:gz", format=tarfile.USTAR_FORMAT) as tar:
        _add_bytes_to_tar(tar, "./debian-binary", b"2.0\n", 0o644)
        _add_bytes_to_tar(tar, "./control.tar.gz", control_tgz, 0o644)
        _add_bytes_to_tar(tar, "./data.tar.gz", data_tgz, 0o644)
    return ipk_path, installed_size


def _md5_file(path):
    import hashlib
    h = hashlib.md5()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def build_packages_index(version, ipk_path, installed_size):
    """Return the `Packages` index text for the single sing-box .ipk.

    opkg needs Package/Version/Architecture + Filename/Size/SHA256sum to fetch
    and verify; MD5Sum is included for older opkg builds. Filename is relative to
    the feed base URL (the arch dir), where the .ipk sits beside Packages.gz."""
    filename = os.path.basename(ipk_path)
    size = os.path.getsize(ipk_path)
    stanza = (
        f"Package: {PKG}\n"
        f"Version: {version}\n"
        f"Architecture: {ARCH}\n"
        f"Installed-Size: {installed_size}\n"
        f"Filename: {filename}\n"
        f"Size: {size}\n"
        f"MD5Sum: {_md5_file(ipk_path)}\n"
        f"SHA256sum: {sha256_file(ipk_path)}\n"
        f"Section: net\n"
        f"Description: sing-box universal proxy platform ({version}). Detour feed.\n"
    )
    return stanza + "\n"  # trailing blank line terminates the stanza


def write_feed(version, out_dir):
    """Build the .ipk + Packages{,.gz,.sig} into out_dir. Returns out_dir."""
    ipk_path, installed_size = build_ipk(version, out_dir)
    print(f"  {ipk_path}  ({os.path.getsize(ipk_path):,} B, sha256 {sha256_file(ipk_path)[:16]}...)")

    packages_txt = build_packages_index(version, ipk_path, installed_size)
    packages_path = os.path.join(out_dir, "Packages")
    with open(packages_path, "w", encoding="utf-8", newline="\n") as f:
        f.write(packages_txt)

    # gzip with no mtime so identical inputs produce identical output (and the
    # `feed` branch only churns when the package actually changes).
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


def publish_feed(version, feed_arch_dir):
    """Force-push the local feed tree to the orphan `feed` branch as one commit.

    Uses a throwaway git repo in releases/feed/.git-publish so the working tree
    (on `main`) is never touched, and a single squashed commit so the large
    sing-box blob does not accumulate across releases."""
    owner, repo, token = _load_github_config()
    remote = f"https://x-access-token:{token}@github.com/{owner}/{repo}.git"

    stage = os.path.join(HERE, "releases", "feed", ".git-publish")
    import shutil
    if os.path.isdir(stage):
        shutil.rmtree(stage, ignore_errors=True)
    os.makedirs(stage)

    # Lay the feed tree under <stage>/<arch>/...
    dst_arch = os.path.join(stage, FEED_ARCH_DIR)
    shutil.copytree(feed_arch_dir, dst_arch)
    # A tiny landing file so the branch root isn't empty / 404 on humans.
    with open(os.path.join(stage, "README.md"), "w", encoding="utf-8", newline="\n") as f:
        f.write(
            "# detour opkg feed\n\n"
            "Auto-generated by `build_feed.py`. Serves sing-box for the detour panel.\n\n"
            "```\n"
            f"src/gz detour https://raw.githubusercontent.com/{owner}/{repo}/"
            f"{FEED_BRANCH}/{FEED_ARCH_DIR}\n"
            "```\n"
        )

    print(f"\n[publish] force-pushing feed tree -> {owner}/{repo}@{FEED_BRANCH}")
    _run(["git", "init", "-q", "-b", FEED_BRANCH], cwd=stage)
    _run(["git", "config", "user.name", "detour-feed"], cwd=stage)
    _run(["git", "config", "user.email", "feed@detour.local"], cwd=stage)
    _run(["git", "add", "-A"], cwd=stage)
    _run(["git", "commit", "-q", "-m", f"feed: sing-box {version}"], cwd=stage)
    _run(["git", "push", "--force", remote, f"{FEED_BRANCH}:{FEED_BRANCH}"],
         cwd=stage, quiet_url=remote)
    shutil.rmtree(stage, ignore_errors=True)

    raw = (f"https://raw.githubusercontent.com/{owner}/{repo}/"
           f"{FEED_BRANCH}/{FEED_ARCH_DIR}")
    print(f"[publish] feed live at: {raw}")
    print(f"[publish] opkg line:    src/gz detour {raw}")
    return raw


def parse_version(v):
    if v.startswith("v"):
        v = v[1:]
    parts = v.split(".")
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        die(f"--version must be sing-box X.Y.Z, got {v!r}")
    return v


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--version", required=True, help="sing-box version, e.g. 1.13.2")
    ap.add_argument("--revision", default=DEFAULT_REVISION,
                    help=f"opkg package revision suffix (default {DEFAULT_REVISION})")
    ap.add_argument("--publish", action="store_true",
                    help="force-push the feed tree to the `feed` branch")
    args = ap.parse_args()

    version = f"{parse_version(args.version)}-{args.revision}"
    print(f"=== Building opkg feed: {PKG} {version} ({ARCH}) ===")
    print(f"Output: {FEED_OUT}")
    write_feed(version, FEED_OUT)

    if args.publish:
        publish_feed(version, FEED_OUT)

    print("\n=== DONE ===")
    if not args.publish:
        print("Publish to the feed branch with: --publish")


if __name__ == "__main__":
    main()
