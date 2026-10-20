import type { Browser, BrowserContext, CDPSession, Locator, Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';
import { OVERLAY } from './kit';

/**
 * Студия для ролика с несколькими рабочими местами: ПК преподавателя и
 * ПК обучаемых пишутся одновременно, каждое — своим контекстом браузера
 * и своим CDP screencast. Закадровый голос, титры и «какой экран сейчас
 * главный» — общая дорожка событий; docs/demo/assemble_multi.py
 * складывает всё в один кадр 1920×1080: активный экран крупно, остальные
 * рядом живыми миниатюрами, подписи и текст диктора снизу.
 *
 * В самой странице только курсор и отметка клика (из kit.ts); титры и
 * карточки рисует сборка, поэтому экран обучаемого в записи чистый.
 */

const NARRATOR = process.env.DEMO_NARRATOR_URL ?? 'http://127.0.0.1:8092';

interface Sound { at: number; file: string; text: string; voice: string }
interface Focus { at: number; screen: string }
interface Card { from: number; to: number; title: string; text: string }
interface Note { from: number; to: number; screen: string; text: string }

const wavSeconds = (buf: Buffer): number => {
  const byteRate = buf.readUInt32LE(28);
  const idx = buf.indexOf('data');
  const size = idx >= 0 ? buf.readUInt32LE(idx + 4) : buf.length - 44;
  return size / byteRate;
};

/** `[112|сто двенадцать]` → на экране «112», в голосе «сто двенадцать». */
const shown = (t: string): string => t.replace(/\[([^|\]]*)\|([^\]]*)\]/g, '$1');
const spoken = (t: string): string => t.replace(/\[([^|\]]*)\|([^\]]*)\]/g, '$2');

const now = (): number => Date.now() / 1000;

/** Одно рабочее место: страница, курсор, запись кадров. */
export class Screen {
  private cdp!: CDPSession;
  private frames: { file: string; at: number }[] = [];
  private pos: [number, number] = [640, 360];
  page!: Page;
  context!: BrowserContext;

  constructor(public name: string, public label: string, public out: string) {
    fs.mkdirSync(path.join(out, name, 'frames'), { recursive: true });
  }

  static async open(browser: Browser, name: string, label: string, out: string,
    baseURL: string): Promise<Screen> {
    const s = new Screen(name, label, out);
    s.context = await browser.newContext({
      viewport: { width: 1280, height: 720 }, deviceScaleFactor: 1, locale: 'ru-RU',
      permissions: ['microphone'], baseURL,
    });
    s.page = await s.context.newPage();
    await s.page.addInitScript(OVERLAY);
    s.cdp = await s.context.newCDPSession(s.page);
    s.cdp.on('Page.screencastFrame', (f) => {
      const file = `${String(s.frames.length).padStart(6, '0')}.jpg`;
      fs.writeFileSync(path.join(out, name, 'frames', file), Buffer.from(f.data, 'base64'));
      s.frames.push({ file, at: f.metadata.timestamp ?? now() });
      void s.cdp.send('Page.screencastFrameAck', { sessionId: f.sessionId }).catch(() => {});
    });
    await s.page.goto('/#/login');
    return s;
  }

  async startRecording(): Promise<void> {
    await this.cdp.send('Page.startScreencast', {
      format: 'jpeg', quality: 90, maxWidth: 1280, maxHeight: 720, everyNthFrame: 1,
    });
  }

  async stopRecording(): Promise<void> {
    await this.cdp.send('Page.stopScreencast').catch(() => {});
    fs.writeFileSync(path.join(this.out, this.name, 'frames.json'), JSON.stringify(this.frames));
  }

  async ring(target: Locator | null): Promise<void> {
    const box = target ? await target.boundingBox().catch(() => null) : null;
    await this.page.evaluate((r) => window.__demoRing(r), box).catch(() => {});
  }

  async moveTo(target: Locator): Promise<void> {
    await target.scrollIntoViewIfNeeded();
    const box = await target.boundingBox();
    if (!box) return;
    const x = box.x + Math.min(box.width / 2, 60);
    const y = box.y + box.height / 2;
    const dist = Math.hypot(x - this.pos[0], y - this.pos[1]);
    await this.page.mouse.move(x, y, { steps: Math.max(10, Math.min(40, Math.round(dist / 20))) });
    this.pos = [x, y];
    await this.page.waitForTimeout(150);
  }

  async click(target: Locator): Promise<void> {
    await this.moveTo(target);
    await target.click();
    await this.page.waitForTimeout(300);
  }

  async type(target: Locator, text: string, delay = 40): Promise<void> {
    await this.moveTo(target);
    await target.click();
    await target.fill('');
    await target.pressSequentially(text, { delay });
    await this.page.waitForTimeout(200);
  }

  async select(target: Locator, value: string | { label: string }): Promise<void> {
    await this.moveTo(target);
    await target.selectOption(value);
    await this.page.waitForTimeout(300);
  }

  async pause(ms: number): Promise<void> {
    await this.page.waitForTimeout(ms);
  }
}

export class Studio {
  screens: Screen[] = [];
  private sounds: Sound[] = [];
  private focus: Focus[] = [];
  private cards: Card[] = [];
  private notes: Note[] = [];
  private windows: { from: number; to: number; factor: number }[] = [];
  private n = 0;
  private openCard: Card | null = null;

  constructor(public out: string) {
    fs.rmSync(out, { recursive: true, force: true });
    fs.mkdirSync(path.join(out, 'audio'), { recursive: true });
  }

  async add(browser: Browser, name: string, label: string, baseURL: string): Promise<Screen> {
    const s = await Screen.open(browser, name, label, this.out, baseURL);
    this.screens.push(s);
    return s;
  }

  async start(): Promise<void> {
    for (const s of this.screens) await s.startRecording();
    this.focus.push({ at: now(), screen: this.screens[0].name });
  }

  async stop(): Promise<void> {
    if (this.openCard) this.openCard.to = now();
    for (const s of this.screens) await s.stopRecording();
    fs.writeFileSync(path.join(this.out, 'screens.json'),
      JSON.stringify(this.screens.map((s) => ({ name: s.name, label: s.label }))));
    fs.writeFileSync(path.join(this.out, 'sounds.json'), JSON.stringify(this.sounds, null, 1));
    fs.writeFileSync(path.join(this.out, 'focus.json'), JSON.stringify(this.focus));
    fs.writeFileSync(path.join(this.out, 'cards.json'), JSON.stringify(this.cards));
    fs.writeFileSync(path.join(this.out, 'notes.json'), JSON.stringify(this.notes));
    fs.writeFileSync(path.join(this.out, 'speedup.json'), JSON.stringify(this.windows));
    for (const s of this.screens) await s.context.close().catch(() => {});
  }

  /** Главный экран ролика с этого момента. */
  show(s: Screen): void {
    this.focus.push({ at: now(), screen: s.name });
  }

  private async voice(text: string, voice: string): Promise<{ file: string; sec: number }> {
    const plain = text.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();
    const r = await fetch(`${NARRATOR}/tts`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: plain, voice }),
    });
    if (!r.ok) throw new Error(`синтез «${plain}»: ${r.status} ${await r.text()}`);
    const buf = Buffer.from(await r.arrayBuffer());
    const file = path.join(this.out, 'audio', `${String(++this.n).padStart(3, '0')}-${voice}.wav`);
    fs.writeFileSync(file, buf);
    return { file, sec: wavSeconds(buf) };
  }

  private wait(ms: number): Promise<void> {
    return new Promise((r) => setTimeout(r, ms));
  }

  /** Закадровый текст: титр внизу кадра и голос диктора; ждём, пока договорит. */
  async say(text: string): Promise<void> {
    const v = await this.voice(spoken(text), 'narrator');
    this.sounds.push({ at: now(), file: v.file, text: shown(text), voice: 'narrator' });
    await this.wait(v.sec * 1000 + 300);
  }

  /** Текст идёт, пока выполняется действие; ждём и то, и другое. */
  async sayWhile(text: string, action: () => Promise<unknown>): Promise<void> {
    const v = await this.voice(spoken(text), 'narrator');
    const started = Date.now();
    this.sounds.push({ at: started / 1000, file: v.file, text: shown(text), voice: 'narrator' });
    await action();
    const left = v.sec * 1000 + 300 - (Date.now() - started);
    if (left > 0) await this.wait(left);
  }

  /** Реплика собеседника из приложения (старший бригады, начальник) — в дорожку. */
  async app(text: string, voice: string): Promise<void> {
    const plain = text.replace(/\s+/g, ' ').trim();
    if (!plain) return;
    const v = await this.voice(plain.slice(0, 300), voice);
    this.sounds.push({ at: now(), file: v.file, text: '', voice });
    await this.wait(v.sec * 1000 + 200);
  }

  /** Полнокадровая карточка (заставка) с этого момента до hideCard(). */
  card(title: string, text = ''): void {
    if (this.openCard) this.openCard.to = now();
    this.openCard = { from: now(), to: 0, title, text };
    this.cards.push(this.openCard);
  }

  hideCard(): void {
    if (this.openCard) this.openCard.to = now();
    this.openCard = null;
  }

  /** Пометка над экраном (например, «ждём модель») на время действия. */
  async note(s: Screen, text: string, action: () => Promise<unknown>): Promise<void> {
    const n: Note = { from: now(), to: 0, screen: s.name, text };
    this.notes.push(n);
    try { await action(); } finally { n.to = now(); }
  }

  /** Длинное ожидание: в ролике сжимается в `factor` раз с плашкой «ускорено». */
  async fast(factor: number, action: () => Promise<unknown>): Promise<void> {
    const from = now();
    try { await action(); } finally { this.windows.push({ from, to: now(), factor }); }
  }

  pause(ms: number): Promise<void> {
    return this.wait(ms);
  }
}
