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
  // The dev server has no /api/chat (the hosted assistant probe 404s there); everything else must be clean.
  page.on('console', (m) => m.type() === 'error' && !/status of 404/.test(m.text()) && problems.push(`console: ${m.text()}`));
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  page.on('requestfailed', (r) => problems.push(`request failed: ${r.url()}`));
  page.on('response', (r) => r.status() >= 400 && !r.url().includes('/api/') && problems.push(`HTTP ${r.status()}: ${r.url()}`));
  await page.goto(url);
  return page;
}

try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ['clipboard-read', 'clipboard-write'] });
  const page = await open(context, base);

  await page.getByRole('heading', { name: /When can work become optional/ }).waitFor();
  ok('app loads and wasm initialises (landing lists the demo households from the engine)');
  expect((await page.getByTestId(/^demo-/).count()) === 4, 'expected four demo households');
  await page.screenshot({ path: `${shots}/01-landing-light.png` });

  await page.getByTestId('demo-dual-income-family').click();
  await page.getByRole('heading', { level: 1, name: 'Dual-income family' }).waitFor();
  ok('demo household loads into the editor');

  // The real engine: a first picture quickly, then refinement to the full run.
  const runDone = (p = page) => p.waitForFunction(() => window.__firesight.run.getState().status === 'done', null, { timeout: 120_000 });
  await page.getByTestId('outcome-sentence').waitFor();
  const first = await page.evaluate(() => window.__firesight.run.getState().firstPictureMs);
  await runDone();
  const sentence = await page.getByTestId('outcome-sentence').innerText();
  expect(/^In (all )?\d+ of 100 simulated lives the plan holds to the end\./.test(sentence), `unexpected outcome sentence: ${sentence}`);
  const figures = await page.getByTestId('headline').innerText();
  expect(/Work becomes optional\s+\d+\.\d/.test(figures) && /The plan holds in\s+\d+\s+(\d+\s+)?of 100/.test(figures), `headline figures missing: ${figures}`);
  expect(/Spending \$[\d.]+[kM] a year now/.test(await page.getByTestId('plan-facts').innerText()), 'the plan facts line should come from planPreview');
  expect((await page.getByTestId('run-notice').count()) === 0, 'no run notice expected when the engine runs');
  const cards = await page.locator('[data-chart-card]').evaluateAll((els) => els.map((e) => e.getAttribute('data-chart-card')));
  for (const card of ['age-work-becomes-optional', 'net-worth-by-age', 'life-stages-by-age', 'spending-by-age', 'what-could-go-wrong']) {
    expect(cards.includes(card), `Overview should show the ${card} chart (found ${cards.join(', ')})`);
  }
  // Every canvas layer has ink on it.
  const blank = await page.evaluate(() =>
    [...document.querySelectorAll('canvas[data-chart-layer]')].filter((c) => {
      const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
      for (let i = 3; i < d.length; i += 4 * 97) if (d[i] > 0) return false;
      return true;
    }).length,
  );
  expect(blank === 0, `${blank} chart canvas(es) are blank`);
  ok(`the engine runs: first picture ${Math.round(first)} ms after load, then "${sentence.slice(0, 58)}…"`);
  await page.screenshot({ path: `${shots}/02-workspace-light.png` });

  // Crosshair and tooltip on the fan, by pointer and by keyboard; the other time charts follow.
  const fan = page.locator('[data-chart-card="net-worth-by-age"]');
  const cursor = fan.locator('[data-chart-cursor]').first();
  await cursor.scrollIntoViewIfNeeded();
  const box = await cursor.boundingBox();
  await page.mouse.move(box.x + box.width * 0.55, box.y + box.height * 0.5);
  await fan.getByText('Median', { exact: true }).last().waitFor();
  expect((await fan.getByText(/lives still have someone alive/).count()) > 0, 'the fan tooltip should state how many lives are still alive');
  await page.screenshot({ path: `${shots}/02b-fan-tooltip.png` });
  await cursor.focus();
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('ArrowRight');
  const spoken = await fan.locator('[aria-live="polite"]').innerText();
  expect(/^Age \d+, \d{4}: median \$/.test(spoken), `keyboard reading should be announced, got "${spoken}"`);
  await page.keyboard.press('Escape');
  ok('fan crosshair: tooltip on hover, arrow keys announce the reading');

  // Table view, CSV, and the dot plot's cumulative twin.
  await fan.getByRole('button', { name: 'View as a table' }).click();
  await fan.getByRole('columnheader', { name: 'Median' }).waitFor();
  expect((await fan.locator('tbody tr').count()) > 40, 'the table view should list every year');
  const [download] = await Promise.all([page.waitForEvent('download'), fan.getByRole('button', { name: 'Download the data (CSV)' }).click()]);
  expect(download.suggestedFilename() === 'net-worth-by-age.csv', `unexpected CSV name ${download.suggestedFilename()}`);
  await fan.getByRole('button', { name: 'Show the chart' }).click();
  await page.getByRole('radio', { name: 'By this age' }).click();
  await page.locator('[data-chart-card="age-work-becomes-optional"] canvas').waitFor();
  await page.getByRole('radio', { name: '100 lives' }).click();
  ok('table view, CSV download and the cumulative toggle work');

  // A sample life opens in Life stories with its path index in the URL.
  await page.getByRole('button', { name: 'a middling one' }).click();
  expect(/^\/life-stories$/.test(new URL(page.url()).pathname) && /path=\d+&why=median/.test(new URL(page.url()).search), `expected a life-stories route, got ${page.url()}`);
  await page.getByRole('tab', { name: 'Overview' }).click();
  ok('"open a life" routes to Life stories with the path index');

  // Edit a salary through the editor.
  await page.getByRole('button', { name: /^Income/ }).click();
  await page.getByRole('button', { name: /Product manager/ }).click();
  const salary = page.getByLabel('Salary', { exact: true });
  // While the edit re-runs, the previous picture must stay on screen: no skeleton, no empty frame.
  await page.evaluate(() => {
    const w = window;
    w.__flicker = { detached: 0, nullSummary: 0, first: 0, t0: performance.now() };
    new MutationObserver(() => {
      if (!document.querySelector('[data-testid="outcome-sentence"]') || document.querySelectorAll('canvas[data-chart-layer]').length < 2) w.__flicker.detached += 1;
    }).observe(document.querySelector('#main'), { childList: true, subtree: true });
    w.__firesight.run.subscribe((s2, p2) => {
      if (s2.summary === null) w.__flicker.nullSummary += 1;
      if (s2.summary !== p2.summary && !w.__flicker.first) w.__flicker.first = performance.now() - w.__flicker.t0;
    });
    w.__flicker.t0 = performance.now();
  });
  const before = await page.getByTestId('headline').innerText();
  await salary.fill('150k');
  await page.getByText('$150k/yr').first().waitFor();
  ok('number input accepts "150k" and the plan updates');
  await runDone();
  await page.waitForTimeout(600);
  const flicker = await page.evaluate(() => window.__flicker);
  expect(flicker.detached === 0 && flicker.nullSummary === 0, `charts flickered during the re-run: ${JSON.stringify(flicker)}`);
  expect((await page.getByTestId('headline').innerText()) !== before, 'the headline should change after a pay rise');
  ok(`an edit refines the charts in place (new picture ${Math.round(flicker.first)} ms after the edit, nothing unmounted)`);

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
  for (const slug of ['early-career-saver', 'dual-income-family', 'side-hustle-landlord', 'startup-employee']) {
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
    // Startup equity: a company, a price, one holding of each kind, the household's own odds, and the main choices.
    const equity = t.locator('[data-section="equity"]');
    await equity.getByRole('button', { name: 'Add a company', exact: true }).click();
    await noErrors('a company');
    await equity.getByRole('textbox', { name: 'Latest price per share', exact: true }).last().fill('14');
    await noErrors('a latest price per share');
    for (const kind of ['Incentive options (ISO)', 'Non-qualified options (NSO)', 'RSUs', 'Shares I own']) {
      await equity.getByRole('button', { name: 'Add a holding', exact: true }).last().click();
      await t.getByRole('dialog', { name: 'Add a holding' }).getByRole('button', { name: new RegExp(`^${kind.replace(/[()]/g, '\\$&')}`) }).click();
      await noErrors(`holding "${kind}"`);
    }
    await equity.getByTestId('equity-preview').last().waitFor();
    expect(/^In (\d+|every one) of 100 futures this pays/.test(await equity.getByTestId('equity-preview').last().innerText()), `${slug}: the equity preview should lead with how often it pays nothing`);
    await equity.getByRole('button', { name: 'Use my own odds', exact: true }).last().click();
    await noErrors('my own odds');
    await equity.getByRole('button', { name: 'Add an outcome', exact: true }).last().click();
    await t.getByRole('dialog', { name: 'Add an outcome' }).getByRole('button', { name: /^Goes public/ }).click();
    await noErrors('an outcome');
    for (const [label, count] of [['Stage', 6], ['Exercise options', 3], ['After an IPO lockup', 3]]) {
      const select = equity.getByRole('combobox', { name: label, exact: true }).last();
      const values = await select.locator('option:not([disabled])').evaluateAll((os) => os.map((o) => o.value));
      expect(values.length === count, `${slug}: ${label} should offer ${count} choices, has ${values.length}`);
      for (const v of values) {
        await select.selectOption(v);
        await noErrors(`${label} = ${v}`);
      }
    }
    if (slug === 'startup-employee') {
      // The Equity results tab (W2's view) opens on the demo that has a company.
      await t.getByRole('tab', { name: 'Equity', exact: true }).click();
      expect(new URL(t.url()).pathname === '/equity', `the Equity tab should be reflected in the URL, got ${t.url()}`);
      await t.getByTestId('equity-lead').waitFor();
      expect(/of 100/.test(await t.getByTestId('equity-lead').innerText()), 'the Equity view should lead with a count of lives');
      await t.getByRole('tab', { name: 'Overview', exact: true }).click();
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
