// The agent loop: one user turn, streamed, with client-side tools.
//
// Invariants the rest of the app relies on:
//   * `history` is append-only while a turn runs, and whenever `runTurn` returns it is
//     a valid Messages API transcript (every tool_use has its tool_result). Thinking
//     blocks are passed back exactly as received: current models bind each one to the
//     conversation that produced it and reject edited histories.
//   * Tool calls of one response run in parallel and their results go back in a
//     single user message, in the order the model asked for them.
//   * Tools never run on a response that was cut off (max_tokens) or refused.
import { classifyError, isAbortError } from './errors';
import type {
  ApiContent,
  ApiMessage,
  ApiResponse,
  AssistantError,
  ModelRequest,
  ToolCall,
  ToolReply,
  Transport,
  TurnEvents,
  TurnResult,
  TurnUsage,
} from './types';

export interface TurnOptions {
  transport: Transport;
  request: Omit<ModelRequest, 'messages'>;
  /** The conversation so far. Mutated: the user turn and everything after is appended. */
  history: ApiMessage[];
  userContent: ApiContent[];
  /** Runs one tool. Must not throw for ordinary failures: return `isError` instead. */
  executeTool(call: ToolCall, signal: AbortSignal): Promise<ToolReply>;
  signal: AbortSignal;
  events?: TurnEvents;
  /** Requests to the model in one turn before the loop gives up. */
  maxSteps?: number;
  /** Retries of one request on overload, rate limit, server and network errors. */
  maxRetries?: number;
  sleep?: (ms: number, signal: AbortSignal) => Promise<void>;
  random?: () => number;
}

export const DEFAULT_MAX_STEPS = 16;
export const DEFAULT_MAX_RETRIES = 4;
const BASE_DELAY_MS = 1_000;
const MAX_DELAY_MS = 20_000;
/** A server-requested wait longer than this is reported instead of slept through. */
const MAX_RETRY_AFTER_MS = 45_000;
const CANCELLED = 'Cancelled by the user before this finished.';

export function abortableSleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal.aborted) return reject(new DOMException('Aborted', 'AbortError'));
    const onAbort = () => {
      clearTimeout(timer);
      reject(new DOMException('Aborted', 'AbortError'));
    };
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort);
      resolve();
    }, ms);
    signal.addEventListener('abort', onAbort, { once: true });
  });
}

/** Exponential backoff with jitter; a `retry-after` from the server wins. */
export function retryDelay(attempt: number, error: AssistantError, random: () => number): number {
  if (error.retryAfterMs !== undefined) return error.retryAfterMs;
  const exp = Math.min(MAX_DELAY_MS, BASE_DELAY_MS * 2 ** attempt);
  return Math.round(exp * (0.75 + 0.5 * random()));
}

const isThinking = (block: { type: string }) => block.type === 'thinking' || block.type === 'redacted_thinking';

export function hasThinking(history: ApiMessage[]): boolean {
  return history.some((m) => m.role === 'assistant' && Array.isArray(m.content) && m.content.some(isThinking));
}

/**
 * Removes every thinking block, in place. Removing all of them is one of the few edits
 * the API's history check allows; it is the recovery when a stored conversation no
 * longer matches the prompt or tools it was produced under.
 */
export function stripThinking(history: ApiMessage[]): void {
  for (let i = history.length - 1; i >= 0; i--) {
    const m = history[i] as ApiMessage;
    if (m.role !== 'assistant' || !Array.isArray(m.content)) continue;
    const kept = m.content.filter((b) => !isThinking(b));
    if (kept.length === m.content.length) continue;
    if (kept.length > 0) history[i] = { ...m, content: kept };
    else history.splice(i, 1);
  }
}

async function readStream(opts: TurnOptions, messages: ApiMessage[]): Promise<ApiResponse> {
  const { events, signal } = opts;
  const stream = opts.transport.stream({ ...opts.request, messages }, signal);
  const onAbort = () => stream.abort();
  signal.addEventListener('abort', onAbort, { once: true });
  try {
    // Iteration and the final read are consumed together: with eager tool-input
    // streaming a tool input that cannot be parsed rejects whichever is awaited.
    for await (const event of stream) {
      if (event.type === 'content_block_start') {
        if (event.content_block.type === 'tool_use') events?.onToolDraft?.({ id: event.content_block.id, name: event.content_block.name });
      } else if (event.type === 'content_block_delta') {
        if (event.delta.type === 'text_delta') events?.onText?.(event.delta.text);
        else if (event.delta.type === 'thinking_delta') events?.onThinking?.(event.delta.thinking);
      }
    }
    return await stream.finalMessage();
  } finally {
    signal.removeEventListener('abort', onAbort);
  }
}

export async function runTurn(opts: TurnOptions): Promise<TurnResult> {
  const { history, events, signal, transport } = opts;
  const maxSteps = opts.maxSteps ?? DEFAULT_MAX_STEPS;
  const maxRetries = opts.maxRetries ?? DEFAULT_MAX_RETRIES;
  const sleep = opts.sleep ?? abortableSleep;
  const random = opts.random ?? Math.random;
  const usage: TurnUsage = { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, lastPrompt: 0, requests: 0 };
  const finish = (outcome: TurnResult['outcome'], extra: Partial<TurnResult> = {}): TurnResult => ({ outcome, usage, ...extra });

  const turnStart = history.length;
  history.push({ role: 'user', content: opts.userContent });
  let recoveredHistory = false;

  for (let step = 0; step < maxSteps; step++) {
    if (signal.aborted) return finish('aborted');
    events?.onStep?.(step);

    let message: ApiResponse | null = null;
    for (let attempt = 0; message === null; attempt++) {
      try {
        message = await readStream(opts, history);
      } catch (thrown) {
        if (signal.aborted || isAbortError(thrown)) return finish('aborted');
        const error = classifyError(thrown, transport.mode);
        // A conversation saved under an older prompt or tool set fails the API's
        // history check. Dropping the thinking blocks is the documented recovery;
        // it is tried once, never as a steady state.
        if (error.kind === 'bad_request' && !recoveredHistory && hasThinking(history)) {
          recoveredHistory = true;
          stripThinking(history);
          events?.onRetry?.({ attempt, delayMs: 0, error });
          continue;
        }
        const tooLong = error.retryAfterMs !== undefined && error.retryAfterMs > MAX_RETRY_AFTER_MS;
        // Unreadable streams (kind `unknown`) get two more goes; API errors get the full budget.
        const cap = error.kind === 'unknown' ? Math.min(2, maxRetries) : maxRetries;
        if (!error.retryable || attempt >= cap || tooLong) return finish('error', { error });
        const delayMs = retryDelay(attempt, error, random);
        events?.onRetry?.({ attempt: attempt + 1, delayMs, error });
        try {
          await sleep(delayMs, signal);
        } catch {
          return finish('aborted');
        }
      }
    }

    usage.requests += 1;
    usage.input += message.usage.input_tokens;
    usage.output += message.usage.output_tokens;
    usage.cacheRead += message.usage.cache_read_input_tokens ?? 0;
    usage.cacheWrite += message.usage.cache_creation_input_tokens ?? 0;
    usage.lastPrompt =
      message.usage.input_tokens + (message.usage.cache_read_input_tokens ?? 0) + (message.usage.cache_creation_input_tokens ?? 0);
    events?.onUsage?.(usage);

    // Check the stop reason before reading content.
    if (message.stop_reason === 'refusal') {
      // Leaving a refused request at the end of the transcript would have every later
      // turn refused as well. Removing a trailing turn is a safe edit.
      if (step === 0) history.length = turnStart;
      return finish('refusal', { refusalCategory: message.stop_details?.category ?? null });
    }

    const calls: ToolCall[] = message.content.flatMap((b) => (b.type === 'tool_use' ? [{ id: b.id, name: b.name, input: b.input }] : []));
    const hasVisible = message.content.some((b) => b.type === 'text' || b.type === 'tool_use');

    if (message.stop_reason === 'max_tokens' && calls.length > 0) {
      // A tool input cut off mid-way usually still parses. Never run it.
      return finish('max_tokens');
    }
    if (hasVisible) history.push({ role: 'assistant', content: message.content as ApiContent[] });
    if (message.stop_reason === 'max_tokens') return finish('max_tokens');
    if (message.stop_reason === 'pause_turn') continue;
    if (calls.length === 0) return finish('complete');

    events?.onToolCalls?.(calls);
    const replies = await Promise.all(
      calls.map(async (call): Promise<ToolReply> => {
        if (signal.aborted) return { content: CANCELLED, isError: true };
        let reply: ToolReply;
        try {
          reply = await opts.executeTool(call, signal);
        } catch (thrown) {
          reply = signal.aborted || isAbortError(thrown)
            ? { content: CANCELLED, isError: true }
            : { content: `The tool failed: ${thrown instanceof Error ? thrown.message : String(thrown)}`, isError: true };
        }
        events?.onToolResult?.(call, reply);
        return reply;
      }),
    );
    history.push({
      role: 'user',
      content: calls.map((call, i) => {
        const reply = replies[i] as ToolReply;
        return { type: 'tool_result' as const, tool_use_id: call.id, content: reply.content, ...(reply.isError ? { is_error: true } : {}) };
      }),
    });
    if (signal.aborted) return finish('aborted');
  }
  return finish('step_limit');
}
