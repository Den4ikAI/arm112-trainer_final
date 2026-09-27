import { test, expect } from '@playwright/test';
import { login, launchTicket3, fillFireCard } from './helpers';

/**
 * Разбор попытки: нейросетевой анализ описания и экспертная оценка.
 *
 * ТЗ: «оценка … на основе анализа ручного ввода текста» и
 * «автоматическая оценка И экспертная оценка». Сервер прогона работает
 * на заглушке моделей, поэтому анализ здесь приходит честным «модель
 * недоступна» — сам разбор текста моделью проверяется в тестах
 * сервера с подставной моделью и вручную на настоящей.
 */
test.describe('Разбор попытки', () => {
  test('курсант видит анализ текста, преподаватель ставит экспертную оценку', async ({ page }, info) => {
    await login(page, info);
    await launchTicket3(page);
    await fillFireCard(page);
    await page.getByRole('textbox', { name: 'Описание со слов заявителя' })
      .fill('Горит крыша частного дома, пострадавших нет');
    const number = (await page.getByTestId('card-number').innerText()).trim();
    await page.getByTestId('save-card').click();
    await expect(page.getByTestId('score')).toBeVisible();

    // Разбор правил — сразу, анализ текста — следом, из очереди
    const review = page.getByTestId('attempt-review');
    await expect(review).toContainText('Анализ текста (нейросеть)');
    await expect(page.getByTestId('analysis-status'))
      .toContainText('модель недоступна', { timeout: 30_000 });
    // Курсант оценку не ставит
    await expect(page.getByTestId('expert-form')).toHaveCount(0);
    const auto = (await page.getByTestId('auto-score').innerText()).trim();

    await login(page, info, 'teacher1');
    await page.getByTestId(`journal-row-${number}`).click();
    const attempt = page.getByTestId('attempt-card112');
    await expect(attempt).toBeVisible();
    await attempt.locator('input[name="expert-score"]').fill('70');
    await attempt.locator('textarea[name="expert-comment"]').fill('Уточнить газификацию');
    await attempt.getByRole('button', { name: 'Поставить оценку' }).click();

    // Экспертная оценка рядом, автоматическая не изменилась
    await expect(attempt.getByTestId('expert-score')).toHaveText('70');
    await expect(attempt.getByTestId('expert-comment')).toHaveText('Уточнить газификацию');
    await expect(attempt.getByTestId('auto-score')).toHaveText(auto);
  });
});
