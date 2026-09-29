// Общий набор для UX-прохода: вход, снимок шага, сбор ошибок.
// Не входит в npm test: проход живой, на своём экземпляре.
import { chromium } from '@playwright/test';
import { mkdirSync, appendFileSync } from 'node:fs';

export const BASE = process.env.UX_BASE ?? 'http://127.0.0.1:5311';
export const API = process.env.UX_API ?? 'http://127.0.0.1:8211';
export const OUT = process.env.UX_OUT ?? '../../../docs/ux/2';
export const LOG = `${OUT}/журнал.txt`;
mkdirSync(OUT, { recursive: true });

export const log = (...a) => { const s = a.join(' '); console.log(s); appendFileSync(LOG, s + '\n'); };

export async function browser(args = []) {
  return chromium.launch({ args });
}

export async function page(b, who, { width = 1600, height = 900, extra = {} } = {}) {
  const ctx = await b.newContext({ viewport: { width, height }, locale: 'ru-RU', ...extra });
  const p = await ctx.newPage();
  p.on('console', (m) => { if (m.type() === 'error') log(`[${who}] console.error:`, m.text().slice(0, 300)); });
  p.on('pageerror', (e) => log(`[${who}] pageerror:`, String(e).slice(0, 300)));
  p.on('response', (r) => {
    if (r.status() >= 400 && r.url().includes('/api/')) {
      log(`[${who}] HTTP ${r.status()} ${r.request().method()} ${r.url().replace(BASE, '')}`);
    }
  });
  return p;
}

export async function login(p, who) {
  await p.goto(`${BASE}/#/login`);
  await p.evaluate(() => localStorage.clear());
  await p.reload();
  await p.locator('input[name="login"]').fill(who);
  await p.locator('input[name="password"]').fill('arm112');
  await p.getByTestId('login-submit').click();
  await p.waitForTimeout(2500);
}

let n = 0;
export async function shot(p, name) {
  n += 1;
  // Номер шага впереди: снимки разных проходов не перемешиваются
  const step = process.env.UX_STEP ? `${process.env.UX_STEP}-` : '';
  const f = `${OUT}/${step}${String(n).padStart(2, '0')}-${name}.png`;
  await p.screenshot({ path: f });
  return f;
}

export async function hscroll(p) {
  return p.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
}

export async function api(method, path, body, token) {
  const r = await fetch(API + path, {
    method,
    headers: { 'Content-Type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await r.text();
  let json = null;
  try { json = text ? JSON.parse(text) : null; } catch { json = text; }
  return { status: r.status, json };
}

export async function token(who) {
  return (await api('POST', '/api/auth/login', { login: who, password: 'arm112' })).json.token;
}
