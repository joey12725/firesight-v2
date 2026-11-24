// Screenshots of the Overview for looking at, not asserting on:
//   BASE_URL=http://localhost:5173 node e2e/shots.mjs [slug,slug] [viewport,viewport] [light,dark]
// Writes e2e/shots/overview-<slug>-<viewport>-<theme>.png (the fold) and -full.png (the
// whole view, by growing the window so the results column does not scroll).
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5173';
const shots = resolve(dirname(fileURLToPath(import.meta.url)), 'shots');
mkdirSync(shots, { recursive: true });
const VIEWPORTS = { desktop: [1440, 900], laptop: [1024, 768], phone: [390, 844] };
const slugs = (process.argv[2] ?? 'dual-income-family,early-career-saver,side-hustle-landlord,full-life').split(',');
const viewports = (process.argv[3] ?? 'desktop,laptop,phone').split(',');
const themes = (process.argv[4] ?? 'light,dark').split(',');
const paths = Number(process.env.PATHS ?? 10000);

const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];
for (const theme of themes) {
  for (const vp of viewports) {
    const [width, height] = VIEWPORTS[vp];
    const context = await browser.newContext({ viewport: { width, height }, colorScheme: theme, deviceScaleFactor: vp === 'phone' ? 2 : 1 });
    for (const slug of slugs) {
      const page = await context.newPage();
      page.on('console', (m) => m.type() === 'error' && !/status of 404/.test(m.text()) && problems.push(`${slug} ${vp} ${theme} console: ${m.text()}`));
      page.on('pageerror', (e) => problems.push(`${slug} ${vp} ${theme} pageerror: ${e.message}`));
      await page.goto(base);
      await page.getByTestId(`demo-${slug}`).click();
      await page.waitForFunction((n) => {
        const fs = window.__firesight;
        if (fs.plan.getState().run.paths !== n) fs.plan.getState().setRun({ paths: n });
        const run = fs.run.getState();
        return run.status === 'done' && run.total === n;
      }, paths, { timeout: 180_000 });
      await page.waitForTimeout(700);
      const name = `${shots}/overview-${slug}-${vp}-${theme}`;
      await page.screenshot({ path: `${name}.png` });
      await page.setViewportSize({ width, height: 3400 });
      await page.waitForTimeout(400);
      await page.screenshot({ path: `${name}-full.png` });
      await page.setViewportSize({ width, height });
      console.log(`  shot ${slug} ${vp} ${theme}`);
      // Start the next household from the landing page.
      await page.evaluate(() => indexedDB.databases?.().then((dbs) => dbs.forEach((d) => d.name && indexedDB.deleteDatabase(d.name))));
      await page.evaluate(() => localStorage.clear());
      await page.close();
    }
    await context.close();
  }
}
await browser.close();
if (problems.length) {
  console.log('Problems:\n' + problems.join('\n'));
  process.exitCode = 1;
}
