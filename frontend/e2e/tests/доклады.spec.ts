import { test, expect, type Page, type TestInfo } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { login, launchTicket3, fillFireCard, USERS, выдатьПроисшествие } from './helpers';

/**
 * Бригада диспетчера ДДС: назначение, доклады старшего и разбор.
 *
 * Оба варианта связи из ответа заказчика (чат задачи, 21.09.2026,
 * сообщение 691): старший звонит сам, или диспетчер звонит ему.
 * Этапы наступают по шкале из настроек, десятки секунд. Ждать их
 * в тесте незачем — сдвигаем этап в прошлое прямо в базе прогона,
 * как ageCard делает с нормативом подтверждения.
 */

const БАЗА_ПРОГОНА =
  (process.env.TEST_DATABASE_URL ?? 'arm112_test').split('/').pop() ?? 'arm112_test';

/** Этап бригады по карточке наступил `seconds` секунд назад. */
const наступил = (number: string, stage: string, seconds: number): void => {
  execFileSync('docker', [
    'exec', 'arm112-db',
    'psql', '-U', 'arm112', '-d', БАЗА_ПРОГОНА, '-qAtc',
    `UPDATE brigade_reports r SET due_at = now() - interval '${seconds} seconds'
       FROM brigade_assignments a JOIN cards c ON c.id = a.card_id
      WHERE r.assignment_id = a.id AND c.number = '${number}'
        AND r.stage = '${stage}'`,
  ]);
};

/** Оператор сохраняет пожар, диспетчер МЧС открывает карточку. */
const карточкаМчс = async (page: Page, info: TestInfo): Promise<string> => {
  await login(page, info);
  await launchTicket3(page);
  await fillFireCard(page);
  await page.getByRole('textbox', { name: 'Улица:' }).fill('Станционная');
  const number = (await page.getByTestId('card-number').innerText()).trim();
  await page.getByTestId('save-card').click();
  await expect(page.getByTestId('score')).toBeVisible();

  выдатьПроисшествие(number);

  await login(page, info, USERS.mchs);
  await page.getByTestId(`journal-row-${number}`).click();
  return number;
};

/**
 * Первая запись — статус и текст: так требует заказчик (чат, 27.09),
 * без текста разбор ставит замечание. Поэтому «Принята» — с текстом.
 */
const статус = async (page: Page, status: string): Promise<void> => {
  await page.getByTestId('pencil-mchs').click();
  await page.getByRole('combobox', { name: 'Статус' }).selectOption(status);
  if (status === 'Принята') {
    await page.getByRole('textbox', { name: 'Комментарий' }).fill('Принято в работу');
  }
  await page.getByRole('button', { name: 'Сохранить статус' }).click();
  await expect(page.getByTestId('service-tab-mchs')).toContainText(status);
};

test.describe('Доклады бригады', () => {
  test('бригаду направляют после «Принята», старший докладывает сам', async ({ page }, info) => {
    const number = await карточкаМчс(page, info);
    const panel = page.getByTestId('brigade-panel');

    // До принятия направлять некого
    await expect(panel).toContainText('после статуса «Принята»');
    await expect(page.getByTestId('brigade-assign')).toHaveCount(0);

    await статус(page, 'Принята');
    await page.getByTestId('brigade-select').selectOption('2');
    await page.getByTestId('brigade-assign').click();
    await expect(panel).toContainText('№ 2');

    // Этап наступил давно — старший уже звонит. Фоновая задача
    // опрашивает раз в две секунды, доклад приходит по каналу
    наступил(number, 'Начало реагирования', 60);
    const incoming = page.getByTestId('brigade-incoming');
    await expect(incoming).toContainText('Старший бригады 2: выехали на вызов.');
    await expect(page.getByTestId('brigade-report')).toContainText('старший позвонил сам');

    // Статус по факту доклада и в пределах норматива
    await статус(page, 'Начало реагирования');
    await expect(page.getByTestId('dds-finding'))
      .toContainText('Начало реагирования — по факту');
    await expect(page.getByTestId('dds-score')).toContainText('100 из 100');
  });

  test('звонок старшему и статус, поставленный раньше доклада', async ({ page }, info) => {
    const number = await карточкаМчс(page, info);
    await статус(page, 'Принята');
    await page.getByTestId('brigade-assign').click();

    // Бригада ещё не выехала — старшему сообщить нечего
    await page.getByTestId('brigade-call').click();
    await expect(page.getByTestId('brigade-answer'))
      .toHaveText('Старший бригады 1: пока без изменений.');

    // Диспетчер «угадал»: сведений о выезде у него нет
    await статус(page, 'Начало реагирования');
    await expect(page.getByTestId('dds-finding'))
      .toContainText('Начало реагирования — не по факту');
    await expect(page.getByTestId('dds-score')).not.toContainText('100 из 100');

    // Бригада уже на месте, но старший ещё не звонил: диспетчер узнаёт
    // об этом своим звонком, раньше доклада
    наступил(number, 'Начало реагирования', 10);
    наступил(number, 'Прибытие', 5);
    await page.getByTestId('brigade-call').click();
    await expect(page.getByTestId('brigade-answer'))
      .toHaveText('Старший бригады 1: прибыли на место.');
    await expect(page.getByTestId('brigade-report').filter({ hasText: 'прибыли на место' }))
      .toContainText('по вашему звонку');

    await статус(page, 'Прибытие');
    await expect(page.getByTestId('dds-finding').filter({ hasText: 'Прибытие' }))
      .toContainText('по факту');
  });
});
