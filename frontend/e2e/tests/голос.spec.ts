import { test, expect, type Page, type TestInfo } from '@playwright/test';
import { login, launchTicket3, fillFireCard, USERS } from './helpers';

/**
 * Голос в разговоре диспетчера ДДС со старшим бригады.
 *
 * Настоящий сервер голоса прогону не нужен: без него проверяем, что
 * всё работает текстом, а с ним — подменяем его ответы в браузере и
 * берём поддельный микрофон Chromium. Проверяется то, за что отвечает
 * клиент: запись уходит на распознавание, реплика — вместе со звонком,
 * ответ старшего — на озвучивание.
 */

test.use({
  launchOptions: {
    args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
  },
  permissions: ['microphone'],
});

/** Короткая тишина в WAV: озвучиванию нужен настоящий звук, не пустота. */
const тишина = (): Buffer => {
  const rate = 16_000;
  const samples = rate / 10;
  const buf = Buffer.alloc(44 + samples * 2);
  buf.write('RIFF', 0);
  buf.writeUInt32LE(36 + samples * 2, 4);
  buf.write('WAVEfmt ', 8);
  buf.writeUInt32LE(16, 16);
  buf.writeUInt16LE(1, 20);
  buf.writeUInt16LE(1, 22);
  buf.writeUInt32LE(rate, 24);
  buf.writeUInt32LE(rate * 2, 28);
  buf.writeUInt16LE(2, 32);
  buf.writeUInt16LE(16, 34);
  buf.write('data', 36);
  buf.writeUInt32LE(samples * 2, 40);
  return buf;
};

/** Оператор сохраняет пожар, диспетчер МЧС принимает и направляет бригаду. */
const бригадаНаправлена = async (page: Page, info: TestInfo): Promise<void> => {
  await login(page, info);
  await launchTicket3(page);
  await fillFireCard(page);
  await page.getByRole('textbox', { name: 'Улица:' }).fill('Станционная');
  const number = (await page.getByTestId('card-number').innerText()).trim();
  await page.getByTestId('save-card').click();
  await expect(page.getByTestId('score')).toBeVisible();

  await login(page, info, USERS.mchs);
  await page.getByTestId(`journal-row-${number}`).click();
  await page.getByTestId('pencil-mchs').click();
  await page.getByRole('combobox', { name: 'Статус' }).selectOption('Принята');
  await page.getByRole('textbox', { name: 'Комментарий' }).fill('Принято в работу');
  await page.getByRole('button', { name: 'Сохранить статус' }).click();
  await expect(page.getByTestId('service-tab-mchs')).toContainText('Принята');
  await page.getByTestId('brigade-assign').click();
  await expect(page.getByTestId('brigade-panel')).toContainText('№ 1');
};

test.describe('Голос со старшим бригады', () => {
  test('без сервера голоса разговор идёт текстом', async ({ page }, info) => {
    await бригадаНаправлена(page, info);

    await expect(page.getByTestId('brigade-talk')).toHaveCount(0);
    await expect(page.getByTestId('brigade-sound')).toHaveCount(0);

    await page.getByTestId('brigade-call').click();
    await expect(page.getByTestId('brigade-answer'))
      .toHaveText('Старший бригады 1: пока без изменений.');
  });

  test('реплика голосом уходит со звонком, ответ старшего озвучивается', async ({ page }, info) => {
    await page.route('**/api/voice/status', (r) => r.fulfill({
      json: { asr: true, tts: true, voices: ['brigade', 'chief'] },
    }));
    await page.route('**/api/voice/asr', (r) => r.fulfill({
      json: { text: 'Доложите обстановку.', durationMs: 1200 },
    }));
    const озвучено: string[] = [];
    await page.route('**/api/voice/tts', (r) => {
      const body = r.request().postDataJSON() as { text: string; voice: string };
      озвучено.push(`${body.voice}: ${body.text}`);
      return r.fulfill({ status: 200, contentType: 'audio/wav', body: тишина() });
    });

    await бригадаНаправлена(page, info);
    await expect(page.getByTestId('brigade-sound')).toBeChecked();

    const talk = page.getByTestId('brigade-talk');
    await talk.click();
    await expect(talk).toHaveText('■ Закончить');
    await page.waitForTimeout(700);

    const звонок = page.waitForRequest((r) => r.url().includes('/brigade/call'));
    await talk.click();
    const тело = (await звонок).postDataJSON() as { said?: string };
    expect(тело.said).toBe('Доложите обстановку.');

    await expect(page.getByTestId('brigade-said')).toHaveText('Вы: Доложите обстановку.');
    await expect(page.getByTestId('brigade-answer'))
      .toHaveText('Старший бригады 1: пока без изменений.');
    await expect.poll(() => озвучено)
      .toContain('brigade: Старший бригады 1: пока без изменений.');
  });

  test('звук выключен — ответ не озвучивается', async ({ page }, info) => {
    await page.route('**/api/voice/status', (r) => r.fulfill({
      json: { asr: true, tts: true, voices: ['brigade', 'chief'] },
    }));
    let запросов = 0;
    await page.route('**/api/voice/tts', (r) => {
      запросов += 1;
      return r.fulfill({ status: 200, contentType: 'audio/wav', body: тишина() });
    });

    await бригадаНаправлена(page, info);
    await page.getByTestId('brigade-sound').uncheck();
    await page.getByTestId('brigade-call').click();
    await expect(page.getByTestId('brigade-answer')).toContainText('пока без изменений');
    await page.waitForTimeout(500);
    expect(запросов).toBe(0);
  });
});
