import { test, expect } from '@playwright/test';
import { go, login, launchTicket3, fillFireCard, USERS, PASSWORD } from './helpers';

/**
 * Прогон против живого сервера: классификатор, оценка и машина статусов
 * считаются на нём, фронтенд их не дублирует. Поэтому проверки здесь —
 * не «нарисовалось ли», а «дошло ли до сервера и вернулось ли обратно».
 */

test.describe('Вход', () => {
  test('форма входа повторяет боевую', async ({ page }, info) => {
    await go(page, info, '/login');
    await expect(page.getByText('ВХОД В СИСТЕМУ')).toBeVisible();
    await expect(page.locator('input[name="workstation"]')).toBeVisible();
    await expect(page.getByText('hd-112@mos.ru')).toBeVisible();
  });

  test('неверный пароль не пускает внутрь', async ({ page }, info) => {
    await go(page, info, '/login');
    await page.locator('input[name="login"]').fill(USERS.operator);
    await page.locator('input[name="password"]').fill('не тот');
    await page.getByTestId('login-submit').click();

    await expect(page.getByTestId('login-error')).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Учебные вызовы' })).toHaveCount(0);
  });

  test('без входа внутрь не пускает', async ({ page }, info) => {
    await go(page, info, '/journal');
    await expect(page.getByText('ВХОД В СИСТЕМУ')).toBeVisible();
  });

  test('перезагрузка страницы не выкидывает на вход', async ({ page }, info) => {
    await login(page, info);
    await page.reload();
    await expect(page.getByRole('heading', { name: 'Учебные вызовы' })).toBeVisible();
  });
});

test.describe('Карточка 112', () => {
  test.beforeEach(async ({ page }, info) => {
    await login(page, info);
    await launchTicket3(page);
  });

  test('опросная карта приходит с сервера и признаки переключаются', async ({ page }) => {
    await page.locator('#type-search').fill('101');
    await page.getByRole('option', { name: '101', exact: true }).click();

    await expect(page.getByRole('group', { name: 'Признак пожара (дом)' })).toBeVisible();

    const flame = page.getByRole('button', { name: 'Открытое пламя / Дым' });
    await expect(flame).toHaveAttribute('aria-pressed', 'false');
    await flame.click();
    await expect(flame).toHaveAttribute('aria-pressed', 'true');
  });

  test('настоящий классификатор подбирает службы по признакам', async ({ page }) => {
    await fillFireCard(page);

    // Пожар в частном доме: службы из ЕКП v046_24, не выдуманные
    await expect(page.getByTestId('service-mchs')).toBeVisible();
    await expect(page.getByTestId('service-codd')).toBeVisible();
    await expect(page.getByTestId('service-gup-msr')).toBeVisible();
  });

  test('неполные признаки — сервер говорит, чего не хватает', async ({ page }) => {
    await page.locator('#type-search').fill('101');
    await page.getByRole('option', { name: '101', exact: true }).click();
    await page.getByRole('group', { name: 'Где' })
      .getByRole('button', { name: 'Улица' }).click();

    // В карте 101 ветки «Улица» нет, признак второго уровня задать нечем.
    // Движок не выдумывает позицию, а объясняет (OPEN-QUESTIONS, вопрос 4).
    await expect(page.getByTestId('classify-reason'))
      .toContainText('признак второго уровня');
  });

  test('автоподобранную службу снять нельзя', async ({ page }) => {
    await fillFireCard(page);
    const tag = page.getByTestId('service-mchs');
    await expect(tag).toBeVisible();
    // Инструкция, раздел 6: список формируется автоматически и править
    // его не следует. Крестика у автоподобранной службы нет.
    await expect(tag.getByRole('button')).toHaveCount(0);
  });

  test('заявитель отвечает только на заданный вопрос', async ({ page }) => {
    const transcript = page.getByTestId('transcript');
    await expect(transcript).toContainText('Горит крыша частного дома');
    await expect(transcript).not.toContainText('Королёв');

    await page.getByTestId('ask-city').click();
    await expect(transcript).toContainText('Королёв');
    await expect(page.getByTestId('ask-city')).toBeDisabled();
  });

  test('ответов на не заданные вопросы нет в разметке страницы', async ({ page }) => {
    // Эталон и ответы на сервере; если бы они приезжали целиком,
    // метрику «выяснил или угадал» можно было бы обойти через DevTools
    const html = await page.content();
    expect(html).not.toContain('Королёв');
    expect(html).not.toContain('не газифицирован');
  });
});

test.describe('Разбор: выяснил против угадал', () => {
  test('поле, заполненное без вопроса, помечается «угадал»', async ({ page }, info) => {
    await login(page, info);
    await launchTicket3(page);

    // Курсант вписывает верный город, НЕ спросив о нём заявителя
    await page.getByRole('textbox', { name: 'Населенный пункт:' }).fill('Королёв');
    await page.getByTestId('save-card').click();

    await expect(page.getByTestId('score')).toBeVisible();
    const guessed = page.getByTestId('finding-guessed');
    await expect(guessed.first()).toBeVisible();
    await expect(guessed.filter({ hasText: 'Королёв' }).first())
      .toContainText('заявитель его не сообщал');
  });

  test('выясненное у заявителя догадкой не считается', async ({ page }, info) => {
    await login(page, info);
    await launchTicket3(page);

    await page.getByTestId('ask-city').click();
    await page.getByRole('textbox', { name: 'Населенный пункт:' }).fill('Королёв');
    await page.getByTestId('save-card').click();

    await expect(page.getByTestId('score')).toBeVisible();
    await expect(page.getByTestId('finding-guessed').filter({ hasText: 'Королёв' }))
      .toHaveCount(0);
  });

  test('ошибка в признаке видна последствием, а не баллом', async ({ page }, info) => {
    await login(page, info);
    await launchTicket3(page);

    await fillFireCard(page);
    // Газификацию указываем неверно: в билете дом не газифицирован
    await page.getByRole('group', { name: 'Проведена ли газификация' })
      .getByRole('button', { name: 'Да', exact: true }).click();
    await page.getByTestId('save-card').click();

    await expect(page.getByTestId('score')).toBeVisible();
    // Из-за неверного признака в списке оповещения появился газ —
    // в жизни он бы выехал зря
    await expect(page.getByText('непрофильный вызов')).toBeVisible();
  });
});

test.describe('Режим ДДС', () => {
  /** Оператор заполняет и сохраняет карточку пожара. */
  const createFireCard = async (page: import('@playwright/test').Page,
                                info: import('@playwright/test').TestInfo) => {
    await login(page, info);
    await launchTicket3(page);
    await fillFireCard(page);
    await page.getByRole('textbox', { name: 'Улица:' }).fill('Станционная');
    const number = await page.getByTestId('card-number').innerText();
    await page.getByTestId('save-card').click();
    await expect(page.getByTestId('score')).toBeVisible();
    return number.trim();
  };

  test('пожар попадает в ленту МЧС и не попадает в ленту Мослифта',
    async ({ page }, info) => {
      const number = await createFireCard(page, info);

      await login(page, info, USERS.mchs);
      await expect(page.getByTestId(`journal-row-${number}`)).toBeVisible();

      await login(page, info, USERS.lift);
      await expect(page.getByTestId(`journal-row-${number}`)).toHaveCount(0);
    });

  test('статусы проставляются последовательно и проверяются сервером',
    async ({ page }, info) => {
      const number = await createFireCard(page, info);
      await login(page, info, USERS.mchs);
      await page.getByTestId(`journal-row-${number}`).click();
      await expect(page.getByTestId('card-class')).toContainText('пожар: частный дом');

      // Первичными доступны только два статуса
      await page.getByTestId('pencil-mchs').click();
      let options = await page.getByRole('combobox', { name: 'Статус' })
        .locator('option').allInnerTexts();
      expect(options).toContain('Принята');
      expect(options).toContain('Не принята');
      expect(options).not.toContain('Прибытие');

      // «Не принята» без комментария сервер отклоняет
      await page.getByRole('combobox', { name: 'Статус' }).selectOption('Не принята');
      await page.getByRole('button', { name: 'Сохранить статус' }).click();
      await expect(page.getByRole('alert')).toContainText('комментарий обязателен');

      // С комментарием проходит
      await page.getByRole('textbox', { name: 'Комментарий' }).fill('нет свободных расчётов');
      await page.getByRole('button', { name: 'Сохранить статус' }).click();
      await expect(page.getByTestId('service-tab-mchs')).toContainText('Не принята');

      // После «Не принята» остаётся только принять
      await page.getByTestId('pencil-mchs').click();
      options = await page.getByRole('combobox', { name: 'Статус' })
        .locator('option').allInnerTexts();
      expect(options.filter((o) => o !== 'Статус')).toEqual(['Принята']);
    });

  test('чужая служба статус не ставит', async ({ page }, info) => {
    const number = await createFireCard(page, info);
    await login(page, info, USERS.mchs);
    await page.getByTestId(`journal-row-${number}`).click();

    // Карандаш есть только у своей службы
    await expect(page.getByTestId('pencil-mchs')).toBeVisible();
    await expect(page.getByTestId('pencil-codd')).toHaveCount(0);
  });

  test('история статусов раскрывается', async ({ page }, info) => {
    const number = await createFireCard(page, info);
    await login(page, info, USERS.mchs);
    await page.getByTestId(`journal-row-${number}`).click();

    await page.getByTestId('pencil-mchs').click();
    await page.getByRole('combobox', { name: 'Статус' }).selectOption('Принята');
    await page.getByRole('textbox', { name: 'Комментарий' }).fill('Направлен расчёт');
    await page.getByRole('button', { name: 'Сохранить статус' }).click();

    await page.getByTestId('service-tab-mchs')
      .getByRole('button', { name: 'История статусов' }).click();

    const history = page.getByTestId('history-mchs');
    await expect(history).toContainText('Принята');
    await expect(history).toContainText('Направлен расчёт');
    // В карточке стоит тот, кто её ведёт
    await expect(history).toContainText('Сидоров С.С.');
  });
});

test.describe('Адаптивность', () => {
  test('на узком экране нет горизонтальной прокрутки', async ({ page }, info) => {
    await page.setViewportSize({ width: 400, height: 900 });
    await login(page, info);
    await launchTicket3(page);

    const overflow = await page.evaluate(() =>
      document.documentElement.scrollWidth - document.documentElement.clientWidth);
    expect(overflow).toBeLessThanOrEqual(1);
  });
});
