// The in-page WebMCP tools against the real engine (the wasm module, in this thread) and
// the real profile store, with a stubbed model context standing in for the browser.
import { readFileSync } from 'node:fs';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { SYSTEM_PROMPT } from '@/assistant/prompt';
import { nodeEngine, type NodeEngine } from '@/assistant/testing/node-engine';
import { RunRegistry, TOOL_NAMES, toolDefinitions } from '@/assistant/tools';
import type { City } from '@/editor/cities';
import { compare, exampleProfiles, initEngineSync, normalize, planPreview } from '@/engine/api';
import { engineInput } from '@/engine/pool';
import type { Profile } from '@/engine/types';
import { useProfileStore, type Scenario } from '@/state/profile-store';
import { startRunController, useRunStore, type RunState } from '@/state/run-store';
import { useSettings } from '@/state/settings-store';
import { VIEWS } from '@/views/registry';
import { activityEnd, activityStart, answerApproval, answerConsent, askApproval, askConsent, MAX_LOG, resetAgentState, resetConsent, useAgent, withdrawApprovals, type AgentAccess } from './agent-store';
import { chartsPending, mountedCharts, registerChart, type ChartEntry } from './chart-registry';
import { invokeTool, toDefinition, type WebMcpToolDefinition } from './core';
import { GUIDE_TOPICS, guideEssentials, guideTopic, SCHEMA_MAP } from './guide';
import { agentPlanStore } from './plan-bridge';
import { findModelContext, registerTools, registerWebMcp, type ModelContextLike } from './register';
import { AGENT_TOOL_NAMES, AGENT_TOOLS } from './tools';
import { DEFAULT_LIMITS, type AgentContext, type AppBridge, type Json, type Limits, type ToolResult } from './types';
import { watchApprovals } from './withdraw';

/**
 * The pool the app's own run store simulates on. Only the last suite drives the real run
 * store; until it puts a pool here, a run that starts simply never reports.
 */
const pagePool = vi.hoisted(() => {
  const never = () => new Promise<never>(() => {});
  return { current: { runMany: never, comparison: never } as { runMany: (...args: never[]) => Promise<unknown>; comparison: (...args: never[]) => Promise<unknown> } };
});
vi.mock('@/engine', async (original) => ({ ...(await original<Record<string, unknown>>()), getPool: () => pagePool.current }));

// Simulations in this thread: quick alone, slow when the machine is busy with other suites.
vi.setConfig({ testTimeout: 60_000 });

beforeAll(() => {
  initEngineSync(readFileSync(new URL('../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
});

const state = () => useProfileStore.getState();
const demo = (): Profile => structuredClone((exampleProfiles().find((e) => e.slug === 'dual-income-family') ?? exampleProfiles()[0])!.profile);

const city = (name: string, country: string, level: number): City => ({
  city: name,
  country,
  countryCode: country.slice(0, 2).toUpperCase(),
  quality: 1,
  overallSingle: level,
  overallFamily: level,
  rent: level,
  homePrice: level,
  groceries: level,
  dining: level,
  transport: level,
  utilities: level,
  childcare: level,
  goodsServices: level,
  search: `${name}, ${country}`.toLowerCase(),
});

/** Everything a test can turn, in one place. */
interface World {
  ctx: AgentContext;
  engine: NodeEngine;
  settings: { access: AgentAccess; autoApply: boolean };
  limits: Limits;
  saved: Map<string, { name: string; profile: Profile; scenarios: Scenario[] }>;
  run: RunState;
  setRun(next: Partial<RunState>): void;
  charts: ChartEntry[];
  chartsPending: boolean;
  view: string;
  notices: { title: string; undo?: boolean }[];
  ready: { promise: Promise<void>; resolve(): void; reject(error: Error): void };
}

const IDLE: RunState = { status: 'idle', error: null, done: 0, total: 0, pathsPerSecond: 0, seed: 0, summary: null, summaryPaths: 0, stale: false, scenarios: [], elapsedMs: 0, firstPictureMs: 0, summarizeMs: 0, preview: null };

let nextId = 1;

function makeWorld(): World {
  const engine = nodeEngine();
  const store = agentPlanStore();
  const listeners = new Set<() => void>();
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  promise.catch(() => {});
  const world = {
    engine,
    settings: { access: 'allow' as AgentAccess, autoApply: false },
    limits: { ...DEFAULT_LIMITS, consentMs: 40, approvalMs: 40, startupMs: 30, chartsMs: 30, chartsGraceMs: 30 },
    saved: new Map(),
    run: IDLE,
    setRun(next: Partial<RunState>) {
      world.run = { ...world.run, ...next };
      listeners.forEach((l) => l());
    },
    charts: [],
    chartsPending: false,
    view: 'overview',
    notices: [],
    ready: { promise, resolve, reject },
  } as unknown as World;
  const open = (id: string, profile: Profile, scenarios: Scenario[] = [], unsaved = false) => state().load({ meta: { id, createdAt: 1, origin: 'test' }, profile, scenarios, unsaved });
  const app: AppBridge = {
    store,
    plan: () => {
      const s = state();
      if (!s.profile || !s.meta) return null;
      return { id: s.meta.id, name: s.profile.name, errors: s.report.errors.length, warnings: s.report.warnings.length, hasSessionHistory: s.past.length > 0 || s.future.length > 0 };
    },
    savedPlans: () => [...world.saved].map(([id, p]) => ({ id, name: p.name })),
    demos: () => exampleProfiles(),
    signedIn: () => useAgent.getState().signedIn,
    views: () => VIEWS.map((v) => ({ id: v.id, label: v.label })),
    activeView: () => world.view,
    showView: (id, lifePath) => {
      world.view = id;
      return lifePath === undefined ? `/${id}` : `/life-stories?path=${lifePath}&why=sample`;
    },
    run: () => world.run,
    onRun: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    charts: () => world.charts,
    chartsPending: () => world.chartsPending,
    normalise: normalize,
    openNew: async ({ profile, scenarios }) => {
      const id = `plan_${nextId++}`;
      world.saved.set(id, { name: profile.name || 'Untitled household', profile, scenarios: scenarios ?? [] });
      open(id, profile, scenarios);
      return id;
    },
    openSaved: async (id) => {
      const hit = world.saved.get(id);
      if (!hit) return false;
      open(id, hit.profile, hit.scenarios);
      return true;
    },
    cities: async () => [city('Denver', 'United States', 100), city('Lisbon', 'Portugal', 60)],
    today: () => '2026-11',
    notify: (message) => world.notices.push(message),
  };
  world.ctx = {
    ready: () => world.ready.promise,
    settings: () => world.settings,
    consent: () => useAgent.getState().consent,
    askConsent: (signal) => askConsent(signal, world.limits.consentMs),
    askApproval: (request, signal) => askApproval(request, signal, world.limits.approvalMs),
    activity: { start: activityStart, end: activityEnd },
    limits: world.limits,
    app,
    engine,
    runs: new RunRegistry(),
  };
  return world;
}

let world: World;
const tool = (name: string) => {
  const found = AGENT_TOOLS.find((t) => t.name === name);
  if (!found) throw new Error(`no tool ${name}`);
  return found;
};
const call = (name: string, input: unknown = {}, options?: unknown): Promise<ToolResult> => invokeTool(tool(name), world.ctx, input, options);
/** A call that must succeed; the result as loose JSON for assertions. */
async function ok(name: string, input: unknown = {}): Promise<Json> {
  const result = await call(name, input);
  expect(result, JSON.stringify(result).slice(0, 400)).toMatchObject({ ok: true });
  return result;
}
async function failure(name: string, input: unknown = {}): Promise<{ code: string; message: string } & Json> {
  const result = await call(name, input);
  expect(result.ok, JSON.stringify(result).slice(0, 400)).toBe(false);
  if (result.ok) throw new Error('unreachable');
  return { ...result, ...result.error };
}
const tick = (ms = 0) => new Promise((resolve) => setTimeout(resolve, ms));
/** Waits until the page would be showing something for the user to answer. */
async function untilAsked(what: 'consent' | 'approval') {
  for (let i = 0; i < 200; i++) {
    const s = useAgent.getState();
    if (what === 'consent' ? s.consentAsked : s.approvals.length > 0) return;
    await tick(1);
  }
  throw new Error(`the ${what} prompt never appeared`);
}

function openDemo(id = 'demo_plan') {
  const profile = demo();
  world.saved.set(id, { name: profile.name, profile, scenarios: [] });
  state().load({ meta: { id, createdAt: 1, origin: 'test' }, profile });
}

/** The app's own run of the open plan, finished, as run-store would hold it. */
async function finishRun(paths = 300) {
  const s = state();
  const profile = s.profile as Profile;
  const results = await world.engine.pool.runMany([{ id: 'base', profile, paths }], { seed: s.run.seed });
  const base = results.get('base');
  if (!base) throw new Error('no run');
  world.setRun({ status: 'done', stale: false, done: paths, total: paths, seed: s.run.seed, summary: base.summary, summaryPaths: paths, preview: planPreview(profile), scenarios: [] });
}

beforeEach(() => {
  resetAgentState();
  state().close();
  world = makeWorld();
  world.ready.resolve();
});

// ---- registration ---------------------------------------------------------------------

/** A model context in the shape Chrome 151 and later ship: promises, duplicate names refused, the signal unregisters. */
function promiseContext() {
  const tools = new Map<string, WebMcpToolDefinition>();
  const context: ModelContextLike & { tools: typeof tools } = {
    tools,
    registerTool(definition, options) {
      return new Promise<void>((resolve, reject) => {
        if (options?.signal?.aborted) return reject(new DOMException('Aborted', 'AbortError'));
        if (tools.has(definition.name)) return reject(new DOMException('Duplicate tool name', 'InvalidStateError'));
        if (!/^[A-Za-z0-9_.-]{1,128}$/.test(definition.name)) return reject(new DOMException('Invalid tool name', 'InvalidStateError'));
        tools.set(definition.name, definition);
        options?.signal?.addEventListener('abort', () => tools.delete(definition.name));
        resolve();
      });
    },
  };
  return context;
}

describe('registration', () => {
  it('finds the model context on the document, falls back to the navigator, and is silent when there is none', () => {
    const onDocument = { registerTool: () => undefined };
    const onNavigator = { registerTool: () => undefined };
    expect(findModelContext({ modelContext: onDocument }, { modelContext: onNavigator })).toBe(onDocument);
    expect(findModelContext({}, { modelContext: onNavigator })).toBe(onNavigator);
    expect(findModelContext({}, {})).toBeUndefined();
    expect(findModelContext(undefined, undefined)).toBeUndefined();
    expect(findModelContext({ modelContext: { registerTool: 'not a function' } }, {})).toBeUndefined();
    // Node has neither global: the default arguments must not throw.
    expect(findModelContext()).toBeUndefined();
  });

  it('registers every tool with a signal, and disposing unregisters them all', async () => {
    const context = promiseContext();
    const definitions = AGENT_TOOLS.map((t) => toDefinition(t, world.ctx));
    const registration = registerTools(context, definitions);
    await registration.done;
    expect([...context.tools.keys()].sort()).toEqual(AGENT_TOOL_NAMES);
    registration.dispose();
    expect(context.tools.size).toBe(0);
    registration.dispose();
    // The same names register again at once: what a hot reload or a second call does.
    await registerTools(context, definitions).done;
    expect(context.tools.size).toBe(AGENT_TOOLS.length);
  });

  it('withdraws the whole batch when the browser refuses one tool', async () => {
    const context = promiseContext();
    const definitions = AGENT_TOOLS.slice(0, 4).map((t) => toDefinition(t, world.ctx));
    context.tools.set(definitions[2]!.name, definitions[2]!);
    const registration = registerTools(context, definitions);
    await expect(registration.done).rejects.toMatchObject({ name: 'InvalidStateError' });
    // Only the one that was there before is left.
    expect([...context.tools.keys()]).toEqual([definitions[2]!.name]);
  });

  it('works with the synchronous shape of early builds, which unregister by name', async () => {
    const names = new Set<string>();
    const context: ModelContextLike = {
      registerTool(definition) {
        if (names.has(definition.name)) throw new DOMException('Duplicate tool name', 'InvalidStateError');
        names.add(definition.name);
        return undefined;
      },
      unregisterTool: (name) => names.delete(name),
    };
    const definitions = AGENT_TOOLS.map((t) => toDefinition(t, world.ctx));
    const registration = registerTools(context, definitions);
    await registration.done;
    expect(names.size).toBe(AGENT_TOOLS.length);
    registration.dispose();
    expect(names.size).toBe(0);

    names.add(definitions[1]!.name);
    await expect(registerTools(context, definitions.slice(0, 3)).done).rejects.toMatchObject({ name: 'InvalidStateError' });
    expect([...names]).toEqual([definitions[1]!.name]);
  });
});

describe('registerWebMcp', () => {
  const globals = globalThis as { document?: unknown };
  const load = async () => ({ definitions: () => AGENT_TOOLS.map((t) => toDefinition(t, world.ctx)) });
  const settle = async (done: () => boolean) => {
    for (let i = 0; i < 200 && !done(); i++) await tick(1);
  };
  afterEach(() => {
    (globalThis as unknown as Record<string, (() => void) | undefined>).__firesightWebMcpDispose?.();
    delete globals.document;
  });

  it('does nothing at all in a browser without WebMCP', async () => {
    let loaded = false;
    registerWebMcp({ ready: async () => {}, load: async () => ((loaded = true), load()) });
    globals.document = {};
    registerWebMcp({ ready: async () => {}, load: async () => ((loaded = true), load()) });
    await tick(5);
    expect(loaded).toBe(false);
    expect(useAgent.getState()).toMatchObject({ available: false, toolCount: 0, registrationError: null });
  });

  it('registers once however often it is called (StrictMode, a hot reload), and says so to the page', async () => {
    const context = promiseContext();
    const refusals: unknown[] = [];
    const register = context.registerTool.bind(context);
    context.registerTool = (definition, options) => (register(definition, options) as Promise<void>).catch((error: unknown) => {
      if ((error as { name?: string }).name !== 'AbortError') refusals.push(error);
      throw error;
    });
    globals.document = { modelContext: context };
    // Twice in a row, before the first has loaded its tools; then again once it has registered.
    registerWebMcp({ ready: async () => {}, load });
    registerWebMcp({ ready: async () => {}, load });
    await settle(() => useAgent.getState().available);
    expect([...context.tools.keys()].sort()).toEqual(AGENT_TOOL_NAMES);
    registerWebMcp({ ready: async () => {}, load });
    expect(useAgent.getState().available).toBe(false);
    await settle(() => useAgent.getState().available);
    expect([...context.tools.keys()].sort()).toEqual(AGENT_TOOL_NAMES);
    expect(useAgent.getState()).toMatchObject({ available: true, toolCount: AGENT_TOOLS.length, registrationError: null });
    // No duplicate was ever offered to the browser.
    expect(refusals).toEqual([]);
  });

  it('withdraws everything and records why when the browser refuses the tools', async () => {
    const context = promiseContext();
    context.tools.set('get_plan', {} as WebMcpToolDefinition);
    globals.document = { modelContext: context };
    registerWebMcp({ ready: async () => {}, load });
    await settle(() => useAgent.getState().registrationError !== null);
    expect(useAgent.getState()).toMatchObject({ available: false, toolCount: 0, registrationError: 'Duplicate tool name' });
    expect([...context.tools.keys()]).toEqual(['get_plan']);
  });
});

// ---- the tool set ---------------------------------------------------------------------

describe('the tool set', () => {
  it('is these tools, by these names', () => {
    expect(AGENT_TOOL_NAMES).toEqual([
      'create_plan', 'edit_plan', 'explain_tax', 'get_app_state', 'get_chart_data', 'get_guide', 'get_plan', 'get_results', 'get_schema', 'inspect_life', 'list_charts', 'list_scenarios',
      'lookup_cost_of_living', 'open_demo', 'open_plan', 'rank_levers', 'remove_scenario', 'run_simulation', 'set_scenario_enabled', 'show_view', 'solve', 'sweep', 'try_scenario', 'undo_agent_edit',
    ]);
    expect(AGENT_TOOL_NAMES).not.toContain('show_chart');
  });

  it('describes every tool for an agent that has no system prompt', () => {
    for (const t of AGENT_TOOLS) {
      expect(t.name).toMatch(/^[A-Za-z0-9_.-]{1,128}$/);
      expect(t.title.length, t.name).toBeGreaterThan(8);
      expect(t.description.length, t.name).toBeGreaterThan(200);
      expect(t.description.length, t.name).toBeLessThan(2000);
      // Phrases that only make sense inside the in-app assistant.
      expect(t.description, t.name).not.toMatch(/this conversation|your instructions|propose_changes|show_chart|this turn|attached to the user's message/);
      expect(t.inputSchema.type).toBe('object');
      expect(JSON.parse(JSON.stringify(t.inputSchema)), t.name).toEqual(t.inputSchema);
      expect(typeof t.annotations.readOnlyHint).toBe('boolean');
      // A tool cannot both change nothing and be consequential.
      expect(t.annotations.readOnlyHint && t.annotations.consequentialHint, t.name).toBe(false);
    }
    const size = JSON.stringify(AGENT_TOOLS.map((t) => ({ name: t.name, title: t.title, description: t.description, inputSchema: t.inputSchema, annotations: t.annotations }))).length;
    expect(size).toBeLessThan(42_000);
  });

  it('marks writes as consequential, reads as read-only, and anything echoing user text as untrusted', () => {
    const where = (pick: (t: (typeof AGENT_TOOLS)[number]) => boolean) => AGENT_TOOLS.filter(pick).map((t) => t.name);
    expect(where((t) => t.annotations.consequentialHint)).toEqual(['create_plan', 'edit_plan', 'open_demo', 'open_plan', 'remove_scenario', 'undo_agent_edit']);
    expect(where((t) => !t.annotations.readOnlyHint && !t.annotations.consequentialHint)).toEqual(['set_scenario_enabled', 'show_view', 'try_scenario']);
    for (const name of ['get_plan', 'get_results', 'list_charts', 'get_chart_data', 'list_scenarios', 'inspect_life', 'get_app_state', 'create_plan', 'edit_plan']) {
      expect(tool(name).annotations.untrustedContentHint, name).toBe(true);
    }
    // Only static text and the state tool answer before the user has let agents in.
    expect(where((t) => t.consent === 'none')).toEqual(['get_app_state', 'get_guide', 'get_schema']);
    expect(where((t) => t.worksWhenOff === true)).toEqual(['get_app_state']);
  });

  it('leaves the assistant\'s twelve tool definitions exactly as they were', () => {
    expect(TOOL_NAMES).toHaveLength(12);
    // The hosted proxy checks these names, and conversations are fingerprinted by the definitions.
    expect(TOOL_NAMES).toEqual(['explain_tax', 'get_plan', 'get_schema', 'inspect_life', 'lookup_cost_of_living', 'propose_changes', 'rank_levers', 'run_simulation', 'show_chart', 'solve', 'sweep', 'try_scenario']);
    for (const definition of toolDefinitions()) {
      const mine = AGENT_TOOLS.find((t) => t.name === (definition.name === 'propose_changes' ? 'edit_plan' : definition.name));
      if (definition.name === 'show_chart') expect(mine).toBeUndefined();
      else {
        expect(mine?.inputSchema, definition.name).toEqual(definition.input_schema);
        expect(mine?.description, definition.name).not.toBe(definition.description);
      }
    }
  });

  it('offers show_view exactly the views the app has', () => {
    expect(tool('show_view').inputSchema.properties?.view?.enum).toEqual(VIEWS.map((v) => v.id));
  });
});

// ---- the call contract ----------------------------------------------------------------

describe('a tool call', () => {
  it('works when the browser passes { signal }, when it passes nothing, and when the input arrives as a JSON string', async () => {
    openDemo();
    const definition = toDefinition(tool('list_scenarios'), world.ctx);
    expect(await definition.execute({}, { signal: new AbortController().signal })).toMatchObject({ ok: true, scenarios: [] });
    expect(await definition.execute({})).toMatchObject({ ok: true, scenarios: [] });
    expect(await definition.execute(undefined)).toMatchObject({ ok: true });
    expect(await definition.execute('{}')).toMatchObject({ ok: true });
    // Firefox's prototype passes a client object, not { signal }.
    expect(await definition.execute({}, { requestUserInteraction: () => undefined })).toMatchObject({ ok: true });
    expect(await toDefinition(tool('get_guide'), world.ctx).execute('{"topic":"limits"}')).toMatchObject({ ok: true, topic: 'limits' });
  });

  it('checks the input against the schema itself and answers with an error object', async () => {
    openDemo();
    expect(await failure('show_view', { view: 'nowhere' })).toMatchObject({ code: 'invalid_input' });
    expect((await failure('edit_plan', { ops: [] })).problems).toEqual(expect.arrayContaining(['input.summary is required', 'input.ops needs at least 1 item(s)']));
    expect(await failure('get_chart_data', {})).toMatchObject({ code: 'invalid_input' });
    expect(await failure('get_results', { timeout_seconds: 0 })).toMatchObject({ code: 'invalid_input' });
    expect(await failure('list_charts', { extra: true })).toMatchObject({ code: 'invalid_input' });
    expect(await failure('get_plan', 'not json')).toMatchObject({ code: 'invalid_input' });
    expect(await failure('create_plan', { profile: [] })).toMatchObject({ code: 'invalid_input' });
  });

  it('never throws: a failure inside the app comes back as an error object', async () => {
    openDemo();
    world.ctx.app.views = () => {
      throw new Error('boom');
    };
    const result = await call('show_view', { view: 'outcomes' });
    expect(result).toMatchObject({ ok: false, error: { code: 'internal_error' } });
    expect(useAgent.getState().log.at(-1)).toMatchObject({ tool: 'show_view', outcome: 'error', code: 'internal_error' });
  });

  it('says so when no plan is open, and names the tools that open one', async () => {
    for (const name of ['get_plan', 'run_simulation', 'get_results', 'list_charts', 'list_scenarios', 'undo_agent_edit', 'show_view']) {
      const error = await failure(name, name === 'show_view' ? { view: 'outcomes' } : {});
      expect(error.code, name).toBe('no_plan');
      expect(error.message, name).toMatch(/create_plan, open_demo or open_plan/);
    }
  });

  it('rejects, and only then, when the caller aborts', async () => {
    openDemo();
    world.settings.access = 'ask';
    const controller = new AbortController();
    const pending = call('get_plan', {}, { signal: controller.signal });
    await untilAsked('consent');
    controller.abort();
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(useAgent.getState().log.at(-1)).toMatchObject({ outcome: 'cancelled' });
    expect(useAgent.getState().inFlight).toBe(0);

    const already = new AbortController();
    already.abort();
    await expect(call('get_plan', {}, { signal: already.signal })).rejects.toMatchObject({ name: 'AbortError' });
  });

  it('waits for the app to be ready, and reports a failed start as an error', async () => {
    const slow = makeWorld();
    world = slow;
    let done = false;
    const pending = call('get_guide', { topic: 'limits' }).then((r) => {
      done = true;
      return r;
    });
    await tick(5);
    // get_guide does not need the engine.
    expect(done).toBe(true);
    await pending;

    let schemaDone = false;
    const schema = call('get_schema', { section: 'Person' }).then((r) => {
      schemaDone = true;
      return r;
    });
    await tick(10);
    expect(schemaDone).toBe(false);
    expect(await call('get_app_state')).toMatchObject({ ok: true, ready: false });
    slow.ready.resolve();
    expect(await schema).toMatchObject({ ok: true, name: 'Person' });

    const broken = makeWorld();
    world = broken;
    broken.ready.reject(new Error('wasm blocked'));
    expect(await failure('get_plan')).toMatchObject({ code: 'app_failed' });
    expect(await call('get_app_state')).toMatchObject({ ok: true, ready: false });
  });

  it('keeps a log of the last 50 calls, without their inputs or results', async () => {
    openDemo();
    for (let i = 0; i < MAX_LOG + 5; i++) await call('list_scenarios');
    const { log, inFlight } = useAgent.getState();
    expect(log).toHaveLength(MAX_LOG);
    expect(inFlight).toBe(0);
    expect(Object.keys(log[0]!).sort()).toEqual(['at', 'id', 'ms', 'outcome', 'tool']);
  });
});

// ---- consent --------------------------------------------------------------------------

describe('consent', () => {
  it('off: every tool refuses except get_app_state, which says nothing about the plan', async () => {
    openDemo();
    world.settings.access = 'off';
    for (const t of AGENT_TOOLS) {
      if (t.name === 'get_app_state') continue;
      expect(await call(t.name, {}), t.name).toMatchObject({ ok: false, error: { code: 'agent_access_off' } });
    }
    const stateResult = await ok('get_app_state');
    expect(stateResult.agent_access).toMatchObject({ mode: 'off', status: 'off' });
    expect(Object.keys(stateResult).sort()).toEqual(['agent_access', 'app', 'note', 'ok']);
    expect(useAgent.getState().consentAsked).toBe(false);
    expect(useAgent.getState().log.at(-2)).toMatchObject({ outcome: 'refused', code: 'agent_access_off' });
  });

  it('ask: the first call that touches the plan waits for the user; allowing lets it and later calls through', async () => {
    openDemo();
    world.settings.access = 'ask';
    // State, the guide and the schema answer without asking, and state hides names until allowed.
    const before = await ok('get_app_state');
    expect(before.agent_access).toMatchObject({ mode: 'ask', status: 'will_ask_user' });
    expect(before.plan).toEqual({ open: true });
    expect(before).not.toHaveProperty('saved_plans');
    await ok('get_guide');
    await ok('get_schema', { section: 'Person' });
    expect(useAgent.getState().consentAsked).toBe(false);

    const first = call('get_plan');
    const second = call('list_scenarios');
    await untilAsked('consent');
    expect(useAgent.getState().inFlight).toBe(2);
    answerConsent(true);
    expect(await first).toMatchObject({ ok: true });
    expect(await second).toMatchObject({ ok: true });
    expect(useAgent.getState().consentAsked).toBe(false);

    await ok('list_scenarios');
    expect(useAgent.getState().consentAsked).toBe(false);
    const after = await ok('get_app_state');
    expect(after.agent_access).toMatchObject({ status: 'allowed' });
    expect(after.plan).toMatchObject({ id: 'demo_plan', validation: { can_run: true } });
    expect(after.saved_plans).toEqual([{ id: 'demo_plan', name: expect.any(String) }]);
  });

  it('ask: declining refuses that call and the ones after it, without asking again', async () => {
    openDemo();
    world.settings.access = 'ask';
    const pending = call('get_plan');
    await untilAsked('consent');
    answerConsent(false);
    expect(await pending).toMatchObject({ ok: false, error: { code: 'agent_access_denied' } });
    expect(await call('run_simulation')).toMatchObject({ ok: false, error: { code: 'agent_access_denied' } });
    expect(useAgent.getState().consentAsked).toBe(false);
    expect((await ok('get_app_state')).agent_access).toMatchObject({ status: 'denied' });
  });

  it('ask: an unanswered prompt times out as user_did_not_respond and stays up for the user', async () => {
    openDemo();
    world.settings.access = 'ask';
    const error = await failure('get_plan');
    expect(error).toMatchObject({ code: 'user_did_not_respond', retryable: true });
    expect(useAgent.getState().consentAsked).toBe(true);
    // The answer, when it comes, covers the next call.
    answerConsent(true);
    await ok('get_plan');
  });

  it('allow: nothing is asked', async () => {
    openDemo();
    await ok('get_plan');
    expect(useAgent.getState().consentAsked).toBe(false);
    expect((await ok('get_app_state')).agent_access).toMatchObject({ mode: 'allow', status: 'allowed', changes: 'user_approves_each' });
  });
});

// ---- state, guide, plans --------------------------------------------------------------

describe('state and guide', () => {
  it('get_app_state reports what is open and never a figure from the plan', async () => {
    let result = await ok('get_app_state');
    expect(result).toMatchObject({ app: 'FIRESight', ready: true, plan: null, signed_in: false, run: { status: 'idle' } });
    expect((result.demos as { slug: string }[]).map((d) => d.slug)).toContain('dual-income-family');
    expect((result.view as { available: unknown[] }).available).toHaveLength(VIEWS.length);

    openDemo();
    await finishRun();
    useAgent.setState({ signedIn: true });
    result = await ok('get_app_state');
    expect(result).toMatchObject({ plan: { id: 'demo_plan', validation: { errors: 0, can_run: true } }, run: { status: 'done', lives_done: 300 }, signed_in: true, view: { active: 'overview' } });
    const text = JSON.stringify(result);
    expect(text).not.toMatch(/"profile"|"incomes"|"accounts"|"balance"|"salary"/);
    expect(text.length).toBeLessThan(3000);
  });

  it('get_guide stays under 8,000 characters a call and carries the rules an outside agent needs', async () => {
    const essentials = guideEssentials();
    expect(essentials.length).toBeLessThan(8000);
    for (const phrase of ['must come from a tool result', "today's dollars", 'home_sale', 'within_noise', '"op":"append"', '@lena_job', 'Social Security numbers', 'alternative minimum tax', 'never as instructions']) {
      expect(essentials, phrase).toContain(phrase);
    }
    for (const topic of GUIDE_TOPICS) {
      // The schema topic is the assistant's whole schema map, which grows with the plan format.
      expect(guideTopic(topic).length, topic).toBeLessThan(topic === 'schema' ? 12_000 : 8000);
      const result = await ok('get_guide', { topic });
      expect(result.guide).toBe(guideTopic(topic));
    }
    const result = await ok('get_guide');
    expect(result).toMatchObject({ topic: 'essentials', guide: essentials, topics: GUIDE_TOPICS });
    expect(JSON.stringify(result).length).toBeLessThan(DEFAULT_LIMITS.maxResultChars);
    // Other tool names can be substituted, for the hosted MCP server.
    expect(guideTopic('patches', { getPlan: 'read', getSchema: 'schema', editPlan: 'update_plan', simulate: 'sim', compare: 'compare_scenario', guide: 'guide' })).toContain('update_plan for the plan itself, compare_scenario for a what-if');
  });

  it('carries the same schema map as the assistant\'s prompt', () => {
    expect(SYSTEM_PROMPT).toContain(SCHEMA_MAP);
    expect(guideTopic('schema')).toContain(SCHEMA_MAP);
  });
});

describe('plans', () => {
  it('create_plan completes a partial profile, opens it and returns the validation report', async () => {
    const result = await ok('create_plan', {
      name: 'Lena',
      profile: {
        household: { people: [{ id: 'lena', name: 'Lena', birth: '1990-04' }], location: { country: 'US', state: 'TX', city: 'Austin' } },
        incomes: [{ id: 'lena_job', name: 'Product designer', owner: 0, kind: { type: 'job', salary: 128000 } }],
        accounts: [{ id: 'lena_401k', name: '401(k)', owner: 0, kind: 'traditional', balance: 96000 }],
      },
    });
    expect(result).toMatchObject({ plan: { name: 'Lena' }, validation: { can_run: true, errors: [] } });
    expect(state().profile).toMatchObject({ name: 'Lena', incomes: [{ id: 'lena_job' }] });
    // Defaults were filled in by the engine.
    expect(state().profile?.retirement.trigger).toBeTruthy();
    expect(world.notices.at(-1)?.title).toMatch(/created the plan "Lena"/);
    expect(world.ctx.app.savedPlans()).toEqual([{ id: (result.plan as { id: string }).id, name: 'Lena' }]);
  });

  it('create_plan accepts an empty profile, and a plan file with scenarios', async () => {
    const empty = await ok('create_plan', { profile: {} });
    expect(empty.validation).toMatchObject({ can_run: expect.any(Boolean) });
    const profile = demo();
    const file = { firesight: 2, profile, scenarios: [{ id: 'later', name: 'Retire later', enabled: true, patch: [{ op: 'set', path: '/retirement/bequest_target', value: 5000 }] }, { nonsense: true }] };
    await ok('create_plan', { profile: file });
    expect(state().profile?.name).toBe(profile.name);
    expect(state().scenarios.map((s) => s.id)).toEqual(['later']);
  });

  it('create_plan refuses what is not a profile, without touching the open plan', async () => {
    openDemo();
    const before = state().profile;
    expect(await failure('create_plan', { profile: { household: { people: 'two' } } })).toMatchObject({ code: 'invalid_profile' });
    expect(await failure('create_plan', { profile: { incomes: [{ id: 'x', kind: { type: 'lottery' } }] } })).toMatchObject({ code: 'invalid_profile' });
    expect(await failure('create_plan', { profile: { accounts: [{ id: 'a', name: 'x'.repeat(250_000) }] } })).toMatchObject({ code: 'invalid_profile' });
    expect(state().profile).toBe(before);
    expect(useAgent.getState().approvals).toHaveLength(0);
  });

  it('asks before replacing a plan that has changes from this session', async () => {
    openDemo();
    state().edit([{ op: 'set', path: '/retirement/bequest_target', value: 1000 }], { label: 'By hand' });
    const pending = call('open_demo', { slug: 'dual-income-family' });
    await untilAsked('approval');
    const card = useAgent.getState().approvals[0]!;
    expect(card).toMatchObject({ kind: 'confirm', tool: 'open_demo', confirmLabel: 'Open it' });
    answerApproval(card.id, { type: 'decline' });
    expect(await pending).toMatchObject({ ok: false, error: { code: 'declined' } });
    expect(state().meta?.id).toBe('demo_plan');

    const again = call('create_plan', { profile: {}, name: 'Fresh' });
    await untilAsked('approval');
    answerApproval(useAgent.getState().approvals[0]!.id, { type: 'accept' });
    expect(await again).toMatchObject({ ok: true, plan: { name: 'Fresh' } });
    expect(state().meta?.id).not.toBe('demo_plan');

    // No history on the open plan: nothing to ask.
    await ok('open_plan', { id: 'demo_plan' });
    expect(state().meta?.id).toBe('demo_plan');
  });

  it('open_demo and open_plan open what was asked for, and say what exists otherwise', async () => {
    const opened = await ok('open_demo', { slug: 'dual-income-family' });
    const id = (opened.plan as { id: string }).id;
    expect(opened.validation).toMatchObject({ can_run: true });
    expect(await ok('open_plan', { id })).toMatchObject({ already_open: true });
    expect((await failure('open_demo', { slug: 'nope' })).message).toContain('dual-income-family');
    expect(await failure('open_plan', { id: 'nope' })).toMatchObject({ code: 'not_found' });
    // Nothing of a saved plan can be read without opening it.
    expect(JSON.stringify(await ok('get_app_state'))).not.toContain('"profile"');
  });
});

// ---- changes --------------------------------------------------------------------------

const RAISE = { summary: 'Raise the bequest target', ops: [{ op: 'set', path: '/retirement/bequest_target', value: 250000 }] };

describe('edit_plan', () => {
  it('shows the user a change card and applies only when they accept, as one undo step labelled as the agent\'s', async () => {
    openDemo();
    const pending = call('edit_plan', RAISE);
    await untilAsked('approval');
    const card = useAgent.getState().approvals[0]!;
    expect(card).toMatchObject({ kind: 'change', tool: 'edit_plan', proposal: { summary: RAISE.summary, entries: [{ op: 'set', path: '/retirement/bequest_target', after: 250000 }] } });
    expect(state().profile?.retirement.bequest_target).not.toBe(250000);
    answerApproval(card.id, { type: 'accept' });
    const result = await pending;
    expect(result).toMatchObject({ ok: true, status: 'accepted', applied_ops: 1 });
    expect(state().profile?.retirement.bequest_target).toBe(250000);
    expect(state().past).toHaveLength(1);
    expect(state().past[0]).toMatchObject({ label: 'Agent: Raise the bequest target', target: 'base' });
    expect(useAgent.getState().approvals).toHaveLength(0);
  });

  it('writes to the baseline even when the user\'s editor is pointed at a scenario', async () => {
    openDemo();
    const added = state().addScenario({ name: 'Other', patch: [{ op: 'set', path: '/retirement/bequest_target', value: 1 }] });
    state().setEditTarget(added.id as string);
    world.settings.autoApply = true;
    await ok('edit_plan', RAISE);
    expect(state().profile?.retirement.bequest_target).toBe(250000);
    expect(state().past.at(-1)).toMatchObject({ target: 'base' });
  });

  it('reports the user\'s edits, a decline and an unanswered card as what they are', async () => {
    openDemo();
    let pending = call('edit_plan', RAISE);
    await untilAsked('approval');
    answerApproval(useAgent.getState().approvals[0]!.id, { type: 'accept_edited', ops: [{ op: 'set', path: '/retirement/bequest_target', value: 100000 }] });
    expect(await pending).toMatchObject({ ok: true, status: 'accepted_with_edits', final_ops: [{ value: 100000 }] });
    expect(state().profile?.retirement.bequest_target).toBe(100000);

    pending = call('edit_plan', RAISE);
    await untilAsked('approval');
    answerApproval(useAgent.getState().approvals[0]!.id, { type: 'decline' });
    expect(await pending).toMatchObject({ ok: false, error: { code: 'declined' } });
    expect(state().profile?.retirement.bequest_target).toBe(100000);

    const unanswered = await failure('edit_plan', RAISE);
    expect(unanswered).toMatchObject({ code: 'user_did_not_respond', retryable: true });
    // The card is withdrawn, so a stale proposal cannot be accepted later.
    expect(useAgent.getState().approvals).toHaveLength(0);
    expect(state().profile?.retirement.bequest_target).toBe(100000);
    expect(useAgent.getState().log.at(-1)).toMatchObject({ tool: 'edit_plan', outcome: 'refused', code: 'user_did_not_respond' });
  });

  it('returns a patch that does not apply, or breaks the plan, without bothering the user', async () => {
    openDemo();
    const missing = await failure('edit_plan', { summary: 'x', ops: [{ op: 'set', path: '/incomes/@nobody/kind/salary', value: 1 }] });
    expect(missing.code).toBe('tool_error');
    expect(missing.message).toMatch(/nobody/);
    const invalid = await failure('edit_plan', { summary: 'x', ops: [{ op: 'set', path: '/household/people', value: [] }] });
    expect(invalid.message).toMatch(/edit_plan|errors/);
    expect(invalid.message).not.toContain('propose_changes');
    expect(useAgent.getState().approvals).toHaveLength(0);
    expect(state().past).toHaveLength(0);
  });

  it('applies at once, with a notice the user can undo from, when they turned on automatic apply', async () => {
    openDemo();
    world.settings.autoApply = true;
    expect(await ok('edit_plan', RAISE)).toMatchObject({ status: 'accepted', automatically: true });
    expect(useAgent.getState().approvals).toHaveLength(0);
    expect(world.notices.at(-1)).toMatchObject({ title: 'A browser agent changed the plan', undo: true });
    expect((await ok('get_app_state')).agent_access).toMatchObject({ changes: 'applied_without_asking' });
  });

  it('cancels a waiting card when the caller aborts', async () => {
    openDemo();
    const controller = new AbortController();
    const pending = call('edit_plan', RAISE, { signal: controller.signal });
    await untilAsked('approval');
    controller.abort();
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(useAgent.getState().approvals).toHaveLength(0);
    expect(state().past).toHaveLength(0);
  });
});

describe('a card belongs to the plan it was raised for', () => {
  const bequest = () => state().profile?.retirement.bequest_target;

  it('edit_plan: accepted after another plan was opened, it changes nothing and says plan_changed', async () => {
    openDemo('plan_a');
    const before = bequest();
    const pending = call('edit_plan', RAISE);
    await untilAsked('approval');
    const card = useAgent.getState().approvals[0]!;
    openDemo('plan_b');
    answerApproval(card.id, { type: 'accept' });
    expect(await pending).toMatchObject({ ok: false, error: { code: 'plan_changed' }, retryable: true });
    expect(state().meta?.id).toBe('plan_b');
    expect(bequest()).toBe(before);
    expect(state().past).toHaveLength(0);
  });

  it('try_scenario keep: the scenario is not saved into another plan, whether it was opened under the card or under the simulation', async () => {
    openDemo('plan_a');
    let pending = call('try_scenario', { ...LATER, keep: true });
    await untilAsked('approval');
    openDemo('plan_b');
    answerApproval(useAgent.getState().approvals[0]!.id, { type: 'accept' });
    expect(await pending).toMatchObject({ ok: true, comparison: { paired: true }, saved_scenario: { saved: false, reason: 'plan_changed' } });
    expect(state().scenarios).toHaveLength(0);

    // With automatic apply there is no card to wait on, only the simulation.
    world.settings.autoApply = true;
    const pool = world.engine.pool;
    const simulate = pool.runMany.bind(pool);
    pool.runMany = (jobs, options) => {
      openDemo('plan_c');
      return simulate(jobs, options);
    };
    pending = call('try_scenario', { ...LATER, keep: true });
    expect(await pending).toMatchObject({ ok: true, saved_scenario: { saved: false, reason: 'plan_changed' } });
    expect(state().meta?.id).toBe('plan_c');
    expect(state().scenarios).toHaveLength(0);
    expect(world.notices).toHaveLength(0);
  });

  it('remove_scenario: a scenario with the same id in another plan is left alone', async () => {
    const scenarios: Scenario[] = [{ id: 'leave_more', name: 'Leave more', patch: LATER.ops as never, enabled: true }];
    state().load({ meta: { id: 'plan_a', createdAt: 1, origin: 'test' }, profile: demo(), scenarios });
    const pending = call('remove_scenario', { id: 'leave_more' });
    await untilAsked('approval');
    state().load({ meta: { id: 'plan_b', createdAt: 1, origin: 'test' }, profile: demo(), scenarios });
    answerApproval(useAgent.getState().approvals[0]!.id, { type: 'accept' });
    expect(await pending).toMatchObject({ ok: false, error: { code: 'plan_changed' } });
    expect(state().scenarios).toMatchObject([{ id: 'leave_more' }]);
    expect(state().past).toHaveLength(0);
  });

  it('the page takes pending cards down when another plan is opened, or the same one is loaded again', async () => {
    const stop = watchApprovals();
    try {
      openDemo('plan_a');
      const before = bequest();
      const edit = call('edit_plan', RAISE);
      await untilAsked('approval');
      // The agent's own open_demo asks nothing here: the open plan has no changes from this session.
      const opened = await ok('open_demo', { slug: exampleProfiles()[0]!.slug });
      expect(useAgent.getState().approvals).toHaveLength(0);
      expect(await edit).toMatchObject({ ok: false, error: { code: 'plan_changed' } });
      expect(state().meta?.id).toBe((opened.plan as Json).id);
      expect(state().past).toHaveLength(0);

      // A reload of the same plan (account sync replacing it) keeps the id: the card still goes.
      openDemo('plan_a');
      const again = call('edit_plan', RAISE);
      await untilAsked('approval');
      openDemo('plan_a');
      expect(useAgent.getState().approvals).toHaveLength(0);
      expect(await again).toMatchObject({ ok: false, error: { code: 'plan_changed' } });
      expect(bequest()).toBe(before);
    } finally {
      stop();
    }
  });
});

describe('switching agent access to Off', () => {
  const access = (value: AgentAccess) => {
    world.settings.access = value;
    useSettings.setState({ agentAccess: value });
  };
  let stop = () => {};
  beforeEach(() => {
    useSettings.setState({ agentAccess: 'allow' });
    stop = watchApprovals();
  });
  afterEach(() => {
    stop();
    useSettings.setState({ agentAccess: 'ask' });
  });

  it('withdraws pending cards and refuses in-flight calls', async () => {
    openDemo();
    const before = state().profile?.retirement.bequest_target;
    const edit = call('edit_plan', RAISE);
    await untilAsked('approval');
    const card = useAgent.getState().approvals[0]!;
    // A read that is already past the access check, waiting on the page's run.
    world.setRun({ status: 'running', done: 100, total: 300 });
    const read = call('get_results');
    await tick(5);

    access('off');
    expect(useAgent.getState().approvals).toHaveLength(0);
    // The card is gone; an answer that was already on its way does nothing.
    answerApproval(card.id, { type: 'accept' });
    expect(await edit).toMatchObject({ ok: false, error: { code: 'agent_access_off' } });
    expect(state().profile?.retirement.bequest_target).toBe(before);
    expect(state().past).toHaveLength(0);

    await finishRun();
    const refused = await read;
    expect(refused).toMatchObject({ ok: false, error: { code: 'agent_access_off' } });
    expect(Object.keys(refused).sort()).toEqual(['error', 'ok']);
    expect(useAgent.getState().log.slice(-2).map((e) => e.outcome)).toEqual(['refused', 'refused']);
  });

  it('holds without the page\'s help: an accept that arrives after Off writes nothing, and no new card is raised', async () => {
    stop();
    openDemo();
    const edit = call('edit_plan', RAISE);
    await untilAsked('approval');
    world.settings.access = 'off';
    answerApproval(useAgent.getState().approvals[0]!.id, { type: 'accept' });
    expect(await edit).toMatchObject({ ok: false, error: { code: 'agent_access_off' } });
    expect(state().past).toHaveLength(0);

    // A what-if that was simulating when access went off must not put its "save?" card up afterwards.
    world.settings.access = 'allow';
    const keep = call('try_scenario', { ...LATER, keep: true });
    world.settings.access = 'off';
    expect(await keep).toMatchObject({ ok: false, error: { code: 'agent_access_off' } });
    expect(useAgent.getState().approvals).toHaveLength(0);
    expect(state().scenarios).toHaveLength(0);
  });

  it('a call waiting on the consent prompt is told access is off, not that it was denied', async () => {
    openDemo();
    access('ask');
    const pending = call('get_plan');
    await untilAsked('consent');
    access('off');
    resetConsent();
    expect(await pending).toMatchObject({ ok: false, error: { code: 'agent_access_off' } });
  });

  it('withdrawApprovals settles every waiting card as withdrawn', async () => {
    const a = askApproval({ kind: 'confirm', tool: 't', title: 'a', body: '', confirmLabel: 'ok' }, undefined, 5000);
    const b = askApproval({ kind: 'confirm', tool: 't', title: 'b', body: '', confirmLabel: 'ok' }, undefined, 5000);
    expect(useAgent.getState().approvals).toHaveLength(2);
    withdrawApprovals();
    expect(await Promise.all([a, b])).toEqual(['withdrawn', 'withdrawn']);
    expect(useAgent.getState().approvals).toHaveLength(0);
  });
});

describe('undo_agent_edit', () => {
  it('undoes the agent\'s own latest change and nothing else', async () => {
    openDemo();
    world.settings.autoApply = true;
    expect(await failure('undo_agent_edit')).toMatchObject({ code: 'nothing_to_undo' });
    const original = state().profile?.retirement.bequest_target;
    await ok('edit_plan', RAISE);
    await ok('edit_plan', { summary: 'And again', ops: [{ op: 'set', path: '/retirement/bequest_target', value: 300000 }] });
    expect(await ok('undo_agent_edit')).toMatchObject({ undone: 'Agent: And again' });
    expect(state().profile?.retirement.bequest_target).toBe(250000);

    // The user edits: the top of the stack is theirs now.
    state().edit([{ op: 'set', path: '/retirement/bequest_target', value: 5 }], { label: 'Set the bequest by hand' });
    const refused = await failure('undo_agent_edit');
    expect(refused.code).toBe('not_an_agent_edit');
    expect(refused.message).toContain('Set the bequest by hand');
    expect(state().profile?.retirement.bequest_target).toBe(5);

    state().undo();
    expect(await ok('undo_agent_edit')).toMatchObject({ undone: 'Agent: Raise the bequest target' });
    expect(state().profile?.retirement.bequest_target).toBe(original);
    // A label alone does not make an edit the agent's.
    state().edit([{ op: 'set', path: '/retirement/bequest_target', value: 7 }], { label: 'Agent: not really' });
    expect(await failure('undo_agent_edit')).toMatchObject({ code: 'not_an_agent_edit' });
  });
});

// ---- scenarios ------------------------------------------------------------------------

const LATER = { name: 'Leave more', ops: [{ op: 'set', path: '/retirement/bequest_target', value: 400000 }], paths: 200 };

describe('scenarios', () => {
  it('try_scenario compares on the same lives and leaves the plan alone', async () => {
    openDemo();
    const before = state().profile;
    const result = await ok('try_scenario', LATER);
    expect(result).toMatchObject({ scenario: 'Leave more', paths: 200, comparison: { paired: true, paired_paths: 200 } });
    expect(result).not.toHaveProperty('saved_scenario');
    expect(state().profile).toBe(before);
    expect(state().scenarios).toHaveLength(0);
    expect(JSON.stringify(result).length).toBeLessThan(8000);
  });

  it('try_scenario with keep saves only when the user says so, and returns the comparison either way', async () => {
    openDemo();
    let pending = call('try_scenario', { ...LATER, keep: true });
    await untilAsked('approval');
    const card = useAgent.getState().approvals[0]!;
    expect(card).toMatchObject({ kind: 'confirm', tool: 'try_scenario', confirmLabel: 'Save scenario' });
    expect(card.kind === 'confirm' && card.title).toContain('Leave more');
    answerApproval(card.id, { type: 'decline' });
    expect(await pending).toMatchObject({ ok: true, comparison: { paired: true }, saved_scenario: { saved: false, reason: 'declined' } });
    expect(state().scenarios).toHaveLength(0);

    pending = call('try_scenario', { ...LATER, keep: true });
    await untilAsked('approval');
    answerApproval(useAgent.getState().approvals[0]!.id, { type: 'accept' });
    const saved = await pending;
    expect(saved).toMatchObject({ ok: true, saved_scenario: { saved: true, id: 'leave_more' } });
    expect(state().scenarios).toMatchObject([{ id: 'leave_more', name: 'Leave more', enabled: true }]);

    expect(await ok('try_scenario', { ...LATER, name: 'Unanswered', keep: true })).toMatchObject({ saved_scenario: { saved: false, reason: 'user_did_not_respond' } });
    expect(state().scenarios).toHaveLength(1);
    // Saving it was the agent's own change, so it can take it back.
    expect(await ok('undo_agent_edit')).toMatchObject({ undone: 'Add scenario "Leave more"' });
    expect(state().scenarios).toHaveLength(0);
  });

  it('lists, switches and deletes scenarios, asking before a deletion', async () => {
    openDemo();
    state().addScenario({ name: 'Leave more', patch: LATER.ops as never });
    expect(await ok('list_scenarios')).toMatchObject({ scenarios: [{ id: 'leave_more', enabled: true, ops: [{ op: 'set' }] }] });
    expect(await ok('set_scenario_enabled', { id: 'leave_more', enabled: true })).toMatchObject({ unchanged: true });
    expect(await ok('set_scenario_enabled', { id: 'leave_more', enabled: false })).toMatchObject({ scenario: { enabled: false } });
    expect(state().scenarios[0]?.enabled).toBe(false);
    expect((await failure('set_scenario_enabled', { id: 'nope', enabled: true })).message).toContain('leave_more');

    let pending = call('remove_scenario', { id: 'leave_more' });
    await untilAsked('approval');
    answerApproval(useAgent.getState().approvals[0]!.id, { type: 'decline' });
    expect(await pending).toMatchObject({ ok: false, error: { code: 'declined' } });
    expect(state().scenarios).toHaveLength(1);

    pending = call('remove_scenario', { id: 'leave_more' });
    await untilAsked('approval');
    answerApproval(useAgent.getState().approvals[0]!.id, { type: 'accept' });
    expect(await pending).toMatchObject({ ok: true, removed: { id: 'leave_more' }, scenarios: [] });
    expect(await failure('remove_scenario', { id: 'leave_more' })).toMatchObject({ code: 'not_found' });
    // Deleting and switching were the agent's: two undo steps bring the scenario back, enabled.
    await ok('undo_agent_edit');
    await ok('undo_agent_edit');
    expect(state().scenarios).toMatchObject([{ id: 'leave_more', enabled: true }]);
  });
});

// ---- results and charts ---------------------------------------------------------------

describe('get_results', () => {
  it('returns the headline figures of the app\'s own run, compactly, never the Summary', async () => {
    openDemo();
    await finishRun();
    const result = await ok('get_results');
    expect(result).toMatchObject({ complete: true, lives: 300, plan: { id: 'demo_plan' }, view: 'overview' });
    const success = result.success as { p: number; lo: number; hi: number };
    expect(success.p).toBeCloseTo(world.run.summary!.headline.success_rate.value, 3);
    expect(success.lo).toBeLessThanOrEqual(success.p);
    expect(result.home_sale).toBeCloseTo(world.run.summary!.headline.home_sale_rate.value, 3);
    expect(result.plan_facts).toMatchObject({ annual_spending: expect.any(Number), funded_requirement: expect.any(Number) });
    expect(result.notes).toContain('home_sale');
    expect(JSON.stringify(result).length).toBeLessThan(3000);
    expect(JSON.stringify(world.run.summary).length).toBeGreaterThan(50_000);
  });

  it('waits for a run in progress to settle', async () => {
    openDemo();
    world.setRun({ status: 'running', done: 100, total: 300 });
    let done = false;
    const pending = call('get_results').then((r) => {
      done = true;
      return r;
    });
    await tick(20);
    expect(done).toBe(false);
    await finishRun();
    expect(await pending).toMatchObject({ ok: true, complete: true });
  });

  it('includes enabled scenarios with their paired difference, and waits for the pairing', async () => {
    openDemo();
    const s = state();
    const profile = s.profile as Profile;
    const other = s.preview([{ op: 'set', path: '/retirement/bequest_target', value: 400000 }]);
    if ('error' in other) throw new Error(other.error);
    const [a, b] = await world.ctx.runs.simulate(world.engine.pool, [{ label: 'a', profile }, { label: 'b', profile: other.profile }], { paths: 200, seed: s.run.seed, signal: new AbortController().signal });
    const scenario = { id: 'leave_more', name: 'Leave more', profile: other.profile, summary: b!.summary, done: 200, paired: null, pairing: 'pending' as const };
    world.setRun({ status: 'done', stale: false, done: 400, total: 400, summary: a!.summary, summaryPaths: 200, preview: planPreview(profile), scenarios: [scenario] });
    const early = await ok('get_results', { wait: false });
    expect(early).toMatchObject({ complete: false, scenarios: [{ id: 'leave_more' }] });
    expect((early.scenarios as Json[])[0]).not.toHaveProperty('versus_plan');

    const pending = call('get_results');
    await tick(5);
    world.setRun({ scenarios: [{ ...scenario, paired: await world.engine.compare(a!, b!), pairing: 'current' }] });
    const result = await pending;
    expect(result).toMatchObject({ ok: true, complete: true, scenarios: [{ id: 'leave_more', name: 'Leave more', versus_plan: { paired: true, paired_paths: 200, success: { within_noise: expect.any(Boolean) } } }] });
    expect(JSON.stringify(result).length).toBeLessThan(5000);
  });

  it('says why there are no figures: errors in the plan, a failed engine, a run that has not finished', async () => {
    openDemo();
    state().edit([{ op: 'set', path: '/household/people', value: [] }]);
    world.setRun({ status: 'blocked', stale: true });
    const blocked = await failure('get_results');
    expect(blocked.code).toBe('plan_has_errors');
    expect((blocked.errors as string[]).length).toBeGreaterThan(0);

    world.setRun({ status: 'error', error: { kind: 'not_implemented', message: 'x' } });
    expect((await failure('get_results')).message).toMatch(/do not estimate/);

    world.setRun({ status: 'running', error: null, done: 10, total: 300, summary: null });
    const started = Date.now();
    expect(await failure('get_results', { timeout_seconds: 1 })).toMatchObject({ code: 'still_running', retryable: true, lives_done: 10 });
    expect(Date.now() - started).toBeGreaterThanOrEqual(900);
    expect(await failure('get_results', { wait: false })).toMatchObject({ code: 'still_running' });

    const controller = new AbortController();
    const pending = call('get_results', {}, { signal: controller.signal });
    await tick(5);
    controller.abort();
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
  });
});

describe('views and charts', () => {
  const rows = (n: number) => Array.from({ length: n }, (_, i) => ({ age: 40 + i, p50: 1_000_000 + i * 1234.56789, label: i % 2 ? 'retired' : null }));
  const netWorth = (n: number): ChartEntry => ({
    id: 'net-worth',
    title: 'Net worth among the living',
    takeaway: () => 'Half of lives end   above $1.2M.',
    caption: () => null,
    table: () => ({ caption: 'Net worth by age', columns: [{ key: 'age', label: 'Age' }, { key: 'p50', label: 'Median ($)' }, { key: 'label', label: 'Status' }], rows: rows(n) }),
  });

  it('show_view navigates the app, and opens one life in Life stories', async () => {
    openDemo();
    expect(await ok('show_view', { view: 'cash-flow' })).toMatchObject({ view: 'cash-flow', label: 'Cash flow', url: '/cash-flow' });
    expect(world.view).toBe('cash-flow');
    expect(await ok('show_view', { life_path: 42 })).toMatchObject({ view: 'life-stories', life_path: 42, url: '/life-stories?path=42&why=sample' });
    expect(await failure('show_view', { view: 'outcomes', life_path: 3 })).toMatchObject({ code: 'invalid_input' });
    expect(await failure('show_view', {})).toMatchObject({ code: 'invalid_input' });
  });

  it('list_charts reads each chart on screen as text, waiting for a view that is still drawing', async () => {
    openDemo();
    expect(await ok('list_charts')).toMatchObject({ view: 'overview', charts: [] });
    setTimeout(() => world.charts.push(netWorth(5), { id: 'quiet', title: 'A sketch', takeaway: () => null, caption: () => 'Small print.', table: null }), 5);
    world.limits.chartsMs = 1000;
    world.limits.chartsGraceMs = 1000;
    expect(await ok('list_charts')).toMatchObject({
      charts: [
        { id: 'net-worth', title: 'Net worth among the living', takeaway: 'Half of lives end above $1.2M.', caption: null, has_table: true },
        { id: 'quiet', takeaway: null, caption: 'Small print.', has_table: false },
      ],
    });
  });

  it('list_charts waits longer while the page says the view is loading, and not at all long for a view with no charts', async () => {
    openDemo();
    world.limits.chartsMs = 2000;
    world.limits.chartsGraceMs = 20;
    // The view that was asked for is still loading behind the previous one: its charts arrive after the grace period.
    world.chartsPending = true;
    setTimeout(() => {
      world.chartsPending = false;
      world.charts.push(netWorth(5));
    }, 150);
    expect((await ok('list_charts')).charts).toHaveLength(1);
    // Nothing is loading and nothing is drawn: the answer is an empty list, soon.
    world.charts.length = 0;
    const started = Date.now();
    expect(await ok('list_charts')).toMatchObject({ charts: [], note: expect.stringContaining('Nothing is drawn') });
    expect(Date.now() - started).toBeLessThan(1500);
    // A view that never finishes loading is given up on at the limit.
    world.chartsPending = true;
    world.limits.chartsMs = 120;
    expect((await ok('list_charts')).charts).toEqual([]);
  });

  it('get_chart_data returns the rows behind a chart, paged and under the size cap', async () => {
    openDemo();
    world.charts.push(netWorth(500), { id: 'quiet', title: 'A sketch', takeaway: () => null, caption: () => null, table: null });
    const first = await ok('get_chart_data', { chart: 'net-worth' });
    expect(first).toMatchObject({ chart: 'net-worth', table: 'Net worth by age', columns: [{ key: 'age', label: 'Age' }, { key: 'p50' }, { key: 'label' }], total_rows: 500, offset: 0, returned: 60, truncated: true, next_offset: 60 });
    expect((first.rows as unknown[][])[1]).toEqual([41, 1001230, 'retired']);
    expect((first.rows as unknown[][])[0]).toEqual([40, 1000000, null]);
    const all = await ok('get_chart_data', { chart: 'Net worth among the living', offset: 400, max_rows: 200 });
    expect(all).toMatchObject({ offset: 400, returned: 100 });
    expect(all).not.toHaveProperty('truncated');
    expect(await failure('get_chart_data', { chart: 'quiet' })).toMatchObject({ code: 'no_table', title: 'A sketch' });
    expect((await failure('get_chart_data', { chart: 'nope' })).message).toContain('net-worth, quiet');

    // Very wide rows: fewer are returned rather than a result over the cap.
    const wide: ChartEntry = { ...netWorth(1), id: 'wide', table: () => ({ caption: 'Wide', columns: Array.from({ length: 30 }, (_, i) => ({ key: `c${i}`, label: `Column ${i}` })), rows: Array.from({ length: 200 }, () => Object.fromEntries(Array.from({ length: 30 }, (_, i) => [`c${i}`, 123456.789 + i]))) }) };
    world.charts.push(wide);
    const capped = await ok('get_chart_data', { chart: 'wide', max_rows: 200 });
    expect(capped).toMatchObject({ truncated: true, columns_omitted: 6 });
    expect(capped.returned as number).toBeLessThan(200);
    expect(JSON.stringify(capped).length).toBeLessThan(DEFAULT_LIMITS.maxResultChars);
  });

  it('the chart registry lists what is mounted, numbers repeated names, and forgets what unmounts', () => {
    const a = registerChart({ stem: 'net-worth', title: 'A', takeaway: () => null, caption: () => null, table: null });
    const b = registerChart({ stem: 'net-worth', title: 'B', takeaway: () => null, caption: () => null, table: null });
    expect(mountedCharts().map((c) => [c.id, c.title])).toEqual([['net-worth', 'A'], ['net-worth-2', 'B']]);
    a();
    expect(mountedCharts().map((c) => c.id)).toEqual(['net-worth']);
    b();
    expect(mountedCharts()).toEqual([]);
  });

  it('the chart registry counts only charts drawn in the results view', () => {
    let place: 'shown' | 'hidden' | 'elsewhere' = 'shown';
    const entry = { title: 'T', takeaway: () => null, caption: () => null, table: null };
    const offs = [registerChart({ ...entry, stem: 'previous-view', place: () => place }), registerChart({ ...entry, stem: 'in-a-message', place: () => 'elsewhere' })];
    expect(mountedCharts().map((c) => c.id)).toEqual(['previous-view']);
    expect(chartsPending()).toBe(false);
    // React keeps the previous view mounted but hidden while the next one loads.
    place = 'hidden';
    expect(mountedCharts()).toEqual([]);
    expect(chartsPending()).toBe(true);
    offs.forEach((off) => off());
    expect(chartsPending()).toBe(false);
  });
});

// ---- the engine tools -----------------------------------------------------------------

describe('the engine tools', () => {
  it('every tool answers on the demo plan with a result under the size cap', async () => {
    openDemo();
    await finishRun();
    world.settings.autoApply = true;
    world.charts.push({ id: 'c', title: 'Chart', takeaway: () => 'It goes up.', caption: () => 'Caption.', table: () => ({ caption: 'Rows', columns: [{ key: 'x', label: 'X' }], rows: [{ x: 1 }] }) });
    const run = await ok('run_simulation', { paths: 300 });
    const path = ((run.trace_paths as Json).median ?? Object.values(run.trace_paths as Json)[0]) as number;
    const life = await ok('inspect_life', { run_id: run.run_id, path });
    const year = (life.tax_years_available as number[] | null)?.[0] ?? (life.table as { year: number[] }).year[0]!;
    const inputs: Record<string, unknown> = {
      get_app_state: {},
      get_guide: {},
      get_plan: {},
      get_schema: { section: 'Income', with_nested: true },
      edit_plan: RAISE,
      run_simulation: { paths: 300 },
      try_scenario: { ...LATER, keep: true },
      rank_levers: { metric: 'success_rate', paths: 200 },
      solve: { variable: 'annual_spending', target_metric: 'success_rate', target_value: 0.8, paths: 200 },
      sweep: { x: { variable: 'retirement_age', values: [58, 62] }, metric: 'success_rate', paths: 200 },
      inspect_life: { run_id: run.run_id, pick: 'median' },
      explain_tax: { run_id: run.run_id, path, year },
      lookup_cost_of_living: { from: 'Denver', to: 'Lisbon' },
      show_view: { view: 'outcomes' },
      get_results: {},
      list_charts: {},
      get_chart_data: { chart: 'c' },
      list_scenarios: {},
      set_scenario_enabled: { id: 'leave_more', enabled: false },
      remove_scenario: { id: 'leave_more' },
      undo_agent_edit: {},
      open_demo: { slug: 'dual-income-family' },
      create_plan: { profile: demo(), name: 'A copy' },
    };
    const sizes: Record<string, number> = {};
    for (const [name, input] of Object.entries(inputs)) {
      const result = await call(name, input);
      expect(result.ok, `${name}: ${JSON.stringify(result).slice(0, 300)}`).toBe(true);
      sizes[name] = JSON.stringify(result).length;
      expect(sizes[name], name).toBeLessThan(DEFAULT_LIMITS.maxResultChars);
    }
    expect(await ok('open_plan', { id: 'demo_plan' })).toMatchObject({ plan: { id: 'demo_plan' } });
    // Every tool was exercised.
    expect([...Object.keys(inputs), 'open_plan'].sort()).toEqual(AGENT_TOOL_NAMES);
    expect(useAgent.getState().approvals).toHaveLength(0);
  }, 120_000);

  it('answers a city that is not in the table with the candidates, as an error object', async () => {
    const error = await failure('lookup_cost_of_living', { from: 'Denver', to: 'Atlantis' });
    expect(error).toMatchObject({ code: 'not_found', found: false, to_not_found: 'Atlantis' });
  });

  it('keeps its own runs, apart from the assistant\'s, and drops them when another plan is opened', async () => {
    openDemo();
    const run = await ok('run_simulation', { paths: 200 });
    expect(world.ctx.runs.ids()).toEqual([run.run_id]);
    await ok('inspect_life', { pick: 'median' });
    openDemo('second_plan');
    const error = await failure('inspect_life', { run_id: run.run_id });
    expect(error.code).toBe('tool_error');
    expect(world.ctx.runs.ids()).toEqual([]);
  });

  it('refuses to simulate a plan with errors, in the outside agent\'s vocabulary', async () => {
    openDemo();
    state().edit([{ op: 'set', path: '/household/people', value: [] }]);
    const error = await failure('run_simulation');
    expect(error.message).toContain('use edit_plan');
    expect(error.message).not.toContain('propose_changes');
  });
});

// ---- get_results on the app's real run store -------------------------------------------

// Last in the file: `startRunController` cannot be stopped, so from here on every plan that is
// opened is simulated by the real run store, on whatever `pagePool` holds.
describe('get_results on the real run store', () => {
  let comparisons: 'answer' | 'fail' = 'answer';

  beforeAll(() => {
    startRunController();
  });
  beforeEach(() => {
    comparisons = 'answer';
    const engine = nodeEngine();
    pagePool.current = {
      // A pool of workers, as far as the run store can tell: results after a moment, final summaries through onProgress.
      async runMany(...args: never[]) {
        const [jobs, options] = args as unknown as [{ id: string; profile: Profile; paths: number }[], { seed: number; signal?: AbortSignal; onProgress?: (id: string, p: unknown) => void }];
        await tick(5);
        const results = await engine.pool.runMany(jobs.map((j) => ({ id: j.id, profile: j.profile, paths: j.paths })), { seed: options.seed });
        if (options.signal?.aborted) throw new DOMException('Aborted', 'AbortError');
        for (const [id, r] of results) options.onProgress?.(id, { summary: r.summary, done: r.paths, summarizeMs: 0 });
        return results;
      },
      // The comparison is a second round trip, after the run is marked done.
      async comparison(...args: never[]) {
        const [a, b, base] = args as unknown as [{ acc: Uint8Array; seed: number }, { acc: Uint8Array }, Profile];
        await tick(30);
        if (comparisons === 'fail') throw new Error('the worker went away');
        return compare(a.acc, b.acc, engineInput(base, a.seed).json);
      },
    };
    world.ctx.app.run = () => useRunStore.getState();
    world.ctx.app.onRun = (listener) => useRunStore.subscribe(listener);
    world.settings.autoApply = true;
  });
  afterEach(() => {
    state().close();
  });

  function openWithScenario() {
    const profile = demo();
    const item = profile.spending.items[0]!;
    const path = `/spending/items/@${item.id}/amount`;
    const value = item.amount * 3;
    state().load({ meta: { id: 'p', createdAt: 1, origin: 'test' }, profile, scenarios: [{ id: 'spend_more', name: 'Spend more', patch: [{ op: 'set', path, value }], enabled: true }], run: { paths: 400, seed: 1 } });
    return { path, value };
  }
  const versus = (result: Json) => ((result.scenarios as Json[])[0]!.versus_plan as { success: { a: number; b: number; delta: number } } | undefined)?.success;
  const p = (result: Json) => (result.success as { p: number }).p;

  it('after edit_plan, waits for the new pairing: never the comparison against the plan as it was', async () => {
    const { path, value } = openWithScenario();
    const first = await ok('get_results');
    expect(first.complete).toBe(true);
    expect(versus(first)?.a).toBe(p(first));
    expect(versus(first)?.delta).toBeLessThan(0);

    // The plan becomes the scenario, so the true difference is now nothing at all.
    await ok('edit_plan', { summary: 'Spend more', ops: [{ op: 'set', path, value }] });
    // Asked not to wait, it has nothing for the changed plan yet, or figures marked incomplete: never the old comparison.
    const early = await call('get_results', { wait: false });
    if (early.ok) {
      expect(early.complete).toBe(false);
      expect((early.scenarios as Json[])[0]).not.toHaveProperty('versus_plan');
    } else expect(early.error.code).toBe('still_running');

    const second = await ok('get_results');
    expect(second.complete).toBe(true);
    expect(p(second)).not.toBe(p(first));
    expect(versus(second)).toMatchObject({ a: p(second), b: p(second), delta: 0 });
    // The Scenarios view was never blanked: the run store kept the last comparison on show throughout.
    expect(useRunStore.getState().scenarios[0]).toMatchObject({ pairing: 'current' });
  });

  it('keeps the previous comparison for the Scenarios view while the next run is under way, marked as not current', async () => {
    const { path, value } = openWithScenario();
    await ok('get_results');
    const before = useRunStore.getState().scenarios[0]!.paired;
    expect(before).not.toBeNull();
    state().edit([{ op: 'set', path, value }]);
    const seen: string[] = [];
    const stop = useRunStore.subscribe((run) => {
      const s = run.scenarios[0];
      if (s && run.status !== 'queued') seen.push(`${run.status}:${s.pairing}:${s.paired === before ? 'previous' : s.paired ? 'new' : 'none'}`);
    });
    await ok('get_results');
    stop();
    expect(seen).toContain('running:pending:previous');
    expect(seen).toContain('done:pending:previous');
    expect(seen.at(-1)).toBe('done:current:new');
  });

  it('a comparison that fails is reported as unavailable at once, not waited for', async () => {
    comparisons = 'fail';
    openWithScenario();
    const started = Date.now();
    const result = await ok('get_results', { timeout_seconds: 20 });
    expect(Date.now() - started).toBeLessThan(15_000);
    expect(result.complete).toBe(true);
    const scenario = (result.scenarios as Json[])[0]!;
    expect(scenario).toMatchObject({ id: 'spend_more', success: { p: expect.any(Number) }, versus_plan_unavailable: expect.stringContaining('try_scenario') });
    expect(scenario).not.toHaveProperty('versus_plan');
    expect(useRunStore.getState().scenarios[0]).toMatchObject({ pairing: 'failed', paired: null });
  });
});
