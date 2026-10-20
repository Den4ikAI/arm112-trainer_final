import { expect, test } from '@playwright/test';
import { go, login, sql, USERS } from './helpers';

/**
 * Финал: тренажёр сосредоточен на работе специалиста ДДС.
 *
 * Обратная связь экспертов 18.10:
 * 1. показать, как ИИ формирует карточки для специалиста ДДС;
 * 2. логины и пароли — на экран входа, «чтобы мы не искали»;
 * 3. систему 112 свернуть, «чтобы глаза не мозолила».
 *
 * Модель на прогоне — заглушка: текста она не пишет, и происшествие
 * обязано собраться без неё. Признаки и службы считает классификатор.
 */

const свернуть112 = (да: boolean) =>
  sql(`UPDATE settings SET value = '${да ? 0 : 1}' WHERE key = 'ui.op112';`);

test.describe('ДДС в центре', () => {
  test.describe.configure({ mode: 'serial' });
  test.beforeAll(() => свернуть112(true));
  test.afterAll(() => свернуть112(false));

  test('на экране входа — учётные записи стенда, вход одним щелчком', async ({ page }, info) => {
    await go(page, info, '/login');
    await page.evaluate(() => localStorage.clear());
    await page.reload();

    const demo = page.getByTestId('demo-access');
    await expect(demo).toBeVisible();
    await expect(demo).toContainText('arm112');
    await expect(page.getByTestId('demo-teacher1')).toContainText('Преподаватель');
    // Диспетчер службы 101 — с названием службы
    await expect(demo.getByText(/Диспетчер ДДС · Служба 101/)).toBeVisible();
    // 112 свёрнут — оператора на входе нет
    await expect(page.getByTestId(`demo-${USERS.operator}`)).toHaveCount(0);

    await page.getByTestId('demo-teacher1').click();
    await expect(page).toHaveURL(/#\/journal/);
    // Вместо билетов 112 — вход в работу с происшествиями ДДС
    await expect(page.getByTestId('dds-start')).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Учебные вызовы' })).toHaveCount(0);
  });

  test('преподаватель собирает происшествие от ИИ для службы и утверждает', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await page.getByTestId('dds-start-ai').click();

    await expect(page.getByTestId('ai-builder')).toBeVisible();
    // Вкладки вызовов 112 нет — только происшествия ДДС
    await expect(page.getByTestId('tab-112')).toHaveCount(0);
    await expect(page.getByTestId('tab-dds')).toHaveAttribute('aria-selected', 'true');

    await page.getByTestId('ai-service').selectOption('mosgaz');
    await page.getByTestId('ai-submit').click();

    const view = page.getByTestId('incident-view');
    await expect(view).toBeVisible({ timeout: 30_000 });
    await expect(page.getByTestId('incident-source')).toContainText('собрано ИИ');
    await expect(page.getByTestId('incident-view-services')).toContainText('104');
    // Полная карточка: утверждается без правки
    await page.getByTestId('incident-approve').click();
    await expect(view).toContainText('утверждено');
  });

  test('занятие собирается только для диспетчеров', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await go(page, info, '/lessons');
    const mode = page.getByTestId('lesson-mode');
    await expect(mode).toHaveValue('dds');
    await expect(mode.locator('option[value="card112"]')).toHaveCount(0);
  });

  test('диспетчер берёт тренировку с ИИ — карточка открывается сама', async ({ page }, info) => {
    await login(page, info, USERS.mchs);
    await page.getByTestId('practice-ai').click();

    await expect(page).toHaveURL(/#\/card-dds\//, { timeout: 30_000 });
    await expect(page.getByTestId('card-number')).toBeVisible();
    await expect(page.getByTestId('card-class')).toBeVisible();

    // В разделе — как выданное на занятии, тема — личная тренировка
    await go(page, info, '/journal');
    await expect(page.getByTestId('student-work'))
      .toContainText('Самостоятельная тренировка с ИИ');
  });

  test('администратор разворачивает 112 в параметрах — интерфейс перестраивается', async ({ page }, info) => {
    await login(page, info, 'admin');
    await go(page, info, '/admin');
    await page.getByTestId('admin-tab-settings').click();
    const row = page.getByTestId('setting-ui.op112');
    await row.locator('input').fill('1');
    await row.getByRole('button', { name: 'сохранить' }).click();
    await expect(row).toContainText('Сохранено');

    await go(page, info, '/scenarios');
    await expect(page.getByTestId('tab-112')).toBeVisible();
  });

  test('систему 112 включают с экрана входа — только паролем администратора', async ({ page }, info) => {
    свернуть112(true);
    await go(page, info, '/login');
    await page.evaluate(() => localStorage.clear());
    await page.reload();

    const sw = page.getByTestId('op112-switch');
    await expect(sw.getByTestId('op112-state')).toHaveText('выключена');
    await expect(page.getByTestId(`demo-${USERS.operator}`)).toHaveCount(0);

    await sw.getByTestId('op112-toggle').click();
    // Преподаватель — не администратор: сервер не даст поменять параметр
    await sw.getByLabel('Логин администратора').fill('teacher1');
    await sw.getByLabel('Пароль администратора').fill('arm112');
    await sw.getByTestId('op112-apply').click();
    await expect(sw.getByTestId('op112-error')).toContainText('только администратор');
    await expect(sw.getByTestId('op112-state')).toHaveText('выключена');

    await sw.getByLabel('Логин администратора').fill('admin');
    await sw.getByTestId('op112-apply').click();
    await expect(sw.getByTestId('op112-state')).toHaveText('включена');
    await expect(page.getByTestId(`demo-${USERS.operator}`)).toBeVisible();
    expect(Number(sql(`SELECT value::text FROM settings WHERE key = 'ui.op112';`).trim())).toBe(1);

    // Обратно — тем же переключателем
    await sw.getByTestId('op112-toggle').click();
    await sw.getByLabel('Пароль администратора').fill('arm112');
    await sw.getByTestId('op112-apply').click();
    await expect(sw.getByTestId('op112-state')).toHaveText('выключена');
    await expect(page.getByTestId(`demo-${USERS.operator}`)).toHaveCount(0);
  });
});
