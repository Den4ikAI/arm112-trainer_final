import { expect, test } from '@playwright/test';
import { login } from './helpers';

/**
 * Утверждения фактов вызова: по ним описание курсанта сверяется
 * кросс-энкодером. Готовит модель, а преподаватель видит их рядом с
 * ответами заявителя и правит до утверждения вызова.
 */
test.describe('Утверждения для сверки описания', () => {
  test('преподаватель правит утверждение, и оно сохраняется', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await page.getByTestId('scenarios-link').click();

    const block = page.getByTestId('fact-statements');
    await expect(block).toBeVisible();
    await expect(block).toContainText('описание курсанта сверяется');

    const поле = block.locator('input[data-testid^="statement-"]').first();
    const ключ = await поле.getAttribute('data-testid');
    const метка = `Проверка утверждения ${Date.now()}`;
    await поле.fill(метка);
    await page.getByTestId('save-statements').click();
    await expect(block.getByRole('status')).toHaveText('Сохранено');

    // После перезагрузки — то же, что записали: утверждение лежит в базе.
    // Точку в конце ставит сервер: утверждение — повествовательное
    await page.reload();
    await expect(page.getByTestId(ключ!)).toHaveValue(`${метка}.`);
  });
});
