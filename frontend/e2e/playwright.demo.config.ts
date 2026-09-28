import { defineConfig, devices } from '@playwright/test';

/**
 * Демо-ролик: сценарий сам проходит тренажёр и пишет экран.
 *
 * Не тест и в `npm test` не входит. Сервер не поднимает: демо-экземпляр
 * запускает docs/demo/server.sh (своя база, свои порты), а весь конвейер
 * записи и сборки MP4 — docs/demo/build.sh.
 */
export default defineConfig({
  testDir: './demo',
  testMatch: /.*\.demo\.ts/,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [['list']],
  timeout: 1_800_000,
  expect: { timeout: 30_000 },
  use: {
    ...devices['Desktop Chrome'],
    baseURL: `http://localhost:${process.env.DEMO_WEB_PORT ?? '5271'}`,
    viewport: { width: 1920, height: 1080 },
    deviceScaleFactor: 1,
    locale: 'ru-RU',
    // Зависший шаг должен падать, а не ждать полчаса
    actionTimeout: 60_000,
    navigationTimeout: 60_000,
    trace: 'off',
    screenshot: 'off',
    video: 'off',
    permissions: ['microphone'],
    launchOptions: {
      args: ['--autoplay-policy=no-user-gesture-required', '--use-fake-ui-for-media-stream'],
    },
  },
});
