#!/bin/bash
# Записать демо-ролик тренажёра целиком: база → экземпляр → сценарий → MP4.
#
#   docs/demo/build.sh gpu          # Qwen3.8-27B в vLLM (:8091), ~9 минут ролика
#   docs/demo/build.sh cpu          # GigaChat 3.1 на процессоре (:8090), ожидания
#                                   # модели ускорены и помечены на экране
#
# Что нужно заранее: сервер модели своего профиля, сервер голоса с весами
# (VOICE_MODELS_DIR), контейнер базы arm112-db, собранный backend
# (cargo build) и node_modules во frontend/apps/react и frontend/e2e.
#
# Ролик пишется на своей чистой базе DEMO_DB (по умолчанию arm112_demo):
# она создаётся заново, классификатор импортируется, учебные вызовы — из
# миграций. Стенд и его база не затрагиваются.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PROFILE="${1:-gpu}"
DB="${DEMO_DB:-arm112_demo}"
OUT_DIR="${DEMO_VIDEO_DIR:-$ROOT/../lct_hack-demo}"
RUN="${DEMO_RUN_DIR:-/tmp/arm112-demo}/$PROFILE"
PY="${DEMO_PY:-/media/denis/work4/venvs/voice/bin/python}"
CARGO="${CARGO:-cargo}"
export DEMO_DB="$DB"

psql_db() { docker exec arm112-db psql -U arm112 -d "$1" -qAtc "$2"; }

echo "== база $DB"
psql_db postgres "DROP DATABASE IF EXISTS $DB WITH (FORCE)"
psql_db postgres "CREATE DATABASE $DB OWNER arm112"
(cd "$ROOT/backend" && DATABASE_URL="postgres://arm112:arm112@localhost:5432/$DB" \
  "$CARGO" run -q --bin import_classifier -- "$ROOT"/task/*v_046_24*.xlsx "v046_24" >/dev/null)

echo "== экземпляр ($PROFILE)"
"$ROOT/docs/demo/server.sh" stop >/dev/null || true
"$ROOT/docs/demo/server.sh" start "$PROFILE"
# На видеокарте начальник переспрашивает моделью в каждой реплике; на
# процессоре — по ключевым словам, модель разбирает доклад один раз
psql_db "$DB" "UPDATE settings SET value = '$([ "$PROFILE" = gpu ] && echo 1 || echo 0)' WHERE key = 'chief.live_model'"

echo "== голос диктора"
curl -sf "http://127.0.0.1:${NARRATOR_PORT:-8193}/health" | grep -q narrator \
  || "$ROOT/docs/demo/narrator.sh" start

echo "== запись"
rm -rf "$RUN"
(
  cd "$ROOT/frontend/e2e"
  DEMO_PROFILE="$PROFILE" DEMO_OUT="$RUN" \
  DEMO_SPEEDUP="$([ "$PROFILE" = cpu ] && echo "${DEMO_SPEEDUP:-8}" || echo 1)" \
  npx playwright test --config playwright.demo.config.ts
)

echo "== сборка MP4"
mkdir -p "$OUT_DIR"
"$PY" "$ROOT/docs/demo/assemble.py" "$RUN" "$OUT_DIR/демо-$PROFILE.mp4"

"$ROOT/docs/demo/server.sh" stop >/dev/null || true
echo "готово: $OUT_DIR/демо-$PROFILE.mp4"
