#!/bin/bash
# Сервер голоса для закадрового текста демо-ролика.
#
#   docs/demo/narrator.sh start | stop
#
# Тот же voice/server.py, отдельным процессом на своём порту (8193):
# в нём есть голос «narrator», а общий сервер голоса стенда
# перезапускать ради ролика незачем.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PORT="${NARRATOR_PORT:-8193}"
RUN="${DEMO_RUN_DIR:-/tmp/arm112-demo}"
mkdir -p "$RUN"

case "${1:-}" in
  start)
    (
      cd "$ROOT/voice"
      VOICE_MODELS_DIR="${VOICE_MODELS_DIR:-/media/denis/work4/llm/voice}" \
      VOICE_CACHE_DIR="${VOICE_CACHE_DIR:-/media/denis/work4/llm/voice/cache-demo}" \
      VOICE_THREADS="${VOICE_THREADS:-6}" \
      nohup "${VOICE_PY:-/media/denis/work4/venvs/voice/bin/uvicorn}" server:app \
        --host 127.0.0.1 --port "$PORT" > "$RUN/narrator.log" 2>&1 &
      echo $! > "$RUN/narrator.pid"
    )
    for _ in $(seq 90); do
      curl -sf "http://127.0.0.1:$PORT/health" | grep -q narrator && break
      sleep 2
    done
    curl -s "http://127.0.0.1:$PORT/health"; echo
    ;;
  stop)
    if [ -f "$RUN/narrator.pid" ]; then
      pid="$(cat "$RUN/narrator.pid")"
      kill "$pid" 2>/dev/null || true
      # Ждём, пока освободится порт: иначе новый запуск наткнётся на старый
      for _ in $(seq 30); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
    fi
    rm -f "$RUN/narrator.pid"
    ;;
  *) echo "использование: $0 start|stop" >&2; exit 2 ;;
esac
