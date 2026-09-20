import type { Page, TestInfo } from '@playwright/test';

/** Приложение на HashRouter — префикс уже в baseURL. */
export const go = async (page: Page, _info: TestInfo, path: string): Promise<void> => {
  await page.goto(path);
};

/** Вход и переход в журнал происшествий. */
export const login = async (page: Page, info: TestInfo): Promise<void> => {
  await go(page, info, '/login');
  await page.getByTestId('login-submit').click();
  await page.getByRole('heading', { name: 'Учебные вызовы' }).waitFor();
};
