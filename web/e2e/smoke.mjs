// Headless smoke check against a running dev or preview server.
//   BASE_URL=http://localhost:5173 npm run smoke
// Uses the system Chrome when present, else Playwright's bundled Chromium.
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5173';
const shots = resolve(dirname(fileURLToPath(import.meta.url)), 'shots');
mkdirSync(shots, { recursive: true });

const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];
const steps = [];
const ok = (name) => {
  steps.push(name);
  console.log(`  ok  ${name}`);
};
const expect = (condition, message) => {
  if (!condition) throw new Error(message);
};

async function open(context, url) {
  const page = await context.newPage();
  page.on('console', (m) => m.type() === 'error' && problems.push(`console: ${m.text()}`));
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  page.on('requestfailed', (r) => problems.push(`request failed: ${r.url()}`));
  await page.goto(url);
  return page;
}

try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ['clipboard-read', 'clipboard-write'] });
  const page = await open(context, base);

  await page.getByRole('heading', { name: /When can work become optional/ }).waitFor();
  ok('app loads and wasm initialises (landing lists the demo households from the engine)');
  expect((await page.getByTestId(/^demo-/).count()) === 3, 'expected three demo households');
  await page.screenshot({ path: `${shots}/01-landing-light.png` });

  await page.getByTestId('demo-dual-income-family').click();
  await page.getByRole('heading', { level: 1, name: 'Dual-income family' }).waitFor();
  ok('demo household loads into the editor');

  await page.getByTestId('run-notice').waitFor();
  expect(/not part of this build/.test(await page.getByTestId('run-notice').innerText()), 'expected the engine-not-implemented notice');
  ok('results area shows the engine-not-implemented state');
  await page.screenshot({ path: `${shots}/02-workspace-light.png` });

  // Edit a salary through the editor.
  await page.getByRole('button', { name: /^Income/ }).click();
  await page.getByRole('button', { name: /Product manager/ }).click();
  const salary = page.getByLabel('Salary', { exact: true });
  await salary.fill('150k');
  await page.getByText('$150k/yr').first().waitFor();
  ok('number input accepts "150k" and the plan updates');

  // Edits to one field within 1.5 s share an undo step; wait so this is its own.
  await page.waitForTimeout(1700);
  await salary.fill('-5');
  await page.getByRole('button', { name: /Plan check/ }).waitFor();
  await page.getByText(/1 to fix/).first().waitFor();
  ok('edits validate live: the engine reports the negative salary');
  await page.getByText(/errors, so it cannot be simulated/).waitFor();
  await page.screenshot({ path: `${shots}/03-validation.png` });

  await page.getByRole('button', { name: /^Undo/ }).click();
  await page.getByText(/1 to fix/).first().waitFor({ state: 'detached' });
  expect((await salary.inputValue()) === '150,000', `after one undo the salary should be 150,000, got ${await salary.inputValue()}`);
  await page.keyboard.press('ControlOrMeta+z');
  await page.getByText('$128k/yr').first().waitFor();
  ok('undo works (button and keyboard)');
  await page.getByRole('button', { name: 'Redo' }).click();
  await page.getByText('$150k/yr').first().waitFor();
  ok('redo works');

  // Scenario from the last change.
  await page.getByRole('button', { name: 'Make the last change a scenario instead' }).click();
  await page.getByText('$128k/yr').first().waitFor();
  await page.getByRole('tab', { name: 'Scenarios' }).click();
  await page.getByRole('heading', { name: 'Scenarios', exact: true }).waitFor();
  await page.getByLabel('Scenario name').waitFor();
  expect((await page.getByLabel('Scenario name').count()) === 1, 'expected one scenario');
  ok('"make this change a scenario" restores the baseline and creates a scenario');
  expect(new URL(page.url()).pathname === '/scenarios', 'tab should be reflected in the URL');

  // Share link round trip, in a fresh browser profile.
  await page.getByRole('button', { name: 'Share this plan' }).click();
  const link = page.getByLabel('Share link');
  await page.waitForFunction(() => document.querySelector('#share-url')?.value.includes('#plan='));
  const url = await link.inputValue();
  const other = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const shared = await open(other, url);
  await shared.getByRole('heading', { level: 1, name: 'Dual-income family' }).waitFor();
  expect(!shared.url().includes('#plan='), 'the fragment should be cleared after import');
  await shared.getByRole('tab', { name: 'Scenarios' }).click();
  await shared.getByLabel('Scenario name').waitFor();
  expect((await shared.getByLabel('Scenario name').count()) === 1, 'the scenario should travel with the link');
  ok(`share link round-trips (${url.length} characters)`);
  await other.close();
  await page.getByRole('button', { name: 'Done' }).click();

  // Persistence: reload restores the household from IndexedDB.
  await page.waitForTimeout(900);
  await page.reload();
  await page.getByRole('heading', { level: 1, name: 'Dual-income family' }).waitFor();
  await page.getByLabel('Scenario name').waitFor();
  ok('reload restores the household and its scenario from IndexedDB');

  // v1 import shows the migration notes.
  await page.getByRole('button', { name: /Dual-income family/ }).first().click();
  await page.getByRole('button', { name: 'Import' }).click();
  const v1 = resolve(dirname(fileURLToPath(import.meta.url)), '../../examples/v1/early-career-saver.json');
  await page.getByLabel('Plan file').setInputFiles(v1);
  await page.getByRole('heading', { name: 'Converted from version 1' }).waitFor();
  await page.screenshot({ path: `${shots}/04-v1-migration.png` });
  await page.getByRole('button', { name: 'Review the plan' }).click();
  ok('v1 import runs migrateV1 and shows its notes');

  // Themes and sizes.
  await page.getByRole('tab', { name: 'Overview' }).click();
  for (const section of ['Spending', 'Savings and accounts']) await page.getByRole('button', { name: new RegExp(`^${section}`) }).click();
  await page.screenshot({ path: `${shots}/05-editor-light.png` });
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('radio', { name: /Dark/ }).click();
  await page.screenshot({ path: `${shots}/06-settings-dark.png` });
  await page.keyboard.press('Escape');
  await page.screenshot({ path: `${shots}/07-workspace-dark.png` });
  expect((await page.evaluate(() => document.documentElement.dataset.theme)) === 'dark', 'theme should be dark');
  ok('theme switches');

  await page.setViewportSize({ width: 900, height: 1100 });
  await page.waitForTimeout(250);
  await page.screenshot({ path: `${shots}/08-tablet-dark.png` });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(250);
  await page.screenshot({ path: `${shots}/09-phone-dark.png` });
  await page.getByRole('button', { name: 'Edit the plan' }).click();
  await page.getByRole('dialog', { name: 'Plan editor' }).waitFor();
  await page.waitForTimeout(350);
  await page.screenshot({ path: `${shots}/10-phone-editor-sheet.png` });
  ok('phone layout opens the editor as a sheet');

  // Guided setup produces a valid plan.
  const fresh = await browser.newContext({ viewport: { width: 1280, height: 860 } });
  const setup = await open(fresh, base);
  await setup.getByRole('button', { name: 'Start the guided setup' }).click();
  const next = () => setup.getByRole('button', { name: 'Continue' }).click();
  await setup.getByLabel('Your first name').fill('Sam');
  await setup.getByLabel('Age').fill('34');
  await next();
  await setup.getByLabel('State').selectOption('CO');
  await next();
  await setup.getByLabel('Pay before tax').fill('95k');
  await next();
  await setup.getByLabel('Rent', { exact: true }).fill('1800');
  await setup.getByLabel('Everything else you spend').fill('2400');
  await next();
  await setup.getByLabel('401(k), traditional IRA').fill('60k');
  await setup.getByLabel('Cash and savings').fill('15k');
  await next();
  await next();
  await setup.screenshot({ path: `${shots}/11-guided-setup.png` });
  await setup.getByRole('button', { name: 'Build my plan' }).click();
  await setup.getByRole('heading', { level: 1, name: "Sam's plan" }).waitFor();
  await setup.getByText('Plan check: nothing to fix').waitFor();
  await setup.screenshot({ path: `${shots}/12-setup-result.png` });
  ok('guided setup builds a plan with no validation errors');
  await fresh.close();

  // Every editor section, on every demo: expand everything, add one of each kind of
  // thing, and require that the engine accepts each addition without errors.
  for (const slug of ['early-career-saver', 'dual-income-family', 'side-hustle-landlord']) {
    // A fresh browser profile each time, so the landing state is shown.
    const tour = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
    const t = await open(tour, base);
    await t.getByTestId(`demo-${slug}`).click();
    await t.locator('[data-section="household"]').waitFor();
    const sections = t.locator('[data-section] > h3 > button[aria-expanded="false"]');
    while (await sections.count()) await sections.first().click();
    const closedItems = t.locator('li[data-path] > div > button[aria-expanded="false"]');
    while (await closedItems.count()) await closedItems.first().click();
    const closedDetails = t.locator('[data-section] button[aria-expanded="false"]:not([aria-haspopup])');
    while (await closedDetails.count()) await closedDetails.first().click();

    const failures = () => t.getByText('That change could not be applied').count();
    const noErrors = async (what) => {
      await t.waitForTimeout(60);
      expect((await failures()) === 0, `${slug}: adding ${what} was rejected by the engine`);
      const text = await t.locator('.flex.h-full.min-h-0.flex-col.bg-surface-1').first().innerText();
      const errors = await t.locator('li button:has(svg.text-bad)').allInnerTexts();
      expect(!/\d+ to fix/.test(text), `${slug}: adding ${what} left validation errors: ${errors.join(' | ')}`);
    };
    for (const name of ['Add a child', 'Job', 'Self-employment', 'Pension', 'Annuity', 'Other', 'Add a grant', 'Add a budget line', 'Cash', 'Taxable', 'Pre-tax', 'Roth', 'HSA', 'Home', 'Rental', 'Vacation home', 'Add a debt']) {
      await t.getByRole('button', { name, exact: true }).first().click();
      await noErrors(name);
    }
    for (const kind of ['Buy a home', 'Sell a property', 'Relocate', 'Windfall', 'Big expense', 'Income change', 'Career break', 'Spending change', 'Savings rate change']) {
      await t.getByRole('button', { name: 'Add a life event' }).click();
      await t.getByRole('dialog', { name: 'Add a life event' }).getByRole('button', { name: new RegExp(`^${kind}`) }).click();
      await noErrors(`event "${kind}"`);
    }
    for (const kind of ['Crash at retirement', 'Lost decade', 'Stagflation']) {
      await t.getByRole('button', { name: 'Add a stress test' }).click();
      await t.getByRole('dialog', { name: 'Add a stress test' }).getByRole('button', { name: new RegExp(`^${kind}`) }).click();
      await noErrors(`stress test "${kind}"`);
    }
    // Every choice of the main unions.
    for (const [label, count] of [['Retire', 5], ['Spending rule', 6], ['Withdrawal order', 3], ['How futures are generated', 4], ['Glide path', 4], ['Tax model', 3], ['Lifespans', 2]]) {
      const select = t.getByLabel(label, { exact: true });
      const values = await select.locator('option:not([disabled])').evaluateAll((os) => os.map((o) => o.value));
      expect(values.length === count, `${slug}: ${label} should offer ${count} choices, has ${values.length}`);
      for (const v of values) {
        await select.selectOption(v);
        await noErrors(`${label} = ${v}`);
      }
    }
    // Capture the whole editor column at its full height.
    await t.evaluate(() => {
      const scroller = document.querySelector('[data-section]')?.parentElement;
      if (scroller) scroller.dataset.fullHeight = String(scroller.scrollHeight);
    });
    const height = Number(await t.evaluate(() => document.querySelector('[data-full-height]')?.getAttribute('data-full-height') ?? 1000));
    await t.setViewportSize({ width: 1440, height: Math.min(height + 120, 14000) });
    await t.waitForTimeout(200);
    await t.screenshot({ path: `${shots}/tour-${slug}.png`, clip: { x: 0, y: 0, width: 400, height: Math.min(height + 120, 14000) } });
    ok(`${slug}: every section renders, and every addable item and union choice is accepted by the engine`);
    await tour.close();
  }

  expect(problems.length === 0, `console problems:\n${problems.join('\n')}`);
  ok('no console errors, page errors or failed requests');
  console.log(`\nsmoke: ${steps.length} checks passed. Screenshots in ${shots}`);
} catch (error) {
  console.error(`\nsmoke FAILED after ${steps.length} checks: ${error.message}`);
  if (problems.length) console.error(problems.join('\n'));
  process.exitCode = 1;
} finally {
  await browser.close();
}
