// A real conversation with Claude, using the same loop, prompt, tool definitions and
// executors as the app, with the engine run in this thread. It costs a few cents and
// needs network access, so it runs only when asked for:
//
//   FIRESIGHT_LIVE_SMOKE=1 ANTHROPIC_API_KEY=sk-ant-... npx vitest run src/assistant/live.test.ts
//
// Optional: FIRESIGHT_LIVE_MODEL=claude-opus-5-5 (default: the app's default model).
import { readFileSync } from 'node:fs';
import { beforeAll, describe, expect, it } from 'vitest';
import { exampleProfiles, initEngineSync } from '@/engine/api';
import { useProfileStore } from '@/state/profile-store';
import { runTurn } from './agent/loop';
import type { ApiMessage, TurnUsage } from './agent/types';
import { DEFAULT_MODEL, isAssistantModel } from './models';
import { SYSTEM_PROMPT } from './prompt';
import { nodeEngine } from './testing/node-engine';
import { currentPlanView, executeTool, RunRegistry, toolDefinitions, type PlanStore, type ToolContext } from './tools';
import { createByokTransport } from './transport';

const key = process.env.ANTHROPIC_API_KEY;
const enabled = Boolean(key) && process.env.FIRESIGHT_LIVE_SMOKE === '1';
const requested = process.env.FIRESIGHT_LIVE_MODEL ?? '';
const model = isAssistantModel(requested) ? requested : DEFAULT_MODEL;

describe.skipIf(!enabled)('live smoke conversation', () => {
  beforeAll(() => {
    initEngineSync(readFileSync(new URL('../engine/pkg/firesight_wasm_bg.wasm', import.meta.url)));
    useProfileStore.getState().load({ meta: { id: 'live', createdAt: 0, origin: 'test' }, profile: exampleProfiles()[0]!.profile });
  });

  it(
    'answers a what-if from tool results, with a paired comparison and a warm cache',
    async () => {
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
      const ctx: ToolContext = {
        store,
        engine: nodeEngine(),
        runs: new RunRegistry(),
        requestApproval: async () => ({ type: 'accept' }),
        autoAccept: () => false,
        cities: async () => [],
        today: () => '2026-11',
      };
      const transport = createByokTransport(key as string);
      const history: ApiMessage[] = [];
      const request = { model, system: SYSTEM_PROMPT, tools: toolDefinitions() };
      const log: string[] = [];
      let usage: TurnUsage | null = null;

      const ask = async (text: string, withPlan: boolean) => {
        const answer: string[] = [];
        const result = await runTurn({
          transport,
          request,
          history,
          userContent: [...(withPlan ? [{ type: 'text' as const, text: `<plan_state>\n${JSON.stringify(currentPlanView(ctx))}\n</plan_state>` }] : []), { type: 'text' as const, text }],
          signal: AbortSignal.timeout(280_000),
          executeTool: async (call, signal) => {
            const { reply, view } = await executeTool(ctx, call, signal);
            log.push(`  tool ${call.name} ${JSON.stringify(call.input).slice(0, 160)} -> ${reply.isError ? 'ERROR ' : ''}${view.title} (${reply.content.length} chars)`);
            return reply;
          },
          events: { onText: (t) => answer.push(t), onUsage: (u) => (usage = { ...u }) },
        });
        log.push(`USER: ${text}`, `OUTCOME: ${result.outcome}${result.error ? ` ${result.error.title}: ${result.error.detail ?? result.error.message}` : ''}`, `ASSISTANT: ${answer.join('')}`, `USAGE: ${JSON.stringify(result.usage)}`, '');
        return { result, text: answer.join('') };
      };

      const first = await ask('What happens to my chances if I retire at 50 instead? Keep it short.', true);
      const second = await ask('And which lever moves the result most?', false);
      console.log(`\nLive smoke on ${model}\n${log.join('\n')}`);

      expect(first.result.outcome).toBe('complete');
      expect(first.text.length).toBeGreaterThan(40);
      // The answer was grounded: a paired scenario run happened before it.
      expect(log.some((l) => l.includes('tool try_scenario') && !l.includes('ERROR'))).toBe(true);
      expect(second.result.outcome).toBe('complete');
      expect(log.some((l) => l.includes('tool rank_levers'))).toBe(true);
      // The frozen prompt + tools prefix was read from the cache on later requests.
      expect((usage as TurnUsage | null)?.cacheRead ?? 0).toBeGreaterThan(0);
    },
    300_000,
  );
});
