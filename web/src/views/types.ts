// The contract between the results workspace and a view. Chart engineers implement
// `web/src/views/<name>/index.tsx` with a default export of `ComponentType<ViewProps>`;
// the entry in `registry.ts` already points at it.
import type { EnginePool } from '@/engine';
import type { Comparison, Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import type { RunState } from '@/state/run-store';

/** One enabled scenario, simulated on the same seed and path indices as the baseline. */
export interface ScenarioComparison {
  id: string;
  name: string;
  /** The baseline with the scenario's patch applied. */
  profile: Profile;
  /** Null until the scenario's first chunk has been summarised. */
  summary: Summary | null;
  /** Paired statistics against the baseline. Null until the engine provides them. */
  paired: Comparison | null;
}

export interface ViewProps {
  /** The baseline plan (complete: every field present). */
  profile: Profile;
  /**
   * The baseline result. Refined progressively while a run is in flight: early values
   * come from a few hundred lives, so draw them without waiting for `run.status === 'done'`.
   * Null before the first result and when the engine cannot run.
   */
  summary: Summary | null;
  /** Enabled scenarios, in the user's order. Empty when there are none. */
  comparison: ScenarioComparison[];
  /** Progress, seed and error state of the live run. `run.stale` marks a summary from before the latest edit. */
  run: RunState;
  /**
   * For work the live run does not cover: `pool.trace(profile, pathIndex)` for a life
   * story, `pool.runMany(...)` for sensitivity sweeps and heat-map grids (pass the
   * run's seed so results are paired), `pool.compare(...)` for ad-hoc comparisons.
   */
  pool: EnginePool;
}
