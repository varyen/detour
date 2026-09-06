# -*- coding: utf-8 -*-
"""Выгрузка ответов CGI в dump/ — по ним anonmap.py строит карту подстановок.

Берутся только читающие действия. Часть из них на GET отвечает «POST required» —
это нормально, такие ответы просто не пригодятся.

    python3 capture.py
"""
import os

import api

ACTIONS = """
status settings profiles_list profiles_export chains_list subscriptions_list route_map domains
whitelist zapret_domains zapret_config singbox_config hosts_get hosts_custom_get health_status
health_urls ping_status autocheck_status egress_blocklist udp_vpn udp_vpn_list lan_clients
iptables log_config rulist_exclude portmap_status cert_status geo_status traffic_counters
traffic_series updates_overview conntrack keepalive_status bins_update_status panel_update_status
""".split()

HERE = os.path.dirname(os.path.abspath(__file__))
DUMP = os.path.join(HERE, 'dump')

if __name__ == '__main__':
    os.makedirs(DUMP, exist_ok=True)
    for a in ACTIONS:
        try:
            t = api.get(a)
        except Exception as e:                      # noqa: BLE001 — интересен сам факт
            t = 'ERR ' + str(e)
        open(os.path.join(DUMP, a + '.json'), 'w', encoding='utf-8').write(t)
        print('%-22s %7d  %s' % (a, len(t), t[:56].replace('\n', ' ')))
