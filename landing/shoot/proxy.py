# -*- coding: utf-8 -*-
"""Обезличивающий прокси перед живой панелью.

Браузер ходит на 127.0.0.1:8099, прокси — на роутер. Статика панели идёт как
есть, а ответы CGI прогоняются через карту подстановок: на кадрах нет ни имён
провайдеров, ни серверов, ни публичных адресов. Запросы в обратную сторону
переводятся из подставных id обратно в настоящие, чтобы панель оставалась
рабочей (шторки профилей, действия, фильтры).
"""
import http.server, socketserver, urllib.request, urllib.error, json, os, sys, re
import api
from anonmap import Scrubber, loadmap

UPSTREAM = os.environ.get('PANEL', 'http://192.168.8.1:8080')
PORT = int(os.environ.get('PORT', '8099'))

SC = Scrubber(loadmap())
REV = SC.reverse()
REV_RE = re.compile('|'.join(re.escape(k) for k in sorted(REV, key=len, reverse=True))) if REV else None
TOKEN = api.token()


def unfake(s):
    return REV_RE.sub(lambda m: REV[m.group(0)], s) if (REV_RE and s) else s


class H(http.server.BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *a):
        pass

    def _do(self, method):
        path = unfake(self.path)
        body = None
        n = int(self.headers.get('Content-Length') or 0)
        if n:
            raw = self.rfile.read(n)
            try:
                body = unfake(raw.decode('utf-8')).encode('utf-8')
            except UnicodeDecodeError:
                body = raw
        req = urllib.request.Request(UPSTREAM + path, data=body, method=method)
        req.add_header('Cookie', 'detour_session=' + TOKEN)
        for h in ('Accept', 'Content-Type', 'User-Agent'):
            if self.headers.get(h):
                req.add_header(h, self.headers[h])
        try:
            r = urllib.request.urlopen(req, timeout=90)
            status, hdrs, data = r.status, r.headers, r.read()
        except urllib.error.HTTPError as e:
            status, hdrs, data = e.code, e.headers, e.read()
        except Exception as e:
            self.send_response(502)
            self.send_header('Content-Type', 'text/plain; charset=utf-8')
            msg = str(e).encode()
            self.send_header('Content-Length', str(len(msg)))
            self.end_headers()
            self.wfile.write(msg)
            return

        # чистим ТОЛЬКО ответы CGI: пройтись подстановками по javascript-бандлу
        # значит переименовать в нём случайные идентификаторы и уронить панель
        if path.startswith('/cgi-bin/'):
            try:
                data = SC.scrub_text(data.decode('utf-8')).encode('utf-8')
            except UnicodeDecodeError:
                pass
        self.send_response(status)
        for k, v in hdrs.items():
            if k.lower() in ('content-length', 'transfer-encoding', 'connection', 'content-encoding'):
                continue
            self.send_header(k, v)
        self.send_header('Content-Length', str(len(data)))
        self.send_header('Cache-Control', 'no-store')
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self._do('GET')

    def do_POST(self):
        self._do('POST')

    def do_HEAD(self):
        self._do('HEAD')


class S(socketserver.ThreadingTCPServer):
    daemon_threads = True
    allow_reuse_address = True


if __name__ == '__main__':
    print('proxy http://127.0.0.1:%d → %s (map %d)' % (PORT, UPSTREAM, len(SC.fwd)), flush=True)
    S(('127.0.0.1', PORT), H).serve_forever()
