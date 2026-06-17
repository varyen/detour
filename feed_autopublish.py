#!/usr/bin/env python3
"""Auto-publish the detour opkg feed when an upstream binary has a newer release.

Run by .github/workflows/feed-autopublish.yml on a schedule. For each binary it
finds the latest upstream release, compares it to what the feed currently serves,
and — if anything is newer — rebuilds + republishes the WHOLE feed via
`build_feed.py --fetch-upstream --publish` (which downloads each binary straight
from its upstream release, so no router-backup checkout is needed).

Safety:
  * sing-box is PINNED to the major.minor the feed currently serves (e.g. 1.13.x).
    A new patch (1.13.14) auto-publishes; a new minor (1.14.x) does NOT — it only
    logs a note, because a sing-box minor can break the 1.13.x config schema and
    needs a human (bump the feed once by hand to move the pin forward).
  * tpws (bol-van/zapret) and nfqws2 (bol-van/zapret2) auto-bump to latest.
  * If the feed's Packages can't be read, it refuses to guess and exits non-zero
    (never publishes from an unknown baseline).

Usage:
    python3 feed_autopublish.py            # publish if anything is newer
    python3 feed_autopublish.py --dry-run  # print the decision, do not publish

Env (set by the workflow):
    DETOUR_GH_OWNER / DETOUR_GH_REPO   feed repo (default varyen/detour)
    DETOUR_PUBLISH_TOKEN | GITHUB_TOKEN GitHub token (raises API limit + lets
                                        build_feed push the feed branch)
"""
import argparse
import json
import os
import subprocess
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
OWNER = os.environ.get("DETOUR_GH_OWNER") or "varyen"
REPO = os.environ.get("DETOUR_GH_REPO") or "detour"
FEED_PACKAGES_URL = (
    f"https://raw.githubusercontent.com/{OWNER}/{REPO}/feed/aarch64/Packages"
)

SINGBOX_REPO = "SagerNet/sing-box"
ZAPRET_REPO = "bol-van/zapret"
ZAPRET2_REPO = "bol-van/zapret2"


def _token():
    return (
        os.environ.get("DETOUR_PUBLISH_TOKEN")
        or os.environ.get("FEED_PUBLISH_TOKEN")
        or os.environ.get("GITHUB_TOKEN")
        or ""
    )


def gh_json(url):
    hdrs = {
        "User-Agent": "detour-feed-autopublish",
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
    }
    t = _token()
    if t:
        hdrs["Authorization"] = f"Bearer {t}"
    req = urllib.request.Request(url, headers=hdrs)
    return json.load(urllib.request.urlopen(req, timeout=60))


def vtuple(v):
    """'v1.13.13-1' -> (1, 13, 13). Non-numeric parts collapse to 0."""
    v = v.lstrip("v").split("-")[0]
    out = []
    for p in v.split("."):
        try:
            out.append(int(p))
        except ValueError:
            out.append(0)
    return tuple(out)


def latest_release_tag(repo):
    """Latest non-prerelease tag (v-stripped) for repo."""
    return gh_json(
        f"https://api.github.com/repos/{repo}/releases/latest"
    ).get("tag_name", "").lstrip("v")


def latest_in_minor(repo, major_minor):
    """(best_in_pin, newest_any): newest stable release whose version starts with
    `major_minor` (e.g. '1.13'), and the newest stable overall (to flag a minor
    bump that's available but pinned out)."""
    rels = gh_json(f"https://api.github.com/repos/{repo}/releases?per_page=50")
    cands = []
    newest_any = None
    for r in rels:
        if r.get("prerelease") or r.get("draft"):
            continue
        tag = (r.get("tag_name") or "").lstrip("v")
        if not tag:
            continue
        if newest_any is None or vtuple(tag) > vtuple(newest_any):
            newest_any = tag
        if ".".join(tag.split(".")[:2]) == major_minor:
            cands.append(tag)
    best = max(cands, key=vtuple) if cands else None
    return best, newest_any


def feed_versions():
    """{pkg: version} the feed currently serves, parsed from its Packages index."""
    req = urllib.request.Request(
        FEED_PACKAGES_URL,
        headers={"User-Agent": "detour-feed-autopublish", "Cache-Control": "no-cache"},
    )
    txt = urllib.request.urlopen(req, timeout=60).read().decode()
    out = {}
    pkg = None
    for line in txt.splitlines():
        if line.startswith("Package:"):
            pkg = line.split(":", 1)[1].strip()
        elif line.startswith("Version:") and pkg:
            out[pkg] = line.split(":", 1)[1].strip().split("-")[0]
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dry-run", action="store_true",
                    help="print the decision; do not build/publish")
    args = ap.parse_args()

    try:
        feed = feed_versions()
    except Exception as e:  # noqa: BLE001 — any failure means unknown baseline
        print(f"ERROR: cannot read feed Packages ({FEED_PACKAGES_URL}): {e}",
              file=sys.stderr)
        sys.exit(1)
    for req in ("sing-box", "tpws-zapret", "nfqws2"):
        if req not in feed:
            print(f"ERROR: feed Packages missing {req}; refusing to guess. got {feed}",
                  file=sys.stderr)
            sys.exit(1)

    cur_sb, cur_tpws, cur_nfq = feed["sing-box"], feed["tpws-zapret"], feed["nfqws2"]
    sb_pin = ".".join(cur_sb.split(".")[:2])

    sb_best, sb_any = latest_in_minor(SINGBOX_REPO, sb_pin)
    tpws_latest = latest_release_tag(ZAPRET_REPO)
    nfq_latest = latest_release_tag(ZAPRET2_REPO)

    sb_t = sb_best if (sb_best and vtuple(sb_best) > vtuple(cur_sb)) else cur_sb
    tpws_t = tpws_latest if (tpws_latest and vtuple(tpws_latest) > vtuple(cur_tpws)) else cur_tpws
    nfq_t = nfq_latest if (nfq_latest and vtuple(nfq_latest) > vtuple(cur_nfq)) else cur_nfq

    print(f"sing-box : feed {cur_sb} | pin {sb_pin}.x | latest-in-pin {sb_best} -> {sb_t}")
    if sb_any and vtuple(sb_any) > vtuple(sb_t):
        print(f"  NOTE: sing-box {sb_any} is out upstream but crosses the {sb_pin} "
              f"pin — bump the feed by hand once to move the pin (config schema risk).")
    print(f"tpws     : feed {cur_tpws} | latest {tpws_latest} -> {tpws_t}")
    print(f"nfqws2   : feed {cur_nfq} | latest {nfq_latest} -> {nfq_t}")

    if sb_t == cur_sb and tpws_t == cur_tpws and nfq_t == cur_nfq:
        print("feed is up to date — nothing to publish.")
        return

    cmd = [sys.executable, os.path.join(HERE, "build_feed.py"), "--fetch-upstream",
           "--version", sb_t, "--tpws-version", tpws_t, "--nfqws2-version", nfq_t]
    if not args.dry_run:
        cmd.append("--publish")
    print("RUN:", " ".join(cmd))
    if args.dry_run:
        print("(dry-run — not publishing)")
        return
    subprocess.run(cmd, check=True, cwd=HERE)
    print("feed published.")


if __name__ == "__main__":
    main()
