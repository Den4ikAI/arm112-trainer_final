// Шаг 2: dds01..dds05 параллельно, каждый в своём окне.
// Лента без перезагрузки, приём за 30 с, «Принята» с текстом, бригада
// (доклад сам, звонок кнопкой, звонок голосом), статусы по фактам,
// отказ без комментария, доклад начальнику, разбор, просмотр сделанного,
// доступ к чужому и к карточкам 112.
//
// Голос: Chromium подставляет вместо микрофона WAV (путь в UX_WAV),
// синтезированный сервером голоса.
import { browser, page, login, shot, log, api, token, BASE } from './kit.mjs';

const WAV = process.env.UX_WAV;
const b = await browser([
  '--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream',
  ...(WAV ? [`--use-file-for-fake-audio-capture=${WAV}`] : []),
]);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const SVC = { dds01: 'mchs', dds02: 'mosgaz', dds03: 'smp', dds04: 'codd', dds05: 'moslift' };
const STAGE_WORDS = {
  'Начало реагирования': 'выехали на вызов', 'Прибытие': 'прибыли на место',
  'Проведение работ': 'приступили к работам', 'Работы завершены': 'работы завершены',
};

const teacher = await token('teacher1');
const work = async (who, section) => (await api('GET', `/api/me/work?section=${section}`, null, await token(who))).json;

async function statusOptions(p, svc) {
  return p.getByTestId(`status-editor-${svc}`).getByLabel('Статус', { exact: true }).locator('option').allTextContents();
}

async function setStatus(p, who, svc, status, comment) {
  const ed = p.getByTestId(`status-editor-${svc}`);
  if (!(await ed.count())) await p.getByTestId(`pencil-${svc}`).click();
  const opts = await statusOptions(p, svc);
  if (!opts.includes(status)) {
    log(`[${who}] статуса «${status}» нет в списке:`, opts.join(' | '));
    await ed.getByLabel('Отменить').click().catch(() => {});
    return false;
  }
  await ed.getByLabel('Статус', { exact: true }).selectOption({ label: status });
  if (comment != null) await ed.getByLabel('Комментарий').fill(comment);
  await ed.getByLabel('Сохранить статус').click();
  await sleep(1200);
  if (await ed.count()) {
    const err = (await ed.textContent()) ?? '';
    log(`[${who}] «${status}» не сохранён, редактор открыт:`, err.replace(/\s+/g, ' ').slice(-120));
    return false;
  }
  log(`[${who}] проставлен «${status}»`);
  return true;
}

const panelText = async (p) => ((await p.getByTestId('brigade-panel').textContent().catch(() => '')) ?? '').replace(/\s+/g, ' ');

async function waitPanel(p, words, ms) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    if ((await panelText(p)).toLowerCase().includes(words)) return true;
    await sleep(1000);
  }
  return false;
}

/** Полный проход одной карточки: приём, бригада, статусы, доклад начальнику. */
async function fullCard(p, who, cardId, { refusal = false, voice = false } = {}) {
  const svc = SVC[who];
  const row = p.getByTestId(`work-row-${cardId}`);
  if (!(await row.count())) { log(`[${who}] нет строки карточки ${cardId} в списке`); return; }
  const t0 = Date.now();
  await row.getByRole('button', { name: 'Открыть' }).click();
  await p.waitForURL(/card-dds/, { timeout: 10000 }).catch(() => log(`[${who}] «Открыть» не перевёл в карточку ДДС: ${p.url()}`));
  await p.getByTestId(`pencil-${svc}`).waitFor({ timeout: 10000 }).catch(() => log(`[${who}] нет карандаша своей службы`));
  log(`[${who}] карточка открыта за ${((Date.now() - t0) / 1000).toFixed(1)} с`);
  await shot(p, `${who}-карточка-открыта`);

  // Бригада до «Принята» — только подсказка
  const early = await panelText(p);
  if (!early.includes('после статуса «Принята»')) log(`[${who}] до «Принята» нет подсказки о бригаде:`, early.slice(0, 120));

  if (!(await setStatus(p, who, svc, 'Принята', 'Карточку принял, направляю бригаду'))) {
    // Статус мог быть уже проставлен соседом той же службы
    log(`[${who}] «Принята» не проставлена`);
  }

  if (refusal) {
    await p.getByTestId(`pencil-${svc}`).click();
    const opts = await statusOptions(p, svc);
    const ref = opts.find((o) => /Отказ/i.test(o));
    if (!ref) log(`[${who}] после «Принята» отказа в списке нет:`, opts.join(' | '));
    else {
      const ed = p.getByTestId(`status-editor-${svc}`);
      await ed.getByLabel('Статус', { exact: true }).selectOption({ label: ref });
      await ed.getByLabel('Сохранить статус').click();
      await sleep(1000);
      const txt = ((await ed.textContent().catch(() => '')) ?? '').replace(/\s+/g, ' ');
      log(`[${who}] отказ без комментария:`, /комментарий обязателен/i.test(txt) ? 'отклонён с подсказкой' : `НЕ отклонён: «${txt.slice(-100)}»`);
      await shot(p, `${who}-отказ-без-комментария`);
      if (await ed.count()) await ed.getByLabel('Отменить').click();
    }
  }

  // Бригада
  const assign = p.getByTestId('brigade-assign');
  if (!(await assign.count())) { log(`[${who}] нет кнопки «Направить бригаду»:`, (await panelText(p)).slice(0, 150)); return; }
  await p.getByTestId('brigade-select').selectOption('1');
  await assign.click();
  const ta = Date.now();
  await sleep(1500);
  const afterAssign = await panelText(p);
  log(`[${who}] после назначения:`, afterAssign.slice(0, 220));
  await shot(p, `${who}-бригада-направлена`);
  const at = (s) => sleep(Math.max(0, ta + s * 1000 - Date.now()));

  // Этап 1 — ждём, что старший позвонит сам
  if (await waitPanel(p, STAGE_WORDS['Начало реагирования'], 70000)) {
    const inc = await p.getByTestId('brigade-incoming').count();
    log(`[${who}] доклад о выезде пришёл через ${((Date.now() - ta) / 1000).toFixed(0)} с, входящий показан: ${inc > 0}`);
    await setStatus(p, who, svc, 'Начало реагирования', 'Бригада выехала');
  } else log(`[${who}] доклада о выезде не дождались за 70 с`);

  // Этап 2 — звоним старшему кнопкой
  await at(62);
  await p.getByTestId('brigade-call').click();
  await sleep(2500);
  const ans = (await p.getByTestId('brigade-answer').textContent().catch(() => '')) ?? '';
  log(`[${who}] ответ старшего по кнопке: «${ans}»`);
  if (/прибыли/i.test(ans)) await setStatus(p, who, svc, 'Прибытие', 'Прибыли на место, со слов старшего');

  // Этап 3 — голосом (или кнопкой, если голоса нет)
  await at(83);
  const talk = p.getByTestId('brigade-talk');
  if (voice && await talk.count()) {
    await talk.click();
    await sleep(4500);
    await talk.click();
    await p.getByTestId('brigade-said').waitFor({ timeout: 30000 }).catch(() => {});
    const said = (await p.getByTestId('brigade-said').textContent().catch(() => '')) ?? '';
    await sleep(2500);
    const ans3 = (await p.getByTestId('brigade-answer').textContent().catch(() => '')) ?? '';
    log(`[${who}] голосом: распознано «${said}», ответ «${ans3}»`);
    await shot(p, `${who}-голосом`);
  } else {
    if (voice) log(`[${who}] кнопки голоса нет:`, await p.getByTestId('brigade-talk-blocked').getAttribute('title').catch(() => 'и блокировки тоже'));
    await p.getByTestId('brigade-call').click();
    await sleep(2500);
  }
  if (await waitPanel(p, STAGE_WORDS['Проведение работ'], 5000)) await setStatus(p, who, svc, 'Проведение работ', 'Приступили к работам');
  else log(`[${who}] на 83 с старший не сказал «приступили к работам»`);

  // Этап 4 — ждём звонка старшего
  if (await waitPanel(p, STAGE_WORDS['Работы завершены'], 110000)) {
    log(`[${who}] доклад о завершении через ${((Date.now() - ta) / 1000).toFixed(0)} с`);
    await setStatus(p, who, svc, 'Работы завершены', 'Работы завершены, бригада свободна');
  } else log(`[${who}] доклада о завершении не дождались`);
  await shot(p, `${who}-работы-завершены`);
  const opts = await (async () => { await p.getByTestId(`pencil-${svc}`).click().catch(() => {}); const o = await statusOptions(p, svc).catch(() => []); await p.getByTestId(`status-editor-${svc}`).getByLabel('Отменить').click().catch(() => {}); return o; })();
  log(`[${who}] после «Работы завершены» в списке:`, opts.join(' | ') || '(редактор не открылся)');

  // Доклад начальнику: докладываем не всё — он должен спросить остальное
  const start = p.getByTestId('chief-start');
  if (await start.count()) {
    await start.click();
    await sleep(4000);
    const box = p.getByPlaceholder('Докладывайте…');
    await box.fill('Товарищ начальник, по карточке бригада номер один выезжала, работы завершены.');
    await p.getByTestId('chief-send').click();
    await p.waitForFunction(() => {
      const l = document.querySelector('[data-testid="chief-lines"]');
      return l && l.querySelectorAll('li').length >= 3;
    }, null, { timeout: 90000 }).catch(() => log(`[${who}] начальник не ответил за 90 с`));
    await sleep(1000);
    const lines = await p.getByTestId('chief-lines').locator('li').allTextContents();
    log(`[${who}] разговор с начальником:`, lines.map((l) => l.replace(/\s+/g, ' ')).join(' || '));
    await shot(p, `${who}-начальник`);
    await p.getByTestId('chief-finish').click().catch(() => log(`[${who}] нет кнопки завершения доклада`));
    await sleep(6000);
    await shot(p, `${who}-начальник-принял`);
  } else log(`[${who}] нет кнопки доклада начальнику`);

  const score = await p.getByTestId('dds-score').textContent().catch(() => '');
  const findings = await p.getByTestId('dds-finding').allTextContents();
  log(`[${who}] разбор: ${score}; замечания: ${findings.join(' | ')}`);
  await p.getByTestId('attempt-review').scrollIntoViewIfNeeded().catch(() => {});
  await shot(p, `${who}-разбор`);
}

async function student(who, plan) {
  const p = await page(b, who);
  await login(p, who);
  const tabs = await Promise.all(['training', 'exam', 'blitz'].map((k) => p.getByTestId(`section-${k}`).count()));
  log(`[${who}] разделы (обучение, экзамен, блиц):`, tabs.join(','));
  await shot(p, `${who}-главная`);
  await plan(p);
  return p;
}

// ─── Лента без перезагрузки: преподаватель запускает новое занятие, пока
// курсанты сидят на главной
const groups = (await api('GET', '/api/groups', null, teacher)).json;
const g1 = groups.find((g) => g.title === 'Диспетчеры ДДС — группа 1').id;
const incidents = (await api('GET', '/api/dds-incidents', null, teacher)).json;
const byTitle = (t) => incidents.find((i) => i.title === t)?.id;

const ids = {};
for (const w of ['dds01', 'dds02', 'dds03', 'dds04', 'dds05']) ids[w] = (await work(w, 'training')).map((r) => r.cardId);

const pages = {};
await Promise.all(Object.keys(SVC).map(async (w) => { pages[w] = await student(w, async () => {}); }));
const before = {};
for (const w of Object.keys(SVC)) before[w] = await pages[w].locator('[data-testid^="work-row-"]').count();

const streamIds = ['Запах газа — происшествие 2', 'ДТП с пострадавшими — происшествие 1', 'Застрял лифт — происшествие 2'].map(byTitle).filter(Boolean);
log('[teacher1] поток-проверка, происшествий найдено:', streamIds.length);
const created = await api('POST', '/api/lessons', {
  groupId: g1, title: 'Проверка ленты без перезагрузки', mode: 'dds', kind: 'training',
  cardIntervalSec: 10, cardIds: streamIds,
}, teacher);
log('[teacher1] создано:', created.status, created.json?.id ?? JSON.stringify(created.json).slice(0, 200));
const st = await api('POST', `/api/lessons/${created.json.id}/start`, {}, teacher);
log('[teacher1] запущено:', st.status);
await sleep(35000);
for (const w of Object.keys(SVC)) {
  const after = await pages[w].locator('[data-testid^="work-row-"]').count();
  const want = (await work(w, 'training')).length;
  log(`[${w}] строк на экране до ${before[w]}, через 35 с ${after}, по серверу ${want}`);
}
await shot(pages.dds02, 'dds02-лента-после-запуска');

// ─── Полные проходы параллельно
const shared = '01ab8c3f-2493-4fae-87b3-0d847198293c'; // 101, Маршала Бирюзова: и МЧС, и ЦОДД
const dtp = '3f7378ff-4aec-4e4d-b1b8-da4d8ddd84dd';   // ДТП, Профсоюзная: и СМП, и ЦОДД
const fresh = async (w) => (await work(w, 'training')).find((r) => !ids[w].includes(r.cardId))?.cardId;

await Promise.all([
  fullCard(pages.dds01, 'dds01', shared, { refusal: true }),
  (async () => { await sleep(8000); await fullCard(pages.dds04, 'dds04', shared, { voice: true }); })(),
  fullCard(pages.dds03, 'dds03', dtp, { voice: true }),
  (async () => { const c = await fresh('dds02'); if (c) await fullCard(pages.dds02, 'dds02', c); else log('[dds02] новой карточки нет'); })(),
  (async () => {
    // dds05 — доступ к чужому
    const p = pages.dds05;
    const c112 = process.env.UX_CARD112 ? { id: process.env.UX_CARD112, number: process.env.UX_NUMBER112 } : null;
    if (c112) {
      await p.goto(`${BASE}/#/card/${c112.number}`); await sleep(2500);
      log('[dds05] /card/<номер 112>:', p.url().replace(BASE, ''), '—', ((await p.locator('body').textContent()) ?? '').replace(/\s+/g, ' ').slice(0, 150));
      await shot(p, 'dds05-карточка-112-по-ссылке');
      await p.goto(`${BASE}/#/card-dds/${c112.id}`); await sleep(2500);
      log('[dds05] /card-dds/<карточка 112 не своей службы>:', ((await p.locator('body').textContent()) ?? '').replace(/\s+/g, ' ').slice(0, 150));
      await shot(p, 'dds05-карточка-112-как-ддс');
      const r = await api('GET', `/api/cards/${c112.id}`, null, await token('dds05'));
      log('[dds05] GET /api/cards/<112>:', r.status);
      const r1 = await api('GET', `/api/cards/${c112.id}`, null, await token('dds01'));
      log('[dds01] GET /api/cards/<112, адресована МЧС>:', r1.status);
    } else log('[dds05] UX_CARD112 не задан');
    await p.goto(`${BASE}/#/card-dds/${shared}`); await sleep(2500);
    log('[dds05] карточка другой службы по ссылке:', ((await p.locator('body').textContent()) ?? '').replace(/\s+/g, ' ').slice(0, 200));
    await shot(p, 'dds05-карточка-не-своей-службы');
    for (const path of ['/debrief', '/lessons', '/admin', '/weights']) {
      await p.goto(`${BASE}/#${path}`); await sleep(1500);
      log(`[dds05] ${path} →`, p.url().replace(BASE, ''), ((await p.locator('body').textContent()) ?? '').replace(/\s+/g, ' ').slice(0, 100));
    }
    await p.goto(`${BASE}/#/journal`); await sleep(1500);
  })(),
  (async () => {
    // Соседи по одной карточке: МЧС (dds01) ставит статусы, у ЦОДД (dds04)
    // вкладка МЧС должна меняться без обновления, а свои статусы — нет
    await sleep(20000);
    const p4 = pages.dds04;
    const tabM = await p4.getByTestId('service-tab-mchs').textContent().catch(() => '(нет вкладки)');
    const tabC = await p4.getByTestId('service-tab-codd').textContent().catch(() => '(нет вкладки)');
    log('[dds04] на 20 с: вкладка МЧС «' + tabM + '», ЦОДД «' + tabC + '», карандаш МЧС:', await p4.getByTestId('pencil-mchs').count());
  })(),
]);

// Сделанное — только просмотр
for (const w of ['dds01', 'dds03']) {
  const p = pages[w];
  await p.goto(`${BASE}/#/journal`); await sleep(2500);
  const rows = await work(w, 'training');
  const done = rows.find((r) => r.status === 'done');
  log(`[${w}] статусы строк:`, rows.map((r) => `${r.cardNumber}:${r.status}:${r.score ?? '—'}`).join(' '));
  await shot(p, `${w}-список-после`);
  if (done) {
    await p.getByTestId(`work-row-${done.cardId}`).getByRole('button').first().click();
    await sleep(2500);
    log(`[${w}] сделанная карточка: карандаш своей службы ${await p.getByTestId(`pencil-${SVC[w]}`).count()}, «Направить» ${await p.getByTestId('brigade-assign').count()}, «Позвонить» ${await p.getByTestId('brigade-call').count()}, «Начать доклад» ${await p.getByTestId('chief-start').count()}`);
    await shot(p, `${w}-сделанная-карточка`);
  }
}

await b.close();
