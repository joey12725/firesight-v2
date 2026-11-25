// `window.__firesight`: read-only handles for the end-to-end and performance scripts in
// web/e2e (and for poking at a run from the console). Nothing in the app reads it.
import { featuresOn } from '@/editor/realism';
import { aggregatorStats, getPool } from '@/engine';
import type { Profile } from '@/engine/types';
import { useProfileStore } from './profile-store';
import { useRunStore } from './run-store';
import { useSettings } from './settings-store';

export interface Diagnostics {
  run: typeof useRunStore;
  plan: typeof useProfileStore;
  settings: typeof useSettings;
  stats: typeof aggregatorStats;
  pool: () => ReturnType<typeof getPool>;
  /** Ids of the optional realism features a profile has switched on. */
  features: (profile: Profile) => string[];
}

export function installDiagnostics() {
  const diagnostics: Diagnostics = { run: useRunStore, plan: useProfileStore, settings: useSettings, stats: aggregatorStats, pool: getPool, features: (profile) => featuresOn(profile).map((f) => f.id) };
  (window as unknown as { __firesight: Diagnostics }).__firesight = diagnostics;
}
