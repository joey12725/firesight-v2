// Screenshots of the Cash flow and Life stories views for looking at, not asserting on:
//   BASE_URL=http://localhost:5182 node e2e/cashflow.mjs [view,view] [slug,slug] [viewport,viewport] [light,dark]
// Views: cash-flow, life-stories. Writes e2e/shots/<view>-<slug>-<viewport>-<theme>.png
// (the fold) and -full.png (the whole view). LIFE=<pick> opens that pick in Life stories
// (p10, median, p90, failing, median_fi); YEAR=<n> also opens the year inspector n years in.
// PANEL=closed closes the assistant panel first, for the widest layout.
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5182';
const shots = resolve(dirname(fileURLToPath(import.meta.url)), 'shots');
mkdirSync(shots, { recursive: true });
const VIEWPORTS = { desktop: [1440, 900], laptop: [1024, 768], phone: [390, 844] };
const views = (process.argv[2] ?? 'cash-flow,life-stories').split(',');
const slugs = (process.argv[3] ?? 'dual-income-family,early-career-saver,side-hustle-landlord').split(',');
const viewports = (process.argv[4] ?? 'desktop,laptop,phone').split(',');
const themes = (process.argv[5] ?? 'light,dark').split(',');
const paths = Number(process.env.PATHS ?? 10000);
const life = process.env.LIFE ?? '';
const year = process.env.YEAR ? Number(process.env.YEAR) : null;
const fullHeight = Number(process.env.FULL_HEIGHT ?? 5200);

const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];
for (const theme of themes) {
  for (const vp of viewports) {
    const [width, height] = VIEWPORTS[vp];
    const context = await browser.newContext({ viewport: { width, height }, colorScheme: theme, deviceScaleFactor: vp === 'phone' ? 2 : 1 });
    for (const slug of slugs) {
      const page = await context.newPage();
      page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !/status of 404|Download the React DevTools/.test(m.text()) && problems.push(`${slug} ${vp} ${theme} console ${m.type()}: ${m.text()}`));
      page.on('pageerror', (e) => problems.push(`${slug} ${vp} ${theme} pageerror: ${e.message}`));
      await page.goto(base);
      await page.getByTestId(`demo-${slug}`).click();
      await page.waitForFunction(
        (n) => {
          const fs = window.__firesight;
          if (fs.plan.getState().run.paths !== n) fs.plan.getState().setRun({ paths: n });
          const run = fs.run.getState();
          return run.status === 'done' && run.total === n;
        },
        paths,
        { timeout: 180_000 },
      );
      if (process.env.PANEL === 'closed') await page.getByRole('button', { name: 'Close the assistant' }).click().catch(() => {});
      for (const view of views) {
        let query = '';
        if (view === 'life-stories' && life) {
          const index = await page.evaluate((label) => window.__firesight.run.getState().summary?.picks.find((p) => p.label === label)?.path_index ?? null, life);
          if (index !== null) query = `?path=${index}&why=${life}`;
        }
        await page.evaluate((url) => {
          window.history.pushState(null, '', url);
          window.dispatchEvent(new PopStateEvent('popstate'));
        }, `/${view}${query}`);
        await page.waitForTimeout(view === 'life-stories' ? 1500 : 900);
        if (view === 'life-stories' && year !== null) {
          const cursor = page.locator('[data-chart-card^="life-"] [data-chart-cursor]').first();
          const box = await cursor.boundingBox();
          if (box) await page.mouse.click(box.x + (box.width * year) / 60, box.y + box.height / 2);
          await page.waitForTimeout(500);
        }
        const name = `${shots}/${view}-${slug}${life ? `-${life}` : ''}-${vp}-${theme}`;
        await page.screenshot({ path: `${name}.png` });
        await page.setViewportSize({ width, height: view === 'life-stories' ? fullHeight + 1200 : fullHeight });
        await page.waitForTimeout(500);
        await page.screenshot({ path: `${name}-full.png` });
        await page.setViewportSize({ width, height });
        console.log(`  shot ${view} ${slug} ${vp} ${theme}`);
      }
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
  console.log('Problems:\n' + [...new Set(problems)].join('\n'));
  process.exitCode = 1;
}
