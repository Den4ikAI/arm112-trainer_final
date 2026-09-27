import { test, expect, type Page } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { login, go, launchTicket3, fillFireCard, USERS } from '../tests/helpers';

/**
 * Кадры для презентации: сквозной путь 112 → карточка → ДДС → бригада
 * → разбор, и экраны преподавателя.
 *
 * Снимать на отдельной чистой базе (TEST_DATABASE_URL), чтобы в кадр не
 * попали чужие данные. Анализ текста осмыслен только с настоящей моделью:
 * ML_PROVIDER=http ML_BASE_URL=… в окружении прогона. Кадры — размером
 * экрана, а не всей страницы: слайд шестнадцать на девять.
 */
const ROOT = path.resolve(__dirname, '../../../docs/presentation/screens');

const БАЗА =
  (process.env.TEST_DATABASE_URL ?? 'arm112_test').split('/').pop() ?? 'arm112_test';

const снять = async (page: Page, имя: string, full = false) => {
  await page.waitForTimeout(800);
  await page.screenshot({ path: path.join(ROOT, `${имя}.png`), fullPage: full });
};

/** Этап бригады по карточке наступил `seconds` секунд назад. */
const наступил = (number: string, stage: string, seconds: number): void => {
  execFileSync('docker', [
    'exec', 'arm112-db',
    'psql', '-U', 'arm112', '-d', БАЗА, '-qAtc',
    `UPDATE brigade_reports r SET due_at = now() - interval '${seconds} seconds'
       FROM brigade_assignments a JOIN cards c ON c.id = a.card_id
      WHERE r.assignment_id = a.id AND c.number = '${number}'
        AND r.stage = '${stage}'`,
  ]);
};

const статус = async (page: Page, status: string, comment?: string) => {
  await page.getByTestId('pencil-mchs').click();
  await page.getByRole('combobox', { name: 'Статус' }).selectOption(status);
  if (comment) {
    await page.getByRole('textbox', { name: 'Комментарий' }).fill(comment);
  }
  await page.getByRole('button', { name: 'Сохранить статус' }).click();
  await expect(page.getByTestId('service-tab-mchs')).toContainText(status);
};

test('кадры для презентации', async ({ page }, info) => {
  test.setTimeout(900_000);

  // Оператор 112: вызов, вопросы заявителю, опросная карта
  await login(page, info);
  await снять(page, 'p01-журнал-курсант');
  await launchTicket3(page);
  await page.getByTestId('ask-city').click();
  await page.waitForTimeout(500);
  await page.getByTestId('ask-victims').click();
  await fillFireCard(page);
  await page.getByRole('textbox', { name: 'Улица:' }).fill('Станционная');
  // Описание нарочно с ошибкой: анализ текста должен её найти
  await page.getByRole('textbox', { name: 'Описание со слов заявителя' })
    .fill('Горит баня на участке, есть пострадавший. Дом газифицирован.');
  const number = (await page.getByTestId('card-number').innerText()).trim();
  await снять(page, 'p02-карточка-112');

  await page.getByTestId('save-card').click();
  await expect(page.getByTestId('score')).toBeVisible();
  await снять(page, 'p03-разбор-112');
  // Анализ модели приходит из очереди следом
  await expect(page.getByTestId('analysis-status'))
    .not.toContainText('готовится', { timeout: 240_000 });
  await page.getByTestId('attempt-review').scrollIntoViewIfNeeded();
  await снять(page, 'p04-анализ-текста');

  // Диспетчер ДДС (МЧС): открыл, принял, назначил бригаду, доклад
  await login(page, info, USERS.mchs);
  await снять(page, 'p05-журнал-ддс');
  await page.getByTestId(`journal-row-${number}`).click();
  await статус(page, 'Принята', 'Принято в работу, направляю бригаду');
  await page.getByTestId('brigade-select').selectOption('2');
  await page.getByTestId('brigade-assign').click();
  await expect(page.getByTestId('brigade-panel')).toContainText('№ 2');
  наступил(number, 'Начало реагирования', 60);
  await expect(page.getByTestId('brigade-incoming'))
    .toContainText('выехали на вызов', { timeout: 30_000 });
  await статус(page, 'Начало реагирования');
  await снять(page, 'p06-карточка-ддс');

  // Преподаватель: журнал, попытки по карточке и экспертная оценка
  await login(page, info, 'teacher1');
  await снять(page, 'p07-журнал-преподаватель');
  await page.getByTestId(`journal-row-${number}`).click();
  const attempt = page.getByTestId('attempt-card112');
  await expect(attempt).toBeVisible();
  await attempt.locator('input[name="expert-score"]').fill('60');
  await attempt.locator('textarea[name="expert-comment"]')
    .fill('Описание расходится с разговором: баня вместо крыши дома, пострадавший и газ — заявитель этого не говорил');
  await attempt.getByRole('button', { name: 'Поставить оценку' }).click();
  await expect(attempt.getByTestId('expert-score')).toHaveText('60');
  await attempt.scrollIntoViewIfNeeded();
  await снять(page, 'p08-экспертная-оценка');

  await go(page, info, '/lessons');
  await снять(page, 'p09-занятия');
  await go(page, info, '/scenarios');
  await page.waitForTimeout(1500);
  await снять(page, 'p10-подготовка-вызовов');
  await go(page, info, '/analytics');
  await page.waitForTimeout(3000);
  await снять(page, 'p11-аналитика');

  await login(page, info, 'admin');
  await go(page, info, '/admin');
  await снять(page, 'p12-администрирование');
});
