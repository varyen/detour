# -*- coding: utf-8 -*-
"""Проверка обезличивания: прогнать выгрузки через карту и поискать остатки.

Ищем то, чего на кадрах быть не должно: настоящие публичные адреса, длинные
токены (в них прячется base64 с почтой и логином) и ключи карты, которые почему-то
не заменились. Пустой вывод — то, что нужно.

    python3 verify.py
"""
import json
import os

from anonmap import RE_IP, RE_LONGTOK, Scrubber, is_private, loadmap

HERE = os.path.dirname(os.path.abspath(__file__))
DUMP = os.path.join(HERE, 'dump')

if __name__ == '__main__':
    sc = Scrubber(loadmap())
    problems = 0
    for f in sorted(os.listdir(DUMP)):
        raw = open(os.path.join(DUMP, f), encoding='utf-8').read()
        s = sc.scrub_text(raw)

        toks = [t for t in RE_LONGTOK.findall(s) if set(t) != {'x'}]
        ips = [i for i in RE_IP.findall(s) if not is_private(i) and not i.startswith('203.0.113.')]
        try:
            json.loads(s)
            broken = None
        except ValueError as e:
            broken = str(e)[:60]

        if toks or ips or broken:
            problems += 1
            print(f)
            if toks:
                print('   токены:', toks[:5])
            if ips:
                print('   адреса:', sorted(set(ips))[:5])
            if broken:
                print('   ответ перестал быть JSON:', broken)

    print('готово, файлов с замечаниями:', problems)
