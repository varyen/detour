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

ARCH = "all"  # static binaries, portable across the aarch64 opkg-arch family
              # (the fleet reports aarch64_cortex-a53_neon-vfpv4, etc.). `all`
              # so a single .ipk installs on every aarch64 router.
FEED_ARCH_DIR = "aarch64"  # logical feed sub-dir (one per binary arch family)
FEED_BRANCH = "feed"
DEFAULT_REVISION = "1"

# The binaries we serve. Both are static and live under router-backup/usr/bin
# (refreshed from the home router by update_backups.py):
#   sing-box    — 1.13.x, musl-free static Go, portable across aarch64
#   tpws-zapret — zapret tpws, aarch64 musl-static (bol-van/zapret prebuilt)
SB_BINARY = os.path.join(BACKUP_HOME, "usr", "bin", "sing-box")
TPWS_BINARY = os.path.join(BACKUP_HOME, "usr", "bin", "tpws-zapret")

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

# Static package specs (binary + paths + maintainer scripts + description). The
# version is supplied at build time.
PKG_SPECS = {
    "sing-box": {
        "binary": SB_BINARY,
        "install_path": "usr/bin/sing-box",
        "postinst": _SINGBOX_POSTINST,
        "prerm": _SINGBOX_PRERM,
        "description": ("sing-box universal proxy platform. Detour feed build for "
                        "OpenWrt/GL.iNet (the distro feed is stuck on 1.8.x)."),
    },
    "tpws-zapret": {
        "binary": TPWS_BINARY,
        "install_path": "usr/bin/tpws-zapret",
        "postinst": _TPWS_POSTINST,
        "prerm": _TPWS_PRERM,
        "description": ("zapret tpws transparent DPI-bypass proxy (bol-van/zapret). "
                        "Detour feed build — zapret is in no opkg feed."),
    },
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


def _build_data_tar_gz(binary_path, install_path):
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.USTAR_FORMAT) as tar:
        # explicit parent dirs (shallowest first) so opkg can extract on a clean fs
        parts = install_path.strip("/").split("/")
        for i in range(1, len(parts)):
            _add_dir_to_tar(tar, "./" + "/".join(parts[:i]) + "/")
        _add_file_to_tar(tar, binary_path, "./" + install_path, 0o755)
    return buf.getvalue()


def build_ipk(pkg, version, out_dir):
    """Assemble one package's .ipk. Returns (ipk_path, installed_size)."""
    spec = PKG_SPECS[pkg]
    binary = spec["binary"]
    if not os.path.isfile(binary):
        die(f"{pkg} binary not found at {binary} — run update_backups.py first")
    installed_size = os.path.getsize(binary)
    control_tgz = _build_control_tar_gz(pkg, version, installed_size, spec)
    data_tgz = _build_data_tar_gz(binary, spec["install_path"])
    os.makedirs(out_dir, exist_ok=True)
    ipk_path = os.path.join(out_dir, f"{pkg}_{version}_{ARCH}.ipk")
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


def build_packages_index(out_dir):
    """Return the `Packages` index covering EVERY .ipk in out_dir.

    Each stanza is the package's own control text plus the feed-side fields opkg
    needs to fetch + verify: Filename/Size/MD5Sum/SHA256sum. Indexing all .ipk
    present means a one-package rebuild never drops the other from the feed."""
    ipks = sorted(n for n in os.listdir(out_dir) if n.endswith(".ipk"))
    if not ipks:
        die(f"no .ipk files in {out_dir} — build at least one package first")
    stanzas = []
    for name in ipks:
        path = os.path.join(out_dir, name)
        control = _extract_control(path).rstrip("\n")
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

    # Guard: every package the panel Depends on must be present in the index.
    present = {n.split("_", 1)[0] for n in os.listdir(out_dir) if n.endswith(".ipk")}
    for required in PKG_SPECS:
        if required not in present:
            die(f"{required} .ipk missing from {out_dir}. Pass its version "
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


def publish_feed(commit_msg, feed_arch_dir):
    """Force-push the local feed tree to the orphan `feed` branch as one commit.

    Uses a throwaway git repo in releases/feed/.git-publish so the working tree
    (on `main`) is never touched, and a single squashed commit so the large
    blobs do not accumulate across releases."""
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
            "Auto-generated by `build_feed.py`. Serves sing-box + tpws-zapret for "
            "the detour panel.\n\n"
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
    _run(["git", "commit", "-q", "-m", commit_msg], cwd=stage)
    _run(["git", "push", "--force", remote, f"{FEED_BRANCH}:{FEED_BRANCH}"],
         cwd=stage, quiet_url=remote)
    shutil.rmtree(stage, ignore_errors=True)

    raw = (f"https://raw.githubusercontent.com/{owner}/{repo}/"
           f"{FEED_BRANCH}/{FEED_ARCH_DIR}")
    print(f"[publish] feed live at: {raw}")
    print(f"[publish] opkg line:    src/gz detour {raw}")
    return raw


def parse_version(v, label):
    if v.startswith("v"):
        v = v[1:]
    parts = v.split(".")
    if not (1 <= len(parts) <= 3) or not all(p.isdigit() for p in parts):
        die(f"--{label} must be a dotted numeric version, got {v!r}")
    return v


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--version", help="sing-box version to build, e.g. 1.13.2")
    ap.add_argument("--tpws-version", help="tpws-zapret (zapret) version to build, e.g. 72.12")
    ap.add_argument("--revision", default=DEFAULT_REVISION,
                    help=f"opkg package revision suffix (default {DEFAULT_REVISION})")
    ap.add_argument("--publish", action="store_true",
                    help="force-push the feed tree to the `feed` branch")
    args = ap.parse_args()

    build_versions = {}
    if args.version:
        build_versions["sing-box"] = f"{parse_version(args.version, 'version')}-{args.revision}"
    if args.tpws_version:
        build_versions["tpws-zapret"] = f"{parse_version(args.tpws_version, 'tpws-version')}-{args.revision}"

    if not build_versions and not os.path.isdir(FEED_OUT):
        die("nothing to build: pass --version (sing-box) and/or --tpws-version")

    label = ", ".join(f"{k} {v}" for k, v in build_versions.items()) or "(re-index only)"
    print(f"=== Building opkg feed: {label} ({ARCH}) ===")
    print(f"Output: {FEED_OUT}")
    write_feed(build_versions, FEED_OUT)

    if args.publish:
        # Derive a commit message from whatever versions are now in the feed.
        _, ipks = build_packages_index(FEED_OUT)
        msg = "feed: " + ", ".join(n[:-4].replace("_" + ARCH, "") for n in ipks)
        publish_feed(msg, FEED_OUT)

    print("\n=== DONE ===")
    if not args.publish:
        print("Publish to the feed branch with: --publish")


if __name__ == "__main__":
    main()
