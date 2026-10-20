import { expect, test, type Page } from '@playwright/test';
import { go, login, sql, USERS } from './helpers';

/**
 * Рекомендации диспетчеру ДДС и следующее задание от ИИ под них.
 *
 * Рекомендации финалистам, п. 5 и 10: цепочка «ИИ формирует задание →
 * обучаемый ДДС выполняет → результат → рекомендации для дальнейшего
 * обучения» без преподавателя. Проверяем путь целиком: диспетчер
 * проставил статус раньше доклада и закрыл карточку — в разборе
 * появилась рекомендация из памятки; по ней он берёт следующее
 * задание от ИИ, и новая карточка помнит, что она отрабатывает.
 *
 * Модель на прогоне — заглушка: происшествие собирается без неё.
 */

/** Сохранённая карточка, направленная в службу 101 и выданная диспетчеру. */
const карточкаДляМчс = (): string => {
  const number = `РД${Date.now()}`;
  sql(`
    DO $$
    DECLARE u uuid; g uuid; t uuid; c uuid; l uuid;
    BEGIN
      INSERT INTO cards (number, saved_at, type_ids, description, origin)
        VALUES ('${number}', now(), '{101}', 'проверка рекомендаций ДДС', 'teacher')
        RETURNING id INTO c;
      INSERT INTO card_services (card_id, service_id) VALUES (c, 'mchs');
      SELECT id INTO u FROM users WHERE login = '${USERS.mchs}';
      SELECT id INTO t FROM users WHERE login = 'teacher1';
      SELECT group_id INTO g FROM group_members WHERE user_id = u LIMIT 1;
      IF g IS NULL THEN
        INSERT INTO training_groups (title, teacher_id)
          VALUES ('выдача ' || gen_random_uuid(), t) RETURNING id INTO g;
        INSERT INTO group_members (group_id, user_id) VALUES (g, u);
      END IF;
      INSERT INTO lessons (group_id, teacher_id, title, mode, started_at, finished_at)
        VALUES (g, t, 'выдача происшествия', 'dds', now(), now()) RETURNING id INTO l;
      INSERT INTO lesson_cards (lesson_id, card_id, release_at) VALUES (l, c, now());
    END $$;`);
  return number;
};

/** Первая запись — статус и текст (заказчик, чат 27.09). */
const статус = async (page: Page, status: string): Promise<void> => {
  await page.getByTestId('pencil-mchs').click();
  await page.getByRole('combobox', { name: 'Статус' }).selectOption(status);
  if (status === 'Принята') {
    await page.getByRole('textbox', { name: 'Комментарий' }).fill('Принято в работу');
  }
  await page.getByRole('button', { name: 'Сохранить статус' }).click();
  await expect(page.getByTestId('service-tab-mchs')).toContainText(status);
};

test.describe('Рекомендации диспетчеру ДДС', () => {
  test.describe.configure({ mode: 'serial' });

  // История рекомендаций личная: прошлые прогоны на той же учётке
  // не должны решать, что увидит эта проверка
  test.beforeAll(() => {
    sql(`DELETE FROM dds_advice WHERE user_id = (SELECT id FROM users WHERE login = '${USERS.mchs}');`);
  });

  test('статус раньше доклада — после закрытия карточки рекомендация и задание от ИИ под неё', async ({ page }, info) => {
    const number = карточкаДляМчс();
    await login(page, info, USERS.mchs);
    await page.getByTestId(`journal-row-${number}`).click();
    await expect(page.getByTestId('brigade-panel')).toBeVisible();

    await статус(page, 'Принята');
    await page.getByTestId('brigade-assign').click();
    await expect(page.getByTestId('brigade-panel')).toContainText('№ 1');

    // Пока карточка в работе, рекомендаций в разборе нет
    await expect(page.getByTestId('my-dds-advice')).toHaveCount(0);

    // Диспетчер «угадал»: сведений о прибытии у него нет
    await статус(page, 'Прибытие');
    await expect(page.getByTestId('dds-finding').first()).toContainText('не по факту');

    // Служба закончила работу — разбор окончательный, рекомендация выдана
    await статус(page, 'Работы завершены');
    const advice = page.getByTestId('my-dds-advice');
    await expect(advice).toBeVisible();
    await expect(advice).toContainText('Что тренировать дальше');
    const open = page.getByTestId('dds-advice-open').filter({ hasText: 'по факту' });
    await expect(open.first()).toBeVisible();

    // Следующее задание от ИИ — без преподавателя: новая карточка своей
    // службы открывается сама и помнит, какую рекомендацию отрабатывает
    const была = page.url();
    await open.first().getByTestId('dds-advice-next').click();
    await expect.poll(() => page.url(), { timeout: 30_000 }).not.toBe(была);
    await expect(page.getByTestId('dds-advice-focus')).toContainText(
      'Задание от ИИ под рекомендацию', { timeout: 30_000 });
    const новый = (await page.getByTestId('card-number').innerText()).trim();
    expect(новый).not.toBe(number);

    // В разделе работ — та же рекомендация и ссылка на выданное задание
    await go(page, info, '/journal');
    const work = page.getByTestId('student-work');
    await expect(work).toContainText('Самостоятельная тренировка с ИИ');
    await expect(work.getByTestId('dds-advice-open').first()).toContainText('по факту');
    await expect(work.getByTestId('dds-advice-next-open').first()).toBeVisible();
  });

  test('преподаватель видит исполнение рекомендаций диспетчеров по группе', async ({ page }, info) => {
    await login(page, info, 'teacher1');
    await go(page, info, '/analytics');
    await page.getByTestId('analytics-people-tab').click();
    // Вкладка «Диспетчеры ДДС» есть только при развёрнутой системе 112
    const tab = page.getByTestId('people-analytics').getByRole('button', { name: 'Диспетчеры ДДС' });
    if (await tab.count()) await tab.click();
    await expect(page.getByTestId('advice-block-dds')).toBeVisible();
  });
});
