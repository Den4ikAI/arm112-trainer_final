import { test, expect } from '@playwright/test';
import { login, launchTicket3 } from './helpers';

/**
 * Полный список типов «Что случилось?».
 *
 * Источник — refs/КАРТОЧКА 112.docx от экспертов. Раньше типов было 19,
 * и обрушение, угрозу взрыва или «Ребёнок в опасности» оператор завести
 * не мог: в поиске их просто не было.
 */
test.describe('Типы происшествия', () => {
  test('обрушение находится поиском и открывает опросную карту', async ({ page }, info) => {
    await login(page, info);
    await launchTicket3(page);

    await page.locator('#type-search').fill('Обрушение');
    await page.getByRole('option', { name: 'Обрушение', exact: true }).click();

    const карта = page.getByTestId('survey-collapse');
    await expect(карта).toBeVisible();
    await expect(карта.getByRole('group', { name: 'Признак 1 (тип происшествия)' }))
      .toBeVisible();
  });

  test('тип без группы в классификаторе честно говорит, что карты нет',
    async ({ page }, info) => {
      await login(page, info);
      await launchTicket3(page);

      await page.locator('#type-search').fill('Нецелевой');
      await page.getByRole('option', { name: 'Нецелевой вызов', exact: true }).click();

      // Группу угадывать нельзя: от неё зависят эталон и список служб
      await expect(page.getByTestId('survey-non-target'))
        .toContainText('Для этого типа опросной карты нет');
    });
});
