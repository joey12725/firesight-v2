// Cross-view review captures, for looking at and for mechanical layout checks:
//   BASE_URL=http://localhost:5173 OUT=/some/dir node e2e/review.mjs [slugs] [views] [layouts] [themes]
// layouts: wide (1440x900, panels collapsed), desktop (1440x900, both panels open),
// laptop (1024x768), phone (390x844). Writes <view>-<slug>-<layout>-<theme>.png (the whole
// view: the window is grown so the results column does not scroll) and reports horizontal
// overflow, clipped text and console problems.
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5173';
const out = process.env.OUT ?? resolve(dirname(fileURLToPath(import.meta.url)), 'shots');
mkdirSync(out, { recursive: true });
const LAYOUTS = { wide: [1440, 900, false], desktop: [1440, 900, true], laptop: [1024, 768, true], phone: [390, 844, true] };
const ALL_VIEWS = ['overview', 'outcomes', 'cash-flow', 'what-matters', 'scenarios', 'life-stories', 'assumptions'];
const arg = (i, d) => (process.argv[i] && process.argv[i] !== '-' ? process.argv[i] : d).split(',');
const slugs = arg(2, 'dual-income-family,early-career-saver,side-hustle-landlord,full-life');
const views = arg(3, ALL_VIEWS.join(','));
const layouts = arg(4, 'wide,desktop,laptop,phone');
const themes = arg(5, 'light,dark');
const paths = Number(process.env.PATHS ?? 10000);
const fold = process.env.FOLD === '1';
const settle = Number(process.env.SETTLE ?? 1500);

const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];
for (const theme of themes) {
  for (const layout of layouts) {
    const [width, height, panels] = LAYOUTS[layout];
    for (const slug of slugs) {
      const context = await browser.newContext({ viewport: { width, height }, colorScheme: theme, deviceScaleFactor: 1 });
      const page = await context.newPage();
      const tag = `${slug} ${layout} ${theme}`;
      page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !/status of 404/.test(m.text()) && problems.push(`${tag} console ${m.type()}: ${m.text().slice(0, 300)}`));
      page.on('pageerror', (e) => problems.push(`${tag} pageerror: ${e.message}`));
      await page.goto(base);
      await page.getByTestId(`demo-${slug}`).click();
      await page.evaluate((panels) => window.__firesight.settings.getState().set({ editorOpen: panels, assistantOpen: panels }), panels);
      await page.waitForFunction((n) => {
        const fs = window.__firesight;
        if (fs.plan.getState().run.paths !== n) fs.plan.getState().setRun({ paths: n });
        const run = fs.run.getState();
        return run.status === 'done' && run.total === n;
      }, paths, { timeout: 180_000 });
      if (process.env.SCENARIO === '1') {
        // One enabled scenario so the Scenarios view has something to pair.
        await page.evaluate(() => {
          const s = window.__firesight.plan.getState();
          const items = s.profile.spending.items;
          s.addScenario({ name: 'Spend 10% less', patch: items.map((it, i) => ({ op: 'set', path: `/spending/items/${i}/amount`, value: Math.round(it.amount * 0.9) })) });
        });
        await page.waitForFunction(() => window.__firesight.run.getState().status === 'done' && window.__firesight.run.getState().scenarios.every((s) => s.paired), null, { timeout: 180_000 }).catch(() => {});
      }
      for (const view of views) {
        await page.evaluate((view) => {
          window.history.pushState(null, '', view === 'overview' ? '/' : `/${view}`);
          window.dispatchEvent(new PopStateEvent('popstate'));
        }, view);
        await page.waitForTimeout(settle);
        if (view === 'what-matters') await page.waitForTimeout(Number(process.env.SWEEP_WAIT ?? 12000));
        const name = `${out}/${view}-${slug}-${layout}-${theme}`;
        if (fold) await page.screenshot({ path: `${name}-fold.png` });
        // Grow the window until the results pane no longer scrolls, then capture everything.
        const need = await page.evaluate(() => {
          const pane = document.querySelector('[data-results-scroll]') ?? document.querySelector('main .overflow-y-auto');
          return pane ? pane.scrollHeight - pane.clientHeight : 0;
        });
        const tall = Math.min(12000, height + need + 40);
        await page.setViewportSize({ width, height: tall });
        await page.waitForTimeout(view === 'what-matters' ? 2500 : 700);
        const issues = await page.evaluate(() => {
          const found = [];
          const doc = document.documentElement;
          if (doc.scrollWidth > doc.clientWidth + 1) found.push(`page overflows horizontally by ${doc.scrollWidth - doc.clientWidth}px`);
          const main = document.querySelector('main');
          if (main) {
            const edge = main.getBoundingClientRect().right;
            for (const el of main.querySelectorAll('h1,h2,h3,p,li,dt,dd,button,figcaption,th,td,label,text')) {
              const r = el.getBoundingClientRect();
              if (r.width === 0 || r.height === 0) continue;
              if (r.right > edge + 2 && !el.closest('.overflow-x-auto,[data-allow-overflow]')) found.push(`past the right edge by ${Math.round(r.right - edge)}px: <${el.tagName.toLowerCase()}> "${(el.textContent ?? '').trim().slice(0, 50)}"`);
            }
          }
          return [...new Set(found)].slice(0, 8);
        });
        for (const i of issues) problems.push(`${view} ${tag}: ${i}`);
        await page.screenshot({ path: `${name}.png` });
        await page.setViewportSize({ width, height });
      }
      console.log(`  shot ${tag}`);
      await context.close();
    }
  }
}
await browser.close();
if (problems.length) {
  console.log('Problems:\n' + [...new Set(problems)].join('\n'));
  process.exitCode = 1;
}
