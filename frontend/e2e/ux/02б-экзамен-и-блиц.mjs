// Экзамен ДДС: разбор скрыт до конца — и в ответе на назначение бригады
// тоже. Блиц «статусы по докладам»: что видит диспетчер.
import { browser, page, login, shot, log, api, token } from './kit.mjs';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const who = process.env.UX_WHO ?? 'dds04';
const svc = process.env.UX_SVC ?? 'codd';
const b = await browser();
const p = await page(b, who);
await login(p, who);
await p.getByTestId('section-exam').click();
await sleep(1500);
const rows = (await api('GET', '/api/me/work?section=exam', null, await token(who))).json;
log(`[${who}] экзамен:`, rows.map((r) => `${r.cardNumber}:${r.status}:hidden=${r.hidden}`).join(' '));
log(`[${who}] кнопок «Открыть» на экзамене:`, await p.locator('[data-testid^="work-row-"]').getByRole('button', { name: 'Открыть' }).count(), 'из', rows.length);
await shot(p, `${who}-экзамен-список`);
const first = rows.find((r) => r.status !== 'done');
if (first) {
  await p.getByTestId(`work-row-${first.cardId}`).getByRole('button', { name: 'Открыть' }).click();
  await sleep(2000);
  const ed = p.getByTestId(`status-editor-${svc}`);
  await p.getByTestId(`pencil-${svc}`).click();
  await ed.getByLabel('Статус', { exact: true }).selectOption({ label: 'Принята' });
  await ed.getByLabel('Комментарий').fill('Принял, направляю');
  await ed.getByLabel('Сохранить статус').click();
  await sleep(1500);
  log(`[${who}] экзамен до назначения:`, (await p.getByTestId('dds-score').textContent()), '; замечаний', await p.getByTestId('dds-finding').count());
  await p.getByTestId('brigade-assign').click();
  await sleep(700);
  log(`[${who}] экзамен сразу после назначения:`, (await p.getByTestId('dds-score').textContent()), '; замечаний', await p.getByTestId('dds-finding').count());
  await shot(p, `${who}-экзамен-после-назначения`);
  await p.getByTestId('brigade-call').click();
  await sleep(1500);
  log(`[${who}] экзамен после звонка старшему:`, (await p.getByTestId('dds-score').textContent()), '; замечаний', await p.getByTestId('dds-finding').count());
  await p.reload();
  await sleep(2500);
  log(`[${who}] экзамен после перезагрузки:`, (await p.getByTestId('dds-score').textContent()), '; замечаний', await p.getByTestId('dds-finding').count());
  const review = await p.getByTestId('attempt-review').textContent().catch(() => '(нет)');
  log(`[${who}] разбор попытки на экзамене:`, (review ?? '').replace(/\s+/g, ' ').slice(0, 200));
}

// Блиц
await p.goto(p.url().replace(/#.*/, '#/journal'));
await sleep(1500);
await p.getByTestId('section-blitz').click();
await sleep(1500);
const blitz = (await api('GET', '/api/me/work?section=blitz', null, await token(who))).json;
log(`[${who}] блиц:`, blitz.map((r) => `${r.cardNumber}:${r.segment}:${r.status}:${r.lessonTitle}`).join(' | '));
await shot(p, `${who}-блиц-список`);
if (blitz[0]) {
  await p.getByTestId(`work-row-${blitz[0].cardId}`).getByRole('button').first().click();
  await sleep(2500);
  log(`[${who}] блиц, карточка:`, (await p.getByTestId('brigade-panel').textContent().catch(() => '(нет панели)')).replace(/\s+/g, ' ').slice(0, 250));
  await shot(p, `${who}-блиц-карточка`);
}
await b.close();
