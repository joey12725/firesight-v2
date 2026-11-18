// The assistant's session: the conversation, the turn in flight, and everything a turn
// needs (transport, tools, approvals). The UI reads `useAssistant` and calls the
// functions exported at the bottom; nothing here imports React.
import { create } from 'zustand';
import { loadCities } from '@/editor/cities';
import type { Comparison } from '@/engine/output-types';
import { useProfileStore } from '@/state/profile-store';
import { useSettings, type AssistantModel } from '@/state/settings-store';
import { currentYearMonth } from '@/ui/format';
import { needsCompaction, summariseHistory } from './agent/context';
import { runTurn, stripThinking } from './agent/loop';
import type { ApiContent, ApiMessage, AssistantError, ToolCall, ToolReply, TurnResult } from './agent/types';
import { attachmentBlocks, type PreparedAttachment } from './attachments';
import {
  browserStorage,
  settle,
  trimDangling,
  type AssistantItem,
  type ChatItem,
  type ConversationStorage,
  type Part,
  type StoredConversation,
  type ToolPart,
  type UserItem,
} from './conversation';
import { splitFollowups } from './followups';
import { promptFingerprint, SYSTEM_PROMPT } from './prompt';
import { realEngine } from './tools/engine-bridge';
import { currentPlanView, executeTool, pendingTitle, RunRegistry, toolDefinitions, type Decision, type EngineBridge, type PlanStore, type Proposal, type ToolContext, type ToolView } from './tools';
import { chooseTransport, detectHosted, HOSTED_OFF, type HostedInfo, type TransportChoice } from './transport';

export interface SendInput {
  text: string;
  attachments?: PreparedAttachment[];
  /** Structured context from another part of the app (a chart's "Explain this"). */
  context?: unknown;
}

export interface AssistantState {
  /** The household whose conversation is loaded; null before the first load. */
  householdId: string | null;
  items: ChatItem[];
  busy: boolean;
  /** A transient line under the conversation: retrying, condensing. */
  status: string | null;
  hosted: HostedInfo;
  hostedChecked: boolean;
  /** A problem that is not part of a turn: no way to reach the model, no plan open. */
  notice: AssistantError | null;
  /** Prompts waiting for the current turn to finish. */
  queued: number;
}

export const useAssistant = create<AssistantState>(() => ({
  householdId: null,
  items: [],
  busy: false,
  status: null,
  hosted: HOSTED_OFF,
  hostedChecked: false,
  notice: null,
  queued: 0,
}));

export interface SessionDeps {
  store: PlanStore;
  engine: () => EngineBridge;
  storage: ConversationStorage;
  settings(): { apiKey: string; model: AssistantModel; accessCode: string; autoAccept: boolean };
  hosted(): Promise<HostedInfo>;
  /** Overridden in tests to script the model. */
  transport(settings: { apiKey: string; model: AssistantModel; accessCode: string }, hosted: HostedInfo): TransportChoice | null;
  cities: ToolContext['cities'];
  today(): string;
  /** Batches UI updates while text streams. */
  schedule(flush: () => void): void;
}

const uid = () => `${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;

function hashText(text: string): string {
  let h = 2166136261;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return (h >>> 0).toString(36);
}

const PLAN_OPEN = '<plan_state>';
const DEFAULT_ATTACHMENT_TEXT = 'Please read what I attached and update the plan from it.';

export class AssistantSession {
  private readonly deps: SessionDeps;
  private readonly tools = toolDefinitions();
  private readonly fingerprint = promptFingerprint(this.tools);
  readonly runs = new RunRegistry();
  private history: ApiMessage[] = [];
  private lastPrompt = 0;
  private planKey: string | null = null;
  private pendingSummary: string | null = null;
  private controller: AbortController | null = null;
  private readonly approvals = new Map<string, (decision: Decision) => void>();
  private readonly queue: SendInput[] = [];
  /** What the user sent in each turn (without the plan state), for regenerate. */
  private readonly sent = new Map<string, { blocks: ApiContent[]; context?: unknown; text: string }>();
  private loading: Promise<void> | null = null;
  private loadedFor: string | null = null;

  constructor(deps: SessionDeps) {
    this.deps = deps;
  }

  /**
   * Core's paired comparison of two runs still in memory (B minus A), for a chart. The
   * one `try_scenario` computed is reused. Null when either run is gone or it fails.
   */
  async comparison(runId: string, compareRunId: string): Promise<Comparison | null> {
    const a = this.runs.get(runId);
    const b = this.runs.get(compareRunId);
    if (!a || !b) return null;
    try {
      return await this.runs.compare(this.deps.engine(), a, b);
    } catch {
      return null;
    }
  }

  // ---- loading and saving -----------------------------------------------------------

  /** Loads the conversation of a household (or clears the panel when there is none). */
  load(householdId: string | null): Promise<void> {
    if (householdId === this.loadedFor && this.loading) return this.loading;
    this.stop();
    this.loadedFor = householdId;
    this.history = [];
    this.lastPrompt = 0;
    this.planKey = null;
    this.pendingSummary = null;
    this.runs.clear();
    this.sent.clear();
    this.queue.length = 0;
    useAssistant.setState({ householdId, items: [], busy: false, status: null, notice: null, queued: 0 });
    this.loading = (async () => {
      if (!householdId) return;
      let stored: StoredConversation | undefined;
      try {
        stored = await this.deps.storage.load(householdId);
      } catch {
        stored = undefined;
      }
      if (!stored || this.loadedFor !== householdId) return;
      const settled = settle(stored);
      this.history = settled.history;
      // Reasoning blocks are bound to the prompt and tools they were produced under.
      if (settled.fingerprint !== this.fingerprint) stripThinking(this.history);
      this.lastPrompt = settled.lastPrompt;
      this.planKey = settled.planKey;
      this.pendingSummary = settled.pendingSummary;
      useAssistant.setState({ items: settled.items });
    })();
    return this.loading;
  }

  private save() {
    const householdId = this.loadedFor;
    if (!householdId) return;
    const conversation: StoredConversation = {
      version: 1,
      items: useAssistant.getState().items,
      history: this.history,
      fingerprint: this.fingerprint,
      lastPrompt: this.lastPrompt,
      planKey: this.planKey,
      pendingSummary: this.pendingSummary,
    };
    void this.deps.storage.save(householdId, conversation).catch(() => undefined);
  }

  async clear() {
    this.stop();
    this.history = [];
    this.lastPrompt = 0;
    this.planKey = null;
    this.pendingSummary = null;
    this.runs.clear();
    this.sent.clear();
    this.queue.length = 0;
    useAssistant.setState({ items: [], status: null, notice: null, queued: 0 });
    if (this.loadedFor) await this.deps.storage.remove(this.loadedFor).catch(() => undefined);
  }

  // ---- reaching the model -----------------------------------------------------------

  async checkHosted(): Promise<HostedInfo> {
    const hosted = await this.deps.hosted();
    useAssistant.setState({ hosted, hostedChecked: true });
    return hosted;
  }

  private async choose(): Promise<TransportChoice | null> {
    const settings = this.deps.settings();
    const hosted = settings.apiKey ? useAssistant.getState().hosted : await this.checkHosted();
    return this.deps.transport(settings, hosted);
  }

  // ---- tools ------------------------------------------------------------------------

  private context(): ToolContext {
    return {
      store: this.deps.store,
      engine: this.deps.engine(),
      runs: this.runs,
      autoAccept: () => this.deps.settings().autoAccept,
      cities: this.deps.cities,
      today: this.deps.today,
      requestApproval: (callId, _proposal: Proposal, signal) =>
        new Promise<Decision>((resolve, reject) => {
          const onAbort = () => {
            this.approvals.delete(callId);
            reject(new DOMException('Aborted', 'AbortError'));
          };
          if (signal.aborted) return onAbort();
          signal.addEventListener('abort', onAbort, { once: true });
          this.approvals.set(callId, (decision) => {
            signal.removeEventListener('abort', onAbort);
            this.approvals.delete(callId);
            resolve(decision);
          });
        }),
    };
  }

  /** The user's answer to a change card. */
  decide(callId: string, decision: Decision) {
    this.approvals.get(callId)?.(decision);
  }

  // ---- a turn -----------------------------------------------------------------------

  /** Sends a message, or queues it behind the turn in flight. Resolves when its turn ends. */
  async send(input: SendInput): Promise<void> {
    const text = input.text.trim();
    const attachments = input.attachments ?? [];
    if (!text && attachments.length === 0) return;
    if (useAssistant.getState().busy) {
      this.queue.push(input);
      useAssistant.setState({ queued: this.queue.length });
      return;
    }
    if (this.loading) await this.loading;
    const body = text || DEFAULT_ATTACHMENT_TEXT;
    const blocks = attachmentBlocks(attachments);
    const user: UserItem = {
      kind: 'user',
      id: uid(),
      text: body,
      attachments: attachments.map((a) => ({ name: a.name, mediaType: a.mediaType, bytes: a.bytes })),
      apiIndex: -1,
    };
    this.sent.set(user.id, { blocks, context: input.context, text: body });
    useAssistant.setState((s) => ({ items: [...s.items, user], notice: null }));
    await this.turn(user);
  }

  /** Runs the last turn again from the same message. */
  async regenerate(): Promise<void> {
    const state = useAssistant.getState();
    if (state.busy) return;
    const items = state.items;
    const at = items.findLastIndex((i) => i.kind === 'user');
    const user = items[at] as UserItem | undefined;
    if (!user) return;
    // Cutting the transcript back to before this turn removes a trailing run, which
    // keeps every earlier reasoning block valid.
    if (user.apiIndex >= 0 && user.apiIndex <= this.history.length) {
      if (!this.sent.has(user.id)) {
        const original = this.history[user.apiIndex];
        const content = original?.role === 'user' && Array.isArray(original.content) ? original.content : [];
        const kept = content.filter((b) => b.type === 'image' || b.type === 'document');
        this.sent.set(user.id, { blocks: kept, text: user.text });
      }
      this.history.length = user.apiIndex;
    } else if (!this.sent.has(user.id)) {
      this.sent.set(user.id, { blocks: [], text: user.text });
    }
    this.planKey = null;
    useAssistant.setState({ items: items.slice(0, at + 1), notice: null });
    await this.turn(user);
  }

  stop() {
    this.controller?.abort();
  }

  private planBlock(ctx: ToolContext): ApiContent | null {
    if (!ctx.store.profile()) return null;
    let text: string;
    try {
      text = JSON.stringify(currentPlanView(ctx));
    } catch {
      return null;
    }
    const key = hashText(text);
    if (key === this.planKey) return null;
    this.planKey = key;
    return { type: 'text', text: `${PLAN_OPEN}\n${text}\n</plan_state>` };
  }

  private setItem(item: ChatItem) {
    useAssistant.setState((s) => ({ items: s.items.map((x) => (x.id === item.id ? item : x)) }));
  }

  private async turn(user: UserItem): Promise<void> {
    const controller = new AbortController();
    this.controller = controller;
    const { signal } = controller;
    useAssistant.setState({ busy: true, status: null });

    const fail = (notice: AssistantError) => {
      useAssistant.setState({ busy: false, status: null, notice });
      this.controller = null;
    };
    if (!this.deps.store.profile()) {
      return fail({ kind: 'unavailable', title: 'No plan is open', message: 'Open a plan, or start a blank one, and the assistant will work on it with you.', retryable: false });
    }
    const choice = await this.choose();
    if (!choice) {
      return fail({ kind: 'unavailable', title: 'The assistant is not connected', message: 'Add a Claude API key in Settings to use the assistant.', retryable: false });
    }
    const { transport, model } = choice;
    const request = { model, system: SYSTEM_PROMPT, tools: this.tools };
    const ctx = this.context();

    // Between turns only: if the transcript has grown too large, continue from a summary.
    if (needsCompaction(this.history, this.lastPrompt, transport.mode)) {
      useAssistant.setState({ status: 'Condensing the earlier conversation' });
      try {
        const summary = await summariseHistory(transport, request, this.history, signal);
        if (summary && summary.type === 'text') {
          this.history = [];
          this.lastPrompt = 0;
          this.planKey = null;
          this.pendingSummary = summary.text;
          useAssistant.setState((s) => ({
            items: [
              ...s.items.slice(0, -1).map((i) => (i.kind === 'user' ? { ...i, apiIndex: -1 } : i)),
              { kind: 'note', id: uid(), text: 'Earlier messages were condensed to keep the conversation within its limits. The assistant continues from its own summary of them; simulation results it needs again will be re-run.' },
              ...s.items.slice(-1),
            ],
          }));
        }
      } catch {
        // Stopped while condensing.
        useAssistant.setState({ busy: false, status: null });
        this.controller = null;
        return;
      }
      useAssistant.setState({ status: null });
    }

    const sent = this.sent.get(user.id) ?? { blocks: [], text: user.text };
    const plan = this.planBlock(ctx);
    const content: ApiContent[] = [
      ...(this.pendingSummary ? [{ type: 'text' as const, text: this.pendingSummary }] : []),
      ...sent.blocks,
      ...(plan ? [plan] : []),
      ...(sent.context !== undefined ? [{ type: 'text' as const, text: `<view_context>\n${JSON.stringify(sent.context)}\n</view_context>` }] : []),
      { type: 'text', text: sent.text },
    ];
    this.pendingSummary = null;
    const placed: UserItem = { ...user, apiIndex: this.history.length };
    this.setItem(placed);

    const draft: AssistantItem = { kind: 'assistant', id: uid(), parts: [], status: 'streaming', followups: [], model, mode: transport.mode };
    useAssistant.setState((s) => ({ items: [...s.items, draft] }));
    let dirty = false;
    const publish = () => {
      dirty = false;
      this.setItem({ ...draft, parts: draft.parts.map((p) => ({ ...p })) });
    };
    const touch = () => {
      if (dirty) return;
      dirty = true;
      this.deps.schedule(() => dirty && publish());
    };
    const toolPart = (id: string) => draft.parts.find((p): p is ToolPart => p.type === 'tool' && p.id === id);
    const appendTo = (type: 'text' | 'thinking', delta: string) => {
      const last = draft.parts[draft.parts.length - 1];
      if (last?.type === type) last.text += delta;
      else draft.parts.push({ type, text: delta });
      touch();
    };
    let stepStart = 0;

    const execute = async (call: ToolCall, toolSignal: AbortSignal): Promise<ToolReply> => {
      const part = toolPart(call.id);
      const show = (view: ToolView, status: ToolPart['status']) => {
        if (!part) return;
        part.title = view.title;
        part.meta = view.meta ?? part.meta;
        part.display = view.display ?? part.display;
        part.status = status;
        publish();
      };
      try {
        const { reply, view } = await executeTool(ctx, call, toolSignal, (view) => show(view, 'running'));
        if (part) part.result = reply.content;
        show(view, reply.isError ? 'error' : 'done');
        return reply;
      } catch (error) {
        if (part) {
          part.status = 'cancelled';
          if (part.display?.type === 'change' && part.display.state === 'pending') part.display = { ...part.display, state: 'cancelled' };
          publish();
        }
        throw error;
      }
    };

    const result: TurnResult = await runTurn({
      transport,
      request,
      history: this.history,
      userContent: content,
      signal,
      executeTool: execute,
      events: {
        onStep: () => {
          stepStart = draft.parts.length;
          if (useAssistant.getState().status) useAssistant.setState({ status: null });
        },
        onText: (delta) => appendTo('text', delta),
        onThinking: (delta) => appendTo('thinking', delta),
        onToolDraft: ({ id, name }) => {
          draft.parts.push({ type: 'tool', id, name, input: undefined, status: 'drafting', title: pendingTitle(name, {}) });
          touch();
        },
        onToolCalls: (calls) => {
          for (const call of calls) {
            let part = toolPart(call.id);
            if (!part) {
              part = { type: 'tool', id: call.id, name: call.name, input: call.input, status: 'running', title: '' };
              draft.parts.push(part);
            }
            part.input = call.input;
            part.status = 'running';
            part.title = pendingTitle(call.name, call.input);
          }
          publish();
        },
        onRetry: ({ delayMs, error }) => {
          draft.parts.length = stepStart;
          publish();
          useAssistant.setState({ status: delayMs > 0 ? `${error.title}. Trying again in ${Math.max(1, Math.round(delayMs / 1000))} s.` : 'Recovering the conversation and trying again.' });
        },
        onUsage: (usage) => {
          this.lastPrompt = usage.lastPrompt;
        },
      },
    }).catch(
      // The loop reports API and tool failures itself; this is the net under everything else,
      // so the panel can never be left busy.
      (thrown: unknown): TurnResult => {
        trimDangling(this.history);
        return {
        outcome: signal.aborted ? 'aborted' : 'error',
        error: { kind: 'unknown', title: 'Something went wrong', message: 'The assistant hit an unexpected problem. Try again.', detail: thrown instanceof Error ? thrown.message : undefined, retryable: true },
        usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, lastPrompt: this.lastPrompt, requests: 0 },
        };
      },
    );

    // Settle what the turn left behind.
    for (const part of draft.parts) {
      if (part.type === 'tool' && (part.status === 'running' || part.status === 'drafting')) part.status = 'cancelled';
    }
    const lastText = draft.parts.findLast((p): p is Extract<Part, { type: 'text' }> => p.type === 'text');
    if (lastText) {
      const split = splitFollowups(lastText.text);
      lastText.text = split.body;
      if (result.outcome === 'complete') draft.followups = split.followups;
    }
    draft.parts = draft.parts.filter((p) => p.type === 'tool' || p.text.trim().length > 0);
    draft.status = (
      { complete: 'done', aborted: 'stopped', refusal: 'refused', max_tokens: 'truncated', step_limit: 'step_limit', error: 'error' } as const
    )[result.outcome];
    draft.error = result.error;
    if (result.outcome === 'refusal' || result.outcome === 'error') this.planKey = null;
    publish();
    this.controller = null;
    useAssistant.setState({ busy: false, status: null });
    this.save();

    const next = this.queue.shift();
    useAssistant.setState({ queued: this.queue.length });
    if (next && result.outcome === 'complete') await this.send(next);
    else if (next) this.queue.length = 0;
  }
}

// ---- the app's session ----------------------------------------------------------------

const profileStore: PlanStore = {
  profile: () => useProfileStore.getState().profile,
  report: () => useProfileStore.getState().report,
  scenarios: () => useProfileStore.getState().scenarios,
  run: () => useProfileStore.getState().run,
  preview: (ops) => useProfileStore.getState().preview(ops),
  edit: (ops, label) => useProfileStore.getState().edit(ops, { label, target: 'base' }),
  addScenario: (input) => useProfileStore.getState().addScenario(input),
};

let session: AssistantSession | null = null;

export function getSession(): AssistantSession {
  if (session) return session;
  let engine: EngineBridge | null = null;
  session = new AssistantSession({
    store: profileStore,
    engine: () => (engine ??= realEngine()),
    storage: browserStorage,
    settings: () => {
      const s = useSettings.getState();
      return { apiKey: s.apiKey, model: s.model, accessCode: s.hostedAccessCode, autoAccept: s.assistantAutoAccept };
    },
    hosted: () => detectHosted(),
    transport: (settings, hosted) => chooseTransport(settings, hosted),
    cities: loadCities,
    today: () => currentYearMonth(),
    schedule: (flush) => (typeof requestAnimationFrame === 'function' ? requestAnimationFrame(flush) : setTimeout(flush, 30)),
  });
  return session;
}
