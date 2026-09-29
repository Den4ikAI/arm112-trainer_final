// Проверка правки узких экранов: на 390×844 строка задания помещается,
// статус своей службы проставляется с телефона.
import { browser, page, login, shot, log, hscroll, BASE } from './kit.mjs';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const b = await browser();
const p = await page(b, 'dds01-390', { width: 390, height: 844 });
await login(p, 'dds01');
log('[dds01-390] разделы после правки: вбок', await hscroll(p), 'px');
const btn = p.locator('[data-testid^="work-row-"] button').first();
const box = await btn.boundingBox();
log('[dds01-390] кнопка первой строки:', Math.round(box.x), '…', Math.round(box.x + box.width));
await shot(p, 'телефон-разделы-после');

await p.goto(`${BASE}/#/card-dds/${process.env.UX_DDS_CARD}`);
await sleep(2000);
await p.getByTestId('pencil-mchs').click();
const ed = p.getByTestId('status-editor-mchs');
const eb = await ed.boundingBox();
log('[dds01-390] окно статуса:', Math.round(eb.x), '…', Math.round(eb.x + eb.width), '; вбок', await hscroll(p), 'px');
await ed.getByLabel('Статус', { exact: true }).selectOption({ label: 'Принята' });
await ed.getByLabel('Комментарий').fill('Принял с телефона');
await shot(p, 'телефон-статус-после');
await ed.getByLabel('Сохранить статус').click();
await sleep(1500);
log('[dds01-390] окно закрылось после сохранения:', (await ed.count()) === 0,
  '; на вкладке:', (await p.getByTestId('service-tab-mchs').textContent()).replace(/\s+/g, ' '));
await p.getByTestId('service-tab-mchs').getByLabel('История статусов').click();
await sleep(500);
const hb = await p.getByTestId('history-mchs').boundingBox();
log('[dds01-390] история:', Math.round(hb.x), '…', Math.round(hb.x + hb.width), '; вбок', await hscroll(p), 'px');
await shot(p, 'телефон-история-после');
await b.close();
