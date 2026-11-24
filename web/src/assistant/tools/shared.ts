// Helpers shared by the tool executors.
import { EngineError, toEngineError } from '@/engine/errors';
import type { PatchOp, Profile } from '@/engine/types';
import type { JsonSchema } from '../agent/validate-input';
import { ToolError, type ToolContext } from './types';

/** Lives per run when the user's plan is simulated for an answer. */
export const FULL_PATHS_MAX = 50_000;
export const FULL_PATHS_DEFAULT_CAP = 25_000;
/** Lives per run when many variants are simulated at once (levers, solver probes). */
export const REDUCED_PATHS = 2_000;
export const SWEEP_PATHS = 1_000;
export const MIN_PATHS = 200;

export const PATCH_OP_SCHEMA: JsonSchema = {
  type: 'object',
  properties: {
    op: { type: 'string', enum: ['set', 'remove', 'append'] },
    path: { type: 'string', minLength: 1, description: 'JSON pointer; address list elements as @id, e.g. /incomes/@alex_job/kind/salary' },
    value: { description: 'Required for set and append. Any JSON value.' },
  },
  required: ['op', 'path'],
  additionalProperties: false,
};

export const PATHS_SCHEMA = (fallback: string): JsonSchema => ({
  type: 'integer',
  minimum: MIN_PATHS,
  maximum: FULL_PATHS_MAX,
  description: `Simulated lives per run. Omit for the default (${fallback}). More lives narrow the sampling intervals and take longer.`,
});

export function clampPaths(requested: number | undefined, fallback: number): number {
  return Math.min(FULL_PATHS_MAX, Math.max(MIN_PATHS, Math.round(requested ?? fallback)));
}

export function fullPaths(ctx: ToolContext, requested: number | undefined): number {
  return clampPaths(requested, Math.min(ctx.store.run().paths, FULL_PATHS_DEFAULT_CAP));
}

export function requireProfile(ctx: ToolContext): Profile {
  const profile = ctx.store.profile();
  if (!profile) throw new ToolError('No plan is open. Ask the user to open or create one.');
  return profile;
}

/** The plan, or an error naming what stops it from being simulated. */
export function requireRunnable(ctx: ToolContext): Profile {
  const profile = requireProfile(ctx);
  const errors = ctx.store.report().errors;
  if (errors.length > 0) {
    throw new ToolError(
      `The plan cannot be simulated until these are fixed (use propose_changes): ${errors
        .slice(0, 8)
        .map((e) => `${e.code} at ${e.path || '/'}: ${e.message}`)
        .join(' | ')}`,
      'The plan has errors',
    );
  }
  return profile;
}

/** Applies ops to a copy of `profile`; the patch engine's message is passed through verbatim. */
export function variant(ctx: ToolContext, profile: Profile, ops: PatchOp[], what: string): Profile {
  let result;
  try {
    result = ctx.engine.applyPatch(profile, ops);
  } catch (error) {
    throw new ToolError(`${what}: ${toEngineError(error).message}`, 'The edit did not apply');
  }
  if (result.errors.length > 0) {
    throw new ToolError(
      `${what}: the edited plan is not valid: ${result.errors
        .slice(0, 6)
        .map((e) => `${e.code} at ${e.path || '/'}: ${e.message}`)
        .join(' | ')}`,
      'The edit makes the plan invalid',
    );
  }
  return result.profile;
}

export function asOps(value: unknown): PatchOp[] {
  return (value as PatchOp[]).map((op) => (op.op === 'remove' ? { op: 'remove', path: op.path } : { op: op.op, path: op.path, value: (op as { value?: unknown }).value ?? null }));
}

/** Engine failures in words the model can relay without inventing numbers. */
export function explainEngineError(error: unknown): ToolError {
  if (error instanceof ToolError) return error;
  const e = error instanceof EngineError ? error : toEngineError(error);
  if (e.kind === 'not_implemented') {
    return new ToolError(
      'The simulation engine in this build cannot run yet, so there are no results. Tell the user plainly that no figures are available; do not estimate any.',
      'The simulator is not available in this build',
    );
  }
  if (e.kind === 'invalid_profile') {
    return new ToolError(`The engine rejected the plan: ${(e.report?.errors ?? []).slice(0, 6).map((i) => `${i.code} at ${i.path || '/'}: ${i.message}`).join(' | ') || e.message}`, 'The plan has errors');
  }
  if (e.kind === 'aborted') throw e;
  return new ToolError(`The simulation failed: ${e.message}`, 'The simulation failed');
}

export const count = (n: number) => n.toLocaleString('en-US');
