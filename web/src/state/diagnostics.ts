// `window.__firesight`: read-only handles for the end-to-end and performance scripts in
// web/e2e (and for poking at a run from the console). Nothing in the app reads it.
import { aggregatorStats, getPool } from '@/engine';
import { useProfileStore } from './profile-store';
import { useRunStore } from './run-store';

export interface Diagnostics {
  run: typeof useRunStore;
  plan: typeof useProfileStore;
  stats: typeof aggregatorStats;
  pool: () => ReturnType<typeof getPool>;
}

export function installDiagnostics() {
  const diagnostics: Diagnostics = { run: useRunStore, plan: useProfileStore, stats: aggregatorStats, pool: getPool };
  (window as unknown as { __firesight: Diagnostics }).__firesight = diagnostics;
}
