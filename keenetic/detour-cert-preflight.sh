#!/bin/sh
# detour-cert-preflight.sh — Keenetic/Entware preflight for issuing a Let's Encrypt
# cert for the Detour panel (HTTP-01 webroot + WAN :80 → lighttpd forward).
#
# READ-ONLY: it gathers facts and prints a verdict + the exact next commands. The only
# thing it writes is a transient self-test token under the panel docroot (removed
# immediately). It changes NOTHING else — no packages, no firewall, no certs.
#
# Run it FIRST on the Keenetic over SSH:
#     sh detour-cert-preflight.sh [ВАШ.ДОМЕН] [ВАШ@EMAIL]
# (domain/email optional — if given, it also checks the A-record and fills the ready-
#  to-run issue commands at the end.)
#
# Then act on the "ИТОГО" section: install anything missing, add the two port-forwards,
# and run the staging issue (the real end-to-end test). Full guide: VERIFY-cert-http01.md.

DOMAIN="$1"
EMAIL="$2"

export PATH="/opt/bin:/opt/sbin:/usr/bin:/usr/sbin:/bin:/sbin"

FAIL=0
WARN=0
ok()     { echo "[OK]    $*"; }
warn()   { echo "[WARN]  $*"; WARN=$((WARN+1)); }
fail()   { echo "[FAIL]  $*"; FAIL=$((FAIL+1)); }
info()   { echo "[INFO]  $*"; }
action() { echo "[ДЕЙСТВИЕ] $*"; }
hr()     { echo "------------------------------------------------------------"; }

echo "============================================================"
echo " Detour · preflight выпуска HTTPS-сертификата (Keenetic)"
echo "============================================================"

# ---- platform ----
hr; echo "1. Платформа"
if [ -d /opt/etc/ndm ] || [ -f /opt/etc/detour/platform ]; then
    ok "KeeneticOS/Entware обнаружен (/opt)."
else
    warn "Не похоже на Keenetic (нет /opt/etc/ndm). Скрипт рассчитан на Keenetic."
fi

# ---- detour-cert + panel package ----
hr; echo "2. Панель Detour и detour-cert"
VER=$(opkg list-installed 2>/dev/null | sed -n 's/^detour-keenetic - \(.*\)/\1/p' | head -1)
[ -n "$VER" ] && info "detour-keenetic: $VER" || warn "пакет detour-keenetic не найден в opkg."
if [ -x /opt/sbin/detour-cert ]; then
    ok "/opt/sbin/detour-cert присутствует."
else
    fail "/opt/sbin/detour-cert НЕТ — обнови панель до 1.12.0+ (detour-keenetic_1.12.0_all.ipk)."
fi

# ---- dependencies ----
hr; echo "3. Зависимости"
if command -v curl >/dev/null 2>&1; then ok "curl есть ($(curl --version 2>/dev/null | head -1 | awk '{print $1, $2}'))."; else fail "нет curl → opkg install curl"; fi
if command -v openssl >/dev/null 2>&1; then ok "openssl есть ($(openssl version 2>/dev/null))."; else fail "нет openssl → opkg install openssl-util"; fi

ACME=""
for p in /opt/.acme.sh/acme.sh /root/.acme.sh/acme.sh "$HOME/.acme.sh/acme.sh"; do
    [ -x "$p" ] && { ACME="$p"; break; }
done
[ -z "$ACME" ] && ACME=$(command -v acme.sh 2>/dev/null)
if [ -n "$ACME" ]; then ok "acme.sh: $ACME ($("$ACME" --version 2>/dev/null | grep -i v | head -1))."
else warn "acme.sh не найден — detour-cert поставит сам при выпуске (opkg install acme.sh / официальный установщик)."; fi

LBIN=/opt/sbin/lighttpd; [ -x "$LBIN" ] || LBIN=/opt/bin/lighttpd
if [ -x "$LBIN" ]; then ok "lighttpd: $LBIN"; else fail "lighttpd не найден (нет панели?)."; fi
if opkg list-installed 2>/dev/null | grep -qi 'lighttpd-mod-openssl'; then
    ok "lighttpd-mod-openssl установлен (нужен для HTTPS на панели)."
else
    warn "НЕТ lighttpd-mod-openssl → для HTTPS-сокета панели: opkg install lighttpd-mod-openssl"
fi
command -v socat >/dev/null 2>&1 && info "socat есть (не требуется — используем webroot, не standalone)." || info "socat нет — это нормально (нужен только для standalone, мы им не пользуемся)."

# ---- lighttpd port ----
hr; echo "4. Порт lighttpd панели"
LP=$(sed -n 's/.*server\.port[^0-9]*\([0-9][0-9]*\).*/\1/p' /opt/etc/lighttpd/detour.conf 2>/dev/null | head -1)
[ -n "$LP" ] || LP=8080
info "lighttpd слушает HTTP на :$LP (сюда будет проброшен внешний :80)."

# ---- port occupancy ----
hr; echo "5. Кто слушает порты"
for P in 80 443 "$LP" 8443; do
    L=$(netstat -tlnp 2>/dev/null | grep -E "[:.]$P " | head -1)
    if [ -n "$L" ]; then
        WHO=$(echo "$L" | grep -oE '[0-9]+/[^ ]+' | head -1)
        [ -n "$WHO" ] || WHO=$(echo "$L" | awk '{print $NF}')
        info ":$P занят → $WHO"
    elif netstat -tln 2>/dev/null | grep -qE "[:.]$P "; then
        # busybox netstat без -p (имя процесса недоступно) — порт всё равно занят
        info ":$P занят (процесс не определён — netstat без -p)"
    else
        info ":$P свободен"
    fi
done
echo "  (Если :443 занят KeeneticOS — для панели возьми :8443 и пробрось WAN :443 → :8443.)"

# ---- panel alive ----
hr; echo "6. Панель отвечает?"
CODE=$(curl -s -o /dev/null -w '%{http_code}' -m 5 "http://127.0.0.1:$LP/detour/" 2>/dev/null)
if [ "$CODE" = "200" ] || [ "$CODE" = "301" ] || [ "$CODE" = "302" ]; then
    ok "http://127.0.0.1:$LP/detour/ → $CODE"
else
    fail "панель не отвечает на :$LP (код '$CODE'). Запусти: /opt/etc/init.d/S51detour-panel start"
fi

# ---- KEY: lighttpd serves .well-known static ----
hr; echo "7. КЛЮЧЕВОЕ: lighttpd отдаёт challenge-статику?"
WR=/opt/share/www
TOK="detour-preflight-$$"
TOKFILE="$WR/.well-known/acme-challenge/$TOK"
mkdir -p "$WR/.well-known/acme-challenge" 2>/dev/null
if echo "$TOK" > "$TOKFILE" 2>/dev/null && [ -s "$TOKFILE" ]; then
    GOT=$(curl -s -m 5 "http://127.0.0.1:$LP/.well-known/acme-challenge/$TOK" 2>/dev/null)
    rm -f "$TOKFILE" 2>/dev/null
    if [ "$GOT" = "$TOK" ]; then
        ok "lighttpd отдаёт /.well-known/acme-challenge/ — HTTP-01 сработает (challenge сервится локально)."
    else
        fail "lighttpd НЕ отдаёт challenge-статику (получили '$GOT'). Без этого выпуск невозможен — пришли вывод этого блока."
    fi
else
    rm -f "$TOKFILE" 2>/dev/null
    # Запись не удалась — это НЕ вина lighttpd; не путаем пользователя «не отдаёт статику».
    fail "не удалось записать тест-токен в $WR/.well-known/acme-challenge/ — проверь права и место (Read-Only FS флешки?)."
fi

# ---- public IP + WAN ----
hr; echo "8. Публичный IP и WAN"
# LAN-IP роутера (цель проброса) — приватный адрес моста (обычно 192.168.1.1 на Keenetic),
# НЕ внешний адрес `ip route get` (тот — WAN-сторона).
# Сначала спросим Keenetic про сегмент Home (иначе при поднятом VPN можно схватить
# адрес туннеля 10.8.0.x); затем мост br0; затем — первый приватный IPv4 как фолбэк.
LANIP=""
if command -v ndmc >/dev/null 2>&1; then
    LANIP=$(ndmc -c 'show interface Home' 2>/dev/null | grep -iE '^[[:space:]]*address:' | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | head -1)
fi
[ -n "$LANIP" ] || LANIP=$(ip -4 addr show dev br0 2>/dev/null | grep -oE 'inet [0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | awk '{print $2}' | head -1)
[ -n "$LANIP" ] || LANIP=$(ip -4 addr show 2>/dev/null | grep -oE 'inet (192\.168\.[0-9]+\.[0-9]+|10\.[0-9]+\.[0-9]+\.[0-9]+|172\.(1[6-9]|2[0-9]|3[01])\.[0-9]+\.[0-9]+)' | awk '{print $2}' | head -1)
[ -n "$LANIP" ] || LANIP="<LAN-IP-роутера, напр. 192.168.1.1>"
info "LAN-IP роутера (цель проброса): $LANIP"
# -k: на свежем Entware ещё нет ca-certificates → без него curl упадёт на верификации
# TLS и внешний IP не определится. Нам нужен лишь отражённый IP, доверие к эхо-сервису
# не важно.
PUBIP=$(curl -k -s -m 8 https://api.ipify.org 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | head -1)
[ -n "$PUBIP" ] || PUBIP=$(curl -k -s -m 8 https://ifconfig.me 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | head -1)
if [ -n "$PUBIP" ]; then
    info "Внешний (публичный) IP роутера: $PUBIP"
    case "$PUBIP" in
        10.*|192.168.*|172.1[6-9].*|172.2[0-9].*|172.3[01].*|100.6[4-9].*|100.[7-9][0-9].*|100.1[01][0-9].*|100.12[0-7].*)
            warn "Похоже на СЕРЫЙ IP (NAT/CGNAT) — HTTP-01 снаружи не достучится. Нужен публичный IP или DNS-01." ;;
    esac
else
    warn "не удалось определить внешний IP (нет интернета на запрос?)."
fi
if command -v ndmc >/dev/null 2>&1; then
    info "WAN-интерфейсы (для проброса выбери тот, что 'connected' с публичным адресом):"
    ndmc -c 'show interface' 2>/dev/null | grep -iE 'interface, name|description|address:|connected:|role' | sed 's/^/    /' | head -40
else
    info "ndmc недоступен из этой оболочки — пробросы делай через веб-интерфейс KeeneticOS."
fi

# ---- existing forwards ----
hr; echo "9. Уже настроенные пробросы (NAT static)"
if command -v ndmc >/dev/null 2>&1; then
    RULES=$(ndmc -c 'show ip static' 2>/dev/null)
    if echo "$RULES" | grep -qE '(^|[^0-9])80([^0-9]|$)'; then info "найдено правило, упоминающее порт 80:"; echo "$RULES" | grep -iE 'port|80|443|interface' | sed 's/^/    /' | head -20
    else warn "правил проброса :80 не видно — их нужно добавить (Шаг ниже)."; fi
else
    info "ndmc недоступен — проверь пробросы в веб-интерфейсе (Сетевые правила → Переадресация портов)."
fi

# ---- A-record (if domain given) ----
hr; echo "10. A-запись домена"
if [ -n "$DOMAIN" ]; then
    # busybox/glibc nslookup печатают ответ как "Address" сразу ПОСЛЕ строки
    # "Name: <домен>"; IP резолвера живёт в верхней секции Server:/Address: (без "Name:").
    # Якоримся на "Name:" → берём следующий Address — иначе при внешнем DNS (8.8.8.8 и т.п.)
    # схватили бы IP DNS-сервера вместо A-записи.
    RES=$(nslookup "$DOMAIN" 2>/dev/null | awk '
        /^[Nn]ame:/ { ans=1; next }
        ans && /[Aa]ddress/ { print; exit }
    ' | grep -oE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' | head -1)
    if [ -n "$RES" ]; then
        info "$DOMAIN → $RES"
        if [ -n "$PUBIP" ] && [ "$RES" = "$PUBIP" ]; then ok "A-запись совпадает с публичным IP роутера."
        elif [ -n "$PUBIP" ]; then fail "A-запись ($RES) НЕ совпадает с публичным IP роутера ($PUBIP) — LE придёт не туда."; fi
    else
        warn "не удалось разрешить $DOMAIN (A-записи нет?)."
    fi
else
    info "домен не передан — пропускаю (запусти 'sh $0 ВАШ.ДОМЕН ВАШ@EMAIL' для проверки A-записи)."
fi

# ============================ ИТОГО ============================
hr
echo "ИТОГО:  FAIL=$FAIL  WARN=$WARN"
hr
if [ "$FAIL" -eq 0 ]; then
    echo "Локально всё готово. Осталось снаружи: A-запись на $PUBIP + два проброса портов."
else
    echo "Есть блокеры (FAIL) — устрани их (см. строки [FAIL] выше) и прогони скрипт снова."
fi
echo
echo "СЛЕДУЮЩИЕ ШАГИ (Keenetic):"
echo
action "1) Пробросы портов (веб-интерфейс KeeneticOS → Сетевые правила → Переадресация портов):"
echo "       • вход TCP 80  → этот роутер ($LANIP), порт назначения $LP   (для ACME-challenge)"
echo "       • вход TCP 443 → этот роутер ($LANIP), порт назначения 443   (или 8443, если :443 занят)"
echo "     либо через ndmc (подставь имя WAN-интерфейса из Шага 8 вместо ISP):"
echo "       ndmc -c \"ip static tcp ISP 80 $LANIP $LP !detour-acme\""
echo "       ndmc -c \"ip static tcp ISP 443 $LANIP 443 !detour-https\""
echo "       ndmc -c \"system configuration save\""
echo
if ! opkg list-installed 2>/dev/null | grep -qi 'lighttpd-mod-openssl'; then
action "2) Поставь TLS-модуль для панели:  opkg install lighttpd-mod-openssl"
echo
fi
action "3) СТЕЙДЖИНГ-выпуск (безопасно, без лимитов — это и есть сквозная проверка проброса):"
if [ -n "$DOMAIN" ] && [ -n "$EMAIL" ]; then
echo "       DETOUR_CERT_STAGING=1 /opt/sbin/detour-cert issue $DOMAIN $EMAIL"
else
echo "       DETOUR_CERT_STAGING=1 /opt/sbin/detour-cert issue ВАШ.ДОМЕН ВАШ@EMAIL"
fi
echo "       /opt/sbin/detour-cert status ; tail -25 /opt/var/log/detour-cert.log"
echo "     Успех = status: \"last_result\":\"ok\".  Провал 'Verify error 404/timeout' = проброс :80 не работает."
echo
action "4) Перед боевым — снеси staging-данные домена, затем боевой выпуск:"
echo "       rm -rf /opt/.acme.sh/ВАШ.ДОМЕН_ecc /root/.acme.sh/ВАШ.ДОМЕН_ecc 2>/dev/null"
echo "       /opt/sbin/detour-cert issue ВАШ.ДОМЕН ВАШ@EMAIL"
echo
action "5) Проверка: открой https://ВАШ.ДОМЕН/detour/ → ⚙ → Push-уведомления → тест."
echo
echo "Подробности и разбор ошибок — keenetic/VERIFY-cert-http01.md."
