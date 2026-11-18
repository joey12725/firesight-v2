// A scripted stand-in for the model, speaking the same stream interface as the SDK.
// Used by the tests and by the offline smoke script; never bundled into the app.
import Anthropic from '@anthropic-ai/sdk';
import type { ApiEvent, ApiResponse, ModelRequest, ModelStream, Transport, TransportMode } from '../agent/types';

export interface ScriptedMessage {
  thinking?: string;
  text?: string;
  tools?: { id?: string; name: string; input: unknown }[];
  stop?: ApiResponse['stop_reason'];
  /** Prompt tokens to report, for compaction tests. */
  promptTokens?: number;
  /** Never finishes: waits until the request is aborted. */
  hang?: boolean;
  /** Fails after streaming the text, as a dropped connection would. */
  failAfterText?: unknown;
}

export type Scripted = ScriptedMessage | { error: unknown } | ((request: ModelRequest) => ScriptedMessage | { error: unknown });

export function apiError(status: number, type: string, message = type, headers: Record<string, string> = {}) {
  return Anthropic.APIError.generate(status, { type: 'error', error: { type, message } }, message, new Headers(headers));
}

let ids = 0;

function build(spec: ScriptedMessage, model: string): { message: ApiResponse; events: ApiEvent[] } {
  const content: unknown[] = [];
  if (spec.thinking) content.push({ type: 'thinking', thinking: spec.thinking, signature: `sig_${++ids}` });
  if (spec.text) content.push({ type: 'text', text: spec.text, citations: null });
  for (const tool of spec.tools ?? []) content.push({ type: 'tool_use', id: tool.id ?? `toolu_${++ids}`, name: tool.name, input: tool.input });
  const stop = spec.stop ?? ((spec.tools?.length ?? 0) > 0 ? 'tool_use' : 'end_turn');
  const usage = { input_tokens: spec.promptTokens ?? 100, output_tokens: 20, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 };
  const message = { id: `msg_${++ids}`, type: 'message', role: 'assistant', model, content, stop_reason: stop, stop_sequence: null, stop_details: stop === 'refusal' ? { type: 'refusal', category: 'cyber', explanation: '' } : null, usage } as unknown as ApiResponse;
  const events: unknown[] = [{ type: 'message_start', message: { ...message, content: [] } }];
  content.forEach((block, index) => {
    const b = block as { type: string; text?: string; thinking?: string; id?: string; name?: string; input?: unknown };
    if (b.type === 'text') {
      events.push({ type: 'content_block_start', index, content_block: { type: 'text', text: '' } });
      // Two deltas, so consumers are exercised on partial text.
      const half = Math.ceil((b.text ?? '').length / 2);
      events.push({ type: 'content_block_delta', index, delta: { type: 'text_delta', text: (b.text ?? '').slice(0, half) } });
      events.push({ type: 'content_block_delta', index, delta: { type: 'text_delta', text: (b.text ?? '').slice(half) } });
    } else if (b.type === 'thinking') {
      events.push({ type: 'content_block_start', index, content_block: { type: 'thinking', thinking: '', signature: '' } });
      events.push({ type: 'content_block_delta', index, delta: { type: 'thinking_delta', thinking: b.thinking } });
    } else {
      events.push({ type: 'content_block_start', index, content_block: { type: 'tool_use', id: b.id, name: b.name, input: {} } });
      events.push({ type: 'content_block_delta', index, delta: { type: 'input_json_delta', partial_json: JSON.stringify(b.input) } });
    }
    events.push({ type: 'content_block_stop', index });
  });
  events.push({ type: 'message_delta', delta: { stop_reason: stop, stop_sequence: null }, usage });
  events.push({ type: 'message_stop' });
  return { message, events: events as ApiEvent[] };
}

export interface FakeTransport extends Transport {
  /** Every request received, in order. Messages are snapshots at the time of the call. */
  requests: ModelRequest[];
}

/** A transport that plays `script` in order, one entry per request. */
export function scriptedTransport(script: Scripted[], mode: TransportMode = 'byok'): FakeTransport {
  const queue = [...script];
  const requests: ModelRequest[] = [];
  return {
    mode,
    requests,
    stream(request, signal): ModelStream {
      requests.push({ ...request, messages: structuredClone(request.messages) });
      const next = queue.shift();
      const entry = typeof next === 'function' ? next(request) : next;
      let aborted = signal.aborted;
      let wake: (() => void) | null = null;
      const abort = () => {
        aborted = true;
        wake?.();
      };
      const run = async function* (): AsyncGenerator<ApiEvent> {
        if (!entry) throw new Error('The script has no response for this request.');
        if ('error' in entry) throw entry.error;
        const { events } = build(entry, request.model);
        if (entry.hang) {
          for (const event of events.slice(0, 3)) yield event;
          await new Promise<void>((resolve) => {
            wake = resolve;
            if (aborted) resolve();
          });
          throw new Anthropic.APIUserAbortError();
        }
        for (const event of events) {
          if (aborted) throw new Anthropic.APIUserAbortError();
          if (entry.failAfterText !== undefined && event.type === 'content_block_stop') throw entry.failAfterText;
          yield event;
        }
      };
      const iterator = run();
      let done = false;
      return {
        [Symbol.asyncIterator]() {
          return {
            next: async () => {
              const r = await iterator.next();
              done = done || r.done === true;
              return r;
            },
          };
        },
        async finalMessage() {
          // Like the SDK: resolves once the stream has been read to the end.
          while (!done) {
            const r = await iterator.next();
            done = r.done === true;
          }
          if (!entry || 'error' in entry) throw new Error('unreachable');
          return build(entry, request.model).message;
        },
        abort,
      };
    },
  };
}
