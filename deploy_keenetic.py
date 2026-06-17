#!/usr/bin/env python3
"""Deploy the detour stack to a Keenetic (KeeneticOS + Entware, mipsel) over SSH.

The OpenWrt counterpart is deploy_router.py. Keenetic is a different platform
(no uci/procd/nftables/uhttpd) so this is a separate orchestrator that targets
the Entware /opt layout described in keenetic/README.md.

    python deploy_keenetic.py --router keenetic
    python deploy_keenetic.py --router keenetic --ipk releases/v1.2.3/detour-keenetic_1.2.3_all.ipk

SINGLE SOURCE OF TRUTH: this builds the authoritative detour-keenetic_*.ipk via
keenetic/build-ipk.py (same package the panel self-update and build_release.py
ship) and `opkg install`s it. opkg's postinst does the real work — seeds config,
disables Entware's S99sing-box autostart, registers the scheduler daemon, starts
services. So a new file only ever needs adding to keenetic/build-ipk.py's FILES
list; there is NO second manifest here to keep in sync.

  (History: this script used to hand-copy a partial, drifting file set — and pulled
   the panel from the stale router-backup/ snapshot and uploaded a bundled sing-box
   that the slim .ipk dropped. Building + installing the .ipk removes all of that.)

⚠ Requires: a routers.local.json entry with platform="keenetic", Entware already
  installed on the router (USB + opkg working), and keenetic/bins/ populated for the
  bundled tpws (run keenetic/fetch-bins.py first). sing-box resolves from the Entware
  feed (sing-box-go) at install time.
"""
import argparse
import importlib.util
import os
import sys

from router_config import load_router, ssh_connect, exec_cmd

HERE = os.path.dirname(os.path.abspath(__file__))
KEEN = os.path.join(HERE, "keenetic")


def step(msg):
    print(f"\n[*] {msg}")


def put_file(ssh, local, remote, mode="0644"):
    """Upload a file via a raw stdin pipe — no SFTP on Entware busybox."""
    with open(local, "rb") as f:
        data = f.read()
    exec_cmd(ssh, f"mkdir -p '{os.path.dirname(remote)}'")
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


def load_build_ipk():
    """Import keenetic/build-ipk.py (hyphenated name → importlib), mirroring
    build_release.py, so the .ipk is built from the one canonical source."""
    spec = importlib.util.spec_from_file_location(
        "keenetic_build_ipk", os.path.join(KEEN, "build-ipk.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def seed_auth_if_absent(ssh, cfg):
    """Seed /opt/etc/detour.auth from routers.local.json creds BEFORE opkg install,
    so the postinst's own 'seed admin/detour if absent' step keeps ours. On re-deploy
    the file already exists → operator's chosen/changed password is preserved."""
    out, _, _ = exec_cmd(ssh, "[ -f /opt/etc/detour.auth ] && echo exists || echo absent")
    if out.strip() != "absent":
        print("  /opt/etc/detour.auth exists, keeping")
        return
    user = cfg.get("panel_user", "admin")
    pw = cfg.get("panel_password") or "detour"
    chan = ssh.get_transport().open_session()
    chan.exec_command("mkdir -p /opt/etc/detour; H=$(openssl passwd -6 -stdin); "
                      f"printf '%s:%s\\n' '{user}' \"$H\" > /opt/etc/detour.auth; "
                      "chmod 600 /opt/etc/detour.auth")
    chan.sendall((pw + "\n").encode())
    chan.shutdown_write(); chan.recv_exit_status(); chan.close()
    print(f"  seeded /opt/etc/detour.auth (user={user})")


def main():
    ap = argparse.ArgumentParser(
        description="Build + install the authoritative detour-keenetic .ipk over SSH.")
    ap.add_argument("--router", "-r", default="keenetic")
    ap.add_argument("--ipk", help="install this prebuilt .ipk instead of building one")
    args = ap.parse_args()

    cfg = load_router(name=args.router)
    if cfg.get("platform") != "keenetic":
        sys.exit(f"router '{args.router}' is platform={cfg.get('platform')!r}, expected 'keenetic'. "
                 "Use deploy_router.py for OpenWrt targets.")
    print(f"=== Deploy detour -> {cfg['name']} ({cfg['host']}) [KeeneticOS+Entware] ===")

    # 1. Build (or take) the .ipk locally first — fail fast before touching the router.
    if args.ipk:
        ipk_path = args.ipk
        if not os.path.isfile(ipk_path):
            sys.exit(f"--ipk not found: {ipk_path}")
        print(f"Using prebuilt {ipk_path}")
    else:
        step("Building detour-keenetic .ipk (source: router_files/ + keenetic/)")
        try:
            ipk_path, sig_path, size = load_build_ipk().build()
        except SystemExit as e:   # build_data() exits on a missing source (e.g. tpws bin)
            sys.exit(str(e) or "build failed — run: python keenetic/fetch-bins.py")
        print(f"  {ipk_path}  (installed {size:,} B){'  + .sig' if sig_path else '  (UNSIGNED)'}")

    ssh = ssh_connect(cfg)

    # 2. Sanity: Entware present.
    out, _, _ = exec_cmd(ssh, "[ -x /opt/bin/opkg ] && echo yes || echo no")
    if out.strip() != "yes":
        sys.exit("Entware not found at /opt/bin/opkg — install Entware first "
                 "(USB + KeeneticOS 'opkg' component), then re-run.")
    out, _, _ = exec_cmd(ssh, "uname -m; opkg print-architecture 2>/dev/null | tail -1")
    print(f"  arch: {out.strip()}  (expect mips + mipselsf)")

    # 3. Panel auth — seed operator creds if this is a fresh box (before install, so the
    #    postinst keeps them; on re-deploy the existing/changed password is preserved).
    step("Panel auth")
    seed_auth_if_absent(ssh, cfg)

    # 4. Upload + opkg install. The postinst seeds config, disables Entware's bundled
    #    S99sing-box, registers S90detour-cron, and starts the services. sing-box and the
    #    other Depends resolve from the Entware feed.
    step("Installing the .ipk (opkg — runs postinst: config seed + service start)")
    remote_ipk = "/tmp/" + os.path.basename(ipk_path)
    put_file(ssh, ipk_path, remote_ipk, "0644")
    out, _, _ = exec_cmd(
        ssh, f"opkg update >/dev/null 2>&1; opkg install --force-reinstall '{remote_ipk}' 2>&1",
        timeout=300)
    print("  " + out.strip().replace("\n", "\n  "))
    exec_cmd(ssh, f"rm -f '{remote_ipk}'")

    # 5. Re-assert firewall now (NDM re-runs the hook on its own reconfigs).
    exec_cmd(ssh, "[ -x /opt/etc/ndm/netfilter.d/50-detour.sh ] && "
                  "/opt/etc/ndm/netfilter.d/50-detour.sh iptables nat 2>&1")

    # 6. Verify listeners.
    step("Verify")
    out, _, _ = exec_cmd(ssh, "netstat -tlnp 2>/dev/null | grep -E ':8080|:12345|:1081' "
                              "|| echo '(no listeners — check logs)'")
    print("  " + out.strip().replace("\n", "\n  "))

    ssh.close()
    print(f"\n=== DONE ===\n  Panel: http://{cfg['host']}:8080/detour/  (login: {cfg.get('panel_user','admin')})")
    print("  ⚠ Validate on device: bins run, lighttpd up, nat REDIRECT works, DNS/ipset wired.")


if __name__ == "__main__":
    main()
