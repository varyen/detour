# Detour на обычном Linux-сервере: OpenWrt 25.12 в контейнере.
#
# Весь код Detour рассчитан на OpenWrt, и переносить его на systemd/apt без
# uci, procd и fw4 — это вторая копия каждой фичи. Вместо этого сервер получает
# настоящий OpenWrt userland в собственном сетевом пространстве контейнера:
# Detour ставится тем же .apk, что и на роутер с OpenWrt 25.12, а его файрвол
# (fw4, iptables, ipset) живёт только внутри контейнера и не трогает файрвол
# хоста. Ядро общее с хостом, поэтому нужные модули netfilter грузит
# install.sh на хосте, а AmneziaWG — userspace (amneziawg-go): модуля
# amneziawg в ядрах дистрибутивов нет.
#
#   docker build -t detour-box linux/                     (x86_64)
#   docker build --build-arg OWRT=armsr-armv8-25.12.0 \
#                -t detour-box linux/                     (arm64)
#
# Обычно образ собирает и запускает install.sh.

ARG OWRT=x86-64-25.12.0

# --- AmneziaWG: userspace-реализация и утилита awg, обе статически ----------
FROM golang:1.25-alpine AS awg-go
RUN apk add --no-cache git \
 && git clone --depth 1 https://github.com/amnezia-vpn/amneziawg-go /src \
 && cd /src && CGO_ENABLED=0 go build -trimpath -ldflags "-s -w" -o /amneziawg-go .

FROM alpine:3.20 AS awg-tools
RUN apk add --no-cache git build-base linux-headers \
 && git clone --depth 1 https://github.com/amnezia-vpn/amneziawg-tools /src \
 && make -C /src/src LDFLAGS=-static WITH_WGQUICK=no WITH_BASHCOMPLETION=no WITH_SYSTEMDUNITS=no \
 && strip /src/src/wg && cp /src/src/wg /awg

# --- sing-box для входа VLESS-Reality ---------------------------------------
# Reality-сервер sing-box 1.13.x (родной фид OpenWrt 25.12) отвергает всех
# клиентов; detour-server берёт для входа VLESS отдельный sing-box-vless, а
# пакет прошивки не трогает. Статический -musl: в OpenWrt нет glibc.
FROM alpine:3.20 AS sb-vless
ARG SINGBOX=1.14.3
RUN apk add --no-cache curl \
 && case "$(uname -m)" in x86_64) a=amd64 ;; aarch64) a=arm64 ;; *) exit 1 ;; esac \
 && curl -fsSL "https://github.com/SagerNet/sing-box/releases/download/v$SINGBOX/sing-box-$SINGBOX-linux-$a-musl.tar.gz" \
    | tar -xz -C /tmp \
 && cp "/tmp/sing-box-$SINGBOX-linux-$a-musl/sing-box" /sing-box-vless

# --- сам Detour -------------------------------------------------------------
FROM openwrt/rootfs:${OWRT}
ARG DETOUR_REPO=varyen/detour

COPY --from=awg-go /amneziawg-go /usr/bin/amneziawg-go
COPY --from=awg-tools /awg /usr/bin/awg
COPY --from=sb-vless /sing-box-vless /usr/bin/sing-box-vless
# Пакет можно подложить рядом (detour_*_noarch.apk в контексте сборки) — тогда
# сборка не ходит за ним на GitHub. Иначе берётся последний релиз. Глоб с
# detour-box-init в одной строке: COPY с глобом, не совпавшим ни с чем, падает.
COPY detour-box-init detour_*_noarch.ap[k] /root/
RUN mkdir -p /var/lock /tmp \
 && mv /root/detour-box-init /sbin/detour-box-init \
 && apk update \
 && apk add ca-bundle curl ip-full \
 && set -- /root/detour_*_noarch.apk \
 && if [ ! -f "$1" ]; then \
        tag=$(curl -fsSL "https://api.github.com/repos/$DETOUR_REPO/releases/latest" \
              | sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -1) \
     && curl -fsSL -o "/root/detour_${tag}_noarch.apk" \
            "https://github.com/$DETOUR_REPO/releases/download/v$tag/detour_${tag}_noarch.apk" \
     && set -- "/root/detour_${tag}_noarch.apk"; \
    fi \
 && apk add --allow-untrusted "$1" \
 && rm -f /root/detour_*.apk \
 && chmod 755 /sbin/detour-box-init /usr/bin/awg /usr/bin/amneziawg-go /usr/bin/sing-box-vless
# postinst запускает первичную установку sing-box и tpws в фоне — при сборке
# фон умирает вместе со слоем, поэтому повторяем её на переднем плане.
RUN /usr/sbin/detour-bootstrap-install >/dev/null 2>&1; \
    test -x /usr/bin/sing-box && test -x /usr/bin/tpws-zapret

EXPOSE 80/tcp 51820/udp
ENTRYPOINT ["/sbin/detour-box-init"]
