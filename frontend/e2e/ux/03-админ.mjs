// Шаг 3: администратор. Завести пользователя, отключить (токен должен
// умереть сразу), удалить с резервной копией; «Параметры», «Состояние»,
// журнал действий, версии классификатора.
import { browser, page, login, shot, log, api, token, BASE } from './kit.mjs';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const b = await browser();
const p = await page(b, 'admin');
p.on('dialog', (d) => { log('[admin] подтверждение:', d.message().replace(/\s+/g, ' ').slice(0, 160)); void d.accept(); });
await login(p, 'admin');
log('[admin] после входа:', p.url().replace(BASE, ''));
await shot(p, 'админ-вход');
if (!p.url().includes('/admin')) {
  await p.goto(`${BASE}/#/admin`);
  await sleep(1500);
}

// Пользователи: завести диспетчера ДДС
await p.getByTestId('admin-tab-users').click();
await sleep(1000);
await shot(p, 'админ-пользователи');
const newLogin = `ux2-${Date.now().toString(36)}`;
await p.getByTestId('add-user').click();
await p.getByLabel('Логин', { exact: true }).fill(newLogin);
await p.getByLabel('Пароль', { exact: true }).fill('arm112arm');
await p.getByLabel('ФИО', { exact: true }).fill('Проверочный Диспетчер УХ');
await p.getByLabel('Роль', { exact: true }).selectOption('student');
await p.getByLabel('Рабочее место', { exact: true }).selectOption('dds');
// Без службы — отказ формы
await p.getByTestId('save-user').click();
await sleep(800);
log('[admin] без службы: пользователь появился?', await p.getByTestId(`user-${newLogin}`).count());
const services = await p.getByLabel('Служба', { exact: true }).locator('option').allTextContents();
log('[admin] служб в списке:', services.length, services.slice(0, 4).join(' | '));
await p.getByLabel('Служба', { exact: true }).selectOption({ index: 1 });
await p.getByTestId('save-user').click();
await sleep(1500);
await shot(p, 'админ-заведён');
log('[admin] заведён:', await p.getByTestId(`user-${newLogin}`).count());

// Его токен — живой; отключаем — токен должен перестать работать
const r0 = await api('POST', '/api/auth/login', { login: newLogin, password: 'arm112arm' });
const tok = r0.json?.token;
log('[новый] вход:', r0.status, '; /api/me/work:', (await api('GET', '/api/me/work?section=training', null, tok)).status);
const row = p.getByTestId(`user-${newLogin}`);
await row.getByRole('button', { name: 'отключить' }).click();
await sleep(1200);
log('[новый] после отключения /api/me/work:', (await api('GET', '/api/me/work?section=training', null, tok)).status,
  '; повторный вход:', (await api('POST', '/api/auth/login', { login: newLogin, password: 'arm112arm' })).status);
await shot(p, 'админ-отключён');
await row.getByRole('button', { name: 'включить' }).click();
await sleep(1000);
log('[новый] после включения старый токен:', (await api('GET', '/api/me/work?section=training', null, tok)).status);

// Удалить: без истории — удаляется полностью
await p.getByTestId(`delete-${newLogin}`).click();
await sleep(1500);
log('[admin] уведомление:', await p.getByTestId('user-notice').textContent().catch(() => '(нет)'));
await shot(p, 'админ-удалён');

// Удалить с историей: dds05 работал сегодня. Удаляем op10 — проверим,
// есть ли у него история; dds05 не трогаем, он нужен дальше
const rowOp = p.getByTestId('user-op10');
if (await rowOp.count()) {
  log('[admin] op10 занятий:', await rowOp.locator('td').nth(6).textContent());
}

// Параметры: поменять и вернуть
await p.getByTestId('admin-tab-settings').click();
await sleep(1000);
const set = p.getByTestId('setting-brigade.count');
const inp = set.locator('input');
const old = await inp.inputValue();
await inp.fill('abc');
await set.getByRole('button', { name: 'сохранить' }).click();
await sleep(800);
log('[admin] параметр «abc»:', (await set.textContent()).replace(/\s+/g, ' ').slice(-80));
await inp.fill('4');
await set.getByRole('button', { name: 'сохранить' }).click();
await sleep(800);
log('[admin] параметр 4:', (await set.textContent()).replace(/\s+/g, ' ').slice(-60));
await inp.fill(old);
await set.getByRole('button', { name: 'сохранить' }).click();
await sleep(800);
await shot(p, 'админ-параметры');

// Состояние
await p.getByTestId('admin-tab-health').click();
await sleep(2500);
log('[admin] состояние:', ((await p.getByTestId('health').textContent().catch(() => '(нет)')) ?? '').replace(/\s+/g, ' ').slice(0, 400));
await shot(p, 'админ-состояние');

// Журнал действий
await p.getByTestId('admin-tab-audit').click();
await sleep(1500);
log('[admin] журнал: всего', await p.getByTestId('audit-total').textContent().catch(() => '?'), '; строк', await p.getByTestId('audit-row').count());
const actSel = p.getByLabel('Фильтр по действию');
if (await actSel.count()) {
  const tag = await actSel.evaluate((e) => e.tagName);
  if (tag === 'SELECT') {
    const opts = await actSel.locator('option').allTextContents();
    log('[admin] действий в фильтре:', opts.length);
    if (opts.length > 1) await actSel.selectOption({ index: 1 });
  } else await actSel.fill('user');
  await sleep(1200);
  log('[admin] после фильтра строк:', await p.getByTestId('audit-row').count());
}
await shot(p, 'админ-журнал');

// Классификатор
await p.getByTestId('admin-tab-classifier').click();
await sleep(1500);
log('[admin] версий:', await p.getByTestId('version-row').count(), '; активная:', ((await p.getByTestId('version-active').first().textContent().catch(() => '(нет)')) ?? '').replace(/\s+/g, ' ').slice(0, 80));
await shot(p, 'админ-классификатор');

await b.close();
