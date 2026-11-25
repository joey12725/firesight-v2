// npm run mcp:smoke
//
// The hosted MCP endpoint, end to end over real HTTP, the way an agent outside the
// browser uses it: sign up a throwaway user through the auth API, create an API key,
// connect with the MCP SDK's own client, create a plan, read it back, change it,
// simulate it and compare a what-if. Prints the plan's /p/<id> link, which is where the
// user would then see it on the website.
//
//   BASE_URL=http://localhost:8787 npm run mcp:smoke     against a running `npm run dev:api`
//   npm run mcp:smoke                                    starts its own server on SMOKE_PORT
//                                                        (default 8791), nothing kept
//
//   FIRESIGHT_API_KEY=fs_…   use this key instead of signing up (for a deployed site;
//                            it must be a read-write key). The account is left alone.
//   --keep                   leave the throwaway account and its plan in place, and print
//                            the sign-in details, so the plan can be opened in the app
//   --json                   print one JSON object at the end instead of the summary
//
// Without --keep the account the script created is deleted again at the end.

import { spawn } from 'node:child_process';
import type { ChildProcess } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { join } from 'node:path';
import { Client, StreamableHTTPClientTransport } from '@modelcontextprotocol/client';
import { REPO_ROOT } from './_lib/dev-runtime.js';

const port = Number(process.env.SMOKE_PORT ?? 8791);
const external = process.env.BASE_URL?.replace(/\/+$/, '');
const base = external ?? `http://localhost:${port}`;
const keep = process.argv.includes('--keep');
const asJson = process.argv.includes('--json');
const suppliedKey = process.env.FIRESIGHT_API_KEY?.trim();
const PASSWORD = `smoke-${randomBytes(12).toString('base64url')}`;

const say = (line: string) => {
  if (!asJson) console.log(line);
};
let checks = 0;
const ok = (name: string) => {
  checks++;
  say(`  ok  ${name}`);
};
function expect(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

interface Reply<T> {
  status: number;
  body: T;
  text: string;
}

/** A browser profile on the site: a cookie jar, and the site's own Origin on writes. */
class Session {
  private readonly jar = new Map<string, string>();
  constructor(private readonly origin: string) {}

  async call<T = Record<string, unknown>>(method: string, path: string, body?: unknown): Promise<Reply<T>> {
    const headers = new Headers();
    if (method !== 'GET') headers.set('origin', this.origin);
    if (this.jar.size > 0) headers.set('cookie', [...this.jar].map(([k, v]) => `${k}=${v}`).join('; '));
    if (body !== undefined) headers.set('content-type', 'application/json');
    const response = await fetch(`${base}${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), redirect: 'manual' });
    for (const cookie of response.headers.getSetCookie()) {
      const [pair = '', ...attributes] = cookie.split(';');
      const eq = pair.indexOf('=');
      if (attributes.some((a) => /^\s*max-age=0\s*$/i.test(a)) || pair.slice(eq + 1) === '') this.jar.delete(pair.slice(0, eq).trim());
      else this.jar.set(pair.slice(0, eq).trim(), pair.slice(eq + 1));
    }
    const text = await response.text();
    let parsed: unknown = text;
    try {
      parsed = text ? JSON.parse(text) : null;
    } catch {
      // Not JSON.
    }
    return { status: response.status, body: parsed as T, text };
  }
}

interface ToolReply<T> {
  isError: boolean;
  data: T;
  text: string[];
}

async function tool<T = Record<string, unknown>>(client: Client, name: string, args: Record<string, unknown> = {}): Promise<ToolReply<T>> {
  const result = await client.callTool({ name, arguments: args });
  const content = (result.content ?? []) as { type: string; text?: string }[];
  return { isError: result.isError === true, data: (result.structuredContent ?? {}) as T, text: content.filter((c) => c.type === 'text').map((c) => c.text ?? '') };
}

/** Like `tool`, and the call must succeed. */
async function must<T = Record<string, unknown>>(client: Client, name: string, args: Record<string, unknown> = {}): Promise<T> {
  const reply = await tool<T>(client, name, args);
  expect(!reply.isError, `${name} failed: ${reply.text.join(' ')}`);
  return reply.data;
}

async function connect(key: string, era: 'auto' | 'legacy'): Promise<Client> {
  const transport = new StreamableHTTPClientTransport(new URL(`${base}/api/mcp`), { requestInit: { headers: { authorization: `Bearer ${key}` } } });
  const client = new Client({ name: 'firesight-mcp-smoke', version: '1.0.0' }, { versionNegotiation: { mode: era } });
  await client.connect(transport);
  return client;
}

async function waitForServer(child: ChildProcess): Promise<void> {
  const deadline = Date.now() + 180_000;
  let exited: number | null = null;
  child.once('exit', (code) => (exited = code ?? 1));
  while (Date.now() < deadline) {
    if (exited !== null) throw new Error(`The dev API server exited with code ${exited} before it was ready.`);
    try {
      if ((await fetch(`${base}/api/config`)).ok) return;
    } catch {
      // Not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error('The dev API server did not start within three minutes.');
}

const profile = {
  name: 'Smoke test household',
  household: { people: [{ id: 'lena', name: 'Lena', birth: '1990-04' }], location: { country: 'US', state: 'TX', city: 'Austin' } },
  incomes: [{ id: 'lena_job', name: 'Product designer', owner: 0, kind: { type: 'job', salary: 128_000, plan: { employee_fraction: 0.08, match_rate: 0.5, match_up_to: 0.06 } } }],
  spending: { items: [{ id: 'living', name: 'Living costs', category: 'other', amount: 4_500, frequency: 'monthly' }] },
  accounts: [{ id: 'lena_401k', name: '401(k)', owner: 0, kind: 'traditional', balance: 96_000 }],
};

let server: ChildProcess | null = null;
let serverOutput = '';

try {
  if (!external) {
    server = spawn(process.execPath, ['--import', 'tsx', join(REPO_ROOT, 'scripts', 'dev-api.ts')], {
      cwd: REPO_ROOT,
      env: { ...process.env, PORT: String(port), BETTER_AUTH_URL: base, FIRESIGHT_DEV_DATA: 'memory', DATABASE_URL: '', FIRESIGHT_DEV_QUIET: '1' },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    server.stdout?.on('data', (chunk: Buffer) => (serverOutput += chunk.toString()));
    server.stderr?.on('data', (chunk: Buffer) => (serverOutput += chunk.toString()));
    await waitForServer(server);
  }
  say(`mcp-smoke against ${base}`);

  // ---- what the deployment says about itself ------------------------------------------
  const config = await (await fetch(`${base}/api/config`)).json() as { accounts: boolean; mcp_url: string | null };
  expect(config.accounts && config.mcp_url, 'Accounts are not set up on this server (GET /api/config says accounts: false), so there is no MCP endpoint.');
  // The site's public origin: what cookies and the Origin check are bound to, and where plan links point.
  const origin = new URL(config.mcp_url).origin;

  const anonymous = await fetch(`${base}/api/mcp`, { method: 'POST', headers: { 'content-type': 'application/json', accept: 'application/json, text/event-stream' }, body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'ping' }) });
  const challenge = anonymous.headers.get('www-authenticate') ?? '';
  expect(anonymous.status === 401 && challenge.startsWith('Bearer '), `a call without a credential should be challenged, got ${anonymous.status}`);
  const metadataUrl = /resource_metadata="([^"]+)"/.exec(challenge)?.[1];
  expect(metadataUrl, 'the challenge should name the protected-resource metadata');
  // Fetched from the server under test, at the path the challenge names.
  const metadata = await (await fetch(`${base}${new URL(metadataUrl).pathname}`)).json() as { resource?: string; authorization_servers?: string[] };
  expect(metadata.resource === config.mcp_url && metadata.authorization_servers?.length === 1, `the protected-resource metadata should name ${config.mcp_url}`);
  ok('a call without a credential gets 401 and is pointed at the sign-in metadata');

  // ---- a user and a key ----------------------------------------------------------------
  let key = suppliedKey ?? '';
  let session: Session | null = null;
  const email = `mcp-smoke-${Date.now()}-${randomBytes(3).toString('hex')}@example.com`;
  if (!key) {
    session = new Session(origin);
    const signUp = await session.call('POST', '/api/auth/sign-up/email', { email, password: PASSWORD, name: 'MCP smoke test' });
    expect(signUp.status === 200, `sign-up failed: ${signUp.status} ${signUp.text}`);
    const created = await session.call<{ key: string }>('POST', '/api/auth/api-key/create', { name: 'Smoke test agent', configId: 'read-write' });
    expect(created.status === 200 && created.body.key.startsWith('fs_'), `key creation failed: ${created.status} ${created.text}`);
    key = created.body.key;
    ok('signed up a throwaway user and created a read-write API key');
  } else {
    ok('using the API key from FIRESIGHT_API_KEY');
  }

  // ---- the agent's side ----------------------------------------------------------------
  const client = await connect(key, 'auto');
  const tools = (await client.listTools()).tools.map((t) => t.name);
  for (const name of ['get_guide', 'get_plan_schema', 'validate_plan', 'list_plans', 'get_plan', 'create_plan', 'update_plan', 'run_simulation', 'compare_scenario']) {
    expect(tools.includes(name), `the server should offer ${name}; it offers ${tools.join(', ')}`);
  }
  expect((client.getInstructions() ?? '').includes('get_guide'), 'the server should introduce itself with instructions');
  ok(`connected as an MCP client (${client.getServerVersion()?.name} ${client.getServerVersion()?.version}); ${tools.length} tools`);

  const guide = await must<{ guide: string }>(client, 'get_guide');
  expect(guide.guide.includes('update_plan'), 'the guide should use this server\'s tool names');
  const person = await must<{ fields: { name: string }[] }>(client, 'get_plan_schema', { section: 'Person' });
  expect(person.fields.some((f) => f.name === 'birth'), 'the schema lookup should describe Person');
  const draft = await must<{ validation: { can_run: boolean } }>(client, 'validate_plan', { profile });
  expect(draft.validation.can_run, 'the draft profile should be valid');
  ok('read the guide, looked up the schema and validated a draft');

  const before = (await must<{ count: number }>(client, 'list_plans')).count;
  const created = await must<{ id: string; rev: number; url: string; updated_by: string }>(client, 'create_plan', { profile });
  expect(/^pl_[0-9A-Za-z]{22}$/.test(created.id) && created.rev === 1, `create_plan returned ${JSON.stringify(created)}`);
  expect(created.url === `${origin}/p/${created.id}`, `the plan link should be ${origin}/p/${created.id}, got ${created.url}`);
  expect((await must<{ count: number }>(client, 'list_plans')).count === before + 1, 'the new plan should be listed');
  ok(`created a plan: ${created.url}`);

  const read = await must<{ rev: number; plan: { incomes: { id: string }[] }; preview: { annual_income: number } }>(client, 'get_plan', { plan_id: created.id });
  expect(read.plan.incomes[0]?.id === 'lena_job' && read.preview.annual_income === 128_000, 'get_plan should return the plan that was saved');
  const updated = await must<{ rev: number }>(client, 'update_plan', { plan_id: created.id, expected_rev: read.rev, ops: [{ op: 'set', path: '/incomes/@lena_job/kind/salary', value: 140_000 }] });
  expect(updated.rev === 2, `update_plan should bring the plan to rev 2, got ${updated.rev}`);
  const stale = await tool<{ error: { code: string } }>(client, 'update_plan', { plan_id: created.id, expected_rev: 1, ops: [{ op: 'set', path: '/incomes/@lena_job/kind/salary', value: 1 }] });
  expect(stale.isError && stale.data.error.code === 'conflict', 'a write against a stale rev should be refused as a conflict');
  ok('read it, changed it, and a stale write was refused');

  const started = Date.now();
  const run = await must<{ paths: number; success: { p: number; lo: number; hi: number }; home_sale: { p: number }; url: string }>(client, 'run_simulation', { plan_id: created.id, paths: 1_000 });
  expect(run.paths === 1_000 && run.success.p > 0 && run.success.p <= 1 && run.success.lo <= run.success.p && run.success.hi >= run.success.p, `run_simulation returned ${JSON.stringify(run.success)}`);
  ok(`simulated 1,000 lives in ${((Date.now() - started) / 1000).toFixed(1)} s: the plan holds in ${Math.round(run.success.p * 100)} of 100 (95% interval ${Math.round(run.success.lo * 100)} to ${Math.round(run.success.hi * 100)})`);

  const whatIf = await must<{ comparison: { paired_paths: number; success: { a: number; b: number; delta: number } }; saved_scenario: { saved: boolean; id: string }; url: string }>(client, 'compare_scenario', {
    plan_id: created.id,
    name: 'Spend 500 more a month',
    ops: [{ op: 'set', path: '/spending/items/@living/amount', value: 5_000 }],
    paths: 1_000,
    save: true,
  });
  expect(whatIf.comparison.paired_paths === 1_000 && Math.abs(whatIf.comparison.success.a - run.success.p) < 0.0006, 'the what-if should be paired against the run just reported');
  expect(whatIf.saved_scenario.saved, 'the scenario should be saved');
  ok(`compared a what-if on the same lives (success ${whatIf.comparison.success.a} to ${whatIf.comparison.success.b}) and saved it as "${whatIf.saved_scenario.id}"`);
  await client.close();

  // A client from before the 2026 revision of the protocol is served the same tools.
  const legacy = await connect(key, 'legacy');
  expect((await must<{ count: number }>(legacy, 'list_plans')).count === before + 1, 'a 2025-era client should see the same plans');
  await legacy.close();
  ok('a 2025-era MCP client works too');

  // ---- the user's side: the website's own API sees what the agent wrote -----------------
  if (session) {
    const onSite = await session.call<{ rev: number; updated_by: string; plan: { scenarios: { id: string }[]; profile: { incomes: { kind: { salary: number } }[] } } }>('GET', `/api/plans/${created.id}`);
    expect(onSite.status === 200 && onSite.body.rev === 3, `the website API should serve the plan at rev 3, got ${onSite.status} ${onSite.text.slice(0, 200)}`);
    expect(onSite.body.updated_by === 'apikey:Smoke test agent', `the write should be attributed to the key, got ${onSite.body.updated_by}`);
    expect(onSite.body.plan.profile.incomes[0]?.kind.salary === 140_000 && onSite.body.plan.scenarios[0]?.id === whatIf.saved_scenario.id, 'the website should see the change and the scenario');
    ok('the signed-in user sees the plan, the change and the scenario through the website API');

    const stranger = new Session(origin);
    expect((await stranger.call('POST', '/api/auth/sign-up/email', { email: `stranger-${email}`, password: PASSWORD, name: 'Someone else' })).status === 200, 'a second user can sign up');
    expect((await stranger.call('GET', `/api/plans/${created.id}`)).status === 404, 'another user must not see the plan');
    await stranger.call('POST', '/api/auth/delete-user', { password: PASSWORD });
    ok('another signed-in user gets 404 for it');
  }

  let deleted = false;
  if (session && !keep) {
    const gone = await session.call('POST', '/api/auth/delete-user', { password: PASSWORD });
    expect(gone.status === 200, `deleting the throwaway account failed: ${gone.status} ${gone.text}`);
    const after = await fetch(`${base}/api/mcp`, { method: 'POST', headers: { authorization: `Bearer ${key}`, 'content-type': 'application/json', accept: 'application/json, text/event-stream' }, body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'ping' }) });
    expect(after.status === 401, `the key should die with the account, got ${after.status}`);
    deleted = true;
    ok('deleted the throwaway account; its key stopped working');
  }

  if (server) expect(!/\[dev-api\].*threw|server_error/.test(serverOutput), `the server reported errors:\n${serverOutput}`);

  const summary = { ok: true, checks, base, origin, mcp_url: config.mcp_url, plan_id: created.id, url: created.url, ...(session && !deleted ? { email, password: PASSWORD, api_key: key } : {}), account_deleted: deleted };
  if (asJson) console.log(JSON.stringify(summary));
  else {
    say(`\nmcp-smoke: ${checks} checks passed against ${base}.`);
    say(`Plan: ${created.url}`);
    if (session && !deleted) {
      say(`Sign in as ${email} with password ${PASSWORD} to see it. API key: ${key}`);
      if (!external) say('(This server and its in-memory database stop when the script exits. Use BASE_URL with --keep to keep the plan.)');
    } else if (deleted) say('(The throwaway account was deleted, and the plan with it. Pass --keep to keep both.)');
  }
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  if (asJson) console.log(JSON.stringify({ ok: false, checks, base, error: message }));
  console.error(`\nmcp-smoke FAILED after ${checks} checks: ${message}`);
  if (serverOutput) console.error(`--- dev API server output ---\n${serverOutput}`);
  process.exitCode = 1;
} finally {
  if (server && server.exitCode === null) {
    server.kill('SIGTERM');
    await new Promise((resolve) => server?.once('exit', resolve));
  }
}
