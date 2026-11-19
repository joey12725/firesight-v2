// Screenshots of the Scenarios view, for looking at:
//   BASE_URL=http://localhost:5184 node e2e/scenarios.mjs [slug,slug] [viewport,viewport] [light,dark] [quick,quick]
// Opens a demo household, creates a few what-ifs with the quick buttons, waits for every
// paired comparison, and writes e2e/shots/scenarios-<slug>-<viewport>-<theme>.png (the
// fold) and -full.png (the whole view). PATHS sets the lives per run (default 10,000);
// DSF=1 keeps a tall phone capture within Chrome's texture limit.
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5184';
const shots = resolve(dirname(fileURLToPath(import.meta.url)), 'shots');
mkdirSync(shots, { recursive: true });
const VIEWPORTS = { wide: [2200, 1200], desktop: [1440, 900], laptop: [1024, 768], phone: [390, 844] };
const slugs = (process.argv[2] ?? 'dual-income-family,early-career-saver,side-hustle-landlord').split(',');
const viewports = (process.argv[3] ?? 'desktop,laptop,phone').split(',');
const themes = (process.argv[4] ?? 'light,dark').split(',');
const quick = (process.argv[5] ?? 'retire_earlier,spend_less,delay_social_security,crash_at_retirement').split(',').filter(Boolean);
const paths = Number(process.env.PATHS ?? 10000);
const tag = process.env.TAG ? `-${process.env.TAG}` : '';
const fullHeight = Number(process.env.FULL_HEIGHT ?? 9000);

const settled = (n) => {
  const fs = window.__firesight;
  if (fs.plan.getState().run.paths !== n) fs.plan.getState().setRun({ paths: n });
  const run = fs.run.getState();
  return run.status === 'done' && run.scenarios.every((s) => s.paired !== null) && run.total === n * (run.scenarios.length + 1);
};

const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];
for (const theme of themes) {
  for (const vp of viewports) {
    const [width, height] = VIEWPORTS[vp];
    const context = await browser.newContext({ viewport: { width, height }, colorScheme: theme, deviceScaleFactor: Number(process.env.DSF ?? (vp === 'phone' ? 2 : 1)) });
    for (const slug of slugs) {
      const page = await context.newPage();
      page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !/status of 404/.test(m.text()) && problems.push(`${slug} ${vp} ${theme} console ${m.type()}: ${m.text()}`));
      page.on('pageerror', (e) => problems.push(`${slug} ${vp} ${theme} pageerror: ${e.message}`));
      await page.goto(base);
      await page.getByTestId(`demo-${slug}`).click();
      await page.waitForFunction(settled, paths, { timeout: 180_000 });
      await page.getByRole('tab', { name: 'Scenarios' }).click();
      await page.getByText('If I change this decision').waitFor();
      if (process.env.EMPTY) await page.screenshot({ path: `${shots}/scenarios-${slug}-${vp}-${theme}-empty.png` });
      for (const key of quick) {
        const button = page.getByTestId(`quick-${key}`);
        if ((await button.count()) === 0) continue;
        await button.click();
        await page.waitForTimeout(150);
      }
      if (process.env.EARLY) {
        await page.waitForTimeout(Number(process.env.EARLY));
        await page.screenshot({ path: `${shots}/scenarios-${slug}-${vp}-${theme}-early.png` });
      }
      await page.waitForFunction(settled, paths, { timeout: 300_000 });
      await page.mouse.move(2, 2);
      await page.getByText('If I change this decision').scrollIntoViewIfNeeded();
      await page.waitForTimeout(1200);
      const name = `${shots}/scenarios-${slug}-${vp}-${theme}${tag}`;
      await page.screenshot({ path: `${name}.png` });
      await page.setViewportSize({ width, height: fullHeight });
      await page.getByText('If I change this decision').scrollIntoViewIfNeeded();
      await page.waitForTimeout(900);
      await page.screenshot({ path: `${name}-full.png` });
      await page.setViewportSize({ width, height });
      if (process.env.COMPACT) {
        for (const paired of [true, false]) {
          await page.evaluate(async (p) => (await import('/src/views/scenarios/compact/probe.tsx')).mount(p), paired);
          await page.waitForTimeout(700);
          await page.locator('#compact-probe').last().screenshot({ path: `${shots}/scenarios-${slug}-${theme}-compact${paired ? '' : '-pending'}.png` });
          await page.evaluate(() => document.querySelectorAll('#compact-probe').forEach((n) => n.remove()));
        }
      }
      const run = await page.evaluate(() => {
        const r = window.__firesight.run.getState();
        return { total: r.total, ms: Math.round(r.elapsedMs), scenarios: r.scenarios.length };
      });
      console.log(`  shot ${slug} ${vp} ${theme}: ${run.scenarios} scenarios, ${run.total} lives in ${run.ms} ms`);
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
