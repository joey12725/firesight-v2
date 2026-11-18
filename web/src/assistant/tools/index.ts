// The assistant's tools: definitions for the model, and the executor the agent loop calls.
import { isAbort } from '@/engine/errors';
import { isAbortError } from '../agent/errors';
import type { ApiTool, ToolCall, ToolReply } from '../agent/types';
import { validateInput } from '../agent/validate-input';
import { rankLevers, solve, sweep } from './explore';
import { explainTax, inspectLife } from './inspect';
import { lookupCostOfLiving, showChart } from './misc';
import { getPlan, getSchema, proposeChanges } from './plan';
import { explainEngineError } from './shared';
import { runSimulation, tryScenario } from './simulate';
import { ToolError, type ToolContext, type ToolSpec, type ToolView } from './types';

export * from './types';
export { RunRegistry, type RunRecord } from './runs';
export { currentPlanView } from './plan';

/** Sorted by name: the tool list is the head of the cached prompt and must never reorder. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any -- each spec is typed at its definition; the registry is heterogeneous
export const TOOLS: ToolSpec<any>[] = [explainTax, getPlan, getSchema, inspectLife, lookupCostOfLiving, proposeChanges, rankLevers, runSimulation, showChart, solve, sweep, tryScenario].sort((a, b) =>
  a.name < b.name ? -1 : 1,
);

export const TOOL_NAMES = TOOLS.map((t) => t.name);

/**
 * Tool definitions for the request. `eager_input_streaming` streams large inputs (a
 * patch with dozens of operations) as they are written; the API then no longer
 * validates them, so `executeTool` checks every input against its schema first.
 */
export function toolDefinitions(): ApiTool[] {
  return TOOLS.map((t) => ({ name: t.name, description: t.description, input_schema: t.input_schema as ApiTool['input_schema'], eager_input_streaming: true }));
}

/** Tool results above this size are a bug in the tool, not something to send. About 4,500 tokens. */
export const MAX_RESULT_CHARS = 16_000;

export interface ToolRun {
  reply: ToolReply;
  view: ToolView;
}

export function pendingTitle(name: string, input: unknown): string {
  const tool = TOOLS.find((t) => t.name === name);
  try {
    return tool ? tool.pending(input) : `Calling ${name}`;
  } catch {
    return `Calling ${name}`;
  }
}

export async function executeTool(ctx: ToolContext, call: ToolCall, signal: AbortSignal, onView?: (view: ToolView) => void): Promise<ToolRun> {
  const fail = (title: string, content: string): ToolRun => ({ reply: { content, isError: true }, view: { title } });
  const tool = TOOLS.find((t) => t.name === call.name);
  if (!tool) return fail(`Unknown tool ${call.name}`, `There is no tool named "${call.name}". Tools: ${TOOL_NAMES.join(', ')}.`);
  const problems = validateInput(tool.input_schema, call.input);
  if (problems.length > 0) {
    // The same shape the API guidance uses for input that arrived malformed.
    return fail(`The input for ${call.name} was not valid`, JSON.stringify({ INVALID_INPUT: call.input, problems: problems.slice(0, 12) }));
  }
  try {
    const outcome = await tool.run(call.input, ctx, { id: call.id, signal, update: (view) => onView?.(view) });
    const content = JSON.stringify(outcome.result);
    if (content.length > MAX_RESULT_CHARS) {
      return fail(outcome.view.title, `The result of ${call.name} was too large to return (${content.length} characters). Ask for less: fewer values, a narrower range, or one section at a time.`);
    }
    return { reply: { content, isError: outcome.isError === true }, view: outcome.view };
  } catch (thrown) {
    if (signal.aborted || isAbort(thrown) || isAbortError(thrown)) throw thrown;
    const error = thrown instanceof ToolError ? thrown : explainEngineError(thrown);
    return fail(error.title, error.message);
  }
}
