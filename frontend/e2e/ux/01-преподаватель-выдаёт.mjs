// Шаг 1: преподаватель кликами собирает занятие ДДС с пятью происшествиями
// поимённо и минимальным интервалом потока, запускает его, а также
// заготовленные экзамен ДДС и блиц «статусы по докладам».
import { browser, page, login, shot, log } from './kit.mjs';

const b = await browser();
const p = await page(b, 'teacher1');
await login(p, 'teacher1');
await shot(p, 'преподаватель-вход');

await p.getByRole('button', { name: /занятия/i }).first().click();
await p.waitForTimeout(1500);
await shot(p, 'занятия');

await p.getByTestId('lesson-group').selectOption({ label: 'Диспетчеры ДДС — группа 1' });
await p.getByTestId('lesson-title').fill('Поток ДДС: пять происшествий');
await p.getByTestId('lesson-mode').selectOption('dds');
await p.getByTestId('lesson-kind').selectOption('training');
await p.getByTestId('lesson-interval').fill('10');
const выбрать = [
  'Пожар на улице — происшествие 1', 'Задымление мусоропровода — происшествие 1',
  'Затопление — происшествие 1', 'ДТП с пострадавшими — происшествие 2', 'Плохо человеку — происшествие 2',
];
const box = p.getByTestId('lesson-material');
for (const t of выбрать) {
  const label = box.locator('label', { hasText: t });
  if (await label.count() === 0) { log('[teacher1] нет в списке материала:', t); continue; }
  await label.locator('input[type="checkbox"]').check();
}
await shot(p, 'занятие-ддс-форма');
await p.getByTestId('lesson-create').click();
await p.waitForTimeout(1500);
await shot(p, 'занятие-ддс-создано');
const start = p.getByTestId('lesson-start');
if (await start.count()) { await start.click(); await p.waitForTimeout(1500); } else log('[teacher1] нет кнопки запуска у нового занятия');
await shot(p, 'занятие-ддс-запущено');

for (const title of ['Экзамен ДДС: пожары, газ, ДТП, лифт, медицина', 'Тренировка по частям: статусы по докладам']) {
  const item = p.getByText(title, { exact: false }).first();
  if (await item.count() === 0) { log('[teacher1] нет занятия в списке:', title); continue; }
  await item.click();
  await p.waitForTimeout(800);
  const s = p.getByTestId('lesson-start');
  if (await s.count()) { await s.click(); await p.waitForTimeout(1200); log('[teacher1] запущено:', title); }
  else log('[teacher1] у занятия нет кнопки запуска:', title);
  await shot(p, 'запуск-' + title.slice(0, 20));
}
await b.close();
