import { test, expect, type Page } from '@playwright/test';
import { login, launchTicket3 } from './helpers';

/**
 * Карта 101 по веткам «Улица» и «Транспорт».
 *
 * На кадрах эксперта (refs/КАРТОЧКА 112, image14–image39) карта растёт
 * по мере ответов: после «Улица» — признак пожара на улице, после него —
 * что горит, после объекта — «Правонарушение». Проверяем именно это
 * и то, что службы подбирает настоящий классификатор.
 */

const группа = (page: Page, name: string) => page.getByRole('group', { name, exact: true });

const выбрать = async (page: Page, group: string, option: string) => {
  await группа(page, group).getByRole('button', { name: option, exact: true }).click();
};

test.describe('Карта 101: улица и транспорт', () => {
  test.beforeEach(async ({ page }, info) => {
    await login(page, info);
    await launchTicket3(page);
    await page.locator('#type-search').fill('101');
    await page.getByRole('option', { name: '101', exact: true }).click();
    await группа(page, 'Где').waitFor();
  });

  test('уличный пожар: вопросы появляются по ответам, службы подбираются', async ({ page }) => {
    // Пока место не выбрано, спрашивать про признак пожара рано
    await expect(группа(page, 'Признак пожара (улица)')).toHaveCount(0);
    await expect(группа(page, 'Признак пожара (дом)')).toHaveCount(0);

    await выбрать(page, 'Где', 'Улица');
    await expect(группа(page, 'Признак пожара (улица)')).toBeVisible();
    await expect(группа(page, 'Признак пожара (дом)')).toHaveCount(0);
    await expect(группа(page, 'Улица (пламя, дым)')).toHaveCount(0);

    await выбрать(page, 'Признак пожара (улица)', 'Открытое пламя / Дым');
    await expect(группа(page, 'Улица (пламя, дым)')).toBeVisible();
    await expect(группа(page, 'Правонарушение')).toHaveCount(0);

    // Кадр 19: пожар мусора — «Служба 101» и ЦОДД
    await выбрать(page, 'Улица (пламя, дым)', 'Мусор');
    await expect(page.getByTestId('service-mchs')).toBeVisible();
    await expect(page.getByTestId('service-codd')).toBeVisible();
    await expect(page.getByTestId('service-mvd')).toHaveCount(0);

    // Кадр 24: правонарушение добавляет «Службу 102»
    await expect(группа(page, 'Правонарушение')).toBeVisible();
    await expect(page.getByRole('textbox', { name: 'Описание правонарушения' })).toHaveCount(0);
    await выбрать(page, 'Правонарушение', 'Есть правонарушение');
    await expect(page.getByRole('textbox', { name: 'Описание правонарушения' })).toBeVisible();
    await expect(page.getByTestId('service-mvd')).toBeVisible();
  });

  test('запах гари на улице — без вопроса «что горит»', async ({ page }) => {
    await выбрать(page, 'Где', 'Улица');
    await выбрать(page, 'Признак пожара (улица)', 'Запах гари');

    await expect(группа(page, 'Улица (пламя, дым)')).toHaveCount(0);
    await expect(page.getByTestId('service-mchs')).toBeVisible();
  });

  test('пожар общественного транспорта', async ({ page }) => {
    await выбрать(page, 'Где', 'Транспорт');
    await expect(группа(page, 'Признак пожара (транспорт)')).toBeVisible();
    await выбрать(page, 'Признак пожара (транспорт)', 'Открытое пламя / Дым');
    await выбрать(page, 'Транспорт (пламя, дым)', 'Общественный транспорт');

    // Кадр 36: «Служба 101», «Служба 102», ЦОДД
    await expect(page.getByTestId('service-mchs')).toBeVisible();
    await expect(page.getByTestId('service-mvd')).toBeVisible();
    await expect(page.getByTestId('service-codd')).toBeVisible();
  });

  test('смена ветки снимает ответы скрывшихся вопросов', async ({ page }) => {
    await выбрать(page, 'Где', 'Улица');
    await выбрать(page, 'Признак пожара (улица)', 'Открытое пламя / Дым');
    await выбрать(page, 'Улица (пламя, дым)', 'Мусор');
    await expect(page.getByTestId('service-mchs')).toBeVisible();

    await выбрать(page, 'Где', 'Транспорт');
    await expect(группа(page, 'Улица (пламя, дым)')).toHaveCount(0);
    // «Мусор» больше не подбирает службы: что горит в транспорте, не сказано
    await expect(page.getByTestId('classify-reason')).toBeVisible();

    await выбрать(page, 'Где', 'Улица');
    await expect(группа(page, 'Признак пожара (улица)')
      .getByRole('button', { name: 'Открытое пламя / Дым', exact: true }))
      .toHaveAttribute('aria-pressed', 'false');
  });
});
