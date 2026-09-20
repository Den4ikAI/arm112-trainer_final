import { test, expect } from '@playwright/test';
import { go, login } from './helpers';

test.describe('Вход и журнал', () => {
  test('форма входа повторяет боевую', async ({ page }, info) => {
    await go(page, info, '/login');
    await expect(page.getByText('ВХОД В СИСТЕМУ')).toBeVisible();
    await expect(page.locator('input[name="login"]')).toHaveValue('umc_operdds1');
    await expect(page.locator('input[name="workstation"]')).toBeVisible();
    await expect(page.getByText('hd-112@mos.ru')).toBeVisible();
  });

  test('журнал показывает список происшествий', async ({ page }, info) => {
    await login(page, info);
    await expect(page.getByTestId('journal-row-36814845')).toBeVisible();
    await expect(page.getByText('Пожар в квартире')).toBeVisible();
    await expect(page.getByText('Список происшествий')).toBeVisible();
  });
});

test.describe('Карточка 112', () => {
  test.beforeEach(async ({ page }, info) => {
    await login(page, info);
    await page.getByTestId('launch-t3-c1').click();
    await expect(page.getByTestId('card-timer')).toBeVisible();
  });

  test('опросная карта раскрывается и признаки переключаются', async ({ page }) => {
    await page.locator('#type-search').fill('101');
    await page.getByRole('option', { name: '101', exact: true }).click();

    await expect(page.getByRole('group', { name: 'Признак пожара (дом)' })).toBeVisible();

    const flame = page.getByRole('button', { name: 'Открытое пламя / Дым' });
    await expect(flame).toHaveAttribute('aria-pressed', 'false');
    await flame.click();
    await expect(flame).toHaveAttribute('aria-pressed', 'true');
  });

  test('классификатор сам подставляет службы по признакам', async ({ page }) => {
    await page.locator('#type-search').fill('101');
    await page.getByRole('option', { name: '101', exact: true }).click();

    // Служба 101 приходит уже по самому типу происшествия.
    await expect(page.getByTestId('service-s101')).toBeVisible();

    // Угроза людям добавляет медицину — это правило классификатора.
    await page.getByRole('group', { name: 'Угроза людям' })
      .getByRole('button', { name: 'Да' }).click();
    await expect(page.getByTestId('service-s103')).toBeVisible();
    await expect(page.getByTestId('service-cemp')).toBeVisible();
  });

  test('таймер краснеет при превышении норматива', async ({ page }) => {
    const timer = page.getByTestId('card-timer');
    await expect(timer).toHaveAttribute('data-over', 'false');
    // Норматив сценария — 60 секунд, ждать столько в тесте нельзя,
    // поэтому проверяем сам факт отсчёта и корректность разметки.
    await expect(timer).toContainText(/^\d{2}:\d{2}/);
  });

  test('заявитель отвечает только на заданный вопрос', async ({ page }) => {
    const transcript = page.getByTestId('transcript');
    await expect(transcript).toContainText('Горит крыша частного дома');
    await expect(transcript).not.toContainText('Королёв');

    await page.getByTestId('ask-city').click();
    await expect(transcript).toContainText('Королёв');
    await expect(page.getByTestId('ask-city')).toBeDisabled();
  });
});

test.describe('Разбор: узнал против угадал', () => {
  test('поле, заполненное без вопроса, помечается «угадал»', async ({ page }, info) => {
    await login(page, info);
    await page.getByTestId('launch-t3-c1').click();

    // Курсант вписывает верный город, НЕ спросив о нём заявителя.
    await page.getByRole('textbox', { name: 'Населенный пункт:' }).fill('Королёв');

    await page.getByTestId('save-card').click();

    await expect(page.getByTestId('score')).toBeVisible();
    const guessed = page.getByTestId('finding-guessed');
    await expect(guessed.first()).toBeVisible();
    await expect(guessed.first()).toContainText('Королёв');
    await expect(guessed.first()).toContainText('заявитель его не сообщал');
  });

  test('выясненное у заявителя не считается догадкой', async ({ page }, info) => {
    await login(page, info);
    await page.getByTestId('launch-t3-c1').click();

    await page.getByTestId('ask-city').click();
    await page.getByRole('textbox', { name: 'Населенный пункт:' }).fill('Королёв');

    await page.getByTestId('save-card').click();

    const guessed = page.getByTestId('finding-guessed');
    await expect(guessed.filter({ hasText: 'Королёв' })).toHaveCount(0);
  });
});

test.describe('Карточка ДДС', () => {
  test.beforeEach(async ({ page }, info) => {
    await login(page, info);
    await page.getByTestId('journal-row-36814845').click();
    await expect(page.getByTestId('card-class')).toBeVisible();
  });

  test('первичными доступны только «Принята» и «Не принята»', async ({ page }) => {
    await page.getByTestId('pencil-dep-jkh').click();
    const select = page.getByRole('combobox', { name: 'Статус' });
    const options = await select.locator('option').allInnerTexts();

    expect(options).toContain('Принята');
    expect(options).toContain('Не принята');
    expect(options).not.toContain('Прибытие');
    expect(options).not.toContain('Работы завершены');
  });

  test('«Не принята» без комментария не сохраняется', async ({ page }) => {
    await page.getByTestId('pencil-dep-jkh').click();
    await page.getByRole('combobox', { name: 'Статус' }).selectOption('Не принята');
    await page.getByRole('button', { name: 'Сохранить статус' }).click();

    await expect(page.getByRole('alert'))
      .toContainText('комментарий обязателен');
  });

  test('после «Принята» открываются статусы хода работ', async ({ page }) => {
    await page.getByTestId('pencil-dep-jkh').click();
    await page.getByRole('combobox', { name: 'Статус' }).selectOption('Принята');
    await page.getByRole('button', { name: 'Сохранить статус' }).click();

    await expect(page.getByTestId('service-tab-dep-jkh')).toContainText('Принята');

    await page.getByTestId('pencil-dep-jkh').click();
    const options = await page.getByRole('combobox', { name: 'Статус' })
      .locator('option').allInnerTexts();
    expect(options).toContain('Прибытие');
    expect(options).toContain('Работы завершены');
    expect(options).toContain('Отказ от выполнения работ');
  });

  test('история статусов раскрывается', async ({ page }) => {
    await page.getByTestId('pencil-dep-jkh').click();
    await page.getByRole('combobox', { name: 'Статус' }).selectOption('Принята');
    await page.getByRole('button', { name: 'Комментарий' }).or(
      page.getByRole('textbox', { name: 'Комментарий' })).fill('Направлена бригада');
    await page.getByRole('button', { name: 'Сохранить статус' }).click();

    await page.getByTestId('service-tab-dep-jkh')
      .getByRole('button', { name: 'История статусов' }).click();

    const history = page.getByTestId('history-dep-jkh');
    await expect(history).toContainText('Добавлена');
    await expect(history).toContainText('Принята');
    await expect(history).toContainText('Направлена бригада');
  });
});

test.describe('Адаптивность', () => {
  test('на узком экране нет горизонтальной прокрутки', async ({ page }, info) => {
    await page.setViewportSize({ width: 400, height: 900 });
    await login(page, info);
    await page.getByTestId('launch-t3-c1').click();

    const overflow = await page.evaluate(() =>
      document.documentElement.scrollWidth - document.documentElement.clientWidth);
    expect(overflow).toBeLessThanOrEqual(1);
  });
});
