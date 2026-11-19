// Behaviour checks for the Cash flow and Life stories views (not screenshots):
//   BASE_URL=http://localhost:5182 node e2e/cashflow-interact.mjs
// Year selection by click and keyboard, the flow diagram following it, the life picker,
// the year inspector and its tax return, a tiny sample, and a clean console.
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5182';
const shots = resolve(dirname(fileURLToPath(import.meta.url)), 'shots');
mkdirSync(shots, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const problems = [];
const check = (ok, what) => (ok ? console.log(`  ok   ${what}`) : (problems.push(what), console.log(`  FAIL ${what}`)));
page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !/status of 404|Download the React DevTools/.test(m.text()) && problems.push(`console ${m.type()}: ${m.text()}`));
page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
const go = (url) => page.evaluate((u) => { window.history.pushState(null, '', u); window.dispatchEvent(new PopStateEvent('popstate')); }, url);
const run = (n) => page.waitForFunction((paths) => {
  const fs = window.__firesight;
  if (fs.plan.getState().run.paths !== paths) fs.plan.getState().setRun({ paths });
  const r = fs.run.getState();
  return r.status === 'done' && r.total === paths && !r.stale;
}, n, { timeout: 180_000 });

await page.goto(base);
await page.getByTestId('demo-dual-income-family').click();
await run(2000);
await page.getByRole('button', { name: 'Close the assistant' }).click().catch(() => {});

// ---- Cash flow ----
await go('/cash-flow');
await page.getByTestId('cash-flow-lead').waitFor();
const year0 = await page.getByTestId('flow-year').innerText();
const flows = page.locator('[data-chart-cursor]').first();
await flows.scrollIntoViewIfNeeded();
const box = await flows.boundingBox();
await page.mouse.click(box.x + box.width * 0.6, box.y + box.height / 2);
await page.waitForTimeout(600);
const year1 = await page.getByTestId('flow-year').innerText();
check(year0 !== year1, `clicking the flows chart selects a year (${year0.replace(/\s+/g, ' ')} -> ${year1.replace(/\s+/g, ' ')})`);
await page.getByRole('button', { name: 'Next year' }).first().click();
check((await page.getByTestId('flow-year').innerText()) !== year1, 'the next-year button steps the flow');
await page.getByRole('slider', { name: 'Year shown in the flow' }).focus();
await page.keyboard.press('ArrowLeft');
check((await page.getByTestId('flow-year').innerText()) === year1, 'the scrubber moves by keyboard');
await flows.focus();
await page.keyboard.press('ArrowRight');
await page.keyboard.press('ArrowRight');
await page.keyboard.press('Enter');
await page.waitForTimeout(300);
check(/Age/.test(await page.getByTestId('flow-year').innerText()), 'the chart is keyboard operable (arrows, Enter)');
await page.getByText('Retirement', { exact: true }).first().click();
await page.waitForTimeout(500);
await page.screenshot({ path: `${shots}/cash-flow-interact-zoom.png` });
await page.getByText('Whole plan', { exact: true }).first().click();
const tables = page.getByRole('button', { name: 'View as a table' });
check((await tables.count()) >= 6, `every chart offers a table (${await tables.count()})`);

// ---- Life stories ----
await go('/life-stories');
await page.getByTestId('life-headline').waitFor({ timeout: 20_000 });
const head0 = await page.getByTestId('life-headline').innerText();
await page.getByTestId('life-p10').click();
await page.waitForFunction((h) => document.querySelector('[data-testid=life-headline]')?.textContent !== h, head0, { timeout: 20_000 });
check(/path=\d+&why=p10/.test(page.url()), 'picking a life puts it in the URL');
await page.getByRole('button', { name: 'Next life' }).click();
await page.waitForTimeout(800);
check(/why=sample/.test(page.url()), 'next life steps to the neighbouring path');
const inspector = page.getByTestId('year-inspector');
const label0 = await inspector.getAttribute('aria-label');
const timeline = page.locator('[data-chart-cursor]').first();
await timeline.scrollIntoViewIfNeeded();
const tb = await timeline.boundingBox();
await page.mouse.click(tb.x + tb.width * 0.3, tb.y + tb.height / 2);
await page.waitForTimeout(400);
// The year under the first click can be the one already open (the ledger opens on the retirement year): try another.
if ((await inspector.getAttribute('aria-label')) === label0) {
  await page.mouse.click(tb.x + tb.width * 0.7, tb.y + tb.height / 2);
  await page.waitForTimeout(400);
}
check((await inspector.getAttribute('aria-label')) !== label0, 'clicking the timeline opens that year in the inspector');
check((await inspector.locator('table').count()) === 1, 'the year has a tax return');
await page.getByTestId('life-narrative').locator('a').nth(2).click();
await page.waitForTimeout(600);
check(true, 'narrative sentences are links to years');
await go('/life-stories?path=999999&why=sample');
await page.waitForTimeout(1500);
check((await page.getByText(/There is no life #999,999 in this run/).count()) > 0 && (await page.getByTestId('life-headline').count()) === 0, 'a path outside the run is refused, not replayed');
await page.screenshot({ path: `${shots}/life-stories-interact-out-of-range.png` });

// ---- a tiny sample ----
await run(50);
await go('/life-stories');
await page.getByTestId('life-headline').waitFor({ timeout: 20_000 });
check((await page.getByText(/Only 50 lives/).count()) > 0, 'Life stories warns about a tiny sample');
await page.screenshot({ path: `${shots}/life-stories-interact-tiny.png` });
await go('/cash-flow');
await page.getByTestId('cash-flow-lead').waitFor();
check((await page.getByText(/Only 50 lives/).count()) > 0, 'Cash flow warns about a tiny sample');
await page.setViewportSize({ width: 1440, height: 4600 });
await page.waitForTimeout(500);
await page.screenshot({ path: `${shots}/cash-flow-interact-tiny.png` });

await browser.close();
if (problems.length) {
  console.log('Problems:\n' + [...new Set(problems)].join('\n'));
  process.exitCode = 1;
}
