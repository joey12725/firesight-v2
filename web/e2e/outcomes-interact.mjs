// Interaction checks for the Outcomes and Assumptions views against a running dev server:
//   BASE_URL=http://localhost:5181 node e2e/outcomes-interact.mjs
// Covers: tiny samples, the scatter's range buttons, keyboard reading, hover tooltip,
// opening a life, the table view, the four-model run and its cancellation, and the
// editor links on Assumptions. Screenshots of the states land in e2e/shots/outcomes-state-*.png.
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5181';
const shots = resolve(dirname(fileURLToPath(import.meta.url)), 'shots');
mkdirSync(shots, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];
const ok = (name) => console.log(`  ok  ${name}`);
const expect = (condition, message) => {
  if (!condition) throw new Error(message);
};
const done = (page, n) =>
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

try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  const page = await context.newPage();
  page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !/status of 404/.test(m.text()) && problems.push(`console ${m.type()}: ${m.text()}`));
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  await page.goto(base);
  await page.getByTestId('demo-early-career-saver').click();

  // A tiny sample: every chart still draws, the page says how rough it is.
  await done(page, 60);
  await page.goto(`${base}/outcomes`);
  await done(page, 60);
  await page.getByTestId('range-sentence').waitFor();
  expect(await page.getByText(/Only 60 lives simulated/).count(), 'tiny-sample warning missing');
  await page.screenshot({ path: `${shots}/outcomes-state-60-lives.png`, fullPage: false });
  ok('60 lives: view renders with a sample-size warning');

  await done(page, 10000);
  await page.waitForTimeout(600);

  // Scatter: range buttons list lives; a listed life opens in Life stories.
  const selection = page.getByTestId('scatter-selection');
  await selection.scrollIntoViewIfNeeded();
  await selection.getByRole('button', { name: 'Unluckiest fifth' }).click();
  await selection.getByText(/of them ran short/).waitFor();
  const lives = selection.getByRole('list').getByRole('button');
  expect((await lives.count()) === 8, `expected 8 listed lives, got ${await lives.count()}`);
  await page.waitForTimeout(200);
  await page.locator('[data-chart-card="early-returns-and-endings"]').screenshot({ path: `${shots}/outcomes-state-scatter-selection.png` });
  ok('scatter: "Unluckiest fifth" lists eight lives');

  // Hover shows the group tooltip; arrow keys read the same values.
  const cursor = page.locator('[data-chart-card="early-returns-and-endings"] [data-chart-cursor]');
  const box = await cursor.boundingBox();
  await page.mouse.move(box.x + box.width * 0.35, box.y + box.height * 0.8);
  await page.getByText(/Median ending/).last().waitFor();
  await page.locator('[data-chart-card="early-returns-and-endings"]').screenshot({ path: `${shots}/outcomes-state-scatter-hover.png` });
  await cursor.focus();
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('ArrowRight');
  const spoken = await page.locator('[data-chart-card="early-returns-and-endings"] [aria-live]').textContent();
  expect(/ran short/.test(spoken ?? ''), `keyboard reading missing: ${spoken}`);
  await page.keyboard.press('Enter');
  await selection.getByText(/of them ran short/).waitFor();
  ok('scatter: hover tooltip, keyboard reading and Enter to select');

  // Age mode.
  await page.getByRole('radio', { name: 'Age the money ran short' }).click();
  await page.waitForTimeout(300);
  await page.locator('[data-chart-card="early-returns-and-endings"]').screenshot({ path: `${shots}/outcomes-state-scatter-age.png` });
  ok('scatter: age mode draws');

  // Table view of a dot plot.
  const wealth = page.locator('[data-chart-card="net-worth-at-end"]');
  await wealth.getByRole('button', { name: 'View as a table' }).click();
  expect((await wealth.getByRole('row').count()) > 9, 'table view has no rows');
  await wealth.getByRole('button', { name: 'Show the chart' }).click();
  ok('table view toggles');

  // Dot-plot keyboard access.
  const dots = wealth.locator('[data-chart-cursor]');
  await dots.focus();
  await page.keyboard.press('ArrowRight');
  expect(/of 100 lives/.test((await wealth.locator('[aria-live]').textContent()) ?? ''), 'dot plot keyboard reading missing');
  ok('dot plot reads by keyboard');

  // Four models: start, see progress, stop, start again, finish.
  const tri = page.locator('[data-chart-card="market-model-comparison"]');
  await tri.scrollIntoViewIfNeeded();
  await tri.screenshot({ path: `${shots}/outcomes-state-models-idle.png` });
  await page.getByTestId('triangulate').click();
  await tri.getByRole('button', { name: 'Stop' }).waitFor();
  await tri.getByRole('button', { name: 'Stop' }).click();
  await page.getByTestId('triangulate').waitFor();
  await page.waitForTimeout(300);
  expect((await page.evaluate(() => window.__firesight.pool().activeJobs)) === 0, 'cancelled model runs are still active');
  ok('four models: cancellation leaves no jobs running');
  const t0 = Date.now();
  await page.getByTestId('triangulate').click();
  await page.waitForFunction(() => document.querySelector('[data-testid="triangulate"]')?.textContent?.includes('Run again'), null, { timeout: 120_000 });
  console.log(`      four models x 2,000 lives took ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  await page.waitForTimeout(300);
  await tri.screenshot({ path: `${shots}/outcomes-state-models-done.png` });
  ok('four models: completes with a sentence and a strip');

  // A listed life opens in Life stories with its path index.
  await selection.scrollIntoViewIfNeeded();
  await selection.getByRole('list').getByRole('button').first().click();
  expect(/\/life-stories\?path=\d+&why=scatter/.test(page.url()), `did not open a life: ${page.url()}`);
  ok('a listed life opens in Life stories');

  // Assumptions: the editor link opens the section and focuses the field.
  await page.goto(`${base}/assumptions`);
  await done(page, 10000);
  await page.getByRole('button', { name: 'Change the market model' }).click();
  await page.waitForTimeout(500);
  const focused = await page.evaluate(() => document.activeElement?.closest('[data-path]')?.getAttribute('data-path') ?? document.activeElement?.closest('[data-section]')?.getAttribute('data-section') ?? null);
  expect(focused && /assumptions/.test(focused), `editor did not focus the market model: ${focused}`);
  ok(`Assumptions: editor link focuses ${focused}`);
  await page.getByText(/next dollar at|no federal income tax|pays the flat foreign rate/).first().waitFor({ timeout: 20_000 });
  ok('Assumptions: tax ladder is marked from the traced life');
} catch (error) {
  problems.push(String(error?.stack ?? error));
}
await browser.close();
if (problems.length) {
  console.log('Problems:\n' + [...new Set(problems)].join('\n'));
  process.exitCode = 1;
} else console.log('All interaction checks passed.');
