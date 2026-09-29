// UX-проход 1: преподаватель и оператор 112 — живьём, кликами, как человек.
//
// Не тест (в npm test не входит): каждый шаг снимается, ошибки консоли,
// исключения страницы и ответы ≥400 собираются в отчёт, упавший шаг не
// останавливает остальные. Экземпляр — frontend/e2e/ux/поднять.sh.
//
//   node frontend/e2e/ux/проход-1.mjs [адрес клиента] [каталог кадров]
import { chromium } from '@playwright/test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const BASE = (process.argv[2] ?? 'http://127.0.0.1:5301').replace(/\/$/, '');
const OUT = process.argv[3] ?? path.resolve('docs/ux/1');
const VOICE = 'http://127.0.0.1:8092';
fs.mkdirSync(OUT, { recursive: true });

const находки = [];   // { кто, шаг, вид, текст }
const шаги = [];      // { кто, шаг, ok, мс, ошибка }
let кадр = 0;

function следить(page, кто) {
  page.on('console', (m) => {
    if (m.type() === 'error') находки.push({ кто, шаг: page.__шаг, вид: 'console.error', текст: m.text().slice(0, 300) });
  });
  page.on('pageerror', (e) => находки.push({ кто, шаг: page.__шаг, вид: 'pageerror', текст: String(e).slice(0, 300) }));
  page.on('response', async (r) => {
    if (r.status() >= 400 && r.url().includes('/api/')) {
      let тело = '';
      try { тело = (await r.text()).slice(0, 200); } catch { /* ответ уже ушёл */ }
      находки.push({ кто, шаг: page.__шаг, вид: `HTTP ${r.status()}`, текст: `${r.request().method()} ${r.url().replace(BASE, '')} ${тело}` });
    }
  });
}

async function шаг(page, кто, имя, fn) {
  page.__шаг = имя;
  const t0 = Date.now();
  try {
    await fn();
    шаги.push({ кто, шаг: имя, ok: true, мс: Date.now() - t0 });
  } catch (e) {
    шаги.push({ кто, шаг: имя, ok: false, мс: Date.now() - t0, ошибка: String(e).split('\n')[0].slice(0, 300) });
  }
  кадр += 1;
  const файл = `${String(кадр).padStart(3, '0')}-${кто}-${имя}`.replace(/[^\p{L}\p{N}-]+/gu, '-').slice(0, 90) + '.png';
  try { await page.screenshot({ path: path.join(OUT, файл) }); } catch { /* страница закрылась */ }
}

async function войти(page, логин) {
  await page.goto(`${BASE}/#/login`);
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.locator('input[name="login"]').fill(логин);
  await page.locator('input[name="password"]').fill('arm112');
  await page.getByTestId('login-submit').click();
  await page.getByLabel('Поиск происшествий').or(page.getByText('Карточки для заполнения')).first().waitFor({ timeout: 15_000 });
}

// Раздел открывается ссылкой, если она есть на текущем экране; иначе —
// через журнал, как сделал бы человек (и это отмечается в находках)
async function в(page, ссылка, путь) {
  const l = page.getByTestId(ссылка);
  if (await l.isVisible().catch(() => false)) return l.click();
  находки.push({ кто: 'нав', шаг: page.__шаг, вид: 'нет ссылки', текст: `${ссылка} нет на ${page.url().replace(BASE, '')}` });
  await page.goto(`${BASE}/#${путь}`);
}

const выбрать = (page, группа, вариант) => page.getByRole('group', { name: группа })
  .getByRole('button', { name: вариант, exact: true }).click();

async function пожар(page) {
  await page.locator('#type-search').fill('101');
  await page.getByRole('option', { name: '101', exact: true }).click();
  await выбрать(page, 'Где', 'Дом');
  await выбрать(page, 'Признак пожара (дом)', 'Открытое пламя / Дым');
  await выбрать(page, 'Дом (пламя, дым)', 'Дом многоквартирный');
  await выбрать(page, 'Внутридомовые объекты (пламя, дым)', 'Квартира');
  await выбрать(page, 'Угроза людям', 'Да');
  await выбрать(page, 'Проведена ли газификация', 'Да');
}

// ─── Преподаватель ───────────────────────────────────────────────────
async function преподаватель(browser, размер) {
  const ctx = await browser.newContext({ viewport: размер, locale: 'ru-RU' });
  const page = await ctx.newPage();
  const кто = `преп-${размер.width}`;
  следить(page, кто);
  await шаг(page, кто, 'вход', () => войти(page, 'teacher1'));
  await шаг(page, кто, 'материал', () => page.getByTestId('scenarios-link').click());

  await шаг(page, кто, 'вызов-по-описанию', async () => {
    await page.getByTestId('create-prompt').click();
    await page.getByTestId('prompt-title').fill('UX: пожар в подвале');
    await page.getByTestId('prompt-text').fill('Мужчина сообщает: из подвала пятиэтажки идёт густой дым, жильцы выходят. Москва, улица Гарибальди, дом 23.');
    await page.getByTestId('prompt-submit').click();
    await page.getByTestId('scenario-title').filter({ hasText: 'UX: пожар в подвале' }).waitFor({ timeout: 120_000 });
  });
  await шаг(page, кто, 'правка-перепиской', async () => {
    await page.getByTestId('chat-input').fill('Сделай заявителя пожилой женщиной и добавь, что в подвале слышен треск.');
    await page.getByTestId('chat-send').click();
    await page.waitForTimeout(12_000);
  });
  await шаг(page, кто, 'утвердить-вызов', async () => {
    await page.getByTestId('scenario-view').getByTestId('approve').click();
    await page.getByTestId('scenario-view').getByText('утверждён').waitFor({ timeout: 10_000 });
  });

  await шаг(page, кто, 'вызов-вручную', async () => {
    await page.getByTestId('create-manual').click();
    const b = page.getByTestId('scenario-builder');
    await b.getByTestId('builder-title').fill('UX: пожар в квартире (вручную)');
    await b.getByTestId('builder-legend').fill('Алло! У соседей из окна дым и пламя!');
    await пожар(page);
    await b.getByLabel('Улица:').fill('Бутлерова');
    await b.getByLabel('Дом/Вл:').fill('8');
    await b.getByTestId('builder-q-0').fill('Пострадавшие есть?');
    await b.getByTestId('builder-a-0').fill('Не знаю, соседка вроде дома.');
    await b.getByTestId('builder-save').click();
    await page.getByTestId('scenario-view').getByTestId('scenario-title').filter({ hasText: 'вручную' }).waitFor({ timeout: 15_000 });
  });
  await шаг(page, кто, 'эталон-вручную', async () => {
    const outcome = await page.getByTestId('scenario-view').getByTestId('outcome').innerText();
    if (outcome.trim() === '—') throw new Error('эталон не посчитан');
    await page.getByTestId('scenario-view').getByTestId('approve').click();
  });

  await шаг(page, кто, 'происшествие-вручную', async () => {
    await page.getByTestId('tab-dds').click();
    await page.getByTestId('create-manual').click();
    const b = page.getByTestId('incident-builder');
    await b.getByTestId('incident-title').fill('UX: пожар в квартире (ДДС)');
    await b.getByLabel('ФИО заявителя').fill('Петрова Анна Ивановна');
    await b.getByLabel('Улица:').fill('Бутлерова');
    await b.getByLabel('Дом/Вл:').fill('8');
    await b.getByTestId('incident-description').fill('Из окна квартиры на 3 этаже пламя, в квартире может быть человек.');
    await пожар(page);
    await b.getByTestId('incident-save').click();
    await page.getByTestId('incident-view').getByTestId('incident-view-services').waitFor({ timeout: 15_000 });
    await page.getByTestId('incident-view').getByTestId('incident-approve').click();
  });
  await шаг(page, кто, 'происшествие-по-описанию', async () => {
    await page.getByTestId('create-prompt').click();
    await page.getByTestId('prompt-title').fill('UX: газ в подъезде (ДДС)');
    await page.getByTestId('prompt-text').fill('Запах газа в подъезде 9-этажного дома, источник не найден. Москва, улица Удальцова, дом 15.');
    await page.getByTestId('prompt-submit').click();
    await page.getByTestId('incident-view').waitFor({ timeout: 120_000 });
  });

  await шаг(page, кто, 'занятие-112-создать', async () => {
    await в(page, 'lessons-link', '/lessons');
    await page.getByTestId('lesson-group').selectOption({ label: 'Операторы 112 — группа 1' });
    await page.getByTestId('lesson-title').fill('UX: пожары, группа 1');
    await page.getByTestId('lesson-mode').selectOption('card112');
    const галочки = page.getByTestId('lesson-material').locator('input[type="checkbox"]');
    for (let i = 0; i < 3; i++) await галочки.nth(i).check();
    await page.getByTestId('lesson-create').click();
    await page.getByTestId('lesson-current').filter({ hasText: 'UX: пожары' }).waitFor();
  });
  await шаг(page, кто, 'занятие-112-старт', async () => {
    await page.getByTestId('lesson-start').click();
    await page.getByTestId('lesson-state').filter({ hasText: 'идёт' }).waitFor();
  });
  await шаг(page, кто, 'ход-занятия', async () => {
    await page.getByTestId('lesson-monitor-link').click();
    await page.getByTestId('lesson-monitor').waitFor();
  });
  return { ctx, page, кто };
}

async function преподаватель_после(t) {
  const { page, кто } = t;
  await шаг(page, кто, 'ход-занятия-во-время-работы', () => page.reload().then(() => page.getByTestId('lesson-monitor').waitFor()));
  await шаг(page, кто, 'экзамен-из-набора-старт', async () => {
    await в(page, 'lessons-link', '/lessons');
    await page.getByRole('button', { name: /Экзамен 112: пожары/ }).first().click();
    await page.getByTestId('lesson-start').click();
    await page.getByTestId('lesson-state').filter({ hasText: 'идёт' }).waitFor();
  });
  await шаг(page, кто, 'блиц-из-набора-старт', async () => {
    await page.getByRole('button', { name: /Тренировка по частям: адрес/ }).first().click();
    await page.getByTestId('lesson-start').click();
    await page.getByTestId('lesson-state').filter({ hasText: 'идёт' }).waitFor();
  });
  await шаг(page, кто, 'веса', async () => {
    await в(page, 'weights-link', '/weights');
    await page.getByRole('heading', { name: 'Веса оценки' }).waitFor();
    await page.getByTestId('weight-completeness').fill('5');
    await page.waitForTimeout(1500);
  });
  await шаг(page, кто, 'веса-сохранить', async () => {
    await page.getByTestId('weights-save').click();
    await page.waitForTimeout(2000);
  });
  await шаг(page, кто, 'аналитика-группа', async () => {
    await в(page, 'analytics-link', '/analytics');
    await page.getByTestId('analytics-people-tab').click();
    await page.getByTestId('people-analytics').waitFor();
  });
  await шаг(page, кто, 'аналитика-ученик', async () => {
    await page.getByTestId('people-table').locator('[data-testid^="student-"]').first().click();
    await page.getByTestId('student-analytics').waitFor();
  });
}

async function преподаватель_эксперт(t) {
  const { page, кто } = t;
  await шаг(page, кто, 'протокол-экзамена', async () => {
    await в(page, 'lessons-link', '/lessons');
    await page.getByRole('button', { name: /Экзамен 112: пожары/ }).first().click();
    await page.getByTestId('lesson-protocol').waitFor();
  });
  await шаг(page, кто, 'экспертная-оценка', async () => {
    await page.getByTestId('lesson-protocol').locator('a').first().click();
    const форма = page.getByTestId('expert-form');
    await форма.waitFor({ timeout: 15_000 });
    await форма.locator('input[name="expert-score"]').fill('85');
    await форма.locator('textarea[name="expert-comment"]').fill('Адрес верный, службы подобраны; уточняйте пострадавших.');
    await форма.getByRole('button').click();
    await page.getByTestId('expert-score').filter({ hasText: '85' }).waitFor({ timeout: 10_000 });
  });
  await шаг(page, кто, 'ход-занятия-после', async () => {
    await в(page, 'lessons-link', '/lessons');
    await page.getByRole('button', { name: /UX: пожары/ }).first().click();
    await page.getByTestId('lesson-monitor-link').click();
    await page.getByTestId('lesson-monitor').waitFor();
  });
}

// ─── Оператор 112 ────────────────────────────────────────────────────
async function оператор(browser, логин, размер, голос) {
  const ctx = await browser.newContext({ viewport: размер, locale: 'ru-RU', permissions: ['microphone'] });
  const page = await ctx.newPage();
  const кто = `${логин}-${размер.width}`;
  следить(page, кто);
  await шаг(page, кто, 'вход', () => войти(page, логин));
  await шаг(page, кто, 'разделы', async () => {
    for (const s of ['section-training', 'section-exam', 'section-blitz']) await page.getByTestId(s).click();
    await page.getByTestId('section-training').click();
  });
  await шаг(page, кто, 'начать-выданный', async () => {
    await page.getByRole('button', { name: 'Начать' }).first().click();
    await page.getByTestId('card-timer').waitFor();
  });
  await шаг(page, кто, 'вопрос-из-списка', async () => {
    await page.locator('[data-testid^="ask-"]').first().click();
    await page.waitForTimeout(1200);
  });
  await шаг(page, кто, 'вопрос-своими-словами', async () => {
    await page.getByTestId('hud-mode-free').click();
    await page.getByTestId('hud-free-input').fill('Скажите, пострадавшие есть?');
    await page.getByTestId('hud-free-send').click();
    await page.waitForTimeout(3000);
  });
  if (голос) {
    await шаг(page, кто, 'вопрос-голосом', async () => {
      await page.getByTestId('hud-talk').click();
      await page.waitForTimeout(3500);
      await page.getByTestId('hud-talk').click();
      await page.waitForTimeout(5000);
    });
  }
  await шаг(page, кто, 'адрес', async () => {
    const адрес = page.getByLabel('Адрес', { exact: false }).first();
    await page.getByLabel('Улица:').fill('Академика Королёва');
    await page.getByLabel('Дом/Вл:').fill('12');
    await адрес.isVisible();
  });
  await шаг(page, кто, 'тип-и-опросная-карта', () => пожар(page));
  await шаг(page, кто, 'второй-тип', async () => {
    await page.locator('#type-search').click();
    await page.locator('#type-search').fill('104');
    await page.getByRole('option', { name: '104', exact: true }).click();
    await выбрать(page, 'Признаки происшествия', 'Запах газа в помещении (в квартире, в доме)');
    await page.waitForTimeout(800);
  });
  await шаг(page, кто, 'описание', async () => {
    await page.getByLabel('Описание со слов заявителя').or(page.locator('textarea').first()).first()
      .fill('Горит квартира на 5 этаже, пламя из окна, дом газифицирован.');
  });
  await шаг(page, кто, 'F5-посреди-вызова', async () => {
    await page.reload();
    await page.getByTestId('card-timer').waitFor({ timeout: 15_000 });
    const улица = await page.getByLabel('Улица:').inputValue();
    if (!улица.includes('Королёва')) throw new Error(`после F5 улица «${улица}»`);
  });
  await шаг(page, кто, 'сохранить', async () => {
    await page.getByTestId('save-card').click();
    await page.getByTestId('score').waitFor({ timeout: 20_000 });
  });
  await шаг(page, кто, 'разбор-внизу', () => page.evaluate(() => window.scrollTo(0, document.body.scrollHeight)));
  await шаг(page, кто, 'повторно-только-просмотр', async () => {
    await page.getByRole('button', { name: 'Вернуться в журнал' }).click();
    await page.getByTestId('section-training').click();
    const первая = page.locator('[data-testid^="work-row-"]').first();
    await первая.getByRole('button', { name: 'Разбор' }).click();
    await page.getByTestId('score').waitFor();
  });
  return { ctx, page, кто };
}

async function оператор_экзамен(t) {
  const { page, кто } = t;
  await шаг(page, кто, 'экзамен', async () => {
    await page.getByRole('button', { name: 'Вернуться в журнал' }).or(page.getByTestId('journal-link')).first().click().catch(() => {});
    await page.goto(`${BASE}/#/journal`);
    await page.getByTestId('section-exam').click();
    await page.getByRole('button', { name: 'Начать' }).first().click();
    await page.getByTestId('card-timer').waitFor();
    await page.getByTestId('save-card').click();
    await page.waitForTimeout(2500);
  });
  await шаг(page, кто, 'экзамен-разбор-скрыт', async () => {
    const видно = await page.getByTestId('score').isVisible().catch(() => false);
    if (видно) throw new Error('разбор виден до конца экзамена');
  });
  await шаг(page, кто, 'блиц', async () => {
    await page.goto(`${BASE}/#/journal`);
    await page.getByTestId('section-blitz').click();
    await page.getByRole('button', { name: 'Начать' }).first().click();
    await page.getByTestId('card-timer').waitFor();
    await page.getByLabel('Улица:').fill('Бутлерова');
    await page.getByLabel('Дом/Вл:').fill('8');
    await page.getByTestId('save-card').click();
    await page.getByRole('heading', { name: 'Разбор блица' }).waitFor({ timeout: 15_000 });
  });
}

async function голос_wav() {
  const r = await fetch(`${VOICE}/tts`, { method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ text: 'Скажите, дом газифицирован?', voice: 'operator' }) });
  if (!r.ok) return null;
  const файл = path.join(os.tmpdir(), 'ux-вопрос.wav');
  fs.writeFileSync(файл, Buffer.from(await r.arrayBuffer()));
  return файл;
}

const wav = await голос_wav();
const b = await chromium.launch();
const bГолос = wav ? await chromium.launch({ args: ['--use-fake-ui-for-media-stream',
  '--use-fake-device-for-media-stream', `--use-file-for-fake-audio-capture=${wav}`] }) : b;

const t = await преподаватель(b, { width: 1600, height: 900 });
const операторы = await Promise.all([
  оператор(bГолос, 'op01', { width: 1920, height: 1080 }, Boolean(wav)),
  оператор(b, 'op02', { width: 1280, height: 720 }, false),
  оператор(b, 'op03', { width: 1600, height: 900 }, false),
]);
await преподаватель_после(t);
// Экзамен и блиц запущены преподавателем для группы 1 — op01..op03 в ней
await Promise.all(операторы.map(оператор_экзамен));
await преподаватель_эксперт(t);

await b.close();
if (bГолос !== b) await bГолос.close();
fs.writeFileSync(path.join(OUT, 'итог.json'), JSON.stringify({ шаги, находки }, null, 1));
const упало = шаги.filter((s) => !s.ok);
console.log(`шагов ${шаги.length}, упало ${упало.length}, находок ${находки.length}`);
for (const s of упало) console.log('✗', s.кто, s.шаг, '—', s.ошибка);
const свод = {};
for (const н of находки) { const k = `${н.вид} ${н.текст.slice(0, 120)}`; свод[k] = (свод[k] ?? 0) + 1; }
for (const [k, n] of Object.entries(свод)) console.log(`• ×${n} ${k}`);
