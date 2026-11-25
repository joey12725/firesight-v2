// What the page knows about browser agents this session: whether the user has let one
// in, what is waiting for the user's answer, which calls are in flight and the last few
// that finished. Held in memory only; nothing here is persisted or sent anywhere.
//
// This module is in the first bundle (the top bar pill and Settings read it), so it stays
// small and imports nothing heavy. The tools themselves load on demand (see register.ts).
import { create } from 'zustand';
import type { Decision, Proposal } from '@/assistant/tools/types';

/** The persisted setting. `ask`: prompt once per page session. `allow`: no prompt. `off`: tools refuse. */
export type AgentAccess = 'ask' | 'allow' | 'off';

/** The user's answer for this page session, when the setting is `ask`. */
export type SessionConsent = 'undecided' | 'granted' | 'denied';

export type ActivityOutcome = 'running' | 'ok' | 'error' | 'refused' | 'cancelled';

/** One tool call, for the activity log. Never holds inputs, results or plan data. */
export interface ActivityEntry {
  id: number;
  tool: string;
  /** Epoch milliseconds when the call arrived. */
  at: number;
  /** How long it took; null while it runs. */
  ms: number | null;
  outcome: ActivityOutcome;
  /** The error code, when there was one. */
  code?: string;
}

/** Something an agent asked to do that the user must accept or decline. */
export type ApprovalRequest =
  | { kind: 'change'; tool: string; proposal: Proposal }
  | { kind: 'confirm'; tool: string; title: string; body: string; details?: string[]; confirmLabel: string };

export type PendingApproval = ApprovalRequest & { id: string };

export type ConsentAnswer = 'granted' | 'denied' | 'timeout';

interface AgentState {
  /** True once this page has registered its tools with the browser. */
  available: boolean;
  toolCount: number;
  /** Why registration failed, when the browser offered WebMCP and refused the tools. */
  registrationError: string | null;
  consent: SessionConsent;
  /** The consent prompt is on screen. */
  consentAsked: boolean;
  approvals: PendingApproval[];
  inFlight: number;
  log: ActivityEntry[];
  /** Set by the accounts code when the user is signed in; reported by `get_app_state`. */
  signedIn: boolean;
}

export const MAX_LOG = 50;

const initial: AgentState = {
  available: false,
  toolCount: 0,
  registrationError: null,
  consent: 'undecided',
  consentAsked: false,
  approvals: [],
  inFlight: 0,
  log: [],
  signedIn: false,
};

export const useAgent = create<AgentState>(() => initial);

const aborted = () => new DOMException('Aborted', 'AbortError');

// ---- consent ------------------------------------------------------------------------

const consentWaiters = new Set<(answer: ConsentAnswer) => void>();

/**
 * Shows the consent prompt and waits for the user. Every call waiting at the same time
 * gets the same answer. Rejects with an AbortError if `signal` aborts. On a timeout the
 * prompt stays on screen: the user's answer, when it comes, covers the calls that follow.
 */
export function askConsent(signal: AbortSignal | undefined, timeoutMs: number): Promise<ConsentAnswer> {
  const decided = useAgent.getState().consent;
  if (decided !== 'undecided') return Promise.resolve(decided);
  if (signal?.aborted) return Promise.reject(aborted());
  return new Promise<ConsentAnswer>((resolve, reject) => {
    const settle = (answer: ConsentAnswer) => {
      clearTimeout(timer);
      signal?.removeEventListener('abort', onAbort);
      consentWaiters.delete(settle);
      resolve(answer);
    };
    const onAbort = () => {
      clearTimeout(timer);
      consentWaiters.delete(settle);
      reject(aborted());
    };
    const timer = setTimeout(() => settle('timeout'), timeoutMs);
    signal?.addEventListener('abort', onAbort, { once: true });
    consentWaiters.add(settle);
    useAgent.setState({ consentAsked: true });
  });
}

/** The user's answer to the consent prompt. It holds until the page is reloaded or the setting changes. */
export function answerConsent(granted: boolean) {
  const answer = granted ? 'granted' : 'denied';
  useAgent.setState({ consent: answer, consentAsked: false });
  for (const settle of [...consentWaiters]) settle(answer);
}

/** Back to "not asked yet": used when the user changes the agent access setting. */
export function resetConsent() {
  useAgent.setState({ consent: 'undecided', consentAsked: false });
  for (const settle of [...consentWaiters]) settle('denied');
}

// ---- approvals ----------------------------------------------------------------------

/** `timeout`: nobody answered in time. `withdrawn`: the page took the card down (see `withdrawApprovals`). */
export type ApprovalAnswer = Decision | 'timeout' | 'withdrawn';

const approvalWaiters = new Map<string, (answer: ApprovalAnswer) => void>();
let nextApproval = 1;

/**
 * Puts a change card (or a plain confirmation) in front of the user and waits. Resolves
 * `'timeout'` and withdraws the card if nobody answers in time, so a stale proposal can
 * never be accepted later. Rejects with an AbortError if `signal` aborts.
 */
export function askApproval(request: ApprovalRequest, signal: AbortSignal | undefined, timeoutMs: number): Promise<ApprovalAnswer> {
  if (signal?.aborted) return Promise.reject(aborted());
  const id = `approval_${nextApproval++}`;
  return new Promise<ApprovalAnswer>((resolve, reject) => {
    const withdraw = () => {
      clearTimeout(timer);
      signal?.removeEventListener('abort', onAbort);
      approvalWaiters.delete(id);
      useAgent.setState((s) => ({ approvals: s.approvals.filter((a) => a.id !== id) }));
    };
    const onAbort = () => {
      withdraw();
      reject(aborted());
    };
    const timer = setTimeout(() => {
      withdraw();
      resolve('timeout');
    }, timeoutMs);
    signal?.addEventListener('abort', onAbort, { once: true });
    approvalWaiters.set(id, (decision) => {
      withdraw();
      resolve(decision);
    });
    useAgent.setState((s) => ({ approvals: [...s.approvals, { ...request, id }] }));
  });
}

/** The user's answer to one pending approval. */
export function answerApproval(id: string, decision: Decision) {
  approvalWaiters.get(id)?.(decision);
}

/**
 * Takes every pending card down unanswered: each waiting call resolves `'withdrawn'` and
 * writes nothing. Used when what a card was raised for no longer holds: another plan was
 * opened, or the user switched agent access off.
 */
export function withdrawApprovals() {
  for (const settle of [...approvalWaiters.values()]) settle('withdrawn');
}

// ---- activity -----------------------------------------------------------------------

let nextActivity = 1;

export function activityStart(tool: string, now = Date.now()): number {
  const id = nextActivity++;
  useAgent.setState((s) => ({ inFlight: s.inFlight + 1, log: [...s.log, { id, tool, at: now, ms: null, outcome: 'running' as const }].slice(-MAX_LOG) }));
  return id;
}

export function activityEnd(id: number, outcome: Exclude<ActivityOutcome, 'running'>, code?: string, now = Date.now()) {
  useAgent.setState((s) => ({
    inFlight: Math.max(0, s.inFlight - 1),
    log: s.log.map((entry) => (entry.id === id ? { ...entry, ms: Math.max(0, now - entry.at), outcome, ...(code ? { code } : {}) } : entry)),
  }));
}

/** For tests: forget everything, including calls still waiting. */
export function resetAgentState() {
  for (const settle of [...consentWaiters]) settle('denied');
  for (const settle of [...approvalWaiters.values()]) settle({ type: 'decline' });
  useAgent.setState(initial);
}
