import { test, type Page, type TestInfo } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { go, login, fillFireCard } from '../tests/helpers';

/** Съёмка раздела «Учебный материал» и выбора материала в занятии. */
const ROOT = path.resolve(__dirname, '../../screenshots');
const shot = async (page: Page, info: TestInfo, name: string) => {
  const dir = path.join(ROOT, info.project.name);
  await mkdir(dir, { recursive: true });
  await page.screenshot({ path: path.join(dir, `${name}.png`), fullPage: false });
};

test('учебный материал: конструкторы и выбор в занятии', async ({ page }, info) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1600, height: 900 });
  await login(page, info, 'teacher1');
  await page.getByTestId('scenarios-link').click();
  await shot(page, info, 'material-01-вызовы-112');

  await page.getByTestId('create-manual').click();
  await page.getByTestId('builder-title').fill('Пожар в частном доме');
  await page.getByTestId('builder-legend').fill('Алло! Горит крыша частного дома, приезжайте!');
  await fillFireCard(page);
  await shot(page, info, 'material-02-вызов-вручную');

  await page.getByTestId('create-prompt').click();
  await page.getByTestId('prompt-text').fill('Пожилая женщина сообщает о запахе газа в подъезде.');
  await shot(page, info, 'material-03-по-описанию');

  await page.getByTestId('tab-dds').click();
  await page.getByTestId('create-manual').click();
  const b = page.getByTestId('incident-builder');
  await b.getByTestId('incident-title').fill('Пожар: частный дом');
  await b.getByLabel('Улица:').fill('Грина');
  await b.getByLabel('Дом/Вл:').fill('11');
  await b.getByTestId('incident-description').fill('Горит крыша частного дома, пострадавших нет.');
  await fillFireCard(page);
  await b.getByTestId('incident-save').click();
  await page.getByTestId('incident-view').waitFor();
  await shot(page, info, 'material-04-происшествие-ддс');

  await go(page, info, '/lessons');
  await page.getByTestId('lesson-mode').selectOption('dds');
  await page.getByTestId('lesson-material').scrollIntoViewIfNeeded();
  await shot(page, info, 'material-05-занятие-выбор');
});
