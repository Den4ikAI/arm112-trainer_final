// Подготовка к проверке доступа: op01 заводит и сохраняет карточку 112
// по пожару в доме (уходит МЧС и ЦОДД, не Мослифту).
import { api, token, log } from './kit.mjs';

const t = await token('op01');
const sc = (await api('GET', '/api/scenarios', null, t)).json;
const fire = sc.find((s) => /пожар/i.test(s.title ?? '')) ?? sc[0];
const s = await api('POST', '/api/sessions', { scenarioId: fire.id, mode: 'card112' }, t);
if (s.status >= 300) { log('[op01] сессия не создана:', s.status, JSON.stringify(s.json)); process.exit(1); }
const sid = s.json.session.id;
const p = await api('PATCH', `/api/sessions/${sid}/card`, {
  description: 'Горит квартира, дым из окна', typeIds: ['101'],
  address: { street: 'Академика Королёва', house: '12' },
}, t);
log('[op01] правка карточки:', p.status);
const sv = await api('POST', `/api/sessions/${sid}/save`, {}, t);
log('[op01] сохранение:', sv.status, JSON.stringify(sv.json).slice(0, 200));
console.log(`CARD112=${s.json.card.id} NUMBER=${s.json.card.number}`);
