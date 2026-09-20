import { defineConfig, devices } from '@playwright/test';
import base from './playwright.config';

/** Съёмка экранов: тот же сервер, другой каталог тестов. */
export default defineConfig({
  ...base,
  testDir: './capture',
  reporter: [['list']],
  use: { ...base.use, trace: 'off', screenshot: 'off' },

  // Тот же размер, что у референсных кадров боевой системы.
  // viewport идёт ПОСЛЕ devices, иначе пресет вернёт свои 1280×720.
  projects: [
    {
      name: 'react',
      use: {
        ...devices['Desktop Chrome'],
        baseURL: 'http://localhost:5173',
        viewport: { width: 1920, height: 1080 },
      },
    },
  ],
});
