import { test } from '@playwright/test';
import path from 'node:path';
import { Demo } from './kit';
import * as step from './steps';

/**
 * Сквозной путь для демо-ролика.
 *
 *   DEMO_PROFILE=gpu|cpu   профиль модели (подпись в конце и набор шагов)
 *   DEMO_OUT=<каталог>     куда сложить кадры и звук (по умолчанию /tmp/arm112-demo/<профиль>)
 *   DEMO_PARTS=intro,teacher,operator,dds,review,outro   какие шаги снимать
 *   DEMO_DB=<база>         демо-база (только для ускорения этапов бригады)
 */
test('демо: сквозной путь', async ({ page }) => {
  const profile = (process.env.DEMO_PROFILE ?? 'gpu') as 'gpu' | 'cpu';
  const out = process.env.DEMO_OUT ?? path.join('/tmp/arm112-demo', profile);
  const parts = new Set((process.env.DEMO_PARTS
    ?? 'intro,teacher,operator,dds,review,outro').split(','));
  const ctx: step.Ctx = { profile, db: process.env.DEMO_DB ?? 'arm112_demo' };

  const d = await Demo.init(page, out);
  await page.goto('/#/login');
  await d.startRecording();
  // Сломанная сцена не валит ролик: остальные снимаются, ошибка — в отчёт
  const errors: string[] = [];
  const scene = async (name: string, run: () => Promise<void>) => {
    if (!parts.has(name)) return;
    try { await run(); } catch (e) {
      errors.push(`${name}: ${String(e).split('\n')[0]}`);
      console.error(`сцена ${name} упала:`, e);
      await d.ring(null).catch(() => {});
    }
  };
  try {
    await scene('intro', () => step.intro(d));
    await scene('teacher', () => step.teacherPrepare(d, ctx));
    await scene('operator', () => step.operator112(d, ctx));
    if (ctx.number) await scene('dds', () => step.dispatcherDds(d, ctx));
    if (ctx.number) await scene('review', () => step.teacherReview(d, ctx));
    await scene('outro', () => step.outro(d, ctx));
  } finally {
    await d.stopRecording();
  }
  if (errors.length) throw new Error(`упали сцены: ${errors.join('; ')}`);
});
