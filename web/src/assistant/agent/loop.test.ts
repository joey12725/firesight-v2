// The agent loop against a scripted model: no network, no engine.
import { describe, expect, it } from 'vitest';
import { apiError, scriptedTransport, type Scripted } from '../testing/fake-model';
import { estimateTokens, needsCompaction, summariseHistory, SUMMARY_OPEN } from './context';
import { classifyError, PROXY_ERRORS } from './errors';
import { hasThinking, runTurn, stripThinking, type TurnOptions } from './loop';
import type { ApiMessage, ToolCall, ToolReply, TurnEvents } from './types';
import { validateInput } from './validate-input';

const request = { model: 'claude-opus-5-5' as const, system: 'system', tools: [] };
const user = (text: string) => [{ type: 'text' as const, text }];

function setup(script: Scripted[], overrides: Partial<TurnOptions> = {}) {
  const transport = scriptedTransport(script);
  const history: ApiMessage[] = [];
  const controller = new AbortController();
  const executed: ToolCall[] = [];
  const text: string[] = [];
  const events: TurnEvents = { onText: (d) => text.push(d) };
  const options: TurnOptions = {
    transport,
    request,
    history,
    userContent: user('How does my plan look?'),
    signal: controller.signal,
    executeTool: async (call) => {
      executed.push(call);
      return { content: JSON.stringify({ ok: call.name }), isError: false };
    },
    events,
    sleep: async () => undefined,
    random: () => 0.5,
    ...overrides,
  };
  return { transport, history, controller, executed, text, options };
}

/** Every tool_use in the transcript is answered in the next message, in order. */
function expectWellFormed(history: ApiMessage[]) {
  history.forEach((m, i) => {
    if (m.role !== 'assistant' || !Array.isArray(m.content)) return;
    const uses = m.content.flatMap((b) => (b.type === 'tool_use' ? [b.id] : []));
    if (uses.length === 0) return;
    const next = history[i + 1];
    expect(next?.role).toBe('user');
    const results = Array.isArray(next?.content) ? next.content.flatMap((b) => (b.type === 'tool_result' ? [b.tool_use_id] : [])) : [];
    expect(results).toEqual(uses);
  });
}

describe('runTurn', () => {
  it('runs a tool call, feeds the result back and returns the final text', async () => {
    const { options, history, executed, text, transport } = setup([
      { thinking: 'I should simulate.', text: 'Let me run it.', tools: [{ id: 't1', name: 'run_simulation', input: { paths: 2000 } }] },
      { text: 'About 91 in 100 lives succeed.' },
    ]);
    const result = await runTurn(options);
    expect(result.outcome).toBe('complete');
    expect(executed).toEqual([{ id: 't1', name: 'run_simulation', input: { paths: 2000 } }]);
    expect(text.join('')).toBe('Let me run it.About 91 in 100 lives succeed.');
    expect(history.map((m) => m.role)).toEqual(['user', 'assistant', 'user', 'assistant']);
    // Thinking blocks go back to the model exactly as they came.
    expect((history[1]?.content as { type: string }[])[0]).toMatchObject({ type: 'thinking', thinking: 'I should simulate.' });
    expect(history[2]?.content).toEqual([{ type: 'tool_result', tool_use_id: 't1', content: '{"ok":"run_simulation"}' }]);
    // The second request carried the whole transcript so far.
    expect(transport.requests[1]?.messages).toHaveLength(3);
    expect(result.usage.requests).toBe(2);
    expectWellFormed(history);
  });

  it('runs parallel tool calls concurrently and returns all results in one message, in order', async () => {
    let running = 0;
    let peak = 0;
    const { options, history } = setup(
      [
        {
          tools: [
            { id: 'a', name: 'try_scenario', input: {} },
            { id: 'b', name: 'lookup_cost_of_living', input: {} },
            { id: 'c', name: 'run_simulation', input: {} },
          ],
        },
        { text: 'Done.' },
      ],
      {
        executeTool: async (call) => {
          running += 1;
          peak = Math.max(peak, running);
          // The first call finishes last: order must not depend on timing.
          await new Promise((r) => setTimeout(r, call.id === 'a' ? 15 : 1));
          running -= 1;
          return { content: call.id, isError: false };
        },
      },
    );
    const result = await runTurn(options);
    expect(result.outcome).toBe('complete');
    expect(peak).toBe(3);
    const results = history[2]?.content as { tool_use_id: string; content: string }[];
    expect(results.map((r) => r.tool_use_id)).toEqual(['a', 'b', 'c']);
    expect(history).toHaveLength(4);
  });

  it('feeds a tool error back and lets the model recover', async () => {
    const replies: ToolReply[] = [
      { content: 'patch operation 0 (set "/incomes/@job/kind/salry") failed: no such key "salry"; available: salary, growth', isError: true },
      { content: '{"status":"accepted"}', isError: false },
    ];
    const { options, history, transport } = setup(
      [
        { tools: [{ id: 'p1', name: 'propose_changes', input: { bad: true } }] },
        { tools: [{ id: 'p2', name: 'propose_changes', input: { good: true } }] },
        { text: 'Your salary is updated.' },
      ],
      { executeTool: async () => replies.shift() as ToolReply },
    );
    const result = await runTurn(options);
    expect(result.outcome).toBe('complete');
    expect(history[2]?.content).toEqual([expect.objectContaining({ type: 'tool_result', tool_use_id: 'p1', is_error: true })]);
    expect(history[4]?.content).toEqual([{ type: 'tool_result', tool_use_id: 'p2', content: '{"status":"accepted"}' }]);
    // The model saw the error verbatim on its second request.
    expect(JSON.stringify(transport.requests[1]?.messages)).toContain('no such key');
  });

  it('turns a thrown tool into an error result instead of failing the turn', async () => {
    const { options, history } = setup([{ tools: [{ id: 'x', name: 'solve', input: {} }] }, { text: 'I could not compute that.' }], {
      executeTool: async () => {
        throw new Error('engine exploded');
      },
    });
    const result = await runTurn(options);
    expect(result.outcome).toBe('complete');
    expect(history[2]?.content).toEqual([{ type: 'tool_result', tool_use_id: 'x', content: 'The tool failed: engine exploded', is_error: true }]);
  });

  it('stops mid-stream on abort and leaves a valid transcript', async () => {
    const { options, history, controller, text } = setup([{ text: 'A long answer that never finishes', hang: true }]);
    const pending = runTurn({
      ...options,
      events: {
        onText: (d) => {
          text.push(d);
          controller.abort();
        },
      },
    });
    const result = await pending;
    expect(result.outcome).toBe('aborted');
    expect(text.length).toBeGreaterThan(0);
    // Nothing half-written is kept: only the user's message.
    expect(history).toHaveLength(1);
    expect(history[0]?.role).toBe('user');
  });

  it('answers every pending tool call when aborted while tools run', async () => {
    const { options, history, controller } = setup([
      {
        tools: [
          { id: 'slow', name: 'sweep', input: {} },
          { id: 'fast', name: 'get_plan', input: {} },
        ],
      },
    ]);
    const result = await runTurn({
      ...options,
      executeTool: (call, signal) =>
        call.id === 'fast'
          ? Promise.resolve({ content: 'plan', isError: false })
          : new Promise((_, reject) => {
              signal.addEventListener('abort', () => reject(new DOMException('Aborted', 'AbortError')));
              setTimeout(() => controller.abort(), 5);
            }),
    });
    expect(result.outcome).toBe('aborted');
    expect(history).toHaveLength(3);
    const results = history[2]?.content as { tool_use_id: string; is_error?: boolean; content: string }[];
    expect(results).toEqual([
      { type: 'tool_result', tool_use_id: 'slow', content: 'Cancelled by the user before this finished.', is_error: true },
      { type: 'tool_result', tool_use_id: 'fast', content: 'plan' },
    ]);
    expectWellFormed(history);
  });

  it('retries overload with backoff, drops the partial output, and succeeds', async () => {
    const sleeps: number[] = [];
    const retries: number[] = [];
    const { options, history, transport } = setup([{ error: apiError(529, 'overloaded_error') }, { text: 'partial', failAfterText: apiError(529, 'overloaded_error') }, { text: 'Here is the answer.' }], {
      sleep: async (ms) => void sleeps.push(ms),
    });
    const result = await runTurn({ ...options, events: { onRetry: (info) => retries.push(info.attempt) } });
    expect(result.outcome).toBe('complete');
    expect(transport.requests).toHaveLength(3);
    expect(sleeps).toEqual([1000, 2000]);
    expect(retries).toEqual([1, 2]);
    expect(history).toHaveLength(2);
    expect(history[1]?.content).toEqual([expect.objectContaining({ type: 'text', text: 'Here is the answer.' })]);
  });

  it('honours retry-after on a rate limit and gives up after the retry budget', async () => {
    const sleeps: number[] = [];
    const limited = () => ({ error: apiError(429, 'rate_limit_error', 'slow down', { 'retry-after': '3' }) });
    const { options, history } = setup([limited(), limited(), limited()], { maxRetries: 2, sleep: async (ms) => void sleeps.push(ms) });
    const result = await runTurn(options);
    expect(result.outcome).toBe('error');
    expect(result.error?.kind).toBe('rate_limit');
    expect(sleeps).toEqual([3000, 3000]);
    expect(history).toHaveLength(1);
  });

  it('does not retry a bad key and says what to do', async () => {
    const { options, transport } = setup([{ error: apiError(401, 'authentication_error', 'invalid x-api-key') }]);
    const result = await runTurn(options);
    expect(result.outcome).toBe('error');
    expect(result.error).toMatchObject({ kind: 'auth', retryable: false });
    expect(result.error?.message).toMatch(/Settings/);
    expect(transport.requests).toHaveLength(1);
  });

  it('reports a network failure as retryable', async () => {
    const { options } = setup([{ error: new TypeError('Failed to fetch') }, { text: 'Back.' }]);
    const result = await runTurn(options);
    expect(result.outcome).toBe('complete');
  });

  it('never runs tools from a refused or truncated response', async () => {
    const refused = setup([{ tools: [{ id: 'r', name: 'run_simulation', input: {} }], stop: 'refusal' }]);
    const a = await runTurn(refused.options);
    expect(a.outcome).toBe('refusal');
    expect(a.refusalCategory).toBe('cyber');
    expect(refused.executed).toHaveLength(0);
    // The refused request is not left at the end of the transcript.
    expect(refused.history).toHaveLength(0);

    const cut = setup([{ tools: [{ id: 'm', name: 'propose_changes', input: { ops: [] } }], stop: 'max_tokens' }]);
    const b = await runTurn(cut.options);
    expect(b.outcome).toBe('max_tokens');
    expect(cut.executed).toHaveLength(0);
    expectWellFormed(cut.history);
  });

  it('stops at the step limit with a valid transcript', async () => {
    const again = (i: number) => ({ tools: [{ id: `s${i}`, name: 'get_plan', input: {} }] });
    const { options, history } = setup([again(1), again(2), again(3)], { maxSteps: 3 });
    const result = await runTurn(options);
    expect(result.outcome).toBe('step_limit');
    expect(history).toHaveLength(7);
    expectWellFormed(history);
  });

  it('recovers once from a history the API rejects by dropping reasoning blocks', async () => {
    const { options, history, transport } = setup([{ error: apiError(400, 'invalid_request_error', 'Invalid `signature` in `thinking` block.') }, { text: 'Carrying on.' }]);
    history.push(
      { role: 'user', content: user('Earlier question') },
      { role: 'assistant', content: [{ type: 'thinking', thinking: 'old', signature: 'sig' }, { type: 'text', text: 'Earlier answer' }] },
    );
    const result = await runTurn(options);
    expect(result.outcome).toBe('complete');
    expect(hasThinking(history)).toBe(false);
    expect(JSON.stringify(transport.requests[1]?.messages)).not.toContain('"thinking"');
    expect(history.map((m) => m.role)).toEqual(['user', 'assistant', 'user', 'assistant']);
  });
});

describe('history helpers', () => {
  it('strips reasoning blocks and drops messages left empty', () => {
    const history: ApiMessage[] = [
      { role: 'user', content: user('q') },
      { role: 'assistant', content: [{ type: 'thinking', thinking: 't', signature: 's' }] },
      { role: 'assistant', content: [{ type: 'thinking', thinking: 't', signature: 's' }, { type: 'text', text: 'a' }] },
    ];
    stripThinking(history);
    expect(history).toEqual([{ role: 'user', content: user('q') }, { role: 'assistant', content: [{ type: 'text', text: 'a' }] }]);
  });
});

describe('context management', () => {
  const long: ApiMessage[] = Array.from({ length: 12 }, (_, i) => ({
    role: i % 2 === 0 ? ('user' as const) : ('assistant' as const),
    content: [{ type: 'text' as const, text: `${i}: ${'x'.repeat(4000)}` }],
  }));

  it('decides to compact from the last prompt size or the estimate', () => {
    expect(needsCompaction(long, 0, 'byok')).toBe(false);
    expect(needsCompaction(long, 200_000, 'byok')).toBe(true);
    expect(needsCompaction(long, 0, 'byok', 5_000)).toBe(true);
    expect(needsCompaction(long.slice(0, 2), 200_000, 'byok')).toBe(false);
    expect(estimateTokens(long)).toBeGreaterThan(12_000);
  });

  it('counts attachments by kind, not by their base64 length', () => {
    const withImage: ApiMessage[] = [{ role: 'user', content: [{ type: 'image', source: { type: 'base64', media_type: 'image/png', data: 'A'.repeat(400_000) } }] }];
    expect(estimateTokens(withImage)).toBeLessThan(5_000);
  });

  it('replaces a long transcript with a summary and nothing else', async () => {
    const transport = scriptedTransport([{ thinking: 'summarising', text: 'The user is Maya, 35, in Denver. Baseline: 91 of 100 lives succeed (10,000 lives, seed 1).' }]);
    const summary = await summariseHistory(transport, request, long, new AbortController().signal);
    expect(summary?.type).toBe('text');
    const text = (summary as { text: string }).text;
    expect(text.startsWith(SUMMARY_OPEN)).toBe(true);
    expect(text).toContain('91 of 100 lives');
    // The summary request reuses the conversation's exact prefix (for the prompt cache)
    // and only appends the instruction.
    const sent = transport.requests[0]?.messages ?? [];
    expect(sent.slice(0, long.length)).toEqual(long);
    expect(sent).toHaveLength(long.length + 1);

    // The new transcript opens with the summary; no old turn or reasoning block is replayed.
    const history: ApiMessage[] = [];
    const next = scriptedTransport([{ text: 'Continuing.' }]);
    const result = await runTurn({ transport: next, request, history, userContent: [summary!, ...user('And if I retire at 55?')], signal: new AbortController().signal, executeTool: async () => ({ content: '', isError: false }) });
    expect(result.outcome).toBe('complete');
    expect(next.requests[0]?.messages).toHaveLength(1);
    expect(JSON.stringify(next.requests[0]?.messages)).not.toContain('xxxx');
  });

  it('keeps the full history when no summary comes back', async () => {
    const transport = scriptedTransport([{ error: apiError(529, 'overloaded_error') }]);
    expect(await summariseHistory(transport, request, long, new AbortController().signal)).toBeNull();
  });
});

describe('classifyError', () => {
  it('names the fix for each failure', () => {
    expect(classifyError(apiError(401, 'authentication_error'), 'byok')).toMatchObject({ kind: 'auth', title: 'The API key was not accepted' });
    expect(classifyError(apiError(401, 'authentication_error'), 'hosted').message).toMatch(/operator/);
    expect(classifyError(apiError(401, PROXY_ERRORS.accessCode), 'hosted').kind).toBe('access_code');
    expect(classifyError(apiError(429, PROXY_ERRORS.rateLimit, 'x', { 'retry-after': '20' }), 'hosted')).toMatchObject({ kind: 'rate_limit', retryable: true, retryAfterMs: 20_000 });
    expect(classifyError(apiError(413, 'request_too_large'), 'hosted').message).toMatch(/4 MB/);
    expect(classifyError(apiError(529, 'overloaded_error'), 'byok')).toMatchObject({ kind: 'overloaded', retryable: true });
    expect(classifyError(apiError(500, 'api_error'), 'byok')).toMatchObject({ kind: 'server', retryable: true });
    expect(classifyError(apiError(404, 'not_found_error'), 'byok').kind).toBe('not_found');
    expect(classifyError(apiError(400, 'invalid_request_error', 'messages: too long'), 'byok')).toMatchObject({ kind: 'bad_request', detail: 'messages: too long' });
    expect(classifyError(new TypeError('Failed to fetch'), 'byok').kind).toBe('network');
  });
});

describe('validateInput', () => {
  const schema = {
    type: 'object',
    properties: {
      name: { type: 'string', minLength: 1 },
      paths: { type: 'integer', minimum: 200, maximum: 20000 },
      metric: { type: 'string', enum: ['success_rate', 'median_fi_age'] },
      ops: { type: 'array', minItems: 1, items: { type: 'object', properties: { op: { type: 'string', enum: ['set'] }, path: { type: 'string' }, value: {} }, required: ['op', 'path'], additionalProperties: false } },
    },
    required: ['name'],
    additionalProperties: false,
  } as const;

  it('accepts valid input and any JSON under an untyped field', () => {
    expect(validateInput(schema, { name: 'x', paths: 2000, ops: [{ op: 'set', path: '/a', value: { nested: [1, null] } }] })).toEqual([]);
  });

  it('reports what a truncated or malformed input is missing', () => {
    expect(validateInput(schema, {})).toEqual(['input.name is required']);
    expect(validateInput(schema, { name: 'x', paths: 20.5 })[0]).toMatch(/must be integer/);
    expect(validateInput(schema, { name: 'x', metric: 'success' })[0]).toMatch(/must be one of/);
    expect(validateInput(schema, { name: 'x', extra: 1 })[0]).toMatch(/not a known field/);
    expect(validateInput(schema, { name: 'x', ops: [{ op: 'set' }] })).toEqual(['input.ops[0].path is required']);
    expect(validateInput(schema, 'not an object')[0]).toMatch(/must be object/);
  });
});
