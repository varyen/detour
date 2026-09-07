#!/bin/sh
# release-install.sh — install a detour package with a usign check.
#
# Понимает ОБА формата: `.ipk` (opkg — OpenWrt <= 24.10, GL.iNet) и `.apk`
# (apk-tools 3 — OpenWrt 25.12+, где opkg больше нет). Менеджер определяется на
# месте, файл берётся по подходящему расширению.
#
# Expects to find <package>.{ipk,apk} and its .sig in the same directory
# (or alongside this script). Use it when the panel is offline and you want
# to push a package over SCP/manual SSH instead of going through GH.
#
# Usage:
#     # 1) Workstation side: copy the package + signature into /tmp/ on router
#     scp releases/v1.0.11/detour_1.0.11_*.ipk* root@<router>:/tmp/
#     scp router_files/release-install.sh root@<router>:/tmp/
#     # 2) Router side:
#     ssh root@<router> 'sh /tmp/release-install.sh /tmp/detour_1.0.11_*.ipk'
#
# Options:
#     --skip-verify   skip the usign check (for emergency installs of an unsigned
#                     local build — never use against unverified upstream files)

set -u

IPK=""
SKIP_VERIFY=0
for arg in "$@"; do
    case "$arg" in
        --skip-verify) SKIP_VERIFY=1 ;;
        *.ipk|*.apk)   IPK="$arg" ;;
        *)             echo "unknown arg: $arg" >&2; exit 2 ;;
    esac
done

# Какой менеджер пакетов на этой прошивке. Каталог /etc/opkg на OpenWrt 25.12
# остаётся (пустой), а бинарника opkg нет — поэтому детект по /etc/apk/world
# (его заводит только apk) плюс наличие самого apk.
PKGM=opkg
EXT=ipk
if [ -f /etc/apk/world ] && command -v apk >/dev/null 2>&1; then
    PKGM=apk
    EXT=apk
fi

# Auto-discover: if the user didn't pass a package path, look next to this
# script — ищем расширение, подходящее ИМЕННО этой прошивке, чтобы не подсунуть
# apk-роутеру .ipk (он отказал бы невнятным «v2 package format error»).
if [ -z "$IPK" ]; then
    here=$(dirname "$0")
    IPK=$(ls -1 "$here"/detour_*."$EXT" 2>/dev/null | head -1)
    [ -n "$IPK" ] || IPK=$(ls -1 /tmp/detour_*."$EXT" 2>/dev/null | head -1)
fi

[ -f "$IPK" ] || { echo "ERROR: package not found (pass path explicitly; ждём .$EXT)" >&2; exit 1; }
case "$IPK" in
    *."$EXT") ;;
    *) echo "ERROR: пакетный менеджер здесь $PKGM — нужен файл .$EXT, а передан $IPK" >&2; exit 1 ;;
esac

SIG="$IPK.sig"
OPKG_KEYRING=/etc/opkg/keys
PINNED_KEY=/etc/detour/release.usign.pub

if [ "$SKIP_VERIFY" = "1" ]; then
    echo "[release-install] WARNING: --skip-verify set, no signature check"
else
    [ -f "$SIG" ] || { echo "ERROR: signature missing: $SIG" >&2; exit 1; }
    # Three trust-anchor priorities:
    #   1) /etc/opkg/keys/ (full keyring scan via -P)
    #   2) /etc/detour/release.usign.pub (pinned single key)
    #   3) public key embedded in the package itself (TOFU first install).
    if [ -d "$OPKG_KEYRING" ] && [ -n "$(ls -1 "$OPKG_KEYRING" 2>/dev/null)" ]; then
        usign -V -m "$IPK" -P "$OPKG_KEYRING" -x "$SIG" \
            || { echo "ERROR: usign verification failed against keyring" >&2; exit 1; }
        echo "[release-install] usign OK (keyring=$OPKG_KEYRING)"
    elif [ -f "$PINNED_KEY" ]; then
        usign -V -m "$IPK" -p "$PINNED_KEY" -x "$SIG" \
            || { echo "ERROR: usign verification failed against pinned key" >&2; exit 1; }
        echo "[release-install] usign OK (pinned=$PINNED_KEY)"
    else
        # TOFU: pull the public key out of the package's data.tar.gz/etc/detour/release.usign.pub
        # and trust THAT, then verify against it. Only safe on a clean first install where
        # the operator manually validated the .ipk integrity over a side channel.
        echo "[release-install] TOFU: extracting bundled public key from the package"
        tofu_dir=$(mktemp -d)
        if [ "$PKGM" = apk ]; then
            # APKv2 — склейка gzip-потоков, дерево файлов лежит прямо в архиве
            # (без вложенного data.tar.gz), поэтому ключ достаётся одним шагом.
            ( cd "$tofu_dir" && tar -xzf "$IPK" etc/detour/release.usign.pub ) \
                || { rm -rf "$tofu_dir"; echo "ERROR: cannot extract embedded pubkey" >&2; exit 1; }
        else
            ( cd "$tofu_dir" && tar -xzf "$IPK" ./data.tar.gz \
                && tar -xzf ./data.tar.gz ./etc/detour/release.usign.pub ) \
                || { rm -rf "$tofu_dir"; echo "ERROR: cannot extract embedded pubkey" >&2; exit 1; }
        fi
        usign -V -m "$IPK" -p "$tofu_dir/etc/detour/release.usign.pub" -x "$SIG" \
            || { rm -rf "$tofu_dir"; echo "ERROR: usign verification failed (TOFU)" >&2; exit 1; }
        rm -rf "$tofu_dir"
        echo "[release-install] usign OK (TOFU)"
    fi
fi

echo "[release-install] installing $IPK via $PKGM ..."
if [ "$PKGM" = apk ]; then
    # Подписи ВНУТРИ пакета нет (доверие строится на открепленной usign-.sig,
    # её проверили выше) — отсюда --allow-untrusted.
    apk add --allow-untrusted "$IPK" || { echo "ERROR: apk add failed" >&2; exit 1; }
else
    opkg install "$IPK" || { echo "ERROR: opkg install failed" >&2; exit 1; }
fi

# Report installed version
if [ "$PKGM" = apk ]; then
    INSTALLED=$(apk list -I detour 2>/dev/null \
        | awk '{ n=$1; if (substr(n,1,7) != "detour-") next
                 v=substr(n,8); sub(/-r[0-9]+$/,"",v); print v; exit }')
else
    INSTALLED=$(opkg list-installed detour 2>/dev/null | awk '{print $3}')
fi
echo "[release-install] installed: detour $INSTALLED"
LAN_IP=$(uci get network.lan.ipaddr 2>/dev/null || echo "<router-ip>")
echo "[release-install] panel: http://$LAN_IP:8080/detour/"
