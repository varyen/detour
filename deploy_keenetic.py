#!/usr/bin/env python3
"""Deploy the detour stack to a Keenetic KN-1810 (KeeneticOS + Entware, mipsel).

The OpenWrt counterpart is deploy_router.py. Keenetic is a different platform
(no uci/procd/nftables/uhttpd) so this is a separate orchestrator that targets
the Entware /opt layout described in keenetic/README.md.

    python deploy_keenetic.py --router keenetic [--skip-binaries]

⚠ NOT YET HARDWARE-VALIDATED. Every step that assumes the device is flagged
  VALIDATE. Requires: a routers.local.json entry with platform="keenetic",
  Entware already installed on the router, and keenetic/bins/ populated
  (run keenetic/fetch-bins.py first).

Prerequisites on the device (do once, by hand or via entware-bootstrap.sh):
  Entware installed at /opt, USB mounted, opkg working.
"""
import argparse
import os
import sys

from router_config import load_router, ssh_connect, exec_cmd

HERE = os.path.dirname(os.path.abspath(__file__))
KEEN = os.path.join(HERE, "keenetic")
BINS = os.path.join(KEEN, "bins")
BACKUP = os.path.join(HERE, "router-backup")
ROUTER_FILES = os.path.join(HERE, "router_files")


def step(msg):
    print(f"\n[*] {msg}")


def put_text(ssh, content, remote, mode="0644", fix_shebang=False):
    """Upload a small text file via base64 (busybox/coreutils base64 on Entware)."""
    import base64
    if fix_shebang:
        # KeeneticOS /bin/sh + /usr/bin/lua are limited/absent — point at Entware.
        lines = content.split("\n", 1)
        sb = lines[0]
        if sb.startswith("#!"):
            sb = sb.replace("/bin/sh", "/opt/bin/sh").replace("/usr/bin/lua", "/opt/bin/lua")
            content = sb + ("\n" + lines[1] if len(lines) > 1 else "")
    b64 = base64.b64encode(content.encode("utf-8")).decode("ascii")
    exec_cmd(ssh, f"mkdir -p '{os.path.dirname(remote)}'")
    chan = ssh.get_transport().open_session()
    chan.exec_command(f"base64 -d > '{remote}' && chmod {mode} '{remote}'")
    chan.sendall(b64)
    chan.shutdown_write()
    chan.recv_exit_status()
    chan.close()


def put_file(ssh, local, remote, mode="0644", fix_shebang=False):
    if fix_shebang:
        with open(local, "rb") as f:
            put_text(ssh, f.read().decode("utf-8"), remote, mode, fix_shebang=True)
        return
    with open(local, "rb") as f:
        data = f.read()
    exec_cmd(ssh, f"mkdir -p '{os.path.dirname(remote)}'")
    # Raw stdin pipe — no SFTP on Entware busybox; handles the ~76 MB sing-box.
    chan = ssh.get_transport().open_session()
    chan.exec_command(f"cat > '{remote}' && chmod {mode} '{remote}'")
    sent = 0
    view = memoryview(data)
    while sent < len(data):
        n = chan.send(view[sent:sent + 65536])
        if n == 0:
            break
        sent += n
    chan.shutdown_write()
    chan.recv_exit_status()
    chan.close()
    print(f"  {remote} ({sent} B)")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--router", "-r", default="keenetic")
    ap.add_argument("--skip-binaries", action="store_true")
    args = ap.parse_args()

    cfg = load_router(name=args.router)
    if cfg.get("platform") != "keenetic":
        sys.exit(f"router '{args.router}' is platform={cfg.get('platform')!r}, expected 'keenetic'. "
                 "Use deploy_router.py for OpenWrt targets.")
    print(f"=== Deploy detour -> {cfg['name']} ({cfg['host']}) [KeeneticOS+Entware] ===")
    ssh = ssh_connect(cfg)

    # 0. Sanity: Entware present.
    out, _, _ = exec_cmd(ssh, "[ -x /opt/bin/opkg ] && echo yes || echo no")
    if out.strip() != "yes":
        sys.exit("Entware not found at /opt/bin/opkg — install Entware first "
                 "(USB + KeeneticOS 'opkg' component), then re-run.")
    out, _, _ = exec_cmd(ssh, "uname -m; opkg print-architecture 2>/dev/null | tail -1")
    print(f"  arch: {out.strip()}  (expect mips + mipselsf)")  # ⚠ VALIDATE

    # 1. Dependencies (mirrors entware-bootstrap.sh; idempotent).
    step("Installing Entware dependencies (opkg)")
    deps = ("iptables ipset dnsmasq-full lighttpd lighttpd-mod-cgi lighttpd-mod-setenv "
            "lua lua-cjson coreutils-base64 openssl-util curl start-stop-daemon")
    out, err, rc = exec_cmd(ssh, f"opkg update >/dev/null 2>&1; opkg install {deps} 2>&1 | tail -3", timeout=300)
    print("  " + out.strip().replace("\n", "\n  "))

    # 2. Directory skeleton.
    exec_cmd(ssh, "mkdir -p /opt/sbin /opt/etc/sing-box/profiles /opt/etc/zapret-tpws "
                  "/opt/etc/detour/subscriptions /opt/var/log /opt/var/run /opt/var/state "
                  "/opt/share/www/detour /opt/share/www/cgi-bin /opt/etc/ndm/netfilter.d "
                  "/opt/etc/lighttpd /tmp/detour-sessions")

    # 3. Binaries (mipsel). Big — pushed over stdin pipe.
    if not args.skip_binaries:
        step("Uploading mipsel binaries")
        for name, dst in (("sing-box", "/opt/sbin/sing-box"), ("tpws-zapret", "/opt/sbin/tpws-zapret")):
            local = os.path.join(BINS, name)
            if not os.path.isfile(local):
                sys.exit(f"missing {local} — run: python keenetic/fetch-bins.py")
            put_file(ssh, local, dst, "0755")
        # ⚠ VALIDATE: smoke test on device — must not print 'Error relocating'.
        out, _, _ = exec_cmd(ssh, "/opt/sbin/sing-box version 2>&1 | head -1")
        print(f"  sing-box: {out.strip()}")

    # 4. Runtime config + service/firewall/web artifacts.
    step("Installing service + firewall + web artifacts")
    put_file(ssh, os.path.join(KEEN, "etc", "detour.conf"), "/opt/etc/detour/detour.conf", "0644")
    put_file(ssh, os.path.join(KEEN, "init.d", "S51detour-panel"), "/opt/etc/init.d/S51detour-panel", "0755")
    put_file(ssh, os.path.join(KEEN, "init.d", "S52detour-singbox"), "/opt/etc/init.d/S52detour-singbox", "0755")
    put_file(ssh, os.path.join(KEEN, "init.d", "S53detour-zapret"), "/opt/etc/init.d/S53detour-zapret", "0755")
    put_file(ssh, os.path.join(KEEN, "ndm", "netfilter.d", "50-detour.sh"),
             "/opt/etc/ndm/netfilter.d/50-detour.sh", "0755")
    put_file(ssh, os.path.join(KEEN, "lighttpd", "detour.conf"), "/opt/etc/lighttpd/detour.conf", "0644")

    # 5. Support scripts (shebang -> Entware interpreters).
    step("Installing support scripts")
    put_file(ssh, os.path.join(ROUTER_FILES, "detour-update"), "/opt/sbin/detour-update", "0755", fix_shebang=True)
    put_file(ssh, os.path.join(ROUTER_FILES, "subscription-refresh"), "/opt/sbin/subscription-refresh", "0755", fix_shebang=True)
    put_file(ssh, os.path.join(ROUTER_FILES, "vpn-keepalive"), "/opt/sbin/vpn-keepalive", "0755", fix_shebang=True)

    # 6. Panel (CGI shebang -> /opt/bin/sh so lighttpd runs it).
    step("Deploying panel (lighttpd doc-root + CGI)")
    put_file(ssh, os.path.join(BACKUP, "www", "detour", "index.html"), "/opt/share/www/detour/index.html", "0644")
    put_file(ssh, os.path.join(BACKUP, "www", "cgi-bin", "detour-api"),
             "/opt/share/www/cgi-bin/detour-api", "0755", fix_shebang=True)

    # 7. Platform marker (the CGI's shim keys off this) + version.
    exec_cmd(ssh, "touch /opt/etc/detour/platform")
    ver = open(os.path.join(HERE, "VERSION")).read().strip()
    put_text(ssh, ver + "\n", "/opt/etc/detour/version", "0644")

    # 8. Panel auth — seed only if absent (preserve operator's password on re-deploy).
    step("Panel auth")
    out, _, _ = exec_cmd(ssh, "[ -f /opt/etc/detour.auth ] && echo exists || echo absent")
    if out.strip() == "absent":
        user = cfg.get("panel_user", "admin")
        pw = cfg.get("panel_password") or "admin"
        chan = ssh.get_transport().open_session()
        chan.exec_command("H=$(openssl passwd -6 -stdin); "
                          f"printf '%s:%s\\n' '{user}' \"$H\" > /opt/etc/detour.auth; "
                          "chmod 600 /opt/etc/detour.auth")
        chan.sendall((pw + "\n").encode())
        chan.shutdown_write(); chan.recv_exit_status(); chan.close()
        print(f"  seeded /opt/etc/detour.auth (user={user})")
    else:
        print("  /opt/etc/detour.auth exists, keeping")

    # 9. Start services. S51 panel first, then proxies. (rc.unslung auto-runs S* on boot.)
    step("Starting services")
    for svc in ("S51detour-panel", "S52detour-singbox", "S53detour-zapret"):
        out, _, _ = exec_cmd(ssh, f"/opt/etc/init.d/{svc} restart 2>&1", timeout=30)
        print(f"  {svc}: {out.strip()[:120]}")
    # Re-assert firewall rules now (NDM will re-run the hook on its own reconfigs).
    exec_cmd(ssh, "/opt/etc/ndm/netfilter.d/50-detour.sh iptables nat 2>&1")

    # 10. Verify.
    step("Verify")
    out, _, _ = exec_cmd(ssh, "netstat -tlnp 2>/dev/null | grep -E ':8080|:12345|:1081' || echo '(no listeners — check logs)'")
    print("  " + out.strip().replace("\n", "\n  "))

    ssh.close()
    print(f"\n=== DONE ===\n  Panel: http://{cfg['host']}:8080/detour/  (login: {cfg.get('panel_user','admin')})")
    print("  ⚠ Validate on device: bins run, lighttpd up, nat REDIRECT works, DNS/ipset wired.")


if __name__ == "__main__":
    main()
