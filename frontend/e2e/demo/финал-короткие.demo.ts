import { expect, test, type Locator } from '@playwright/test';
import path from 'node:path';
import { Screen, Studio } from './студия';

/**
 * Короткие ролики финала (по 40–90 с), тем же конвейером, что и главный:
 *   «IP-телефон и справочник» — преподаватель правит телефоны ДДС,
 *   диспетчер звонит руководителю организации, журнал звонков;
 *   «Администратор» — система 112 включается только администратором с
 *   экрана входа, параметры тренажёра.
 *
 *   DEMO_OUT=<каталог>   кадры и звук (по умолчанию /tmp/arm112-demo/<ролик>)
 */

const PASSWORD = 'arm112';
const visible = async (l: Locator): Promise<boolean> => (await l.count()) > 0 && l.first().isVisible();

const login = async (s: Screen, user: string): Promise<void> => {
  const p = s.page;
  await p.goto('/#/login');
  await p.evaluate(() => localStorage.clear());
  await p.reload();
  await s.type(p.locator('input[name="login"]'), user, 50);
  await s.type(p.locator('input[name="password"]'), PASSWORD, 35);
  await s.click(p.getByTestId('login-submit'));
  await p.getByTestId('student-work').or(p.getByLabel('Поиск происшествий'))
    .first().waitFor({ timeout: 60_000 });
  await s.pause(400);
};

test('финал: IP-телефон и справочник', async ({ browser, baseURL }) => {
  test.setTimeout(900_000);
  const st = new Studio(process.env.DEMO_OUT ?? path.join('/tmp/arm112-demo', 'phone'));
  const T = await st.add(browser, 'teacher', 'ПК преподавателя · teacher1', baseURL!);
  const A = await st.add(browser, 'dds01', 'ПК обучаемого · dds01 · Служба 101', baseURL!);
  await st.start();
  try {
    st.card('IP-телефон диспетчера', 'Справочник службы ведёт преподаватель, диспетчер звонит с рабочего места');
    await st.say('IP-телефон диспетчера: справочник службы ведёт преподаватель, звонки идут с рабочего места и остаются в журнале.');
    st.hideCard();

    st.show(T);
    const p = T.page;
    await st.sayWhile('Преподаватель открывает «телефоны ДДС» и выбирает службу.', async () => {
      await login(T, 'teacher1');
      await T.click(p.getByTestId('phonebook-link'));
      await T.select(p.getByTestId('phonebook-service'), 'mchs');
      await expect(p.getByTestId('phonebook-row').first()).toBeVisible();
    });
    await st.sayWhile('В справочнике — старший бригады, начальник, руководитель организации. Преподаватель подписывает руководителя по фамилии и добавляет заявителя.', async () => {
      const rows = p.getByTestId('phonebook-row');
      const head = rows.filter({ has: p.locator('[data-testid="phonebook-role"][value="org_head"]') }).first();
      const row = (await head.count()) ? head : rows.nth(3);
      await T.type(row.getByTestId('phonebook-title'), 'Руководитель организации Петров', 35);
      const add = p.getByTestId('phonebook-add-applicant');
      if (await visible(add) && await add.isEnabled()) await T.click(add);
      await T.click(p.getByTestId('phonebook-save'));
      await expect(p.getByTestId('phonebook-note')).toContainText('Сохранено', { timeout: 15_000 });
    });

    st.show(A);
    const q = A.page;
    await st.sayWhile('Диспетчер на своём месте берёт происшествие от ИИ.', async () => {
      await login(A, 'dds01');
      await A.click(q.getByTestId('practice-ai'));
      await expect(q).toHaveURL(/#\/card-dds\//, { timeout: 180_000 });
      await expect(q.getByTestId('brigade-panel')).toBeVisible({ timeout: 60_000 });
    });
    await A.ring(q.getByTestId('phone-launcher'));
    await st.say('Телефон спрятан за зелёным кружком, чтобы не загромождать карточку.');
    await A.ring(null);
    await A.click(q.getByTestId('phone-launcher'));
    await expect(q.getByTestId('ip-phone')).toBeVisible();
    await A.ring(q.getByTestId('ip-phone'));
    await st.say('Статус линии, добавочный, телефонная книга службы — та самая, что заполнил преподаватель.');
    await A.ring(null);
    const org = q.getByTestId('phone-call-org_head');
    await st.sayWhile('Звонок руководителю организации — одним нажатием.', async () => {
      await A.click(org);
      await expect(q.getByTestId('phone-timer')).toBeVisible({ timeout: 15_000 });
    });
    const lines = q.getByTestId('phone-lines').locator('li');
    await st.app((await lines.first().innerText().catch(() => '')).replace(/^[^:]{0,40}:\s*/, ''), 'chief');
    await A.type(q.getByTestId('phone-text'), 'Докладываю: по вашему адресу направлена бригада, прибытие через десять минут.', 20);
    await A.click(q.getByTestId('phone-send'));
    await expect(lines).toHaveCount(3, { timeout: 60_000 });
    await st.app((await lines.last().innerText().catch(() => '')).replace(/^[^:]{0,40}:\s*/, ''), 'chief');
    await A.click(q.getByTestId('phone-hangup'));
    await st.sayWhile('Собеседника ведёт модель — он отвечает по карточке. Можно говорить и голосом: речь распознаётся на своём сервере.', async () => {
      await expect(q.getByTestId('phone-status')).toContainText('доступен', { timeout: 15_000 });
    });
    await A.click(q.getByTestId('phone-tab-log'));
    await A.ring(q.getByTestId('phone-log'));
    await st.say('Журнал вызовов: кому, когда, сколько длился разговор и запись. Его же видит преподаватель на карточке.');
    await A.ring(null);
    await st.pause(600);
  } finally {
    await st.stop();
  }
});

test('финал: администратор и система 112', async ({ browser, baseURL }) => {
  test.setTimeout(600_000);
  const st = new Studio(process.env.DEMO_OUT ?? path.join('/tmp/arm112-demo', 'admin'));
  const A = await st.add(browser, 'admin', 'ПК администратора', baseURL!);
  await st.start();
  const p = A.page;
  try {
    st.card('Администратор', 'Система 112 включается только администратором; параметры тренажёра');
    await st.say('Тренажёр по умолчанию — для диспетчеров ДДС. Приём вызовов [112|сто двенадцать] включает только администратор.');
    st.hideCard();
    st.show(A);
    await p.goto('/#/login');
    await p.evaluate(() => localStorage.clear());
    await p.reload();
    await p.getByTestId('op112-switch').waitFor({ timeout: 30_000 });
    await A.ring(p.getByTestId('op112-switch'));
    await st.say('Переключатель — прямо на экране входа, но спросит логин и пароль администратора.');
    await A.ring(null);
    await st.sayWhile('Администратор включает [112|сто двенадцать] — и у преподавателя появляются вызовы и учебный материал для операторов.', async () => {
      await A.click(p.getByTestId('op112-toggle'));
      await A.type(p.getByLabel('Логин администратора'), 'admin', 50);
      await A.type(p.getByLabel('Пароль администратора'), PASSWORD, 35);
      await A.click(p.getByTestId('op112-apply'));
      await expect(p.getByTestId('op112-state')).toContainText('включена', { timeout: 30_000 });
    });
    await st.pause(800);
    await st.sayWhile('Выключить так же просто.', async () => {
      await A.click(p.getByTestId('op112-toggle'));
      await A.type(p.getByLabel('Логин администратора'), 'admin', 50);
      await A.type(p.getByLabel('Пароль администратора'), PASSWORD, 35);
      await A.click(p.getByTestId('op112-apply'));
      await expect(p.getByTestId('op112-state')).toContainText('выключена', { timeout: 30_000 });
    });
    await st.sayWhile('Администратор входит в систему: пользователи, журнал действий, параметры.', async () => {
      await A.type(p.locator('input[name="login"]'), 'admin', 50);
      await A.type(p.locator('input[name="password"]'), PASSWORD, 35);
      await A.click(p.getByTestId('login-submit'));
      await p.waitForTimeout(1500);
      await p.goto('/#/admin');
      await A.click(p.getByTestId('admin-tab-settings'));
      await expect(p.getByTestId('settings-tab')).toBeVisible({ timeout: 30_000 });
    });
    await A.moveTo(p.getByTestId('setting-ui.phone'));
    await A.ring(p.getByTestId('setting-ui.phone'));
    await st.say('Параметры: нормативы, шкала бригады, телефон, веса оценки — всё меняется здесь, без правки кода.');
    await A.ring(null);
    const demoTab = p.getByTestId('admin-tab-demo');
    if (await visible(demoTab)) {
      await st.sayWhile('Для показа есть демо-статистика: одна кнопка заполняет стенд учебными группами, занятиями и попытками.', async () => {
        await A.click(demoTab);
        await expect(p.getByTestId('demo-stats-panel')).toBeVisible({ timeout: 30_000 });
        await A.moveTo(p.getByTestId('demo-stats-panel'));
      });
      const fill = p.getByTestId('demo-stats-fill');
      if (await visible(fill) && await fill.isEnabled()) {
        await A.click(fill);
        await expect(p.getByTestId('demo-stats-note')).toBeVisible({ timeout: 120_000 }).catch(() => {});
      }
      await A.ring(p.getByTestId('demo-stats-state'));
      await st.say('Очистить её так же просто — останутся только учётные записи и учебный материал.');
      await A.ring(null);
    }
    await st.pause(600);
  } finally {
    await st.stop();
  }
});
