import type { CDPSession, Locator, Page } from '@playwright/test';
import fs from 'node:fs';
import path from 'node:path';

/**
 * Инструменты демо-ролика: видимый курсор, титры, закадровый голос и
 * запись экрана.
 *
 * Запись — через CDP screencast, а не через встроенное видео Playwright:
 * то пишется в VP8 с низким битрейтом, и мелкий текст карточки на нём
 * расплывается. Screencast отдаёт кадры JPEG с отметкой времени; из них
 * docs/demo/build.sh собирает MP4. Системного курсора в записи нет ни
 * там, ни там, поэтому курсор и отметка клика рисуются в самой странице.
 *
 * Закадровый текст синтезируется нашим же сервером голоса (голос
 * «narrator»), и шаг ждёт, пока фраза договорится: так титр, голос и
 * действие на экране не расходятся.
 */

const NARRATOR = process.env.DEMO_NARRATOR_URL ?? 'http://127.0.0.1:8193';

/** Код, который ставится в каждую загружаемую страницу. */
export const OVERLAY = `(() => {
  const S = sessionStorage;
  const install = () => {
    if (document.getElementById('demo-cursor')) return;
    const css = document.createElement('style');
    css.textContent = \`
      #demo-cursor { position: fixed; z-index: 2147483647; pointer-events: none;
        width: 28px; height: 28px; margin: -3px 0 0 -3px; transition: transform .08s;
        filter: drop-shadow(0 2px 3px rgba(0,0,0,.45)); }
      #demo-cursor.down { transform: scale(.85); }
      body:has(#demo-card) #demo-cursor { display: none; }
      .demo-ripple { position: fixed; z-index: 2147483646; pointer-events: none;
        width: 16px; height: 16px; margin: -8px 0 0 -8px; border-radius: 50%;
        border: 3px solid #FF0053; animation: demo-ripple .6s ease-out forwards; }
      @keyframes demo-ripple { to { transform: scale(4.5); opacity: 0; } }
      #demo-caption { position: fixed; z-index: 2147483645; pointer-events: none;
        left: 50%; bottom: 36px; transform: translateX(-50%); max-width: 1500px;
        padding: 14px 28px; border-radius: 14px; background: rgba(20,20,32,.86);
        color: #fff; font: 600 27px/1.35 Montserrat, 'Segoe UI', Arial, sans-serif;
        text-align: center; box-shadow: 0 6px 24px rgba(0,0,0,.35);
        transition: opacity .25s; }
      #demo-caption:empty { opacity: 0; }
      #demo-caption small { display: block; margin-top: 4px; font-weight: 500;
        font-size: 19px; color: #c9c6ee; }
      #demo-card { position: fixed; inset: 0; z-index: 2147483644; display: flex;
        flex-direction: column; align-items: center; justify-content: center; gap: 22px;
        background: linear-gradient(135deg, #1b1b2e 0%, #2d2a5a 60%, #8A83D1 140%);
        color: #fff; font-family: Montserrat, 'Segoe UI', Arial, sans-serif; text-align: center; }
      #demo-card h1 { margin: 0; font-size: 64px; font-weight: 800; letter-spacing: -.5px; }
      #demo-card h1 span { color: #FF0053; }
      #demo-card p { margin: 0; max-width: 1300px; font-size: 30px; line-height: 1.4; color: #dedcf5; }
      #demo-badge { position: fixed; z-index: 2147483645; pointer-events: none; top: 18px;
        left: 50%; transform: translateX(-50%); padding: 8px 18px; border-radius: 999px;
        background: #FF0053; color: #fff; font: 700 22px/1.2 Montserrat, 'Segoe UI', Arial, sans-serif;
        box-shadow: 0 4px 14px rgba(0,0,0,.3); }
      #demo-badge:empty { display: none; }
      #demo-ring { position: fixed; z-index: 2147483643; pointer-events: none;
        border: 3px solid #FF0053; border-radius: 10px; box-shadow: 0 0 0 6px rgba(255,0,83,.18);
        transition: all .25s ease; opacity: 0; }
    \`;
    document.head.appendChild(css);

    const cur = document.createElement('div');
    cur.id = 'demo-cursor';
    cur.innerHTML = '<svg viewBox="0 0 28 28" width="28" height="28"><path d="M3 2 L3 22 L8.5 16.8 L12.3 25 L16 23.4 L12.3 15.3 L20 15.3 Z" fill="#fff" stroke="#111" stroke-width="1.6" stroke-linejoin="round"/></svg>';
    const pos = JSON.parse(S.getItem('demo-pos') || '[960,540]');
    cur.style.left = pos[0] + 'px'; cur.style.top = pos[1] + 'px';
    document.body.appendChild(cur);

    const cap = document.createElement('div');
    cap.id = 'demo-caption';
    cap.innerHTML = S.getItem('demo-caption') || '';
    document.body.appendChild(cap);

    const ring = document.createElement('div');
    ring.id = 'demo-ring';
    document.body.appendChild(ring);

    const badge = document.createElement('div');
    badge.id = 'demo-badge';
    badge.textContent = S.getItem('demo-badge') || '';
    document.body.appendChild(badge);

    const card = S.getItem('demo-card');
    if (card) window.__demoCard(...JSON.parse(card));

    addEventListener('mousemove', (e) => {
      cur.style.left = e.clientX + 'px'; cur.style.top = e.clientY + 'px';
      S.setItem('demo-pos', JSON.stringify([e.clientX, e.clientY]));
    }, true);
    addEventListener('mousedown', (e) => {
      cur.classList.add('down');
      const r = document.createElement('div');
      r.className = 'demo-ripple';
      r.style.left = e.clientX + 'px'; r.style.top = e.clientY + 'px';
      document.body.appendChild(r);
      setTimeout(() => r.remove(), 700);
    }, true);
    addEventListener('mouseup', () => cur.classList.remove('down'), true);
  };

  window.__demoCaption = (html) => {
    S.setItem('demo-caption', html);
    const cap = document.getElementById('demo-caption');
    if (cap) cap.innerHTML = html;
  };
  window.__demoCard = (title, text) => {
    S.setItem('demo-card', JSON.stringify([title, text]));
    let el = document.getElementById('demo-card');
    if (!el) { el = document.createElement('div'); el.id = 'demo-card'; document.body.appendChild(el); }
    el.innerHTML = '<h1>' + title + '</h1>' + (text ? '<p>' + text + '</p>' : '');
  };
  window.__demoCardHide = () => {
    S.removeItem('demo-card');
    document.getElementById('demo-card')?.remove();
  };
  window.__demoBadge = (t) => {
    if (t) S.setItem('demo-badge', t); else S.removeItem('demo-badge');
    const el = document.getElementById('demo-badge');
    if (el) el.textContent = t || '';
  };
  window.__demoRing = (r) => {
    const el = document.getElementById('demo-ring');
    if (!el) return;
    if (!r) { el.style.opacity = '0'; return; }
    Object.assign(el.style, { left: (r.x - 6) + 'px', top: (r.y - 6) + 'px',
      width: (r.width + 12) + 'px', height: (r.height + 12) + 'px', opacity: '1' });
  };

  // Поддельный микрофон: вместо устройства — заданный WAV. Так в ролике
  // «говорит» диспетчер, а распознаёт его настоящий сервер голоса
  const md = navigator.mediaDevices;
  if (md && md.getUserMedia) {
    const orig = md.getUserMedia.bind(md);
    md.getUserMedia = async (c) => {
      const b64 = window.__demoMic;
      if (!c || !c.audio || !b64) return orig(c);
      const ctx = new AudioContext();
      const bytes = Uint8Array.from(atob(b64), (ch) => ch.charCodeAt(0));
      const buf = await ctx.decodeAudioData(bytes.buffer);
      const src = ctx.createBufferSource();
      src.buffer = buf;
      const dst = ctx.createMediaStreamDestination();
      src.connect(dst);
      src.start();
      return dst.stream;
    };
  }

  if (document.readyState === 'loading') addEventListener('DOMContentLoaded', install);
  else install();
})();`;

/** Событие звуковой дорожки: что и когда прозвучало. */
interface Sound { at: number; file: string; text: string; voice: string }

const wavSeconds = (buf: Buffer): number => {
  // Заголовок RIFF: байт/с — со смещения 28, размер данных — у «data»
  const byteRate = buf.readUInt32LE(28);
  const idx = buf.indexOf('data');
  const size = idx >= 0 ? buf.readUInt32LE(idx + 4) : buf.length - 44;
  return size / byteRate;
};

/** `[112|сто двенадцать]` → на экране «112», в голосе «сто двенадцать». */
const shown = (t: string): string => t.replace(/\[([^|\]]*)\|([^\]]*)\]/g, '$1');
const spoken = (t: string): string => t.replace(/\[([^|\]]*)\|([^\]]*)\]/g, '$2');
const html = (t: string, sub?: string): string =>
  sub ? `${shown(t)}<small>${sub}</small>` : shown(t);

export class Demo {
  private cdp!: CDPSession;
  private frames: { file: string; at: number }[] = [];
  private sounds: Sound[] = [];
  private n = 0;
  private pos: [number, number] = [960, 540];
  private windows: { from: number; to: number; factor: number }[] = [];

  constructor(public page: Page, public out: string) {
    fs.mkdirSync(path.join(out, 'frames'), { recursive: true });
    fs.mkdirSync(path.join(out, 'audio'), { recursive: true });
  }

  static async init(page: Page, out: string): Promise<Demo> {
    const d = new Demo(page, out);
    await page.addInitScript(OVERLAY);
    d.cdp = await page.context().newCDPSession(page);
    d.cdp.on('Page.screencastFrame', (f) => {
      const file = `${String(d.frames.length).padStart(6, '0')}.jpg`;
      fs.writeFileSync(path.join(out, 'frames', file), Buffer.from(f.data, 'base64'));
      d.frames.push({ file, at: f.metadata.timestamp ?? Date.now() / 1000 });
      void d.cdp.send('Page.screencastFrameAck', { sessionId: f.sessionId }).catch(() => {});
    });
    return d;
  }

  async startRecording(): Promise<void> {
    await this.cdp.send('Page.startScreencast', {
      format: 'jpeg', quality: 92, maxWidth: 1920, maxHeight: 1080, everyNthFrame: 1,
    });
  }

  async stopRecording(): Promise<void> {
    await this.cdp.send('Page.stopScreencast').catch(() => {});
    fs.writeFileSync(path.join(this.out, 'frames.json'), JSON.stringify(this.frames));
    fs.writeFileSync(path.join(this.out, 'sounds.json'), JSON.stringify(this.sounds, null, 1));
    fs.writeFileSync(path.join(this.out, 'speedup.json'), JSON.stringify(this.windows));
  }

  /** Синтезировать фразу; вернуть файл и длительность. */
  async voice(text: string, voice: string): Promise<{ file: string; sec: number }> {
    const plain = text.replace(/<[^>]+>/g, ' ').replace(/\s+/g, ' ').trim();
    const r = await fetch(`${NARRATOR}/tts`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text: plain, voice }),
    });
    if (!r.ok) throw new Error(`синтез «${plain}»: ${r.status} ${await r.text()}`);
    const buf = Buffer.from(await r.arrayBuffer());
    const file = path.join(this.out, 'audio', `${String(++this.n).padStart(3, '0')}-${voice}.wav`);
    fs.writeFileSync(file, buf);
    return { file, sec: wavSeconds(buf) };
  }

  /** Отметить звук в дорожке сейчас и дождаться, пока он договорится. */
  private async play(text: string, voice: string, waitFull = true): Promise<number> {
    const v = await this.voice(text, voice);
    this.sounds.push({ at: Date.now() / 1000, file: v.file, text, voice });
    if (waitFull) await this.page.waitForTimeout(v.sec * 1000 + 250);
    return v.sec;
  }

  /**
   * Титр с закадровым голосом. `sub` — мелкая строка под титром, без
   * озвучки. В титре пишем цифрами («112»), а читаем словами: для этого
   * `[показать|сказать]` — на экране первое, в голосе второе.
   */
  async say(text: string, sub?: string): Promise<void> {
    await this.page.evaluate((h) => window.__demoCaption(h), html(text, sub));
    await this.play(spoken(text), 'narrator');
  }

  /** Титр и голос идут, пока выполняется действие; ждём и то, и другое. */
  async sayWhile(text: string, action: () => Promise<unknown>, sub?: string): Promise<void> {
    await this.page.evaluate((h) => window.__demoCaption(h), html(text, sub));
    const v = await this.voice(spoken(text), 'narrator');
    const started = Date.now();
    this.sounds.push({ at: started / 1000, file: v.file, text, voice: 'narrator' });
    await action();
    const left = v.sec * 1000 + 250 - (Date.now() - started);
    if (left > 0) await this.page.waitForTimeout(left);
  }

  /** Реплика собеседника в приложении (старший, начальник): в дорожку. */
  async app(text: string, voice: string): Promise<void> {
    await this.play(text, voice);
  }

  async caption(html: string): Promise<void> {
    await this.page.evaluate((h) => window.__demoCaption(h), html);
  }

  async card(title: string, text = ''): Promise<void> {
    await this.page.evaluate(([t, s]) => window.__demoCard(t, s), [title, text]);
  }

  async hideCard(): Promise<void> {
    await this.page.evaluate(() => window.__demoCardHide());
  }

  async ring(target: Locator | null): Promise<void> {
    const box = target ? await target.boundingBox() : null;
    await this.page.evaluate((r) => window.__demoRing(r), box);
  }

  /** Плавно подвести курсор к элементу. */
  async moveTo(target: Locator): Promise<void> {
    await target.scrollIntoViewIfNeeded();
    const box = await target.boundingBox();
    if (!box) return;
    const x = box.x + Math.min(box.width / 2, 60);
    const y = box.y + box.height / 2;
    const dist = Math.hypot(x - this.pos[0], y - this.pos[1]);
    await this.page.mouse.move(x, y, { steps: Math.max(12, Math.min(45, Math.round(dist / 18))) });
    this.pos = [x, y];
    await this.page.waitForTimeout(180);
  }

  async click(target: Locator): Promise<void> {
    await this.moveTo(target);
    await target.click();
    await this.page.waitForTimeout(350);
  }

  /** Набрать текст по буквам, как человек. */
  async type(target: Locator, text: string, delay = 45): Promise<void> {
    await this.moveTo(target);
    await target.click();
    // Поле может быть уже заполнено (форма входа помнит логин)
    await target.fill('');
    await target.pressSequentially(text, { delay });
    await this.page.waitForTimeout(250);
  }

  async select(target: Locator, value: string | { label: string }): Promise<void> {
    await this.moveTo(target);
    await target.selectOption(value);
    await this.page.waitForTimeout(350);
  }

  /** Микрофон «скажет» эту фразу голосом диспетчера при следующей записи. */
  async mic(text: string, voice = 'operator'): Promise<number> {
    const v = await this.voice(text, voice);
    const b64 = fs.readFileSync(v.file).toString('base64');
    await this.page.evaluate((b) => { window.__demoMic = b; }, b64);
    this.pendingMic = { ...v, text, voice };
    return v.sec;
  }

  private pendingMic: { file: string; sec: number; text: string; voice: string } | null = null;

  /** Отметить в дорожке, что диспетчер начал говорить в микрофон. */
  micStarted(): number {
    const m = this.pendingMic;
    if (!m) return 0;
    this.sounds.push({ at: Date.now() / 1000, file: m.file, text: m.text, voice: m.voice });
    this.pendingMic = null;
    return m.sec;
  }

  /**
   * Ожидание модели. На процессоре оно длинное, и в ролике его можно
   * ускорить (DEMO_SPEEDUP=8): отрезок помечается на экране плашкой
   * «ускорено», а сборка сжимает его во столько раз. Честно: зритель
   * видит, что время сжато и во сколько раз.
   */
  async waitModel(action: () => Promise<unknown>): Promise<void> {
    const factor = Number(process.env.DEMO_SPEEDUP ?? '1');
    if (!(factor > 1)) { await action(); return; }
    await this.page.evaluate((t) => window.__demoBadge(t),
      `⏩ ожидание модели на процессоре — в ролике ускорено ×${factor}`);
    const from = Date.now() / 1000;
    try {
      await action();
    } finally {
      this.windows.push({ from, to: Date.now() / 1000, factor });
      await this.page.evaluate(() => window.__demoBadge(''));
    }
  }

  async pause(ms: number): Promise<void> {
    await this.page.waitForTimeout(ms);
  }
}

declare global {
  interface Window {
    __demoCaption: (html: string) => void;
    __demoCard: (title: string, text: string) => void;
    __demoCardHide: () => void;
    __demoBadge: (t: string) => void;
    __demoRing: (r: { x: number; y: number; width: number; height: number } | null) => void;
    __demoMic?: string;
  }
}
