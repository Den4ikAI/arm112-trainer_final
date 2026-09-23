# Сервер языковой модели: llama.cpp с поддержкой MTP для GigaChat 3.1.
#
# Опубликованный llama.cpp не умеет спекулятивное декодирование на голове
# MTP у этой архитектуры: без Q-LoRA он падает на проверке
# `hparams.n_lora_q > 0`. Нужен наш патч к рантайму. Патч конвертера здесь
# не нужен — модель уже сконвертирована и копируется на ВМ готовой.
# Подробности и замеры — backend/docs/MODEL-SERVER.md.
#
# Коммит закреплён: патч снят именно с него (проверено — применённый к
# исходникам этого коммита, он даёт ровно тот файл, что собран на стенде).
# На свежем master патч может не лечь.
#
# Собирается на самой ВМ, поэтому GGML_NATIVE включён: код оптимизируется
# под её процессор. Переносить собранный образ на другую машину не стоит.

FROM debian:bookworm-slim AS build
ARG LLAMA_COMMIT=a894dae939d426954ce54bb604824f1ae918a0c5
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential cmake git ca-certificates patch \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /src
RUN git clone https://github.com/ggml-org/llama.cpp.git \
 && cd llama.cpp && git checkout "$LLAMA_COMMIT"

COPY backend/docs/mtp-2-runtime.patch /src/
RUN cd llama.cpp && patch -p0 < /src/mtp-2-runtime.patch

# Параметры — как у сборки на стенде, кроме двух: статическая сборка
# (один файл в образе вместо набора .so) и без OpenSSL — сервер модели
# виден только внутри сети compose, HTTPS ему не нужен
RUN cd llama.cpp \
 && cmake -B build -DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=ON \
      -DGGML_OPENMP=ON -DLLAMA_OPENSSL=OFF -DBUILD_SHARED_LIBS=OFF \
 && cmake --build build --target llama-server -j "$(nproc)"

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends curl ca-certificates libgomp1 \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/llama.cpp/build/bin/llama-server /usr/local/bin/llama-server

# Файл модели не кладётся в образ (6,45 ГБ) — монтируется томом
VOLUME /models
EXPOSE 8090

# --spec-type draft-mtp --spec-draft-n-max 1 — ключевые параметры: по
# умолчанию llama.cpp предлагает три черновых токена, и лишние проверки
# съедают весь выигрыш. На одном токене приём 73 % и 14,8 ток/с против
# 10,3 без MTP (6 потоков)
ENTRYPOINT ["sh", "-c", "exec llama-server -m /models/${MODEL_FILE:-full-mtp-q4_K_M.gguf} -t ${MODEL_THREADS:-6} -c ${MODEL_CTX:-8192} --jinja --spec-type draft-mtp --spec-draft-n-max 1 --host 0.0.0.0 --port 8090"]
