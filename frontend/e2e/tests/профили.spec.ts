import { test, expect, type APIRequestContext } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { go, login, PASSWORD, USERS, выдатьВызов } from './helpers';

/**
 * Два рабочих места в одном интерфейсе: оператор 112 и диспетчер ДДС.
 *
 * Организатор, чат 18.09: «один интерфейс, одна учётка, две мини-роли».
 * Денис со стенда: диспетчер ДДС видел билеты 112 и «Продолжить»
 * незаконченный вызов — «убери возможность заполнять карточку 112 у
 * оператора ДДС». Проверяем экран каждого профиля и то, что чужой экран
 * по прямой ссылке объясняет отказ, а сервер действие не пропускает.
 */

const БАЗА =
  (process.env.TEST_DATABASE_URL ?? 'arm112_test').split('/').pop() ?? 'arm112_test';
const API = `http://127.0.0.1:${process.env.E2E_API_PORT ?? '8081'}`;

const sql = (запрос: string): string =>
  execFileSync('docker', [
    'exec', 'arm112-db', 'psql', '-U', 'arm112', '-d', БАЗА, '-qAtc', запрос,
  ]).toString().trim();

const войти = async (request: APIRequestContext, user: string): Promise<string> => {
  const r = await request.post(`${API}/api/auth/login`, { data: { login: user, password: PASSWORD } });
  expect(r.ok()).toBeTruthy();
  return (await r.json()).token as string;
};

test.describe('Профили рабочего места', () => {
  test('у диспетчера ДДС в журнале нет билетов 112 и нет «Продолжить»', async ({ page }, info) => {
    await login(page, info, USERS.mchs);

    await expect(page.getByTestId('duty-badge')).toContainText('Диспетчер ДДС');
    await expect(page.getByRole('heading', { name: 'Учебные вызовы' })).toHaveCount(0);
    await expect(page.locator('[data-testid^="launch-t"]')).toHaveCount(0);
    await expect(page.getByTestId('unfinished-call')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Продолжить' })).toHaveCount(0);
    await page.screenshot({ path: info.outputPath('журнал-ддс.png'), fullPage: true });

    // Прямая ссылка на приём вызова — объяснение, а не пустой экран
    await go(page, info, '/card/1');
    await expect(page.getByTestId('not-for-duty')).toContainText('Недоступно для вашего профиля');
    await expect(page.getByTestId('not-for-duty')).toContainText('оператора 112');
    await expect(page.getByTestId('card-timer')).toHaveCount(0);
  });

  test('у оператора 112 — выданные карточки для заполнения, списка происшествий нет',
    async ({ page }, info) => {
    выдатьВызов(USERS.operator, 3, 1);
    await login(page, info, USERS.operator);

    await expect(page.getByTestId('duty-badge')).toHaveText('Оператор 112');
    // Денис, 29.09: у оператора 112 нет списка происшествий — только
    // разделы с выданными карточками
    await expect(page.getByLabel('Поиск происшествий')).toHaveCount(0);
    await expect(page.locator('[data-testid^="journal-row-"]')).toHaveCount(0);
    await expect(page.getByTestId('section-training')).toHaveText('Учебные вызовы');
    await expect(page.getByTestId('section-exam')).toHaveText('Экзамены');
    await expect(page.getByTestId('section-blitz')).toHaveText('Тренировка по частям');
    await page.getByTestId('section-training').click();
    await expect(page.getByTestId('launch-t3-c1').first()).toBeVisible();
    await page.screenshot({ path: info.outputPath('журнал-112.png'), fullPage: true });

    // Чужая карточка ДДС (ушла только в МЧС) оператору не открывается
    const номер = `Е${Date.now()}`;
    const карточка = sql(
      `INSERT INTO cards (number, saved_at, type_ids, description)
       VALUES ('${номер}', now(), '{101}', 'проверка профилей') RETURNING id`);
    sql(`INSERT INTO card_services (card_id, service_id) VALUES ('${карточка}', 'mchs')`);
    await go(page, info, `/card-dds/${карточка}`);
    await expect(page.getByText('Карточка недоступна')).toBeVisible();
    sql(`DELETE FROM cards WHERE id = '${карточка}'`);
  });

  test('переведённый в ДДС не продолжит начатый вызов 112', async ({ page, request }, info) => {
    // Свой курсант: оператор 112, начал вызов и не закончил
    const логин = `практикант${Date.now()}`;
    const id = sql(
      `INSERT INTO users (login, password_hash, fio, role)
       SELECT '${логин}', password_hash, 'Практикантов П.П.', 'student'
         FROM users WHERE login = 'student1' RETURNING id`);
    const токен = await войти(request, логин);
    const сценарий = sql(
      `SELECT id FROM scenarios WHERE status = 'approved' AND ticket = 3 AND call_no = 1 LIMIT 1`);
    const начато = await request.post(`${API}/api/sessions`, {
      headers: { Authorization: `Bearer ${токен}` },
      data: { scenarioId: сценарий, mode: 'card112' },
    });
    expect(начато.status()).toBe(201);
    const { session, card } = await начато.json();

    // Преподаватель направил его диспетчером в МЧС
    const админ = await войти(request, 'admin');
    const смена = await request.patch(`${API}/api/users/${id}/duty`, {
      headers: { Authorization: `Bearer ${админ}` },
      data: { duty: 'dds', serviceId: 'mchs' },
    });
    expect(смена.ok()).toBeTruthy();

    // Журнал — уже диспетчерский: ни билетов, ни «Продолжить»
    await login(page, info, логин);
    await expect(page.getByTestId('duty-badge')).toContainText('Диспетчер ДДС');
    await expect(page.getByTestId('unfinished-call')).toHaveCount(0);
    await expect(page.getByRole('heading', { name: 'Учебные вызовы' })).toHaveCount(0);

    // Старую ссылку на карточку не открыть
    await go(page, info, `/card/${card.number}`);
    await expect(page.getByTestId('not-for-duty')).toBeVisible();

    // И сервер тем же токеном заполнить её не даст
    const правка = await request.patch(`${API}/api/sessions/${session.id}/card`, {
      headers: { Authorization: `Bearer ${токен}` },
      data: { description: 'после перевода в ДДС' },
    });
    expect(правка.status()).toBe(403);
    expect((await правка.json()).error).toBe('wrong_duty');
    const сохранение = await request.post(`${API}/api/sessions/${session.id}/save`, {
      headers: { Authorization: `Bearer ${токен}` },
    });
    expect(сохранение.status()).toBe(403);

    sql(`DELETE FROM cards WHERE session_id = '${session.id}'`);
    sql(`DELETE FROM sessions WHERE user_id = '${id}'`);
    sql(`UPDATE audit_log SET user_id = NULL WHERE user_id = '${id}'`);
    sql(`DELETE FROM users WHERE id = '${id}'`);
  });

  test('администратор направляет курсанта на другое рабочее место', async ({ page }, info) => {
    const логин = `направляемый${Date.now()}`;
    const id = sql(
      `INSERT INTO users (login, password_hash, fio, role)
       SELECT '${логин}', password_hash, 'Направляемов Н.Н.', 'student'
         FROM users WHERE login = 'student1' RETURNING id`);

    await login(page, info, 'admin');
    await go(page, info, '/admin');
    await page.getByTestId('admin-tab-users').click();
    const редактор = page.getByTestId(`duty-${логин}`);
    await expect(редактор.getByLabel(`Рабочее место ${логин}`)).toHaveValue('op112');
    await редактор.getByLabel(`Рабочее место ${логин}`).selectOption('dds');
    await редактор.getByLabel(`Служба ${логин}`).selectOption('mchs');
    await page.getByTestId(`duty-apply-${логин}`).click();
    await expect.poll(() => sql(`SELECT duty || '/' || service_id FROM users WHERE id = '${id}'`))
      .toBe('dds/mchs');

    sql(`UPDATE audit_log SET user_id = NULL WHERE user_id = '${id}'`);
    sql(`DELETE FROM users WHERE id = '${id}'`);
  });
});
