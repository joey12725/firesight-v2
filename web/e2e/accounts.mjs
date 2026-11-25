// End-to-end check of accounts, synced plans and the agent hand-off, in a real browser
// against the real API:
//   (repository root)  PORT=8787 npm run dev:api          in one terminal
//   (web/)             npm run dev                        in another
//   (web/)             BASE_URL=http://localhost:5173 npm run accounts
// If Vite runs on another port, start the API with BETTER_AUTH_URL set to that origin.
// With FIRESIGHT_OAUTH_DCR=1 on the API the OAuth consent page is driven too; without
// it that part is skipped, and says so.
//
// The journey: a signed-out visitor sees a local-first app; signs up; creates an API
// key (shown once); an agent outside the browser uses that key on the hosted MCP
// endpoint to create a plan, which appears on the site without a reload; the plan opens
// with its charts; the agent changes it and the open view follows; a /p/<id> link opens
// it at a view; sync turned on and off in one tab is followed by a second tab of the
// same browser, which uploads nothing once it is off; a local plan is synced by its
// switch and a second browser sees it and edits it live; a conflicting edit is offered both ways out; an app connects through
// the consent page; the account is deleted and nothing is left signed in.
import { randomBytes } from 'node:crypto';
import { chromium } from 'playwright';

const base = (process.env.BASE_URL ?? 'http://localhost:5173').replace(/\/+$/, '');
const browser = await chromium.launch({ channel: 'chrome' }).catch(() => chromium.launch());
const problems = [];
let passed = 0;
let skippedSteps = 0;
const ok = (name) => {
  passed += 1;
  console.log(`  ok  ${name}`);
};
const skip = (name, why) => {
  skippedSteps += 1;
  console.log(`  --  ${name}: skipped (${why})`);
};
const expect = (condition, message) => {
  if (!condition) throw new Error(message);
};

// The statuses this journey provokes on purpose: a plan request after the session has
// ended (401), a link to a plan that is gone (404), a stale upload (409).
const EXPECTED_STATUS = /status of (401|404|409)/;
function watch(page, tag) {
  page.on('console', (m) => (m.type() === 'error' || m.type() === 'warning') && !EXPECTED_STATUS.test(m.text()) && problems.push(`${tag} console ${m.type()}: ${m.text().slice(0, 300)}`));
  page.on('pageerror', (e) => problems.push(`${tag} pageerror: ${e.message}`));
}

const EMAIL = `e2e-${randomBytes(5).toString('hex')}@example.com`;
const PASSWORD = `e2e ${randomBytes(9).toString('base64url')} staple`;
const AGENT_PLAN = {
  name: 'Lena, drafted by an agent',
  household: { people: [{ id: 'lena', name: 'Lena', birth: '1990-04' }], location: { country: 'US', state: 'TX', city: 'Austin' } },
  incomes: [{ id: 'lena_job', name: 'Product designer', owner: 0, kind: { type: 'job', salary: 128000, plan: { employee_fraction: 0.08, match_rate: 0.5, match_up_to: 0.06 } } }],
  spending: { items: [{ id: 'living', name: 'Living costs', category: 'other', amount: 4200, frequency: 'monthly' }] },
  accounts: [{ id: 'lena_401k', name: '401(k)', owner: 0, kind: 'traditional', balance: 96000 }],
};

// ---- the agent: the hosted MCP endpoint over HTTP, authorised by the API key ----
let rpcId = 0;
async function mcp(key, name, args) {
  const response = await fetch(`${base}/api/mcp`, {
    method: 'POST',
    headers: { authorization: `Bearer ${key}`, 'content-type': 'application/json', accept: 'application/json, text/event-stream' },
    body: JSON.stringify({ jsonrpc: '2.0', id: ++rpcId, method: 'tools/call', params: { name, arguments: args } }),
  });
  const text = await response.text();
  expect(response.ok, `MCP ${name}: HTTP ${response.status} ${text.slice(0, 300)}`);
  const framed = (response.headers.get('content-type') ?? '').includes('text/event-stream');
  const payload = framed ? text.split('\n').filter((line) => line.startsWith('data:')).map((line) => line.slice(5).trim()).at(-1) : text;
  const message = JSON.parse(payload ?? 'null');
  expect(message?.result && !message.result.isError, `MCP ${name} failed: ${JSON.stringify(message?.error ?? message?.result?.content).slice(0, 400)}`);
  return message.result.structuredContent;
}

/** Waits for something outside the page to become true, such as what the account holds. */
async function until(check, message, timeout = 30_000) {
  const deadline = Date.now() + timeout;
  for (;;) {
    if (await check()) return;
    if (Date.now() > deadline) throw new Error(`timed out waiting: ${message}`);
    await new Promise((resolve) => setTimeout(resolve, 400));
  }
}

// ---- reading the page ----
const settled = (page, timeout = 240_000) =>
  page.waitForFunction(
    () => {
      const run = window.__firesight.run.getState();
      return run.status === 'done' && !run.stale && run.scenarios.every((s) => s.paired);
    },
    null,
    { timeout },
  );
const plan = (page) =>
  page.evaluate(() => {
    const s = window.__firesight.plan.getState();
    const job = s.profile?.incomes?.[0]?.kind;
    return { id: s.meta?.id ?? null, origin: s.meta?.origin ?? null, name: s.profile?.name ?? null, salary: job && 'salary' in job ? job.salary : null, bequest: s.profile?.retirement.bequest_target ?? null, dirty: s.revision !== s.savedRevision };
  });
const headline = (page) => page.evaluate(() => window.__firesight.run.getState().summary?.headline.success_rate.value ?? null);
/** The account's plans, asked for by the page itself: its session cookie, nothing else. */
const accountPlans = (page) =>
  page.evaluate(async () => {
    const response = await fetch('/api/plans', { headers: { accept: 'application/json' } });
    return response.ok ? (await response.json()).plans : response.status;
  });
const accountPlan = (page, id) => page.evaluate(async (id) => (await (await fetch(`/api/plans/${id}`)).json()), id);
const toastWith = (page, text, timeout = 30_000) => page.getByRole('list', { name: 'Notifications' }).getByText(text).first().waitFor({ timeout });
const clearToasts = async (page) => {
  for (const button of await page.getByRole('list', { name: 'Notifications' }).getByRole('button', { name: 'Dismiss' }).all()) await button.click().catch(() => undefined);
};
/** The account link in a household's stored record (IndexedDB, shared by every tab), or null. */
const storedLink = (page, id) =>
  page.evaluate(
    (id) =>
      new Promise((resolve, reject) => {
        const open = indexedDB.open('firesight');
        open.onerror = () => reject(open.error);
        open.onsuccess = () => {
          const get = open.result.transaction('households').objectStore('households').get(id);
          get.onerror = () => reject(get.error);
          get.onsuccess = () => resolve(get.result?.remote ?? null);
        };
      }),
    id,
  );
const setBequest = async (page, value) => {
  await page.evaluate((value) => window.__firesight.plan.getState().edit([{ op: 'set', path: '/retirement/bequest_target', value }], { target: 'base' }), value);
  await page.waitForFunction(() => window.__firesight.plan.getState().revision === window.__firesight.plan.getState().savedRevision);
};
const switcher = (page) => page.getByTestId('household-switcher');
const openSwitcher = async (page) => {
  if (!(await page.getByRole('dialog', { name: 'Households' }).count())) await switcher(page).click();
  return page.getByRole('dialog', { name: 'Households' });
};
const closeSwitcher = async (page) => {
  if (await page.getByRole('dialog', { name: 'Households' }).count()) await page.keyboard.press('Escape');
};
const syncState = async (page) => {
  const panel = await openSwitcher(page);
  const state = await panel.getByTestId('sync-row').getAttribute('data-sync');
  await closeSwitcher(page);
  return state;
};
async function signIn(page, { create = false } = {}) {
  if (create) await page.getByRole('radio', { name: 'Create account' }).or(page.getByRole('button', { name: 'Create account', exact: true })).first().click();
  if (create) await page.getByLabel('Name').fill('E2E Person');
  await page.getByLabel('Email').fill(EMAIL);
  await page.getByLabel('Password').fill(PASSWORD);
  await page.locator('form button[type="submit"]').click();
}

try {
  const config = await (await fetch(`${base}/api/config`).catch(() => null))?.json().catch(() => null);
  expect(config?.accounts === true, `There is no account API behind ${base}. Start \`npm run dev:api\` at the repository root (with BETTER_AUTH_URL=${base} if that is not its default), then \`npm run dev\` in web/.`);
  expect(config.mcp_url === `${base}/api/mcp`, `The API thinks the site is at ${config.mcp_url}, not ${base}/api/mcp: start it with BETTER_AUTH_URL=${base}.`);
  console.log(`accounts: testing ${base} in Chrome ${browser.version()}`);

  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  watch(page, 'main');

  // ---- signed out: the local-first app, with a way in ----
  await page.goto(base);
  await page.getByRole('heading', { name: /When can work become optional/ }).waitFor();
  await page.getByTestId('sign-in').waitFor();
  expect((await page.getByText('No account needed. Without one, your plan is stored in this browser and is never uploaded.').count()) === 1, 'the landing page should say what holds without an account');
  expect((await page.getByTestId('landing-sign-in').count()) === 1, 'the landing page should offer sign-in');
  const requests = [];
  const code = [];
  page.on('request', (r) => r.url().includes('/api/plans') && requests.push(r.url()));
  page.on('request', (r) => /cloud-store|cloud-ui|auth\/pages|better-auth/.test(r.url()) && code.push(new URL(r.url()).pathname));
  await page.getByTestId('demo-dual-income-family').click();
  await settled(page);
  await page.evaluate(() => window.__firesight.plan.getState().edit([{ op: 'set', path: '/retirement/bequest_target', value: 123000 }]));
  await page.waitForFunction(() => window.__firesight.plan.getState().revision === window.__firesight.plan.getState().savedRevision);
  await page.waitForTimeout(1500);
  expect(requests.length === 0, `signed out, saving a plan must not call the plans API; saw ${requests.join(', ')}`);
  expect(code.length === 0, `signed out, the sync code, its UI and the auth client must not be downloaded; saw ${code.join(', ')}`);
  const localDemo = await plan(page);
  const signedOutPanel = await openSwitcher(page);
  expect((await signedOutPanel.getByText('In this browser').count()) === 1 && (await signedOutPanel.getByTestId('sync-row').count()) === 0, 'signed out, the switcher should list local plans and offer no sync switch');
  await closeSwitcher(page);
  ok('signed out: the app is local-first, makes no plan requests, downloads no account code, and offers sign-in');

  await page.goto(`${base}/account`);
  await page.waitForURL(/\/login\?next=%2Faccount$/);
  ok('/account while signed out goes to the login page and remembers where to return');

  // ---- sign up ----
  await page.getByRole('heading', { name: 'Sign in', exact: true }).waitFor();
  await page.getByLabel('Email').fill(EMAIL);
  await page.getByLabel('Password').fill('wrong-password-here');
  await page.locator('form button[type="submit"]').click();
  await page.getByRole('alert').waitFor();
  expect(new URL(page.url()).pathname === '/login', 'a wrong password should stay on the login page');
  await signIn(page, { create: true });
  await page.waitForURL(`${base}/account`);
  expect((await page.getByTestId('account-email').textContent())?.trim() === EMAIL, 'the account page should show the email signed up with');
  const cookies = await context.cookies();
  const session = cookies.find((c) => c.name.includes('session_token'));
  expect(session?.httpOnly === true && session.sameSite === 'Lax', `the session should be an HttpOnly, SameSite=Lax cookie: ${JSON.stringify(session && { ...session, value: '…' })}`);
  const stored = await page.evaluate(() => JSON.stringify(Object.entries(localStorage)) + JSON.stringify(Object.entries(sessionStorage)));
  expect(!stored.includes(decodeURIComponent(session.value).split('.')[0]), 'the session token must not be in localStorage or sessionStorage');
  ok('sign-up: a wrong password is refused; a new account lands on /account with an HttpOnly cookie and no token in storage');

  // ---- an API key, shown once ----
  const keys = page.getByTestId('api-keys');
  await keys.getByLabel('Name').fill('Claude Code');
  await keys.getByRole('button', { name: 'Create key' }).click();
  const secretField = page.getByTestId('new-key-secret');
  await secretField.waitFor();
  const key = await secretField.inputValue();
  expect(/^fs_[A-Za-z0-9_-]{20,}$/.test(key), `the new key should be an fs_ secret, got ${key.slice(0, 6)}…`);
  expect((await page.getByTestId('new-key').textContent()).includes(`Bearer ${key}`), 'the Claude Code command should carry the new key');
  await page.getByTestId('key-list').getByText('Claude Code').waitFor();
  const row = await page.getByTestId('key-list').textContent();
  expect(row.includes(`${key.slice(0, 6)}…`) && row.includes('read and write') && !row.includes(key), `the list should show the prefix and the access, never the secret: ${row.slice(0, 200)}`);
  await page.getByRole('button', { name: 'I have copied it' }).click();
  expect(!(await page.content()).includes(key), 'once dismissed, the secret should be nowhere on the page');
  await page.reload();
  await page.getByTestId('key-list').getByText('Claude Code').waitFor();
  expect(!(await page.content()).includes(key) && (await page.getByTestId('new-key').count()) === 0, 'after a reload the secret is not shown again');
  expect((await page.getByTestId('connect-ai').textContent()).includes(`${base}/api/mcp`), 'the account page should give the MCP address');
  for (const tab of ['Claude Code', 'Cursor', 'VS Code', 'Anything else', 'Claude']) {
    await page.getByTestId('connect-ai').getByRole('tab', { name: tab, exact: true }).click();
    expect((await page.getByTestId('connect-ai').getByRole('tabpanel').textContent()).length > 80, `the ${tab} instructions should not be empty`);
  }
  ok('API key: created with a name and access, the secret shown once with a ready command, listed by prefix only');

  // ---- the agent creates a plan; the site shows it without a reload ----
  await page.getByRole('button', { name: 'Back to FIRESight' }).first().click();
  await settled(page);
  expect((await plan(page)).id === localDemo.id, 'the local plan should still be open after visiting the account page');
  const created = await mcp(key, 'create_plan', { profile: AGENT_PLAN });
  expect(/^pl_[0-9A-Za-z]{22}$/.test(created.id) && created.url === `${base}/p/${created.id}`, `create_plan should return an id and its link: ${JSON.stringify(created).slice(0, 200)}`);
  await toastWith(page, `New plan in your account: ${AGENT_PLAN.name}`);
  const panel = await openSwitcher(page);
  const inAccount = panel.getByTestId('account-plans');
  await inAccount.getByText(AGENT_PLAN.name).waitFor();
  expect((await inAccount.textContent()).includes('from Claude Code key'), `the account group should say who wrote the plan: ${await inAccount.textContent()}`);
  expect((await page.evaluate(() => performance.getEntriesByType('navigation').length)) === 1, 'the page should not have reloaded');
  ok('an agent with the API key creates a plan over MCP; it appears in the switcher, attributed, without a reload');

  // ---- open it: charts ----
  await inAccount.getByText(AGENT_PLAN.name).click();
  await page.waitForFunction((name) => window.__firesight.plan.getState().profile?.name === name, AGENT_PLAN.name);
  await settled(page);
  const opened = await plan(page);
  expect(opened.origin === 'account' && opened.salary === 128000, `the agent's plan should be open: ${JSON.stringify(opened)}`);
  await page.locator('[data-chart-card]').first().waitFor();
  const before = await headline(page);
  expect(typeof before === 'number' && (await page.getByTestId('headline').count()) === 1, 'the agent’s plan should have run and show its headline figures');
  expect((await syncState(page)) === 'synced', 'a plan opened from the account should be synced');
  const afterOpen = await openSwitcher(page);
  expect((await afterOpen.getByTestId('account-plans').count()) === 0, 'once opened, the plan is listed under this browser, not as account-only');
  await closeSwitcher(page);
  await clearToasts(page);
  ok('opening it fetches a copy into this browser, runs it and draws the charts; it is synced from the start');

  // ---- the agent changes it; the open view follows ----
  const updated = await mcp(key, 'update_plan', { plan_id: created.id, expected_rev: created.rev, ops: [{ op: 'set', path: '/incomes/@lena_job/kind/salary', value: 61000 }] });
  expect(updated.rev === created.rev + 1, `update_plan should bump the revision: ${JSON.stringify(updated).slice(0, 200)}`);
  await toastWith(page, 'Updated by Claude Code key');
  expect((await plan(page)).salary === 61000, 'the open plan should now hold the agent’s change');
  await settled(page);
  const after = await headline(page);
  expect(after !== before, `halving the salary should move the headline figure (${before} → ${after})`);
  expect((await accountPlan(page, created.id)).rev === updated.rev, 'loading the agent’s version must not upload anything back');
  expect((await page.evaluate(() => performance.getEntriesByType('navigation').length)) === 1, 'the page should not have reloaded');
  await clearToasts(page);
  ok('the agent updates the plan; the open view loads the new version, says who wrote it, and re-runs');

  // ---- an edit here reaches the account ----
  await page.evaluate(() => window.__firesight.plan.getState().edit([{ op: 'set', path: '/incomes/@lena_job/kind/salary', value: 99000 }], { target: 'base' }));
  await until(async () => (await accountPlan(page, created.id)).plan.profile.incomes[0].kind.salary === 99000, 'the edit made on the site should reach the account');
  const pushed = await accountPlan(page, created.id);
  expect(pushed.rev === updated.rev + 1 && pushed.updated_by === 'web', `the edit should be revision ${updated.rev + 1} by the website: ${pushed.rev} ${pushed.updated_by}`);
  const agentSees = await mcp(key, 'get_plan', { plan_id: created.id, full: true });
  expect(JSON.stringify(agentSees).includes('99000'), 'the agent should read the change made on the website');
  ok('an edit on the site is uploaded with If-Match, and the agent reads it back');

  // ---- a plan link ----
  await page.goto(`${base}/p/${created.id}?view=cash-flow`);
  await page.waitForURL(`${base}/cash-flow`);
  await page.waitForFunction((name) => window.__firesight.plan.getState().profile?.name === name, AGENT_PLAN.name);
  expect((await page.getByRole('tab', { name: 'Cash flow', exact: true }).getAttribute('aria-selected')) === 'true', 'the link should open the plan at the Cash flow view');
  await page.goto(`${base}/p/pl_${'0'.repeat(22)}`);
  await toastWith(page, 'Could not open that plan');
  await page.waitForURL(`${base}/`);
  await clearToasts(page);
  ok('/p/<id>?view=cash-flow opens the plan at that view and tidies the address; an unknown id says so');

  // ---- sync a local plan with its switch ----
  let list = await openSwitcher(page);
  await list.getByRole('button', { name: 'Dual-income family' }).first().click();
  await page.waitForFunction((id) => window.__firesight.plan.getState().meta?.id === id, localDemo.id);
  await settled(page);
  expect((await syncState(page)) === 'off', 'a plan made in this browser should not be synced until asked');
  expect((await accountPlans(page)).length === 1, 'the account should hold only the agent’s plan so far');

  // ---- two tabs of one browser: what one turns on or off, the other follows ----
  const tab = await context.newPage();
  watch(tab, 'tab');
  await tab.goto(base);
  await tab.waitForFunction((id) => window.__firesight.plan.getState().meta?.id === id, localDemo.id);
  await settled(tab);
  expect((await syncState(tab)) === 'off', 'the second tab should open the same plan, not synced');
  list = await openSwitcher(page);
  await list.getByRole('switch').click();
  await toastWith(page, 'Synced to your account');
  await closeSwitcher(page);
  await clearToasts(page);
  const twin = (await accountPlans(page)).find((p) => p.name === 'Dual-income family');
  expect(twin?.rev === 1, 'turning sync on in the first tab should put the plan in the account');
  await until(async () => (await syncState(tab)) === 'synced', 'the second tab should learn that the plan is synced now');
  const tabPanel = await openSwitcher(tab);
  expect((await tabPanel.getByTestId('account-plans').getByText('Dual-income family').count()) === 0, 'the second tab must not list its own plan as a different one in the account');
  await closeSwitcher(tab);
  await setBequest(tab, 555000);
  await until(async () => (await accountPlan(page, twin.id)).plan.profile.retirement.bequest_target === 555000, 'an edit in the second tab should reach the account, not erase the link');
  expect((await storedLink(tab, localDemo.id))?.id === twin.id, 'the stored record should still carry the link after the second tab saved');
  ok('two tabs: sync turned on in one is on in the other, whose edits go to the same plan in the account');

  list = await openSwitcher(page);
  await list.getByRole('switch').click();
  await page.getByTestId('account-copy-keep').click();
  await toastWith(page, 'No longer synced');
  await closeSwitcher(page);
  await clearToasts(page);
  const left = await accountPlan(page, twin.id);
  expect((await storedLink(page, localDemo.id)) === null, 'turning sync off should remove the link from the stored record');
  await until(async () => (await syncState(tab)) === 'off', 'the second tab should learn that sync was turned off');
  const sent = [];
  tab.on('request', (r) => r.url().includes('/api/plans/') && r.method() !== 'GET' && sent.push(`${r.method()} ${new URL(r.url()).pathname}`));
  await setBequest(tab, 123000);
  await tab.waitForTimeout(4000);
  const stillThere = await accountPlan(page, twin.id);
  expect(sent.length === 0, `with sync off, the second tab must send nothing to the account; it sent ${sent.join(', ')}`);
  expect(stillThere.rev === left.rev && stillThere.plan.profile.retirement.bequest_target === 555000, `the account's copy must stay as it was when sync was turned off: rev ${left.rev} → ${stillThere.rev}`);
  expect((await storedLink(tab, localDemo.id)) === null, 'the second tab’s save must not write the link back');
  await tab.reload();
  await tab.waitForFunction((id) => window.__firesight.plan.getState().meta?.id === id, localDemo.id);
  expect((await syncState(tab)) === 'off', 'after a reload the plan must still be unsynced');
  await tab.close();
  ok('two tabs: sync turned off in one stops the other, which uploads nothing and does not turn it back on');

  // Back to where the journey was: the kept copy leaves the account, and this tab shows what the other one saved.
  expect((await page.evaluate(async (id) => (await fetch(`/api/plans/${id}`, { method: 'DELETE' })).status, twin.id)) === 204, 'removing the kept account copy');
  await page.reload();
  await page.waitForFunction((id) => window.__firesight.plan.getState().meta?.id === id, localDemo.id);
  await settled(page);
  expect((await plan(page)).bequest === 123000 && (await syncState(page)) === 'off', 'the plan should be as the second tab left it, and not synced');
  expect((await accountPlans(page)).length === 1, 'the account should hold only the agent’s plan again');
  list = await openSwitcher(page);
  await list.getByRole('switch').click();
  await toastWith(page, 'Synced to your account');
  await closeSwitcher(page);
  let plans = await accountPlans(page);
  const demoRemote = plans.find((p) => p.name === 'Dual-income family');
  expect(plans.length === 2 && demoRemote?.rev === 1 && demoRemote.updated_by === 'web', `the demo should now be in the account: ${JSON.stringify(plans.map((p) => [p.name, p.rev]))}`);
  expect((await accountPlan(page, demoRemote.id)).plan.profile.retirement.bequest_target === 123000, 'the account copy should carry the edit made before signing in');
  expect((await syncState(page)) === 'synced', 'the switch should show the plan as synced');
  const agentList = await mcp(key, 'list_plans', {});
  expect(JSON.stringify(agentList).includes(demoRemote.id), 'the agent should now see the synced plan');
  await clearToasts(page);
  ok('"Sync to account" on a local plan uploads it; the agent can see it only from then on');

  // ---- a second browser ----
  const context2 = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const page2 = await context2.newPage();
  watch(page2, 'second');
  await page2.goto(`${base}/p/${demoRemote.id}?view=outcomes`);
  await page2.waitForURL(/\/login\?next=/);
  await signIn(page2);
  await page2.waitForURL(`${base}/outcomes`, { timeout: 30_000 });
  await page2.waitForFunction(() => window.__firesight.plan.getState().profile?.name === 'Dual-income family');
  expect((await plan(page2)).bequest === 123000, 'the second browser should open the synced plan with its contents');
  await settled(page2);
  const panel2 = await openSwitcher(page2);
  await panel2.getByTestId('account-plans').getByText(AGENT_PLAN.name).waitFor();
  await closeSwitcher(page2);
  ok('a second browser: the plan link goes through sign-in and opens the synced plan; the other plan is listed in the account');

  await page2.evaluate(() => window.__firesight.plan.getState().edit([{ op: 'set', path: '/retirement/bequest_target', value: 250000 }], { target: 'base' }));
  await toastWith(page, 'Updated from another browser', 45_000);
  expect((await plan(page)).bequest === 250000, 'the first browser should follow the second one’s edit');
  await settled(page);
  await clearToasts(page);
  ok('an edit in one browser reaches the other while it has the plan open');

  // ---- a conflict ----
  // Hold this browser's look at the account, so the agent's change is not noticed before the edit here is uploaded.
  const isList = (url) => url.pathname === '/api/plans' && url.search === '';
  const holdList = async () => {
    let release = () => undefined;
    const held = new Promise((resolve) => (release = resolve));
    await page.route(isList, async (route) => {
      await held;
      await route.continue().catch(() => undefined);
    });
    return async () => {
      release();
      await page.unroute(isList);
    };
  };
  let letGo = await holdList();
  let list2 = await openSwitcher(page);
  await list2.getByRole('button', { name: AGENT_PLAN.name }).first().click();
  await page.waitForFunction((name) => window.__firesight.plan.getState().profile?.name === name, AGENT_PLAN.name);
  const current = await accountPlan(page, created.id);
  await mcp(key, 'update_plan', { plan_id: created.id, expected_rev: current.rev, ops: [{ op: 'set', path: '/incomes/@lena_job/kind/salary', value: 70000 }] });
  await page.evaluate(() => window.__firesight.plan.getState().edit([{ op: 'set', path: '/incomes/@lena_job/kind/salary', value: 111000 }], { target: 'base' }));
  await page.getByTestId('conflict').waitFor({ timeout: 30_000 });
  expect((await page.getByTestId('conflict').textContent()).includes('Claude Code key'), 'the conflict should say who wrote the other version');
  expect((await accountPlan(page, created.id)).plan.profile.incomes[0].kind.salary === 70000 && (await plan(page)).salary === 111000, 'neither side should have been overwritten');
  await page.getByTestId('conflict-copy').click();
  await toastWith(page, 'Kept your version as a copy');
  const copy = await plan(page);
  expect(copy.name === `${AGENT_PLAN.name} (my copy)` && copy.salary === 111000 && copy.origin === 'copy', `my version should be open as a copy: ${JSON.stringify(copy)}`);
  expect((await syncState(page)) === 'off', 'the copy should not be synced');
  await letGo();
  list2 = await openSwitcher(page);
  await list2.getByRole('listitem').filter({ hasText: AGENT_PLAN.name }).filter({ hasNotText: '(my copy)' }).getByRole('button').first().click();
  await page.waitForFunction((name) => window.__firesight.plan.getState().profile?.name === name, AGENT_PLAN.name);
  expect((await plan(page)).salary === 70000, 'the synced plan should hold the agent’s version');
  expect((await accountPlans(page)).length === 2, 'the copy must not have been uploaded');
  await clearToasts(page);
  ok('a conflict overwrites nothing; "Keep mine as a copy" leaves my version as a local plan and the agent’s in the synced one');

  await mcp(key, 'update_plan', { plan_id: created.id, expected_rev: (await accountPlan(page, created.id)).rev, ops: [{ op: 'set', path: '/incomes/@lena_job/kind/salary', value: 71000 }] });
  letGo = await holdList();
  await page.evaluate(() => window.__firesight.plan.getState().edit([{ op: 'set', path: '/incomes/@lena_job/kind/salary', value: 112000 }], { target: 'base' }));
  await page.getByTestId('conflict-theirs').click({ timeout: 30_000 });
  await toastWith(page, 'Loaded the newer version');
  expect((await plan(page)).salary === 71000, '"Load the newer version" should put the agent’s version in the editor');
  await letGo();
  await clearToasts(page);
  ok('"Load the newer version" replaces what is open with the account’s version');

  // ---- deleting a synced plan asks where from ----
  list2 = await openSwitcher(page);
  await list2.getByRole('button', { name: `Delete ${AGENT_PLAN.name}`, exact: true }).click();
  await page.getByTestId('account-copy-keep').click();
  await page.waitForFunction(() => window.__firesight.plan.getState().profile === null);
  expect((await accountPlans(page)).some((p) => p.id === created.id), '"Only from this browser" must leave the plan in the account');
  await page.getByTestId('landing-account-plans').getByText(AGENT_PLAN.name).waitFor({ timeout: 30_000 });
  await page.getByTestId('landing-account-plans').getByText(AGENT_PLAN.name).click();
  await page.waitForFunction((name) => window.__firesight.plan.getState().profile?.name === name, AGENT_PLAN.name);
  list2 = await openSwitcher(page);
  await list2.getByRole('button', { name: `Delete ${AGENT_PLAN.name}`, exact: true }).click();
  await page.getByTestId('account-copy-remove').click();
  await page.waitForFunction(() => window.__firesight.plan.getState().profile === null);
  await until(async () => (await accountPlan(page, created.id)).error === 'not_found', '"Delete everywhere" should remove the plan from the account');
  ok('deleting a synced plan asks: "Only from this browser" keeps the account copy (and it can be reopened); "Delete everywhere" removes both');

  // ---- an app connects through the consent page ----
  if (config.dcr) {
    const redirect = 'http://127.0.0.1:33418/callback';
    const registration = await fetch(`${base}/api/auth/oauth2/register`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ client_name: 'E2E Desk Agent', redirect_uris: [redirect], token_endpoint_auth_method: 'none', grant_types: ['authorization_code', 'refresh_token'], response_types: ['code'] }) });
    const client = await registration.json();
    expect(registration.status === 201 && client.client_id, `registering a test client: ${registration.status}`);
    const query = new URLSearchParams({ response_type: 'code', client_id: client.client_id, redirect_uri: redirect, scope: 'plans:read plans:write offline_access', state: 'e2e-state', code_challenge: randomBytes(32).toString('base64url'), code_challenge_method: 'S256', resource: `${base}/api/mcp` });
    // The app's own callback address: nothing listens there, so answer for it.
    await context.route(`${redirect}*`, (route) => route.fulfill({ status: 200, contentType: 'text/plain', body: 'the app got its answer' }));
    await page.goto(`${base}/api/auth/oauth2/authorize?${query}`);
    await page.waitForURL(/\/consent\?/);
    await page.getByTestId('consent-app').waitFor();
    const consent = await page.locator('main').textContent();
    expect(consent.includes('Allow E2E Desk Agent to work with your plans?') && consent.includes(client.client_id) && consent.includes('127.0.0.1:33418') && consent.includes('an app running on this computer'), `the consent page should name the app, its id and where the answer goes: ${consent.slice(0, 400)}`);
    expect(consent.includes('Read the plans synced to your account') && consent.includes('Create plans in your account') && consent.includes(EMAIL), 'the consent page should say what is being allowed, and for which account');
    await page.getByRole('button', { name: 'Deny' }).click();
    await page.waitForURL(/^http:\/\/127\.0\.0\.1:33418\/callback/);
    expect(new URL(page.url()).searchParams.get('error') === 'access_denied', `Deny should return access_denied to the app: ${page.url()}`);
    await page.goto(`${base}/api/auth/oauth2/authorize?${query}`);
    await page.waitForURL(/\/consent\?/);
    await page.getByRole('button', { name: 'Allow' }).click();
    await page.waitForURL(/^http:\/\/127\.0\.0\.1:33418\/callback/);
    const back = new URL(page.url()).searchParams;
    expect(back.get('code') && back.get('state') === 'e2e-state' && back.get('iss') === `${base}/api/auth`, `Allow should return a code, the state and the issuer: ${page.url().slice(0, 200)}`);
    await page.goto(`${base}/account`);
    const apps = page.getByTestId('connected-apps');
    await apps.getByText('E2E Desk Agent').waitFor();
    page.once('dialog', (dialog) => void dialog.accept());
    await apps.getByRole('button', { name: 'Disconnect' }).click();
    await apps.getByText('No apps are connected.').waitFor();
    ok('OAuth: the consent page names the app, its id and its callback host; Deny and Allow return to the app; the app is listed and can be disconnected');

    // Signed out, the same request goes by way of the login page.
    const context3 = await browser.newContext({ viewport: { width: 420, height: 800 } });
    const page3 = await context3.newPage();
    watch(page3, 'phone');
    await context3.route(`${redirect}*`, (route) => route.fulfill({ status: 200, contentType: 'text/plain', body: 'the app got its answer' }));
    await page3.goto(`${base}/api/auth/oauth2/authorize?${query}`);
    await page3.waitForURL(/\/login\?/);
    await page3.getByRole('heading', { name: 'Sign in to connect an app' }).waitFor();
    expect(await page3.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'the login page should fit a phone without sideways scrolling');
    await signIn(page3);
    await page3.waitForURL(/\/consent\?/);
    await page3.getByTestId('consent-app').waitFor();
    expect(await page3.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), 'the consent page should fit a phone without sideways scrolling');
    await page3.getByRole('button', { name: 'Allow' }).click();
    await page3.waitForURL(/^http:\/\/127\.0\.0\.1:33418\/callback\?.*code=/);
    await context3.close();
    ok('OAuth while signed out: authorize → login (at phone width) → sign in → consent → back to the app with a code');
  } else {
    skip('OAuth consent page', 'the API was started without FIRESIGHT_OAUTH_DCR=1, so the script cannot register a test client');
  }

  // ---- the privacy note ----
  await page.goto(base);
  await page.getByRole('heading', { name: /When can work become optional/ }).waitFor();
  await page.getByRole('button', { name: 'Settings' }).click();
  const note = await page.getByTestId('privacy-account').textContent();
  expect(note.includes('a key the server holds') && note.includes('the server can read them'), `Settings should say plainly what the server can read: ${note}`);
  await page.keyboard.press('Escape');
  ok('Settings explains the two modes, and that the server can read synced plans');

  // ---- revoke the key ----
  await page.goto(`${base}/account`);
  page.once('dialog', (dialog) => void dialog.accept());
  await page.getByTestId('key-list').getByRole('button', { name: 'Revoke' }).click();
  await page.getByTestId('api-keys').getByText('No keys yet.').waitFor();
  const refused = await fetch(`${base}/api/mcp`, { method: 'POST', headers: { authorization: `Bearer ${key}`, 'content-type': 'application/json', accept: 'application/json, text/event-stream' }, body: JSON.stringify({ jsonrpc: '2.0', id: 99, method: 'tools/list' }) });
  expect(refused.status === 401, `a revoked key should be refused by the MCP endpoint, got ${refused.status}`);
  ok('revoking the key cuts the agent off at once');

  // ---- delete the account ----
  await page.getByRole('button', { name: 'Delete account' }).click();
  const dialog = page.getByRole('dialog', { name: 'Delete your account?' });
  // An account with a password confirms with it (one without is asked for its email instead: src/auth/delete-account.test.ts).
  await dialog.locator('[data-testid="delete-account"][data-confirm="password"]').waitFor();
  await dialog.getByLabel('Your password, to confirm').fill(PASSWORD);
  await dialog.getByRole('button', { name: 'Delete my account' }).click();
  await toastWith(page, 'Your account has been deleted');
  await page.waitForURL(`${base}/`);
  await page.getByTestId('sign-in').waitFor();
  expect((await accountPlans(page)) === 401, 'after deletion there is no session');
  // What was opened here is still here, as ordinary local plans.
  await page.getByRole('button', { name: /Dual-income family/ }).first().click();
  await page.waitForFunction(() => window.__firesight.plan.getState().profile?.name === 'Dual-income family');
  const leftovers = await openSwitcher(page);
  expect((await leftovers.getByTestId('sync-row').count()) === 0 && (await leftovers.getByLabel(/Sync/).count()) === 0, 'after deletion no plan should still claim to be synced');
  await closeSwitcher(page);
  await page.goto(`${base}/login`);
  await signIn(page);
  await page.getByRole('alert').waitFor();
  expect(new URL(page.url()).pathname === '/login', 'the deleted account cannot sign in');
  // The other browser finds out on its next look at the account.
  await toastWith(page2, 'You have been signed out', 45_000);
  await page2.getByTestId('sign-in').waitFor();
  expect((await plan(page2)).name === 'Dual-income family', 'the second browser keeps the plan it had open');
  ok('deleting the account ends every session, removes the server copies, and leaves local plans in place and unsynced');

  await context2.close();
  await context.close();
} catch (error) {
  problems.push(error instanceof Error ? (error.stack ?? error.message) : String(error));
} finally {
  await browser.close();
}

if (problems.length > 0) {
  console.error(`\naccounts: FAILED after ${passed} checks\n${problems.map((p) => `  ${p}`).join('\n')}`);
  process.exit(1);
}
console.log(`\naccounts: ${passed} checks passed${skippedSteps ? `, ${skippedSteps} skipped` : ''}, console clean`);
