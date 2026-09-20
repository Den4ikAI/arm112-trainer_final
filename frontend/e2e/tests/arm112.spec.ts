import { test, expect } from '@playwright/test';
import { go, login, launchTicket3, fillFireCard, ageCard, USERS } from './helpers';

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

  /**
   * Норматив памятки ДДС: 30 секунд на подтверждение. Ждать их
   * в тесте нельзя, поэтому сдвигаем время сохранения карточки
   * в прошлое прямо в базе — тем же приёмом, что и серверные тесты.
   */
  test('непринятая карточка краснеет после норматива', async ({ page }, info) => {
    const number = await createFireCard(page, info);
    await login(page, info, USERS.mchs);

    const row = page.getByTestId(`journal-row-${number}`);
    await expect(row).toBeVisible();
    await expect(row).toHaveAttribute('data-overdue', 'false');

    await ageCard(number, 40);

    // Журнал сам опрашивает сервер раз в пять секунд
    await expect(row).toHaveAttribute('data-overdue', 'true', { timeout: 20_000 });
    await expect(row).toContainText('Не оповещено');
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

test.describe('Живой класс', () => {
  /**
   * То, ради чего канал и нужен: одно происшествие уходит сразу
   * в несколько служб, значит один сценарий разворачивается
   * в несколько учебных задач одновременно.
   *
   * Два окна: в одном оператор 112, в другом диспетчер МЧС. Карточка
   * должна долететь без обновления страницы.
   */
  test('карточка от оператора появляется у диспетчера без перезагрузки',
    async ({ browser }, info) => {
      const operatorCtx = await browser.newContext();
      const dispatcherCtx = await browser.newContext();
      const operator = await operatorCtx.newPage();
      const dispatcher = await dispatcherCtx.newPage();

      try {
        // Диспетчер уже сидит в журнале и ничего не делает
        await login(dispatcher, info, USERS.mchs);

        // Оператор заполняет и сохраняет карточку
        await login(operator, info);
        await launchTicket3(operator);
        await fillFireCard(operator);
        const number = (await operator.getByTestId('card-number').innerText()).trim();

        // Номер выдан, но карточка ещё не сохранена — у диспетчера
        // её быть не должно
        const row = dispatcher.getByTestId(`journal-row-${number}`);
        await expect(row).toHaveCount(0);

        await operator.getByTestId('save-card').click();
        await expect(operator.getByTestId('score')).toBeVisible();

        // Диспетчер страницу не трогал — строка пришла каналом
        await expect(row).toBeVisible({ timeout: 10_000 });
      } finally {
        await operatorCtx.close();
        await dispatcherCtx.close();
      }
    });

  /** Пожар в частном доме не профилен для Мослифта. */
  test('чужая карточка в ленту не приходит', async ({ browser }, info) => {
    const liftCtx = await browser.newContext();
    const operatorCtx = await browser.newContext();
    const lift = await liftCtx.newPage();
    const operator = await operatorCtx.newPage();

    try {
      await login(lift, info, USERS.lift);

      await login(operator, info);
      await launchTicket3(operator);
      await fillFireCard(operator);
      const number = (await operator.getByTestId('card-number').innerText()).trim();
      await operator.getByTestId('save-card').click();
      await expect(operator.getByTestId('score')).toBeVisible();

      // Даём каналу время доставить то, чего доставлять не должен
      await lift.waitForTimeout(3000);
      await expect(lift.getByTestId(`journal-row-${number}`)).toHaveCount(0);
    } finally {
      await liftCtx.close();
      await operatorCtx.close();
    }
  });
});

test.describe('Администрирование', () => {
  test('курсанту раздел не показывается и не открывается',
    async ({ page }, info) => {
      await login(page, info);
      await expect(page.getByTestId('admin-link')).toHaveCount(0);

      await go(page, info, '/admin');
      await expect(page.getByText('доступен преподавателю и администратору')).toBeVisible();
    });

  /**
   * Раздел «Аудит» инструкции требует, чтобы было видно, кто, что и
   * когда сделал. Вход — тоже событие безопасности.
   */
  test('журнал действий показывает вход и сохранение карточки',
    async ({ page }, info) => {
      await login(page, info);
      await launchTicket3(page);
      await fillFireCard(page);
      const number = (await page.getByTestId('card-number').innerText()).trim();
      await page.getByTestId('save-card').click();
      await expect(page.getByTestId('score')).toBeVisible();

      await login(page, info, 'teacher1');
      await page.getByTestId('admin-link').click();

      await expect(page.getByTestId('audit-total')).toBeVisible();
      await page.getByRole('combobox', { name: 'Фильтр по действию' })
        .selectOption('card.save');
      await expect(page.getByTestId('audit-row').first()).toContainText('card.save');

      // Вход тоже виден
      await page.getByRole('combobox', { name: 'Фильтр по действию' })
        .selectOption('auth.login');
      await expect(page.getByTestId('audit-row').first()).toContainText('auth.login');
      expect(number).toMatch(/^\d+$/);
    });

  test('преподаватель видит людей, но завести не может', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await page.getByTestId('admin-link').click();
    await page.getByTestId('admin-tab-users').click();

    await expect(page.getByTestId('user-student1')).toBeVisible();
    await expect(page.getByTestId('add-user')).toHaveCount(0);
  });

  test('администратор заводит и отключает учётную запись', async ({ page }, info) => {
    await login(page, info, 'admin');
    await page.getByTestId('admin-link').click();
    await page.getByTestId('admin-tab-users').click();

    const login_ = `курсант${Date.now()}`;
    await page.getByTestId('add-user').click();
    await page.getByRole('textbox', { name: 'Логин' }).fill(login_);
    await page.getByRole('textbox', { name: 'Пароль' }).fill('длинныйПароль');
    await page.getByRole('textbox', { name: 'ФИО', exact: true }).fill('Новиков Н.Н.');
    await page.getByTestId('save-user').click();

    const row = page.getByTestId(`user-${login_}`);
    await expect(row).toBeVisible();

    // Учётные записи не удаляются, а отключаются: на них ссылается история
    await row.getByRole('button', { name: 'отключить' }).click();
    await expect(row.getByRole('button', { name: 'включить' })).toBeVisible();
  });

  test('активная версия классификатора ровно одна', async ({ page }, info) => {
    await login(page, info, 'admin');
    await page.getByTestId('admin-link').click();
    await page.getByTestId('admin-tab-classifier').click();

    await expect(page.getByTestId('version-row').first()).toBeVisible();
    await expect(page.getByTestId('version-active')).toHaveCount(1);
  });

  test('состояние сервера показывается', async ({ page }, info) => {
    await login(page, info, 'admin');
    await page.getByTestId('admin-link').click();
    await page.getByTestId('admin-tab-health').click();

    const health = page.getByTestId('health');
    await expect(health).toContainText('отвечает');
    await expect(health).toContainText('Открытых каналов класса');
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
