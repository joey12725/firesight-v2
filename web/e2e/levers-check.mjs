// Behaviour checks for the What matters view (not screenshots):
//   BASE_URL=http://localhost:5183 node e2e/levers-check.mjs
// An edit cancels the sweeps in flight and holds the old picture dimmed; leaving the view
// cancels everything; coming back is served from the cache; keyboard reaches the rows.
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5183';
const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const problems = [];
const check = (ok, what) => (console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`), ok || problems.push(what));
page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !/status of 404/.test(m.text()) && problems.push(`console ${m.type()}: ${m.text()}`));
page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));

await page.goto(base);
await page.getByTestId('demo-dual-income-family').click();
await page.waitForFunction(() => window.__firesight.run.getState().status === 'done', null, { timeout: 180_000 });
const jobs = () => page.evaluate(() => window.__firesight.pool().activeJobs);
const panel = () => page.evaluate(() => document.querySelector('[role=tabpanel]')?.textContent ?? '');

await page.getByRole('tab', { name: 'What matters' }).click();
await page.getByTestId('lead-sentence').waitFor({ timeout: 60_000 });
const before = await page.getByTestId('lead-sentence').textContent();
check((await jobs()) > 0, 'the tornado keeps sharpening after its first picture');

// Edit while a sweep is in flight: stale work stops at once, the old picture is held.
await page.evaluate(() => window.__firesight.plan.getState().edit([{ op: 'set', path: '/portfolio/expense_ratio', value: 0.004 }]));
await page.waitForFunction(() => /Updating/.test(document.querySelector('[role=tabpanel]')?.textContent ?? ''), null, { timeout: 5_000 }).then(() => check(true, 'an edit marks the tornado as updating')).catch(() => check(false, 'an edit marks the tornado as updating'));
check((await page.getByTestId('lead-sentence').count()) === 1, 'the previous picture stays on screen while updating');
await page.waitForFunction(() => !/Updating|Sharpening|Early picture|Simulating/.test(document.querySelector('[role=tabpanel]')?.textContent ?? ''), null, { timeout: 120_000 });
check(/4,000 lives/.test(await panel()), 'the tornado settles at 4,000 lives per test');

// Leaving the view cancels everything it started.
await page.evaluate(() => window.__firesight.plan.getState().edit([{ op: 'set', path: '/portfolio/expense_ratio', value: 0.003 }]));
await page.waitForTimeout(600);
await page.getByRole('tab', { name: 'Overview' }).click();
await page.waitForFunction(() => window.__firesight.run.getState().status === 'done', null, { timeout: 60_000 });
await page.waitForTimeout(300);
check((await jobs()) === 0, 'leaving the view leaves no sweep running');

// Undo twice: back to the first plan, whose tornado is cached and appears at once, complete.
await page.evaluate(() => (window.__firesight.plan.getState().undo(), window.__firesight.plan.getState().undo()));
await page.waitForFunction(() => window.__firesight.run.getState().status === 'done', null, { timeout: 60_000 });
await page.getByRole('tab', { name: 'What matters' }).click();
await page.getByTestId('lead-sentence').waitFor({ timeout: 10_000 });
await page.waitForTimeout(150);
check((await page.getByTestId('lead-sentence').textContent()) !== null && !/Sharpening|Early picture|Simulating|Updating/.test((await panel()).slice(0, 400)), 'a plan seen before is served from the cache');
console.log(`  first: ${before}`);

// Keyboard: a tornado row takes focus, Enter opens the paired detail, the scenario is added through the store.
await page.locator('[data-lever-bar]').first().focus();
await page.keyboard.press('Enter');
await page.getByTestId('lever-detail').waitFor({ timeout: 5_000 }).then(() => check(true, 'Enter on a row opens the paired detail')).catch(() => check(false, 'Enter on a row opens the paired detail'));
await page.getByTestId('lever-detail').getByRole('button', { name: 'Try this change' }).click();
check((await page.evaluate(() => window.__firesight.plan.getState().scenarios.length)) === 1, '"Try this change" adds a scenario through the profile store');
await page.getByTestId('lever-detail').getByText('In your scenarios').waitFor({ timeout: 5_000 }).then(() => check(true, 'the button then points to the scenario')).catch(() => check(false, 'the button then points to the scenario'));

// The map starts only once scrolled into view; arrow keys read its cells.
await page.locator('[data-chart-card=trade-off-map]').scrollIntoViewIfNeeded();
await page.locator('[data-chart-card=trade-off-map] [data-chart-cursor]').waitFor({ timeout: 60_000 });
await page.locator('[data-chart-card=trade-off-map] [data-chart-cursor]').focus();
await page.keyboard.press('ArrowRight');
await page.keyboard.press('ArrowUp');
const spoken = await page.locator('[data-chart-card=trade-off-map] [aria-live]').textContent();
check(/Trigger rate .*%, spending \$/.test(spoken ?? ''), `arrow keys read a map cell aloud (${spoken})`);
// The table view of each chart has rows.
for (const card of ['what-moves-holds', 'trade-off-map']) {
  await page.locator(`[data-chart-card=${card}]`).getByRole('button', { name: 'View as a table' }).click();
  const rows = await page.locator(`[data-chart-card=${card}] tbody tr`).count();
  check(rows >= 10, `${card}: the table view has ${rows} rows`);
  await page.locator(`[data-chart-card=${card}]`).getByRole('button', { name: 'Show the chart' }).click();
}
await browser.close();
if (problems.length) {
  console.log('Problems:\n' + problems.join('\n'));
  process.exitCode = 1;
}
