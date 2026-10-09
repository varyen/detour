"""Заглушка RCI KeeneticOS (localhost:79) для стенда detour-server.

Помнит интерфейсы WireguardN и их пиров; на `up` создаёт dummy-интерфейс nwgN,
`show interface` отдаёт пиров с фиктивными счётчиками. Команды пишутся в
/tmp/rci.log. Без файла /tmp/no-wg компонент wireguard «установлен».
"""
import http.server, json, os, re, socket, subprocess

IFS = {}

def err(msg):
    return {"status": [{"status": "error", "code": "7405600", "ident": "Command::Base", "message": msg}]}

def ok(msg=""):
    return {"status": [{"status": "message", "code": "0", "ident": "Command::Base", "message": msg}]}

def comps():
    return "base,opkg,ssh" + ("" if os.path.exists("/tmp/no-wg") else ",wireguard")

def run(cmd):
    with open("/tmp/rci.log", "a") as f:
        f.write(cmd + "\n")
    w = cmd.split()
    if cmd == "show version":
        return {"release": "5.01.C.7.0-0", "title": "5.1.7", "ndw": {"components": comps()}}
    if w[:2] == ["show", "interface"] and len(w) == 3:
        i = IFS.get(w[2])
        if not i:
            return err('unable to find "%s"' % w[2])
        peers = [{"public-key": k, "endpoint": {"address": "203.0.113.7", "port": 40000},
                  "rxbytes": 1000 * (n + 1), "txbytes": 5000 * (n + 1), "last-handshake": 12, "online": True}
                 for n, k in enumerate(i["peers"])]
        return {"id": w[2], "state": "up" if i["up"] else "down", "wireguard": {"public-key": i.get("key", ""),
                "listen-port": i.get("port", 0), "peer": peers}}
    if w[:2] == ["no", "interface"]:
        if w[2] in IFS:
            subprocess.call(["ip", "link", "del", "nwg" + w[2][9:]])
            del IFS[w[2]]
            return ok("interface removed")
        return err("no such interface")
    if w[0] == "interface" and re.match(r"Wireguard\d+$", w[1]):
        name = w[1]
        i = IFS.setdefault(name, {"peers": {}, "up": False})
        rest = w[2:]
        if not rest:
            return ok()
        if rest == ["up"]:
            dev = "nwg" + name[9:]
            subprocess.call(["ip", "link", "add", dev, "type", "dummy"], stderr=subprocess.DEVNULL)
            subprocess.call(["ip", "link", "set", dev, "up"])
            if i.get("addr"):
                subprocess.call(["ip", "addr", "add", i["addr"], "dev", dev], stderr=subprocess.DEVNULL)
            i["up"] = True
            return ok()
        if rest[0] == "ip" and rest[1] == "address":
            n = sum(bin(int(x)).count("1") for x in rest[3].split("."))
            i["addr"] = "%s/%d" % (rest[2], n)
            return ok()
        if rest[0] == "wireguard":
            if rest[1] == "asc":
                if len(rest) != 11 or not all(x.isdigit() for x in rest[2:]):
                    return err("argument parse error")
                return ok()
            if rest[1] == "private-key":
                i["key"] = "server"
                return ok()
            if rest[1] == "listen-port":
                i["port"] = int(rest[2])
                return ok()
            if rest[1] == "peer":
                p = i["peers"].setdefault(rest[2], {})
                if len(rest) > 3:
                    p[rest[3]] = rest[4:]
                return ok()
        return ok()
    if cmd in ("system configuration save",):
        return ok()
    return ok()

class H(http.server.BaseHTTPRequestHandler):
    def _send(self, obj):
        b = json.dumps(obj, indent=2).encode()
        self.send_response(200); self.send_header("Content-Type", "application/json"); self.end_headers()
        self.wfile.write(b)
    def do_GET(self):
        if self.path == "/rci/show/version":
            return self._send(run("show version"))
        self._send(err("unknown path"))
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if isinstance(body, list):
            return self._send([{"parse": run(x["parse"])} for x in body])
        self._send({"parse": run(body["parse"])})
    def log_message(self, *a):
        pass

class S(http.server.ThreadingHTTPServer):
    address_family = socket.AF_INET6

S(("::", 79), H).serve_forever()
