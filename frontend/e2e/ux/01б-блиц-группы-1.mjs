// Поправка шага 1: первый прогон по неточному совпадению запустил блиц
// второй группы. Закрываем его и запускаем блиц первой группы.
import { api, token, log } from './kit.mjs';

const t = await token('teacher1');
const { json: lessons } = await api('GET', '/api/lessons', null, t);
for (const l of lessons) {
  log('[teacher1] занятие', l.id, JSON.stringify({ title: l.title, kind: l.kind, mode: l.mode, started: l.started_at ?? l.startedAt, group: l.group_name ?? l.groupName ?? l.group_id }));
}
const g2 = lessons.find((l) => l.title === 'Тренировка по частям: статусы по докладам (2)');
if (g2) log('[teacher1] завершить блиц группы 2:', (await api('POST', `/api/lessons/${g2.id}/finish`, {}, t)).status);
const g1 = lessons.find((l) => l.title === 'Тренировка по частям: статусы по докладам');
if (g1) {
  const r = await api('POST', `/api/lessons/${g1.id}/start`, {}, t);
  log('[teacher1] запуск блица группы 1:', r.status, JSON.stringify(r.json).slice(0, 200));
}
