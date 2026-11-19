// The plan being edited: base profile, scenarios (named patches against it), the
// engine's validation report, undo/redo and dirty tracking.
//
// Every change to the plan goes through `edit(ops)`, so undo/redo, validation and
// "make this a scenario" behave the same for the editor, imports and the assistant.
import { create } from 'zustand';
import { applyPatch, toEngineError } from '@/engine';
import type { PatchOp, Profile, Report } from '@/engine/types';
import { compactPatch, describeOp } from './patches';
import { uniqueId } from './pointer';

export interface Scenario {
  id: string;
  name: string;
  /** Edits that turn the base profile into this scenario (core `scenario.rs`). */
  patch: PatchOp[];
  /** Enabled scenarios are simulated alongside the baseline and shown in comparisons. */
  enabled: boolean;
}

export interface ResolvedScenario {
  /** Base profile with the patch applied; null when the patch no longer applies. */
  profile: Profile | null;
  report: Report | null;
  error: string | null;
}

export interface RunSettings {
  paths: number;
  seed: number;
}

/** 'base' or a scenario id: where `edit()` writes by default. */
export type EditTarget = string;

export type EditResult = { ok: true; report: Report } | { ok: false; error: string };

export interface EditOptions {
  /** Shown in undo/redo affordances. Defaults to a description of the first operation. */
  label?: string;
  /** Consecutive edits with the same key within a short window share one undo step. */
  coalesce?: string;
  /** Overrides the store's current edit target. */
  target?: EditTarget;
}

interface Snapshot {
  profile: Profile;
  scenarios: Scenario[];
}

interface HistoryEntry {
  before: Snapshot;
  label: string;
  key: string | null;
  at: number;
  ops: PatchOp[];
  target: EditTarget;
}

export interface HouseholdMeta {
  id: string;
  createdAt: number;
  /** Where the plan came from, for the switcher: a demo slug, 'import', 'setup', 'share'. */
  origin: string;
}

export interface LoadInput {
  meta: HouseholdMeta;
  profile: Profile;
  scenarios?: Scenario[];
  run?: Partial<RunSettings>;
  /** Assumptions made when converting a v1 profile; shown once after import. */
  migrationNotes?: string[];
  /** True when this is new and has not been written to storage yet. */
  unsaved?: boolean;
}

interface ProfileState {
  meta: HouseholdMeta | null;
  profile: Profile | null;
  report: Report;
  scenarios: Scenario[];
  resolved: Record<string, ResolvedScenario>;
  editTarget: EditTarget;
  run: RunSettings;
  past: HistoryEntry[];
  future: { snapshot: Snapshot; label: string; ops: PatchOp[]; target: EditTarget }[];
  /** Increases with every change that should be persisted. */
  revision: number;
  savedRevision: number;
  migrationNotes: string[] | null;

  load: (input: LoadInput) => void;
  close: () => void;
  edit: (ops: PatchOp[], options?: EditOptions) => EditResult;
  /** Applies `ops` to the base profile without committing: for previews and diffs. */
  preview: (ops: PatchOp[]) => { profile: Profile; report: Report } | { error: string };
  undo: () => void;
  redo: () => void;

  addScenario: (input: { name: string; patch: PatchOp[]; enabled?: boolean }) => EditResult & { id?: string };
  updateScenario: (id: string, change: Partial<Pick<Scenario, 'name' | 'patch' | 'enabled'>>) => EditResult;
  removeScenario: (id: string) => void;
  /**
   * Makes a scenario the plan: its patch is applied to the baseline and the scenario is
   * retired, as one history entry, so a single undo restores both.
   */
  promoteScenario: (id: string) => EditResult;
  setEditTarget: (target: EditTarget) => void;
  /** Undoes the most recent baseline edit and re-creates it as a scenario. */
  moveLastEditToScenario: (name?: string) => EditResult & { id?: string };

  setRun: (run: Partial<RunSettings>) => void;
  markSaved: (revision: number) => void;
  dismissMigrationNotes: () => void;
}

const EMPTY_REPORT: Report = { errors: [], warnings: [] };
const COALESCE_MS = 1500;
const MAX_HISTORY = 200;

function resolve(profile: Profile, scenarios: Scenario[]): Record<string, ResolvedScenario> {
  const base = JSON.stringify(profile);
  const out: Record<string, ResolvedScenario> = {};
  for (const s of scenarios) {
    try {
      const r = applyPatch(base, s.patch);
      out[s.id] = { profile: r.profile, report: { errors: r.errors, warnings: r.warnings }, error: null };
    } catch (error) {
      out[s.id] = { profile: null, report: null, error: toEngineError(error).message };
    }
  }
  return out;
}

export const useProfileStore = create<ProfileState>((set, get) => {
  /** Commits a new snapshot with a history entry. */
  const commit = (
    next: Snapshot & { report?: Report; resolved?: Record<string, ResolvedScenario> },
    entry: Omit<HistoryEntry, 'before' | 'at'>,
  ) => {
    const s = get();
    if (!s.profile) return;
    const now = Date.now();
    const last = s.past.at(-1);
    const merge =
      entry.key !== null && last && last.key === entry.key && last.target === entry.target && now - last.at < COALESCE_MS;
    const past = merge
      ? [...s.past.slice(0, -1), { ...last, at: now, ops: compactPatch([...last.ops, ...entry.ops]) }]
      : [...s.past, { ...entry, before: { profile: s.profile, scenarios: s.scenarios }, at: now }].slice(-MAX_HISTORY);
    set({
      profile: next.profile,
      scenarios: next.scenarios,
      report: next.report ?? s.report,
      resolved: next.resolved ?? resolve(next.profile, next.scenarios),
      past,
      future: [],
      revision: s.revision + 1,
    });
  };

  const restore = (snapshot: Snapshot) => {
    const r = applyPatch(snapshot.profile, []);
    const s = get();
    set({
      profile: snapshot.profile,
      scenarios: snapshot.scenarios,
      report: { errors: r.errors, warnings: r.warnings },
      resolved: resolve(snapshot.profile, snapshot.scenarios),
      editTarget: snapshot.scenarios.some((x) => x.id === s.editTarget) ? s.editTarget : 'base',
      revision: s.revision + 1,
    });
  };

  return {
    meta: null,
    profile: null,
    report: EMPTY_REPORT,
    scenarios: [],
    resolved: {},
    editTarget: 'base',
    run: { paths: 10_000, seed: 1 },
    past: [],
    future: [],
    revision: 0,
    savedRevision: 0,
    migrationNotes: null,

    load: (input) => {
      // Through the engine once, so the stored profile is complete and canonical.
      const r = applyPatch(input.profile, []);
      const scenarios = input.scenarios ?? [];
      const revision = get().revision + 1;
      set({
        meta: input.meta,
        profile: r.profile,
        report: { errors: r.errors, warnings: r.warnings },
        scenarios,
        resolved: resolve(r.profile, scenarios),
        editTarget: 'base',
        run: {
          paths: input.run?.paths ?? r.profile.simulation.paths,
          seed: input.run?.seed ?? r.profile.simulation.seed,
        },
        past: [],
        future: [],
        revision,
        savedRevision: input.unsaved ? revision - 1 : revision,
        migrationNotes: input.migrationNotes?.length ? input.migrationNotes : null,
      });
    },

    close: () =>
      set({
        meta: null,
        profile: null,
        report: EMPTY_REPORT,
        scenarios: [],
        resolved: {},
        editTarget: 'base',
        past: [],
        future: [],
        migrationNotes: null,
      }),

    edit: (ops, options = {}) => {
      const s = get();
      if (!s.profile) return { ok: false, error: 'No plan is open.' };
      if (ops.length === 0) return { ok: true, report: s.report };
      const target = options.target ?? s.editTarget;
      const label = options.label ?? describeOp(ops[0] as PatchOp);
      const key = options.coalesce ?? null;
      try {
        if (target === 'base') {
          const r = applyPatch(s.profile, ops);
          const report = { errors: r.errors, warnings: r.warnings };
          commit({ profile: r.profile, scenarios: s.scenarios, report }, { label, key, ops, target });
          return { ok: true, report };
        }
        const scenario = s.scenarios.find((x) => x.id === target);
        if (!scenario) return { ok: false, error: `There is no scenario "${target}".` };
        const patch = compactPatch([...scenario.patch, ...ops]);
        const r = applyPatch(s.profile, patch);
        const scenarios = s.scenarios.map((x) => (x.id === target ? { ...x, patch } : x));
        commit({ profile: s.profile, scenarios }, { label, key, ops, target });
        return { ok: true, report: { errors: r.errors, warnings: r.warnings } };
      } catch (error) {
        return { ok: false, error: toEngineError(error).message };
      }
    },

    preview: (ops) => {
      const s = get();
      if (!s.profile) return { error: 'No plan is open.' };
      try {
        const r = applyPatch(s.profile, ops);
        return { profile: r.profile, report: { errors: r.errors, warnings: r.warnings } };
      } catch (error) {
        return { error: toEngineError(error).message };
      }
    },

    undo: () => {
      const s = get();
      const entry = s.past.at(-1);
      if (!entry || !s.profile) return;
      set({
        past: s.past.slice(0, -1),
        future: [
          ...s.future,
          { snapshot: { profile: s.profile, scenarios: s.scenarios }, label: entry.label, ops: entry.ops, target: entry.target },
        ],
      });
      restore(entry.before);
    },

    redo: () => {
      const s = get();
      const next = s.future.at(-1);
      if (!next || !s.profile) return;
      set({
        future: s.future.slice(0, -1),
        past: [
          ...s.past,
          {
            before: { profile: s.profile, scenarios: s.scenarios },
            label: next.label,
            key: null,
            at: 0,
            ops: next.ops,
            target: next.target,
          },
        ],
      });
      restore(next.snapshot);
    },

    addScenario: ({ name, patch, enabled = true }) => {
      const s = get();
      if (!s.profile) return { ok: false, error: 'No plan is open.' };
      try {
        const r = applyPatch(s.profile, patch);
        const id = uniqueId(name, s.scenarios.map((x) => x.id).concat('base'));
        const scenarios = [...s.scenarios, { id, name, patch: compactPatch(patch), enabled }];
        commit({ profile: s.profile, scenarios }, { label: `Add scenario "${name}"`, key: null, ops: [], target: id });
        return { ok: true, report: { errors: r.errors, warnings: r.warnings }, id };
      } catch (error) {
        return { ok: false, error: toEngineError(error).message };
      }
    },

    updateScenario: (id, change) => {
      const s = get();
      const scenario = s.scenarios.find((x) => x.id === id);
      if (!s.profile || !scenario) return { ok: false, error: `There is no scenario "${id}".` };
      try {
        const r = applyPatch(s.profile, change.patch ?? scenario.patch);
        const scenarios = s.scenarios.map((x) => (x.id === id ? { ...x, ...change } : x));
        commit(
          { profile: s.profile, scenarios },
          { label: `Change scenario "${scenario.name}"`, key: change.name !== undefined ? `scenario-name:${id}` : null, ops: [], target: id },
        );
        return { ok: true, report: { errors: r.errors, warnings: r.warnings } };
      } catch (error) {
        return { ok: false, error: toEngineError(error).message };
      }
    },

    removeScenario: (id) => {
      const s = get();
      const scenario = s.scenarios.find((x) => x.id === id);
      if (!s.profile || !scenario) return;
      commit(
        { profile: s.profile, scenarios: s.scenarios.filter((x) => x.id !== id) },
        { label: `Remove scenario "${scenario.name}"`, key: null, ops: [], target: 'base' },
      );
      if (get().editTarget === id) set({ editTarget: 'base' });
    },

    promoteScenario: (id) => {
      const s = get();
      const scenario = s.scenarios.find((x) => x.id === id);
      if (!s.profile || !scenario) return { ok: false, error: `There is no scenario "${id}".` };
      try {
        const r = applyPatch(s.profile, scenario.patch);
        const report = { errors: r.errors, warnings: r.warnings };
        commit({ profile: r.profile, scenarios: s.scenarios.filter((x) => x.id !== id), report }, { label: `Make “${scenario.name}” the plan`, key: null, ops: scenario.patch, target: 'base' });
        if (get().editTarget === id) set({ editTarget: 'base' });
        return { ok: true, report };
      } catch (error) {
        return { ok: false, error: toEngineError(error).message };
      }
    },

    setEditTarget: (target) => {
      if (target === 'base' || get().scenarios.some((x) => x.id === target)) set({ editTarget: target });
    },

    moveLastEditToScenario: (name) => {
      const s = get();
      const entry = s.past.at(-1);
      if (!entry || entry.target !== 'base' || entry.ops.length === 0) {
        return { ok: false, error: 'The last change was not an edit to the baseline plan.' };
      }
      get().undo();
      const result = get().addScenario({ name: name ?? entry.label, patch: entry.ops });
      if (!result.ok) get().redo();
      return result;
    },

    setRun: (run) => set((s) => ({ run: { ...s.run, ...run }, revision: s.revision + 1 })),
    markSaved: (revision) => set((s) => (revision > s.savedRevision ? { savedRevision: revision } : s)),
    dismissMigrationNotes: () => set({ migrationNotes: null }),
  };
});

// ---- selectors ------------------------------------------------------------------

/** The profile the editor shows: the baseline, or the baseline with the target scenario applied. */
export function selectEffectiveProfile(s: ProfileState): Profile | null {
  if (s.editTarget === 'base') return s.profile;
  return s.resolved[s.editTarget]?.profile ?? s.profile;
}

export function selectEffectiveReport(s: ProfileState): Report {
  if (s.editTarget === 'base') return s.report;
  return s.resolved[s.editTarget]?.report ?? s.report;
}

export const selectDirty = (s: ProfileState) => s.revision !== s.savedRevision;
export const selectCanUndo = (s: ProfileState) => s.past.length > 0;
export const selectCanRedo = (s: ProfileState) => s.future.length > 0;

/** The profile with the household's run settings written into `simulation`, for export. */
export function profileForExport(s: Pick<ProfileState, 'profile' | 'run'>): Profile | null {
  return s.profile && { ...s.profile, simulation: { ...s.profile.simulation, ...s.run } };
}
