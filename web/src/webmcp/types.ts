// What an in-page tool is, and what it is given to work with. As with the assistant's
// tools, everything a tool touches comes through its context, so each one runs unchanged
// against the real app in the browser and against fakes in node tests.
import type { JsonSchema } from '@/assistant/agent/validate-input';
import type { EngineBridge, PlanStore, RunRegistry } from '@/assistant/tools';
import type { City } from '@/editor/cities';
import type { Issue, Profile } from '@/engine/types';
import type { EditResult, Scenario } from '@/state/profile-store';
import type { RunState } from '@/state/run-store';
import type { ActivityOutcome, AgentAccess, ApprovalAnswer, ApprovalRequest, ConsentAnswer, SessionConsent } from './agent-store';
import type { ChartEntry } from './chart-registry';

export type Json = Record<string, unknown>;

/** Every tool resolves one of these: a plain JSON object, never a thrown error. */
export type ToolResult = ({ ok: true } & Json) | ({ ok: false; error: { code: string; message: string } } & Json);

/** WebMCP annotations. All three are always stated. */
export interface ToolAnnotations {
  /** The tool changes nothing: not the plan, not what the user is looking at. */
  readOnlyHint: boolean;
  /** The tool changes the user's plan or replaces what is open. */
  consequentialHint: boolean;
  /** The result can carry text a person typed (names, notes): data, not instructions. */
  untrustedContentHint: boolean;
}

export interface ToolCall {
  /** Always present: a signal that never aborts stands in when the browser passes none. */
  signal: AbortSignal;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any -- each tool is typed at its definition; the registry is heterogeneous
export interface AgentTool<I = any> {
  /** 1 to 128 characters of [A-Za-z0-9_.-]. */
  name: string;
  title: string;
  /** Written for an agent that has no system prompt: what it does, when to call it, units, what comes back. */
  description: string;
  inputSchema: JsonSchema & { type: 'object' };
  annotations: ToolAnnotations;
  /**
   * `required`: the tool returns plan data or acts on the app, so it waits for the user
   * to let agents in. `none`: it answers regardless (state, guide, schema).
   */
  consent: 'required' | 'none';
  /** Answers even when agent access is switched off. Only `get_app_state`. */
  worksWhenOff?: boolean;
  /** False for the one tool that must answer while the app is still starting. */
  awaitReady?: boolean;
  run(input: I, ctx: AgentContext, call: ToolCall): Promise<Json>;
}

/** A failure the calling agent should read and act on. Becomes `{ ok: false, error: { code, message }, ...extra }`. */
export class AgentError extends Error {
  readonly code: string;
  readonly extra: Json;
  constructor(code: string, message: string, extra: Json = {}) {
    super(message);
    this.name = 'AgentError';
    this.code = code;
    this.extra = extra;
  }
}

/** The plan as the agent tools use it: the assistant's view of it, plus scenarios and undo. */
export interface AgentPlanStore extends PlanStore {
  setScenarioEnabled(id: string, enabled: boolean): EditResult;
  /** False when there is no such scenario. */
  removeScenario(id: string): boolean;
  /** Undoes the latest change only if it is one this agent made. */
  undoAgentEdit(): { ok: true; label: string } | { ok: false; reason: 'nothing_to_undo' | 'not_an_agent_edit'; latest?: string };
}

export interface OpenPlan {
  id: string;
  name: string;
  errors: number;
  warnings: number;
  /** The session has edits that undo could step through, or changes not yet saved. */
  hasSessionHistory: boolean;
}

export interface AppBridge {
  plan(): OpenPlan | null;
  store: AgentPlanStore;
  /** Plans saved in this browser: id and name only. */
  savedPlans(): { id: string; name: string }[];
  demos(): { slug: string; title: string; summary: string; profile: Profile }[];
  signedIn(): boolean;
  views(): { id: string; label: string }[];
  activeView(): string;
  /** Navigates the visible app and returns the address now shown (path and query). */
  showView(id: string, lifePath?: number): string;
  /** The app's own run: the one the user's charts are drawn from. */
  run(): RunState;
  onRun(listener: () => void): () => void;
  /** The charts of the visible view that are drawn now. */
  charts(): ChartEntry[];
  /** True while the view that was asked for is still loading behind the previous one. */
  chartsPending(): boolean;
  /** Completes a full or partial profile with defaults and validates it. Throws an `EngineError` when it is not a profile at all. */
  normalise(profile: unknown): { profile: Profile; errors: Issue[]; warnings: Issue[] };
  /** Saves the open plan, then opens `profile` as a new plan saved in this browser. Returns its id. */
  openNew(input: { profile: Profile; scenarios?: Scenario[]; origin: string }): Promise<string>;
  /** Saves the open plan, then opens a saved one. False when it is not there. */
  openSaved(id: string): Promise<boolean>;
  cities(): Promise<City[]>;
  /** "YYYY-MM". */
  today(): string;
  /** A toast for the user, for things an agent did without a card. `undo` offers the plan's undo. */
  notify(message: { title: string; description?: string; undo?: boolean }): void;
}

export interface Limits {
  /** How long a call waits for the user to answer the consent prompt. */
  consentMs: number;
  /** How long a call waits for the user to accept or decline a change. */
  approvalMs: number;
  /** How long `get_app_state` waits for the app to finish starting before saying it has not. */
  startupMs: number;
  /** How long chart tools wait for a view that is still loading to draw. */
  chartsMs: number;
  /** How long they wait when nothing says a view is loading: a view can simply have no charts. */
  chartsGraceMs: number;
  /** Results longer than this (as JSON) are refused. */
  maxResultChars: number;
}

export const DEFAULT_LIMITS: Limits = { consentMs: 120_000, approvalMs: 300_000, startupMs: 5_000, chartsMs: 8_000, chartsGraceMs: 1_500, maxResultChars: 16_000 };

export interface AgentContext {
  /** Resolves when the app has loaded the engine and restored the last plan; rejects if the engine failed to load. */
  ready(): Promise<void>;
  settings(): { access: AgentAccess; autoApply: boolean };
  consent(): SessionConsent;
  askConsent(signal: AbortSignal): Promise<ConsentAnswer>;
  askApproval(request: ApprovalRequest, signal: AbortSignal): Promise<ApprovalAnswer>;
  activity: { start(tool: string): number; end(id: number, outcome: Exclude<ActivityOutcome, 'running'>, code?: string): void };
  limits: Limits;
  app: AppBridge;
  engine: EngineBridge;
  /** This agent surface's own runs, apart from the in-app assistant's. Cleared when the open plan changes. */
  runs: RunRegistry;
}
