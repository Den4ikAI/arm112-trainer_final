// Проверка презентации финала: снимает каждый слайд финал.html в PNG 1920×1080,
// ищет переполнения (элементы за краем слайда, текст, не влезающий в блок)
// и печатает PDF — слайд на страницу.
//
// NODE_PATH=frontend/e2e/node_modules node docs/presentation/check_final.cjs <каталог для PNG> [файл PDF]
const { chromium } = require('playwright');
const path = require('path');
const HTML = path.join(__dirname, 'финал.html');
const OUT = process.argv[2] || path.join(__dirname, 'screens', 'final-check');
const PDF = process.argv[3];
(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
  await page.goto('file://' + HTML, { waitUntil: 'load' });
  await page.waitForTimeout(500);
  const n = await page.evaluate(() => document.querySelectorAll('.slide').length);
  for (let i = 0; i < n; i++) {
    await page.evaluate((i) => window.__show(i), i);
    await page.waitForTimeout(2200);
    const over = await page.evaluate(() => {
      const s = document.querySelector('.slide.on');
      const r0 = s.getBoundingClientRect();
      const k = r0.width / 1920;
      const bad = [];
      s.querySelectorAll('*').forEach((el) => {
        const r = el.getBoundingClientRect();
        if (r.width === 0) return;
        if (r.right > r0.right + 1 || r.bottom > r0.bottom + 1) bad.push(el.className + ':' + Math.round((r.right - r0.left) / k) + 'x' + Math.round((r.bottom - r0.top) / k));
      });
      s.querySelectorAll('.panel, .step, .box, .c, .t, .v, .stat, .fact, .card').forEach((el) => {
        if (el.scrollHeight > el.clientHeight + 2) bad.push('overflow:' + el.className + ':' + el.scrollHeight + '>' + el.clientHeight);
      });
      return bad.slice(0, 12);
    });
    await page.screenshot({ path: path.join(OUT, `s${String(i + 1).padStart(2, '0')}.png`) });
    console.log('slide', i + 1, over.length ? over.join(' | ') : 'ok');
  }
  if (PDF) {
    await page.emulateMedia({ media: 'print' });
    await page.pdf({ path: PDF, width: '1920px', height: '1080px', printBackground: true, margin: { top: 0, right: 0, bottom: 0, left: 0 } });
    console.log('pdf', PDF);
  }
  await browser.close();
})().catch((e) => { console.error(e); process.exit(1); });
