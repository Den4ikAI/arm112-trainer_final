// Шаг 4: узкие экраны 390×844 и 768×1024. На каждом экране — есть ли
// прокрутка вбок, помещаются ли кнопки по ширине и не перекрыты ли они.
import { browser, page, login, shot, log, hscroll, BASE } from './kit.mjs';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const b = await browser();
const LESSON = process.env.UX_LESSON;
const DDS_CARD = process.env.UX_DDS_CARD;
const NUMBER112 = process.env.UX_NUMBER112;

/** Кнопки и поля, которые не помещаются по ширине или перекрыты. */
async function reach(p) {
  return p.evaluate(() => {
    const W = document.documentElement.clientWidth;
    const bad = [];
    for (const el of document.querySelectorAll('button, select, input, a, [role="tab"]')) {
      const r = el.getBoundingClientRect();
      if (r.width === 0 || r.height === 0) continue;
      const name = (el.getAttribute('aria-label') || el.textContent || el.getAttribute('data-testid') || el.tagName).trim().replace(/\s+/g, ' ').slice(0, 40);
      if (r.right > W + 1 || r.left < -1) { bad.push(`за краем: ${name} (${Math.round(r.left)}…${Math.round(r.right)})`); continue; }
      // Перекрытие проверяем только у видимых по высоте
      if (r.top < 0 || r.bottom > window.innerHeight) continue;
      const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      if (hit && hit !== el && !el.contains(hit) && !hit.contains(el)) {
        const by = (hit.getAttribute('data-testid') || hit.className || hit.tagName).toString().slice(0, 40);
        bad.push(`перекрыт: ${name} ← ${by}`);
      }
    }
    return bad;
  });
}

/** Текст, обрезанный без многоточия и без прокрутки. */
async function clipped(p) {
  return p.evaluate(() => {
    const out = [];
    for (const el of document.querySelectorAll('body *')) {
      if (el.children.length) continue;
      const cs = getComputedStyle(el);
      if (el.scrollWidth > el.clientWidth + 2 && cs.overflowX === 'hidden' && cs.textOverflow !== 'ellipsis' && el.clientWidth > 0) {
        out.push((el.textContent || '').trim().slice(0, 40));
      }
    }
    return out.slice(0, 8);
  });
}

async function check(p, who, title) {
  await sleep(1500);
  const hs = await hscroll(p);
  const bad = await reach(p);
  const cl = await clipped(p);
  log(`[${who}] ${title}: вбок ${hs}px; ${bad.length ? bad.slice(0, 8).join('; ') : 'кнопки на месте'}${cl.length ? '; обрезано: ' + cl.join(' | ') : ''}`);
  await shot(p, `${title}`);
}

for (const [w, h] of [[390, 844], [768, 1024]]) {
  const tag = `${w}`;
  // Вход
  const p0 = await page(b, `гость-${tag}`, { width: w, height: h });
  await p0.goto(`${BASE}/#/login`);
  await check(p0, `гость-${tag}`, `${tag}-вход`);
  await p0.context().close();

  // Диспетчер ДДС: разделы и карточка
  const pd = await page(b, `dds01-${tag}`, { width: w, height: h });
  await login(pd, 'dds01');
  await check(pd, `dds01-${tag}`, `${tag}-ддс-разделы`);
  if (DDS_CARD) {
    await pd.goto(`${BASE}/#/card-dds/${DDS_CARD}`);
    await check(pd, `dds01-${tag}`, `${tag}-карточка-ддс`);
    await pd.getByTestId('pencil-mchs').click().catch(() => log(`[dds01-${tag}] карандаш МЧС не нажимается`));
    await check(pd, `dds01-${tag}`, `${tag}-карточка-ддс-статус`);
  }
  await pd.context().close();

  // Оператор 112: разделы и карточка
  const po = await page(b, `op01-${tag}`, { width: w, height: h });
  await login(po, 'op01');
  await check(po, `op01-${tag}`, `${tag}-112-разделы`);
  const start = po.locator('[data-testid^="launch-"]').first();
  if (await start.count()) {
    await start.click();
    await sleep(3000);
    await check(po, `op01-${tag}`, `${tag}-карточка-112`);
  } else if (NUMBER112) {
    await po.goto(`${BASE}/#/card/${NUMBER112}`);
    await check(po, `op01-${tag}`, `${tag}-карточка-112`);
  }
  await po.context().close();

  // Преподаватель: занятия, ход занятия, веса
  const pt = await page(b, `teacher1-${tag}`, { width: w, height: h });
  await login(pt, 'teacher1');
  await pt.goto(`${BASE}/#/lessons`);
  await check(pt, `teacher1-${tag}`, `${tag}-занятия`);
  if (LESSON) {
    await pt.goto(`${BASE}/#/lessons/${LESSON}/monitor`);
    await check(pt, `teacher1-${tag}`, `${tag}-ход-занятия`);
  }
  await pt.goto(`${BASE}/#/weights`);
  await check(pt, `teacher1-${tag}`, `${tag}-веса`);
  await pt.context().close();

  // Администратор
  const pa = await page(b, `admin-${tag}`, { width: w, height: h });
  await login(pa, 'admin');
  await pa.goto(`${BASE}/#/admin`);
  await check(pa, `admin-${tag}`, `${tag}-админ`);
  await pa.getByTestId('admin-tab-users').click().catch(() => {});
  await check(pa, `admin-${tag}`, `${tag}-админ-пользователи`);
  await pa.context().close();
}
await b.close();
