// Эталоны для sharelink.rs из самого uri.ts: парсеры панели и службы обязаны
// давать одинаковый outbound. Запуск:
//   node --experimental-strip-types client/scripts/gen-sharelink-fixtures.mjs
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const uri = await import(new URL("../../panel/src/components/profiles/uri.ts", import.meta.url));

const b64 = (s) => Buffer.from(s, "utf8").toString("base64");
const b64url = (s) => b64(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const U = "00000000-0000-0000-0000-000000000001";

const vmess = (o) => "vmess://" + b64(JSON.stringify(o));

const links = [
  `vless://${U}@vpn.example.com:443?security=reality&sni=www.example.com&fp=chrome&pbk=PUBKEY123&sid=ab12&type=tcp&flow=xtls-rprx-vision#%F0%9F%87%B3%F0%9F%87%B1%20NL%20Reality`,
  `vless://${U}@vpn.example.com:8443?security=tls&type=ws&path=%2Fws%3Fed%3D2048&host=cdn.example.com&sni=cdn.example.com&alpn=h2,http/1.1#WS`,
  `vless://${U}@[2001:db8::1]:443?security=tls&type=grpc&serviceName=grpcsvc#IPv6%20gRPC`,
  `vless://${U}@vpn.example.com:443?security=reality&sni=www.example.com&fp=chrome&pbk=K&sid=&type=xhttp&path=%2Fx#xhttp`,
  `VLESS://${U}@vpn.example.com:443?security=none#Upper`,
  `vless://${U}@vpn.example.com:443/?security=tls&type=h2&path=%2Fh2&host=h2.example.com#H2`,
  `trojan://p%40ss+word@vpn.example.com:443?sni=vpn.example.com&allowInsecure=1#Trojan+DE`,
  `trojan://pw@vpn.example.com:443?security=none&type=ws&path=%2Ft#Trojan%20WS`,
  vmess({ v: "2", ps: "Москва VMess", add: "vpn.example.com", port: "443", id: U, aid: "0", net: "ws", path: "/v", host: "h.example.com", tls: "tls", sni: "" }) + "#junk",
  vmess({ v: 2, ps: "gRPC", add: "vpn.example.com", port: 8443, id: U, aid: 64, net: "grpc", path: "svc", tls: "", fp: "firefox", alpn: "h2" }),
  `ss://${b64url("aes-256-gcm:pass")}@vpn.example.com:8388#SS`,
  `ss://${b64("chacha20-ietf-poly1305:secret@vpn.example.com:8389")}#Legacy`,
  `ss://2022-blake3-aes-128-gcm:a2V5@vpn.example.com:443/?plugin=obfs#Plain`,
  `hysteria2://pass@vpn.example.com:8443?sni=vpn.example.com&obfs=salamander&obfs-password=ob&insecure=1&alpn=h3&fp=chrome#HY2`,
  `hy2://pass@vpn.example.com?sni=x.example.com#hy2%20default%20port`,
  `tuic://${U}:pa%3Ass@vpn.example.com:443?congestion_control=bbr&alpn=h3&sni=vpn.example.com&allow_insecure=1#TUIC`,
  `socks5://user:pw@proxy.example.com:1080#SOCKS`,
  `socks://proxy.example.com:1080`,
  `https://user:pw@proxy.example.com:443#HTTPS%20proxy`,
  `http://proxy.example.com:3128`,
  `wireguard://key@vpn.example.com:51820#WG`,
  `vmess://not-base64!!`,
  `just text`,
];

const cases = links.map((l) => {
  const d = uri.parseShareLink(l);
  return d ? { uri: l, name: d.name, outbound: uri.outboundFromDraft(d) } : { uri: l };
});

const out = fileURLToPath(new URL("../crates/core/tests/sharelinks.json", import.meta.url));
writeFileSync(out, JSON.stringify(cases, null, 2) + "\n");
console.log(`${cases.length} эталонов → ${out}`);
