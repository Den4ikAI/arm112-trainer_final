#!/bin/bash
# Поднять свой экземпляр тренажёра для записи демо-ролика.
#
#   docs/demo/server.sh start gpu   # модель — Gemma 4 26B-A4B в vLLM (:8095)
#   docs/demo/server.sh start cpu   # модель — GigaChat 3.1 в llama.cpp (:8090)
#   docs/demo/server.sh stop
#
# Своя база (DEMO_DB, по умолчанию arm112_demo), свои порты (API 8171,
# клиент 5271), чтобы запись не задела стенд. Сервер голоса — общий
# (:8092), он ничего не хранит.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
DB="${DEMO_DB:-arm112_demo}"
API_PORT="${DEMO_API_PORT:-8171}"
WEB_PORT="${DEMO_WEB_PORT:-5271}"
RUN="${DEMO_RUN_DIR:-/tmp/arm112-demo}"
mkdir -p "$RUN"

case "${1:-}" in
  start)
    profile="${2:-gpu}"
    case "$profile" in
      gpu) ml_url="${ML_BASE_URL_GPU:-http://127.0.0.1:8095}" ;;
      cpu) ml_url="${ML_BASE_URL_CPU:-http://127.0.0.1:8090}" ;;
      *) echo "профиль: gpu или cpu" >&2; exit 2 ;;
    esac
    (
      cd "$ROOT/backend"
      DATABASE_URL="postgres://arm112:arm112@localhost:5432/$DB" \
      BIND_ADDR="127.0.0.1:$API_PORT" \
      ML_PROVIDER=http ML_BASE_URL="$ml_url" ML_MODEL=local \
      VOICE_BASE_URL="${VOICE_BASE_URL:-http://127.0.0.1:8092}" \
      nohup ./target/debug/arm112 > "$RUN/api.log" 2>&1 &
      echo $! > "$RUN/api.pid"
    )
    (
      cd "$ROOT/frontend/apps/react"
      VITE_API_TARGET="http://127.0.0.1:$API_PORT" \
      nohup npm run dev -- --host 127.0.0.1 --port "$WEB_PORT" --strictPort > "$RUN/web.log" 2>&1 &
      echo $! > "$RUN/web.pid"
    )
    for _ in $(seq 60); do
      curl -sf "http://127.0.0.1:$API_PORT/api/admin/health" >/dev/null \
        && curl -sf "http://127.0.0.1:$WEB_PORT" >/dev/null && break
      sleep 2
    done
    echo "демо-экземпляр ($profile): api :$API_PORT, клиент :$WEB_PORT, база $DB"
    ;;
  stop)
    for f in api web; do
      [ -f "$RUN/$f.pid" ] && kill "$(cat "$RUN/$f.pid")" 2>/dev/null || true
      rm -f "$RUN/$f.pid"
    done
    # vite порождает дочерний процесс — добиваем по порту
    pid=$(ss -ltnp 2>/dev/null | grep ":$WEB_PORT " | grep -o 'pid=[0-9]*' | cut -d= -f2 | head -1)
    [ -n "${pid:-}" ] && kill "$pid" 2>/dev/null || true
    echo "остановлено"
    ;;
  *) echo "использование: $0 start gpu|cpu | stop" >&2; exit 2 ;;
esac
