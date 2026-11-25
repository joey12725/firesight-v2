// FIRESight hosted assistant proxy (Vercel Function, Node.js runtime, Web handler).
//
//   GET  /api/chat   { available, models, access_code_required }
//   POST /api/chat   the Messages API request the app built, streamed back unchanged
//
// The browser sends exactly what it would send to api.anthropic.com; this function adds
// the operator's key (ANTHROPIC_API_KEY) and pipes the server-sent events through. It
// stores nothing and logs nothing: there is no console call in this file, by design.
//
// It is deliberately awkward to use as a free general-purpose model endpoint. A request
// is forwarded only if it comes from this site's own pages, carries the access code
// when one is configured, is within the rate limit, is small enough, names an allowed
// model, and has the shape of a FIRESight assistant request: the app's system prompt
// marker, exactly the app's tool names, no server tools, no extra parameters. Those
// checks make abuse inconvenient, not impossible: everything a browser can send, a
// script can imitate. See docs/assistant.md for what each check does and does not stop.
//
// This file has no imports so that it deploys without a package.json at the repository
// root. The constants it shares with the app are duplicated here and compared with the
// app's by a test (web/src/assistant/transport/proxy.test.ts).

declare const process: { env: Record<string, string | undefined> };

export type Env = Record<string, string | undefined>;

/** Must equal PROMPT_MARKER in web/src/assistant/prompt.ts. */
export const PROMPT_MARKER = '[FIRESight planning assistant, prompt v3]';

/** Must equal the app's tool names (web/src/assistant/tools/index.ts), sorted. */
export const TOOL_NAMES = [
  'explain_tax',
  'get_plan',
  'get_schema',
  'inspect_life',
  'lookup_cost_of_living',
  'propose_changes',
  'rank_levers',
  'run_simulation',
  'show_chart',
  'solve',
  'sweep',
  'try_scenario',
];

/** Must equal ASSISTANT_MODELS in web/src/state/settings-store.ts. Most capable first. */
export const MODELS = ['claude-fable-5-2', 'claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5'];

/** Beta features the app uses (web/src/assistant/models.ts). Others are not forwarded. */
export const BETAS = ['context-management-2025-06-27', 'server-side-fallback-2026-07-01'];

export const MAX_OUTPUT_TOKENS = 32_000;
/** Vercel rejects request bodies over 4.5 MB before this function runs. */
export const MAX_BODY_BYTES = 4_000_000;
export const MAX_MESSAGES = 600;
/** The version the app's SDK speaks. The body is forwarded as built, so the version must match it. */
const API_VERSIONS = ['2026-10-21'];
const UPSTREAM = 'https://api.anthropic.com/v1/messages';
const ACCESS_HEADER = 'x-firesight-access';

const ALLOWED_KEYS = new Set(['model', 'max_tokens', 'stream', 'system', 'tools', 'messages', 'thinking', 'output_config', 'cache_control', 'context_management', 'fallbacks', 'tool_choice']);

function reply(status: number, type: string, message: string, headers: Record<string, string> = {}): Response {
  // The Anthropic error envelope, so the SDK in the browser raises its usual typed errors.
  return new Response(JSON.stringify({ type: 'error', error: { type, message } }), {
    status,
    headers: { 'content-type': 'application/json', 'cache-control': 'no-store', ...headers },
  });
}

export function allowedModels(env: Env): string[] {
  const wanted = (env.FIRESIGHT_MODELS ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);
  return wanted.length > 0 ? MODELS.filter((m) => wanted.includes(m)) : MODELS;
}

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);

/** Why a body is not a FIRESight assistant request, or null when it is. */
export function validateBody(body: unknown, env: Env): string | null {
  if (!isRecord(body)) return 'The body must be a JSON object.';
  for (const key of Object.keys(body)) if (!ALLOWED_KEYS.has(key)) return `The parameter "${key}" is not accepted here.`;
  if (typeof body.model !== 'string' || !allowedModels(env).includes(body.model)) return 'That model is not offered by this site.';
  if (typeof body.max_tokens !== 'number' || !Number.isInteger(body.max_tokens) || body.max_tokens < 1 || body.max_tokens > MAX_OUTPUT_TOKENS) {
    return `max_tokens must be between 1 and ${MAX_OUTPUT_TOKENS}.`;
  }
  if (body.stream !== true) return 'Only streamed requests are accepted.';

  const system = body.system;
  const systemText = typeof system === 'string' ? system : Array.isArray(system) && isRecord(system[0]) && typeof system[0].text === 'string' ? system[0].text : '';
  if (!systemText.startsWith(PROMPT_MARKER)) return 'The request does not carry the FIRESight system prompt.';
  if (Array.isArray(system) && !system.every((b) => isRecord(b) && b.type === 'text')) return 'The system prompt must be text.';

  if (!Array.isArray(body.tools)) return 'The request does not carry the FIRESight tools.';
  const names: string[] = [];
  for (const tool of body.tools) {
    if (!isRecord(tool) || typeof tool.name !== 'string' || !isRecord(tool.input_schema)) return 'A tool definition is malformed.';
    // Server tools (web search, code execution, MCP) are identified by `type`.
    if (tool.type !== undefined && tool.type !== 'custom') return 'Only the FIRESight tools are accepted.';
    names.push(tool.name);
  }
  names.sort();
  if (names.length !== TOOL_NAMES.length || names.some((name, i) => name !== TOOL_NAMES[i])) return 'The request does not carry the FIRESight tools.';

  if (!Array.isArray(body.messages) || body.messages.length === 0 || body.messages.length > MAX_MESSAGES) return 'The conversation is empty or too long.';
  if (body.thinking !== undefined && !(isRecord(body.thinking) && body.thinking.type === 'adaptive')) return 'Only adaptive thinking is accepted.';
  if (body.output_config !== undefined) {
    const oc = body.output_config;
    if (!isRecord(oc) || Object.keys(oc).some((k) => k !== 'effort') || !['low', 'medium', 'high'].includes(oc.effort as string)) return 'output_config may only set effort to low, medium or high.';
  }
  if (body.tool_choice !== undefined && !(isRecord(body.tool_choice) && (body.tool_choice.type === 'auto' || body.tool_choice.type === 'none'))) return 'tool_choice must be auto or none.';
  if (body.fallbacks !== undefined && body.fallbacks !== 'default') return 'fallbacks may only be "default".';
  return null;
}

// ---- same-origin --------------------------------------------------------------------

/**
 * True when the request comes from a page of this site (or an origin the operator
 * listed). Browsers set Origin on every cross-site POST and scripts on other sites
 * cannot forge it, so this stops other websites from spending the operator's key
 * through their visitors. It does not stop a non-browser client, which can send any
 * header it likes.
 */
export function sameOrigin(request: Request, env: Env): boolean {
  const origin = request.headers.get('origin');
  if (!origin) return false;
  const extra = (env.FIRESIGHT_ALLOWED_ORIGINS ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean);
  if (extra.includes(origin)) return true;
  const site = request.headers.get('sec-fetch-site');
  if (site && site !== 'same-origin') return false;
  let originHost: string;
  try {
    originHost = new URL(origin).host;
  } catch {
    return false;
  }
  const host = request.headers.get('x-forwarded-host') ?? request.headers.get('host') ?? new URL(request.url).host;
  return originHost === host;
}

function sameText(a: string, b: string): boolean {
  // Compare without stopping at the first difference.
  let diff = a.length ^ b.length;
  for (let i = 0; i < Math.max(a.length, b.length); i++) diff |= (a.charCodeAt(i) || 0) ^ (b.charCodeAt(i) || 0);
  return diff === 0;
}

// ---- rate limit ---------------------------------------------------------------------

/**
 * A fixed-window counter per client address, held in this instance's memory.
 *
 * Its limits, honestly: every running instance of the function has its own counters
 * and they vanish when the instance is recycled, so under load a client may get a
 * multiple of the nominal limit, and after a cold start it starts from zero. It is a
 * speed bump against one address hammering the endpoint, not a quota. For a real
 * quota put a shared store (Vercel KV, Upstash) or Vercel's firewall rate limiting in
 * front, and set a spend limit on the API key's workspace, which is the backstop that
 * actually bounds cost.
 */
export class RateLimiter {
  private readonly windows = new Map<string, { minute: number; minuteCount: number; day: number; dayCount: number }>();
  constructor(
    private readonly perMinute: number,
    private readonly perDay: number,
  ) {}

  /** Seconds to wait, or 0 when the request may proceed (and is counted). */
  check(key: string, now: number): number {
    const minute = Math.floor(now / 60_000);
    const day = Math.floor(now / 86_400_000);
    if (this.windows.size > 5_000) {
      for (const [k, w] of this.windows) if (w.day !== day) this.windows.delete(k);
      if (this.windows.size > 5_000) this.windows.clear();
    }
    const w = this.windows.get(key) ?? { minute, minuteCount: 0, day, dayCount: 0 };
    if (w.minute !== minute) {
      w.minute = minute;
      w.minuteCount = 0;
    }
    if (w.day !== day) {
      w.day = day;
      w.dayCount = 0;
    }
    this.windows.set(key, w);
    if (w.dayCount >= this.perDay) return Math.ceil(((day + 1) * 86_400_000 - now) / 1000);
    if (w.minuteCount >= this.perMinute) return Math.ceil(((minute + 1) * 60_000 - now) / 1000);
    w.minuteCount += 1;
    w.dayCount += 1;
    return 0;
  }
}

const positive = (raw: string | undefined, fallback: number) => {
  const n = Number(raw);
  return Number.isFinite(n) && n > 0 ? Math.floor(n) : fallback;
};

let limiter: RateLimiter | null = null;

function clientAddress(request: Request): string {
  // On Vercel the first entry is the client as seen by the platform, not client-supplied.
  return request.headers.get('x-forwarded-for')?.split(',')[0]?.trim() || request.headers.get('x-real-ip') || 'unknown';
}

// ---- handler ------------------------------------------------------------------------

export interface Deps {
  fetch: typeof fetch;
  now: () => number;
  limiter?: RateLimiter;
}

export async function handle(request: Request, env: Env, deps: Deps = { fetch, now: Date.now }): Promise<Response> {
  const key = env.ANTHROPIC_API_KEY;
  const accessCode = env.FIRESIGHT_ACCESS_CODE;

  if (request.method === 'GET' || request.method === 'HEAD') {
    return new Response(JSON.stringify({ available: Boolean(key), models: key ? allowedModels(env) : [], access_code_required: Boolean(key && accessCode) }), {
      headers: { 'content-type': 'application/json', 'cache-control': 'no-store' },
    });
  }
  if (request.method !== 'POST') return reply(405, 'firesight_rejected', 'Use POST.', { allow: 'GET, POST' });
  if (!key) return reply(503, 'firesight_unavailable', 'The hosted assistant is not configured on this site.');
  if (!sameOrigin(request, env)) return reply(403, 'firesight_origin', 'Requests are accepted only from this site.');
  if (accessCode && !sameText(request.headers.get(ACCESS_HEADER) ?? '', accessCode)) {
    return reply(401, 'firesight_access_code', 'An access code is required.');
  }

  const rate = deps.limiter ?? (limiter ??= new RateLimiter(positive(env.FIRESIGHT_RATE_PER_MINUTE, 30), positive(env.FIRESIGHT_RATE_PER_DAY, 600)));
  const wait = rate.check(clientAddress(request), deps.now());
  if (wait > 0) return reply(429, 'firesight_rate_limit', 'Too many requests from this address. Try again later.', { 'retry-after': String(wait) });

  const declared = Number(request.headers.get('content-length') ?? '0');
  if (declared > MAX_BODY_BYTES) return reply(413, 'firesight_too_large', 'The request is too large.');
  let bytes: ArrayBuffer;
  try {
    bytes = await request.arrayBuffer();
  } catch {
    return reply(400, 'firesight_rejected', 'The request body could not be read.');
  }
  if (bytes.byteLength > MAX_BODY_BYTES) return reply(413, 'firesight_too_large', 'The request is too large.');
  const text = new TextDecoder().decode(bytes);
  let body: unknown;
  try {
    body = JSON.parse(text);
  } catch {
    return reply(400, 'firesight_rejected', 'The body is not valid JSON.');
  }
  const problem = validateBody(body, env);
  if (problem) return reply(400, 'firesight_rejected', problem);

  const version = request.headers.get('anthropic-version') ?? '';
  if (!API_VERSIONS.includes(version)) return reply(400, 'firesight_rejected', 'Unsupported API version.');
  const betas = (request.headers.get('anthropic-beta') ?? '')
    .split(',')
    .map((s) => s.trim())
    .filter((b) => BETAS.includes(b));
  const beta = new URL(request.url).searchParams.get('beta') === 'true';

  let upstream: Response;
  try {
    upstream = await deps.fetch(`${UPSTREAM}${beta ? '?beta=true' : ''}`, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        // The credential is sent the way the current SDK sends an API key.
        authorization: `Bearer ${key}`,
        'anthropic-version': version,
        ...(betas.length > 0 ? { 'anthropic-beta': betas.join(',') } : {}),
      },
      body: text,
      // The visitor closing the tab or pressing Stop cancels the upstream request too.
      signal: request.signal,
    });
  } catch {
    return reply(502, 'api_error', 'The assistant service could not be reached.');
  }

  // Pipe the event stream (or the API's own error body) through untouched.
  const headers: Record<string, string> = {
    'content-type': upstream.headers.get('content-type') ?? 'application/json',
    'cache-control': 'no-store, no-transform',
    'x-accel-buffering': 'no',
  };
  for (const name of ['retry-after', 'request-id']) {
    const value = upstream.headers.get(name);
    if (value) headers[name] = value;
  }
  return new Response(upstream.body, { status: upstream.status, headers });
}

export default {
  fetch(request: Request): Promise<Response> {
    return handle(request, process.env);
  },
};
