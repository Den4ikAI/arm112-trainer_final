import { test, expect, type Page } from '@playwright/test';
import { go, login, fillFireCard } from './helpers';

/**
 * Учебный материал преподавателя (замечания команды, п. 6): вызов 112
 * и происшествие ДДС собираются вручную, утверждаются и сразу доступны
 * при составлении занятия.
 */
test.describe('Учебный материал преподавателя', () => {
  const составление = async (page: Page, info: Parameters<typeof go>[1], mode: 'card112' | 'dds') => {
    await go(page, info, '/lessons');
    await page.getByTestId('lesson-mode').selectOption(mode);
    return page.getByTestId('lesson-material');
  };

  test('вызов 112 вручную: черновик с эталоном, утверждение, виден в составлении', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await page.getByTestId('scenarios-link').click();
    await page.getByTestId('create-manual').click();

    const тема = `Пожар в частном доме ${Date.now()}`;
    const b = page.getByTestId('scenario-builder');
    await b.getByTestId('builder-title').fill(тема);
    await b.getByTestId('builder-legend').fill('Алло! Горит крыша частного дома, приезжайте!');
    await fillFireCard(page);
    await b.getByLabel('Улица:').fill('Станционная');
    await b.getByLabel('Дом/Вл:').fill('28');
    await b.getByTestId('builder-q-0').fill('Пострадавшие есть?');
    await b.getByTestId('builder-a-0').fill('Нет, все вышли.');
    await b.getByTestId('builder-save').click();

    const view = page.getByTestId('scenario-view');
    await expect(view.getByTestId('scenario-title')).toHaveText(тема);
    await expect(view.getByTestId('facts')).toContainText('Пострадавшие есть?');
    // Эталон посчитал классификатор
    await expect(view.getByTestId('outcome')).not.toHaveText('—');

    await view.getByTestId('approve').click();
    await expect(view).toContainText('утверждён');

    const материал = await составление(page, info, 'card112');
    await expect(материал).toContainText(тема);
  });

  test('происшествие ДДС вручную: службы от классификатора, утверждение, виден в составлении', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await page.getByTestId('scenarios-link').click();
    await page.getByTestId('tab-dds').click();
    await page.getByTestId('create-manual').click();

    const тема = `Пожар: частный дом ${Date.now()}`;
    const b = page.getByTestId('incident-builder');
    await b.getByTestId('incident-title').fill(тема);
    await b.getByLabel('ФИО заявителя').fill('Сидоров Иван Петрович');
    await b.getByLabel('Улица:').fill('Грина');
    await b.getByLabel('Дом/Вл:').fill('11');
    await b.getByTestId('incident-description').fill('Горит крыша частного дома, пострадавших нет.');
    await fillFireCard(page);
    await b.getByTestId('incident-save').click();

    const view = page.getByTestId('incident-view');
    await expect(view).toContainText(тема);
    await expect(view.getByTestId('incident-view-services')).toContainText('Служба 101');

    await view.getByTestId('incident-approve').click();
    await expect(view).toContainText('утверждено');

    const материал = await составление(page, info, 'dds');
    await expect(материал).toContainText(тема);
  });

  test('вызов по описанию: черновик появляется в списке', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await page.getByTestId('scenarios-link').click();
    await page.getByTestId('create-prompt').click();
    const тема = `Запах газа ${Date.now()}`;
    await page.getByTestId('prompt-title').fill(тема);
    await page.getByTestId('prompt-text').fill('Пожилая женщина сообщает о запахе газа в подъезде жилого дома.');
    await page.getByTestId('prompt-submit').click();

    // Без модели — черновик с описанием; с моделью — черновик от модели.
    // В обоих случаях тема та, что дал преподаватель
    await expect(page.getByTestId('scenario-title')).toHaveText(тема, { timeout: 180_000 });
    await expect(page.getByTestId('scenario-view')).toContainText('черновик');
  });

  /**
   * UX-проход 1, находки 10 и 11: пока модель думает, «утвердить»
   * нажималось, и правка ложилась на уже утверждённый вызов; сбой
   * модели показывался как «internal». Ответ модели задерживаем и
   * отдаём тем же 503, что сервер отдаёт на оборванный JSON.
   */
  test('пока модель правит вызов, утвердить нельзя; сбой модели объяснён', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await page.getByTestId('scenarios-link').click();
    await page.getByTestId('create-prompt').click();
    const тема = `Правка в диалоге ${Date.now()}`;
    await page.getByTestId('prompt-title').fill(тема);
    await page.getByTestId('prompt-text').fill('Мужчина сообщает о дыме из окна соседней квартиры.');
    await page.getByTestId('prompt-submit').click();
    await expect(page.getByTestId('scenario-title')).toHaveText(тема, { timeout: 180_000 });

    let отпустить: () => void = () => {};
    const ответ = new Promise<void>((r) => { отпустить = r; });
    await page.route('**/api/scenarios/*/chat', async (route) => {
      if (route.request().method() !== 'POST') return route.continue();
      await ответ;
      await route.fulfill({
        status: 503,
        contentType: 'application/json',
        body: JSON.stringify({
          error: 'unavailable',
          detail: 'Модель ответила не по форме — повторите замечание. Вызов остался прежним.',
        }),
      });
    });

    const view = page.getByTestId('scenario-view');
    await expect(view.getByTestId('approve')).toBeEnabled();
    await page.getByTestId('chat-input').fill('Сделай заявителя пожилой женщиной');
    await page.getByTestId('chat-send').click();
    await expect(view.getByTestId('approve')).toBeDisabled();

    отпустить();
    await expect(page.getByRole('alert')).toContainText('не по форме');
    await expect(view.getByTestId('approve')).toBeEnabled();
    await expect(view).toContainText('черновик');
  });
});
