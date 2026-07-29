"""Push individual files to a router over SSH (stdin pipe, no SFTP).

Usage: python push_files.py [--router NAME] <local>:<remote>[:<mode>] ...
"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from router_config import load_router, ssh_connect, exec_cmd

args = sys.argv[1:]
name = "home"
if args and args[0] == "--router":
    name = args[1]
    args = args[2:]

def read_payload(path):
    """Bytes to send. CRLF->LF only for real text.

    The CRLF strip exists because the repo lives on a Windows checkout and router
    shell scripts must not arrive with \\r line endings. Applying it blindly also
    rewrites BINARY payloads: a .ipk that happens to contain the byte pair 0d 0a
    (gzip output regularly does) loses those bytes and arrives corrupt — opkg still
    "installed" such a package, and only the usign check caught it. So: decode as
    UTF-8 first, and leave anything that isn't text exactly as it is on disk.
    """
    data = open(path, "rb").read()
    try:
        data.decode("utf-8")
    except UnicodeDecodeError:
        return data
    return data.replace(b"\r\n", b"\n")


r = load_router(name=name)
ssh = ssh_connect(r)
for spec in args:
    local, remote, mode = (spec.split(":") + ["0755"])[:3]
    data = read_payload(local)
    chan = ssh.get_transport().open_session()
    chan.exec_command("cat > '%s.tmp' && chmod %s '%s.tmp' && mv '%s.tmp' '%s'"
                      % (remote, mode, remote, remote, remote))
    chan.sendall(data)
    chan.shutdown_write()
    rc = chan.recv_exit_status()
    err = chan.recv_stderr(65535).decode("utf-8", "replace")
    print("%-40s -> %-40s %d bytes  rc=%d %s" % (local, remote, len(data), rc, err.strip()))
    chan.close()
ssh.close()
