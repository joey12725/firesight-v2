// The profile store as the agent tools use it. Every write goes to the baseline plan as
// one undo step labelled "Agent: …", and the store remembers which history entries are
// the agent's own, so `undo_agent_edit` can never step back over something the user did.
import { useProfileStore } from '@/state/profile-store';
import type { AgentPlanStore } from './types';

export function agentPlanStore(): AgentPlanStore {
  const state = () => useProfileStore.getState();
  /** History entries this surface created, by identity. Entries leave with the plan they belong to. */
  const mine = new WeakSet<object>();
  /** Runs a store write and claims the history entry it pushed, if it pushed one. */
  const claim = <T>(write: () => T): T => {
    const before = state().past.at(-1);
    const result = write();
    const after = state().past.at(-1);
    if (after && after !== before) mine.add(after);
    return result;
  };
  return {
    profile: () => state().profile,
    report: () => state().report,
    scenarios: () => state().scenarios,
    run: () => state().run,
    preview: (ops) => state().preview(ops),
    edit: (ops, label) => claim(() => state().edit(ops, { label: `Agent: ${label}`, target: 'base' })),
    addScenario: (input) => claim(() => state().addScenario(input)),
    setScenarioEnabled: (id, enabled) => claim(() => state().updateScenario(id, { enabled })),
    removeScenario: (id) => {
      if (!state().scenarios.some((s) => s.id === id)) return false;
      claim(() => state().removeScenario(id));
      return true;
    },
    undoAgentEdit: () => {
      const latest = state().past.at(-1);
      if (!latest) return { ok: false, reason: 'nothing_to_undo' };
      if (!mine.has(latest)) return { ok: false, reason: 'not_an_agent_edit', latest: latest.label };
      state().undo();
      return { ok: true, label: latest.label };
    },
  };
}
