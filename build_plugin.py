#!/usr/bin/env python3
"""Сборка плагинов Detour: plugins/<id>/ → пакеты detour-plugin-<id> + каталог.

Usage:
    python build_plugin.py                 # все плагины из plugins/
    python build_plugin.py hello           # только перечисленные
    python build_plugin.py --check         # только проверить манифесты
    python build_plugin.py --publish       # собрать ВСЕ и выложить каталог в ветку feed

На каждый плагин три пакета одного содержимого, как у панели:
    detour-plugin-<id>_<ver>_all.ipk       — opkg (OpenWrt ≤ 24.10, GL.iNet)
    detour-plugin-<id>_<ver>_noarch.apk    — apk-tools 3 (OpenWrt 25.12+, Linux-контейнер)
    detour-plugin-<id>_<ver>_keenetic.ipk  — Keenetic/Entware (пути под /opt)
и к каждому .sig (usign, ключ релизов). Каталог — releases/plugins/feed/:
index.json (+ .sig) и <id>/<пакеты>; --publish кладёт его в каталог plugins/
ветки feed. Публикуются всегда ВСЕ плагины репозитория: каталог ветки
заменяется целиком, и плагин, собранный не в этот раз, иначе пропал бы.

Формат плагина описан в plugins/README.md.
"""

import argparse
import hashlib
import io
import json
import os
import re
import shutil
import sys
import tarfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import apk_pkg  # noqa: E402
from usign_compat import sign_file  # noqa: E402

PLUGINS_DIR = os.path.join(HERE, "plugins")
OUT_DIR = os.path.join(HERE, "releases", "plugins")
FEED_OUT = os.path.join(OUT_DIR, "feed")
KEY_SEC = os.path.join(HERE, "keys", "release.usign.sec")
PKG_PREFIX = "detour-plugin-"

ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,31}$")
OP_RE = re.compile(r"^[a-z0-9][a-z0-9_-]{0,47}$")
VER_RE = re.compile(r"^\d+\.\d+\.\d+$")
PLATFORMS = ("openwrt", "keenetic")
AREAS = ("services", "rules", "profiles")
FIELD_TYPES = ("text", "password", "number", "bool", "select", "textarea")
RESERVED_OPS = ("install", "remove", "enable", "disable", "settings")

# Пути OpenWrt → Keenetic/Entware. Порядок важен: длинные префиксы раньше.
KEENETIC_REMAP = (
    ("/usr/share/", "/opt/share/"),
    ("/usr/sbin/", "/opt/sbin/"),
    ("/usr/bin/", "/opt/bin/"),
    ("/usr/lib/", "/opt/lib/"),
    ("/sbin/", "/opt/sbin/"),
    ("/bin/", "/opt/bin/"),
    ("/www/", "/opt/share/www/"),
    ("/etc/", "/opt/etc/"),
)

# Куда плагину нельзя класть файлы: пользовательское состояние и вход в панель.
FORBIDDEN_DST = (
    "/etc/detour.auth", "/etc/detour/plugins/", "/etc/sing-box/profiles/",
    "/etc/sing-box/settings.json", "/etc/opkg/", "/etc/apk/",
)


class PluginError(Exception):
    pass


def fail(pid, msg):
    raise PluginError(f"{pid}: {msg}")


def core_paths():
    """Пути файлов самой панели — плагин не должен их перезаписывать."""
    try:
        import build_release
        return {"/" + d for _, d, _ in build_release.PANEL_FILES}
    except Exception:
        return set()


def to_keenetic(pid, dst):
    if dst.startswith("/opt/"):
        return dst
    if dst.startswith("/etc/init.d/"):
        fail(pid, f"{dst}: init-скрипт OpenWrt на Keenetic не переносится — "
                  "добавьте отдельный файл с \"platform\": \"keenetic\" (S##имя в /opt/etc/init.d/)")
    for a, b in KEENETIC_REMAP:
        if dst.startswith(a):
            return b + dst[len(a):]
    fail(pid, f"{dst}: не знаю, куда это положить на Keenetic — задайте отдельный файл с \"platform\": \"keenetic\"")


def fix_shebang(data):
    if not data.startswith(b"#!"):
        return data
    nl = data.find(b"\n")
    first, rest = data[:nl], data[nl:]
    first = first.replace(b"/bin/sh", b"/opt/bin/sh").replace(b"/usr/bin/lua", b"/opt/bin/lua")
    first = first.replace(b"/opt/opt/", b"/opt/")
    return first + rest


def walk(root):
    for base, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in ("node_modules", ".git", "src"))
        for name in sorted(files):
            full = os.path.join(base, name)
            yield full, os.path.relpath(full, root).replace(os.sep, "/")


def load_manifest(pid):
    pdir = os.path.join(PLUGINS_DIR, pid)
    path = os.path.join(pdir, "plugin.json")
    if not os.path.isfile(path):
        fail(pid, "нет plugin.json")
    try:
        with open(path, encoding="utf-8") as f:
            m = json.load(f)
    except ValueError as e:
        fail(pid, f"plugin.json не JSON: {e}")
    validate(pid, m, pdir)
    return m


def validate(pid, m, pdir):
    if not ID_RE.match(pid):
        fail(pid, "id каталога: a-z, 0-9 и дефис, до 32 символов")
    if m.get("id") != pid:
        fail(pid, f"id в манифесте ({m.get('id')!r}) не совпадает с каталогом")
    if not VER_RE.match(str(m.get("version", ""))):
        fail(pid, "version: нужен X.Y.Z")
    if not isinstance(m.get("name"), str) or not m["name"].strip():
        fail(pid, "name обязателен")
    if "catalog" in m and not isinstance(m["catalog"], bool):
        fail(pid, "catalog: true или false")
    if "min_panel" in m and not VER_RE.match(str(m["min_panel"])):
        fail(pid, "min_panel: нужен X.Y.Z")
    plats = m.get("platforms", list(PLATFORMS))
    if not plats or any(p not in PLATFORMS for p in plats):
        fail(pid, f"platforms: подмножество {PLATFORMS}")
    deps = m.get("depends", {})
    if not isinstance(deps, dict) or any(k not in PLATFORMS or not isinstance(v, list) for k, v in deps.items()):
        fail(pid, "depends: {\"openwrt\": [...], \"keenetic\": [...]}")
    for c in m.get("cron", []):
        if not OP_RE.match(str(c.get("op", ""))) or not isinstance(c.get("every"), int) or c["every"] < 60:
            fail(pid, "cron: [{\"op\": \"tick\", \"every\": <секунд, ≥ 60>}]")
    ui = m.get("ui", {})
    if not isinstance(ui, dict):
        fail(pid, "ui должен быть объектом")
    card = ui.get("card")
    if card is not None:
        if not isinstance(card, dict) or not card.get("title"):
            fail(pid, "ui.card: нужен title")
        if card.get("span", 2) not in (2, 3, 4, 6):
            fail(pid, "ui.card.span: 2, 3, 4 или 6")
        if "op" in card and not OP_RE.match(str(card["op"])):
            fail(pid, "ui.card.op: недопустимое имя")
    keys = set()
    for s in ui.get("sections", []):
        if not s.get("id") or not s.get("title"):
            fail(pid, "ui.sections[]: нужны id и title")
        if s.get("area", "services") not in AREAS:
            fail(pid, f"ui.sections[].area: одно из {AREAS}")
        for f in s.get("fields", []):
            k = f.get("key")
            if not isinstance(k, str) or not re.match(r"^[a-zA-Z0-9_]+$", k):
                fail(pid, f"поле {k!r}: key из a-z, 0-9, _")
            if k in keys:
                fail(pid, f"поле {k}: key повторяется")
            keys.add(k)
            if f.get("type", "text") not in FIELD_TYPES:
                fail(pid, f"поле {k}: type одно из {FIELD_TYPES}")
            if f.get("type") == "select" and not f.get("options"):
                fail(pid, f"поле {k}: у select нужны options")
        for a in s.get("actions", []):
            if not OP_RE.match(str(a.get("op", ""))) or a["op"] in RESERVED_OPS:
                fail(pid, f"действие {a.get('op')!r}: недопустимое имя")
    for a in (card or {}).get("actions", []):
        if not OP_RE.match(str(a.get("op", ""))) or a["op"] in RESERVED_OPS:
            fail(pid, f"действие {a.get('op')!r}: недопустимое имя")
    mod = ui.get("module")
    if mod is not None:
        if not re.match(r"^[\w./-]+\.m?js$", mod) or ".." in mod:
            fail(pid, "ui.module: путь к .js внутри panel/")
        if not os.path.isfile(os.path.join(panel_root(pdir), mod)):
            fail(pid, f"ui.module: нет файла panel/{mod}")
    needs_api = bool(m.get("cron")) or bool(card and (card.get("op") or card.get("actions"))) \
        or any(s.get("actions") for s in ui.get("sections", []))
    if needs_api and not os.path.isfile(os.path.join(pdir, "api")):
        fail(pid, "манифест зовёт действия, а обработчика api нет")
    for f in m.get("files", []):
        src, dst = f.get("src", ""), f.get("dst", "")
        if not src or ".." in src.replace("\\", "/").split("/") or os.path.isabs(src):
            fail(pid, f"files: src {src!r} должен быть путём внутри плагина")
        if not os.path.isfile(os.path.join(pdir, src)):
            fail(pid, f"files: нет файла {src}")
        if not dst.startswith("/") or ".." in dst.split("/"):
            fail(pid, f"files: dst {dst!r} — абсолютный путь на роутере")
        if f.get("platform") not in (None, *PLATFORMS):
            fail(pid, f"files: platform одно из {PLATFORMS}")


def panel_root(pdir):
    """Сборка UI плагина (panel/dist), если она есть, иначе сам panel/."""
    dist = os.path.join(pdir, "panel", "dist")
    return dist if os.path.isdir(dist) else os.path.join(pdir, "panel")


def collect(pid, m, platform):
    """[(абсолютный путь источника, путь в пакете без /, mode, данные)] для платформы."""
    pdir = os.path.join(PLUGINS_DIR, pid)
    keen = platform == "keenetic"
    share = f"/usr/share/detour/plugins/{pid}"
    www = f"/www/detour/plugins/{pid}"
    out = []

    def add(src, dst, mode):
        if keen:
            dst = to_keenetic(pid, dst)
        with open(src, "rb") as f:
            data = f.read()
        if data.startswith(b"#!") and b"\r\n" in data:
            fail(pid, f"{os.path.relpath(src, pdir)}: CRLF в скрипте — шебанг `#!...\\r` на роутере не запустится")
        if keen and mode & 0o111:
            data = fix_shebang(data)
        out.append((src, dst.lstrip("/"), mode, data))

    add(os.path.join(pdir, "plugin.json"), f"{share}/plugin.json", 0o644)
    if os.path.isfile(os.path.join(pdir, "api")):
        add(os.path.join(pdir, "api"), f"{share}/api", 0o755)
    proot = panel_root(pdir)
    if os.path.isdir(proot):
        for full, rel in walk(proot):
            if rel.startswith("dist/") or rel in ("package.json", "package-lock.json") or rel.endswith(".ts"):
                continue
            add(full, f"{www}/{rel}", 0o644)
    cgi = os.path.join(pdir, "cgi-bin")
    if os.path.isdir(cgi):
        for full, rel in walk(cgi):
            add(full, f"/www/cgi-bin/{rel}", 0o755)
    for f in m.get("files", []):
        if f.get("platform") and f["platform"] != platform:
            continue
        mode = int(str(f.get("mode", "0644")), 8)
        add(os.path.join(pdir, f["src"]), f["dst"], mode)

    core = core_paths()
    seen = set()
    for _, dst, _, _ in out:
        full = "/" + dst
        if full in seen:
            fail(pid, f"{full}: два файла в одно место")
        seen.add(full)
        if any(full == p or full.startswith(p) for p in FORBIDDEN_DST) \
                or full.replace("/opt/", "/", 1) in core or full in core:
            fail(pid, f"{full}: этот путь принадлежит панели или пользовательским данным")
    return out


def scripts(pid, platform):
    tool = "/opt/sbin/detour-plugin" if platform == "keenetic" else "/usr/sbin/detour-plugin"
    sh = "#!/opt/bin/sh" if platform == "keenetic" else "#!/bin/sh"
    postinst = f"""{sh}
[ -x {tool} ] && {tool} postinst {pid}
exit 0
"""
    prerm = f"""{sh}
# opkg: $1 = remove | upgrade
[ -x {tool} ] && {tool} prerm {pid} "$1"
exit 0
"""
    return postinst, prerm


def tar_add(tar, name, data, mode, dirtype=False):
    info = tarfile.TarInfo(name)
    info.mode = mode
    info.uid = info.gid = 0
    info.uname = info.gname = "root"
    info.mtime = int(time.time())
    if dirtype:
        info.type = tarfile.DIRTYPE
        tar.addfile(info)
    else:
        info.size = len(data)
        tar.addfile(info, io.BytesIO(data))


def build_ipk(pid, m, platform, files, out_path):
    deps = ["detour-keenetic" if platform == "keenetic" else "detour"] + m.get("depends", {}).get(platform, [])
    size = sum(len(d) for *_, d in files)
    control = (
        f"Package: {PKG_PREFIX}{pid}\n"
        f"Version: {m['version']}\n"
        f"Depends: {', '.join(deps)}\n"
        f"Source: https://github.com/varyen/detour\n"
        f"License: {m.get('license', 'MIT')}\n"
        f"Section: net\nPriority: optional\n"
        f"Maintainer: {m.get('author') or 'detour'}\n"
        f"Architecture: all\n"
        f"Installed-Size: {size}\n"
        f"Description: Detour plugin: {m['name']}\n"
    )
    postinst, prerm = scripts(pid, platform)
    cbuf = io.BytesIO()
    with tarfile.open(fileobj=cbuf, mode="w:gz", format=tarfile.USTAR_FORMAT) as t:
        tar_add(t, "./control", control.encode(), 0o644)
        tar_add(t, "./postinst", postinst.encode(), 0o755)
        tar_add(t, "./prerm", prerm.encode(), 0o755)
    dirs = set()
    for _, dst, _, _ in files:
        parts = dst.split("/")
        for i in range(1, len(parts)):
            dirs.add("/".join(parts[:i]))
    dbuf = io.BytesIO()
    with tarfile.open(fileobj=dbuf, mode="w:gz", format=tarfile.USTAR_FORMAT) as t:
        for d in sorted(dirs):
            tar_add(t, "./" + d + "/", b"", 0o755, dirtype=True)
        for _, dst, mode, data in files:
            tar_add(t, "./" + dst, data, mode)
    with tarfile.open(out_path, "w:gz", format=tarfile.USTAR_FORMAT) as t:
        tar_add(t, "./debian-binary", b"2.0\n", 0o644)
        tar_add(t, "./control.tar.gz", cbuf.getvalue(), 0o644)
        tar_add(t, "./data.tar.gz", dbuf.getvalue(), 0o644)


def build_apk(pid, m, files, out_path, tmpdir):
    # apk_pkg берёт файлы с диска — скрипты и пути одинаковы с ipk-сборкой OpenWrt.
    entries = []
    for i, (_, dst, mode, data) in enumerate(files):
        p = os.path.join(tmpdir, f"f{i}")
        with open(p, "wb") as f:
            f.write(data)
        entries.append((p, dst, mode))
    postinst, prerm = scripts(pid, "openwrt")
    # apk зовёт pre-deinstall с версией, а не с opkg-действием — подставляем его.
    pre_remove = prerm.replace('"$1"', "remove")
    pre_upgrade = prerm.replace('"$1"', "upgrade")
    deps = ["detour"] + m.get("depends", {}).get("openwrt", [])
    apk_pkg.build_apk(
        out_path,
        pkgname=PKG_PREFIX + pid,
        version=m["version"],
        description=f"Detour plugin: {m['name']}",
        url="https://github.com/varyen/detour",
        license_=m.get("license", "MIT"),
        maintainer=m.get("author") or "detour",
        depends=deps,
        file_entries=entries,
        scripts={
            "post-install": postinst,
            "post-upgrade": postinst,
            "pre-upgrade": pre_upgrade,
            "pre-deinstall": pre_remove,
        },
    )


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def sign(path, enabled):
    if not enabled:
        return
    if not os.path.isfile(KEY_SEC):
        raise SystemExit(f"нет ключа подписи {KEY_SEC} — запустите с --no-sign для локальной сборки")
    sign_file(path, KEY_SEC, path + ".sig")


def build_plugin(pid, do_sign):
    m = load_manifest(pid)
    ver = m["version"]
    plats = m.get("platforms", list(PLATFORMS))
    out = os.path.join(OUT_DIR, pid, ver)
    os.makedirs(out, exist_ok=True)
    tmp = os.path.join(out, ".apk-src")
    os.makedirs(tmp, exist_ok=True)
    built = {}
    try:
        if "openwrt" in plats:
            files = collect(pid, m, "openwrt")
            ipk = os.path.join(out, f"{PKG_PREFIX}{pid}_{ver}_all.ipk")
            build_ipk(pid, m, "openwrt", files, ipk)
            built["ipk"] = ipk
            apk = os.path.join(out, f"{PKG_PREFIX}{pid}_{ver}_noarch.apk")
            build_apk(pid, m, files, apk, tmp)
            built["apk"] = apk
        if "keenetic" in plats:
            files = collect(pid, m, "keenetic")
            kipk = os.path.join(out, f"{PKG_PREFIX}{pid}_{ver}_keenetic.ipk")
            build_ipk(pid, m, "keenetic", files, kipk)
            built["keenetic"] = kipk
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    for p in built.values():
        sign(p, do_sign)
        print(f"  {os.path.relpath(p, HERE)}")
    return m, built


def write_catalog(results, do_sign):
    if os.path.isdir(FEED_OUT):
        shutil.rmtree(FEED_OUT)
    os.makedirs(FEED_OUT)
    entries = []
    for m, built in results:
        pid = m["id"]
        # "catalog": false — плагин живёт только в репозитории (пример, черновик).
        if m.get("catalog") is False:
            print(f"  {pid}: catalog=false — в каталог не идёт")
            continue
        os.makedirs(os.path.join(FEED_OUT, pid))
        files = {}
        for kind, path in built.items():
            name = os.path.basename(path)
            dst = os.path.join(FEED_OUT, pid, name)
            shutil.copy2(path, dst)
            if os.path.isfile(path + ".sig"):
                shutil.copy2(path + ".sig", dst + ".sig")
            files[kind] = {"path": f"{pid}/{name}", "sha256": sha256(path), "size": os.path.getsize(path)}
        ui = m.get("ui", {})
        entries.append({
            "id": pid,
            "name": m["name"],
            "version": m["version"],
            "description": m.get("description", ""),
            "author": m.get("author", ""),
            "icon": m.get("icon", ""),
            "platforms": m.get("platforms", list(PLATFORMS)),
            "min_panel": m.get("min_panel", ""),
            "has_card": bool(ui.get("card")),
            "files": files,
        })
    index = os.path.join(FEED_OUT, "index.json")
    with open(index, "w", encoding="utf-8", newline="\n") as f:
        json.dump({"generated": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "plugins": entries},
                  f, ensure_ascii=False, indent=1)
    sign(index, do_sign)
    print(f"каталог: {os.path.relpath(index, HERE)} ({len(entries)} шт.)")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("ids", nargs="*", help="id плагинов (по умолчанию — все)")
    ap.add_argument("--check", action="store_true", help="только проверить манифесты и раскладку файлов")
    ap.add_argument("--no-sign", action="store_true", help="не подписывать (локальная проверка)")
    ap.add_argument("--publish", action="store_true", help="выложить каталог в ветку feed (собирает все плагины)")
    args = ap.parse_args()

    all_ids = sorted(d for d in os.listdir(PLUGINS_DIR)
                     if os.path.isfile(os.path.join(PLUGINS_DIR, d, "plugin.json")))
    ids = args.ids or all_ids
    if args.publish and args.ids:
        raise SystemExit("--publish собирает все плагины: каталог ветки заменяется целиком")
    unknown = [i for i in ids if i not in all_ids]
    if unknown:
        raise SystemExit(f"нет таких плагинов: {', '.join(unknown)}")

    try:
        if args.check:
            for pid in ids:
                m = load_manifest(pid)
                for p in m.get("platforms", list(PLATFORMS)):
                    collect(pid, m, p)
                print(f"{pid} {m['version']}: ok")
            return
        results = []
        for pid in ids:
            print(f"{pid}:")
            results.append(build_plugin(pid, not args.no_sign))
    except PluginError as e:
        raise SystemExit(f"ошибка: {e}")
    write_catalog(results, not args.no_sign)

    if args.publish:
        if args.no_sign:
            raise SystemExit("--publish без подписи не выкладывается")
        from build_feed import publish_feed
        names = ", ".join(f"{m['id']} {m['version']}" for m, _ in results)
        publish_feed(f"plugins: {names}", {"plugins": FEED_OUT})


if __name__ == "__main__":
    main()
