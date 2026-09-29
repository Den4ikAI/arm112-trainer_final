#!/usr/bin/env bash
# Поднять свой экземпляр для UX-прохода: сервер 8201, клиент 5301,
# база arm112_ux1 (копия тестовой с классификатором), учебный набор.
# Модель — GPU (vLLM Gemma на 8095), голос — 8092: они уже запущены.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
LOG="${UX_LOG:-/tmp/ux1}"
mkdir -p "$LOG"
BIN="${UX_BIN:-/media/denis/work4/lct_hack/backend/target-ux1/debug/arm112}"

cd "$ROOT/backend"
BIND_ADDR=127.0.0.1:8201 \
DATABASE_URL=postgres://arm112:arm112@localhost:5432/arm112_ux1 \
ML_PROVIDER=http ML_BASE_URL=http://127.0.0.1:8095 ML_MODEL=local \
VOICE_BASE_URL=http://127.0.0.1:8092 \
  nohup "$BIN" > "$LOG/srv.log" 2>&1 &
echo $! > "$LOG/srv.pid"

cd "$ROOT/frontend/apps/react"
VITE_API_TARGET=http://127.0.0.1:8201 nohup npx vite --host 127.0.0.1 --port 5301 --strictPort \
  > "$LOG/web.log" 2>&1 &
echo $! > "$LOG/web.pid"

for _ in $(seq 1 60); do
  curl -sf http://127.0.0.1:8201/api/admin/health >/dev/null && break
  sleep 1
done
curl -s -o /dev/null -w "api %{http_code}\n" http://127.0.0.1:8201/api/admin/health
sleep 3
curl -s -o /dev/null -w "web %{http_code}\n" http://127.0.0.1:5301/
