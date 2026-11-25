// End-to-end journeys across views, against a running dev or preview server:
//   BASE_URL=http://localhost:4173 node e2e/journeys.mjs
// One browser, real engine. Each journey is something a person does that crosses views:
// an edit refining every view, scenarios created from the tornado, the map, the solver and
// the quick buttons arriving in Scenarios with paired results, a named life opening in
// Life stories, v1 imports, undo and redo across scenario operations, a rename reaching
// the run, a household's conversation going with it, the landing hand-off, a stray URL.
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5173';
const here = dirname(fileURLToPath(import.meta.url));
const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];
let passed = 0;
const ok = (name) => {
  passed += 1;
  console.log(`  ok  ${name}`);
};
const expect = (condition, message) => {
  if (!condition) throw new Error(message);
};

function watch(page, tag) {
  page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !/status of 404/.test(m.text()) && problems.push(`${tag} console ${m.type()}: ${m.text().slice(0, 300)}`));
  page.on('pageerror', (e) => problems.push(`${tag} pageerror: ${e.message}`));
}
const settled = (page, timeout = 240_000) =>
  page.waitForFunction(
    () => {
      const run = window.__firesight.run.getState();
      return run.status === 'done' && !run.stale && run.scenarios.every((s) => s.paired);
    },
    null,
    { timeout },
  );
const state = (page) =>
  page.evaluate(() => {
    const s = window.__firesight.plan.getState();
    const r = window.__firesight.run.getState();
    return { scenarios: s.scenarios.map((x) => ({ id: x.id, name: x.name })), ran: r.scenarios.map((x) => ({ id: x.id, name: x.name, paired: Boolean(x.paired) })), past: s.past.length, future: s.future.length, bequest: s.profile?.retirement.bequest_target, success: r.summary?.headline.success_rate.value };
  });
const tab = (page, name) => page.getByRole('tab', { name, exact: true }).click();

try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  watch(page, 'main');

  // ---- a stray address ----
  await page.goto(`${base}/not-a-view/at-all`);
  await page.getByRole('heading', { name: /When can work become optional/ }).waitFor();
  expect(new URL(page.url()).pathname === '/', `a path that names no view should be put right, got ${page.url()}`);
  ok('an unknown address shows the app at / instead of a dead page');

  await page.getByTestId('demo-dual-income-family').click();
  await settled(page);
  expect(await page.getByTestId('headline-home-sale').count(), 'the headline strip should state the home-sale rate beside the success figure');
  expect(await page.getByTestId('home-sale-sentence').count(), 'the Overview should state the home-sale rate under its sentence');
  const spendingFigure = await page.getByTestId('headline-spending').innerText();
  const delivered = Number(/(\d+)%/.exec(spendingFigure)?.[1]);
  expect(delivered <= 100 && /Spending delivered/.test(spendingFigure), `the delivered share is headlined capped at 100%: ${spendingFigure}`);
  ok(`success is headlined with the home sale beside it, and delivered spending as a capped share (${delivered}%)`);
  expect((await page.getByRole('tab', { name: 'Equity', exact: true }).count()) === 0, 'a plan without a private company has no Equity tab');
  expect((await page.getByTestId('headline-equity').count()) === 0 && (await page.getByTestId('overview-equity').count()) === 0, 'a plan without a private company says nothing about equity');

  // ---- an edit refines every view ----
  const before = await state(page);
  await page.evaluate(() => {
    const s = window.__firesight.plan.getState();
    const i = s.profile.spending.items.findIndex((x) => x.amount > 0);
    s.edit([{ op: 'set', path: `/spending/items/${i}/amount`, value: Math.round(s.profile.spending.items[i].amount * 1.25) }], { label: 'Spend more on one line' });
  });
  await settled(page);
  const after = await state(page);
  expect(after.success !== before.success, 'the edit should change the result');
  const LEADS = { Overview: 'outcome-sentence', Outcomes: 'range-sentence', 'Cash flow': 'cash-flow-lead', 'What matters': 'lead-sentence', Scenarios: 'scenarios-lead', 'Life stories': 'life-headline', Assumptions: 'assumptions-sentence' };
  for (const [name, testId] of Object.entries(LEADS)) {
    await tab(page, name);
    await page.getByTestId(testId).waitFor({ timeout: 120_000 });
    expect((await page.getByText('Updating…').count()) === 0 || name === 'What matters', `${name} still shows the old picture`);
    const blank = await page.evaluate(() => [...document.querySelectorAll('main canvas[data-chart-layer]')].filter((c) => c.width > 0 && c.height > 0 && !c.getContext('2d').getImageData(0, 0, c.width, c.height).data.some((v, i) => i % 4 === 3 && v > 0)).length);
    expect(blank === 0, `${name}: ${blank} blank chart canvas(es) after the edit`);
  }
  ok('an edit refines all seven views (each renders its lead from the new run, no blank chart)');
  await page.keyboard.press('ControlOrMeta+z');
  await settled(page);
  expect(Math.abs((await state(page)).success - before.success) < 1e-12, 'undo should bring back the same result');
  ok('undoing the edit brings back the identical result (same seed, same lives)');

  // ---- scenarios from the tornado, the map, the solver and the quick buttons ----
  await tab(page, 'What matters');
  await page.getByTestId('lead-sentence').waitFor({ timeout: 120_000 });
  const win = page.getByTestId('wins').getByRole('button', { name: 'Try this change' }).first();
  await win.waitFor({ timeout: 120_000 });
  await win.click();
  const row = page.locator('[data-chart-card^="what-moves"] [role="button"], [data-chart-card^="what-moves"] button[aria-label]').first();
  await page.waitForFunction(() => window.__firesight.plan.getState().scenarios.length === 1);
  ok('a scenario is created from the biggest win');

  // The map: wait for cells, pick one that is not the plan, adopt it.
  const map = page.locator('[data-chart-card="trade-off-map"]');
  await map.scrollIntoViewIfNeeded();
  const cursor = map.locator('[data-chart-cursor]');
  await cursor.waitFor({ timeout: 240_000 });
  // Cells of the coarse pass sit on every other node: wait for the fine pass so any cell can be chosen.
  await page.waitForFunction(() => { const t = document.querySelector('[data-chart-card="trade-off-map"]')?.textContent ?? ''; return /lives per cell/.test(t) && !/coarse pass/.test(t); }, null, { timeout: 300_000 });
  await cursor.focus();
  for (const key of ['ArrowRight', 'ArrowRight', 'ArrowUp', 'Enter']) await page.keyboard.press(key);
  await page.getByTestId('cell-detail').waitFor({ timeout: 30_000 });
  await page.getByTestId('cell-detail').getByRole('button', { name: 'Adopt as a scenario' }).click();
  await page.waitForFunction(() => window.__firesight.plan.getState().scenarios.length === 2);
  ok('a scenario is adopted from a cell of the trade-off map');

  // The solver.
  const form = page.getByTestId('solver-form');
  await form.scrollIntoViewIfNeeded();
  await form.getByRole('button', { name: 'Find it' }).click();
  const apply = page.getByRole('button', { name: /^Apply .* as a scenario$/ });
  await apply.waitFor({ timeout: 240_000 });
  await apply.click();
  await page.waitForFunction(() => window.__firesight.plan.getState().scenarios.length === 3);
  ok('a scenario is applied from the solver’s answer');

  await tab(page, 'Scenarios');
  const quick = page.locator('[data-testid^="quick-"]').first();
  await quick.scrollIntoViewIfNeeded();
  await quick.click();
  await page.waitForFunction(() => window.__firesight.plan.getState().scenarios.length === 4);
  await settled(page);
  const made = await state(page);
  expect(made.ran.length === 4 && made.ran.every((s) => s.paired), `all four scenarios should be simulated and paired: ${JSON.stringify(made.ran)}`);
  expect((await page.locator('[data-testid^="scenario-"]').filter({ has: page.getByLabel('Scenario name') }).count()) === 4, 'the bench should list four scenarios');
  await page.getByTestId('scenario-detail').waitFor();
  await page.getByTestId('flip-sentence').waitFor();
  ok('all four appear in Scenarios, simulated on the same lives, with paired results');
  void row;

  // ---- a rename reaches the run without a new simulation ----
  const runBefore = await page.evaluate(() => window.__firesight.run.getState().summary);
  await page.evaluate(() => {
    const s = window.__firesight.plan.getState();
    s.updateScenario(s.scenarios[0].id, { name: 'Renamed in the journey' });
  });
  await page.waitForFunction(() => window.__firesight.run.getState().scenarios[0]?.name === 'Renamed in the journey');
  expect(await page.evaluate((b) => window.__firesight.run.getState().status === 'done' && window.__firesight.run.getState().summary !== null && b !== null, runBefore), 'a rename must not start a new run');
  expect((await page.evaluate(() => window.__firesight.run.getState().scenarios[0].paired)) !== null, 'the paired result should survive a rename');
  ok('a scenario rename reaches the run’s names (and every view reading them) with no re-run');

  // ---- undo and redo across scenario operations ----
  const s0 = await state(page);
  const firstId = s0.scenarios[0].id;
  await page.evaluate((id) => window.__firesight.plan.getState().promoteScenario(id), firstId);
  await settled(page);
  let s1 = await state(page);
  expect(s1.scenarios.length === 3 && s1.past === s0.past + 1, 'promote is one history step');
  await page.evaluate(() => window.__firesight.plan.getState().removeScenario(window.__firesight.plan.getState().scenarios[0].id));
  await page.keyboard.press('ControlOrMeta+z');
  await page.keyboard.press('ControlOrMeta+z');
  await settled(page);
  s1 = await state(page);
  expect(JSON.stringify(s1.scenarios) === JSON.stringify(s0.scenarios) && Math.abs(s1.success - s0.success) < 1e-12, 'two undos restore the scenarios and the plan exactly');
  await page.keyboard.press('ControlOrMeta+Shift+z');
  await settled(page);
  s1 = await state(page);
  expect(s1.scenarios.length === 3 && !s1.scenarios.some((x) => x.id === firstId), 'redo makes the scenario the plan again');
  await page.keyboard.press('ControlOrMeta+z');
  await settled(page);
  ok('promote is one undo step; undo and redo walk back and forth across delete, promote and the plan');

  // ---- named lives: the fan's links open that life ----
  await tab(page, 'Overview');
  const pick = await page.evaluate(() => window.__firesight.run.getState().summary.picks.find((p) => p.label === 'p10').path_index);
  expect(await page.evaluate(() => window.__firesight.run.getState().summary.pick_paths.length >= 3), 'the summary carries the named lives’ paths');
  await page.getByRole('button', { name: 'an unlucky life' }).click();
  await page.getByTestId('life-headline').waitFor({ timeout: 60_000 });
  expect(new URL(page.url()).searchParams.get('path') === String(pick) && new URL(page.url()).searchParams.get('why') === 'p10', `the unlucky life should open: ${page.url()}`);
  ok(`the fan’s “unlucky life” (drawn from pick_paths, no trace) opens life #${pick} in Life stories`);

  // ---- deleting a household deletes its conversation ----
  const id = await page.evaluate(() => window.__firesight.plan.getState().meta.id);
  const conversation = (hid) =>
    page.evaluate(
      (hid) =>
        new Promise((done) => {
          const open = indexedDB.open('firesight-assistant');
          open.onupgradeneeded = () => open.result.createObjectStore('conversations');
          open.onsuccess = () => {
            const db = open.result;
            if (!db.objectStoreNames.contains('conversations')) return done(false);
            const get = db.transaction('conversations').objectStore('conversations').get(hid);
            get.onsuccess = () => done(get.result !== undefined);
            get.onerror = () => done(false);
          };
          open.onerror = () => done(false);
        }),
      hid,
    );
  await page.evaluate(
    (hid) =>
      new Promise((done) => {
        const open = indexedDB.open('firesight-assistant');
        open.onupgradeneeded = () => open.result.createObjectStore('conversations');
        open.onsuccess = () => {
          const tx = open.result.transaction('conversations', 'readwrite');
          tx.objectStore('conversations').put({ items: [], history: [], marker: 'journey' }, hid);
          tx.oncomplete = () => done(true);
        };
      }),
    id,
  );
  expect(await conversation(id), 'the test conversation should be stored');
  page.once('dialog', (d) => void d.accept());
  await page.getByRole('button', { name: /Dual-income family/ }).first().click();
  await page.getByRole('button', { name: /^Delete Dual-income family/ }).first().click({ force: true });
  await page.getByRole('heading', { name: /When can work become optional/ }).waitFor();
  await page.waitForTimeout(500);
  expect(!(await conversation(id)), 'the conversation should be deleted with its household');
  ok('deleting a household deletes its assistant conversation');

  // ---- v1 imports ----
  for (const slug of ['dual-income-family', 'early-career-saver', 'side-hustle-landlord']) {
    await page.getByRole('button', { name: 'Import a plan' }).click();
    await page.getByLabel('Plan file').setInputFiles(resolve(here, `../../examples/v1/${slug}.json`));
    await page.getByRole('heading', { name: 'Converted from version 1' }).waitFor();
    await page.getByRole('button', { name: 'Review the plan' }).click();
    await settled(page);
    const run = await page.evaluate(() => ({ errors: window.__firesight.plan.getState().report.errors.length, paths: window.__firesight.run.getState().summary?.paths }));
    expect(run.errors === 0 && run.paths > 0, `v1 ${slug} should import and simulate: ${JSON.stringify(run)}`);
    await tab(page, 'Overview');
    await page.getByTestId('outcome-sentence').waitFor();
    // Back to the landing page through the household switcher.
    await page.locator('header').getByRole('button').filter({ hasText: /\S/ }).first().click();
    await page.getByRole('button', { name: 'New', exact: true }).click();
    await page.getByRole('heading', { name: /When can work become optional/ }).waitFor();
  }
  ok('the three version 1 example plans import, convert and simulate');

  // ---- startup equity: the tab, the zero comparison, the what-if in Scenarios ----
  await page.getByTestId('demo-startup-employee').click();
  await settled(page);
  const equityNote = await page.getByTestId('headline-equity').innerText();
  expect(/^and in \d+ of 100 of the lives where Northwind Robotics fails$/.test(equityNote), `the pinned success figure should say what happens if the company fails: ${equityNote}`);
  const card = await page.getByTestId('overview-equity').innerText();
  // "No cash" is the gross count, with the lives that end out of pocket named beside it.
  expect(/If Northwind Robotics fails the plan holds in \d+ of 100 of those lives; if it is sold or listed, in \d+\. The equity pays out no cash in \d+ of 100 lives, and in \d+ it costs more than it returns\./.test(card), `the Overview card: ${card}`);
  await page.getByTestId('overview-equity').click();
  await page.getByTestId('equity-lead').waitFor();
  expect(new URL(page.url()).pathname === '/equity', 'the Overview card opens the Equity view');
  // The standing comparison runs by itself, on the same lives, once the plan's own run is done.
  await page.getByTestId('equity-with-without').filter({ hasText: 'of the same lives' }).waitFor({ timeout: 120_000 });
  const withWithout = await page.getByTestId('equity-with-without').innerText();
  // The figure gets a decimal when the two sides would otherwise read the same; it is still the pinned strip's figure.
  const holds = Math.round(Number(/holds in ([\d.]+) of 100 lives with the equity as modelled/.exec(withWithout)?.[1]));
  const headlineHolds = Number(/(\d+)\s*of 100/.exec(await page.getByTestId('headline-success').innerText())?.[1]);
  expect(holds === headlineHolds, `the Equity view and the pinned strip give one answer: ${holds} against ${headlineHolds}`);
  expect(/with it left out \(difference /.test(withWithout) && !/The equity is the difference between holding and not/.test(withWithout), `the comparison should not credit the payout: ${withWithout}`);
  const pays = await page.getByTestId('equity-pays').innerText();
  expect(/^No cash ever comes back in \d+ of 100 lives\. After the cost of exercising and the tax it caused, the household /.test(pays) && !/comes to nothing/.test(pays), `"nothing" should mean one thing on the page: ${pays}`);
  expect(/judgements anchored to published figures/.test(await page.getByTestId('equity-odds-note').innerText()), 'the page should say the default odds are judgement, not a measured rate');
  // Exercising now is capped by what the household can pay: the row says how much of it the plan really makes.
  await page.getByTestId('exercise-shortfall').waitFor({ timeout: 60_000 });
  expect(/^The plan can pay for [\d,]+ of the 58,000 vested options: /.test(await page.getByTestId('exercise-shortfall').innerText()), 'the exercise-now row should state what is really exercised');
  expect((await page.getByTestId('decision-sentence').count()) === 0, 'no decision is compared until asked');
  await page.getByTestId('compare-equity_exercise_now').click();
  await page.getByTestId('decision-sentence').first().waitFor({ timeout: 120_000 });
  expect(/^Exercising now /.test(await page.getByTestId('decision-sentence').first().innerText()), 'the decision reads as a sentence by outcome');
  ok(`the startup demo shows the Equity tab, the note under the success figure and the comparison with the equity at zero (${holds} of 100 as modelled)`);

  await tab(page, 'Scenarios');
  await page.getByTestId('quick-equity_zero').click();
  await settled(page);
  const zero = (await state(page)).ran.find((s) => s.name === 'Equity left out');
  expect(zero?.paired, 'counting the equity at zero should run as a paired scenario');
  await page.getByTestId('flip-sentence').waitFor();
  await page.getByTestId('scenario-by-outcome').waitFor();
  expect(/Northwind Robotics/.test(await page.getByTestId('scenario-by-outcome').innerText()), 'the paired difference is broken down by what happens to the company');
  ok('“Count the equity at zero” in Scenarios gives a paired difference, by what happens to the company');

  // The household's own odds: a count the remainder cannot cover is refused whole, never cut down to its first digit.
  {
    const equity = page.locator('[data-section="equity"]');
    if ((await equity.getByRole('button', { name: 'Use my own odds' }).count()) === 0) {
      await equity.locator('h3 > button').click();
      await equity.locator('li[data-path^="/private_equity/"]').first().locator('button[aria-expanded]').first().click();
    }
    await equity.getByRole('button', { name: 'Use my own odds', exact: true }).click();
    const stored = () => page.evaluate(() => window.__firesight.plan.getState().profile.private_equity[0].outcomes.find((r) => r.kind === 'fails').probability);
    const seeded = await page.evaluate(() => window.__firesight.plan.getState().profile.private_equity[0].outcomes.map((r) => [r.multiple_low, r.multiple_high]));
    expect(seeded.flat().every((m) => m === Number(m.toPrecision(2))), `seeded price multiples should have two significant figures: ${JSON.stringify(seeded)}`);
    expect((await stored()) === 0.3, 'the demo company seeds "shuts down" at 30 of 100');
    const fails = equity.getByRole('textbox', { name: 'Shuts down, futures of 100', exact: true });
    await fails.click();
    await page.keyboard.press('ControlOrMeta+A');
    await page.keyboard.type('7', { delay: 50 });
    expect((await stored()) === 0.3, 'a digit typed on the way to a value must not be written');
    await page.keyboard.type('0', { delay: 50 });
    await page.keyboard.press('Tab');
    await page.waitForTimeout(300);
    expect((await stored()) === 0.3, `typing 70 where 61 fit must leave the odds as they were, got ${await stored()}`);
    expect((await fails.inputValue()) === '30', `the field should show what the row still holds, got ${await fails.inputValue()}`);
    await equity.getByText('You typed 70; only 61 fit, so this stays at 30. Lower another row first.').waitFor();
    await fails.click();
    await page.keyboard.press('ControlOrMeta+A');
    await page.keyboard.type('45', { delay: 50 });
    await page.keyboard.press('Enter');
    await page.waitForTimeout(300);
    expect((await stored()) === 0.45, 'an entry that fits is written when the field is left');
    // A low price raised above the high one moves the high one with it, in one edit.
    const low = equity.getByRole('textbox', { name: 'Is acquired: low price', exact: true });
    await low.click();
    await page.keyboard.press('ControlOrMeta+A');
    await page.keyboard.type('30', { delay: 50 });
    await page.keyboard.press('Tab');
    await page.waitForTimeout(300);
    const acquired = await page.evaluate(() => window.__firesight.plan.getState().profile.private_equity[0].outcomes.find((r) => r.kind === 'acquired'));
    expect(acquired.multiple_low === 30 && acquired.multiple_high === 30, `the price range should stay in order: ${JSON.stringify(acquired)}`);
    expect((await page.evaluate(() => window.__firesight.plan.getState().report.errors.length)) === 0, 'an ordered pair is a valid plan');
    await equity.getByRole('button', { name: 'Back to typical odds', exact: true }).click();
    await settled(page);
    ok('a typed count that does not fit is refused whole with a message, and a price range stays in order');
  }

  // Leaving the equity out of the plan itself: the view says so and offers to count it again.
  await page.evaluate(() => window.__firesight.plan.getState().edit([{ op: 'set', path: '/private_equity/0/included', value: false }], { label: 'Leave the equity out', target: 'base' }));
  await settled(page);
  await tab(page, 'Equity');
  await page.getByText('The equity is not counted in this plan.').first().waitFor();
  expect((await page.getByTestId('headline-equity').count()) === 0 && (await page.getByTestId('equity-pays').count()) === 0, 'zeros are not shown as results when the equity is left out');
  await page.getByTestId('equity-count').click();
  await settled(page);
  await page.getByTestId('equity-pays').waitFor();
  ok('a plan that leaves the equity out says so, hides the dollar panels and can count it again');
  await page.locator('header').getByRole('button').filter({ hasText: /\S/ }).first().click();
  await page.getByRole('button', { name: 'New', exact: true }).click();
  await page.getByRole('heading', { name: /When can work become optional/ }).waitFor();

  // ---- the landing hand-off ----
  await page.getByLabel('Describe your situation').fill('We are 38 and 41 in Denver with two kids and about $210k between us.');
  await page.getByRole('button', { name: 'Hand this to the assistant' }).click();
  const panel = page.getByRole('complementary', { name: 'Assistant' });
  await panel.getByText('We are 38 and 41 in Denver').first().waitFor({ timeout: 30_000 });
  expect((await page.getByLabel('Describe your situation').count()) === 0, 'the hand-off should open a plan');
  ok('the landing description opens a blank plan and arrives in the assistant as its first message');

  await context.close();
} catch (error) {
  problems.push(error instanceof Error ? (error.stack ?? error.message) : String(error));
}
await browser.close();
if (problems.length) {
  console.log(`\njourneys FAILED after ${passed} checks:\n` + [...new Set(problems)].join('\n'));
  process.exitCode = 1;
} else {
  console.log(`\njourneys: ${passed} checks passed, console clean.`);
}
