# -*- coding: utf-8 -*-
"""Карта обезличивания для скриншотов лендинга.

Собирает реальные идентификаторы (имена и id профилей, группы, серверы, SNI,
ключи, адреса подписок) и назначает каждому правдоподобную подставную величину.
Плюс обобщённые правила на всё, что осталось: публичные IP, e-mail, MAC, UUID,
хосты личных доменов. Карта сохраняется в map.json — кадры между запусками
получаются одинаковыми.
"""
import json, re, os, hashlib

HERE = os.path.dirname(os.path.abspath(__file__))
DUMP = os.path.join(HERE, 'dump')
MAPF = os.path.join(HERE, 'map.json')

FLAG = re.compile('[\U0001F1E6-\U0001F1FF]{2}')

try:
    from api import CFG                       # хост и логин панели — из локального конфига
except Exception:                             # noqa: BLE001 — без конфига карта всё равно строится
    CFG = {}

PANEL_USER = (CFG.get('panel_user') or '').strip()


def personal_domains():
    """Домены, которые надо подменить: адрес панели и его родительский домен."""
    dom = (CFG.get('public_domain') or '').strip().lower()
    if not dom:
        return []
    out = [dom]
    parent = dom.split('.', 1)[1] if dom.count('.') > 1 else ''
    if parent:
        out.append(parent)
    return out


def local_overrides():
    """Точечные замены под конкретный роутер (файл gitignored, может не быть)."""
    p = os.path.join(HERE, 'local_overrides.json')
    if not os.path.exists(p):
        return {}
    return json.load(open(p, encoding='utf-8'))


def target_version():
    """Версия, под которую собирается лендинг, — из VERSION в корне репозитория."""
    p = os.path.join(os.path.dirname(os.path.dirname(HERE)), 'VERSION')
    return open(p, encoding='utf-8').read().strip() if os.path.exists(p) else ''


VERSION = target_version()

CITY = {
    'GR': ('Афины', 'Греция'), 'KZ': ('Алматы', 'Казахстан'), 'NL': ('Амстердам', 'Нидерланды'),
    'DE': ('Франкфурт', 'Германия'), 'CO': ('Богота', 'Колумбия'), 'SK': ('Братислава', 'Словакия'),
    'BE': ('Брюссель', 'Бельгия'), 'HU': ('Будапешт', 'Венгрия'), 'AR': ('Буэнос-Айрес', 'Аргентина'),
    'RO': ('Бухарест', 'Румыния'), 'IE': ('Дублин', 'Ирландия'), 'FI': ('Хельсинки', 'Финляндия'),
    'FR': ('Париж', 'Франция'), 'AE': ('Дубай', 'ОАЭ'), 'HK': ('Гонконг', 'Гонконг'),
    'US': ('Нью-Йорк', 'США'), 'ES': ('Мадрид', 'Испания'), 'IT': ('Милан', 'Италия'),
    'CA': ('Торонто', 'Канада'), 'MX': ('Мехико', 'Мексика'), 'UA': ('Киев', 'Украина'),
    'DK': ('Копенгаген', 'Дания'), 'MY': ('Куала-Лумпур', 'Малайзия'), 'NG': ('Лагос', 'Нигерия'),
    'PE': ('Лима', 'Перу'), 'PT': ('Лиссабон', 'Португалия'), 'GB': ('Лондон', 'Британия'),
    'RU': ('Москва', 'Россия'), 'NO': ('Осло', 'Норвегия'), 'CZ': ('Прага', 'Чехия'),
    'BR': ('Сан-Паулу', 'Бразилия'), 'KR': ('Сеул', 'Корея'), 'SE': ('Стокгольм', 'Швеция'),
    'CH': ('Цюрих', 'Швейцария'), 'AU': ('Сидней', 'Австралия'), 'SG': ('Сингапур', 'Сингапур'),
    'BG': ('София', 'Болгария'), 'TR': ('Стамбул', 'Турция'), 'IL': ('Тель-Авив', 'Израиль'),
    'JP': ('Токио', 'Япония'), 'PL': ('Варшава', 'Польша'), 'AT': ('Вена', 'Австрия'),
    'LT': ('Вильнюс', 'Литва'), 'ZA': ('Кейптаун', 'ЮАР'), 'HR': ('Загреб', 'Хорватия'),
}

GROUP_FAKE = ['Example VPN', 'Cloud Exit', 'Прокси', 'Свой сервер', 'Резерв', 'Свой VDS', 'WARP']

TYPE_LABEL = {
    'socks5': 'SOCKS5', 'http-proxy': 'HTTP-прокси', 'wireguard': 'WireGuard',
    'hysteria2': 'Hysteria2', 'vmess': 'VMess', 'trojan': 'Trojan', 'vless': 'VLESS',
}

VPN_DOMAIN = 'example-vpn.net'


def cc_of(name):
    m = FLAG.search(name or '')
    if not m:
        return None
    return ''.join(chr(ord(x) - 0x1F1E6 + 65) for x in m.group())


def slug(s):
    tr = {'а':'a','б':'b','в':'v','г':'g','д':'d','е':'e','ё':'e','ж':'zh','з':'z','и':'i','й':'y',
          'к':'k','л':'l','м':'m','н':'n','о':'o','п':'p','р':'r','с':'s','т':'t','у':'u','ф':'f',
          'х':'h','ц':'c','ч':'ch','ш':'sh','щ':'sch','ъ':'','ы':'y','ь':'','э':'e','ю':'yu','я':'ya'}
    s = ''.join(tr.get(ch, ch) for ch in s.lower())
    return re.sub(r'[^a-z0-9]+', '_', s).strip('_') or 'node'


def load(name):
    p = os.path.join(DUMP, name + '.json')
    if not os.path.exists(p):
        return None
    try:
        return json.load(open(p, encoding='utf-8'))
    except Exception:
        return None


def build():
    fwd = {}

    def put(real, fake):
        if real and isinstance(real, str) and real not in fwd and real != fake:
            fwd[real] = fake

    profiles = (load('profiles_list') or {}).get('profiles', [])

    seen_groups = []
    for p in profiles:
        g = (p.get('group') or '').strip()
        if g and g not in seen_groups:
            seen_groups.append(g)
    for i, g in enumerate(seen_groups):
        put(g, GROUP_FAKE[i] if i < len(GROUP_FAKE) else 'Группа %d' % (i + 1))

    # имена и id профилей: город/страна по флагу из имени
    used, host_of = {}, {}
    for p in profiles:
        name, pid = p.get('name') or '', p.get('id') or ''
        cc = cc_of(name)
        if cc and cc in CITY:
            city, country = CITY[cc]
            used[cc] = used.get(cc, 0) + 1
            n = used[cc]
            fake = '%s %s, %s' % (FLAG.search(name).group(), city, country) + ('' if n == 1 else ' %d' % n)
            host_of[pid] = '%s-%02d.%s' % (cc.lower(), n, VPN_DOMAIN)
        else:
            t = TYPE_LABEL.get(p.get('type'), 'Узел')
            used[t] = used.get(t, 0) + 1
            fake = '%s %02d' % (t, used[t])
            host_of[pid] = '%s-%02d.%s' % (slug(t), used[t], VPN_DOMAIN)
        put(name, fake)
        put(pid, slug(re.sub(r'[\U0001F1E6-\U0001F1FF]', '', fake)))

    # серверы, SNI, ключи — из полного экспорта
    exp = load('profiles_export') or {}
    hostmap, hidx = {}, [0]

    def fake_host(real):
        if real not in hostmap:
            hidx[0] += 1
            hostmap[real] = 'srv-%02d.%s' % (hidx[0], VPN_DOMAIN)
        return hostmap[real]

    for p in exp.get('profiles', []):
        ob = p.get('outbound') or {}
        pid = p.get('id') or ''
        base = host_of.get(pid)
        for key in ('server',):
            v = ob.get(key)
            if isinstance(v, str) and v and not re.fullmatch(r'[\d.]+', v):
                if base and v not in hostmap:
                    hostmap[v] = base
                put(v, hostmap.get(v) or fake_host(v))
        sni = ((ob.get('tls') or {}).get('server_name'))
        if isinstance(sni, str) and sni:
            put(sni, hostmap.get(sni) or (base if base else fake_host(sni)))
        for key in ('uuid', 'password', 'username'):
            v = ob.get(key)
            if isinstance(v, str) and len(v) >= 6:
                put(v, 'xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx' if key == 'uuid' else '••••••••')
        tls = ob.get('tls') or {}
        rel = tls.get('reality') or {}
        for key in ('public_key', 'short_id'):
            v = rel.get(key)
            if isinstance(v, str) and len(v) >= 6:
                put(v, 'REALITY-PUBLIC-KEY' if key == 'public_key' else '00ff')
        for peer in (ob.get('peers') or []):
            a = peer.get('address') if isinstance(peer, dict) else None
            if isinstance(a, str) and a and not re.fullmatch(r'[\d.]+', a):
                put(a, fake_host(a))

    # цепочки
    for i, c in enumerate((load('chains_list') or {}).get('chains') or []):
        put(c.get('name') or '', 'Цепочка %d' % (i + 1))
        put(c.get('id') or '', 'chain_%d' % (i + 1))

    # подписки: ответ — просто список, а имя лежит в title (не name), плюс своя
    # группа у каждой строки. Токен в самом адресе добивает общее правило.
    subs = load('subscriptions_list')
    if isinstance(subs, dict):
        subs = subs.get('subscriptions') or []
    for i, s in enumerate(subs or []):
        if not isinstance(s, dict):
            continue
        put(s.get('url') or '', 'https://sub.%s/api/v1/client/subscribe' % VPN_DOMAIN)
        for key in ('title', 'group'):
            v = (s.get(key) or '').strip()
            if v:
                put(v, 'Example VPN' if i == 0 else 'Подписка %d' % (i + 1))
        put(s.get('id') or '', 'sub_%d' % (i + 1))

    # добор: любой хост, попавший в ответы, где доменов «по делу» быть не может
    # (там только серверы провайдера, адреса подписок и наш собственный домен)
    SENSITIVE = ('subscriptions_list', 'profiles_export', 'chains_list', 'cert_status',
                 'portmap_status', 'settings', 'ping_status', 'health_status', 'geo_status')
    for name in SENSITIVE:
        p = os.path.join(DUMP, name + '.json')
        if not os.path.exists(p):
            continue
        for h in set(RE_HOST.findall(open(p, encoding='utf-8').read())):
            low = h.lower()
            if not looks_like_host(low) or low in fwd or low in KEEP_HOSTS:
                continue
            if low.endswith('example.com') or low.endswith(VPN_DOMAIN):
                continue
            if any(low == d or low.endswith('.' + d) for d in PERSONAL_DOMAINS):
                continue          # личные домены чистит обобщённое правило
            if low in hostmap:
                continue
            put(h, 'sub.%s' % VPN_DOMAIN if name == 'subscriptions_list' else fake_host(h))

    # Правки поверх карты: активный профиль, домен панели и логин, версия.
    st = load('settings') or {}
    act = st.get('active_profile') or ''
    if act:
        # id виден на схеме потока («Через VPN → …»), поэтому не слаг с городом,
        # а короткий тег — так же, как выглядят настоящие теги в конфиге
        fwd[act] = 'nl-rotterdam'
        for p in profiles:
            if p.get('id') == act and p.get('name'):
                fwd[p['name']] = '🇳🇱 Роттердам, Нидерланды'
    # Домен панели и логин берём из routers.local.json: настоящих хостов и имён
    # в трекаемом коде быть не должно (см. «Персональные данные» в CLAUDE.md).
    # Домен обязан идти в карту раньше логина — иначе «vasya» съест «vasya.ru».
    for dom in personal_domains():
        fwd[dom] = 'h.example.com' if dom.count('.') > 1 else 'example.com'
    if PANEL_USER:
        fwd[PANEL_USER] = 'admin'

    # На роутере обычно предыдущий релиз, а снимаем мы dev-сборку уже нового:
    # без подмены номер версии на кадрах спорил бы с текстом лендинга.
    installed = ((load('status') or {}).get('version') or '').strip()
    if installed and installed != VERSION:
        fwd[installed] = VERSION

    # Всё остальное личное, что встретилось именно на этом роутере (например,
    # какие сайты хозяин держит в списках), — в local_overrides.json рядом.
    fwd.update(local_overrides())

    # часть имён на роутере лежит в кракозябрах (двойная перекодировка utf-8 →
    # cp1251): на кадре это выглядит поломкой панели, хотя дело в данных
    for m in (load('portmap_status') or {}).get('mappings') or []:
        nm = m.get('name') or ''
        try:
            fixed = nm.encode('cp1251').decode('utf-8')
        except (UnicodeEncodeError, UnicodeDecodeError):
            continue
        if fixed != nm:
            fwd[nm] = fixed

    return fwd


PERSONAL_DOMAINS = personal_domains()
# хвосты, по которым RE_HOST ловит имена файлов, а не домены
NOT_TLD = {'json','log','conf','list','db','txt','sh','js','css','html','htm','png','svg','ipk',
           'sig','tmp','lock','pid','sock','md','py','lua','gz','tar','ini','yml','yaml','pem',
           'key','crt','htpasswd','cfg','bak','old','local','initd','service','so','ko','bin',
           'sample','example','tpl','env','state','pub','sec','cache','dat','idx','msg','out'}


def looks_like_host(h):
    return h.rsplit('.', 1)[-1].lower() not in NOT_TLD
KEEP_HOSTS = {'github.com', 'raw.githubusercontent.com', 'api.github.com', 'db-ip.com'}

RE_EMAIL = re.compile(r'[\w.+-]+@[\w-]+\.[\w.]+')
RE_IP = re.compile(r'\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b')
RE_MAC = re.compile(r'\b(?:[0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}\b')
RE_UUID = re.compile(r'\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b', re.I)
RE_HOST = re.compile(r'\b(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z]{2,24}\b', re.I)
# Хвост адреса подписки — это ключ доступа, и в нём бывает base64 с логином и
# почтой внутри (проверено: один из токенов расшифровывался в «you@example.com»).
# Поэтому маскируем любую длинную буквенно-цифровую последовательность —
# без подчёркиваний, чтобы не задеть имена цепочек файрвола и ключи конфига.
# Только буквы и цифры: в JSON слэши экранированы («\/»), и класс со слэшем
# съедал разделители пути целиком, ломая разметку ответа.
RE_LONGTOK = re.compile(r'(?<![A-Za-z0-9])(?=[A-Za-z0-9]*\d)[A-Za-z0-9]{14,}(?![A-Za-z0-9])')


def is_private(ip):
    o = [int(x) for x in ip.split('.')]
    if any(x > 255 for x in o):
        return True
    return (o[0] in (10, 127, 0) or (o[0] == 192 and o[1] == 168)
            or (o[0] == 172 and 16 <= o[1] <= 31) or o[0] >= 224
            or (o[0] == 169 and o[1] == 254) or (o[0] == 100 and 64 <= o[1] <= 127))


def stable(seed, mod):
    return int(hashlib.md5(seed.encode()).hexdigest(), 16) % mod


class Scrubber:
    def __init__(self, fwd):
        self.fwd = dict(fwd)
        self.dyn = {}
        keys = sorted(self.fwd, key=len, reverse=True)
        self.re_exact = re.compile('|'.join(re.escape(k) for k in keys)) if keys else None

    def reverse(self):
        out = {}
        for k, v in list(self.fwd.items()) + list(self.dyn.items()):
            out.setdefault(v, k)
        return out

    def _host(self, m):
        h = m.group(0)
        low = h.lower()
        if low in KEEP_HOSTS or low.endswith('.local') or low.endswith('.lan') or not looks_like_host(low):
            return h
        for d in PERSONAL_DOMAINS:
            if low == d or low.endswith('.' + d):
                sub = low[: -len(d) - 1]
                fake = 'example.com' if not sub else ('h.example.com' if sub in ('i', 'h') else sub + '.example.com')
                self.dyn[h] = fake
                return fake
        return h

    def _ip(self, m):
        ip = m.group(0)
        if is_private(ip):
            return ip
        fake = '203.0.113.%d' % (1 + stable(ip, 250))
        self.dyn[ip] = fake
        return fake

    def _mac(self, m):
        s = m.group(0)
        return 'aa:bb:cc:%02x:%02x:%02x' % (stable(s, 255), stable(s[::-1], 255), stable(s + 'x', 255))

    def scrub_text(self, t):
        if self.re_exact:
            t = self.re_exact.sub(lambda m: self.fwd[m.group(0)], t)
        t = RE_EMAIL.sub('you@example.com', t)
        t = RE_HOST.sub(self._host, t)
        t = RE_IP.sub(self._ip, t)
        t = RE_MAC.sub(self._mac, t)
        t = RE_UUID.sub('xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx', t)
        t = RE_LONGTOK.sub(lambda m: 'x' * min(len(m.group(0)), 24), t)
        return t


def loadmap():
    return json.load(open(MAPF, encoding='utf-8'))


if __name__ == '__main__':
    fwd = build()
    json.dump(fwd, open(MAPF, 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
    print('map entries:', len(fwd))
