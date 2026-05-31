#!/usr/bin/env python3
"""Fetch mipsel (soft-float) binaries for the Keenetic KN-1810 (MT7621) port.

KN-1810 = MediaTek MT7621AT, dual-core MIPS 1004Kc, little-endian, NO FPU →
ABI = mipsel **soft-float**. Both binaries below must match that ABI or they
fail at load ("Error relocating" / SIGILL) on the device.

  * sing-box  — official release asset `linux-mipsle-softfloat` (Go, GOMIPS=softfloat)
  * tpws      — from bol-van/zapret release, dir `binaries/mips32r1-lsb` (LE, soft-float baseline)

Output: keenetic/bins/{sing-box,tpws-zapret}  (gitignored; the script is the source of truth)

Run:  python keenetic/fetch-bins.py [--singbox-version 1.13.2]
"""
import argparse
import io
import json
import os
import sys
import tarfile
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "bins")

SINGBOX_REPO = "SagerNet/sing-box"
ZAPRET_REPO = "bol-van/zapret"
# MT7621 is soft-float little-endian. sing-box names this `linux-mipsle-softfloat`.
SINGBOX_ASSET_SUBSTR = "linux-mipsle-softfloat"
# zapret ships static binaries per arch under binaries/linux-<arch>/. MT7621 is
# standard MIPS little-endian → linux-mipsel. (linux-mips = big-endian, linux-lexra
# = old Realtek MIPS — both WRONG for MT7621.)
ZAPRET_TPWS_PATHS = ("binaries/linux-mipsel/tpws",)


def gh_json(url):
    req = urllib.request.Request(url, headers={"User-Agent": "detour-keenetic"})
    # optional token (raises rate limit) from routers.local.json
    cfg_path = os.path.join(os.path.dirname(HERE), "routers.local.json")
    if os.path.isfile(cfg_path):
        tok = (json.load(open(cfg_path, encoding="utf-8")).get("github") or {})
        tok = tok.get("token") or tok.get("publish_token")
        if tok:
            req.add_header("Authorization", f"Bearer {tok}")
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def download(url):
    req = urllib.request.Request(url, headers={"User-Agent": "detour-keenetic"})
    with urllib.request.urlopen(req, timeout=300) as r:
        return r.read()


def fetch_singbox(version):
    dst = os.path.join(OUT, "sing-box")
    if os.path.isfile(dst):
        print(f"[sing-box] already present: {dst} ({os.path.getsize(dst)} B) — skip")
        return
    rel = gh_json(f"https://api.github.com/repos/{SINGBOX_REPO}/releases/tags/v{version}")
    asset = next((a for a in rel["assets"]
                  if SINGBOX_ASSET_SUBSTR in a["name"] and a["name"].endswith(".tar.gz")), None)
    if not asset:
        names = [a["name"] for a in rel["assets"]]
        sys.exit(f"sing-box v{version}: no {SINGBOX_ASSET_SUBSTR} asset. have: {names}")
    print(f"[sing-box] {asset['name']} ({asset['size']} B)")
    blob = download(asset["browser_download_url"])
    tf = tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz")
    member = next(m for m in tf.getmembers() if m.name.endswith("/sing-box") or m.name == "sing-box")
    data = tf.extractfile(member).read()
    dst = os.path.join(OUT, "sing-box")
    open(dst, "wb").write(data)
    os.chmod(dst, 0o755)
    print(f"[sing-box] -> {dst} ({len(data)} B)")


def fetch_tpws():
    rel = gh_json(f"https://api.github.com/repos/{ZAPRET_REPO}/releases/latest")
    asset = next((a for a in rel["assets"] if a["name"].endswith((".tar.gz", ".tgz"))), None)
    if not asset:
        sys.exit(f"zapret: no tarball asset. have: {[a['name'] for a in rel['assets']]}")
    print(f"[tpws] zapret {rel['tag_name']} -> {asset['name']} ({asset['size']} B)")
    blob = download(asset["browser_download_url"])
    tf = tarfile.open(fileobj=io.BytesIO(blob), mode="r:gz")
    names = tf.getnames()
    member = None
    for want in ZAPRET_TPWS_PATHS:
        member = next((m for m in tf.getmembers() if m.name.endswith(want)), None)
        if member:
            break
    if not member:
        cand = [n for n in names if n.endswith("/tpws")]
        sys.exit(f"tpws: none of {ZAPRET_TPWS_PATHS} in tarball. tpws candidates: {cand}")
    data = tf.extractfile(member).read()
    dst = os.path.join(OUT, "tpws-zapret")
    open(dst, "wb").write(data)
    os.chmod(dst, 0o755)
    print(f"[tpws] {member.name} -> {dst} ({len(data)} B)")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--singbox-version", default="1.13.2")
    ap.add_argument("--skip-tpws", action="store_true")
    args = ap.parse_args()
    os.makedirs(OUT, exist_ok=True)
    fetch_singbox(args.singbox_version)
    if not args.skip_tpws:
        fetch_tpws()
    print("\nNOTE: ABI not yet verified on hardware — run `file` / `readelf -h` and a smoke "
          "test on the KN-1810 (watch for 'Error relocating' = wrong float ABI).")


if __name__ == "__main__":
    main()
