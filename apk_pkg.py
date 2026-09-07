#!/usr/bin/env python3
"""Сборка пакетов формата **APKv2** на чистом Python (без apk-tools/abuild).

Зачем. OpenWrt 25.12 переехал с opkg на apk-tools 3: `apk add` на нашем `.ipk`
падает с `v2 package format error`, потому что внутри `.ipk` лежат
`debian-binary` + `control.tar.gz` + `data.tar.gz`, а apk ждёт совсем другое.
Собирать пакеты чужим тулом нельзя — сборка идёт с Windows и из CI, apk-tools
там нет. Поэтому пишем формат сами; он простой и полностью документирован
исходниками apk-tools (`src/extract_v2.c`, `src/tar.c`).

Почему v2, а не v3 (ADB). apk-tools 3 умеет ставить оба, а v3 — это бинарный
формат ADB со схемами и блоками, повторять который в Python дорого и хрупко.
v2 — три (у нас два) склеенных gzip-потока с обычными tar внутри. Проверено на
живом `apk-tools 3.0.2` из образа `openwrt/rootfs:x86-64-25.12.0`.

Устройство файла (конкатенация НЕЗАВИСИМЫХ gzip-потоков, apk режет по их
границам):

    [gzip] control-сегмент : tar с `.PKGINFO` и maintainer-скриптами
    [gzip] data-сегмент    : tar с деревом файлов пакета

Сегмент подписи (`.SIGN.RSA.<ключ>`) не пишем: apk умеет читать пакет и без
него, а наша цепочка доверия и так строится на usign — рядом с пакетом лежит
открепленная `.apk.sig`, которую роутер проверяет тем же пиннингованным ключом,
что и `.ipk.sig`. Ставится такой пакет с `--allow-untrusted` (ровно как opkg у
нас ставится с выключенным check_signature).

Три требования формата, о которые легко разбиться (все проверены на железе):

1. `.PKGINFO` ОБЯЗАН содержать `datahash = <sha256 сжатого data-сегмента>`.
   Без него `apk_pkg_read` возвращает `-APKE_V2PKG_FORMAT` — тот самый
   «v2 package format error».
2. Каждый обычный файл в data-tar обязан нести pax-заголовок
   `APK-TOOLS.checksum.SHA1 = <hex sha1 содержимого>`. Иначе распаковка каждого
   файла падает с «file format is obsolete (e.g. missing embedded checksum)» —
   пакет при этом «ставится», скрипты выполняются, а файлов нет.
3. `arch` для платформонезависимого пакета в OpenWrt — `noarch`, НЕ `all`
   (как в opkg). С `all` apk молча объявляет пакет `uninstallable`.
"""
import gzip
import hashlib
import io
import os
import tarfile
import time

# Имена maintainer-скриптов в control-сегменте (apk_script_type в apk-tools).
# Ключ — как их называем мы, значение — имя файла внутри tar.
APK_SCRIPTS = (
    "pre-install",
    "post-install",
    "pre-upgrade",
    "post-upgrade",
    "pre-deinstall",
    "post-deinstall",
    "trigger",
)

# Платформонезависимая архитектура в терминах apk/OpenWrt.
NOARCH = "noarch"


def _tar_entry(tf, name, content, mode, typ=tarfile.REGTYPE, mtime=0, link=""):
    ti = tarfile.TarInfo(name)
    ti.type = typ
    ti.mode = mode
    ti.uid = ti.gid = 0
    ti.uname = ti.gname = "root"
    ti.mtime = mtime
    ti.linkname = link
    if typ == tarfile.REGTYPE:
        ti.size = len(content)
        # Требование №2: apk проверяет sha1 каждого файла из pax-заголовка.
        ti.pax_headers = {"APK-TOOLS.checksum.SHA1": hashlib.sha1(content).hexdigest()}
        tf.addfile(ti, io.BytesIO(content))
    else:
        ti.size = 0
        tf.addfile(ti)


def _gzip_stream(raw):
    """Один самостоятельный gzip-поток (mtime=0 — сборка воспроизводима)."""
    out = io.BytesIO()
    with gzip.GzipFile(fileobj=out, mode="wb", compresslevel=9, mtime=0) as f:
        f.write(raw)
    return out.getvalue()


def _strip_tar_terminator(raw):
    """Убрать хвост из нулевых блоков (аналог `abuild-tar --cut`).

    Каждый сегмент — отдельный gzip-поток, и apk считает его границей конец
    потока, а не нулевые блоки tar. abuild режет хвост, чтобы сегменты
    склеивались встык; повторяем поведение, чтобы не расходиться со штатными
    пакетами.
    """
    while raw.endswith(b"\0" * 512):
        raw = raw[:-512]
    return raw


def build_data_segment(file_entries, mtime):
    """data-сегмент: gzip(tar с деревом файлов). Возвращает (bytes, installed_size).

    `file_entries` — список `(абсолютный путь источника, путь в пакете, режим)`.
    Пути внутри apk идут БЕЗ ведущего `./` (в отличие от `.ipk`). Каталоги
    выписываем явно и по возрастанию глубины: без них распаковка в
    несуществующий каталог падает.
    """
    dirs = set()
    for _src, dest_rel, _mode in file_entries:
        parts = dest_rel.strip("/").split("/")
        for i in range(1, len(parts)):
            dirs.add("/".join(parts[:i]))

    installed = 0
    buf = io.BytesIO()
    # PAX — обязательно: в USTAR некуда положить APK-TOOLS.checksum.SHA1.
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.PAX_FORMAT) as tf:
        for d in sorted(dirs):
            _tar_entry(tf, d + "/", b"", 0o755, tarfile.DIRTYPE, mtime)
        for src, dest_rel, mode in sorted(file_entries, key=lambda e: e[1]):
            with open(src, "rb") as f:
                content = f.read()
            installed += len(content)
            _tar_entry(tf, dest_rel.strip("/"), content, mode, tarfile.REGTYPE, mtime)
    return _gzip_stream(buf.getvalue()), installed


def build_control_segment(pkginfo_text, scripts, mtime):
    """control-сегмент: gzip(tar с `.PKGINFO` и `.<script>`), хвост срезан."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.USTAR_FORMAT) as tf:
        _tar_entry(tf, ".PKGINFO", pkginfo_text.encode("utf-8"), 0o644,
                   tarfile.REGTYPE, mtime)
        for name in APK_SCRIPTS:
            body = scripts.get(name)
            if body is None:
                continue
            if isinstance(body, str):
                body = body.encode("utf-8")
            _tar_entry(tf, "." + name, body, 0o755, tarfile.REGTYPE, mtime)
    return _gzip_stream(_strip_tar_terminator(buf.getvalue()))


def render_pkginfo(*, pkgname, pkgver, arch, description, url, license_, maintainer,
                   depends, datahash, installed_size, builddate, provides=(),
                   replaces=(), origin=None):
    lines = [
        "# Generated by apk_pkg.py (detour)",
        "pkgname = %s" % pkgname,
        "pkgver = %s" % pkgver,
        "pkgdesc = %s" % description,
        "url = %s" % url,
        "builddate = %d" % builddate,
        "packager = %s" % maintainer,
        "size = %d" % installed_size,
        "arch = %s" % arch,
        "origin = %s" % (origin or pkgname),
        "license = %s" % license_,
        # Требование №1: без datahash apk отвергает пакет целиком.
        "datahash = %s" % datahash,
    ]
    for d in depends:
        lines.append("depend = %s" % d)
    for p in provides:
        lines.append("provides = %s" % p)
    for r in replaces:
        lines.append("replaces = %s" % r)
    return "\n".join(lines) + "\n"


def build_apk(out_path, *, pkgname, version, release=0, arch=NOARCH, description,
              url, license_="MIT", maintainer, depends=(), file_entries,
              scripts=None, provides=(), replaces=(), builddate=None):
    """Собрать `.apk` формата APKv2. Возвращает (путь, installed_size).

    `version` — наша семантическая версия (`1.57.0`), `release` — apk-ревизия
    (`-r0`): apk сравнивает версии как `1.57.0-r0`, поэтому суффикс обязателен,
    иначе `apk upgrade` не увидит разницы между сборками одной версии.
    """
    scripts = scripts or {}
    unknown = set(scripts) - set(APK_SCRIPTS)
    if unknown:
        raise ValueError("неизвестные apk-скрипты: %s" % ", ".join(sorted(unknown)))

    mtime = int(builddate if builddate is not None else time.time())

    data_seg, installed_size = build_data_segment(file_entries, mtime)
    datahash = hashlib.sha256(data_seg).hexdigest()

    pkginfo = render_pkginfo(
        pkgname=pkgname,
        pkgver="%s-r%d" % (version, release),
        arch=arch,
        description=description,
        url=url,
        license_=license_,
        maintainer=maintainer,
        depends=list(depends),
        datahash=datahash,
        installed_size=installed_size,
        builddate=mtime,
        provides=list(provides),
        replaces=list(replaces),
    )
    control_seg = build_control_segment(pkginfo, scripts, mtime)

    with open(out_path, "wb") as f:
        f.write(control_seg)
        f.write(data_seg)
    return out_path, installed_size


def parse_depends(depends_str):
    """`"lua, lua-cjson, curl"` → `["lua", "lua-cjson", "curl"]`.

    Принимает ту же строку, что уходит в `Depends:` пакета opkg, чтобы список
    зависимостей жил в одном месте на оба формата.
    """
    out = []
    for chunk in (depends_str or "").split(","):
        dep = chunk.strip()
        if dep:
            out.append(dep)
    return out
