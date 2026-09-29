import { test, expect, type Page } from '@playwright/test';
import { login, launchTicket3 } from './helpers';

/**
 * Тип происшествия: выбор из списка, несколько типов в одной карточке и
 * все уровни признаков.
 *
 * Жалоба с проверки 29.09: «признак второго уровня может не появиться
 * для заполнения». У карт со скриншотов (101, 104, «Взрыв») второй и
 * третий уровни классификатора теперь дописываются из него же.
 */

const карта = (page: Page, typeId: string) => page.getByTestId(`survey-${typeId}`);

const выбрать = async (page: Page, typeId: string, group: string, option: string) => {
  await карта(page, typeId).getByRole('group', { name: group, exact: true })
    .getByRole('button', { name: option, exact: true }).click();
};

/** Тип из выпадающего списка — долгим нажатием, как рукой. */
const добавитьТип = async (page: Page, title: string) => {
  await page.locator('#type-search').click();
  // Раньше список закрывался по потере фокуса через 150 мс, и нажатие
  // дольше этого до пункта не доходило
  await page.getByRole('option', { name: title, exact: true }).click({ delay: 400 });
};

test.describe('Тип происшествия', () => {
  test.beforeEach(async ({ page }, info) => {
    await login(page, info);
    await launchTicket3(page);
  });

  test('список открывается по фокусу, долгий клик добавляет тип', async ({ page }) => {
    await добавитьТип(page, '104');
    await expect(карта(page, '104')).toBeVisible();
    // Выбранный тип в списке больше не предлагается — снимают его крестиком
    await page.locator('#type-search').click();
    await expect(page.getByRole('option', { name: '104', exact: true })).toHaveCount(0);
  });

  test('два типа в одной карточке заполняются до конца, службы от обоих', async ({ page }) => {
    await добавитьТип(page, '101');
    await добавитьТип(page, '104');
    await expect(карта(page, '101')).toBeVisible();
    await expect(карта(page, '104')).toBeVisible();

    await выбрать(page, '101', 'Где', 'Дом');
    await выбрать(page, '101', 'Признак пожара (дом)', 'Открытое пламя / Дым');
    await выбрать(page, '101', 'Дом (пламя, дым)', 'Дом частный');

    const газ = 'Запах газа в помещении (в квартире, в доме)';
    await выбрать(page, '104', 'Признаки происшествия', газ);
    // Второй и третий уровни, которых на кадре не было
    await выбрать(page, '104', `Признак 2 (${газ})`, 'Квартира помещение');
    await выбрать(page, '104', `Признак 3 (${газ})`, 'Открыто');

    await expect(page.getByTestId('service-mchs')).toBeVisible();
    await expect(page.getByTestId('service-mosgaz')).toBeVisible();

    // Убрали 104 — ушла его карта, и службы только от пожара
    await page.getByRole('button', { name: '104', exact: true }).click();
    await expect(карта(page, '104')).toHaveCount(0);
    await expect(page.getByTestId('service-mchs')).toBeVisible();
    await expect(page.getByTestId('service-mosgaz')).toHaveCount(0);
  });

  test('«Взрыв»: здание раскрывается объектами классификатора', async ({ page }) => {
    await добавитьТип(page, 'Взрыв');
    await выбрать(page, 'explosion', 'Где взрыв', 'Здание / Объект');
    await выбрать(page, 'explosion', 'Признак 2 (Здание / Объект)', 'Лечебное заведение');
    await expect(page.getByTestId('service-mchs')).toBeVisible();
  });

  test('101: «Здание / объект» — объекты, а не вопросы про жилой дом', async ({ page }) => {
    await добавитьТип(page, '101');
    await выбрать(page, '101', 'Где', 'Здание / объект');
    const объект = карта(page, '101').getByRole('group', { name: 'Признак 2 (Здание / объект)', exact: true });
    await expect(объект).toBeVisible();
    await expect(объект.getByRole('button', { name: 'лечебное', exact: true })).toBeVisible();
    await expect(карта(page, '101').getByRole('group', { name: 'Дом (пламя, дым)', exact: true })).toHaveCount(0);
  });

  test('ДТП: второй признак появляется по первому', async ({ page }) => {
    await добавитьТип(page, 'ДТП');
    await expect(карта(page, 'dtp').getByRole('group', { name: 'Признак 2', exact: true })).toHaveCount(0);
    await выбрать(page, 'dtp', 'Признак 1 (тип происшествия)', 'ДТП пострадавшие');
    await выбрать(page, 'dtp', 'Признак 2', 'Наезд на пешехода');
    await expect(page.getByTestId('service-smp')).toBeVisible();
  });
});
