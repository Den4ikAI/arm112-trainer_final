import { expect, test } from '@playwright/test';
import { fillFireCard, launchTicket3, login, USERS } from './helpers';

/**
 * Экраны преподавателя: ход занятия по местам, веса оценки, аналитика
 * по ученику. Образец — ролик demo-chat.mp4 (монитор класса, «Веса
 * оценки», отчёт с лидерами и отстающими).
 */
test.describe('Преподаватель', () => {
  test('ход занятия: операторы 112 и диспетчеры ДДС отдельно', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await page.getByTestId('lessons-link').click();
    const тема = `Монитор ${Date.now()}`;
    await page.getByTestId('lesson-title').fill(тема);
    await page.getByTestId('lesson-create').click();
    await expect(page.getByTestId('lesson-current')).toHaveText(тема);
    await page.getByTestId('lesson-start').click();
    await expect(page.getByTestId('lesson-state')).toHaveText('идёт');

    await page.getByTestId('lesson-monitor-link').click();
    const монитор = page.getByTestId('lesson-monitor');
    await expect(монитор).toBeVisible();
    await expect(page.getByTestId('monitor-status')).toContainText('опаздывают сейчас');
    await expect(page.getByRole('heading', { name: /Операторы 112/ })).toBeVisible();
    await expect(page.getByRole('heading', { name: /Диспетчеры ДДС/ })).toBeVisible();
    // Места есть хотя бы в одном из блоков, у каждого — счётчики
    await expect(монитор.locator('[data-testid^="seat-"]').first()).toBeVisible();

    await page.getByTestId('monitor-finish').click();
    await expect(page.getByTestId('monitor-finish')).toHaveCount(0);
  });

  test('веса оценки: ползунок сразу меняет «стало», без сохранения', async ({ page }, info) => {
    // Нужна хотя бы одна сданная попытка с «угаданными» признаками:
    // без вопросов заявителю полнота карточки не набирается
    await login(page, info, USERS.operator);
    await launchTicket3(page);
    await fillFireCard(page);
    await page.getByTestId('save-card').click();
    await page.getByTestId('score').waitFor();

    await login(page, info, 'teacher1');
    await page.getByTestId('weights-link').click();
    await expect(page.getByRole('heading', { name: 'Веса оценки' })).toBeVisible();
    const изменят = page.getByTestId('weights-changed');
    await expect(изменят).toHaveText(/^0 из \d+$/);

    await page.getByTestId('weight-completeness').fill('0');
    await expect(изменят).not.toHaveText(/^0 из/, { timeout: 10_000 });
    // Не сохраняем: веса общие, от них зависят баллы других проверок
    await expect(page.getByTestId('weights-save')).toBeEnabled();
  });

  test('аналитика по ученику открывается из группы', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await page.getByTestId('analytics-link').click();
    await page.getByTestId('analytics-people-tab').click();
    await expect(page.getByTestId('people-analytics')).toBeVisible();
    await expect(page.getByTestId('people-table')).toBeVisible();
    await page.getByTestId('people-table').locator('[data-testid^="student-"]').first().click();
    await expect(page.getByTestId('student-analytics')).toBeVisible();
    await expect(page.getByRole('heading', { name: 'История попыток' })).toBeVisible();
  });
});
