import { test, expect, type WebSocketRoute } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { login, launchTicket3 } from './helpers';

/**
 * ТЗ §7: «работоспособность при кратковременных сбоях сети (до 30
 * секунд) с автоматическим восстановлением сессий без потери данных».
 *
 * Курсант продолжает заполнять карточку, пока сети нет. Правки копятся
 * в клиенте и уходят сами, когда связь вернулась, — без перезагрузки
 * и без повторного ввода. Проверяем по базе, а не по экрану: на экране
 * введённое видно и без сервера.
 */

const БАЗА_ПРОГОНА =
  (process.env.TEST_DATABASE_URL ?? 'arm112_test').split('/').pop() ?? 'arm112_test';

/** Адрес и описание последней карточки, которую завёл курсант. */
const вБазе = (login: string): { settlement: string; street: string; description: string } => {
  const out = execFileSync('docker', [
    'exec', 'arm112-db', 'psql', '-U', 'arm112', '-d', БАЗА_ПРОГОНА, '-qAtc',
    `SELECT json_build_object(
              'settlement', coalesce(c.address->>'settlement', ''),
              'street', coalesce(c.address->>'street', ''),
              'description', c.description)
       FROM cards c JOIN users u ON u.id = c.created_by
      WHERE u.login = '${login}'
      ORDER BY c.created_at DESC LIMIT 1`,
  ]).toString().trim();
  return JSON.parse(out);
};

test('правки, сделанные без сети, доходят до сервера после 30 секунд обрыва', async ({ page, context }, info) => {
  test.setTimeout(120_000);
  // Эмуляция offline в Chromium уже открытый WebSocket не рвёт — рвём
  // сами: пока «сети нет», канал закрыт и новые попытки отбиваются.
  // Заодно запоминаем, когда канал открывался
  let сетьЕсть = true;
  const каналы: number[] = [];
  const открытые: WebSocketRoute[] = [];
  await page.routeWebSocket(/\/ws/, (ws) => {
    if (!сетьЕсть) {
      void ws.close();
      return;
    }
    каналы.push(Date.now());
    ws.connectToServer();
    открытые.push(ws);
  });

  await login(page, info);
  await launchTicket3(page);

  // Город спрошен до обрыва — после восстановления он должен остаться
  // «выясненным», а не превратиться в догадку
  await page.getByTestId('ask-city').click();
  await expect(page.getByTestId('transcript')).toContainText('Королёв');

  await context.setOffline(true);
  сетьЕсть = false;
  await Promise.all(открытые.splice(0).map((ws) => ws.close()));

  await page.getByRole('textbox', { name: 'Населенный пункт:' }).fill('Королёв');
  await page.getByRole('textbox', { name: 'Улица:' }).fill('Станционная');
  await page.getByLabel('Описание со слов заявителя').fill('Горит крыша частного дома');

  // Клиент честно говорит, что связи нет, но введённое не потеряно
  await expect(page.getByRole('status')).toContainText('правки не потеряны');
  expect(вБазе('student1').settlement).toBe('');

  await page.waitForTimeout(30_000);
  сетьЕсть = true;
  await context.setOffline(false);
  const вернулась = Date.now();

  // Досылка — по событию «сеть есть», страховка — повтор раз в 3 с
  await expect.poll(() => вБазе('student1'), { timeout: 15_000, intervals: [250] })
    .toEqual({
      settlement: 'Королёв',
      street: 'Станционная',
      description: 'Горит крыша частного дома',
    });
  // Для backend/docs/замеры/НАГРУЗКА.md: сколько догоняли после обрыва
  console.log(`правки в базе через ${Date.now() - вернулась} мс после возврата сети`);

  // Канал класса тоже поднялся сам — не через полминуты по расписанию
  // попыток (после обрыва в 30 с следующая была бы только через 18 с),
  // а сразу, как браузер сообщил о сети
  await expect.poll(() => каналы.filter((t) => t >= вернулась).length, { timeout: 5_000 })
    .toBeGreaterThan(0);
  console.log(`канал событий открыт через ${каналы.find((t) => t >= вернулась)! - вернулась} мс`);
  await expect(page.getByRole('status')).toHaveCount(0);

  // Сессия та же: сохраняется и разбирается как обычно
  await page.getByTestId('save-card').click();
  await expect(page.getByTestId('score')).toBeVisible();
  await expect(page.getByTestId('finding-guessed').filter({ hasText: 'Королёв' }))
    .toHaveCount(0);
});
