import { test } from '@playwright/test';
import path from 'node:path';
import { login, launchTicket3 } from '../tests/helpers';

/** Съёмка выведенных опросных карт: смотреть глазами на самые большие. */
const ROOT = path.resolve(__dirname, '../../screenshots/pages');

test('выведенные карты', async ({ page }, info) => {
  await login(page, info);
  await launchTicket3(page);

  await page.locator('#type-search').fill('103');
  await page.getByRole('option', { name: '103', exact: true }).click();
  await page.waitForTimeout(800);
  await page.screenshot({ path: path.join(ROOT, '12-карта-103.png'), fullPage: true });
});
