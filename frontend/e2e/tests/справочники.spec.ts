import { test, expect } from '@playwright/test';
import { login, launchTicket3 } from './helpers';

/**
 * Справочники — ровно как в боевой системе: выпадающий список
 * «что случилось?» (refs/КАРТОЧКА 112, image2…13) и окно «Добавьте
 * службы» (refs/СЛУЖБЫ 112, image1…55). Выписки кадров —
 * backend/docs/справочники/.
 */
test.describe('Справочники по кадрам боевой системы', () => {
  test('список «что случилось?» открывается по фокусу целиком и в порядке кадров',
    async ({ page }, info) => {
      await login(page, info);
      await launchTicket3(page);

      await page.locator('#type-search').focus();
      const пункты = page.getByRole('listbox').getByRole('option');
      await expect(пункты).toHaveCount(51);
      await expect(пункты.nth(0)).toHaveText('101');
      await expect(пункты.nth(4)).toHaveText('Аварии и происшествия в городском хозяйстве');
      await expect(пункты.nth(8)).toHaveText('Благодарность службам');
      await expect(пункты.nth(9)).toHaveText('БПЛА');
      await expect(пункты.last()).toHaveText('Экологические происшествия');

      // Ввод сужает список
      await page.locator('#type-search').fill('справка');
      await expect(пункты).toHaveCount(7);
      await expect(пункты.nth(4)).toHaveText('Справка-ГИБДД');
    });

  test('быстрые плашки — как на кадре пустой карточки', async ({ page }, info) => {
    await login(page, info);
    await launchTicket3(page);
    const плашки = page.getByRole('region', { name: 'Что случилось' })
      .getByRole('button');
    await expect(плашки.first()).toHaveText('ДТП');
    await expect(плашки.nth(2)).toHaveText('104');
    await expect(плашки.nth(9)).toHaveText('Справка-101');
  });

  test('окно «Добавьте службы» — список с кадров, скрытых в нём нет',
    async ({ page }, info) => {
      await login(page, info);
      await launchTicket3(page);

      await page.getByRole('button', { name: 'Добавить службу' }).click();
      const окно = page.getByRole('dialog', { name: 'Добавьте службы' });
      const пункты = окно.getByRole('listitem');
      await expect(пункты).toHaveCount(211);
      await expect(пункты.nth(0)).toContainText('Служба 101 (ГУ МЧС России по г.Москве');
      await expect(пункты.nth(1)).toHaveText('ФСБ');
      await expect(пункты.nth(5)).toHaveText('ОГДЦ');

      const поиск = окно.getByLabel('Поиск службы');
      await поиск.fill('Щукино');
      await expect(пункты).toHaveCount(1);
      await expect(пункты.first()).toHaveText('Поселение Щукино (ДДС района Щукино города Москвы)');

      // Столбца классификатора, которого нет в окне боевой системы, нет и здесь
      await поиск.fill('Территориальные ОИВ');
      await expect(пункты).toHaveCount(0);

      // Привычные слова по-прежнему находят номерные службы
      await поиск.fill('скорая');
      await expect(пункты.first()).toContainText('Служба 103');
    });
});
