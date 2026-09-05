"""Проверить нумерацию слотов маршрутов и отлов «осиротевших» целей.

Вырезает НАСТОЯЩИЕ функции из `router_files/sing-box.initd` и гоняет их на
синтетической карте маршрутов, где одна цель — профиль, вторая — цепочка, а у
третьей цели нет ни того, ни другого.

Смысл: цель, которую `route_map_slots` пропускает, раньше означала, что её адреса
молча уходят НАПРЯМУЮ мимо VPN. Тест фиксирует два инварианта:
  * пропущенная цель попадает в список `route_map_orphans` (её адреса будут
    отклоняться, а не утекать);
  * пропуск не сдвигает нумерацию уцелевших целей — номер задаёт и ipset, и порт
    редиректа, и разъезд этих двух слоёв уже приводил к утечке (v1.42.0).

    python3 tools/routemap-slots-test.py     # из корня репозитория
"""

import io
import os
import re
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(os.path.dirname(HERE), "router_files", "sing-box.initd")
src = io.open(SRC, encoding="utf-8").read()


def grab(name):
    m = re.search(r"^%s\(\) \{.*?^\}" % re.escape(name), src, re.S | re.M)
    if not m:
        sys.exit("не найдена функция %s в %s" % (name, SRC))
    return m.group(0)


tmp = tempfile.mkdtemp().replace("\\", "/")
os.makedirs(tmp + "/profiles")
io.open(tmp + "/profiles/germany.json", "w").write("{}")
io.open(tmp + "/chains.json", "w", encoding="utf-8").write(
    '[{"id":"de_then_warp","name":"DE → WARP"}]'
)
io.open(tmp + "/route-map.list", "w", encoding="utf-8", newline="\n").write(
    "// === route: germany ===\n"
    "api.ipify.org\n"
    "1.2.3.4\n"
    "// === route: de_then_warp ===\n"
    "gemini.google.com\n"
    "// === route: deleted_profile ===\n"
    "secret.example.com\n"
    "// === route: empty_section ===\n"
)

harness = "\n".join([
    "#!/bin/sh",
    "ROUTE_MAP='%s/route-map.list'" % tmp,
    "CHAINS_FILE='%s/chains.json'" % tmp,
    "CONFIG_DIR='%s'" % tmp,
    "ROUTE_PORT_BASE=12400",
    grab("route_map_targets"),
    grab("route_map_section"),
    grab("chain_ids"),
    grab("route_target_exists"),
    grab("route_map_slots"),
    grab("route_map_orphans"),
    'echo "--slots--"; route_map_slots',
    'echo "--orphans--"; route_map_orphans',
])
hp = tmp + "/harness.sh"
io.open(hp, "w", encoding="utf-8", newline="\n").write(harness)

r = subprocess.run(["sh", hp], capture_output=True, text=True)
if r.stderr.strip():
    sys.stderr.write(r.stderr)
out = r.stdout
slots = out.split("--slots--")[1].split("--orphans--")[0].strip().splitlines()
orphans = out.split("--orphans--")[1].strip().splitlines()

print("слоты:")
for line in slots:
    print("   ", line)
print("осиротевшие цели:", ", ".join(orphans) or "(нет)")

fail = []
if [l.split() for l in slots] != [
    ["1", "germany", "12401", "singbox_t1"],
    ["2", "de_then_warp", "12402", "singbox_t2"],
]:
    fail.append("нумерация слотов разъехалась: цель без профиля не должна сдвигать остальные")
if orphans != ["deleted_profile"]:
    fail.append("осиротевшая цель не распознана: её адреса ушли бы напрямую")

print("---")
if fail:
    for f in fail:
        print("ОШИБКА:", f)
    sys.exit(1)
print("нумерация устойчива к пропавшей цели, пропавшая цель поймана — ок")
