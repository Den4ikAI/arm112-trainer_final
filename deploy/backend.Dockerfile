# Сервер тренажёра: сборка и запуск.
#
# Контекст сборки — корень репозитория (см. docker-compose.yml): кроме
# исходников сервера в образ кладутся файл классификатора из task/ —
# его импортируют при первом запуске — и разобранные билеты, из которых
# преподаватель готовит учебные вызовы.

# ── Сборка ───────────────────────────────────────────────────────────
FROM rust:1-bookworm AS build
WORKDIR /src/backend
COPY backend/ ./

# Кэши реестра и target не попадают в слои: повторная сборка на ВМ
# после правки идёт минуты, а не с нуля. Бинарники копируем из кэша
# в том же шаге — после него кэш уже не виден
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/backend/target \
    cargo build --release --locked --bin arm112 --bin import_classifier \
 && mkdir -p /out \
 && cp target/release/arm112 target/release/import_classifier /out/

# ── Запуск ───────────────────────────────────────────────────────────
# Основа — образ PostgreSQL 16, а не голый Debian: серверу для ежесуточной
# резервной копии (требование ТЗ) нужен pg_dump той же версии, что и база.
# pg_dump 15 из Debian с сервером 16 работать отказывается. Заодно здесь
# есть pg_isready и psql — ими пользуется сценарий запуска.
FROM postgres:16-bookworm
RUN apt-get update \
 && apt-get install -y --no-install-recommends curl ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=build /out/arm112 /out/import_classifier /app/bin/
COPY task/*.xlsx /app/task/
COPY extracted/bilety_96_scenarios.md /app/extracted/
COPY deploy/backend-entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh

# Внутри контейнера слушаем все интерфейсы: снаружи сервер всё равно
# не виден, к нему ходит только nginx по внутренней сети compose
ENV BIND_ADDR=0.0.0.0:8080 \
    TICKETS_FILE=/app/extracted/bilety_96_scenarios.md \
    RUST_LOG=info

EXPOSE 8080
# У образа postgres своя точка входа — заменяем её
ENTRYPOINT ["/app/entrypoint.sh"]
CMD []
