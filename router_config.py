"""Shared router config loader for all deploy scripts.

Usage:
    from router_config import load_router, ssh_connect
    cfg = load_router()                  # --router NAME or default
    ssh = ssh_connect(cfg)
"""
import argparse
import json
import os
import sys
import paramiko

_HERE = os.path.dirname(os.path.abspath(__file__))
# Lookup order: ROUTERS_CONFIG env override > routers.local.json > routers.json (legacy).
# routers.example.json is the committed template; never load it directly.
_CANDIDATE_PATHS = [
    os.environ.get("ROUTERS_CONFIG") or "",
    os.path.join(_HERE, "routers.local.json"),
    os.path.join(_HERE, "routers.json"),
]


def _find_config():
    for p in _CANDIDATE_PATHS:
        if p and os.path.isfile(p):
            return p
    sys.exit(
        "routers.local.json not found — copy routers.example.json to "
        "routers.local.json and fill in real credentials "
        "(or set ROUTERS_CONFIG=<path>)."
    )


def _load_config():
    with open(_find_config(), "r", encoding="utf-8") as f:
        return json.load(f)


def load_global_config():
    """Return the full routers.local.json (top-level keys like github, default)."""
    return _load_config()


def load_router(name=None, parser=None):
    """Resolve which router to operate on.

    Order of precedence: explicit `name` arg > --router CLI arg > $ROUTER env > default.
    """
    cfg = _load_config()

    if name is None:
        if parser is None:
            parser = argparse.ArgumentParser(add_help=False)
        parser.add_argument(
            "--router", "-r",
            default=os.environ.get("ROUTER"),
            help=f"Router name from routers.local.json (default: {cfg['default']})",
        )
        args, _ = parser.parse_known_args()
        name = args.router or cfg["default"]

    routers = cfg["routers"]
    if name not in routers:
        sys.exit(f"unknown router '{name}'. available: {', '.join(routers)}")

    r = dict(routers[name])
    r["name"] = name
    return r


def ssh_connect(router_cfg, timeout=10):
    ssh = paramiko.SSHClient()
    ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    ssh.connect(
        router_cfg["host"],
        port=router_cfg.get("port", 22),
        username=router_cfg["user"],
        password=router_cfg["password"],
        timeout=timeout,
    )
    return ssh


def exec_cmd(ssh, cmd, timeout=30):
    stdin, stdout, stderr = ssh.exec_command(cmd, timeout=timeout)
    out = stdout.read().decode("utf-8", errors="replace")
    err = stderr.read().decode("utf-8", errors="replace")
    rc = stdout.channel.recv_exit_status()
    return out, err, rc
