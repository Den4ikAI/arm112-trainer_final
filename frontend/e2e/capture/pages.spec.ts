import { test } from '@playwright/test';
import path from 'node:path';
import { login, go, launchTicket3, fillFireCard } from '../tests/helpers';

/**
 * Съёмка всех экранов подряд — чтобы смотреть на них глазами,
 * а не гадать. Кадры складываются в screenshots/pages/.
 */
const ROOT = path.resolve(__dirname, '../../screenshots/pages');

const снять = async (page: import('@playwright/test').Page, имя: string) => {
  await page.waitForTimeout(700);
  await page.screenshot({ path: path.join(ROOT, `${имя}.png`), fullPage: true });
};

test('экраны преподавателя', async ({ page }, info) => {
  await login(page, info, 'teacher1');
  await снять(page, '01-журнал-преподаватель');

  // Переходим по адресу, а не кликом: на странице вызовов ссылку
  // перекрывает окно переписки, пока оно грузится
  await go(page, info, '/lessons');
  await снять(page, '02-занятия');

  await go(page, info, '/scenarios');
  await снять(page, '03-вызовы');

  await go(page, info, '/analytics');
  await page.waitForTimeout(3000);
  await снять(page, '04-аналитика');
});

test('экраны администратора', async ({ page }, info) => {
  await login(page, info, 'admin');
  await go(page, info, '/admin');
  await снять(page, '05-администрирование');

  await page.getByTestId('admin-tab-users').click();
  await снять(page, '06-учётные-записи');

  await page.getByTestId('admin-tab-classifier').click();
  await снять(page, '07-классификатор');
});

test('экраны курсанта', async ({ page }, info) => {
  await login(page, info);
  await снять(page, '08-журнал-курсант');

  await launchTicket3(page);
  await снять(page, '09-карточка-112');

  await fillFireCard(page);
  await page.getByTestId('save-card').click();
  await page.waitForTimeout(1200);
  await снять(page, '10-разбор');

  await go(page, info, '/login');
  await снять(page, '11-вход');
});
