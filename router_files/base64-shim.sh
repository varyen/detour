#!/bin/sh
# Tiny base64 wrapper for systems where busybox base64 applet is absent
# (e.g. some MediaTek-targeted OpenWrt 21.02 builds).
# Uses openssl base64 under the hood. Translates `base64 [FILE]` and
# `base64 -d [FILE]` to the openssl `-in FILE` equivalent.
decode=0
case "$1" in
    -d|--decode|-D) decode=1; shift ;;
    -w) shift; shift ;;  # -w <cols>: openssl always wraps at 64, ignore value
esac

if [ -n "$1" ] && [ "$1" != "-" ]; then
    if [ "$decode" = "1" ]; then
        exec openssl base64 -d -A -in "$1"
    else
        exec openssl base64 -in "$1"
    fi
fi

if [ "$decode" = "1" ]; then
    exec openssl base64 -d -A
else
    exec openssl base64
fi
