import { defineConfig, devices } from '@playwright/test';

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
      // Приложение на HashRouter, отсюда «#» в базовом адресе.
      use: { ...devices['Desktop Chrome'], baseURL: 'http://localhost:5173/#' },
    },
  ],

  webServer: [
    {
      // Классификатор, оценка и статусы живут на сервере: без него
      // прогонять эти сценарии бессмысленно. Нужна поднятая база
      // (docker compose up -d) с применёнными миграциями.
      command: 'cargo run --quiet',
      cwd: '../../backend',
      url: 'http://127.0.0.1:8080/api/admin/health',
      reuseExistingServer: true,
      timeout: 180_000,
    },
    {
      command: 'npm run dev -- --host 127.0.0.1',
      cwd: '../apps/react',
      url: 'http://localhost:5173',
      reuseExistingServer: true,
      timeout: 120_000,
    },
  ],
});
