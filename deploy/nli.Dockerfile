# Сервер кросс-энкодера NLI: сверка описания курсанта с фактами вызова
# на процессоре. Код — nli/server.py, веса монтируются томом /models:
#
#   /models/qwen3.5-2b-nli-v5/   AlexWortega/openjev, подкаталог чекпоинта
#                                (или qwen3.5-4b-nli-v5 — точнее, но вдвое
#                                больше памяти; см. NLI_MODEL в deploy/.env)
#
# PyTorch ставится сборкой для процессора: CUDA-колесо в разы больше и
# на ВМ без видеокарты бесполезно.

FROM python:3.12-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends curl \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY nli/requirements.txt .
RUN pip install --no-cache-dir torch --index-url https://download.pytorch.org/whl/cpu \
 && pip install --no-cache-dir -r requirements.txt
COPY nli/server.py nli/openjev.py ./

VOLUME /models
EXPOSE 8093

ENTRYPOINT ["sh", "-c", "exec uvicorn server:app --host 0.0.0.0 --port 8093 --workers 1"]
