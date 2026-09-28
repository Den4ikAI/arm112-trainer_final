import { test, expect } from '@playwright/test';
import { login, launchTicket3 } from './helpers';

/**
 * Разговор оператора 112 с заявителем своими словами — текстом или
 * голосом (заказчик 27.09: «будет плюсом»).
 *
 * Сервер прогона работает на заглушке модели: свободный вопрос он
 * понимает по словам, детерминированно. Голос подменяется в браузере,
 * микрофон — поддельный Chromium: проверяется то, за что отвечает
 * клиент.
 */

test.use({
  launchOptions: {
    args: ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'],
  },
  permissions: ['microphone'],
});

/** Короткая тишина в WAV: озвучиванию нужен настоящий звук. */
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

test.describe('Заявитель: вопрос своими словами', () => {
  test('напечатанный вопрос понят, непонятый — заявительница переспрашивает', async ({ page }, info) => {
    await login(page, info);
    await launchTicket3(page);

    // Голоса нет — нет ни микрофона, ни переключателя звука
    await page.getByTestId('hud-mode-free').click();
    await expect(page.getByTestId('hud-talk')).toHaveCount(0);
    await expect(page.getByTestId('hud-sound')).toHaveCount(0);

    await page.getByTestId('hud-free-input').fill('А город у вас какой?');
    await page.getByTestId('hud-free-send').click();

    const разговор = page.getByTestId('transcript');
    await expect(разговор).toContainText('А город у вас какой?');
    await expect(разговор).toContainText('Королёв');
    await expect(page.getByText('1 / 5')).toBeVisible();

    await page.getByTestId('hud-free-input').fill('Как вас зовут?');
    await page.getByTestId('hud-free-input').press('Enter');
    await expect(разговор).toContainText('Не поняла вас, повторите, пожалуйста.');
    await expect(page.getByText('1 / 5')).toBeVisible();

    // Вернулись к списку — выясненный город зачёркнут, как после кнопки
    await page.getByTestId('hud-mode-buttons').click();
    await expect(page.getByTestId('ask-city')).toBeDisabled();
  });

  test('вопрос голосом распознаётся, ответ звучит голосом заявительницы', async ({ page }, info) => {
    await page.route('**/api/voice/status', (r) => r.fulfill({
      json: { asr: true, tts: true, voices: ['brigade', 'chief', 'applicant_m', 'applicant_f'] },
    }));
    await page.route('**/api/voice/asr', (r) => r.fulfill({
      json: { text: 'Пострадавшие есть кто-нибудь?', durationMs: 1400 },
    }));
    const озвучено: string[] = [];
    await page.route('**/api/voice/tts', (r) => {
      const body = r.request().postDataJSON() as { text: string; voice: string };
      озвучено.push(`${body.voice}: ${body.text}`);
      return r.fulfill({ status: 200, contentType: 'audio/wav', body: тишина() });
    });

    await login(page, info);
    await launchTicket3(page);
    await expect(page.getByTestId('hud-sound')).toBeChecked();
    // Заявительница начинает разговор сама — легенда звучит сразу
    await expect.poll(() => озвучено.some((x) => x.startsWith('applicant_f: Алло!'))).toBe(true);

    await page.getByTestId('hud-mode-free').click();
    const talk = page.getByTestId('hud-talk');
    await talk.click();
    await expect(talk).toHaveText('■ Закончить');
    await page.waitForTimeout(700);

    const вопрос = page.waitForRequest((r) => r.url().includes('/ask-free'));
    await talk.click();
    expect(((await вопрос).postDataJSON() as { text: string }).text)
      .toBe('Пострадавшие есть кто-нибудь?');

    const разговор = page.getByTestId('transcript');
    await expect(разговор).toContainText('Пострадавшие есть кто-нибудь?');
    await expect(разговор).toContainText('пострадавших нет');
    await expect.poll(() => озвучено.some((x) => x.startsWith('applicant_f: Нет, пострадавших нет')))
      .toBe(true);
  });
});
