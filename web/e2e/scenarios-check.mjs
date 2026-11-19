// Interaction checks for the Scenarios view against a running dev server:
//   BASE_URL=http://localhost:5184 node e2e/scenarios-check.mjs
// Covers: the empty bench, a one-press what-if, the tiny-sample warning, rename,
// duplicate, switching off, edit target, delete and promote with undo, choosing a
// scenario from the ranking by keyboard, the table view, the same-life replay and its
// link into Life stories. State screenshots land in e2e/shots/scenarios-state-*.png.
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5184';
const shots = resolve(dirname(fileURLToPath(import.meta.url)), 'shots');
mkdirSync(shots, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];
const ok = (name) => console.log(`  ok  ${name}`);
const expect = (condition, message) => {
  if (!condition) throw new Error(message);
};
const settled = (page, n) =>
  page.waitForFunction(
    (count) => {
      const fs = window.__firesight;
      if (fs.plan.getState().run.paths !== count) fs.plan.getState().setRun({ paths: count });
      const run = fs.run.getState();
      const enabled = fs.plan.getState().scenarios.filter((s) => s.enabled).length;
      return run.status === 'done' && !run.stale && run.scenarios.length === enabled && run.scenarios.every((s) => s.paired !== null) && run.total === count * (enabled + 1);
    },
    n,
    { timeout: 240_000 },
  );
const plan = (page) =>
  page.evaluate(() => {
    const s = window.__firesight.plan.getState();
    return { scenarios: s.scenarios.map((x) => ({ id: x.id, name: x.name, enabled: x.enabled, ops: x.patch.length })), editTarget: s.editTarget, base: JSON.stringify(s.profile) };
  });

try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  const page = await context.newPage();
  page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !/status of 404/.test(m.text()) && problems.push(`console ${m.type()}: ${m.text()}`));
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  await page.goto(base);
  await page.getByTestId('demo-dual-income-family').click();
  await settled(page, 60);
  await page.getByRole('tab', { name: 'Scenarios' }).click();
  await page.getByText('No scenarios yet').waitFor();
  expect((await page.getByTestId('scenario-detail').count()) === 0, 'detail shown with no scenario');
  ok('empty bench: says so and offers the what-ifs');

  // One press makes and runs a what-if; with 60 lives the page says how rough it is.
  await page.getByTestId('quick-spend_less').click();
  await settled(page, 60);
  await page.getByTestId('flip-sentence').waitFor();
  expect(await page.getByText(/Only 60 lives paired/).count(), 'tiny-sample warning missing');
  await page.getByTestId('scenario-detail').scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${shots}/scenarios-state-60-lives.png` });
  ok('60 lives: the comparison renders with a sample-size warning');

  await settled(page, 10000);
  const first = (await plan(page)).scenarios[0];
  expect(first && first.ops > 0, 'quick scenario missing');

  // Rename in place.
  const row = page.getByTestId(`scenario-${first.id}`);
  await row.getByLabel('Scenario name').fill('Leaner retirement');
  await page.getByTestId('scenario-detail').getByText('Leaner retirement').first().waitFor();
  expect((await plan(page)).scenarios[0].name === 'Leaner retirement', 'rename did not reach the store');
  ok('rename: the detail follows the new name');

  // Duplicate, then a second what-if: the board and the ranking appear.
  await row.getByRole('button', { name: 'More actions' }).click();
  await page.getByRole('menuitem', { name: 'Duplicate' }).click();
  expect((await plan(page)).scenarios.length === 2, 'duplicate did not add a scenario');
  await page.getByTestId('quick-delay_social_security').click();
  await settled(page, 10000);
  await page.getByText('All 3 scenarios against the plan').waitFor();
  ok('duplicate and a second what-if: three scenarios ranked');

  // Choose from the ranking by keyboard.
  const rankButton = page.locator('[data-chart-card] button[aria-label*="Compare with the plan in detail"]').first();
  await rankButton.focus();
  await page.keyboard.press('Enter');
  expect((await rankButton.getAttribute('aria-pressed')) === 'true', 'ranking row not selected by keyboard');
  ok('ranking: a row is chosen with the keyboard');

  // The table view of a chart.
  const card = page.locator('[data-chart-card]').filter({ hasText: 'The difference, life by life' }).first();
  await card.getByRole('button', { name: 'View as a table' }).click();
  await card.getByRole('table').waitFor();
  await card.getByRole('button', { name: 'Show the chart' }).click();
  ok('table view: the per-life differences read as a table');

  // Switch a scenario off: it leaves the run and says so.
  const copy = (await plan(page)).scenarios[1];
  const copyRow = page.getByTestId(`scenario-${copy.id}`);
  await copyRow.getByRole('switch').click();
  await copyRow.getByText('Not simulated').waitFor();
  await settled(page, 10000);
  await page.getByText('All 2 scenarios against the plan').waitFor();
  ok('switch off: the scenario leaves the run and the board');

  // Edit target.
  await row.getByRole('button', { name: /^Edit/ }).click();
  expect((await plan(page)).editTarget === first.id, 'edit target not set');
  await row.getByText('Editing', { exact: true }).waitFor();
  await row.getByRole('button', { name: 'Stop editing' }).click();
  expect((await plan(page)).editTarget === 'base', 'edit target not restored');
  ok('edit in the plan editor: sets and clears the edit target');

  // Delete with undo.
  await copyRow.getByRole('button', { name: 'More actions' }).click();
  await page.getByRole('menuitem', { name: 'Delete' }).click();
  expect((await plan(page)).scenarios.length === 2, 'delete did not remove the scenario');
  await page.getByRole('button', { name: 'Undo' }).last().click();
  expect((await plan(page)).scenarios.length === 3, 'undo did not restore the scenario');
  ok('delete: undo restores it');

  // The same life under both decisions, and the way into Life stories.
  await settled(page, 10000);
  const life = page.locator('[data-chart-card]').filter({ hasText: 'The same life, two decisions' }).first();
  await life.scrollIntoViewIfNeeded();
  await life.getByText(/In life #\d+/).waitFor({ timeout: 30_000 });
  await page.waitForTimeout(400);
  await life.screenshot({ path: `${shots}/scenarios-state-same-life.png` });
  const options = life.getByRole('radio');
  if ((await options.count()) > 1) {
    const before = await life.getByText(/In life #\d+/).textContent();
    await options.last().click();
    await page.waitForFunction((t) => ![...document.querySelectorAll('[data-chart-card]')].some((c) => c.textContent?.includes(t)), before, { timeout: 30_000 });
  }
  ok('same life: replays under both plans and switches between picked lives');

  // Promote with undo.
  const before = await plan(page);
  await row.getByRole('button', { name: 'More actions' }).click();
  await page.getByRole('menuitem', { name: 'Make this the plan' }).click();
  const promoted = await plan(page);
  expect(promoted.scenarios.length === before.scenarios.length - 1 && promoted.base !== before.base, 'promote did not change the plan');
  await page.screenshot({ path: `${shots}/scenarios-state-promoted.png` });
  await page.getByRole('button', { name: 'Undo' }).last().click();
  const restored = await plan(page);
  expect(restored.scenarios.length === before.scenarios.length && restored.base === before.base, 'undo did not restore the plan and the scenario');
  ok('make this the plan: applies the patch to the baseline, and undo restores both');

  await settled(page, 10000);
  await life.scrollIntoViewIfNeeded();
  await life.getByRole('button', { name: 'Read this life year by year' }).click();
  await page.waitForFunction(() => document.querySelector('[role="tab"][aria-selected="true"]')?.textContent?.includes('Life stories'), null, { timeout: 15_000 });
  ok('same life: links out to Life stories');
} catch (error) {
  problems.push(String(error?.stack ?? error));
} finally {
  await browser.close();
}
if (problems.length) {
  console.log('Problems:\n' + [...new Set(problems)].join('\n'));
  process.exitCode = 1;
} else console.log('All checks passed.');
