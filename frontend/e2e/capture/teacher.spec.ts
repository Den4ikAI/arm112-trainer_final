import { mkdir } from 'node:fs/promises';
import path from 'node:path';
import { expect, test, type Page } from '@playwright/test';
import { login, USERS } from '../tests/helpers';

/**
 * Съёмка экранов преподавателя: ход занятия по местам, веса оценки,
 * аналитика по людям. Кадры — в docs/presentation/screens/teacher/.
 */
const DIR = path.resolve(__dirname, '../../../docs/presentation/screens/teacher');

const shot = async (page: Page, name: string) => {
  await mkdir(DIR, { recursive: true });
  await page.screenshot({ path: path.join(DIR, `${name}.png`), fullPage: true });
};

test('экраны преподавателя', async ({ browser }, info) => {
  test.setTimeout(180_000);
  const teacherCtx = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const teacher = await teacherCtx.newPage();
  await login(teacher, info, 'teacher1');
  await teacher.getByTestId('lessons-link').click();
  await teacher.getByTestId('lesson-title').fill('Смена 112 — пожары');
  await teacher.getByTestId('lesson-create').click();
  await teacher.getByTestId('lesson-start').click();
  await expect(teacher.getByTestId('lesson-state')).toHaveText('идёт');

  // Оператор 112 берёт вызов занятия и начинает заполнять карточку
  const opCtx = await browser.newContext({ viewport: { width: 1600, height: 900 } });
  const op = await opCtx.newPage();
  await login(op, info, USERS.operator);
  await op.locator('[data-testid^="launch-"]').first().click();
  await op.getByTestId('card-timer').waitFor();
  await op.locator('#type-search').fill('101');
  await op.getByRole('option', { name: '101', exact: true }).click();

  await teacher.getByTestId('lesson-monitor-link').click();
  await expect(teacher.getByTestId('lesson-monitor')).toBeVisible();
  await teacher.waitForTimeout(3500);
  await shot(teacher, '01-ход-занятия');

  await teacher.goto('/#/weights');
  await expect(teacher.getByRole('heading', { name: 'Веса оценки' })).toBeVisible();
  await teacher.getByTestId('weight-address').fill('5');
  await teacher.getByTestId('weight-completeness').fill('4');
  await teacher.waitForTimeout(1500);
  await shot(teacher, '02-веса-оценки');

  await teacher.goto('/#/analytics');
  await teacher.getByTestId('analytics-people-tab').click();
  await expect(teacher.getByTestId('people-table')).toBeVisible();
  await shot(teacher, '03-аналитика-группы');
  await teacher.getByTestId('people-table').locator('[data-testid^="student-"]').first().click();
  await expect(teacher.getByTestId('student-analytics')).toBeVisible();
  await shot(teacher, '04-аналитика-ученика');

  // Занятие завершаем: идущее ограничивает список вызовов группы, и
  // следующие прогоны на той же базе падали бы по чужой причине
  await teacher.goto('/#/lessons');
  await teacher.getByTestId('lesson-finish').click();
  await expect(teacher.getByTestId('lesson-state')).toHaveText('завершено');
  await opCtx.close();
  await teacherCtx.close();
});
