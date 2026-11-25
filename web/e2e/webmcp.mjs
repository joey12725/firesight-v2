// End-to-end check of the in-page WebMCP tools, against a running dev or preview server:
//   BASE_URL=http://localhost:5173 npm run webmcp
//   WEBMCP_BROWSER=chromium BASE_URL=… npm run webmcp     (Playwright's Chromium instead of the installed Chrome)
// Launches Chrome with WebMCP switched on (--enable-features=WebMCP) and plays the part
// of a browser agent through the browser's own API: document.modelContext.getTools()
// and executeTool(). One journey: read the app's state, create a plan from JSON, be
// stopped by the consent prompt until the user allows it, propose an edit and have the
// user accept it on a card, simulate, read the results, put a view in front of the user
// and read a chart off it. Then the edges: decline, abort, auto-apply, undo, access off,
// "Always allow" across a reload, and a browser without WebMCP (nothing loaded, nothing
// logged).
//
// If the browser has no document.modelContext even with the flag (a Chromium too old, or
// a browser that is not Chromium), the script says so and exits 0: there is nothing to test.
import { chromium } from 'playwright';

const base = process.env.BASE_URL ?? 'http://localhost:5173';
const ARGS = ['--enable-features=WebMCP'];
// The installed Chrome by default, else the Chromium that Playwright downloads. WEBMCP_BROWSER=chromium picks the latter outright.
const launch = (args) => (process.env.WEBMCP_BROWSER === 'chromium' ? chromium.launch({ args }) : chromium.launch({ channel: 'chrome', args }).catch(() => chromium.launch({ args })));

const problems = [];
/** Lines the browser logged about a rejected tool call. Only an aborted call may cause one. */
const browserNotices = [];
let passed = 0;
const ok = (name) => {
  passed += 1;
  console.log(`  ok  ${name}`);
};
const expect = (condition, message) => {
  if (!condition) throw new Error(message);
};

function watch(page, tag) {
  // The dev server has no /api (the hosted assistant probe 404s there); everything else must be clean.
  // Chrome itself logs "WebMCP tool execution failed" when a tool rejects, which ours do only when the caller aborts.
  page.on('console', (m) => {
    if (m.type() !== 'error' && m.type() !== 'warning') return;
    if (/status of 404/.test(m.text())) return;
    if (/WebMCP tool execution failed/.test(m.text())) return void browserNotices.push(m.text());
    problems.push(`${tag} console ${m.type()}: ${m.text().slice(0, 300)}`);
  });
  page.on('pageerror', (e) => problems.push(`${tag} pageerror: ${e.message}`));
}

/** The tools the page has registered, as the browser reports them. */
const listTools = (page) =>
  page.evaluate(async () => {
    const tools = await document.modelContext.getTools();
    return tools.filter((t) => t.window === undefined || t.window === null || t.window === window).map((t) => ({ name: t.name, title: t.title, description: t.description, inputSchema: typeof t.inputSchema === 'string' ? JSON.parse(t.inputSchema) : t.inputSchema, annotations: t.annotations ?? null }));
  });

/**
 * Starts a tool call in the page the way an agent's host would, and returns at once with
 * an id; `finish` collects the result. Results are decoded per the consumer notes: null
 * is a navigation, anything else is JSON.
 */
async function begin(page, name, input = {}, { abortable = false } = {}) {
  return page.evaluate(
    async ({ name, input, abortable }) => {
      const calls = (window.__webmcpCalls ??= new Map());
      const id = `${name}#${calls.size + 1}`;
      const tool = (await document.modelContext.getTools()).find((t) => t.name === name);
      if (!tool) throw new Error(`no tool named ${name}`);
      const controller = abortable ? new AbortController() : null;
      const options = controller ? { signal: controller.signal } : undefined;
      const run = () =>
        document.modelContext.executeTool(tool, input, options).catch((error) => {
          // Chrome 149 to 154 took the input as a JSON string.
          if (/Failed to parse input/.test(String(error?.message))) return document.modelContext.executeTool(tool, JSON.stringify(input), options);
          throw error;
        });
      const entry = { controller, settled: null };
      entry.promise = run().then(
        (text) => (entry.settled = { text }),
        (error) => (entry.settled = { thrown: { name: error?.name ?? 'Error', message: String(error?.message ?? error) } }),
      );
      calls.set(id, entry);
      return id;
    },
    { name, input, abortable },
  );
}

async function finish(page, id, timeout = 120_000) {
  await page.waitForFunction((id) => window.__webmcpCalls.get(id).settled !== null, id, { timeout });
  const settled = await page.evaluate((id) => window.__webmcpCalls.get(id).settled, id);
  if (settled.thrown) return { thrown: settled.thrown };
  if (settled.text === null) return { navigated: true };
  try {
    return JSON.parse(settled.text);
  } catch {
    return { raw: settled.text };
  }
}

const abort = (page, id) => page.evaluate((id) => window.__webmcpCalls.get(id).controller.abort(), id);
const call = async (page, name, input, timeout) => finish(page, await begin(page, name, input), timeout);
/** A call that must succeed. */
async function good(page, name, input, timeout) {
  const result = await call(page, name, input, timeout);
  expect(result.ok === true, `${name} should succeed: ${JSON.stringify(result).slice(0, 500)}`);
  return result;
}
const plan = (page) =>
  page.evaluate(() => {
    const s = window.__firesight.plan.getState();
    return { id: s.meta?.id ?? null, name: s.profile?.name ?? null, bequest: s.profile?.retirement.bequest_target ?? null, past: s.past.map((p) => p.label), scenarios: s.scenarios.map((x) => x.id), errors: s.report.errors.length };
  });
const settings = (page) => page.evaluate(() => ({ access: window.__firesight.settings.getState().agentAccess, autoApply: window.__firesight.settings.getState().agentAutoApply }));
const waitForTools = (page) => page.waitForFunction(async () => (await document.modelContext.getTools()).length >= 20, null, { timeout: 30_000 });

const LENA = {
  household: { people: [{ id: 'lena', name: 'Lena', birth: '1990-04' }], location: { country: 'US', state: 'TX', city: 'Austin' } },
  incomes: [{ id: 'lena_job', name: 'Product designer', owner: 0, kind: { type: 'job', salary: 128000, plan: { employee_fraction: 0.08, match_rate: 0.5, match_up_to: 0.06 } } }],
  spending: { items: [{ id: 'living', name: 'Living costs', category: 'other', amount: 4200, frequency: 'monthly' }] },
  accounts: [{ id: 'lena_401k', name: '401(k)', owner: 0, kind: 'traditional', balance: 96000 }],
};

const browser = await launch(ARGS);
const version = browser.version();
let skipped = false;

try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  watch(page, 'main');
  await page.goto(base);
  await page.getByRole('heading', { name: /When can work become optional/ }).waitFor();

  if (!(await page.evaluate(() => typeof document.modelContext?.registerTool === 'function' && typeof document.modelContext?.getTools === 'function'))) {
    skipped = true;
    console.log(`\nwebmcp: SKIPPED. ${version} has no document.modelContext with --enable-features=WebMCP, so there is nothing to test here. Use Chrome 150 or later.`);
  } else {
    console.log(`webmcp: testing in Chrome ${version}`);

    // ---- registration ----
    await waitForTools(page);
    const tools = await listTools(page);
    const names = tools.map((t) => t.name);
    expect(names.length === 24 && JSON.stringify(names) === JSON.stringify([...names].sort()), `24 tools, sorted by name: ${names.join(', ')}`);
    for (const required of ['get_app_state', 'get_guide', 'create_plan', 'edit_plan', 'get_results', 'show_view', 'list_charts', 'get_chart_data', 'run_simulation', 'try_scenario', 'undo_agent_edit']) expect(names.includes(required), `tool ${required} is registered`);
    expect(!names.includes('show_chart') && !names.includes('propose_changes'), 'the assistant-only tools are not offered');
    const edit = tools.find((t) => t.name === 'edit_plan');
    expect(edit.description.length > 200 && edit.inputSchema?.properties?.ops && edit.inputSchema.required.includes('summary'), 'edit_plan carries its description and input schema');
    if (edit.annotations) {
      expect(edit.annotations.readOnlyHint === false, 'edit_plan is not read-only');
      expect(tools.find((t) => t.name === 'get_results').annotations.readOnlyHint === true, 'get_results is read-only');
    }
    ok(`the page registers its ${names.length} tools with the browser, sorted, with schemas and annotations`);

    // ---- state, before the user has allowed anything ----
    const first = await good(page, 'get_app_state');
    expect(first.ready === true && first.plan === null && first.agent_access.status === 'will_ask_user' && first.demos.length >= 3 && first.view.available.length === 7, `initial state: ${JSON.stringify(first).slice(0, 400)}`);
    const guide = await good(page, 'get_guide');
    expect(guide.guide.includes('must come from a tool result') && guide.guide.length < 8000, 'the guide carries the grounding rule');
    const person = await good(page, 'get_schema', { section: 'Person' });
    expect(person.name === 'Person' && person.fields.some((f) => f.name === 'birth'), 'get_schema answers from the engine');
    expect((await page.getByTestId('agent-consent').count()) === 0, 'state, guide and schema do not ask the user for anything');
    ok('get_app_state, get_guide and get_schema answer without touching the plan or prompting the user');

    const bad = await call(page, 'create_plan', { profile: 'nope' });
    expect(bad.ok === false && bad.error.code === 'invalid_input' && (await page.getByTestId('agent-consent').count()) === 0, `invalid input is an error object, not a throw or a prompt: ${JSON.stringify(bad)}`);
    ok('input is validated in the page: a malformed call resolves { ok: false, error } and the user is not bothered');

    // ---- create a plan: stopped by the consent prompt ----
    const creating = await begin(page, 'create_plan', { profile: LENA, name: 'Lena (from an agent)' });
    const consent = page.getByTestId('agent-consent');
    await consent.waitFor();
    expect((await plan(page)).id === null, 'nothing is created while the prompt is unanswered');
    expect((await page.locator('dialog[open]').count()) === 0, 'the consent prompt is not a modal dialog');
    await page.getByTestId('agent-pill').waitFor();
    expect(/waiting for you/.test(await page.getByTestId('agent-pill').getAttribute('aria-label')), 'the top bar says an agent is waiting');
    // The page behind the prompt still works: open and close Settings.
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await page.getByRole('heading', { name: 'Browser agents' }).waitFor();
    expect(await page.getByTestId('agent-consent').isVisible(), 'the prompt stays reachable while a dialog is open');
    await page.keyboard.press('Escape');
    await consent.getByRole('button', { name: 'Allow for this session' }).click();
    const created = await finish(page, creating);
    expect(created.ok === true && created.plan.name === 'Lena (from an agent)' && created.validation.can_run === true, `create_plan: ${JSON.stringify(created).slice(0, 500)}`);
    await consent.waitFor({ state: 'detached' });
    await page.getByTestId('headline').waitFor({ timeout: 120_000 });
    const opened = await plan(page);
    expect(opened.id === created.plan.id && opened.name === 'Lena (from an agent)' && opened.errors === 0, `the plan is open in the app: ${JSON.stringify(opened)}`);
    await page.locator('header').getByRole('button', { name: /Lena \(from an agent\)/ }).waitFor();
    ok('create_plan waits on a non-modal consent prompt; once allowed, the plan built from JSON opens on the site');

    // ---- results of the app's own run ----
    const results = await good(page, 'get_results', {}, 240_000);
    const shown = await page.evaluate(() => {
      const run = window.__firesight.run.getState();
      return { status: run.status, stale: run.stale, lives: run.summary.paths, success: run.summary.headline.success_rate.value, homeSale: run.summary.headline.home_sale_rate.value };
    });
    expect(results.complete === true && shown.status === 'done' && !shown.stale, `get_results waits for the run to finish: ${JSON.stringify({ complete: results.complete, shown })}`);
    expect(results.lives === shown.lives && Math.abs(results.success.p - shown.success) < 0.0006 && Math.abs(results.home_sale - shown.homeSale) < 0.0006, `the figures are the ones on screen: ${JSON.stringify({ tool: results.success, shown })}`);
    expect(results.plan_facts.annual_spending > 0 && JSON.stringify(results).length < 4000 && !('headline' in results), 'the result is compact, never the raw Summary');
    ok(`get_results returns the headline figures on screen (${results.lives.toLocaleString('en-US')} lives, holds in ${Math.round(results.success.p * 100)} of 100)`);

    const state = await good(page, 'get_app_state');
    expect(state.plan.id === created.plan.id && state.agent_access.status === 'allowed' && state.saved_plans.some((p) => p.id === created.plan.id) && state.run.status === 'done', `state after creating: ${JSON.stringify(state).slice(0, 500)}`);
    const read = await good(page, 'get_plan');
    expect(read.plan.incomes[0].id === 'lena_job' && read.validation.can_run === true && JSON.stringify(read).length < 16_000, 'get_plan returns the plan with ids, pruned of defaults');
    ok('once allowed, get_app_state names the plan and lists the saved ones, and get_plan reads it');

    // ---- an edit, approved on a card ----
    const before = await plan(page);
    const editing = await begin(page, 'edit_plan', {
      summary: 'Raise the 401(k) contribution to 12%',
      ops: [{ op: 'set', path: '/incomes/@lena_job/kind/plan/employee_fraction', value: 0.12 }],
      assumptions: [{ field: 'Contribution rate', value: '12% of salary', basis: 'stated', confidence: 'high' }],
    });
    const card = page.getByTestId('agent-approval');
    await card.waitFor();
    await card.getByText('Raise the 401(k) contribution to 12%').waitFor();
    expect((await card.getByText('Proposed change').count()) === 1 && (await card.getByText('edit_plan').count()) >= 1, 'the card says what is proposed and which tool asked');
    expect((await plan(page)).past.length === before.past.length, 'nothing changes until the user accepts');
    await card.getByRole('button', { name: 'Accept', exact: true }).click();
    const edited = await finish(page, editing);
    expect(edited.ok === true && edited.status === 'accepted' && edited.applied_ops === 1, `edit_plan: ${JSON.stringify(edited)}`);
    await card.waitFor({ state: 'detached' });
    const after = await plan(page);
    expect(after.past.at(-1) === 'Agent: Raise the 401(k) contribution to 12%', `the edit is one undo step labelled as the agent's: ${JSON.stringify(after.past)}`);
    expect((await page.evaluate(() => window.__firesight.plan.getState().profile.incomes[0].kind.plan.employee_fraction)) === 0.12, 'the plan carries the accepted value');
    ok('edit_plan shows a change card with no assistant panel and no API key; accepting applies it as one "Agent: …" undo step');

    const declining = await begin(page, 'edit_plan', { summary: 'Stop saving altogether', ops: [{ op: 'set', path: '/incomes/@lena_job/kind/plan/employee_fraction', value: 0 }] });
    await card.waitFor();
    await card.getByRole('button', { name: 'Decline' }).click();
    const declined = await finish(page, declining);
    expect(declined.ok === false && declined.error.code === 'declined' && (await plan(page)).past.length === after.past.length, `a declined edit leaves the plan alone: ${JSON.stringify(declined)}`);
    ok('declining a card returns { ok: false, error: { code: "declined" } } and changes nothing');

    // ---- abort while the user is being asked ----
    const aborting = await begin(page, 'edit_plan', { summary: 'An edit nobody waits for', ops: [{ op: 'set', path: '/retirement/bequest_target', value: 123456 }] }, { abortable: true });
    await card.waitFor();
    await abort(page, aborting);
    const aborted = await finish(page, aborting);
    expect(Boolean(aborted.thrown), `an aborted call rejects: ${JSON.stringify(aborted)}`);
    expect(browserNotices.length <= 1, `only the aborted call rejected: ${browserNotices.join(' | ')}`);
    await card.waitFor({ state: 'detached' });
    expect((await plan(page)).bequest !== 123456, 'the aborted edit was not applied');
    ok(`aborting a call that is waiting for the user rejects it (${aborted.thrown.name}) and withdraws the card`);

    // ---- run, compare, results after the edit ----
    const rerun = await good(page, 'get_results', {}, 240_000);
    expect(rerun.complete === true && rerun.lives === results.lives, 'the page re-simulated the edited plan');
    const digest = await good(page, 'run_simulation', { paths: 1000 }, 180_000);
    expect(digest.run_id && digest.paths === 1000 && digest.success.p >= 0 && digest.success.p <= 1 && digest.trace_paths, `run_simulation digest: ${JSON.stringify(digest).slice(0, 300)}`);
    const whatIf = await good(page, 'try_scenario', { name: 'Spend less', ops: [{ op: 'set', path: '/spending/items/@living/amount', value: 3600 }], paths: 1000 }, 180_000);
    expect(whatIf.comparison.paired === true && whatIf.comparison.paired_paths === 1000 && whatIf.base_run_id === digest.run_id && !('saved_scenario' in whatIf), `try_scenario pairs with the same lives and reuses the base run: ${JSON.stringify(whatIf).slice(0, 300)}`);
    expect((await plan(page)).scenarios.length === 0, 'a what-if leaves the plan and its scenarios alone');
    const life = await good(page, 'inspect_life', { run_id: digest.run_id, pick: 'median' }, 60_000);
    expect(life.table.year.length > 10 && Number.isInteger(life.path), 'inspect_life follows one life year by year');
    ok('run_simulation, try_scenario and inspect_life run on the worker pool from an outside call, paired on the same lives');

    // ---- show a view, read its charts ----
    const view = await good(page, 'show_view', { view: 'outcomes' });
    expect(view.view === 'outcomes' && new URL(page.url()).pathname === '/outcomes', `show_view navigates the visible app: ${page.url()}`);
    expect((await page.getByRole('tab', { name: 'Outcomes', exact: true }).getAttribute('aria-selected')) === 'true', 'the Outcomes tab is selected');
    const charts = await good(page, 'list_charts', {}, 60_000);
    expect(charts.view === 'outcomes' && charts.charts.length >= 1, `list_charts lists the charts on screen: ${JSON.stringify(charts).slice(0, 300)}`);
    const onScreen = await page.locator('[data-chart-card]').count();
    expect(charts.charts.length === onScreen, `every chart card on screen is listed (${charts.charts.length} of ${onScreen})`);
    const withWords = charts.charts.find((c) => c.takeaway);
    expect(Boolean(withWords), 'at least one chart carries its takeaway sentence');
    expect((await page.locator('[data-chart-takeaway]').allInnerTexts()).some((t) => t.replace(/\s+/g, ' ').trim() === withWords.takeaway), `the takeaway is the sentence on the page: ${withWords.takeaway}`);
    // A chart that supports another has data but no toolbar of its own; compare against one that shows the toggle.
    let tabular = null;
    for (const c of charts.charts.filter((x) => x.has_table)) {
      if ((await page.locator(`[data-chart-card="${c.id}"]`).first().getByRole('button', { name: 'View as a table' }).count()) > 0) tabular ??= c;
    }
    expect(Boolean(tabular), 'at least one chart offers its data as a table on the page');
    const data = await good(page, 'get_chart_data', { chart: tabular.id, max_rows: 12 });
    expect(data.columns.length >= 2 && data.rows.length >= 1 && data.rows.length <= 12 && data.rows.every((r) => r.length === data.columns.length) && data.total_rows >= data.rows.length, `get_chart_data returns rows in column order: ${JSON.stringify(data).slice(0, 300)}`);
    // The same table the page offers under "View as a table".
    const section = page.locator(`[data-chart-card="${tabular.id}"]`).first();
    await section.getByRole('button', { name: 'View as a table' }).click();
    const tableRows = await section.locator('tbody tr').count();
    expect(tableRows === data.total_rows, `the tool reads the table the page shows (${data.total_rows} rows, page has ${tableRows})`);
    await section.getByRole('button', { name: 'Show the chart' }).click();
    ok(`show_view puts Outcomes on screen; list_charts reads ${charts.charts.length} charts and get_chart_data the ${data.total_rows} rows behind "${tabular.title}"`);

    const lifeView = await good(page, 'show_view', { life_path: life.path });
    expect(lifeView.view === 'life-stories' && new URL(page.url()).pathname === '/life-stories' && new URL(page.url()).searchParams.get('path') === String(life.path), `show_view opens one life: ${page.url()}`);
    ok('show_view with life_path opens that simulated life in Life stories');

    // ---- auto-apply, scenarios, undo ----
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const section2 = page.getByTestId('agent-settings');
    await section2.getByText(/This browser offers WebMCP, so an AI agent running in it can call FIRESight's 24 tools/).waitFor();
    await section2.getByRole('switch', { name: 'Apply agent changes without asking' }).click();
    await page.keyboard.press('Escape');
    expect((await settings(page)).autoApply === true, 'the auto-apply setting is on');
    const auto = await good(page, 'edit_plan', { summary: 'Leave $50,000 behind', ops: [{ op: 'set', path: '/retirement/bequest_target', value: 50000 }] });
    expect(auto.status === 'accepted' && auto.automatically === true && (await plan(page)).bequest === 50000 && (await card.count()) === 0, `with auto-apply there is no card: ${JSON.stringify(auto)}`);
    await page.getByText('A browser agent changed the plan').waitFor();
    const kept = await good(page, 'try_scenario', { name: 'Spend less', ops: [{ op: 'set', path: '/spending/items/@living/amount', value: 3600 }], paths: 1000, keep: true }, 180_000);
    expect(kept.saved_scenario.saved === true && (await plan(page)).scenarios.includes(kept.saved_scenario.id), `try_scenario with keep saves it: ${JSON.stringify(kept.saved_scenario)}`);
    const listed = await good(page, 'list_scenarios');
    expect(listed.scenarios.length === 1 && listed.scenarios[0].enabled === true, 'list_scenarios shows it');
    const withScenario = await good(page, 'get_results', {}, 240_000);
    expect(withScenario.complete === true && withScenario.scenarios.length === 1 && withScenario.scenarios[0].versus_plan?.paired === true, `get_results carries the scenario with its paired difference: ${JSON.stringify(withScenario.scenarios).slice(0, 300)}`);
    await good(page, 'set_scenario_enabled', { id: kept.saved_scenario.id, enabled: false });
    await good(page, 'remove_scenario', { id: kept.saved_scenario.id });
    expect((await plan(page)).scenarios.length === 0, 'the scenario is removed');
    for (let i = 0; i < 3; i++) await good(page, 'undo_agent_edit');
    expect((await plan(page)).scenarios.length === 0 && (await plan(page)).bequest === 50000, 'three undos take back the delete, the switch and the save');
    const undone = await good(page, 'undo_agent_edit');
    expect(undone.undone === 'Agent: Leave $50,000 behind' && (await plan(page)).bequest !== 50000, `undo_agent_edit reverts the agent's edit: ${JSON.stringify(undone)}`);
    // The user changes something: the agent may not undo that.
    await page.evaluate(() => window.__firesight.plan.getState().edit([{ op: 'set', path: '/retirement/bequest_target', value: 777 }], { label: 'Set by hand', target: 'base' }));
    const refused = await call(page, 'undo_agent_edit');
    expect(refused.ok === false && refused.error.code === 'not_an_agent_edit' && (await plan(page)).bequest === 777, `the user's own edit is left alone: ${JSON.stringify(refused)}`);
    ok('with auto-apply edits land with a notice; scenarios can be kept, switched and removed; undo_agent_edit takes back only the agent\'s own changes');

    // ---- the activity log ----
    await page.getByTestId('agent-pill').click();
    const log = page.getByRole('dialog', { name: 'Browser agent activity' }).getByTestId('agent-activity');
    await log.waitFor();
    const entries = await log.locator('li').allInnerTexts();
    expect(entries.length >= 20 && entries.length <= 50 && /undo_agent_edit/.test(entries[0]) && /Not allowed|Error/.test(entries[0]) && entries.some((e) => /create_plan/.test(e) && /Done/.test(e)), `the activity log lists the calls, newest first: ${entries.slice(0, 3).join(' | ')}`);
    expect(!entries.join(' ').includes('128000') && !entries.join(' ').includes('Lena'), 'the log holds no plan data');
    await page.keyboard.press('Escape');
    ok(`the top bar pill opens an activity log of the last calls (${entries.length} here), with outcomes and no plan data`);

    // ---- access off ----
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await page.getByTestId('agent-settings').getByRole('radio', { name: 'Off' }).click();
    await page.keyboard.press('Escape');
    const off = await call(page, 'get_plan');
    expect(off.ok === false && off.error.code === 'agent_access_off', `access off refuses: ${JSON.stringify(off)}`);
    const offState = await good(page, 'get_app_state');
    expect(offState.agent_access.status === 'off' && !('plan' in offState) && !('saved_plans' in offState), 'get_app_state still answers, with nothing about the plan');
    ok('with agent access off, every tool but get_app_state refuses, and that one says only that access is off');

    // ---- "Always allow" survives a reload; "Ask first" does not carry a session over ----
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await page.getByTestId('agent-settings').getByRole('radio', { name: 'Ask first' }).click();
    await page.keyboard.press('Escape');
    await page.reload();
    await page.getByTestId('headline').waitFor({ timeout: 120_000 });
    await waitForTools(page);
    const reading = await begin(page, 'get_plan');
    await page.getByTestId('agent-consent').waitFor();
    await page.getByTestId('agent-consent').getByRole('button', { name: 'Always allow' }).click();
    expect((await finish(page, reading)).ok === true && (await settings(page)).access === 'allow', 'Always allow answers the call and is saved as the setting');
    await page.reload();
    await page.getByTestId('headline').waitFor({ timeout: 120_000 });
    await waitForTools(page);
    expect((await good(page, 'get_plan')).plan.incomes[0].id === 'lena_job' && (await page.getByTestId('agent-consent').count()) === 0, 'after a reload nothing is asked');
    ok('a new page session asks again under "Ask first"; "Always allow" is remembered across reloads');

    // ---- "Don't allow" ----
    await page.evaluate(() => window.__firesight.settings.getState().set({ agentAccess: 'ask' }));
    await page.reload();
    await page.getByTestId('headline').waitFor({ timeout: 120_000 });
    await waitForTools(page);
    const denying = await begin(page, 'get_plan');
    await page.getByTestId('agent-consent').getByRole('button', { name: "Don't allow" }).click();
    const denied = await finish(page, denying);
    const deniedAgain = await call(page, 'get_results');
    expect(denied.ok === false && denied.error.code === 'agent_access_denied' && deniedAgain.error?.code === 'agent_access_denied' && (await page.getByTestId('agent-consent').count()) === 0, `declining consent refuses for the session without asking again: ${JSON.stringify([denied, deniedAgain])}`);
    ok('"Don\'t allow" refuses plan tools for the rest of the page session and does not ask again');
    expect(browserNotices.length <= 1, `no tool call rejected apart from the aborted one: ${browserNotices.join(' | ')}`);
    await context.close();

    // ---- a phone-width page: the prompt fits ----
    const phone = await browser.newContext({ viewport: { width: 390, height: 844 } });
    const small = await phone.newPage();
    watch(small, 'phone');
    await small.goto(base);
    await small.getByRole('heading', { name: /When can work become optional/ }).waitFor();
    await waitForTools(small);
    const asking = await begin(small, 'open_demo', { slug: 'dual-income-family' });
    const prompt = small.getByTestId('agent-consent');
    await prompt.waitFor();
    const box = await prompt.boundingBox();
    expect(box.x >= 0 && box.x + box.width <= 390 && (await small.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)), `the prompt fits a 390px screen: ${JSON.stringify(box)}`);
    await prompt.getByRole('button', { name: 'Allow for this session' }).click();
    expect((await finish(small, asking)).ok === true, 'open_demo opens the demo on a phone-width page');
    await small.getByTestId('headline').waitFor({ timeout: 120_000 });
    ok('at phone width the consent prompt fits the screen, and open_demo opens a demo plan');
    await phone.close();
  }
} catch (error) {
  problems.push(error instanceof Error ? (error.stack ?? error.message) : String(error));
}
await browser.close();

// ---- a browser without WebMCP: nothing is loaded and nothing is logged ----
if (!skipped && problems.length === 0) {
  // The feature is off by default; switched off explicitly here in case a later Chrome ships it on.
  const plain = await launch(['--disable-features=WebMCP']);
  try {
    const context = await plain.newContext({ viewport: { width: 1440, height: 900 } });
    const page = await context.newPage();
    const messages = [];
    const requests = [];
    page.on('console', (m) => !/status of 404/.test(m.text()) && messages.push(`${m.type()}: ${m.text().slice(0, 200)}`));
    page.on('pageerror', (e) => messages.push(`pageerror: ${e.message}`));
    page.on('request', (r) => requests.push(r.url()));
    await page.goto(base);
    await page.getByRole('heading', { name: /When can work become optional/ }).waitFor();
    if (await page.evaluate(() => 'modelContext' in document || 'modelContext' in navigator)) {
      console.log(`  --  ${plain.version()} offers document.modelContext even with the feature disabled; the no-WebMCP check was not run`);
    } else {
      await page.getByTestId('demo-dual-income-family').click();
      await page.getByTestId('headline').waitFor({ timeout: 120_000 });
      // Vite's dev server logs its own connection lines at debug level; anything else is ours.
      const ours = messages.filter((m) => !/^debug: \[vite\]/.test(m) && !/Download the React DevTools/.test(m));
      expect(ours.length === 0, `no console output without WebMCP: ${ours.join(' | ')}`);
      expect(!requests.some((u) => /webmcp\/(runtime|tools|guide|ui)/.test(u)), `the tools are not downloaded without WebMCP: ${requests.filter((u) => /webmcp/.test(u)).join(', ')}`);
      expect((await page.getByTestId('agent-pill').count()) === 0, 'no agent pill without an agent');
      await page.getByRole('button', { name: 'Settings', exact: true }).click();
      await page.getByTestId('agent-settings').getByText(/This browser does not offer it to this page/).waitFor();
      ok('without WebMCP the app works as before: no console output, the tool code is never downloaded, and Settings says nothing is exposed');
    }
    await context.close();
  } catch (error) {
    problems.push(error instanceof Error ? (error.stack ?? error.message) : String(error));
  }
  await plain.close();
}

if (problems.length) {
  console.log(`\nwebmcp FAILED after ${passed} checks (Chrome ${version}):\n` + [...new Set(problems)].join('\n'));
  process.exitCode = 1;
} else if (!skipped) {
  console.log(`\nwebmcp: ${passed} checks passed in Chrome ${version}, console clean.`);
}
