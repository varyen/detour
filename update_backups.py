#!/usr/bin/env python3
"""Fetch current configs and code from a router to router-backup/<name>/.

Usage: python3 update_backups.py [--router NAME]
"""

import paramiko
import os
import sys
import argparse

from router_config import load_router

_ARGP = argparse.ArgumentParser()
_ARGP.add_argument("--router", "-r", default=os.environ.get("ROUTER"))
_ARGS = _ARGP.parse_args()

_CFG = load_router(_ARGS.router)
ROUTER_HOST = _CFG["host"]
ROUTER_PORT = _CFG.get("port", 22)
ROUTER_USER = _CFG["user"]
ROUTER_PASS = _CFG["password"]
ROUTER_NAME = _CFG["name"]

_HERE = os.path.dirname(os.path.abspath(__file__))
# Per-router backup directory; default (home) keeps legacy path router-backup/.
BACKUP_DIR = (
    os.path.join(_HERE, "router-backup")
    if ROUTER_NAME == "home"
    else os.path.join(_HERE, "router-backup", ROUTER_NAME)
)

# Files to backup: (remote_path, local_relative_path)
FILES_TO_BACKUP = [
    # sing-box
    ("/etc/sing-box/config.json", "etc/sing-box/config.json"),
    ("/etc/sing-box/proxy-domains.list", "etc/sing-box/proxy-domains.list"),
    ("/etc/sing-box/settings.json", "etc/sing-box/settings.json"),
    ("/etc/sing-box/whitelist-domains.list", "etc/sing-box/whitelist-domains.list"),
    ("/etc/init.d/sing-box", "etc/init.d/sing-box"),
    ("/etc/config/sing-box", "etc/config/sing-box"),
    # zapret-tpws
    ("/etc/zapret-tpws.conf", "etc/zapret-tpws.conf"),
    ("/etc/zapret-tpws/domains.list", "etc/zapret-tpws/domains.list"),
    ("/etc/init.d/zapret-tpws", "etc/init.d/zapret-tpws"),
    # system
    ("/etc/sysctl.d/99-mptcp.conf", "etc/sysctl.d/99-mptcp.conf"),
    ("/etc/passwd", "etc/passwd"),
    ("/etc/shadow", "etc/shadow"),
    ("/etc/group", "etc/group"),
    ("/etc/hosts", "etc/hosts"),
    ("/etc/profile", "etc/profile"),
    ("/etc/rc.local", "etc/rc.local"),
    ("/etc/opkg.conf", "etc/opkg.conf"),
    ("/etc/dnsmasq.conf", "etc/dnsmasq.conf"),
    ("/etc/sysupgrade.conf", "etc/sysupgrade.conf"),
    # config
    ("/etc/config/dhcp", "etc/config/dhcp"),
    ("/etc/config/dropbear", "etc/config/dropbear"),
    ("/etc/config/firewall", "etc/config/firewall"),
    ("/etc/config/luci", "etc/config/luci"),
    ("/etc/config/network", "etc/config/network"),
    ("/etc/config/system", "etc/config/system"),
    ("/etc/config/uhttpd", "etc/config/uhttpd"),
    ("/etc/config/wireguard", "etc/config/wireguard"),
    ("/etc/config/wireless", "etc/config/wireless"),
    # dropbear keys
    ("/etc/dropbear/dropbear_ecdsa_host_key", "etc/dropbear/dropbear_ecdsa_host_key"),
    ("/etc/dropbear/dropbear_ed25519_host_key", "etc/dropbear/dropbear_ed25519_host_key"),
    ("/etc/dropbear/dropbear_rsa_host_key", "etc/dropbear/dropbear_rsa_host_key"),
    # web panel
    ("/www/cgi-bin/detour-api", "www/cgi-bin/detour-api"),
    ("/www/detour/index.html", "www/detour/index.html"),
    # auth
    ("/etc/detour.auth", "etc/detour.auth"),
    # nginx / HTTPS
    ("/etc/nginx/conf.d/detour.conf", "etc/nginx/conf.d/detour.conf"),
    ("/etc/nginx/ssl/router.example.com/fullchain.pem", "etc/nginx/ssl/router.example.com/fullchain.pem"),
    ("/etc/nginx/ssl/router.example.com/privkey.pem", "etc/nginx/ssl/router.example.com/privkey.pem"),
    # acme renewal
    ("/root/acme-renew.sh", "root/acme-renew.sh"),
    # hotplug guards
    ("/etc/hotplug.d/iface/99-proxy-guard", "etc/hotplug.d/iface/99-proxy-guard"),
    # firewall include: keep LAN routable when proxies are stopped
    ("/etc/firewall.lan_mark_fallback", "etc/firewall.lan_mark_fallback"),
    # lan-proxy (router-side reverse proxy stack)
    ("/etc/lan-proxy/hosts.json", "etc/lan-proxy/hosts.json"),
    ("/etc/lan-proxy/lan-proxy.auth", "etc/lan-proxy/lan-proxy.auth"),
    ("/etc/lan-proxy/api-token", "etc/lan-proxy/api-token"),
    ("/www/cgi-bin/lan-proxy-api", "www/cgi-bin/lan-proxy-api"),
    ("/www/lan-proxy/index.html", "www/lan-proxy/index.html"),
    ("/usr/sbin/lan-proxy-render", "usr/sbin/lan-proxy-render"),
    ("/etc/nginx/conf.d/lan-proxy-routes.conf", "etc/nginx/conf.d/lan-proxy-routes.conf"),
]

# Large binary files - check if changed before downloading
BINARY_FILES = [
    ("/usr/bin/sing-box", "usr/bin/sing-box"),
    ("/usr/bin/tpws-zapret", "usr/bin/tpws-zapret"),
]

# Snapshot commands: (command, local_relative_path)
SNAPSHOT_COMMANDS = [
    ("iptables-save 2>/dev/null", "root/iptables-backup.rules"),
    ("nft list ruleset 2>/dev/null", "root/nftables-backup.rules"),
]


def ssh_connect():
    client = paramiko.SSHClient()
    client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    client.connect(ROUTER_HOST, port=ROUTER_PORT, username=ROUTER_USER, password=ROUTER_PASS, timeout=10)
    return client


def exec_cmd(client, cmd):
    stdin, stdout, stderr = client.exec_command(cmd, timeout=30)
    out = stdout.read()
    err = stderr.read()
    rc = stdout.channel.recv_exit_status()
    return out, err, rc


def fetch_file(client, remote_path, local_path):
    """Fetch a file from router via base64 over SSH"""
    out, err, rc = exec_cmd(client, f"base64 '{remote_path}' 2>/dev/null")
    if rc != 0 or not out:
        return False, f"File not found or empty: {remote_path}"
    
    import base64
    try:
        data = base64.b64decode(out)
    except Exception as e:
        return False, f"base64 decode error: {e}"
    
    os.makedirs(os.path.dirname(local_path), exist_ok=True)
    with open(local_path, "wb") as f:
        f.write(data)
    return True, f"{len(data)} bytes"


def fetch_large_file(client, remote_path, local_path):
    """Fetch a large binary file using SSH stdin pipe"""
    # Check remote file size and md5
    out, err, rc = exec_cmd(client, f"wc -c < '{remote_path}' 2>/dev/null")
    if rc != 0:
        return False, f"File not found: {remote_path}"
    remote_size = int(out.strip())
    
    # Check local file
    if os.path.exists(local_path):
        local_size = os.path.getsize(local_path)
        if local_size == remote_size:
            # Check md5 to be sure
            out_md5, _, _ = exec_cmd(client, f"md5sum '{remote_path}' 2>/dev/null | cut -d' ' -f1")
            remote_md5 = out_md5.strip().decode() if out_md5 else ""
            
            import hashlib
            with open(local_path, "rb") as f:
                local_md5 = hashlib.md5(f.read()).hexdigest()
            
            if remote_md5 == local_md5:
                return True, f"unchanged ({remote_size} bytes)"
    
    # Download via cat through SSH channel
    print(f"    Downloading {remote_size} bytes...")
    transport = client.get_transport()
    channel = transport.open_session()
    channel.exec_command(f"cat '{remote_path}'")
    
    os.makedirs(os.path.dirname(local_path), exist_ok=True)
    data = b""
    while True:
        chunk = channel.recv(65536)
        if not chunk:
            break
        data += chunk
    channel.close()
    
    with open(local_path, "wb") as f:
        f.write(data)
    return True, f"{len(data)} bytes (downloaded)"


def main():
    print(f"Router: {ROUTER_NAME} ({ROUTER_HOST}) -> {BACKUP_DIR}")
    print(f"Connecting to {ROUTER_HOST}...")
    client = ssh_connect()
    print("Connected!\n")
    
    # Fetch text/config files
    print("=== Fetching config files ===")
    ok_count = 0
    fail_count = 0
    for remote, local_rel in FILES_TO_BACKUP:
        local = os.path.join(BACKUP_DIR, local_rel)
        success, msg = fetch_file(client, remote, local)
        status = "OK" if success else "FAIL"
        if success:
            ok_count += 1
        else:
            fail_count += 1
        print(f"  [{status}] {remote} -> {local_rel} ({msg})")
    
    # Fetch sing-box profiles (dynamic directory)
    print("\n=== Fetching sing-box profiles ===")
    profiles_dir = os.path.join(BACKUP_DIR, "etc", "sing-box", "profiles")
    os.makedirs(profiles_dir, exist_ok=True)
    out, _, rc = exec_cmd(client, "ls /etc/sing-box/profiles/*.json 2>/dev/null")
    if out:
        for rpath in out.decode().strip().splitlines():
            rpath = rpath.strip()
            if not rpath:
                continue
            fname = os.path.basename(rpath)
            local = os.path.join(profiles_dir, fname)
            success, msg = fetch_file(client, rpath, local)
            status = "OK" if success else "FAIL"
            if success:
                ok_count += 1
            else:
                fail_count += 1
            print(f"  [{status}] {rpath} -> etc/sing-box/profiles/{fname} ({msg})")
    else:
        print("  No profiles found")

    # Fetch binary files (with change detection)
    print("\n=== Fetching binary files ===")
    for remote, local_rel in BINARY_FILES:
        local = os.path.join(BACKUP_DIR, local_rel)
        success, msg = fetch_large_file(client, remote, local)
        status = "OK" if success else "FAIL"
        if success:
            ok_count += 1
        else:
            fail_count += 1
        print(f"  [{status}] {remote} -> {local_rel} ({msg})")
    
    # Run snapshot commands
    print("\n=== Fetching snapshots ===")
    for cmd, local_rel in SNAPSHOT_COMMANDS:
        local = os.path.join(BACKUP_DIR, local_rel)
        out, err, rc = exec_cmd(client, cmd)
        os.makedirs(os.path.dirname(local), exist_ok=True)
        with open(local, "wb") as f:
            f.write(out)
        size = len(out)
        print(f"  [OK] {cmd.split()[0]}... -> {local_rel} ({size} bytes)")
        ok_count += 1
    
    client.close()
    print(f"\nDone! OK: {ok_count}, Failed: {fail_count}")
    return 0 if fail_count == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
