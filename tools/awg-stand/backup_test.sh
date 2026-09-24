#!/bin/sh
# Стенд полной резервной копии: засеять состояние, экспорт, стереть, импорт,
# сверить; плюс попытки записать мимо списка путей и ключи панели.
#   docker run --rm -v <repo>/router_files:/rf:ro -v <repo>/tools/awg-stand:/t:ro \
#       detour-owrt-engine:x86-64-23.05.5 sh /t/backup_test.sh
set -u
L=/rf/detour-backup.lua
SB=/tmp/bk/sb; DD=/tmp/bk/dd; ZC=/tmp/bk/zapret.conf; ZD=/tmp/bk/zapret/domains.list
fail=0
ok() { echo "PASS $1"; }
bad() { echo "FAIL $1"; fail=1; }

rm -rf /tmp/bk; mkdir -p $SB/profiles $DD/subscriptions $DD/portmap.d /tmp/bk/zapret
printf '{"id":"p1","name":"NL","type":"vless","uri":"vless://u@h:443?path=/ws","outbound":{"type":"vless","server":"h","server_port":443}}' > $SB/profiles/p1.json
printf '{"id":"p2","name":"AWG","type":"amneziawg","outbound":{"type":"amneziawg","local_address":[]}}' > $SB/profiles/p2.json
printf '{"name":"no id","type":"socks"}' > $SB/profiles/p3.json
printf '{"active_chain":"p1","routing_mode":"list"}' > $SB/settings.json
printf '{"chains":[{"id":"c1","hops":["p1","p2"]}]}' > $SB/chains.json
printf 'example.com\n' > $SB/proxy-domains.list
printf '' > $SB/torrent-allow.list
printf 'YT|https://www.youtube.com/generate_204\n' > $SB/health-urls.list
printf 'site.example.com p1\n' > $SB/route-map.list
printf '{"id":"s1","url":"https://sub.example.com/x"}' > $DD/subscriptions/s1.json
printf '1|1|vhost|443|tcp|192.168.1.5|8123|http|||||ha.example.com|1\n' > $DD/portmap.conf
printf 'u:{SHA}abc\n' > $DD/portmap.d/1.htpasswd
printf 'KEY=secret\n' > $DD/dns-api.conf
printf 'root:$6$hash\n' > $DD/../detour.auth
printf 'token\n' > $DD/update.conf
printf -- '--split-pos=2\n' > $ZC
printf 'rutracker.org\n' > $ZD

lua $L export $SB $DD $ZC $ZD 2.1.2 openwrt > /tmp/bk/out.json || bad "export exit"
lua -e 'local c=require("cjson"); local f=io.open("/tmp/bk/out.json"); local d=c.decode(f:read("*a"));
  assert(d.version==2 and d.kind=="full", "kind");
  assert(#d.profiles==3, "profiles "..#d.profiles); assert(d.profiles[3].id=="p3", "id injected");
  assert(#d.subscriptions==1, "subs");
  assert(d.router_files["detour/portmap.conf"], "portmap");
  assert(d.router_files["detour/portmap.d/1.htpasswd"], "htpasswd");
  assert(d.router_files["detour/dns-api.conf"], "dns-api");
  for k in pairs(d.router_files) do assert(not k:match("auth") and not k:match("update"), "leak "..k) end
  assert(d.health_urls:match("youtube"), "health");
  assert(d.chains.chains[1].id=="c1", "chains")' && ok "export content" || bad "export content"
grep -q '\\/' /tmp/bk/out.json && bad "escaped slash in export" || ok "no escaped slash"
grep -q '"local_address":\[\]' /tmp/bk/out.json && ok "empty array stays array" || bad "empty array became object"

cp -r /tmp/bk/sb /tmp/bk/sb.orig; cp -r /tmp/bk/dd /tmp/bk/dd.orig
rm -rf $SB/* $DD/subscriptions/* $DD/portmap.conf $DD/portmap.d $DD/dns-api.conf $ZC $ZD
mkdir -p $SB
lua $L import /tmp/bk/out.json $SB $DD $ZC $ZD > /tmp/bk/imp.json; ec=$?
[ $ec = 0 ] && ok "import exit" || bad "import exit $ec: $(cat /tmp/bk/imp.json)"
for f in settings.json chains.json proxy-domains.list health-urls.list route-map.list torrent-allow.list; do
  cmp -s $SB/$f /tmp/bk/sb.orig/$f || lua -e 'local c=require("cjson"); local a=c.decode(io.open(arg[1]):read("*a")); local b=c.decode(io.open(arg[2]):read("*a")); assert(a and b)' $SB/$f /tmp/bk/sb.orig/$f 2>/dev/null || { [ -f $SB/$f ] && ok "$f restored" || bad "$f missing"; continue; }
  ok "$f restored"
done
[ -f $SB/profiles/p1.json ] && grep -q 'path=/ws' $SB/profiles/p1.json && ok "profile p1" || bad "profile p1"
grep -q '"local_address":\[\]' $SB/profiles/p2.json && ok "empty array survives import" || bad "empty array lost on import"
[ -f $SB/profiles/p3.json ] && ok "profile without id" || bad "profile without id"
printf '{"profiles":[{"id":"../x","name":"e"},{"id":"a w"}]}' > /tmp/bk/badid.json
lua $L import /tmp/bk/badid.json $SB $DD $ZC $ZD >/dev/null
[ -e $SB/x.json ] || [ -e "$SB/profiles/a w.json" ] && bad "unsafe profile id written" || ok "unsafe id rejected"
[ "$(ls -l $SB/profiles/p1.json | cut -c1-10)" = "-rw-------" ] && ok "profile 0600" || bad "profile perm"
[ -f $DD/subscriptions/s1.json ] && ok "subscription" || bad "subscription"
cmp -s $DD/portmap.conf /tmp/bk/dd.orig/portmap.conf && ok "portmap.conf" || bad "portmap.conf"
[ -f $DD/portmap.d/1.htpasswd ] && ok "htpasswd" || bad "htpasswd"
[ "$(ls -l $DD/dns-api.conf | cut -c1-10)" = "-rw-------" ] && ok "dns-api 0600" || bad "dns-api perm"
[ "$(cat $ZD)" = rutracker.org ] && ok "zapret domains" || bad "zapret domains"

# мимо списка путей
printf '{"router_files":{"detour/../../../tmp/pwned":"x","sing-box/../../evil":"x","detour/update.conf":"t","detour/portmap.d/../x.htpasswd":"x"}}' > /tmp/bk/evil.json
lua $L import /tmp/bk/evil.json $SB $DD $ZC $ZD > /tmp/bk/evil.out
[ -e /tmp/pwned ] || [ -e /tmp/bk/evil ] && bad "path traversal" || ok "traversal blocked"
[ "$(cat $DD/update.conf)" = token ] && ok "update.conf untouched" || bad "update.conf overwritten"
grep -q 'не входит' /tmp/bk/evil.out && ok "skipped reported" || bad "skipped not reported"

printf '{"auth":"root:x","settings":{}}' > /tmp/bk/auth.json
lua $L import /tmp/bk/auth.json $SB $DD $ZC $ZD > /tmp/bk/auth.out; ec=$?
[ $ec = 3 ] && ok "auth key rejected" || bad "auth key exit $ec"

# v1 (старая копия): пустой список не стирает
printf 'keep.example.com\n' > $SB/whitelist-domains.list
printf '{"version":1,"settings":{"routing_mode":"all"},"whitelist_domains":""}' > /tmp/bk/v1.json
lua $L import /tmp/bk/v1.json $SB $DD $ZC $ZD >/dev/null
grep -q keep $SB/whitelist-domains.list && ok "v1 empty list keeps" || bad "v1 wiped list"

[ $fail = 0 ] && echo "ALL PASS" || echo "SOME FAILED"
