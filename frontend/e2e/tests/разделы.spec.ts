import { test, expect } from '@playwright/test';
import path from 'node:path';
import { login, fillFireCard, sql, USERS, выдатьВызов, выдатьПроисшествие } from './helpers';

/**
 * Разделы курсанта (Денис, 29.09): у оператора 112 — «Учебные вызовы»,
 * «Экзамены», «Тренировка по частям»; у диспетчера ДДС — «Учебные
 * происшествия», «Экзамены», «Тренировка по частям». Выдаёт только
 * преподаватель; закончённое открывается на просмотр, пройти заново нельзя.
 */
const КАДРЫ = path.resolve(__dirname, '../../screenshots/react');

test.describe('Разделы курсанта', () => {
  test('оператор 112: выданный вызов проходится один раз, потом — только разбор',
    async ({ page }, info) => {
      выдатьВызов(USERS.operator, 3, 1);
      await login(page, info, USERS.operator);
      await page.getByTestId('section-training').click();

      const начать = page.getByTestId('launch-t3-c1').first();
      const строка = page.locator('tr', { has: начать });
      await expect(строка.getByTestId('work-status')).toHaveText('не начато');
      await page.screenshot({ path: path.join(КАДРЫ, '16-разделы-112.png') });

      await начать.click();
      await page.getByTestId('card-timer').waitFor();
      await fillFireCard(page);
      await page.getByTestId('save-card').click();
      await expect(page.getByTestId('score')).toBeVisible();

      // Вернулся — строка закончена, с баллом, и начать её снова нельзя
      await page.getByRole('button', { name: 'Вернуться в журнал' }).click();
      await page.getByTestId('section-training').click();
      const первая = page.locator('[data-testid^="work-row-"]').first();
      await expect(первая.getByTestId('work-status')).toHaveText('закончено');
      await expect(первая.getByRole('button', { name: 'Разбор' })).toBeVisible();
      await expect(первая.getByRole('button', { name: 'Начать' })).toHaveCount(0);

      // Разбор открывается на просмотр
      await первая.getByRole('button', { name: 'Разбор' }).click();
      await expect(page.getByTestId('score')).toBeVisible();
    });

  test('диспетчер ДДС: в разделе и в ленте — только выданное преподавателем',
    async ({ page }, info) => {
      // Два учебных происшествия МЧС: одно выдано, другое нет
      const выдано = `В${Date.now()}`;
      const невыдано = `Н${Date.now()}`;
      for (const n of [выдано, невыдано]) {
        const id = sql(`INSERT INTO cards (number, saved_at, type_ids, description, origin, address)
                        VALUES ('${n}', now(), '{101}', 'Горит крыша частного дома', 'teacher',
                                '{"settlement": "Королёв", "street": "Станционная", "house": "28"}')
                        RETURNING id`).trim();
        sql(`INSERT INTO card_services (card_id, service_id) VALUES ('${id}', 'mchs')`);
      }
      выдатьПроисшествие(выдано, USERS.mchs);

      await login(page, info, USERS.mchs);
      await expect(page.getByTestId('section-training')).toHaveText('Учебные происшествия');
      await expect(page.getByTestId('section-exam')).toHaveText('Экзамены');
      await expect(page.getByTestId('section-blitz')).toHaveText('Тренировка по частям');
      await page.getByTestId('section-training').click();

      await expect(page.getByTestId(`journal-row-${выдано}`)).toBeVisible();
      await expect(page.getByTestId(`journal-row-${невыдано}`)).toHaveCount(0);
      await expect(page.locator('[data-testid^="work-row-"]').first()).toBeVisible();
      await page.screenshot({ path: path.join(КАДРЫ, '17-разделы-ддс.png') });
    });
});
