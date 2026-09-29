// Быстрый взгляд на данные: node probe.mjs <login> <METHOD> <path> [json]
import { api, token } from './kit.mjs';
const [who, method, path, body] = process.argv.slice(2);
const t = await token(who);
const r = await api(method, path, body ? JSON.parse(body) : null, t);
console.log(r.status, JSON.stringify(r.json, null, 1).slice(0, 6000));
