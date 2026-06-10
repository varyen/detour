#!/usr/bin/env python3
"""Targeted deploy of the version-chips feature to a router (panel + CGI + updater).

Pushes only the three changed files and refreshes the 6h `check-all` cron — it
does NOT touch firewall/services/init.d (use deploy_router.py for a full sync).

    python3 deploy_chips.py [--router home]
"""
import argparse
import hashlib
import os

from router_config import load_router, ssh_connect, exec_cmd

HERE = os.path.dirname(os.path.abspath(__file__))
RF = os.path.join(HERE, "router_files")


def upload(ssh, content, remote, mode):
    if isinstance(content, str):
        content = content.encode("utf-8")
    chan = ssh.get_transport().open_session()
    chan.exec_command(f"cat > {remote}")
    chan.sendall(content)
    chan.shutdown_write()
    chan.recv_exit_status()
    chan.close()
    exec_cmd(ssh, f"chmod {mode} {remote}")
    out, _, _ = exec_cmd(ssh, f"md5sum {remote} 2>/dev/null | cut -d' ' -f1")
    return out.strip()


def local_md5(path):
    with open(path, "rb") as f:
        return hashlib.md5(f.read()).hexdigest()


FILES = [
    ("detour-api", "/www/cgi-bin/detour-api", "0755"),
    ("index.html", "/www/detour/index.html", "0644"),
    ("detour-update", "/usr/sbin/detour-update", "0755"),
]
CRON_LINE = "0 */6 * * * /usr/sbin/detour-update check-all >/var/log/detour-update.log 2>&1"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--router", "-r", default="home")
    args = ap.parse_args()

    cfg = load_router(args.router)
    print(f"=== Deploying version-chips to {cfg['name']} ({cfg['host']}) ===")
    ssh = ssh_connect(cfg)

    print("-- uploading files (md5 verified)")
    for name, remote, mode in FILES:
        local = os.path.join(RF, name)
        rmd5 = upload(ssh, open(local, "rb").read(), remote, mode)
        ok = "OK" if rmd5 == local_md5(local) else "MISMATCH!"
        print(f"   {remote:32} {rmd5}  {ok}")

    print("-- refreshing 6h check-all cron (default on)")
    exec_cmd(
        ssh,
        "( crontab -l 2>/dev/null | grep -v 'detour-update' ; "
        f"echo '{CRON_LINE}' ) | crontab -",
    )
    exec_cmd(ssh, "/etc/init.d/cron enable >/dev/null 2>&1; /etc/init.d/cron restart >/dev/null 2>&1")
    out, _, _ = exec_cmd(ssh, "crontab -l 2>/dev/null | grep detour-update")
    print(f"   cron: {out.strip()}")

    print("-- seeding update state now (detour-update check-all) ...")
    out, err, rc = exec_cmd(ssh, "/usr/sbin/detour-update check-all 2>&1; echo RC=$?")
    print("   " + "\n   ".join((out or err).strip().splitlines()[-8:]))

    print("-- current versions (status payload binaries):")
    out, _, _ = exec_cmd(
        ssh,
        "for f in detour-update detour-bins detour-tpws detour-nfqws2; do "
        "echo \"$f: $(cat /var/state/$f.json 2>/dev/null || echo none)\"; done",
    )
    print("   " + "\n   ".join(out.strip().splitlines()))
    ssh.close()
    print("=== done — reload the panel (Ctrl+Shift+R) and check the header chips ===")


if __name__ == "__main__":
    main()
