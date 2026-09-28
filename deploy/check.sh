#!/usr/bin/env bash
# Проверка развёрнутого стенда снаружи, через nginx — так, как его
# увидит эксперт по ссылке.
#
#   deploy/check.sh                        # https://127.0.0.1:WEB_TLS_PORT
#   deploy/check.sh https://<адрес>        # с другой машины
#
# Ничего не меняет: только входит учебной учётной записью и читает.
set -uo pipefail

base="${1:-https://127.0.0.1:${WEB_TLS_PORT:-443}}"
fail=0
ok()  { printf '  ✓ %s\n' "$1"; }
bad() { printf '  ✗ %s\n' "$1"; fail=1; }

echo "проверяю ${base}"

code=$(curl -ks -o /dev/null -w '%{http_code}' "${base}/")
[ "$code" = "200" ] && ok "клиент открывается" || bad "клиент: HTTP ${code}"

health=$(curl -kfsS "${base}/api/admin/health" 2>/dev/null || true)
if [ -n "$health" ]; then
  ok "сервер жив: ${health}"
else
  bad "сервер не отвечает на /api/admin/health"
fi

token=$(curl -ks -X POST "${base}/api/auth/login" \
  -H 'content-type: application/json' \
  -d '{"login":"student1","password":"arm112"}' \
  | python3 -c 'import sys,json; print(json.load(sys.stdin).get("token",""))' 2>/dev/null || true)
[ -n "$token" ] && ok "вход учебной учётной записью" || bad "вход не удался"

if [ -n "$token" ]; then
  auth="authorization: Bearer ${token}"

  code=$(curl -ks -o /dev/null -w '%{http_code}' "${base}/api/cards?scope=service" -H "$auth")
  [ "$code" = "200" ] && ok "журнал происшествий" || bad "журнал: HTTP ${code}"

  n=$(curl -ks "${base}/api/scenarios" -H "$auth" \
    | python3 -c 'import sys,json; print(len(json.load(sys.stdin)))' 2>/dev/null || echo 0)
  [ "${n:-0}" -gt 0 ] && ok "учебных вызовов: ${n}" || bad "учебных вызовов нет"

  # Карта ДТП выводится из классификатора при старте: есть она —
  # значит, классификатор импортирован и активен
  code=$(curl -ks -o /dev/null -w '%{http_code}' "${base}/api/catalog/survey-cards/dtp" -H "$auth")
  [ "$code" = "200" ] && ok "классификатор загружен (карта ДТП есть)" || bad "карты ДТП нет: HTTP ${code}"

  # Канал событий: nginx должен пропускать Upgrade до сервера
  code=$(curl -ks --http1.1 -o /dev/null -w '%{http_code}' --max-time 3 \
    -H 'Connection: Upgrade' -H 'Upgrade: websocket' \
    -H 'Sec-WebSocket-Version: 13' -H 'Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==' \
    "${base}/ws?token=${token}")
  [ "$code" = "101" ] && ok "канал событий (WebSocket)" || bad "WebSocket: HTTP ${code}, ждали 101"
fi

code=$(curl -ks -o /dev/null -w '%{http_code}' "${base}/api/docs/")
[ "$code" = "200" ] && ok "описание API" || bad "описание API: HTTP ${code}"

case "$health" in
  *'"ml_provider":"http"'*) ok "языковая модель подключена" ;;
  *'"ml_provider":"mock"'*) echo "  · языковая модель не подключена (mock): всё, кроме ответов модели, работает" ;;
esac

[ "$fail" = "0" ] && echo "стенд в порядке" || echo "есть проблемы — см. выше"
exit "$fail"
