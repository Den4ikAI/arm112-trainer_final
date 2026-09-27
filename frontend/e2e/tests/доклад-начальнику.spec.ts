import { test, expect, type Page, type TestInfo } from '@playwright/test';
import { login, launchTicket3, fillFireCard, USERS } from './helpers';

/**
 * Доклад диспетчера ДДС вышестоящему начальнику.
 *
 * Заказчик (чат задачи, 27.09.2026): голосовое общение «ДДС с вышестоящим
 * начальником и ДДС с руководителем бригады». Прогон идёт текстом: сервера
 * голоса в прогоне нет, и звонок обязан работать без него. Модели тоже
 * нет — начальник переспрашивает по ключевым словам.
 */

/** Оператор сохраняет пожар, диспетчер МЧС открывает карточку и принимает. */
const карточкаМчс = async (page: Page, info: TestInfo): Promise<string> => {
  await login(page, info);
  await launchTicket3(page);
  await fillFireCard(page);
  await page.getByRole('textbox', { name: 'Улица:' }).fill('Станционная');
  const number = (await page.getByTestId('card-number').innerText()).trim();
  await page.getByTestId('save-card').click();
  await expect(page.getByTestId('score')).toBeVisible();

  await login(page, info, USERS.mchs);
  await page.getByTestId(`journal-row-${number}`).click();
  await page.getByTestId('pencil-mchs').click();
  await page.getByRole('combobox', { name: 'Статус' }).selectOption('Принята');
  await page.getByRole('textbox', { name: 'Комментарий' }).fill('Принято в работу');
  await page.getByRole('button', { name: 'Сохранить статус' }).click();
  await expect(page.getByTestId('service-tab-mchs')).toContainText('Принята');
  return number;
};

const сказать = async (page: Page, text: string): Promise<void> => {
  const lines = page.getByTestId('chief-lines').locator('li');
  const before = await lines.count();
  await page.locator('textarea[name="chief-say"]').fill(text);
  await page.getByTestId('chief-send').click();
  // Своя реплика и ответ начальника
  await expect(lines).toHaveCount(before + 2);
};

test.describe('Доклад начальнику', () => {
  test('диспетчер докладывает текстом, начальник переспрашивает и принимает', async ({ page }, info) => {
    const number = await карточкаМчс(page, info);
    const panel = page.getByTestId('chief-report');
    const lines = page.getByTestId('chief-lines');

    await page.getByTestId('chief-start').click();
    await expect(lines).toContainText('Слушаю вас. Докладывайте.');

    // Не сказал адреса — начальник о нём и спрашивает
    await сказать(page, 'Докладываю: пожар, горит крыша частного дома.');
    await expect(lines.locator('li').last()).toContainText('Где это? Назовите адрес.');

    await сказать(page, 'Улица Станционная.');
    await expect(lines.locator('li').last()).toContainText('Начальник');

    // Закончить можно самому — начальник принимает, разбор тут же
    await page.getByTestId('chief-finish').click();
    await expect(lines.locator('li').last()).toContainText('Принял.');
    const review = panel.getByTestId('chief-review');
    await expect(review).toContainText('Доложено');
    await expect(review.getByTestId('chief-item-what')).toContainText('доложено');
    await expect(review.getByTestId('chief-item-where')).toContainText('«Улица Станционная.»');

    // После «Принял» можно позвонить ещё раз, но не продолжать тот же разговор
    await expect(page.getByTestId('chief-send')).toHaveCount(0);
    await expect(page.getByTestId('chief-start')).toContainText('Позвонить ещё раз');

    // Преподаватель видит доклад в разборе попытки ДДС
    await login(page, info, 'teacher1');
    await page.getByTestId(`journal-row-${number}`).click();
    const attempt = page.getByTestId('attempt-dds');
    await expect(attempt).toContainText('Доклад начальнику');
    await expect(attempt.getByTestId('chief-review')).toContainText('Доложено');
    // Сам звонок преподаватель не ведёт
    await expect(page.getByTestId('chief-report')).toHaveCount(0);
  });
});
