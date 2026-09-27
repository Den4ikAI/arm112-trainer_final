# Сервер голоса: распознавание (GigaAM v3) и синтез (TeraTTSv2) на
# процессоре. Код — voice/server.py, веса монтируются томом /models:
#
#   /models/gigaam-v3-onnx/   istupakov/gigaam-v3-onnx, файлы v3_e2e_rnnt*
#   /models/TeraTTSv2/        TeraSpace/TeraTTSv2 целиком (рантайм — рядом
#                             с весами, в образ его не копируем)
#
# PyTorch не нужен: обе модели в ONNX. Образ — около гигабайта из-за
# onnxruntime и transformers (из него рантайму TeraTTS нужен только
# токенизатор для расстановки ударений).

FROM python:3.12-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends curl \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY voice/requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY voice/server.py .

ENV VOICE_MODELS_DIR=/models VOICE_CACHE_DIR=/cache
VOLUME /models
EXPOSE 8091

ENTRYPOINT ["sh", "-c", "exec uvicorn server:app --host 0.0.0.0 --port 8091 --workers 1"]
