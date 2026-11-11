// Builds and renders the film (film.html + app.css + app.js + film.js) with headless Chromium.
//   bun render.mjs --stills 1,2.5,9      PNGs to look at (.tmp/promo/stills); --no-cursor leaves the pointer out (posters)
//   bun render.mjs --cues                cues.json: the picture events that get a sound, and the music to put under them
//   --timing <name> (or TIMING=<name>)   cut the film to timing/<name>.json instead of the default, be-happy
//   --lang en (or FILM_LANG=en)          the English film (copy.js); Chinese by default
//   bun render.mjs --frames              every frame, --sub samples each (motion blur), over --workers browsers
// Run it from .tmp/promo (it needs playwright-core there); CHROME points at a Chromium binary.
// Optional, private, never committed: chars.json (character art, made by chars.tsx) and icons/appicon.png.
import { chromium } from 'playwright-core';
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
let repo = here; while (!existsSync(resolve(repo, 'product.json'))) repo = dirname(repo);
const src = resolve(repo, 'promo'), out = resolve(repo, '.tmp/promo');
const arg = (k, d) => { const i = process.argv.indexOf('--' + k); return i < 0 ? d : (process.argv[i + 1] ?? true); };
const fps = +arg('fps', 60), workers = +arg('workers', 4), sub = +arg('sub', 4), shutter = +arg('shutter', 0.5);

const name = JSON.parse(readFileSync(resolve(repo, 'product.json'), 'utf8')).name;
mkdirSync(out, { recursive: true });
const chars = existsSync(resolve(out, 'chars.json')) ? readFileSync(resolve(out, 'chars.json'), 'utf8') : '{}';
// which track the film is cut to: timing/be-happy.json, or the one --timing <name> or TIMING=<name> picks
const timing = readFileSync(resolve(src, 'timing', arg('timing', process.env.TIMING || 'be-happy') + '.json'), 'utf8');
const lang = arg('lang', process.env.FILM_LANG || 'zh');
if (!['zh', 'en'].includes(lang)) throw new Error(`unknown language: ${lang}`);
const html = readFileSync(resolve(src, 'film.html'), 'utf8')
  .replace(/\/\*@include ([\w.]+)\*\//g, (_, f) => readFileSync(resolve(src, f), 'utf8'))
  .replaceAll('{{PRODUCT_NAME}}', name).replaceAll('{{LANG}}', lang).replaceAll('{{HTML_LANG}}', lang === 'zh' ? 'zh-CN' : 'en').replace('/*CHARS*/{}', () => chars).replace('/*TIMING*/null', () => timing);
const built = resolve(out, 'film.html');
writeFileSync(built, html);

async function open() {
  const browser = await chromium.launch({ executablePath: process.env.CHROME, args: ['--disable-gpu', '--force-color-profile=srgb', '--hide-scrollbars', '--font-render-hinting=none', '--allow-file-access-from-files'] });
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
  page.on('pageerror', e => console.error('PAGE ERROR', e.message));
  page.on('console', m => m.type() === 'error' && console.error('CONSOLE', m.text()));
  await page.goto('file://' + built + '#render');
  await page.evaluate(() => window.ready);
  const cdp = await page.context().newCDPSession(page);
  const shot = async (t, file, format = 'png') => {
    // a capture sometimes never returns and the page stays stuck: report it so the caller reopens the browser
    const work = (async () => {
      await page.evaluate(t => window.seek(t), t);
      return cdp.send('Page.captureScreenshot', format === 'png' ? { format, optimizeForSpeed: true } : { format, quality: 95, optimizeForSpeed: true });
    })().catch(() => null);
    const r = await Promise.race([work, new Promise(ok => setTimeout(ok, 8000))]);
    if (!r) return false;
    writeFileSync(file, Buffer.from(r.data, 'base64'));
    return true;
  };
  return { browser, page, shot };
}

if (arg('cues')) {
  const { page } = await open();
  writeFileSync(resolve(out, 'cues.json'), JSON.stringify(await page.evaluate(() => ({ duration: window.DURATION, cues: window.CUES, music: window.MUSIC }))));
} else if (arg('stills')) {
  const dir = resolve(out, 'stills'); mkdirSync(dir, { recursive: true });
  const { page, shot } = await open();
  if (arg('no-cursor')) await page.addStyleTag({ content: '#cursor{display:none!important}' });   // a poster frame: no pointer
  for (const t of String(arg('stills')).split(',').map(Number)) await shot(t, resolve(dir, `t${t.toFixed(2).padStart(5, '0')}.png`));
} else {
  // frame f, sample k is the picture at f/fps + k * shutter / (sub * fps); ffmpeg averages each frame's samples
  const dir = resolve(out, 'frames'); mkdirSync(dir, { recursive: true });
  const { browser, page } = await open(); const total = Math.round((await page.evaluate(() => window.DURATION)) * fps) * sub; await browser.close();
  const from = Math.round(+arg('from', 0) * fps) * sub, to = arg('to') ? Math.round(+arg('to') * fps) * sub : total;
  let next = from, done = 0; const t0 = Date.now();
  await Promise.all(Array.from({ length: workers }, async () => {
    let { browser, shot } = await open();
    while (next < to) {
      const i = next++, t = Math.floor(i / sub) / fps + (i % sub) * shutter / (sub * fps);
      while (!(await shot(t, resolve(dir, String(i).padStart(6, '0') + '.jpg'), 'jpeg'))) {
        browser.close().catch(() => {}); ({ browser, shot } = await open());
      }
      if (++done % 2000 === 0) console.log(`${done}/${to - from} samples, ${((Date.now() - t0) / 1000).toFixed(0)}s`);
    }
    browser.close().catch(() => {});
  }));
  console.log(`done: ${done} samples in ${((Date.now() - t0) / 1000).toFixed(0)}s`);
}
process.exit(0);   // a stuck browser that was replaced never closes
