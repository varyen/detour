# Release signing keys

The project signs releases with **usign(1) Ed25519**, the standard OpenWrt
signature format. Two key files live here:

- `release.usign.pub` — public key, committed to the repo and shipped inside
  every `.ipk` as both `etc/detour/release.usign.pub` and
  `etc/opkg/keys/<keynum>`.
- `release.usign.sec` — secret key, **gitignored**. Stays only on the build
  machine. `chmod 0600`.

Current key fingerprint (keynum): **2253f33757fa7939**.

## Generation (one-time)

`usign` ships with every OpenWrt build, so the cleanest way is to generate
the keypair on a router and download it:

```sh
ssh root@<router> 'usign -G -s /tmp/sec -p /tmp/pub -c "Your release key" && \
    cat /tmp/pub /tmp/sec && rm -f /tmp/sec /tmp/pub'
# Split the output into keys/release.usign.{pub,sec} on the build machine.
chmod 0600 keys/release.usign.sec
```

If you have `usign` available locally (LEDE/OpenWrt build host, or compiled
manually), the call is the same: `usign -G -s release.usign.sec -p release.usign.pub`.

A Python implementation of the signature format lives at the repo root
([../usign_compat.py](../usign_compat.py)). It can sign/verify and is
cross-validated end-to-end against the router's `usign -V`.

## Manual sign / verify

```sh
# Sign with Python (build_release.py uses this internally):
python3 -c "from usign_compat import sign_file; sign_file('my.ipk', 'keys/release.usign.sec')"

# Sign with usign (anywhere usign is installed):
usign -S -m my.ipk -s keys/release.usign.sec -x my.ipk.sig

# Verify on the router:
usign -V -m my.ipk -p /etc/detour/release.usign.pub -x my.ipk.sig
# (or scan the opkg keyring)
usign -V -m my.ipk -P /etc/opkg/keys -x my.ipk.sig
```

## Key rotation

1. Generate a new keypair.
2. Ship a release that includes the new `release.usign.pub` at the standard
   paths (the build script does this automatically: it pins the public key in
   both `/etc/detour/release.usign.pub` and
   `/etc/opkg/keys/<new-keynum>`).
3. Sign that release with the **old** key so existing routers will trust the
   transition.
4. Once every fleet member has the release, future builds can sign with the
   new key. Remove the old keynum file from `etc/opkg/keys/` in a follow-up
   release.

## Why usign + .ipk?

Releases are standard OpenWrt building blocks rather than a bespoke format:

- `.ipk` makes the package installable straight from LuCI's
  `System → Software → Upload Package` UI.
- `opkg` handles file replacement, dep resolution, version registry, and
  postinst — we no longer reinvent those.
- `usign` is the signature format every OpenWrt router already understands;
  no extra OpenSSL plumbing on the router side.
