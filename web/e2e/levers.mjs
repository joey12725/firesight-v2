// Screenshots and checks of the What matters view, for looking at:
//   BASE_URL=http://localhost:5183 node e2e/levers.mjs [slug,slug] [viewport,viewport] [light,dark] [quick]
// Writes e2e/shots/levers-<slug>-<viewport>-<theme>-full.png plus interaction shots for the first household.
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5183';
const shots = resolve(dirname(fileURLToPath(import.meta.url)), 'shots');
mkdirSync(shots, { recursive: true });
const VIEWPORTS = { desktop: [1440, 900], laptop: [1024, 768], phone: [390, 844] };
const slugs = (process.argv[2] ?? 'dual-income-family,early-career-saver,side-hustle-landlord').split(',');
const viewports = (process.argv[3] ?? 'desktop,laptop,phone').split(',');
const themes = (process.argv[4] ?? 'light,dark').split(',');
const interactions = process.argv[5] !== 'quick';

const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];
const timings = [];
for (const theme of themes) {
  for (const vp of viewports) {
    const [width, height] = VIEWPORTS[vp];
    const context = await browser.newContext({ viewport: { width, height }, colorScheme: theme, deviceScaleFactor: vp === 'phone' ? 2 : 1 });
    for (const slug of slugs) {
      const page = await context.newPage();
      page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !/status of 404/.test(m.text()) && problems.push(`${slug} ${vp} ${theme} console ${m.type()}: ${m.text()}`));
      page.on('pageerror', (e) => problems.push(`${slug} ${vp} ${theme} pageerror: ${e.message}`));
      await page.goto(base);
      await page.getByTestId(`demo-${slug}`).click();
      await page.waitForFunction(() => window.__firesight.run.getState().status === 'done', null, { timeout: 180_000 });
      const t0 = Date.now();
      await page.getByRole('tab', { name: 'What matters' }).click();
      await page.getByTestId('lead-sentence').waitFor({ timeout: 60_000 });
      const first = Date.now() - t0;
      // Tall window so the whole view is laid out (and the map scrolls into view and starts).
      await page.setViewportSize({ width, height: 4200 });
      await page.waitForFunction(() => !/Sharpening|Early picture|Simulating|Updating/.test(document.querySelector('[role=tabpanel]')?.textContent ?? ''), null, { timeout: 240_000 });
      const settled = Date.now() - t0;
      timings.push(`${slug} ${vp} ${theme}: first tornado ${first} ms, all sweeps settled ${settled} ms`);
      await page.waitForTimeout(500);
      const name = `${shots}/levers-${slug}-${vp}-${theme}`;
      if (interactions && slug === slugs[0]) {
        // Solver: default goal and variable.
        await page.getByRole('button', { name: 'Find it' }).click();
        await page.waitForFunction(() => /Find it/.test(document.querySelector('[data-testid=solver-form]')?.textContent ?? '') && document.querySelector('[data-chart-card=find-the-number] [data-chart-frame]'), null, { timeout: 120_000 });
        // Select the first tornado row and a map cell.
        await page.locator('[data-lever-bar]').first().click();
        const cursor = page.locator('[data-chart-card=trade-off-map] [data-chart-cursor]');
        const box = await cursor.boundingBox();
        if (box) await page.mouse.click(box.x + box.width * 0.3, box.y + box.height * 0.3);
        await page.waitForTimeout(400);
      }
      await page.screenshot({ path: `${name}-full.png` });
      if (interactions && slug === slugs[0] && vp === 'desktop') {
        for (const measure of ['Runs short', 'Work optional', 'Wealth at the end', 'Spending delivered']) {
          await page.getByRole('radio', { name: measure }).click();
          await page.waitForTimeout(700);
          await page.screenshot({ path: `${name}-${measure.toLowerCase().replace(/ /g, '-')}.png` });
        }
      }
      await page.setViewportSize({ width, height });
      console.log(`  shot ${slug} ${vp} ${theme}`);
      await page.evaluate(() => indexedDB.databases?.().then((dbs) => dbs.forEach((d) => d.name && indexedDB.deleteDatabase(d.name))));
      await page.evaluate(() => localStorage.clear());
      await page.close();
    }
    await context.close();
  }
}
await browser.close();
console.log(timings.join('\n'));
if (problems.length) {
  console.log('Problems:\n' + problems.join('\n'));
  process.exitCode = 1;
}
