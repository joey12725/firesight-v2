// Screenshots and checks of the Outcomes and Assumptions views, for looking at:
//   BASE_URL=http://localhost:5181 node e2e/outcomes.mjs [slugs] [viewports] [themes] [views] [variants]
// Variants: asis (the demo as shipped), historical (market model switched to history as
// it happened, which adds the cohort chart), triangulate (presses the four-models
// button and waits), fixedage (retire at a fixed age, so the two age plots differ).
// Writes e2e/shots/<view>-<slug>-<viewport>-<theme>[-variant].png (whole view).
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5181';
const shots = resolve(dirname(fileURLToPath(import.meta.url)), 'shots');
mkdirSync(shots, { recursive: true });
const VIEWPORTS = { desktop: [1440, 900], laptop: [1024, 768], phone: [390, 844] };
const list = (i, fallback) => (process.argv[i] && process.argv[i] !== '-' ? process.argv[i] : fallback).split(',');
const slugs = list(2, 'dual-income-family,early-career-saver,side-hustle-landlord');
const viewports = list(3, 'desktop,laptop,phone');
const themes = list(4, 'light,dark');
const views = list(5, 'outcomes,assumptions');
const variants = list(6, 'asis');
const paths = Number(process.env.PATHS ?? 10000);

const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];

const runDone = (page, n) =>
  page.waitForFunction(
    (count) => {
      const fs = window.__firesight;
      if (fs.plan.getState().run.paths !== count) fs.plan.getState().setRun({ paths: count });
      const run = fs.run.getState();
      return run.status === 'done' && run.total === count && !run.stale;
    },
    n,
    { timeout: 240_000 },
  );

for (const theme of themes) {
  for (const vp of viewports) {
    const [width, height] = VIEWPORTS[vp];
    const context = await browser.newContext({ viewport: { width, height }, colorScheme: theme, deviceScaleFactor: vp === 'phone' ? 2 : 1 });
    for (const slug of slugs) {
      for (const variant of variants) {
        const page = await context.newPage();
        const tag = `${slug} ${vp} ${theme} ${variant}`;
        page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !/status of 404/.test(m.text()) && problems.push(`${tag} console ${m.type()}: ${m.text()}`));
        page.on('pageerror', (e) => problems.push(`${tag} pageerror: ${e.message}`));
        await page.goto(base);
        await page.getByTestId(`demo-${slug}`).click();
        await runDone(page, paths);
        if (variant === 'historical') {
          await page.evaluate(() => window.__firesight.plan.getState().edit([{ op: 'set', path: '/assumptions/market/kind', value: { type: 'historical' } }]));
          await page.waitForTimeout(300);
          await runDone(page, paths);
        }
        if (variant === 'fixedage') {
          await page.evaluate(() => window.__firesight.plan.getState().edit([{ op: 'set', path: '/retirement/trigger', value: { type: 'at_age', age: 60 } }]));
          await page.waitForTimeout(300);
          await runDone(page, paths);
        }
        for (const view of views) {
          await page.goto(`${base}/${view}`);
          await runDone(page, paths);
          await page.waitForTimeout(900);
          if (variant === 'historical' && view === 'outcomes') await page.waitForFunction(() => !document.body.innerText.includes('Tracing'), null, { timeout: 60_000 }).catch(() => problems.push(`${tag}: cohort traces did not finish`));
          if (variant === 'triangulate' && view === 'outcomes') {
            await page.getByTestId('triangulate').click();
            await page.waitForFunction(() => document.querySelector('[data-testid="triangulate"]')?.textContent?.includes('Run again'), null, { timeout: 120_000 });
            await page.waitForTimeout(400);
          }
          const name = `${shots}/${view}-${slug}-${vp}-${theme}${variant === 'asis' ? '' : `-${variant}`}`;
          // Grow the window so the results column does not scroll, then capture all of it.
          const tall = await page.evaluate(() => {
            const scroller = [...document.querySelectorAll('*')].filter((e) => e.scrollHeight > e.clientHeight + 40 && getComputedStyle(e).overflowY !== 'visible').sort((a, b) => b.scrollHeight - a.scrollHeight)[0];
            return scroller ? scroller.scrollHeight + 260 : document.documentElement.scrollHeight;
          });
          await page.setViewportSize({ width, height: Math.min(12000, Math.max(height, tall)) });
          await page.waitForTimeout(600);
          await page.screenshot({ path: `${name}.png` });
          const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
          if (overflow > 1) problems.push(`${tag} ${view}: page overflows horizontally by ${overflow}px`);
          await page.setViewportSize({ width, height });
          console.log(`  shot ${view} ${tag}`);
        }
        await page.evaluate(() => indexedDB.databases?.().then((dbs) => dbs.forEach((d) => d.name && indexedDB.deleteDatabase(d.name))));
        await page.evaluate(() => localStorage.clear());
        await page.close();
      }
    }
    await context.close();
  }
}
await browser.close();
if (problems.length) {
  console.log('Problems:\n' + [...new Set(problems)].join('\n'));
  process.exitCode = 1;
}
