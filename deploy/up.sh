#!/usr/bin/env bash
# Поднять стенд тренажёра одной командой.
#
#   deploy/up.sh
#
# Повторный запуск безопасен: пересобирает изменившиеся образы и
# пересоздаёт только то, что поменялось. Данные базы в томе не трогает.
set -euo pipefail
cd "$(dirname "$0")"

if [ ! -f .env ]; then
  cp .env.example .env
  # Секреты генерируем на месте: пример лежит в открытом репозитории,
  # и пароль из него был бы известен всем
  sed -i "s/^DB_PASSWORD=.*/DB_PASSWORD=$(openssl rand -hex 24)/" .env
  sed -i "s/^JWT_SECRET=.*/JWT_SECRET=$(openssl rand -hex 32)/" .env
  chmod 600 .env
  echo "создан deploy/.env с новыми секретами"
fi

set -a
# shellcheck disable=SC1091
. ./.env
set +a

# Модель включаем, только если файл весов на месте: без него сервер
# модели не стартует, а тренажёр прекрасно работает и без неё
profiles=()
model="${MODEL_DIR:-/opt/arm112/model}/${MODEL_FILE:-full-mtp-q4_K_M.gguf}"
if [ -n "${ML_BASE_URL:-}" ]; then
  # Модель уже развёрнута на другой машине контура — свой контейнер
  # модели не нужен
  export ML_PROVIDER=http
  echo "модель по адресу $ML_BASE_URL — свой контейнер модели не поднимаю"
elif [ -f "$model" ]; then
  export ML_PROVIDER=http
  profiles+=(model)
  echo "модель найдена: $model — поднимаю вместе с ней"
else
  export ML_PROVIDER=mock
  echo "файла модели нет ($model) — поднимаю без неё:"
  echo "  классификация, оценка и интерфейс работают, ответы модели — нет"
fi

# Голос — так же: свой контейнер, если веса на месте, или чужой адрес
voice_dir="${VOICE_DIR:-/opt/arm112/voice}"
if [ -n "${VOICE_BASE_URL:-}" ]; then
  echo "голос по адресу $VOICE_BASE_URL — свой контейнер голоса не поднимаю"
elif [ -d "$voice_dir/gigaam-v3-onnx" ] && [ -d "$voice_dir/TeraTTSv2" ]; then
  export VOICE_BASE_URL=http://voice:8091
  profiles+=(voice)
  echo "веса голоса найдены: $voice_dir — поднимаю голос"
else
  echo "весов голоса нет ($voice_dir) — разговор со старшим бригады будет текстом"
fi

# Кросс-энкодер для сверки описания — так же, по наличию весов
nli_dir="${NLI_DIR:-/opt/arm112/nli}"
if [ -n "${NLI_BASE_URL:-}" ]; then
  echo "кросс-энкодер по адресу $NLI_BASE_URL — свой контейнер не поднимаю"
elif [ -d "$nli_dir/${NLI_MODEL:-qwen3.5-2b-nli-v5}" ]; then
  export NLI_BASE_URL=http://nli:8093
  profiles+=(nli)
  echo "веса кросс-энкодера найдены: $nli_dir — поднимаю сверку описания"
else
  echo "весов кросс-энкодера нет ($nli_dir) — описание сверяет языковая модель"
fi

# Сертификат для HTTPS. Без HTTPS браузер не даёт микрофон, и голосовой
# ввод на стенде недоступен. Свой сертификат — положить в deploy/tls/
# (cert.pem, key.pem); иначе выпускаем самоподписанный на адрес машины
addr=$(hostname -I 2>/dev/null | awk '{print $1}')
if [ ! -f tls/cert.pem ] || [ ! -f tls/key.pem ]; then
  mkdir -p tls
  san="DNS:localhost,IP:127.0.0.1${addr:+,IP:$addr}${TLS_HOST:+,DNS:$TLS_HOST}"
  openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
    -keyout tls/key.pem -out tls/cert.pem -subj "/CN=arm112-trainer" \
    -addext "subjectAltName=$san" 2>/dev/null
  chmod 600 tls/key.pem
  echo "выпущен самоподписанный сертификат ($san): браузер один раз предупредит"
fi

COMPOSE_PROFILES=$(IFS=,; echo "${profiles[*]}")
export COMPOSE_PROFILES

docker compose up -d --build --remove-orphans

port="${WEB_TLS_PORT:-443}"
echo "жду готовности (первая сборка — 10–20 минут)…"
for _ in $(seq 1 180); do
  # -k: сертификат может быть самоподписанным
  if curl -kfsS "https://127.0.0.1:${port}/api/admin/health" >/dev/null 2>&1; then
    echo
    echo "готово: https://${addr:-127.0.0.1}:${port}/"
    echo "учётные записи: admin / teacher1 / student1…student5, пароль arm112"
    docker compose ps
    exit 0
  fi
  sleep 5
done

echo "сервер не ответил за 15 минут. Логи: docker compose -f deploy/docker-compose.yml logs backend" >&2
exit 1
