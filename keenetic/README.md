# Detour on Keenetic KN-1810 (KeeneticOS + Entware) — port blueprint

> Status: **device-independent scaffold**, not yet validated on hardware.
> Everything marked **⚠ VALIDATE** must be checked over SSH on a real KN-1810
> before it can be trusted. We have no physical device yet — this is built from
> Entware/KeeneticOS documentation and community practice.

## Target

| | |
|---|---|
| Model | Keenetic KN-1810 ("Titan"/"Ultra") |
| SoC | MediaTek **MT7621AT**, dual-core MIPS 1004Kc @ 880 MHz |
| ABI | **mipsel, soft-float** (no FPU), little-endian, 32-bit |
| RAM / flash | 256 MB / 128 MB (so heavy bins go on USB, not flash) |
| OS | **KeeneticOS** (NDM) — **not** OpenWrt |
| Package env | **Entware** mounted at `/opt` (USB 3.0 stick) — repo `mipselsf-k3.4` |

The big difference from the GL.iNet target: KeeneticOS is proprietary (NDM), so
there is no `uci`, no procd `/etc/init.d`, no nftables `fw4`, no `uhttpd`, no
`/www`. Everything lives under `/opt` (Entware) and integrates with NDM via
hook directories. There is also **no MPTCP/QSDK bug** here, so the
`net.mptcp.enabled=0` workaround is not needed.

## Binaries

**sing-box comes from the Entware feed, not bundled.** The `mipsel-3.4` feed has
`sing-box-go` (Provides: `sing-box`, currently 1.13.3, installs to
`/opt/bin/sing-box`). The detour package just declares `Depends: sing-box`, so
opkg downloads it (~18 MB) built for the exact arch — which **eliminates the
float-ABI risk** (no `-softfloat` guesswork; the feed build is correct by
construction). `opkg upgrade` keeps it current.

**tpws is bundled** (~127 KB) — zapret/`tpws`/`nfqws` are NOT in the Entware feed,
so `fetch-bins.py` still pulls `tpws` from bol-van/zapret `binaries/linux-mipsel/tpws`
(ELF32 LE MIPS) into `keenetic/bins/`. (It also fetches sing-box, now unused by the
package — harmless.)

⚠ VALIDATE on device: `/opt/bin/sing-box version` must not say `Error relocating`
(it won't if pulled from the feed), and `tpws --help` runs.

## Path layout (all under /opt — survives KeeneticOS firmware updates on the USB volume)

| Purpose | GL.iNet (OpenWrt) | KN-1810 (Entware) |
|---|---|---|
| sing-box bin | `/usr/bin/sing-box` | `/opt/sbin/sing-box` |
| tpws bin | `/usr/bin/tpws-zapret` | `/opt/sbin/tpws-zapret` |
| sing-box cfg | `/etc/sing-box/` | `/opt/etc/sing-box/` |
| zapret cfg | `/etc/zapret-tpws.conf`, `/etc/zapret-tpws/` | `/opt/etc/zapret-tpws.conf`, `/opt/etc/zapret-tpws/` |
| panel state/auth | `/etc/detour/`, `/etc/detour.auth` | `/opt/etc/detour/`, `/opt/etc/detour.auth` |
| services | `/etc/init.d/{sing-box,zapret-tpws}` (procd) | `/opt/etc/init.d/S52detour-singbox`, `S53detour-zapret` |
| panel web | `/www/detour/`, `/www/cgi-bin/detour-api` (uhttpd) | `/opt/share/www/detour/`, `/opt/share/www/cgi-bin/detour-api` (lighttpd) |
| firewall | nftables `fw4` rules | iptables rules re-applied via `/opt/etc/ndm/netfilter.d/` |
| log | `/var/log/sing-box.log` | `/opt/var/log/sing-box.log` |
| sessions | `/tmp/detour-sessions` | `/tmp/detour-sessions` (RAM, same) |

Runtime config knobs in `/opt/etc/detour/detour.conf` (LAN_IF, ports, log path) so
the init.d/hook scripts stay device-agnostic.

## Component approach

### 1. Entware bootstrap (`entware-bootstrap.sh`)
`opkg update && opkg install` the deps: `iptables ipset dnsmasq-full lighttpd
lighttpd-mod-cgi lighttpd-mod-setenv lua lua-cjson coreutils-base64 openssl-util
curl start-stop-daemon`. (Entware `mipselsf-k3.4` has all of these.)

### 2. Services (`init.d/S52detour-singbox`, `S53detour-zapret`)
Entware `rc.unslung` runs `/opt/etc/init.d/S*` on boot. Plain `start-stop-daemon`
with a pidfile (no procd). `start` also drops an enable-marker
(`/opt/etc/detour/<svc>.enabled`) and applies the firewall hook; `stop` removes
the marker + rules.

### 3. Transparent proxy / firewall (`ndm/netfilter.d/50-detour.sh`)
KeeneticOS rebuilds iptables on every reconfig and calls scripts in
`/opt/etc/ndm/netfilter.d/`. Our hook (idempotently, gated by the enable-markers):
- `nat PREROUTING`: ipset `zapret_domains` → `REDIRECT :1081`; sing-box-mode TCP → `REDIRECT :12345`
- `filter INPUT`: accept tcp/8080 from LAN (panel)

⚠ VALIDATE: the exact NDM hook interface (args vs env `$type`/`$table`), and the LAN
bridge name (assumed `br0`).

### 4. DNS + ipset
KeeneticOS owns port 53. To populate `singbox_domains`/`zapret_domains` ipsets from
DNS like on OpenWrt, run **Entware dnsmasq-full** and make Keenetic use it. Two device
options (⚠ VALIDATE which Keenetic allows):
- point Keenetic's upstream/`ndmq` DNS at `127.0.0.1#<entware-dnsmasq-port>`, or
- run Entware dnsmasq on `:53` and disable Keenetic's resolver.
dnsmasq config uses the same `ipset=/domain/singbox_domains` lines we generate today.

### 5. Web panel (`lighttpd/detour.conf`)
A dedicated lighttpd instance on `:8080` (KeeneticOS web UI stays on `:80`),
doc-root `/opt/share/www/detour`, `/cgi-bin/` → `/opt/share/www/cgi-bin` via mod_cgi.
The **CGI itself needs a platform shim**: a header that detects Entware
(`[ -d /opt/etc/init.d ]`) and switches INITD path, the firewall-apply command
(`nft …` → the netfilter.d hook + `iptables`), and all `/etc|/www|/usr/bin` paths to
their `/opt` equivalents. (Not yet written — most device-dependent piece.)

### 6. Deploy/release tooling
Add `platform: "keenetic"` to the router entry in `routers.local.json`;
`deploy_router.py` branches to a Keenetic path (Entware install instead of uci/opkg-system).
Bins ship as a separate mipsel channel (or an Entware `mipselsf` opkg `.ipk`).

## Test package (hand to a tester with a real KN-1810)

`python keenetic/build-ipk.py` → `releases/keenetic/detour-keenetic_<ver>_all.ipk`
(Entware-installable, mipsel bins + panel + init.d/netfilter.d/lighttpd under /opt,
arch `all` so opkg side-loads it; `Depends` pulls the Entware deps).

On the router (Entware must already be installed):
```
opkg update
opkg install ./detour-keenetic_<ver>_all.ipk
# panel: http://<router-ip>:8080/detour/   login: admin / detour  (CHANGE IT)
```
**What works in this build (v1.4.0 — parity pass, ⚠ all UNVALIDATED on hardware):**
panel + login, sing-box/zapret start-stop, explicit IP/CIDR redirect, and now also:
- **Domain→ipset routing** — `S50detour-dns` runs an Entware dnsmasq on
  `:$DETOUR_DNS_PORT` (5354) that tags resolved IPs into the ipsets via generated
  `ipset=/domain/...` config; `50-detour.sh` transparently REDIRECTs LAN `:53` to
  it. Same domain lists as OpenWrt. (single-instance; route-map targets all funnel
  through the one sing-box, which splits them via route.rules.)
- **Hosts-DNS** — `detour-hosts` shipped; serves `addn-hosts=/tmp/hosts` via the
  detour dnsmasq; re-materialized at boot by `S51detour-panel`.
- **«Все через VPN»** — re-asserted by `50-detour.sh` from the `allvpn.enabled`
  marker, so it survives NDM firewall rebuilds.
- **VPN road-warrior redirect** — `vpn_redirect_ifaces` honored by the hook.
- **Self-update** — `detour-update` has a `/opt` shim; pulls the
  `detour-keenetic_*.ipk` asset, `opkg install` (skips the feed/sing-box ensure;
  usign check skipped if `usign` is absent on Entware). Needs `GH_TOKEN` in
  `/opt/etc/detour/update.conf`.
- **Subscriptions / keep-alive** — `subscription-refresh` + `vpn-keepalive` shipped.
- **Scheduler** — `/opt/sbin/detour-cron` + `S90detour-cron` run the periodic jobs
  (keep-alive, sub-refresh, 6h update auto-check) as an init.d-launched daemon
  loop. KeeneticOS kills the shell `crond` spawns for a job, so cron silently
  never fires; the daemon sidesteps it entirely (same session model as the panel /
  proxy daemons). Tunable via `DETOUR_CRON_TICK`; update check honors `AUTO_CHECK`.

**⚠ VALIDATE (new device-dependent assumptions in this pass):**
- `/opt/sbin/dnsmasq` (dnsmasq-full) exists and supports `ipset=`.
- KeeneticOS does not itself force-redirect/intercept client `:53` (some firmwares
  do — would collide with our transparent `:53` REDIRECT).
- `DETOUR_DNS_UPSTREAM` (default `1.1.1.1`): while redirected, clients lose
  KeeneticOS local-name resolution — set a preferred resolver in `detour.conf`.
- Entware crond scheduling for `vpn-keepalive`/`subscription-refresh`/auto-check
  cron (paths are `/opt`, but how cron is registered on KeeneticOS varies).
- `start-stop-daemon` keeps the daemons up; `xt_set`/`ipset` available in the
  KeeneticOS iptables.

**Please report back (this is our remote Phase-0 validation):**
`uname -m`; `opkg print-architecture`; `/opt/sbin/sing-box version` (must NOT say
`Error relocating`); LAN bridge name (`ip -o link`, expect br0); whether the panel
loads + login works; `ls /opt/etc/ndm/netfilter.d/` honored after a reboot;
any errors in `/opt/var/log/`.

## Validate-on-device checklist (Phase 0 when SSH is available)
- `uname -m`, KeeneticOS version, `opkg print-architecture`
- Entware present + mount point, USB free space (≥200 MB for sing-box)
- `./sing-box version`, `./tpws --help` run without `Error relocating`
- LAN bridge name (`ip a`, expect `br0`), iptables present, ipset present
- NDM hook dirs exist: `/opt/etc/ndm/netfilter.d/`, `fw.d/`, `ifstatechanged.d/`
- how SSH is exposed (dropbear via Entware? KeeneticOS CLI?), file transfer (base64 vs sftp)
