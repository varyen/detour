#!/usr/bin/env python3
"""Universal bootstrap/sync of the detour stack to any router from routers.local.json.

Usage:
    python3 deploy_router.py [--router NAME] [--full] [--skip-binaries] [--skip-panel-auth]

What it does (idempotent):
    1. sysctl: persist net.mptcp.enabled=0
    2. uhttpd: ensure listen_http binds on 0.0.0.0:8080
    3. opkg feed: add the `detour` feed (serves sing-box 1.13.x), install sing-box
       via opkg; push the bundled tpws-zapret directly (it's in no feed)
    4. /etc/init.d/{sing-box,zapret-tpws}: upload from router_files/*.initd, enable
    5. /etc/firewall.lan_mark_fallback + uci firewall include
    6. /etc/sing-box/* (config.json, settings.json, *.list, profiles/) and /etc/zapret-tpws/*
       (only if --full or files absent on target)
    7. /www/cgi-bin/detour-api, /www/detour/index.html (from router-backup/, always)
    8. /etc/detour.auth (only if absent, or --reset-panel-auth)
    9. /etc/hotplug.d/iface/99-proxy-guard
   10. Restart services
"""
import argparse
import base64
import json
import os
import secrets
import string
import sys
import time

import paramiko

from router_config import load_router, load_global_config, ssh_connect, exec_cmd

HERE = os.path.dirname(os.path.abspath(__file__))
ROUTER_FILES = os.path.join(HERE, "router_files")
BACKUP_HOME = os.path.join(HERE, "router-backup")  # canonical source for live configs

# Default panel user when routers.local.json entry lacks panel_user.
# Panel password MUST be set per-router in routers.local.json (panel_password).
DEFAULT_PANEL_USER = "admin"

# Self-hosted opkg feed serving sing-box (build_feed.py → varyen/detour@feed).
# Keep in sync with build_release.py FEED_LINE and router_files/detour-update.
FEED_NAME = "detour"
FEED_LINE = "src/gz detour https://raw.githubusercontent.com/varyen/detour/feed/aarch64"


def upload(ssh, content, remote, mode="0644"):
    """Push small file via raw channel stdin (paramiko 5.x compat)."""
    if isinstance(content, str):
        content = content.encode("utf-8")
    transport = ssh.get_transport()
    chan = transport.open_session()
    chan.exec_command(f"cat > {remote}")
    chan.sendall(content)
    chan.shutdown_write()
    chan.recv_exit_status()
    chan.close()
    exec_cmd(ssh, f"chmod {mode} {remote}")
    out, _, _ = exec_cmd(ssh, f"wc -c < {remote}")
    return int(out.strip() or 0)


def upload_large(ssh, local_path, remote, mode="0755"):
    """Push large binary via channel stdin (no SFTP on busybox)."""
    sz = os.path.getsize(local_path)
    transport = ssh.get_transport()
    chan = transport.open_session()
    chan.exec_command(f"cat > {remote}")
    with open(local_path, "rb") as f:
        while True:
            chunk = f.read(65536)
            if not chunk:
                break
            chan.sendall(chunk)
    chan.shutdown_write()
    chan.recv_exit_status()
    chan.close()
    exec_cmd(ssh, f"chmod {mode} {remote}")
    out, _, _ = exec_cmd(ssh, f"wc -c < {remote}")
    return int(out.strip() or 0), sz


def remote_exists(ssh, path):
    out, _, _ = exec_cmd(ssh, f"test -e {path} && echo yes || echo no")
    return out.strip() == "yes"


def remote_md5(ssh, path):
    out, _, rc = exec_cmd(ssh, f"md5sum {path} 2>/dev/null | cut -d' ' -f1")
    return out.strip() if rc == 0 else ""


def local_md5(path):
    import hashlib
    with open(path, "rb") as f:
        return hashlib.md5(f.read()).hexdigest()


def step(msg):
    print(f"\n[*] {msg}")


# ---- individual steps ----

def step_mptcp(ssh):
    step("Disabling broken MPTCP")
    exec_cmd(ssh, "sysctl -w net.mptcp.enabled=0 2>/dev/null")
    upload(ssh, "net.mptcp.enabled=0\n", "/etc/sysctl.d/99-mptcp.conf")
    out, _, _ = exec_cmd(ssh, "sysctl net.mptcp.enabled 2>/dev/null")
    print(f"  {out.strip()}")


def step_base64_shim(ssh):
    """Install openssl-backed base64 shim if busybox lacks the applet."""
    out, _, _ = exec_cmd(ssh, "command -v base64 || true")
    if out.strip():
        return
    step("Installing /usr/bin/base64 shim (busybox lacks the applet)")
    with open(os.path.join(ROUTER_FILES, "base64-shim.sh"), "rb") as f:
        n = upload(ssh, f.read(), "/usr/bin/base64", "0755")
    print(f"  /usr/bin/base64: {n} bytes")
    out, _, _ = exec_cmd(ssh, "echo 'aGVsbG8=' | base64 -d")
    print(f"  selftest: {out.strip()!r} (expect 'hello')")


def step_uhttpd(ssh):
    step("Ensuring uhttpd binds on 0.0.0.0:8080")
    out, _, _ = exec_cmd(ssh, "uci get uhttpd.main.listen_http 2>/dev/null")
    current = out.strip()
    print(f"  current: {current}")
    if "0.0.0.0:8080" in current:
        print("  already 0.0.0.0:8080 — keeping")
        return
    exec_cmd(
        ssh,
        "uci -q delete uhttpd.main.listen_http; "
        "uci add_list uhttpd.main.listen_http='0.0.0.0:8080'; "
        "uci add_list uhttpd.main.listen_http='[::]:8080'; "
        "uci commit uhttpd",
    )
    out, _, _ = exec_cmd(ssh, "uci get uhttpd.main.listen_http")
    print(f"  new: {out.strip()}")
    exec_cmd(ssh, "/etc/init.d/uhttpd restart 2>&1 >/dev/null")
    time.sleep(1)


def step_feed(ssh):
    step("Configuring opkg feed (sing-box)")
    # Add our feed line idempotently, then refresh the package index so
    # `opkg install sing-box` resolves to our 1.13.x (the distro feed's 1.8.10
    # loses on version comparison). OpenWrt path is /etc/opkg/customfeeds.conf.
    exec_cmd(
        ssh,
        "touch /etc/opkg/customfeeds.conf; "
        f"grep -qs '^src/gz {FEED_NAME} ' /etc/opkg/customfeeds.conf || "
        f"echo '{FEED_LINE}' >> /etc/opkg/customfeeds.conf",
    )
    out, _, _ = exec_cmd(ssh, "opkg update 2>&1 | tail -4", timeout=120)
    print("  opkg update:\n    " + "\n    ".join(l for l in out.splitlines() if l.strip()))


def step_binaries(ssh, force=False):
    step("Installing sing-box (opkg feed) + tpws-zapret (bundled)")
    # tpws-zapret is in no opkg feed → push the bundled musl-static binary directly.
    local = os.path.join(BACKUP_HOME, "usr", "bin", "tpws-zapret")
    if os.path.exists(local):
        if not force and remote_exists(ssh, "/usr/bin/tpws-zapret") \
                and remote_md5(ssh, "/usr/bin/tpws-zapret") == local_md5(local):
            print("  /usr/bin/tpws-zapret: unchanged")
        else:
            print(f"  uploading /usr/bin/tpws-zapret ({os.path.getsize(local)} bytes) ...")
            got, sz = upload_large(ssh, local, "/usr/bin/tpws-zapret")
            print(f"    -> {got}/{sz} bytes")
            assert got == sz, "size mismatch for /usr/bin/tpws-zapret"
    else:
        print(f"  SKIP tpws: {local} (not in backup)")

    # sing-box comes from our opkg feed. --force-overwrite takes over any
    # pre-existing UNOWNED /usr/bin/sing-box (older direct deploys); afterwards
    # opkg owns it and `opkg upgrade sing-box` is clean.
    out, _, _ = exec_cmd(ssh, "opkg install --force-overwrite sing-box 2>&1 | tail -5", timeout=240)
    print("  opkg install sing-box:\n    " + "\n    ".join(l for l in out.splitlines() if l.strip()))

    # Safety net: if the feed was unreachable and we still have the bundled
    # binary locally, push it directly so the router isn't left without sing-box.
    if not remote_exists(ssh, "/usr/bin/sing-box"):
        sb_local = os.path.join(BACKUP_HOME, "usr", "bin", "sing-box")
        if os.path.exists(sb_local):
            print("  feed install failed — falling back to direct upload of bundled sing-box")
            got, sz = upload_large(ssh, sb_local, "/usr/bin/sing-box")
            print(f"    -> {got}/{sz} bytes")
        else:
            print("  WARNING: sing-box not installed and no local fallback binary")

    sbver, _, _ = exec_cmd(ssh, "opkg list-installed sing-box 2>/dev/null | awk '{print $3}' | head -1")
    print(f"  sing-box: {(sbver or '').strip() or '(not opkg-owned)'}")


def step_initd(ssh):
    step("Uploading init.d scripts (firewall-agnostic)")
    pairs = [
        (os.path.join(ROUTER_FILES, "sing-box.initd"), "/etc/init.d/sing-box"),
        (os.path.join(ROUTER_FILES, "zapret-tpws.initd"), "/etc/init.d/zapret-tpws"),
    ]
    for local, remote in pairs:
        with open(local, "rb") as f:
            content = f.read()
        n = upload(ssh, content, remote, "0755")
        print(f"  {remote}: {n} bytes")


def step_lan_mark_fallback(ssh):
    step("Installing /etc/firewall.lan_mark_fallback + uci include")
    with open(os.path.join(ROUTER_FILES, "firewall.lan_mark_fallback"), "rb") as f:
        upload(ssh, f.read(), "/etc/firewall.lan_mark_fallback", "0755")

    out, _, _ = exec_cmd(ssh, "uci show firewall | grep lan_mark_fallback")
    if "lan_mark_fallback" in out:
        print("  uci include already present")
    else:
        # fw3 + fw4 both accept this format
        exec_cmd(
            ssh,
            "uci add firewall include; "
            "uci rename firewall.@include[-1]=lan_mark_fallback; "
            "uci set firewall.lan_mark_fallback.name='lan_mark_fallback'; "
            "uci set firewall.lan_mark_fallback.type='script'; "
            "uci set firewall.lan_mark_fallback.path='/etc/firewall.lan_mark_fallback'; "
            "uci commit firewall",
        )
        print("  uci include added")
    # apply immediately
    exec_cmd(ssh, "/etc/firewall.lan_mark_fallback 2>&1")


def step_singbox_configs(ssh, force=False):
    step("Uploading sing-box configs (if missing or --full)")
    files = [
        ("etc/sing-box/config.json", "/etc/sing-box/config.json"),
        ("etc/sing-box/settings.json", "/etc/sing-box/settings.json"),
        ("etc/sing-box/proxy-domains.list", "/etc/sing-box/proxy-domains.list"),
        ("etc/sing-box/whitelist-domains.list", "/etc/sing-box/whitelist-domains.list"),
    ]
    exec_cmd(ssh, "mkdir -p /etc/sing-box/profiles")
    for local_rel, remote in files:
        local = os.path.join(BACKUP_HOME, local_rel)
        if not os.path.exists(local):
            print(f"  SKIP: {local} (not in backup)")
            continue
        if not force and remote_exists(ssh, remote):
            print(f"  {remote}: already present, keeping")
            continue
        with open(local, "rb") as f:
            content = f.read()
        n = upload(ssh, content, remote, "0644")
        print(f"  {remote}: {n} bytes")
    # profiles dir — upload all
    profiles_dir = os.path.join(BACKUP_HOME, "etc", "sing-box", "profiles")
    if os.path.isdir(profiles_dir):
        for fname in sorted(os.listdir(profiles_dir)):
            if not fname.endswith(".json"):
                continue
            local = os.path.join(profiles_dir, fname)
            remote = f"/etc/sing-box/profiles/{fname}"
            if not force and remote_exists(ssh, remote):
                continue
            with open(local, "rb") as f:
                content = f.read()
            upload(ssh, content, remote, "0644")
        print(f"  profiles: {len([f for f in os.listdir(profiles_dir) if f.endswith('.json')])} files synced")


def step_zapret_configs(ssh, force=False):
    step("Uploading zapret-tpws configs")
    exec_cmd(ssh, "mkdir -p /etc/zapret-tpws")
    pairs = [
        ("etc/zapret-tpws.conf", "/etc/zapret-tpws.conf"),
        ("etc/zapret-tpws/domains.list", "/etc/zapret-tpws/domains.list"),
    ]
    for local_rel, remote in pairs:
        local = os.path.join(BACKUP_HOME, local_rel)
        if not os.path.exists(local):
            print(f"  SKIP: {local} (not in backup)")
            continue
        if not force and remote_exists(ssh, remote):
            print(f"  {remote}: already present, keeping")
            continue
        with open(local, "rb") as f:
            content = f.read()
        n = upload(ssh, content, remote, "0644")
        print(f"  {remote}: {n} bytes")


def step_panel(ssh):
    step("Deploying panel CGI + HTML")
    exec_cmd(ssh, "mkdir -p /www/cgi-bin /www/detour /tmp/detour-sessions")
    cgi = os.path.join(BACKUP_HOME, "www", "cgi-bin", "detour-api")
    html = os.path.join(BACKUP_HOME, "www", "detour", "index.html")
    with open(cgi, "rb") as f:
        n = upload(ssh, f.read(), "/www/cgi-bin/detour-api", "0755")
    print(f"  /www/cgi-bin/detour-api: {n} bytes")
    with open(html, "rb") as f:
        n = upload(ssh, f.read(), "/www/detour/index.html", "0644")
    print(f"  /www/detour/index.html: {n} bytes")


def step_auth(ssh, cfg, reset=False):
    step("Setting up panel auth")
    panel_user = cfg.get("panel_user", DEFAULT_PANEL_USER)
    panel_pass = cfg.get("panel_password") or "(set panel_password in routers.local.json)"
    if not reset and remote_exists(ssh, "/etc/detour.auth"):
        out, _, _ = exec_cmd(ssh, "head -1 /etc/detour.auth | cut -d: -f1")
        existing_user = out.strip()
        if existing_user == panel_user:
            print(f"  /etc/detour.auth exists (user={existing_user}), keeping")
            return
        print(f"  /etc/detour.auth user mismatch ({existing_user!r} != {panel_user!r}); rewriting")
    # Generate salt locally (BusyBox `dd if=/dev/urandom` can return empty,
    # and openssl with empty salt outputs literal "<NULL>").
    salt = "".join(secrets.choice(string.ascii_letters + string.digits) for _ in range(16))
    transport = ssh.get_transport()
    chan = transport.open_session()
    chan.exec_command(f"openssl passwd -6 -salt '{salt}' -stdin")
    chan.sendall(panel_pass.encode("utf-8"))
    chan.shutdown_write()
    h = b""
    while True:
        buf = chan.recv(4096)
        if not buf:
            break
        h += buf
    chan.recv_exit_status()
    chan.close()
    h = h.decode("utf-8").strip()
    if not h.startswith("$6$"):
        print(f"  ERROR generating hash: {h}")
        return
    upload(ssh, f"{panel_user}:{h}\n", "/etc/detour.auth", "0600")
    # Invalidate any existing sessions tied to the previous user.
    exec_cmd(ssh, "rm -f /tmp/detour-sessions/* 2>/dev/null")
    print(f"  auth set: user={panel_user}, pass={panel_pass}")


def step_updater(ssh, cfg, global_cfg):
    """Install the self-update infrastructure on the router.

    Components:
        /usr/sbin/detour-update           — the updater script
        /etc/detour/release.usign.pub     — pinned usign public key
        /etc/detour/update.conf           — GH owner/repo/token
        /etc/detour/version               — current version marker
        /etc/crontabs/root entry               — auto-check every 6 hours
    """
    step("Installing self-update infrastructure")

    exec_cmd(ssh, "mkdir -p /etc/detour /var/state")

    # 1. Updater script
    with open(os.path.join(ROUTER_FILES, "detour-update"), "rb") as f:
        n = upload(ssh, f.read(), "/usr/sbin/detour-update", "0755")
    print(f"  /usr/sbin/detour-update: {n} bytes")

    # 2. usign public key (used by detour-update + release-install.sh).
    # Installed in two locations:
    #   - /etc/detour/release.usign.pub (pinned single-file fallback)
    #   - /etc/opkg/keys/<keynum_hex>        (standard OpenWrt keyring)
    # We also clean up the legacy ECDSA key file if present.
    pub_key_path = os.path.join(HERE, "keys", "release.usign.pub")
    if os.path.isfile(pub_key_path):
        with open(pub_key_path, "rb") as f:
            pub_bytes = f.read()
        # Derive keyring filename from the key fingerprint to stay consistent
        # with build_release.py (it pins the same path inside the .ipk).
        from usign_compat import load_public_key
        keynum, _ = load_public_key(pub_key_path)
        keyring_path = f"/etc/opkg/keys/{keynum.hex()}"
        exec_cmd(ssh, "mkdir -p /etc/opkg/keys")
        upload(ssh, pub_bytes, "/etc/detour/release.usign.pub", "0644")
        upload(ssh, pub_bytes, keyring_path, "0644")
        print(f"  /etc/detour/release.usign.pub: {len(pub_bytes)} bytes")
        print(f"  {keyring_path}: {len(pub_bytes)} bytes (opkg keyring)")
        # Sweep legacy ECDSA key if it's still lying around — detour-update
        # no longer reads it and leaving it would just confuse debugging.
        exec_cmd(ssh, "rm -f /etc/detour/release.pub")
    else:
        print(f"  WARN: {pub_key_path} missing — updates will reject signature")

    # 3. update.conf (gh owner/repo/token from global_cfg if available)
    gh = (global_cfg or {}).get("github", {}) if global_cfg else {}
    gh_owner = gh.get("owner", "")
    gh_repo = gh.get("repo", "")
    gh_token = gh.get("token", "")
    conf_lines = [
        "# Auto-generated by deploy_router.py. Token-grade credentials — chmod 0600.",
        f"GH_OWNER={gh_owner}",
        f"GH_REPO={gh_repo}",
        f"GH_TOKEN={gh_token}",
        "",
    ]
    upload(ssh, "\n".join(conf_lines), "/etc/detour/update.conf", "0600")
    print(f"  /etc/detour/update.conf: owner={gh_owner!r} repo={gh_repo!r} "
          f"token={'set' if gh_token else 'EMPTY'}")

    # 4. Version file (read from VERSION if present, else 0.0.0 baseline)
    version_path = os.path.join(HERE, "VERSION")
    cur_version = "0.0.0"
    if os.path.isfile(version_path):
        cur_version = open(version_path).read().strip() or "0.0.0"
    # Only seed if absent on router; never downgrade an existing version marker.
    if not remote_exists(ssh, "/etc/detour/version"):
        upload(ssh, cur_version + "\n", "/etc/detour/version", "0644")
        print(f"  /etc/detour/version: {cur_version} (seeded)")
    else:
        out, _, _ = exec_cmd(ssh, "cat /etc/detour/version")
        print(f"  /etc/detour/version: {out.strip()} (kept)")

    # 5. Cron: check every 6h
    cron_line = "0 */6 * * * /usr/sbin/detour-update check >/var/log/detour-update.log 2>&1"
    exec_cmd(
        ssh,
        # Idempotently install one cron line for the updater check.
        "( crontab -l 2>/dev/null | grep -v 'detour-update' ; "
        f"echo '{cron_line}' ) | crontab -",
    )
    exec_cmd(ssh, "/etc/init.d/cron enable >/dev/null 2>&1; /etc/init.d/cron restart >/dev/null 2>&1")
    out, _, _ = exec_cmd(ssh, "crontab -l 2>/dev/null | grep detour-update")
    print(f"  cron: {out.strip() or 'NOT INSTALLED'}")

    # 6. Subscription auto-refresh helper (Lua) + multi-subscription storage dir.
    sub_refresh_local = os.path.join(ROUTER_FILES, "subscription-refresh")
    if os.path.isfile(sub_refresh_local):
        with open(sub_refresh_local, "rb") as f:
            n = upload(ssh, f.read(), "/usr/sbin/subscription-refresh", "0755")
        print(f"  /usr/sbin/subscription-refresh: {n} bytes")
        # Per-group subscription metadata lives in /etc/detour/subscriptions/<id>.json
        # (v2 schema). The Lua helper still falls back to the legacy single-file
        # /etc/detour/subscription.json when this directory is empty.
        exec_cmd(ssh, "mkdir -p /etc/detour/subscriptions && chmod 0700 /etc/detour/subscriptions")
        # Hourly tick: the helper itself decides which subscriptions are due based
        # on their per-subscription `interval_hours` (default 24h) + autoupdate flag.
        sub_cron = "17 * * * * /usr/sbin/subscription-refresh >/var/log/subscription-refresh.log 2>&1"
        exec_cmd(
            ssh,
            "( crontab -l 2>/dev/null | grep -v 'subscription-refresh' ; "
            f"echo '{sub_cron}' ) | crontab -",
        )
        out, _, _ = exec_cmd(ssh, "crontab -l 2>/dev/null | grep subscription-refresh")
        print(f"  cron: {out.strip() or 'NOT INSTALLED'}")

    # 7. VPN keep-alive probe (records active-VPN reachability for the panel).
    keepalive_local = os.path.join(ROUTER_FILES, "vpn-keepalive")
    if os.path.isfile(keepalive_local):
        with open(keepalive_local, "rb") as f:
            n = upload(ssh, f.read(), "/usr/sbin/vpn-keepalive", "0755")
        print(f"  /usr/sbin/vpn-keepalive: {n} bytes")
        # Probe every 5 minutes. Record-only — never restarts/fails over.
        ka_cron = "*/5 * * * * /usr/sbin/vpn-keepalive >/dev/null 2>&1"
        exec_cmd(
            ssh,
            "( crontab -l 2>/dev/null | grep -v 'vpn-keepalive' ; "
            f"echo '{ka_cron}' ) | crontab -",
        )
        out, _, _ = exec_cmd(ssh, "crontab -l 2>/dev/null | grep vpn-keepalive")
        print(f"  cron: {out.strip() or 'NOT INSTALLED'}")


def step_hotplug_guard(ssh):
    step("Installing hotplug guard")
    local = os.path.join(BACKUP_HOME, "etc", "hotplug.d", "iface", "99-proxy-guard")
    if not os.path.exists(local):
        print(f"  SKIP: {local} missing")
        return
    exec_cmd(ssh, "mkdir -p /etc/hotplug.d/iface")
    with open(local, "rb") as f:
        n = upload(ssh, f.read(), "/etc/hotplug.d/iface/99-proxy-guard", "0755")
    print(f"  /etc/hotplug.d/iface/99-proxy-guard: {n} bytes")


def step_enable_services(ssh):
    step("Enabling + starting services")
    for svc in ["zapret-tpws", "sing-box"]:
        exec_cmd(ssh, f"/etc/init.d/{svc} enable 2>&1")
    # zapret first (must run before sing-box for iptables ordering)
    out, _, _ = exec_cmd(ssh, "/etc/init.d/zapret-tpws restart 2>&1", timeout=60)
    print(f"  zapret-tpws restart: {out[:200]}")
    time.sleep(2)
    out, _, _ = exec_cmd(ssh, "/etc/init.d/sing-box restart 2>&1", timeout=60)
    print(f"  sing-box restart: {out[:200]}")
    time.sleep(3)
    out, _, _ = exec_cmd(ssh, "ps w | grep -E '[s]ing-box|[t]pws-zapret' | awk '{print $5\" \"$6}'")
    print(f"  procs:\n    " + "\n    ".join(l for l in out.splitlines() if l.strip()))


def step_verify(ssh, host):
    step("Verifying")
    checks = [
        ("netstat -tlnup 2>/dev/null | grep -E ':(8080|12345|1081)'", "listening ports"),
        ("ipset list singbox_domains 2>/dev/null | head -8", "ipset singbox_domains"),
        ("ipset list zapret_domains 2>/dev/null | head -8", "ipset zapret_domains"),
        ("iptables -t nat -L PREROUTING -n 2>/dev/null | head -8", "nat PREROUTING"),
        (
            f"curl -s -o /dev/null -w 'HTTP %{{http_code}}' "
            f"http://127.0.0.1:8080/cgi-bin/detour-api?action=status",
            "panel auth check (expect 401)",
        ),
    ]
    for cmd, label in checks:
        out, _, _ = exec_cmd(ssh, cmd)
        print(f"  [{label}]")
        for l in out.splitlines()[:10]:
            print(f"    {l}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--router", "-r", default=os.environ.get("ROUTER"))
    ap.add_argument("--full", action="store_true",
                    help="overwrite all configs (default: keep existing configs)")
    ap.add_argument("--skip-binaries", action="store_true")
    ap.add_argument("--reset-panel-auth", action="store_true")
    args = ap.parse_args()

    cfg = load_router(args.router)
    global_cfg = load_global_config()
    print(f"=== Deploying to {cfg['name']} ({cfg['host']}) — {cfg.get('label','')} ===")
    ssh = ssh_connect(cfg)

    step_mptcp(ssh)
    step_base64_shim(ssh)
    step_uhttpd(ssh)
    step_feed(ssh)
    if not args.skip_binaries:
        step_binaries(ssh, force=args.full)
    step_initd(ssh)
    step_lan_mark_fallback(ssh)
    step_singbox_configs(ssh, force=args.full)
    step_zapret_configs(ssh, force=args.full)
    step_panel(ssh)
    step_auth(ssh, cfg, reset=args.reset_panel_auth)
    step_updater(ssh, cfg, global_cfg)
    step_hotplug_guard(ssh)
    step_enable_services(ssh)
    step_verify(ssh, cfg["host"])

    panel_user = cfg.get("panel_user", DEFAULT_PANEL_USER)
    panel_pass = cfg.get("panel_password") or "(set panel_password in routers.local.json)"
    print("\n=== DONE ===")
    print(f"Panel: http://{cfg['host']}:8080/detour/")
    print(f"Login: {panel_user}")
    print(f"Pass:  {panel_pass}")
    ssh.close()


if __name__ == "__main__":
    main()
