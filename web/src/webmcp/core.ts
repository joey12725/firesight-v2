// The one path every tool call takes: read the abort signal if the browser gave one,
// check the input against the tool's schema (WebMCP does not), wait for the app to be
// ready, ask the user if they have not yet let agents in, run, cap the result. Whatever
// happens the caller gets a plain JSON object; the only thing thrown is an abort.
import { validateInput } from '@/assistant/agent/validate-input';
import { ToolError } from '@/assistant/tools/types';
import { AgentError, type AgentContext, type AgentTool, type Json, type ToolResult } from './types';

/** The `{ signal }` WebMCP passes as the second argument since Chrome 153; absent before, and elsewhere something else entirely. */
export function signalOf(options: unknown): AbortSignal | undefined {
  const signal = (options as { signal?: unknown } | null | undefined)?.signal;
  return signal instanceof AbortSignal ? signal : undefined;
}

const abortError = () => new DOMException('Aborted', 'AbortError');

/** `promise`, unless `signal` aborts first. */
export function abortable<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  if (signal.aborted) return Promise.reject(abortError());
  return new Promise<T>((resolve, reject) => {
    const onAbort = () => reject(abortError());
    signal.addEventListener('abort', onAbort, { once: true });
    promise.then(
      (value) => {
        signal.removeEventListener('abort', onAbort);
        resolve(value);
      },
      (error: unknown) => {
        signal.removeEventListener('abort', onAbort);
        reject(error);
      },
    );
  });
}

/** Resolves after `ms`, or rejects with an AbortError as soon as `signal` aborts. */
export function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return abortable(new Promise<void>((resolve) => setTimeout(resolve, ms)), signal);
}

export const fail = (code: string, message: string, extra: Json = {}): ToolResult => ({ ok: false, error: { code, message }, ...extra });

/** Chrome before 155 handed tools their input as a JSON string; a tool with no arguments may get nothing at all. */
function readInput(raw: unknown): unknown {
  if (raw === undefined || raw === null) return {};
  if (typeof raw !== 'string') return raw;
  try {
    return JSON.parse(raw) as unknown;
  } catch {
    return raw;
  }
}

const REFUSALS: Record<string, string> = {
  agent_access_off: 'The user has switched off agent access to FIRESight in this browser (Settings, Browser agents). Tell them; do not retry.',
  agent_access_denied: 'The user did not allow agent access to their plan in this browser. Tell them; they can change it under Settings, Browser agents. Do not retry.',
  user_did_not_respond: 'The user was asked on the FIRESight page and has not answered yet. Ask them to look at the page, then call again.',
};

async function guarded(tool: AgentTool, ctx: AgentContext, raw: unknown, signal: AbortSignal): Promise<ToolResult> {
  const { access } = ctx.settings();
  // The user can switch access off while a call waits or runs. It is read again after every wait, so a
  // call that was already under way answers as any other would: off, and nothing from the plan.
  const off = () => ctx.settings().access === 'off' && !tool.worksWhenOff;
  const refuseOff = () => fail('agent_access_off', REFUSALS.agent_access_off as string);
  if (off()) return refuseOff();

  const input = readInput(raw);
  const problems = validateInput(tool.inputSchema, input);
  if (problems.length > 0) {
    return fail('invalid_input', `The input for ${tool.name} is not valid: ${problems.slice(0, 6).join('; ')}.`, { problems: problems.slice(0, 12) });
  }

  if (tool.awaitReady !== false) {
    try {
      await abortable(ctx.ready(), signal);
    } catch (error) {
      if (signal.aborted) throw error;
      return fail('app_failed', `FIRESight could not start its simulation engine in this browser: ${error instanceof Error ? error.message : String(error)}. Ask the user to reload the page.`);
    }
  }

  if (tool.consent === 'required' && access === 'ask') {
    const answer = ctx.consent() === 'undecided' ? await ctx.askConsent(signal) : ctx.consent();
    if (off()) return refuseOff();
    if (answer === 'denied') return fail('agent_access_denied', REFUSALS.agent_access_denied as string);
    if (answer !== 'granted') return fail('user_did_not_respond', REFUSALS.user_did_not_respond as string, { retryable: true });
  }

  let result: Json;
  try {
    result = await tool.run(input, ctx, { signal });
  } catch (error) {
    if (off() && (error instanceof AgentError || error instanceof ToolError)) return refuseOff();
    if (error instanceof AgentError) return fail(error.code, error.message, error.extra);
    if (error instanceof ToolError) return fail('tool_error', error.message);
    throw error;
  }
  if (off()) return refuseOff();
  const out: ToolResult = { ok: true, ...result };
  const size = JSON.stringify(out).length;
  if (size > ctx.limits.maxResultChars) {
    return fail('result_too_large', `The result of ${tool.name} was too large to return (${size} characters). Ask for less: fewer rows, a narrower range, or one section at a time.`);
  }
  return out;
}

const REFUSED = new Set(['agent_access_off', 'agent_access_denied', 'user_did_not_respond', 'declined']);

/**
 * Runs one tool call from the browser. `options` is whatever WebMCP passed as the
 * second argument, or nothing. Resolves a JSON object; rejects only when the call was
 * aborted.
 */
export async function invokeTool(tool: AgentTool, ctx: AgentContext, rawInput: unknown, options?: unknown): Promise<ToolResult> {
  const signal = signalOf(options) ?? new AbortController().signal;
  const entry = ctx.activity.start(tool.name);
  try {
    const result = await guarded(tool, ctx, rawInput, signal);
    if (result.ok) ctx.activity.end(entry, 'ok');
    else ctx.activity.end(entry, REFUSED.has(result.error.code) ? 'refused' : 'error', result.error.code);
    return result;
  } catch (error) {
    if (signal.aborted) {
      ctx.activity.end(entry, 'cancelled');
      throw error;
    }
    // A bug in a tool, not something the agent did. The message carries no plan data.
    ctx.activity.end(entry, 'error', 'internal_error');
    return fail('internal_error', `${tool.name} failed inside FIRESight: ${error instanceof Error ? error.message : String(error)}`.slice(0, 400));
  }
}

/** A tool as `document.modelContext.registerTool` takes it. */
export interface WebMcpToolDefinition {
  name: string;
  title: string;
  description: string;
  inputSchema: object;
  annotations: { readOnlyHint: boolean; consequentialHint: boolean; untrustedContentHint: boolean };
  execute(input: unknown, options?: unknown): Promise<ToolResult>;
}

/** The WebMCP form of a tool, bound to a context. The schema is copied so the browser cannot be handed our frozen literals. */
export function toDefinition(tool: AgentTool, ctx: AgentContext): WebMcpToolDefinition {
  return {
    name: tool.name,
    title: tool.title,
    description: tool.description,
    inputSchema: JSON.parse(JSON.stringify(tool.inputSchema)) as object,
    annotations: { ...tool.annotations },
    execute: (input, options) => invokeTool(tool, ctx, input, options),
  };
}
