"""Ad-hoc SSH command runner for Detour routers.
Usage: python rsh.py [router_name] < command_script   (command read from stdin)
"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from router_config import load_router, ssh_connect, exec_cmd

name = sys.argv[1] if len(sys.argv) > 1 else "home"
cmd = sys.stdin.read()
r = load_router(name=name)
ssh = ssh_connect(r)
out, err, rc = exec_cmd(ssh, cmd, timeout=90)
sys.stdout.write(out)
if err.strip():
    sys.stderr.write("\n[STDERR]\n" + err)
sys.stdout.write("\n[rc=%d]\n" % rc)
ssh.close()
