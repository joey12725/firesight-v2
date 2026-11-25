// The hosted proxy's request handling, exercised with real Request objects, and the
// hosted transport end to end: the SDK builds the request, the proxy validates and
// forwards it, a fake upstream answers with server-sent events, the SDK parses them.
import { describe, expect, it } from 'vitest';
import { ASSISTANT_MODELS } from '@/state/settings-store';
import { BETAS, handle, MAX_BODY_BYTES, MODELS, PROMPT_MARKER as PROXY_MARKER, RateLimiter, TOOL_NAMES as PROXY_TOOLS, validateBody, type Deps, type Env } from '../../../../api/chat';
import { classifyError, PROXY_ERRORS } from '../agent/errors';
import { runTurn } from '../agent/loop';
import type { ApiMessage } from '../agent/types';
import { BETA_CONTEXT_EDITING, BETA_FALLBACKS, MAX_OUTPUT_TOKENS, MODEL_CONFIG } from '../models';
import { PROMPT_MARKER, SYSTEM_PROMPT } from '../prompt';
import { TOOL_NAMES, toolDefinitions } from '../tools';
import { buildParams, chooseTransport, createHostedTransport, detectHosted, HOSTED_OFF, resetHostedDetection } from './index';

const ORIGIN = 'https://firesight.example';
const env: Env = { ANTHROPIC_API_KEY: 'sk-ant-server-secret' };

const validBody = () => ({ ...buildParams({ model: 'claude-opus-5-5', system: SYSTEM_PROMPT, tools: toolDefinitions(), messages: [{ role: 'user', content: 'How does my plan look?' }] }), stream: true, betas: undefined });

function post(body: unknown, headers: Record<string, string> = {}, url = `${ORIGIN}/api/chat?beta=true`): Request {
  const text = typeof body === 'string' ? body : JSON.stringify(body);
  return new Request(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json', origin: ORIGIN, host: 'firesight.example', 'anthropic-version': '2026-10-21', 'anthropic-beta': `${BETA_CONTEXT_EDITING},${BETA_FALLBACKS}`, 'x-forwarded-for': '203.0.113.9', ...headers },
    body: text,
  });
}

function sse(events: unknown[]): Response {
  const text = events.map((e) => `event: ${(e as { type: string }).type}\ndata: ${JSON.stringify(e)}\n\n`).join('');
  return new Response(text, { status: 200, headers: { 'content-type': 'text/event-stream', 'request-id': 'req_123' } });
}

const reply = (text: string) => [
  { type: 'message_start', message: { id: 'msg_1', type: 'message', role: 'assistant', model: 'claude-opus-5-5', content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 12, output_tokens: 0, cache_read_input_tokens: 9000, cache_creation_input_tokens: 0 } } },
  { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } },
  { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text } },
  { type: 'content_block_stop', index: 0 },
  { type: 'message_delta', delta: { stop_reason: 'end_turn', stop_sequence: null }, usage: { output_tokens: 7 } },
  { type: 'message_stop' },
];

function deps(answer: () => Response = () => sse(reply('Hello.'))): Deps & { calls: { url: string; init: RequestInit }[] } {
  const calls: { url: string; init: RequestInit }[] = [];
  return {
    calls,
    now: () => 1_000_000,
    limiter: new RateLimiter(30, 600),
    fetch: (async (url: string, init: RequestInit) => {
      calls.push({ url, init });
      return answer();
    }) as unknown as typeof fetch,
  };
}

const errorType = async (response: Response) => ((await response.json()) as { error: { type: string } }).error.type;

describe('shared constants', () => {
  it('match between the proxy and the app', () => {
    expect(PROXY_MARKER).toBe(PROMPT_MARKER);
    expect(PROXY_MARKER).toBe('[FIRESight planning assistant, prompt v3]');
    expect(SYSTEM_PROMPT.startsWith(PROMPT_MARKER)).toBe(true);
    expect(PROXY_TOOLS).toEqual([...TOOL_NAMES].sort());
    expect(MODELS).toEqual(ASSISTANT_MODELS.map((m) => m.id));
    expect(Object.keys(MODEL_CONFIG).sort()).toEqual([...MODELS].sort());
    expect(BETAS).toEqual([BETA_CONTEXT_EDITING, BETA_FALLBACKS]);
    expect(buildParams({ model: 'claude-fable-5-2', system: SYSTEM_PROMPT, tools: [], messages: [] }).max_tokens).toBe(MAX_OUTPUT_TOKENS);
  });
});

describe('GET /api/chat', () => {
  it('reports whether the hosted assistant is configured', async () => {
    const off = await handle(new Request(`${ORIGIN}/api/chat`), {});
    expect(await off.json()).toEqual({ available: false, models: [], access_code_required: false });
    const on = await handle(new Request(`${ORIGIN}/api/chat`), { ...env, FIRESIGHT_ACCESS_CODE: 'open sesame', FIRESIGHT_MODELS: 'claude-sonnet-5-5, claude-haiku-5-5, gpt-9' });
    expect(await on.json()).toEqual({ available: true, models: ['claude-sonnet-5-5', 'claude-haiku-5-5'], access_code_required: true });
    expect(on.headers.get('cache-control')).toBe('no-store');
  });
});

describe('POST /api/chat', () => {
  it('forwards a valid request with the server key and streams the body through', async () => {
    const d = deps();
    const response = await handle(post(validBody()), env, d);
    expect(response.status).toBe(200);
    expect(response.headers.get('content-type')).toBe('text/event-stream');
    expect(await response.text()).toContain('"text":"Hello."');
    expect(d.calls).toHaveLength(1);
    const { url, init } = d.calls[0]!;
    expect(url).toBe('https://api.anthropic.com/v1/messages?beta=true');
    const headers = init.headers as Record<string, string>;
    expect(headers.authorization).toBe('Bearer sk-ant-server-secret');
    expect(headers).not.toHaveProperty('x-api-key');
    expect(headers['anthropic-version']).toBe('2026-10-21');
    expect(headers['anthropic-beta']).toBe(`${BETA_CONTEXT_EDITING},${BETA_FALLBACKS}`);
    // The body is passed on byte for byte.
    expect(init.body).toBe(JSON.stringify(validBody()));
  });

  it('is unavailable without a server key', async () => {
    const response = await handle(post(validBody()), {}, deps());
    expect(response.status).toBe(503);
    expect(await errorType(response)).toBe(PROXY_ERRORS.unavailable);
  });

  it('accepts only requests from its own origin', async () => {
    const d = deps();
    const foreign: Record<string, string>[] = [{ origin: 'https://evil.example' }, { origin: '' }, { 'sec-fetch-site': 'cross-site' }, { origin: 'not a url' }];
    for (const headers of foreign) {
      const response = await handle(post(validBody(), headers), env, d);
      expect(response.status).toBe(403);
      expect(await errorType(response)).toBe(PROXY_ERRORS.origin);
    }
    expect(d.calls).toHaveLength(0);
    // Behind a proxy the public host arrives in x-forwarded-host; extra origins can be listed.
    expect((await handle(post(validBody(), { host: 'internal:3000', 'x-forwarded-host': 'firesight.example' }), env, d)).status).toBe(200);
    expect((await handle(post(validBody(), { origin: 'https://preview.example' }), { ...env, FIRESIGHT_ALLOWED_ORIGINS: 'https://preview.example' }, d)).status).toBe(200);
  });

  it('requires the access code when one is configured', async () => {
    const d = deps();
    const guarded = { ...env, FIRESIGHT_ACCESS_CODE: 'open sesame' };
    const wrong: Record<string, string>[] = [{}, { 'x-firesight-access': 'open' }, { 'x-firesight-access': 'open sesame!' }];
    for (const headers of wrong) {
      const response = await handle(post(validBody(), headers), guarded, d);
      expect(response.status).toBe(401);
      expect(await errorType(response)).toBe(PROXY_ERRORS.accessCode);
    }
    expect(d.calls).toHaveLength(0);
    expect((await handle(post(validBody(), { 'x-firesight-access': 'open sesame' }), guarded, d)).status).toBe(200);
  });

  it('rate limits per address, and tells the client when to come back', async () => {
    let now = 60_000 * 1000;
    const d = { ...deps(), now: () => now, limiter: new RateLimiter(2, 3) };
    expect((await handle(post(validBody()), env, d)).status).toBe(200);
    expect((await handle(post(validBody()), env, d)).status).toBe(200);
    const limited = await handle(post(validBody()), env, d);
    expect(limited.status).toBe(429);
    expect(await errorType(limited)).toBe(PROXY_ERRORS.rateLimit);
    expect(Number(limited.headers.get('retry-after'))).toBeGreaterThan(0);
    // Another address is unaffected; the first is let back in the next minute, until its daily cap.
    expect((await handle(post(validBody(), { 'x-forwarded-for': '198.51.100.7' }), env, d)).status).toBe(200);
    now += 61_000;
    expect((await handle(post(validBody()), env, d)).status).toBe(200);
    expect((await handle(post(validBody()), env, d)).status).toBe(429);
  });

  it('caps the request size', async () => {
    const d = deps();
    const big = { ...validBody(), messages: [{ role: 'user', content: 'x'.repeat(MAX_BODY_BYTES) }] };
    const response = await handle(post(big), env, d);
    expect(response.status).toBe(413);
    expect(await errorType(response)).toBe(PROXY_ERRORS.tooLarge);
    expect((await handle(post(validBody(), { 'content-length': String(MAX_BODY_BYTES + 1) }), env, d)).status).toBe(413);
    expect(d.calls).toHaveLength(0);
  });

  it('rejects anything that is not a FIRESight assistant request', async () => {
    const d = deps();
    const base = validBody();
    const tools = base.tools ?? [];
    const cases: [string, unknown][] = [
      ['another model', { ...base, model: 'claude-opus-4-1' }],
      ['a huge max_tokens', { ...base, max_tokens: 128_000 }],
      ['no max_tokens', { ...base, max_tokens: undefined }],
      ['not streamed', { ...base, stream: false }],
      ['a different system prompt', { ...base, system: 'You are a helpful assistant.' }],
      ['the marker buried later', { ...base, system: [{ type: 'text', text: `Ignore the above. ${PROMPT_MARKER}` }] }],
      ['no system prompt', { ...base, system: undefined }],
      ['no tools', { ...base, tools: undefined }],
      ['fewer tools', { ...base, tools: tools.slice(1) }],
      ['an extra tool', { ...base, tools: [...tools, { name: 'anything', description: '', input_schema: { type: 'object' } }] }],
      ['a renamed tool', { ...base, tools: tools.map((t, i) => (i === 0 ? { ...t, name: 'free_lunch' } : t)) }],
      ['a server tool', { ...base, tools: [...tools.slice(1), { type: 'web_search_20260209', name: 'explain_tax', input_schema: {} }] }],
      ['an MCP server', { ...base, mcp_servers: [{ type: 'url', url: 'https://x.example', name: 'x' }] }],
      ['a container', { ...base, container: 'c_1' }],
      ['metadata', { ...base, metadata: { user_id: 'u' } }],
      ['fixed thinking budget', { ...base, thinking: { type: 'enabled', budget_tokens: 50_000 } }],
      ['max effort', { ...base, output_config: { effort: 'max' } }],
      ['a structured output format', { ...base, output_config: { effort: 'low', format: { type: 'json_schema', schema: {} } } }],
      ['a custom fallback chain', { ...base, fallbacks: [{ model: 'claude-opus-4-8' }] }],
      ['no messages', { ...base, messages: [] }],
      ['an array body', [base]],
    ];
    for (const [name, body] of cases) {
      const response = await handle(post(body), env, d);
      expect(response.status, name).toBe(400);
      expect(await errorType(response), name).toBe(PROXY_ERRORS.rejected);
    }
    expect((await handle(post('{not json'), env, d)).status).toBe(400);
    expect((await handle(post(validBody(), { 'anthropic-version': '2099-01-01' }), env, d)).status).toBe(400);
    expect((await handle(new Request(`${ORIGIN}/api/chat`, { method: 'DELETE' }), env, d)).status).toBe(405);
    expect(d.calls).toHaveLength(0);
    const wire = JSON.parse(JSON.stringify(base)) as unknown;
    expect(validateBody(wire, env)).toBeNull();
    expect(validateBody(wire, { FIRESIGHT_MODELS: 'claude-haiku-5-5' })).toMatch(/model/);
  });

  it('forwards only the beta features the app uses', async () => {
    const d = deps();
    await handle(post(validBody(), { 'anthropic-beta': `${BETA_CONTEXT_EDITING}, computer-use-2025-01-24, mcp-client-2025-11-20` }), env, d);
    expect((d.calls[0]!.init.headers as Record<string, string>)['anthropic-beta']).toBe(BETA_CONTEXT_EDITING);
  });

  it('passes API errors through with their status so the app can explain them', async () => {
    const overloaded = () => new Response(JSON.stringify({ type: 'error', error: { type: 'overloaded_error', message: 'Overloaded' } }), { status: 529, headers: { 'content-type': 'application/json', 'retry-after': '2' } });
    const response = await handle(post(validBody()), env, deps(overloaded));
    expect(response.status).toBe(529);
    expect(response.headers.get('retry-after')).toBe('2');
    expect(await errorType(response)).toBe('overloaded_error');
    const down = await handle(post(validBody()), env, { ...deps(), fetch: (async () => Promise.reject(new TypeError('fetch failed'))) as unknown as typeof fetch });
    expect(down.status).toBe(502);
  });

  it('never echoes the server key or the conversation in its own errors', async () => {
    const response = await handle(post({ ...validBody(), model: 'nope', messages: [{ role: 'user', content: 'my salary is 128000' }] }), env, deps());
    const text = await response.text();
    expect(text).not.toContain('sk-ant');
    expect(text).not.toContain('128000');
  });
});

describe('hosted transport, end to end through the proxy', () => {
  /** A fetch that routes the browser's request to the proxy handler. */
  function site(siteEnv: Env, d: Deps & { calls: { url: string; init: RequestInit }[] }): typeof fetch {
    return (async (input: string, init: RequestInit) => {
      const headers = new Headers(init.headers);
      // What a browser adds to a same-origin fetch.
      headers.set('origin', ORIGIN);
      headers.set('host', 'firesight.example');
      headers.set('sec-fetch-site', 'same-origin');
      return handle(new Request(input, { ...init, headers }), siteEnv, d);
    }) as unknown as typeof fetch;
  }

  it('sends what the proxy accepts and reads the stream back', async () => {
    const d = deps(() => sse(reply('In about 91 of 100 simulated lives the plan holds.')));
    const transport = createHostedTransport({ origin: ORIGIN, fetch: site(env, d) });
    const history: ApiMessage[] = [];
    const text: string[] = [];
    const result = await runTurn({
      transport,
      request: { model: 'claude-fable-5-2', system: SYSTEM_PROMPT, tools: toolDefinitions() },
      history,
      userContent: [{ type: 'text', text: 'How does my plan look?' }],
      signal: new AbortController().signal,
      executeTool: async () => ({ content: '{}', isError: false }),
      events: { onText: (t) => text.push(t) },
    });
    expect(result.outcome).toBe('complete');
    expect(text.join('')).toBe('In about 91 of 100 simulated lives the plan holds.');
    expect(result.usage.cacheRead).toBe(9000);

    const { init } = d.calls[0]!;
    const headers = init.headers as Record<string, string>;
    // The placeholder credential the SDK holds never leaves the browser; the server key is used.
    expect(headers.authorization).toBe('Bearer sk-ant-server-secret');
    expect(JSON.stringify(headers)).not.toContain('hosted-proxy');
    const sent = JSON.parse(init.body as string) as Record<string, unknown>;
    expect(sent.model).toBe('claude-fable-5-2');
    expect(sent.stream).toBe(true);
    expect(sent.thinking).toEqual({ type: 'adaptive', display: 'summarized' });
    expect(sent.output_config).toEqual({ effort: 'medium' });
    expect(sent.fallbacks).toBe('default');
    expect(sent).not.toHaveProperty('betas');
    expect(sent).not.toHaveProperty('temperature');
    // Cache breakpoints: the frozen system prompt, and the moving tail of the conversation.
    expect((sent.system as { cache_control?: unknown }[])[0]!.cache_control).toEqual({ type: 'ephemeral' });
    expect(sent.cache_control).toEqual({ type: 'ephemeral' });
    expect((sent.tools as { name: string; eager_input_streaming: boolean }[]).every((t) => t.eager_input_streaming)).toBe(true);
  });

  it('surfaces proxy refusals as errors the app can explain', async () => {
    const guarded = { ...env, FIRESIGHT_ACCESS_CODE: 'open sesame' };
    const run = (accessCode?: string) =>
      runTurn({
        transport: createHostedTransport({ origin: ORIGIN, accessCode, fetch: site(guarded, deps()) }),
        request: { model: 'claude-opus-5-5', system: SYSTEM_PROMPT, tools: toolDefinitions() },
        history: [],
        userContent: [{ type: 'text', text: 'Hi' }],
        signal: new AbortController().signal,
        executeTool: async () => ({ content: '{}', isError: false }),
      });
    const denied = await run();
    expect(denied.outcome).toBe('error');
    expect(denied.error).toMatchObject({ kind: 'access_code', retryable: false });
    expect((await run('open sesame')).outcome).toBe('complete');
  });

  it('explains an upstream overload after retrying', async () => {
    const overloaded = () => new Response(JSON.stringify({ type: 'error', error: { type: 'overloaded_error', message: 'Overloaded' } }), { status: 529, headers: { 'content-type': 'application/json' } });
    const d = deps(overloaded);
    const result = await runTurn({
      transport: createHostedTransport({ origin: ORIGIN, fetch: site(env, d) }),
      request: { model: 'claude-opus-5-5', system: SYSTEM_PROMPT, tools: toolDefinitions() },
      history: [],
      userContent: [{ type: 'text', text: 'Hi' }],
      signal: new AbortController().signal,
      executeTool: async () => ({ content: '{}', isError: false }),
      maxRetries: 1,
      sleep: async () => undefined,
    });
    expect(result.error?.kind).toBe('overloaded');
    expect(d.calls).toHaveLength(2);
    expect(classifyError(new TypeError('Failed to fetch'), 'hosted').kind).toBe('network');
  });
});

describe('choosing a transport', () => {
  it('prefers the user\'s key, falls back to the hosted proxy, else nothing', () => {
    const hosted = { available: true, models: ['claude-sonnet-5-5' as const], accessCodeRequired: false };
    expect(chooseTransport({ apiKey: 'sk-ant-user', model: 'claude-fable-5-2', accessCode: '' }, hosted, ORIGIN)).toMatchObject({ model: 'claude-fable-5-2', transport: { mode: 'byok' } });
    // The hosted proxy only offers what the operator allows.
    expect(chooseTransport({ apiKey: '', model: 'claude-fable-5-2', accessCode: '' }, hosted, ORIGIN)).toMatchObject({ model: 'claude-sonnet-5-5', transport: { mode: 'hosted' } });
    expect(chooseTransport({ apiKey: '', model: 'claude-fable-5-2', accessCode: '' }, HOSTED_OFF, ORIGIN)).toBeNull();
  });

  it('detects hosted availability, and treats a static host as unavailable', async () => {
    resetHostedDetection();
    const yes = await detectHosted((async () => handle(new Request(`${ORIGIN}/api/chat`), env)) as unknown as typeof fetch, ORIGIN);
    expect(yes).toEqual({ available: true, models: MODELS, accessCodeRequired: false });
    resetHostedDetection();
    const html = await detectHosted((async () => new Response('<!doctype html>', { headers: { 'content-type': 'text/html' } })) as unknown as typeof fetch, ORIGIN);
    expect(html).toEqual(HOSTED_OFF);
    resetHostedDetection();
    expect(await detectHosted((async () => Promise.reject(new TypeError('offline'))) as unknown as typeof fetch, ORIGIN)).toEqual(HOSTED_OFF);
    resetHostedDetection();
  });
});
