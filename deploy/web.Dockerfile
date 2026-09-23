# Клиент тренажёра: статическая сборка и nginx перед сервером.
#
# Для разработки клиент живёт на Vite с прокси — в проде так нельзя:
# dev-сервер медленный, пересобирает на лету и не рассчитан на
# несколько дней непрерывной работы. Здесь клиент собирается один раз,
# а nginx раздаёт статику и проксирует /api и /ws на сервер.

# ── Сборка ───────────────────────────────────────────────────────────
# Vite 8 требует Node 20.19+
FROM node:20-bookworm-slim AS build
WORKDIR /src/frontend

# Общие стили лежат вне приложения, на них ссылается псевдоним @styles
COPY frontend/packages ./packages

# Сначала только манифесты: слой с зависимостями переживает правки кода
COPY frontend/apps/react/package.json frontend/apps/react/package-lock.json ./apps/react/
RUN cd apps/react && npm ci --no-audit --no-fund

COPY frontend/apps/react ./apps/react
RUN cd apps/react && npm run build

# ── Раздача ──────────────────────────────────────────────────────────
FROM nginx:1.27-alpine
COPY deploy/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /src/frontend/apps/react/dist /usr/share/nginx/html
EXPOSE 80
