"""Прогнать НАСТОЯЩИЙ render_lighttpd из detour-portmap на образце конфига.

Keenetic-ветку проброса нельзя проверить без железа, но БОЛЬШУЮ часть её ошибок
видно раньше: они в генерируемом тексте. Поэтому функция вырезается из
`router_files/detour-portmap` как есть — не переписывается здесь, — вокруг неё
подставляются заглушки платформы, и результат печатается.

Так был найден баг, из-за которого второе имя на общем :443 убивало весь конфиг:
lighttpd сливает контексты с одинаковым условием, а `ssl.pemfile`, объявленный в
слитом контексте дважды, для него фатальная ошибка.

Проверяет ТЕКСТ, а не то, что lighttpd его примет: `lighttpd -tt` на живой
Keenetic остаётся обязательным шагом.

    python3 keenetic/render-lighttpd-test.py     # из корня репозитория
"""

import io
import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(os.path.dirname(HERE), "router_files", "detour-portmap")

src = io.open(SRC, encoding="utf-8").read()


def grab(name):
    """Тело shell-функции целиком, от заголовка до закрывающей скобки в первой колонке."""
    m = re.search(r"^%s\(\) \{.*?^\}" % re.escape(name), src, re.S | re.M)
    if not m:
        sys.exit("не найдена функция %s в %s" % (name, SRC))
    return m.group(0)


tmp = tempfile.mkdtemp().replace("\\", "/")
conf = tmp + "/portmap.conf"
out = tmp + "/detour-portmap.conf"

# Образец покрывает всё, что рендерер обязан различать: порт-режим с паролем,
# два имени на одном порту (тот самый случай слияния контекстов), выключенное
# правило и dnat (его делает iptables-хук, а не lighttpd).
io.open(conf, "w", encoding="utf-8", newline="\n").write(
    "web|1|https|8443|tcp|192.168.1.50|80|http|any|admin|1|Web|\n"
    "ha|1|vhost|443|tcp|192.168.1.60|8123|http|any|||HA|ha.example.com|0\n"
    "ha2|1|vhost|443|tcp|192.168.1.60|8123|http|any|||HA2|hass.example.com|1\n"
    "off|0|vhost|443|tcp|192.168.1.60|8123|http|any|||Off|nope.example.com|1\n"
    "ssh|1|dnat|2222|tcp|192.168.1.70|22|http|any|||SSH||\n"
)

harness = "\n".join([
    "#!/bin/sh",
    "CONF='%s'" % conf,
    "LIGHTTPD_CONF='%s'" % out,
    "LIGHTTPD_CONFD='%s'" % tmp,
    "AUTH_DIR='%s'" % tmp,
    "log() { :; }",
    "panel_restart() { :; }",
    "have_lighttpd() { return 0; }",
    "lighttpd_has_proxy() { return 0; }",
    "cert_dir() { echo '%s'; }" % tmp,
    # lighttpd на машине разработчика нет; заглушка говорит «конфиг принят»,
    # иначе рендер откатится по своей же защите и проверять будет нечего.
    "lighttpd() { return 0; }",
    grab("fld"),
    grab("render_lighttpd"),
    "touch '%s/web.htpasswd' '%s/combined.pem'" % (tmp, tmp),
    "render_lighttpd",
    'cat "$LIGHTTPD_CONF"',
])
hp = tmp + "/harness.sh"
io.open(hp, "w", encoding="utf-8", newline="\n").write(harness)

r = subprocess.run(["sh", hp], capture_output=True, text=True)
sys.stdout.write(r.stdout)
if r.stderr.strip():
    sys.stderr.write("\n[STDERR]\n" + r.stderr)

# Единственная машинная проверка: в каждом контексте сокета ssl.pemfile ровно один.
blocks = re.split(r'^\$SERVER\["socket"\]', r.stdout, flags=re.M)[1:]
bad = [b.splitlines()[0].strip() for b in blocks if b.count("ssl.pemfile") != 1]
print("---")
if bad:
    print("ОШИБКА: ssl.pemfile повторяется в контексте(ах): %s" % ", ".join(bad))
    sys.exit(1)
print("блоков сокетов: %d, ssl.pemfile в каждом ровно один — ок" % len(blocks))
sys.exit(r.returncode)
