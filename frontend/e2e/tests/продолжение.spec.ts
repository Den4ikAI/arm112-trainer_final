import { test, expect, type Page, type TestInfo } from '@playwright/test';
import { go, login, launchTicket3, PASSWORD } from './helpers';

/**
 * Замечания проверки стенда 23.09.
 *
 * Главное — продолжение вызова: после F5 экран писал «Занятие не
 * запущено», и вся начатая работа терялась. Плюс числа, которые
 * врали: «−1 / 5» в разборе, «6 / 5» служб, «NaN мин NaN с» у
 * администратора, «Записей на странице: 10» при 50 строках.
 *
 * Курсант свой: «незаконченный вызов» — последний у пользователя,
 * и на общей учётке его перехватили бы соседние проверки.
 */

const ЕГО_ПАРОЛЬ = 'продолжение-2026';

const завести = async (page: Page): Promise<string> => {
  const вход = await page.request.post('/api/auth/login',
    { data: { login: 'admin', password: PASSWORD } });
  const { token } = await вход.json();
  const login = `продолжение${Date.now()}`;
  const ответ = await page.request.post('/api/admin/users', {
    headers: { authorization: `Bearer ${token}` },
    data: { login, password: ЕГО_ПАРОЛЬ, fio: 'Проверочный П.П.', role: 'student', serviceId: 'mchs' },
  });
  expect(ответ.status()).toBe(201);
  return login;
};

const войти = async (page: Page, info: TestInfo, login: string) => {
  await go(page, info, '/login');
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.locator('input[name="login"]').fill(login);
  await page.locator('input[name="password"]').fill(ЕГО_ПАРОЛЬ);
  await page.getByTestId('login-submit').click();
  await page.getByRole('heading', { name: 'Учебные вызовы' }).waitFor();
};

test.describe('Продолжение вызова', () => {
  test('после обновления страницы вызов продолжается с того же места', async ({ page }, info) => {
    await войти(page, info, await завести(page));
    await launchTicket3(page);

    const номер = await page.getByTestId('card-number').innerText();
    await page.getByLabel('ФИО заявителя').fill('Проверка восстановления');
    await page.getByTestId('ask-city').click();
    await expect(page.getByTestId('transcript')).toContainText('Королёв');
    // Правки уходят на сервер с задержкой — даём им уйти
    await page.waitForTimeout(1200);

    await page.reload();

    await expect(page.getByTestId('card-number')).toHaveText(номер);
    await expect(page.getByLabel('ФИО заявителя')).toHaveValue('Проверка восстановления');
    await expect(page.getByTestId('transcript')).toContainText('Королёв');
    await expect(page.getByTestId('ask-city')).toBeDisabled();
    // Таймер идёт от начала вызова, а не с нуля
    await expect(page.getByTestId('card-timer')).not.toContainText('00:00');
  });

  test('из журнала незаконченный вызов можно продолжить', async ({ page }, info) => {
    const login = await завести(page);
    await войти(page, info, login);
    await launchTicket3(page);
    const номер = await page.getByTestId('card-number').innerText();

    await page.getByTestId('leave-card').click();
    // Повторный вход — как новая вкладка: помнит только сервер
    await войти(page, info, login);

    const баннер = page.getByTestId('unfinished-call');
    await expect(баннер).toContainText(номер);
    await page.getByTestId('continue-call').click();
    await expect(page.getByTestId('card-number')).toHaveText(номер);
  });

  test('разбор переживает обновление страницы, а вопросы считаются вопросами', async ({ page }, info) => {
    await войти(page, info, await завести(page));
    await launchTicket3(page);

    // Ни одного вопроса, но несколько угаданных полей: раньше тут
    // выходило отрицательное «выяснено»
    await page.getByRole('textbox', { name: 'Населенный пункт:' }).fill('Королёв');
    await page.getByTestId('save-card').click();
    await expect(page.getByTestId('score')).toBeVisible();
    await expect(page.getByText('Задано вопросов заявителю')).toBeVisible();
    await expect(page.locator('body')).not.toContainText('-1 / ');
    await expect(page.locator('body')).toContainText('0 / 5');

    const балл = await page.getByTestId('score').textContent() ?? '';
    await page.reload();
    await expect(page.getByTestId('score')).toHaveText(балл);
    await expect(page.getByText('Требуемые службы направлены')).toBeVisible();
  });
});

test.describe('Числа на экранах', () => {
  test('состояние сервера без NaN и пустых полей', async ({ page }, info) => {
    await login(page, info, 'admin');
    await page.getByTestId('admin-link').click();
    await page.getByTestId('admin-tab-health').click();

    const health = page.getByTestId('health');
    await expect(health).toContainText('отвечает');
    await expect(health).not.toContainText('NaN');
    await expect(health).not.toContainText('нет данных');
  });

  test('подвал журнала говорит правду о страницах', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    const подвал = page.getByTestId('journal-pager');
    await expect(подвал).toContainText('Записей на странице: 10');

    // Подвал и строки приходят из одного ответа, но рисуются не в один
    // кадр: подвал успевал показать «1-1», пока строк было ещё ноль.
    // Сверяем, когда обе части сошлись, а не в первый попавшийся момент
    const строки = page.locator('[data-testid^="journal-row-"]');
    const диапазон = async () => {
      const [, с, по] = (await подвал.innerText()).match(/(\d+)-(\d+) из (\d+)/) ?? [];
      return Number(по) - Number(с) + 1;
    };
    await expect.poll(async () => (await диапазон()) - (await строки.count())).toBe(0);
    const строк = await строки.count();
    expect(строк).toBeLessThanOrEqual(10);
    const [, , , из] = (await подвал.innerText()).match(/(\d+)-(\d+) из (\d+)/) ?? [];
    expect(Number(из)).toBeGreaterThanOrEqual(строк);

    // Дальше первой страницы можно уйти, если она есть
    if (Number(из) > 10) {
      await page.getByRole('button', { name: 'Следующая страница' }).click();
      await expect(подвал).toContainText('Страница: 2');
    }
  });
});
