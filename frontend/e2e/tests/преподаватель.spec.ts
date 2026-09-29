import { expect, test } from '@playwright/test';
import { fillFireCard, launchTicket3, login, sql, USERS } from './helpers';

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

  test('аналитика группы диспетчеров открывается на их вкладке, а не на пустой «Операторы 112»',
    async ({ page }, info) => {
      // UX-проход 1: у группы из одних диспетчеров ДДС аналитика по
      // ученикам открывалась пустой вкладкой 112 — «Обучающихся 0»
      const группа = `ДДС-аналитика ${Date.now()}`;
      const gid = sql(`INSERT INTO training_groups (title, teacher_id)
                       SELECT '${группа}', id FROM users WHERE login = 'teacher1' RETURNING id`).trim();
      sql(`INSERT INTO group_members (group_id, user_id)
           SELECT '${gid}', id FROM users WHERE login = '${USERS.mchs}'`);
      try {
        await login(page, info, 'teacher1');
        await page.getByTestId('analytics-link').click();
        await page.getByLabel('Группа').selectOption({ label: группа });
        await page.getByTestId('analytics-people-tab').click();
        await expect(page.getByRole('button', { name: 'Диспетчеры ДДС', exact: true })).toHaveAttribute('aria-pressed', 'true');
        await expect(page.getByTestId('people-table').locator('[data-testid^="student-"]')).toHaveCount(1);
        // Нажатая вкладка остаётся нажатой, даже пустая
        await page.getByRole('button', { name: 'Операторы 112', exact: true }).click();
        await expect(page.getByRole('button', { name: 'Операторы 112', exact: true })).toHaveAttribute('aria-pressed', 'true');
        await expect(page.getByTestId('people-table').locator('[data-testid^="student-"]')).toHaveCount(0);
      } finally {
        sql(`DELETE FROM training_groups WHERE id = '${gid}'`);
      }
    });
});
