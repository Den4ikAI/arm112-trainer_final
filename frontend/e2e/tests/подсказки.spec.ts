import { expect, test } from '@playwright/test';
import { login, sql, USERS } from './helpers';

/**
 * Финал: подсказки по интерфейсу рабочего места диспетчера ДДС.
 *
 * Рекомендация заказчика (п. 7): самостоятельное знакомство обучаемого
 * ДДС с интерфейсом — подсказки назначений функциональных окон и кнопок.
 * Кнопка «?» включает накладку: окна обведены и пронумерованы, у
 * текущего — пояснение, «Далее»/«Назад» ведут по окнам в порядке
 * работы по памятке. Вёрстка под накладкой не меняется.
 */

test.describe('Подсказки по интерфейсу ДДС', () => {
  test.describe.configure({ mode: 'serial' });

  let телефонБыл = '';
  test.beforeAll(() => {
    телефонБыл = sql(`SELECT value::text FROM settings WHERE key = 'ui.phone';`).trim();
    sql(`UPDATE settings SET value = '1' WHERE key = 'ui.phone';`);
  });
  test.afterAll(() => {
    if (телефонБыл) sql(`UPDATE settings SET value = '${телефонБыл}' WHERE key = 'ui.phone';`);
  });

  test('в ленте диспетчера подсказки объясняют задания и ленту происшествий', async ({ page }, info) => {
    await login(page, info, USERS.mchs);
    await expect(page).toHaveURL(/#\/journal/);

    // Курсант ещё не заглядывал — кнопка подписана как новая
    const кнопка = page.getByTestId('hints-toggle');
    await expect(кнопка).toBeVisible();
    await expect(кнопка).toHaveAttribute('data-seen', 'false');
    await expect(page.getByTestId('hints-layer')).toHaveCount(0);

    await кнопка.click();
    await expect(кнопка).toHaveAttribute('aria-pressed', 'true');
    await expect(кнопка).toHaveAttribute('data-seen', 'true');
    await expect(page.getByTestId('hint-num-work')).toBeVisible();
    await expect(page.getByTestId('hint-num-practice')).toBeVisible();
    await expect(page.getByTestId('hint-num-table')).toBeVisible();

    // Номер на рамке открывает пояснение этого окна
    await page.getByTestId('hint-num-work').click();
    await expect(page.getByTestId('hint-title')).toHaveText('Мои задания');
    await expect(page.getByTestId('hint-text')).toContainText('выданные преподавателем');

    // Интерфейс под накладкой живой: разделы заданий переключаются
    await page.getByTestId('section-exam').click();
    await expect(page.getByTestId('section-exam')).toHaveAttribute('aria-selected', 'true');

    await page.getByTestId('hints-close').click();
    await expect(page.getByTestId('hints-layer')).toHaveCount(0);
    await expect(кнопка).toHaveAttribute('aria-pressed', 'false');
    // Запомнено: при следующем входе кнопка уже не «новая»
    await page.reload();
    await expect(page.getByTestId('hints-toggle')).toHaveAttribute('data-seen', 'true');
  });

  test('на карточке ДДС тур идёт по окнам в порядке памятки и выключается', async ({ page }, info) => {
    await login(page, info, USERS.mchs);
    await page.getByTestId('practice-ai').click();
    await expect(page).toHaveURL(/#\/card-dds\//, { timeout: 30_000 });
    await expect(page.getByTestId('brigade-panel')).toBeVisible();

    await page.getByTestId('hints-toggle').click();
    const слой = page.getByTestId('hints-layer');
    await expect(слой).toBeVisible();
    // Ключевые окна рабочего места размечены
    for (const key of ['header', 'callbar', 'address', 'description', 'norms', 'services',
      'pencil', 'brigade', 'phone', 'chief', 'flags', 'class']) {
      await expect(page.getByTestId(`hint-num-${key}`)).toBeVisible();
    }
    await expect(page.getByTestId('hints-counter')).toContainText(/^1 из \d+$/);
    await expect(page.getByTestId('hint-title')).toHaveText('Шапка карточки');
    await expect(page.getByTestId('hints-prev')).toBeDisabled();

    // «Далее» ведёт по окнам; сначала — что пришло от 112 и читается без правок
    await page.getByTestId('hints-next').click();
    await expect(page.getByTestId('hint-title')).toHaveText('Телефоны заявителя');
    await expect(page.getByTestId('hint-text')).toContainText('без редактирования');
    await expect(page.getByTestId('hints-prev')).toBeEnabled();

    // Статусы — по памятке
    await page.getByTestId('hint-num-services').click();
    await expect(page.getByTestId('hint-title')).toHaveText('Службы и статусы реагирования');
    await expect(page.getByTestId('hint-text')).toContainText('«Принята» или «Не принята»');
    await expect(page.getByTestId('hint-text')).toContainText('«Работы завершены»');

    await page.getByTestId('hint-num-phone').click();
    await expect(page.getByTestId('hint-title')).toHaveText('IP-телефон');
    await page.getByTestId('hints-prev').click();
    await expect(page.getByTestId('hint-title')).toHaveText('Бригада');

    // Накладка не ломает рабочее место: телефон открывается кружком и в режиме подсказок
    await page.getByTestId('phone-launcher').click();
    await expect(page.getByTestId('ip-phone')).toBeVisible();

    // Esc выключает подсказки
    await page.keyboard.press('Escape');
    await expect(слой).toHaveCount(0);
    await expect(page.getByTestId('hint-callout')).toHaveCount(0);
    await expect(page.getByTestId('hints-toggle')).toHaveAttribute('aria-pressed', 'false');
  });

  test('у преподавателя и оператора 112 режима подсказок ДДС нет', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await expect(page).toHaveURL(/#\/journal/);
    await expect(page.getByTestId('hints-toggle')).toHaveCount(0);

    await login(page, info, USERS.operator);
    await expect(page).toHaveURL(/#\/journal/);
    await expect(page.getByTestId('hints-toggle')).toHaveCount(0);
  });
});
