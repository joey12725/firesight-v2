// The tools wired to the real app. Loaded on demand, and only in a browser that offers
// WebMCP (see register.ts): the tool definitions, the guide text and the assistant's
// executors stay out of the first bundle for everyone else.
import { RunRegistry } from '@/assistant/tools';
import { realEngine } from '@/assistant/tools/engine-bridge';
import { loadCities } from '@/editor/cities';
import { exampleProfiles, normalize } from '@/engine';
import { createHousehold, openHousehold, saveNow, useHouseholds } from '@/state/households';
import { selectDirty, useProfileStore } from '@/state/profile-store';
import { activeView, navigateToLife, navigateToView } from '@/state/route';
import { useRunStore } from '@/state/run-store';
import { useSettings } from '@/state/settings-store';
import { toast } from '@/state/ui-store';
import { currentYearMonth } from '@/ui/format';
import { DEFAULT_VIEW, VIEWS } from '@/views/registry';
import { activityEnd, activityStart, askApproval, askConsent, useAgent } from './agent-store';
import { chartsPending, mountedCharts } from './chart-registry';
import { toDefinition, type WebMcpToolDefinition } from './core';
import { agentPlanStore } from './plan-bridge';
import { AGENT_TOOLS } from './tools';
import { DEFAULT_LIMITS, type AgentContext, type AppBridge } from './types';
import { watchApprovals } from './withdraw';

function appBridge(): AppBridge {
  const store = agentPlanStore();
  return {
    store,
    plan: () => {
      const s = useProfileStore.getState();
      if (!s.profile || !s.meta) return null;
      return {
        id: s.meta.id,
        name: s.profile.name,
        errors: s.report.errors.length,
        warnings: s.report.warnings.length,
        hasSessionHistory: s.past.length > 0 || s.future.length > 0 || selectDirty(s),
      };
    },
    savedPlans: () => useHouseholds.getState().list.map((h) => ({ id: h.id, name: h.name })),
    demos: () => exampleProfiles(),
    signedIn: () => useAgent.getState().signedIn,
    views: () => VIEWS.map((v) => ({ id: v.id, label: v.label })),
    activeView,
    showView: (id, lifePath) => {
      if (lifePath !== undefined) {
        navigateToLife(lifePath, 'sample');
        return `/life-stories?path=${Math.round(lifePath)}&why=sample`;
      }
      navigateToView(id);
      return id === DEFAULT_VIEW ? '/' : `/${id}`;
    },
    run: () => useRunStore.getState(),
    onRun: (listener) => useRunStore.subscribe(listener),
    charts: mountedCharts,
    chartsPending,
    normalise: normalize,
    openNew: async ({ profile, scenarios, origin }) => {
      // `createHousehold` replaces the open plan without saving it, so save first; then write the
      // new one at once so it is in the list of saved plans by the time the tool answers.
      await saveNow();
      const id = createHousehold({ profile, scenarios, origin });
      await saveNow();
      return id;
    },
    openSaved: openHousehold,
    cities: loadCities,
    today: () => currentYearMonth(),
    notify: ({ title, description, undo }) => {
      toast({ title, description, ...(undo ? { action: { label: 'Undo', run: () => useProfileStore.getState().undo() } } : {}) });
    },
  };
}

/** The context every tool call runs in. `ready` is the app's boot promise. */
export function createContext(options: { ready: () => Promise<void> }): AgentContext {
  let engine: AgentContext['engine'] | null = null;
  // Cards can only exist once there are tools to raise them, so this is early enough.
  watchApprovals();
  return {
    ready: options.ready,
    settings: () => {
      const s = useSettings.getState();
      return { access: s.agentAccess, autoApply: s.agentAutoApply };
    },
    consent: () => useAgent.getState().consent,
    askConsent: (signal) => askConsent(signal, DEFAULT_LIMITS.consentMs),
    askApproval: (request, signal) => askApproval(request, signal, DEFAULT_LIMITS.approvalMs),
    activity: { start: activityStart, end: activityEnd },
    limits: DEFAULT_LIMITS,
    app: appBridge(),
    get engine() {
      return (engine ??= realEngine());
    },
    runs: new RunRegistry(),
  };
}

/** Every tool, bound to the real app, in the form the browser registers. */
export function definitions(options: { ready: () => Promise<void> }): WebMcpToolDefinition[] {
  const ctx = createContext(options);
  return AGENT_TOOLS.map((tool) => toDefinition(tool, ctx));
}
