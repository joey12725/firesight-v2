// Renders the raster brand assets from the app's own mark (public/icon.svg): PNG icons
// for platforms that do not take SVG, and the 1200x630 social card. Run by hand when the
// mark changes: `node scripts/brand-assets.mjs`. The outputs are committed.
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const pub = resolve(dirname(fileURLToPath(import.meta.url)), '../public');
const icon = readFileSync(`${pub}/icon.svg`, 'utf8');
const maskable = readFileSync(`${pub}/icon-maskable.svg`, 'utf8');
const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());

async function render(html, width, height, file, transparent = true) {
  const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 1 });
  await page.setContent(`<!doctype html><html><body style="margin:0;${transparent ? 'background:transparent' : ''}">${html}</body></html>`);
  await page.screenshot({ path: `${pub}/${file}`, omitBackground: transparent, clip: { x: 0, y: 0, width, height } });
  await page.close();
}
const sized = (svg, size) => svg.replace('<svg ', `<svg width="${size}" height="${size}" style="display:block" `);

await render(sized(icon, 32), 32, 32, 'favicon-32.png');
await render(sized(icon, 192), 192, 192, 'icon-192.png');
await render(sized(icon, 512), 512, 512, 'icon-512.png');
await render(sized(maskable, 512), 512, 512, 'icon-maskable-512.png', false);
// iOS rounds the corners itself and shows transparency as black: a full-bleed square.
await render(sized(maskable.replace('scale(.72)', 'scale(.84)'), 180), 180, 180, 'apple-touch-icon.png', false);

// The social card: the mark, the question the product answers, and the mark's two curves drawn large.
await render(
  `<div style="width:1200px;height:630px;background:#1b1a17;color:#f7f6f3;font-family:Geist,'Helvetica Neue',Helvetica,Arial,sans-serif;position:relative;overflow:hidden">
    <svg viewBox="0 0 1200 630" width="1200" height="630" style="position:absolute;inset:0">
      <path d="M520 560 C760 540 880 430 960 300 C1030 190 1100 130 1210 110" fill="none" stroke="#f2733b" stroke-width="26" stroke-linecap="round"/>
      <path d="M520 560 C780 560 980 520 1210 400" fill="none" stroke="#f2733b" stroke-opacity=".4" stroke-width="26" stroke-linecap="round"/>
      <circle cx="520" cy="560" r="22" fill="#f7f6f3"/>
    </svg>
    <div style="position:absolute;left:80px;top:72px;display:flex;align-items:center;gap:18px">
      ${sized(icon, 56)}
      <span style="font-size:34px;font-weight:600;letter-spacing:-0.02em">FIRESight</span>
    </div>
    <div style="position:absolute;left:80px;top:196px;width:760px;font-size:68px;line-height:1.08;font-weight:600;letter-spacing:-0.03em">When can work become optional, and how sure is that?</div>
    <div style="position:absolute;left:80px;bottom:76px;width:420px;font-size:26px;line-height:1.35;color:#b9b4a9">Thousands of simulated financial lives, run in your browser.</div>
  </div>`,
  1200,
  630,
  'social-card.png',
  false,
);
await browser.close();
console.log('brand assets written to', pub);
