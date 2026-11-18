// What a tool is, and what it is given to work with. Everything a tool touches comes
// through `ToolContext`, so each executor runs unchanged against the real engine and
// store in the app and against fakes in tests.
import type { EnginePool } from '@/engine/pool';
import type { Comparison, PathTrace, PlanPreview, SummaryDigest } from '@/engine/output-types';
import type { Lever, PatchOp, Profile, Report } from '@/engine/types';
import type { City } from '@/editor/cities';
import type { EditResult, Scenario } from '@/state/profile-store';
import type { JsonSchema } from '../agent/validate-input';
import type { ChartKind } from '../charts';
import type { RunRecord, RunRegistry } from './runs';

/** The plan as the tools see it: the profile store, narrowed to what they use. */
export interface PlanStore {
  profile(): Profile | null;
  report(): Report;
  scenarios(): Scenario[];
  /** The household's run settings. `seed` is the session's fixed seed for every tool. */
  run(): { paths: number; seed: number };
  preview(ops: PatchOp[]): { profile: Profile; report: Report } | { error: string };
  /** Applies to the baseline plan as one undoable step. */
  edit(ops: PatchOp[], label: string): EditResult;
  addScenario(input: { name: string; patch: PatchOp[] }): EditResult & { id?: string };
}

export interface EngineBridge {
  pool: Pick<EnginePool, 'runMany' | 'trace'>;
  /** Throws an `EngineError` whose message is written for a model to read and retry. */
  applyPatch(profile: Profile, ops: PatchOp[]): { profile: Profile; errors: Report['errors']; warnings: Report['warnings'] };
  levers(profile: Profile): Lever[];
  schema(): unknown;
  /** Core's deterministic plan facts. Throws an `EngineError` of kind `invalid_profile` when the plan cannot be compiled. */
  planPreview(profile: Profile): PlanPreview;
  /** Core's compact form of a finished run, computed from its accumulator. */
  digest(run: RunRecord): SummaryDigest;
  /** Core's paired comparison, B minus A life by life. Both runs must share a seed and a number of lives. */
  compare(a: RunRecord, b: RunRecord): Promise<Comparison>;
}

export interface ChangeEntry {
  op: PatchOp['op'];
  path: string;
  /** Where the edit lands, in words: "Incomes / Alex's job / salary". */
  label: string;
  before: unknown;
  after: unknown;
}

export interface Assumption {
  field: string;
  value: string;
  basis: 'stated' | 'inferred' | 'default';
  confidence: 'high' | 'medium' | 'low';
}

export interface Proposal {
  summary: string;
  ops: PatchOp[];
  entries: ChangeEntry[];
  assumptions: Assumption[];
  /** Validation findings the change would introduce. */
  newWarnings: string[];
}

export type Decision = { type: 'accept' } | { type: 'accept_edited'; ops: PatchOp[] } | { type: 'decline'; reason?: string };

export type ProposalState = 'pending' | 'accepted' | 'edited' | 'declined' | 'auto' | 'failed' | 'cancelled';

export type ToolDisplay =
  | { type: 'change'; proposal: Proposal; state: ProposalState }
  | { type: 'chart'; kind: ChartKind; caption: string; runId: string | null; compareRunId: string | null; dataId: string | null };

export interface ToolView {
  /** One line for the activity row, in the past tense: "Simulated 10,000 lives". */
  title: string;
  /** Facts for the "Based on" strip under an answer. */
  meta?: { paths?: number; seed?: number; paired?: boolean; runs?: number };
  display?: ToolDisplay;
}

export interface ToolOutcome {
  /** Compact JSON for the model. Never a raw Summary. */
  result: unknown;
  isError?: boolean;
  view: ToolView;
}

export interface ToolCallInfo {
  id: string;
  signal: AbortSignal;
  /** Lets a tool update its card while it waits (a proposal awaiting a decision). */
  update(view: ToolView): void;
}

export interface ToolContext {
  store: PlanStore;
  engine: EngineBridge;
  runs: RunRegistry;
  /** Resolves when the user accepts, edits or declines; rejects if the turn is stopped. */
  requestApproval(callId: string, proposal: Proposal, signal: AbortSignal): Promise<Decision>;
  autoAccept(): boolean;
  cities(): Promise<City[]>;
  today(): string;
}

export interface ToolSpec<I = never> {
  name: string;
  description: string;
  input_schema: JsonSchema & { type: 'object' };
  /** The activity row while the tool runs: "Simulating 10,000 lives". */
  pending(input: I): string;
  run(input: I, ctx: ToolContext, call: ToolCallInfo): Promise<ToolOutcome>;
}

/** A failure the model should read and act on. */
export class ToolError extends Error {
  readonly title: string;
  constructor(message: string, title = 'Could not complete this step') {
    super(message);
    this.name = 'ToolError';
    this.title = title;
  }
}

export type { PathTrace };
