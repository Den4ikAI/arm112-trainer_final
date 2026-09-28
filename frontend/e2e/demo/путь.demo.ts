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
  try {
    if (parts.has('intro')) await step.intro(d);
    if (parts.has('teacher')) await step.teacherPrepare(d, ctx);
    if (parts.has('operator')) await step.operator112(d, ctx);
    if (parts.has('dds') && ctx.number) await step.dispatcherDds(d, ctx);
    if (parts.has('review') && ctx.number) await step.teacherReview(d, ctx);
    if (parts.has('outro')) await step.outro(d, ctx);
  } finally {
    await d.stopRecording();
  }
});
