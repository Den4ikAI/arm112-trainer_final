import { defineConfig, devices } from '@playwright/test';

/**
 * Порты прогона. По умолчанию свои, отдельные от стенда (8080/5173).
 * Задаются окружением, чтобы несколько прогонов могли идти разом —
 * у каждого своя база и свои порты, и они не подхватывают чужой сервер.
 */
const API_PORT = process.env.E2E_API_PORT ?? '8081';
const WEB_PORT = process.env.E2E_WEB_PORT ?? '5174';

/**
 * Прогон сценариев против фронтенда тренажёра АРМ-112.
 * Dev-сервер поднимается сам, отдельно запускать не нужно.
 */
export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  workers: 1,
  reporter: [['list'], ['html', { outputFolder: 'report', open: 'never' }]],
  // Запас на холодный старт Vite при первом прогоне.
  timeout: 60_000,
  expect: { timeout: 15_000 },

  use: {
    viewport: { width: 1600, height: 900 },
    locale: 'ru-RU',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },

  projects: [
    {
      name: 'react',
      // Путь к странице собирает go() из helpers: приложение на
      // HashRouter, и «#» должен попасть внутрь пути, а не в базу —
      // абсолютный путь его бы затёр.
      use: { ...devices['Desktop Chrome'], baseURL: `http://localhost:${WEB_PORT}` },
    },
  ],

  webServer: [
    {
      // Классификатор, оценка и статусы живут на сервере: без него
      // прогонять эти сценарии бессмысленно. Нужна поднятая база
      // (docker compose up -d) с применёнными миграциями.
      command: 'cargo run --quiet',
      cwd: '../../backend',
      // Своя база и свой порт. Пока прогон ходил в ту же базу, что
      // и стенд, в учётных записях копились сотни курсантов, а в
      // переписке сценариев — реплики «проверка 1789977379959».
      // Порт тоже свой, иначе reuseExistingServer подхватит стенд
      // и весь смысл отдельной базы пропадёт.
      // Окружение доливаем, а не заменяем: иначе теряется PATH.
      // Заодно дописываем каталог rustup: в неинтерактивной оболочке
      // его в PATH нет, и cargo не находится
      env: {
        ...process.env,
        PATH: `${process.env.HOME}/.cargo/bin:${process.env.PATH}`,
        DATABASE_URL: process.env.TEST_DATABASE_URL
          ?? 'postgres://arm112:arm112@localhost:5432/arm112_test',
        BIND_ADDR: `127.0.0.1:${API_PORT}`,
      },
      url: `http://127.0.0.1:${API_PORT}/api/admin/health`,
      reuseExistingServer: false,
      timeout: 180_000,
    },
    {
      // Свой порт и свой прокси: иначе прогон подхватит уже
      // поднятый для разработки клиент, а тот смотрит на стенд
      command: `npm run dev -- --host 127.0.0.1 --port ${WEB_PORT}`,
      cwd: '../apps/react',
      env: { ...process.env, VITE_API_TARGET: `http://127.0.0.1:${API_PORT}` },
      url: `http://localhost:${WEB_PORT}`,
      reuseExistingServer: false,
      timeout: 120_000,
    },
  ],
});
