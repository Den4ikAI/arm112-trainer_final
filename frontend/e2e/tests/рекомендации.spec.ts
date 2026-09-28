import { test, expect, type Page, type TestInfo } from '@playwright/test';
import { go, login, launchTicket3, fillFireCard, PASSWORD } from './helpers';

/**
 * Рекомендации курсанту и проверка того, сработали ли они.
 *
 * Критерий финальной экспертизы — «проверка рекомендаций сервиса
 * и реальных рекомендаций». Проверяем путь целиком: ошибка повторилась —
 * появилась рекомендация, следующий вызов её проверил, и курсант
 * видит вердикт.
 *
 * Курсант свой, заведённый администратором прямо в проверке: история
 * у рекомендаций личная, и на общей учётке итог зависел бы от прошлых
 * прогонов.
 */

const ЕГО_ПАРОЛЬ = 'рекомендации-2026';

const завести = async (page: Page): Promise<string> => {
  const вход = await page.request.post('/api/auth/login',
    { data: { login: 'admin', password: PASSWORD } });
  const { token } = await вход.json();
  const login = `рекомендации${Date.now()}`;
  const ответ = await page.request.post('/api/admin/users', {
    headers: { authorization: `Bearer ${token}` },
    // Оператор 112: рекомендации — по приёму вызовов
    data: { login, password: ЕГО_ПАРОЛЬ, fio: 'Проверочный Р.Р.', role: 'student', duty: 'op112' },
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

/**
 * Отработать билет 3. `как_надо` — сначала задать все вопросы
 * заявителю и заполнить адрес: тогда ни пустых, ни угаданных полей.
 */
const отработать = async (page: Page, info: TestInfo, login: string, как_надо: boolean) => {
  await войти(page, info, login);
  await launchTicket3(page);
  if (как_надо) {
    // Список берём заранее и ждём, пока кнопка погаснет: иначе цикл
    // успевает схватить её снова до перерисовки и ждёт неактивную
    const вопросы = await page.locator('[data-testid^="ask-"]').evaluateAll(
      (кнопки) => кнопки.map((к) => к.getAttribute('data-testid') ?? ''));
    for (const id of вопросы) {
      await page.getByTestId(id).click();
      await expect(page.getByTestId(id)).toBeDisabled();
    }
    await page.getByRole('textbox', { name: 'Субъект:' }).fill('Московская область');
    await page.getByRole('textbox', { name: 'Населенный пункт:' }).fill('Королёв');
    await page.getByRole('textbox', { name: 'Улица:' }).fill('Станционная');
    await page.getByRole('textbox', { name: 'Дом/Вл:' }).fill('28');
  }
  await fillFireCard(page);
  await page.getByTestId('save-card').click();
  await expect(page.getByTestId('score')).toBeVisible();
};

test.describe('Рекомендации', () => {
  test('повторившаяся ошибка даёт рекомендацию, а следующий вызов её проверяет',
    async ({ page }, info) => {
      test.setTimeout(120_000);
      const курсант = await завести(page);

      // Первый раз адрес не заполнен — это ещё случайность
      await отработать(page, info, курсант, false);
      await expect(page.getByTestId('my-advice')).toHaveCount(0);

      // Второй раз то же — привычка, пора рекомендовать. Пустое поле
      // тяжелее угаданного, поэтому первыми идут поля адреса
      await отработать(page, info, курсант, false);
      await expect(page.getByTestId('advice-open')).toHaveCount(3);
      await expect(page.getByTestId('my-advice')).toContainText('Не оставляйте пустым поле «Дом/Вл.»');

      // Третий раз всё выяснил у заявителя и заполнил адрес —
      // рекомендации выполнены
      await отработать(page, info, курсант, true);
      const вердикт = page.getByTestId('advice-verdict');
      await expect(вердикт.first()).toBeVisible();
      await expect(вердикт.first()).toContainText('выполнена');
      await expect(вердикт.filter({ hasText: 'не выполнена' })).toHaveCount(0);
    });

  test('преподаватель видит, сработали ли рекомендации', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await page.getByTestId('analytics-link').click();

    const блок = page.getByTestId('advice-block');
    await expect(блок).toBeVisible();
    if (await page.getByTestId('advice-checked').count()) {
      // Доля без базовой линии ничего не значит — показываем обе
      await expect(блок).toContainText('рекомендаций сработали');
      await expect(блок).toContainText('без рекомендации');
    } else {
      await expect(page.getByTestId('advice-empty')).toBeVisible();
    }
  });
});
