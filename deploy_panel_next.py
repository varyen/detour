#!/usr/bin/env python3
"""Залить собранную Vue-панель (panel/dist) на роутер в /www/detour-next/.

Только для итерации во время разработки: в релиз панель уезжает пакетом
(build_release.py / keenetic/build-ipk.py кладут те же файлы). Здесь нет ни
подписи, ни реестра opkg — файлы просто перезаписываются.

    python3 deploy_panel_next.py [--router NAME]

Передача — tar.gz через stdin SSH-канала (SFTP на роутерах недоступен, а
base64 через exec_command рвётся на сотнях килобайт).
"""
import argparse
import io
import os
import sys
import tarfile

from router_config import load_router, ssh_connect

HERE = os.path.dirname(os.path.abspath(__file__))
DIST = os.path.join(HERE, "panel", "dist")


def make_tar():
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        for dirpath, dirnames, filenames in os.walk(DIST):
            dirnames.sort()
            for name in sorted(filenames):
                src = os.path.join(dirpath, name)
                rel = os.path.relpath(src, DIST).replace(os.sep, "/")
                info = tar.gettarinfo(src, arcname=rel)
                info.uid = info.gid = 0
                info.uname = info.gname = "root"
                info.mode = 0o644
                with open(src, "rb") as f:
                    tar.addfile(info, f)
    return buf.getvalue()


def run(ssh, cmd):
    _, out, err = ssh.exec_command(cmd, timeout=120)
    rc = out.channel.recv_exit_status()
    return rc, out.read().decode("utf-8", "replace"), err.read().decode("utf-8", "replace")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    cfg = load_router(parser=ap)
    if not os.path.isdir(DIST):
        sys.exit("нет panel/dist — собери: cd panel && npm ci && npm run build")

    keenetic = str(cfg.get("platform", "")).lower() == "keenetic"
    dest = "/opt/share/www/detour-next" if keenetic else "/www/detour-next"

    blob = make_tar()
    print(f"panel/dist → {cfg['host']}:{dest}  ({len(blob)} байт tar.gz)")

    ssh = ssh_connect(cfg)
    try:
        rc, _, err = run(ssh, f"rm -rf {dest} && mkdir -p {dest}")
        if rc != 0:
            sys.exit(f"не смог подготовить {dest}: {err.strip()}")

        chan = ssh.get_transport().open_session()
        chan.exec_command(f"tar -xzf - -C {dest}")
        chan.sendall(blob)
        chan.shutdown_write()
        rc = chan.recv_exit_status()
        stderr = chan.recv_stderr(65536).decode("utf-8", "replace")
        chan.close()
        if rc != 0:
            sys.exit(f"распаковка не удалась (rc={rc}): {stderr.strip()}")

        rc, out, _ = run(ssh, f"find {dest} -type f | wc -l; du -sh {dest} | cut -f1")
        print("файлов / объём:", " ".join(out.split()))
    finally:
        ssh.close()


if __name__ == "__main__":
    main()
