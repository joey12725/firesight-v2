// README animations, written to docs/images at the repository root:
//   npm run build && npx vite preview --port 4173 &
//   BASE_URL=http://localhost:4173 node e2e/docs-gifs.mjs [tour,live-edit]
//
// Each clip is recorded with the Chrome DevTools screencast (lossless PNG frames with their
// real timestamps), then turned into a GIF with ffmpeg using a palette made for that clip.
// Needs ffmpeg on the PATH. Every figure on screen is real engine output for a demo
// household; nothing is scripted except where the pointer goes.
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:4173';
const out = resolve(dirname(fileURLToPath(import.meta.url)), '../../docs/images');
mkdirSync(out, { recursive: true });
const only = process.argv[2]?.split(',');
const want = (name) => !only || only.includes(name);
const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];

const settled = (page) =>
  page.waitForFunction(() => {
    const r = window.__firesight.run.getState();
    return r.status === 'done' && !r.stale;
  }, null, { timeout: 240_000 });

async function context({ width = 1200, height = 760, editor = false } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: 'light', deviceScaleFactor: 1 });
  // Start with the panels the clip wants, so the first frame is already laid out.
  await ctx.addInitScript((editor) => {
    localStorage.setItem('firesight.settings', JSON.stringify({ state: { theme: 'light', editorOpen: editor, assistantOpen: false }, version: 1 }));
  }, editor);
  const page = await ctx.newPage();
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  return { ctx, page };
}

/** Records whatever `act` does to the page and writes `<name>.gif`. */
async function record(page, name, act, { fps = 12, width, hold = 2500 } = {}) {
  const cdp = await page.context().newCDPSession(page);
  const frames = [];
  cdp.on('Page.screencastFrame', async ({ data, metadata, sessionId }) => {
    frames.push({ data, t: metadata.timestamp });
    await cdp.send('Page.screencastFrameAck', { sessionId }).catch(() => {});
  });
  const { width: vw, height: vh } = page.viewportSize();
  await cdp.send('Page.startScreencast', { format: 'png', everyNthFrame: 1, maxWidth: vw, maxHeight: vh });
  const started = Date.now() / 1000;
  await act();
  await page.waitForTimeout(300);
  await cdp.send('Page.stopScreencast');
  const ended = Date.now() / 1000 + hold / 1000;

  const dir = mkdtempSync(join(tmpdir(), `firesight-${name}-`));
  try {
    // The screencast only sends a frame when the page repaints, so each frame is held until the next.
    const list = [];
    frames.forEach((f, i) => {
      const file = join(dir, `${String(i).padStart(5, '0')}.png`);
      writeFileSync(file, Buffer.from(f.data, 'base64'));
      const next = i + 1 < frames.length ? frames[i + 1].t : ended;
      const from = i === 0 ? Math.min(f.t, started) : f.t;
      list.push(`file '${file}'`, `duration ${Math.max(0.001, next - from).toFixed(3)}`);
    });
    list.push(`file '${join(dir, `${String(frames.length - 1).padStart(5, '0')}.png`)}'`);
    writeFileSync(join(dir, 'list.txt'), list.join('\n'));
    const scale = width ? `scale=${width}:-1:flags=lanczos,` : '';
    const gif = join(out, `${name}.gif`);
    execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i', join(dir, 'list.txt'),
      '-vf', `fps=${fps},${scale}split[a][b];[a]palettegen=max_colors=192:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle`,
      '-loop', '0', gif]);
    const kb = Math.round(execFileSync('stat', ['-f', '%z', gif]).toString() / 1024);
    console.log(`  ${name}.gif  ${frames.length} frames, ${(ended - started).toFixed(1)} s, ${kb} KB`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

// Each view's lead sentence appears once its charts have something to draw.
const LEADS = { Overview: 'outcome-sentence', Outcomes: 'range-sentence', 'Cash flow': 'cash-flow-lead', 'Life stories': 'life-headline' };
const tab = async (page, view, wait = 2200) => {
  await page.getByRole('tab', { name: view, exact: true }).click();
  await page.getByTestId(LEADS[view]).waitFor({ timeout: 60_000 });
  await page.waitForTimeout(wait);
};

try {
  if (want('tour')) {
    // From the front door to a finished plan: open a demo, let a hundred thousand lives arrive, walk the views.
    const { ctx, page } = await context();
    await page.goto(base);
    await page.getByTestId('demo-dual-income-family').waitFor();
    await page.waitForTimeout(500);
    await record(page, 'tour', async () => {
      await page.waitForTimeout(1500);
      await page.getByTestId('demo-dual-income-family').hover();
      await page.waitForTimeout(500);
      await page.getByTestId('demo-dual-income-family').click();
      await settled(page);
      await page.waitForTimeout(1500);
      await page.getByLabel('Lives to simulate').getByText('100k', { exact: true }).click();
      await settled(page);
      await page.waitForTimeout(1500);
      await page.locator('[data-results-scroll]').evaluate((el) => el.scrollBy({ top: 520, behavior: 'smooth' }));
      await page.waitForTimeout(2200);
      await tab(page, 'Outcomes');
      await tab(page, 'Cash flow');
      await tab(page, 'Life stories', 2800);
      await tab(page, 'Overview', 600);
    }, { hold: 1500 });
    await ctx.close();
  }

  if (want('live-edit')) {
    // Drag one decision and watch the answer move: the withdrawal rate that counts as "enough".
    const { ctx, page } = await context({ editor: true });
    await page.goto(base);
    await page.getByTestId('demo-dual-income-family').click();
    await settled(page);
    await page.getByRole('button', { name: /Retirement plan/ }).first().click();
    const slider = page.getByRole('slider', { name: 'Withdrawal rate that counts as funded' });
    await slider.scrollIntoViewIfNeeded();
    await slider.evaluate((el) => el.closest('.scroll-thin, [class*="overflow-y"]')?.scrollBy({ top: 120 }));
    await page.waitForTimeout(800);
    await record(page, 'live-edit', async () => {
      await page.waitForTimeout(1200);
      await slider.focus();
      for (const key of ['ArrowRight', 'ArrowRight', 'ArrowRight', 'ArrowRight', 'ArrowLeft', 'ArrowLeft', 'ArrowLeft', 'ArrowLeft', 'ArrowLeft', 'ArrowLeft']) {
        await page.keyboard.press(key);
        await page.waitForTimeout(250);
        await settled(page);
        await page.waitForTimeout(900);
      }
      await page.keyboard.press('ArrowRight');
      await page.keyboard.press('ArrowRight');
      await settled(page);
    }, { hold: 2000 });
    await ctx.close();
  }
} catch (error) {
  problems.push(error instanceof Error ? (error.stack ?? error.message) : String(error));
}
await browser.close();
if (problems.length) {
  console.log('Problems:\n' + [...new Set(problems)].join('\n'));
  process.exitCode = 1;
}
