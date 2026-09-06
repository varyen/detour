# -*- coding: utf-8 -*-
"""Доступ к CGI панели с рабочей машины: логин и GET-действия.

Пароль берётся из routers.local.json (gitignored). Сессия кладётся рядом, в
session.txt, чтобы не логиниться на каждый запрос — файл живёт только локально.

    python3 api.py                 # напечатать токен сессии
    python3 api.py status          # ответ действия status
"""
import base64
import json
import os
import sys
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
TOKF = os.path.join(HERE, 'session.txt')

ROUTER = os.environ.get('ROUTER', '')
CONFIG = os.environ.get('ROUTERS_CONFIG', os.path.join(ROOT, 'routers.local.json'))


def cfg():
    d = json.load(open(CONFIG, encoding='utf-8'))
    name = ROUTER or d.get('default') or 'home'
    return d['routers'][name]


CFG = cfg()
BASE = os.environ.get('PANEL', 'http://%s:8080' % CFG['host'])


def login():
    body = base64.b64encode((CFG['panel_user'] + '\n' + CFG['panel_password']).encode())
    req = urllib.request.Request(BASE + '/cgi-bin/detour-api?action=login', data=body, method='POST')
    r = urllib.request.urlopen(req, timeout=20)
    tok = r.headers.get('Set-Cookie', '').split('detour_session=')[1].split(';')[0]
    open(TOKF, 'w').write(tok)
    return tok


def token():
    if os.path.exists(TOKF):
        return open(TOKF).read().strip()
    return login()


def get(action, **kw):
    q = '&'.join('%s=%s' % (k, v) for k, v in kw.items())
    url = BASE + '/cgi-bin/detour-api?action=' + action + (('&' + q) if q else '')
    req = urllib.request.Request(url, headers={'Cookie': 'detour_session=' + token()})
    return urllib.request.urlopen(req, timeout=60).read().decode('utf-8', 'replace')


if __name__ == '__main__':
    print(get(sys.argv[1]) if len(sys.argv) > 1 else token())
