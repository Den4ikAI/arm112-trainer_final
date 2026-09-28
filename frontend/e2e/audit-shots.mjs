// Скриншоты для аудита по критериям: основные экраны на десктопе
// (1600×900) и на телефоне (400×900). Запускается против своего
// экземпляра тренажёра, не стенда:
//   AUDIT_URL=http://127.0.0.1:5285 node audit-shots.mjs <каталог>
import { chromium } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';

const BASE = process.env.AUDIT_URL ?? 'http://127.0.0.1:5285';
const OUT = process.argv[2];
await mkdir(OUT, { recursive: true });

const browser = await chromium.launch();

const войти = async (page, логин) => {
  await page.goto(`${BASE}/#/login`);
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.locator('input[name="login"]').fill(логин);
  await page.locator('input[name="password"]').fill('arm112');
  await page.getByTestId('login-submit').click();
  await page.getByLabel('Поиск происшествий').waitFor();
};

const снять = async (page, имя) => {
  await page.waitForTimeout(700);
  await page.screenshot({ path: path.join(OUT, `${имя}.png`) });
};

for (const [ширина, метка] of [[1600, 'desktop'], [400, 'mobile']]) {
  const ctx = await browser.newContext({ viewport: { width: ширина, height: 900 }, locale: 'ru-RU' });
  const page = await ctx.newPage();

  await page.goto(`${BASE}/#/login`);
  await снять(page, `${метка}-01-вход`);

  // Оператор 112: журнал, приём вызова, разбор
  await войти(page, 'student1');
  await снять(page, `${метка}-02-журнал-оператор112`);
  await page.getByTestId('launch-t3-c1').click();
  await page.getByTestId('card-timer').waitFor();
  await снять(page, `${метка}-03-карточка112`);
  if (метка === 'desktop') {
    // Справочники — как в окнах боевой системы (кадры КАРТОЧКА 112, СЛУЖБЫ 112)
    await page.locator('#type-search').click();
    await снять(page, `${метка}-03а-типы`);
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Добавить службу' }).click();
    await page.getByRole('dialog', { name: 'Добавьте службы' }).waitFor();
    await снять(page, `${метка}-03б-службы`);
    await page.getByRole('button', { name: 'Сохранить и закрыть' }).click();
  }
  await page.getByTestId('save-card').click();
  await page.getByTestId('score').waitFor({ timeout: 30000 }).catch(() => {});
  await снять(page, `${метка}-04-разбор112`);

  // Диспетчер ДДС: лента своей службы и карточка
  await войти(page, 'student3');
  await снять(page, `${метка}-05-журнал-ддс`);
  const строка = page.locator('[data-testid^="journal-row-"]').first();
  if (await строка.count()) {
    await строка.click();
    await page.getByTestId('card-class').waitFor({ timeout: 15000 }).catch(() => {});
    await снять(page, `${метка}-06-карточка-ддс`);
  }

  // Преподаватель: аналитика, занятия
  await войти(page, 'teacher1');
  const аналитика = page.getByRole('button', { name: /аналитика/ });
  if (await аналитика.count()) {
    await аналитика.first().click();
    await page.waitForTimeout(2500);
    await снять(page, `${метка}-07-аналитика`);
  }
  const занятия = page.getByRole('button', { name: /^занятия$/ });
  if (await занятия.count()) {
    await занятия.first().click();
    await page.waitForTimeout(1500);
    await снять(page, `${метка}-08-занятия`);
  }

  // Администратор: параметры
  await войти(page, 'admin');
  await page.getByTestId('admin-link').click();
  await page.getByTestId('admin-tab-settings').click();
  await page.getByTestId('settings-tab').waitFor();
  await снять(page, `${метка}-09-параметры`);

  await ctx.close();
}

await browser.close();
