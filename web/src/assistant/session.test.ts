// The whole assistant below the UI: a session driving the real agent loop, tools and
// profile store against a scripted model and a fake simulation pool.
import { readFileSync } from 'node:fs';
import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { applyPatch, exampleProfiles, initEngineSync, levers, schema } from '@/engine/api';
import { useProfileStore } from '@/state/profile-store';
import type { ApiMessage } from './agent/types';
import { settle, type AssistantItem, type ConversationStorage, type StoredConversation, type ToolPart, type UserItem } from './conversation';
import { promptFingerprint } from './prompt';
import { AssistantSession, useAssistant, type SessionDeps } from './session';
import { fakePool } from './testing/fake-engine';
import { scriptedTransport, type FakeTransport, type Scripted } from './testing/fake-model';
import { toolDefinitions, type PlanStore } from './tools';
import { HOSTED_OFF } from './transport';

beforeAll(() => {
  initEngineSync(readFileSync(new URL('../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
});

const plan = () => useProfileStore.getState();
const store: PlanStore = {
  profile: () => plan().profile,
  report: () => plan().report,
  scenarios: () => plan().scenarios,
  run: () => plan().run,
  preview: (ops) => plan().preview(ops),
  edit: (ops, label) => plan().edit(ops, { label, target: 'base' }),
  addScenario: (input) => plan().addScenario(input),
};

function memoryStorage(): ConversationStorage & { saved: Map<string, StoredConversation> } {
  const saved = new Map<string, StoredConversation>();
  return {
    saved,
    load: async (id) => structuredClone(saved.get(id)),
    save: async (id, c) => void saved.set(id, structuredClone(c)),
    remove: async (id) => void saved.delete(id),
  };
}

let transport: FakeTransport | null;
let storage: ReturnType<typeof memoryStorage>;
let settings: { apiKey: string; model: 'claude-opus-5-5'; accessCode: string; autoAccept: boolean };

function session(script: Scripted[] | null, overrides: Partial<SessionDeps> = {}): AssistantSession {
  transport = script ? scriptedTransport(script) : null;
  const pool = fakePool();
  return new AssistantSession({
    store,
    engine: () => ({ pool, applyPatch, levers, schema }),
    storage,
    settings: () => settings,
    hosted: async () => HOSTED_OFF,
    transport: () => (transport ? { transport, model: 'claude-opus-5-5' } : null),
    cities: async () => [],
    today: () => '2026-11',
    schedule: (flush) => flush(),
    ...overrides,
  });
}

const items = () => useAssistant.getState().items;
const lastAssistant = () => items().findLast((i): i is AssistantItem => i.kind === 'assistant')!;
const texts = (message: ApiMessage | undefined) => (Array.isArray(message?.content) ? message.content.flatMap((b) => (b.type === 'text' ? [b.text] : [])) : []);
const salaryPath = () => `/incomes/@${plan().profile!.incomes[0]!.id}/kind/salary`;

beforeEach(() => {
  plan().load({ meta: { id: 'home', createdAt: 0, origin: 'test' }, profile: exampleProfiles()[0]!.profile });
  storage = memoryStorage();
  settings = { apiKey: 'sk-ant-test', model: 'claude-opus-5-5', accessCode: '', autoAccept: false };
});

describe('a conversation', () => {
  it('answers from a simulation, shows what was run, and offers follow-ups', async () => {
    const s = session([
      { thinking: 'Simulate first.', tools: [{ id: 'r1', name: 'run_simulation', input: { paths: 2000 } }] },
      { text: 'The plan holds in most simulated lives.\n\n<followups>\n- What if I retire at 55?\n- What moves the result most?\n</followups>' },
    ]);
    await s.load('home');
    await s.send({ text: 'How does my plan look?' });

    const [user, answer] = items() as [UserItem, AssistantItem];
    expect(user).toMatchObject({ kind: 'user', text: 'How does my plan look?', apiIndex: 0 });
    expect(answer.status).toBe('done');
    expect(answer.followups).toEqual(['What if I retire at 55?', 'What moves the result most?']);
    const tool = answer.parts.find((p): p is ToolPart => p.type === 'tool')!;
    expect(tool).toMatchObject({ name: 'run_simulation', status: 'done', title: 'Simulated 2,000 lives of the current plan', meta: { paths: 2000, seed: 1, runs: 1 } });
    expect(JSON.parse(tool.result!)).toMatchObject({ paths: 2000, seed: 1 });
    // The follow-up block is never shown as text.
    expect(answer.parts.at(-1)).toEqual({ type: 'text', text: 'The plan holds in most simulated lives.' });
    expect(useAssistant.getState().busy).toBe(false);

    // The model saw the plan with the question, and its tool result on the next request.
    const first = texts(transport!.requests[0]!.messages[0]);
    expect(first[0]).toMatch(/^<plan_state>/);
    expect(first[0]).toContain('"annual_budget_now"');
    expect(first.at(-1)).toBe('How does my plan look?');
    expect(transport!.requests[0]!.system).toContain('FIRESight');
    expect(transport!.requests[0]!.tools.map((t) => t.name)).toContain('try_scenario');
    expect(JSON.stringify(transport!.requests[1]!.messages.at(-1))).toContain('"tool_result"');
  });

  it('attaches the plan again only after it has changed', async () => {
    const s = session([{ text: 'One.' }, { text: 'Two.' }, { text: 'Three.' }]);
    await s.load('home');
    await s.send({ text: 'First' });
    await s.send({ text: 'Second' });
    plan().edit([{ op: 'set', path: salaryPath(), value: 140_000 }]);
    await s.send({ text: 'Third', context: { view: 'outcomes', depletion_rate: 0.09 } });
    const sent = transport!.requests.map((r) => texts(r.messages.at(-1)));
    expect(sent[0]!.some((t) => t.startsWith('<plan_state>'))).toBe(true);
    expect(sent[1]).toEqual(['Second']);
    expect(sent[2]![0]).toMatch(/^<plan_state>/);
    expect(sent[2]![0]).toContain('140000');
    expect(sent[2]![1]).toBe('<view_context>\n{"view":"outcomes","depletion_rate":0.09}\n</view_context>');
    // The system prompt and tools are byte-identical on every request: the cached prefix.
    expect(new Set(transport!.requests.map((r) => r.system + JSON.stringify(r.tools))).size).toBe(1);
    // Earlier turns are never rewritten.
    expect(transport!.requests[2]!.messages.slice(0, 2)).toEqual(transport!.requests[0]!.messages.concat(transport!.requests[1]!.messages.slice(1, 2)));
  });

  it('waits for the user to accept a proposed change, then tells the model', async () => {
    const s = session([
      { tools: [{ id: 'p1', name: 'propose_changes', input: { summary: 'Set the salary to 150,000', ops: [{ op: 'set', path: salaryPath(), value: 150_000 }], assumptions: [{ field: 'Salary', value: '$150,000', basis: 'stated', confidence: 'high' }] } }] },
      { text: 'Done: your salary is updated.' },
    ]);
    await s.load('home');
    const turn = s.send({ text: 'I got a raise to 150k' });
    await new Promise((r) => setTimeout(r, 10));
    const pending = lastAssistant().parts.find((p): p is ToolPart => p.type === 'tool')!;
    expect(pending.display).toMatchObject({ type: 'change', state: 'pending' });
    expect(useAssistant.getState().busy).toBe(true);
    expect((plan().profile!.incomes[0]!.kind as { salary: number }).salary).not.toBe(150_000);

    s.decide('p1', { type: 'accept' });
    await turn;
    expect((plan().profile!.incomes[0]!.kind as { salary: number }).salary).toBe(150_000);
    expect(lastAssistant().parts.find((p): p is ToolPart => p.type === 'tool')!.display).toMatchObject({ type: 'change', state: 'accepted' });
    expect(JSON.stringify(transport!.requests[1]!.messages.at(-1))).toContain('\\"status\\":\\"accepted\\"');
    plan().undo();
    expect((plan().profile!.incomes[0]!.kind as { salary: number }).salary).not.toBe(150_000);
  });

  it('stops cleanly while a change card is waiting', async () => {
    const s = session([{ tools: [{ id: 'p1', name: 'propose_changes', input: { summary: 'Change', ops: [{ op: 'set', path: salaryPath(), value: 1 }] } }] }]);
    await s.load('home');
    const turn = s.send({ text: 'Change it' });
    await new Promise((r) => setTimeout(r, 10));
    s.stop();
    await turn;
    const answer = lastAssistant();
    expect(answer.status).toBe('stopped');
    expect(answer.parts.find((p): p is ToolPart => p.type === 'tool')).toMatchObject({ status: 'cancelled', display: { state: 'cancelled' } });
    // The saved transcript can be continued: the cancelled call has its result.
    const saved = storage.saved.get('home')!;
    expect(saved.history.at(-1)!.role).toBe('user');
    expect(JSON.stringify(saved.history.at(-1))).toContain('Cancelled by the user');
  });

  it('saves per household and restores a conversation that can continue', async () => {
    const s = session([{ thinking: 'hm', text: 'First answer.' }]);
    await s.load('home');
    await s.send({ text: 'Hello' });
    expect(storage.saved.get('home')!.items).toHaveLength(2);
    expect(storage.saved.get('home')!.fingerprint).toBe(promptFingerprint(toolDefinitions()));

    const again = session([{ text: 'Second answer.' }]);
    await again.load('other');
    expect(items()).toEqual([]);
    await again.load('home');
    expect(items().map((i) => i.kind)).toEqual(['user', 'assistant']);
    await again.send({ text: 'And then?' });
    expect(transport!.requests[0]!.messages.map((m) => m.role)).toEqual(['user', 'assistant', 'user']);
    // Same prompt and tools: the reasoning block is replayed untouched.
    expect(JSON.stringify(transport!.requests[0]!.messages[1])).toContain('"thinking"');
  });

  it('drops saved reasoning when the prompt or tools have changed since', async () => {
    const stored: StoredConversation = {
      version: 1,
      items: [],
      history: [
        { role: 'user', content: [{ type: 'text', text: 'Old question' }] },
        { role: 'assistant', content: [{ type: 'thinking', thinking: 'old', signature: 's' }, { type: 'text', text: 'Old answer' }] },
      ],
      fingerprint: 'an-older-prompt',
      lastPrompt: 0,
      planKey: null,
      pendingSummary: null,
    };
    storage.saved.set('home', stored);
    const s = session([{ text: 'Fresh.' }]);
    await s.load('home');
    await s.send({ text: 'Continue' });
    expect(JSON.stringify(transport!.requests[0]!.messages)).not.toContain('"thinking"');
    expect(transport!.requests[0]!.messages).toHaveLength(3);
  });

  it('answers again from the same message, cutting only the end of the transcript', async () => {
    const s = session([{ text: 'Before.' }, { text: 'First try.' }, { text: 'Second try.' }]);
    await s.load('home');
    await s.send({ text: 'Earlier' });
    await s.send({ text: 'Which is better?' });
    await s.regenerate();
    expect(items().map((i) => i.kind)).toEqual(['user', 'assistant', 'user', 'assistant']);
    expect(lastAssistant().parts).toEqual([{ type: 'text', text: 'Second try.' }]);
    const regenerated = transport!.requests[2]!.messages;
    expect(regenerated).toHaveLength(3);
    expect(regenerated.slice(0, 2)).toEqual(transport!.requests[1]!.messages.slice(0, 2));
    expect(texts(regenerated[2]).at(-1)).toBe('Which is better?');
  });

  it('condenses a long conversation into a summary between turns', async () => {
    const s = session([
      { text: 'A short answer.' },
      { text: 'A long answer.', promptTokens: 400_000 },
      { text: 'Summary: the user is Maya; the plan holds in 91 of 100 lives (10,000 lives, seed 1).' },
      { text: 'Continuing from the summary.' },
    ]);
    await s.load('home');
    await s.send({ text: 'First question' });
    await s.send({ text: 'Second question' });
    await s.send({ text: 'Third question' });
    // The second answer reported a prompt past the limit, so the third turn began with the summary request.
    expect(transport!.requests).toHaveLength(4);
    expect(texts(transport!.requests[2]!.messages.at(-1))[0]).toMatch(/will continue from a summary/);
    const kinds = items().map((i) => i.kind);
    expect(kinds.filter((k) => k === 'note')).toHaveLength(1);
    const final = transport!.requests.at(-1)!.messages;
    expect(final).toHaveLength(1);
    const blocks = texts(final[0]);
    expect(blocks[0]).toMatch(/^<conversation_summary>/);
    expect(blocks[0]).toContain('91 of 100 lives');
    expect(blocks.some((t) => t.startsWith('<plan_state>'))).toBe(true);
    expect(blocks.at(-1)).toBe('Third question');
    expect(lastAssistant().parts).toEqual([{ type: 'text', text: 'Continuing from the summary.' }]);
  });

  it('explains what to do when there is no way to reach the model', async () => {
    const s = session(null);
    await s.load('home');
    await s.send({ text: 'Hello?' });
    expect(useAssistant.getState().notice).toMatchObject({ kind: 'unavailable' });
    expect(useAssistant.getState().busy).toBe(false);
    // The question is kept, and can be sent again once connected.
    expect(items()).toEqual([expect.objectContaining({ kind: 'user', text: 'Hello?' })]);
  });

  it('shows an API failure on the answer and recovers on retry', async () => {
    const { apiError } = await import('./testing/fake-model');
    const s = session([{ error: apiError(401, 'authentication_error', 'invalid x-api-key') }, { text: 'Now it works.' }]);
    await s.load('home');
    await s.send({ text: 'Hi' });
    expect(lastAssistant()).toMatchObject({ status: 'error', error: { kind: 'auth' } });
    await s.regenerate();
    expect(lastAssistant()).toMatchObject({ status: 'done', parts: [{ type: 'text', text: 'Now it works.' }] });
    expect(transport!.requests[1]!.messages).toHaveLength(1);
  });

  it('clears the conversation without touching the plan', async () => {
    const s = session([{ text: 'Hello.' }]);
    await s.load('home');
    const before = plan().profile;
    await s.send({ text: 'Hi' });
    await s.clear();
    expect(items()).toEqual([]);
    expect(storage.saved.has('home')).toBe(false);
    expect(plan().profile).toBe(before);
  });
});

describe('settle', () => {
  it('closes whatever was in flight when the page went away', () => {
    const stored: StoredConversation = {
      version: 1,
      items: [
        { kind: 'user', id: 'u', text: 'q', attachments: [], apiIndex: 0 },
        {
          kind: 'assistant',
          id: 'a',
          status: 'streaming',
          followups: [],
          model: 'claude-opus-5-5',
          mode: 'byok',
          parts: [{ type: 'tool', id: 't', name: 'propose_changes', input: {}, status: 'running', title: 'x', display: { type: 'change', state: 'pending', proposal: { summary: 's', ops: [], entries: [], assumptions: [], newWarnings: [] } } }],
        },
      ],
      history: [
        { role: 'user', content: [{ type: 'text', text: 'q' }] },
        { role: 'assistant', content: [{ type: 'tool_use', id: 't', name: 'propose_changes', input: {} }] },
      ],
      fingerprint: 'f',
      lastPrompt: 0,
      planKey: null,
      pendingSummary: null,
    };
    const settled = settle(stored);
    expect(settled.items[1]).toMatchObject({ status: 'stopped', parts: [{ status: 'cancelled', display: { state: 'cancelled' } }] });
    expect(settled.history).toHaveLength(1);
  });
});
