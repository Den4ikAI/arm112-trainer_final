import { test, type Page, type TestInfo } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';

/**
 * Съёмка экранов. Кадры складываются в screenshots/react/ и сопоставимы
 * с screenshots/reference/ — снимками боевой системы.
 */

const ROOT = path.resolve(__dirname, '../../screenshots');

const dirFor = (info: TestInfo) => path.join(ROOT, info.project.name);

const shot = async (page: Page, info: TestInfo, name: string) => {
  const dir = dirFor(info);
  await mkdir(dir, { recursive: true });
  // Таймер тикает и делает снимки нестабильными — фиксируем его перед съёмкой.
  await page.waitForTimeout(150);
  await page.screenshot({ path: path.join(dir, `${name}.png`), fullPage: false });
};

/** Приложение на HashRouter: «#» — часть пути, см. tests/helpers.ts. */
const go = async (page: Page, _info: TestInfo, p: string) => {
  await page.goto(`/#${p}`);
};

/** Учебные пользователи из миграции 20260921120000, пароль у всех один. */
const OPERATOR = 'student1';
const MCHS = 'student3';

const login = async (page: Page, info: TestInfo, user = OPERATOR) => {
  await go(page, info, '/login');
  // Гасим сохранённый токен: в сценарии съёмки мы меняем пользователя,
  // а вошедшего приложение уводит с формы входа обратно в журнал
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.locator('input[name="login"]').fill(user);
  await page.locator('input[name="password"]').fill('arm112');
  await page.getByTestId('login-submit').click();
  // Журнал есть у всех; «Учебные вызовы» — только у оператора 112
  await page.getByLabel('Поиск происшествий').waitFor();
};

const pickFireType = async (page: Page) => {
  await page.locator('#type-search').fill('101');
  await page.getByRole('option', { name: '101', exact: true }).click();
};

/** Признаки пожара в частном доме — «пожар: частный дом» по ЕКП. */
const pickFireSigns = async (page: Page) => {
  const pick = async (group: string, option: string) =>
    page.getByRole('group', { name: group })
      .getByRole('button', { name: option, exact: true }).click();
  await pick('Где', 'Дом');
  await pick('Признак пожара (дом)', 'Открытое пламя / Дым');
  await pick('Дом (пламя, дым)', 'Дом частный');
  await pick('Внутридомовые объекты (пламя, дым)', 'Крыша');
  await pick('Угроза людям', 'Нет');
  await pick('Проведена ли газификация', 'Нет');
};

/** Оператор заполняет и сохраняет карточку; возвращает её номер. */
const createFireCard = async (page: Page, info: TestInfo): Promise<string> => {
  await login(page, info);
  await page.getByTestId('launch-t3-c1').click();
  await page.getByTestId('card-timer').waitFor();
  await pickFireType(page);
  await pickFireSigns(page);
  await fillCardProperly(page);
  const number = (await page.getByTestId('card-number').innerText()).trim();
  await page.getByTestId('save-card').click();
  await page.getByTestId('score').waitFor();
  return number;
};

/** Заполняет карточку так, как это сделал бы грамотный курсант. */
const fillCardProperly = async (page: Page) => {
  await page.getByRole('textbox', { name: 'ФИО заявителя' }).fill('Иванова Инна Степановна');
  await page.getByRole('combobox', { name: 'Статус заявителя' }).selectOption('очевидец');
  await page.getByRole('textbox', { name: 'Единая адресная строка' })
    .fill('Московская область, Королёв, ул. Станционная, 28');
  await page.getByRole('textbox', { name: 'Субъект:' }).fill('Московская область');
  await page.getByRole('textbox', { name: 'Населенный пункт:' }).fill('Королёв');
  await page.getByRole('textbox', { name: 'Улица:' }).fill('Станционная');
  await page.getByRole('textbox', { name: 'Дом/Вл:' }).fill('28');
  await page.getByRole('textbox', { name: 'Описание со слов заявителя' })
    .fill('Горит крыша частного дома, открытое пламя, пострадавших нет, дом не газифицирован.');
};

test.describe('Съёмка экранов', () => {
  test('вход и журнал', async ({ page }, info) => {
    await go(page, info, '/login');
    await page.evaluate(() => localStorage.clear());
    await page.reload();
    await page.locator('input[name="login"]').fill(OPERATOR);
    await page.locator('input[name="password"]').fill('arm112');
    await shot(page, info, '01-login');

    await page.getByTestId('login-submit').click();
    await page.getByRole('heading', { name: 'Учебные вызовы' }).waitFor();
    await shot(page, info, '02-journal');
  });

  test('карточка 112', async ({ page }, info) => {
    await login(page, info);
    await page.getByTestId('launch-t3-c1').click();
    await page.getByTestId('card-timer').waitFor();
    await shot(page, info, '03-card112-empty');

    await page.locator('#type-search').fill('10');
    await shot(page, info, '04-card112-type-dropdown');

    await page.locator('#type-search').fill('101');
    await page.getByRole('option', { name: '101', exact: true }).click();
    await shot(page, info, '05-card112-survey-101');

    // Курсант ведёт разговор: каждый вопрос раскрывает скрытый факт.
    for (const id of ['flame', 'house', 'city', 'victims', 'gas']) {
      await page.getByTestId(`ask-${id}`).click();
    }
    await shot(page, info, '06-card112-dialogue');

    await pickFireSigns(page);
    await fillCardProperly(page);
    await shot(page, info, '07-card112-filled');

    await page.getByRole('button', { name: 'Добавить службу' }).click();
    await page.getByRole('dialog', { name: 'Добавьте службы' }).waitFor();
    await shot(page, info, '08-card112-services-modal');
    await page.getByRole('button', { name: 'Сохранить и закрыть' }).click();
  });

  test('превышение норматива', async ({ page }, info) => {
    test.setTimeout(120_000);
    await login(page, info);
    await page.getByTestId('launch-t3-c1').click();
    await page.getByTestId('card-timer').waitFor();
    await pickFireType(page);
    // Норматив сценария — 60 секунд. Ждём, чтобы снять красный таймер:
    // это фирменный признак боевой системы.
    await page.waitForTimeout(63_000);
    await shot(page, info, '09-card112-over-norm');
  });

  test('разбор: узнал против угадал', async ({ page }, info) => {
    await login(page, info);
    await page.getByTestId('launch-t3-c1').click();
    await page.getByTestId('card-timer').waitFor();

    await pickFireType(page);
    // Спрашиваем только часть — остальное курсант «знает сам».
    await page.getByTestId('ask-flame').click();
    await page.getByTestId('ask-house').click();

    await pickFireSigns(page);
    await fillCardProperly(page);

    // Playwright заполняет карточку за доли секунды, и на кадре разбора
    // было бы 00:00. Пауза даёт правдоподобное время против норматива.
    await page.waitForTimeout(9_000);

    await page.getByTestId('save-card').click();
    await page.getByTestId('score').waitFor();
    await shot(page, info, '10-debrief');

    await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
    await shot(page, info, '11-debrief-bottom');
  });

  // Карточку снимает уже диспетчер МЧС: пожар профилен для него,
  // в ленту Деп. ЖКХ он не попадает
  test('карточка ДДС', async ({ page }, info) => {
    const number = await createFireCard(page, info);

    await login(page, info, MCHS);
    await page.getByTestId(`journal-row-${number}`).click();
    await page.getByTestId('card-class').waitFor();
    await shot(page, info, '12-card-dds');

    await page.getByTestId('pencil-mchs').click();
    await shot(page, info, '13-card-dds-status-editor');

    await page.getByRole('combobox', { name: 'Статус' }).selectOption('Принята');
    await page.getByRole('textbox', { name: 'Комментарий' })
      .fill('Направлен расчёт');
    await page.getByRole('button', { name: 'Сохранить статус' }).click();

    // Ждём, пока статус вернётся с сервера: иначе следующий клик
    // по карандашу придётся на ещё открытый редактор и закроет его
    await page.getByTestId('service-tab-mchs').getByText('Принята').waitFor();

    await page.getByTestId('pencil-mchs').click();
    await shot(page, info, '14-card-dds-next-statuses');
    await page.getByRole('combobox', { name: 'Статус' }).selectOption('Начало реагирования');
    await page.getByRole('textbox', { name: 'Комментарий' })
      .fill('Расчёт выехал на место');
    await page.getByRole('button', { name: 'Сохранить статус' }).click();
    await page.getByTestId('service-tab-mchs').getByText('Начало реагирования').waitFor();

    await page.getByTestId('service-tab-mchs')
      .getByRole('button', { name: 'История статусов' }).click();
    await shot(page, info, '15-card-dds-history');
  });

  test('отказ без комментария', async ({ page }, info) => {
    const number = await createFireCard(page, info);

    await login(page, info, MCHS);
    await page.getByTestId(`journal-row-${number}`).click();
    await page.getByTestId('card-class').waitFor();

    await page.getByTestId('pencil-mchs').click();
    await page.getByRole('combobox', { name: 'Статус' }).selectOption('Не принята');
    await page.getByRole('button', { name: 'Сохранить статус' }).click();
    await page.getByRole('alert').waitFor();
    await shot(page, info, '16-card-dds-comment-required');
  });

  test('мобильная ширина', async ({ page }, info) => {
    await page.setViewportSize({ width: 400, height: 900 });
    await login(page, info);
    await shot(page, info, '17-mobile-journal');

    await page.getByTestId('launch-t3-c1').click();
    await page.getByTestId('card-timer').waitFor();
    await shot(page, info, '18-mobile-card112');
  });
});
