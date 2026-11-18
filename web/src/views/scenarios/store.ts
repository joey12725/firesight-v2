// What the reader has chosen on the Scenarios view. Kept outside the component tree so
// the choices survive a visit to another view and a re-run.
import { create } from 'zustand';
import type { MetricKey } from './model';

interface ScenarioUi {
  /** The scenario compared with the plan in detail. Null follows the first enabled one. */
  selected: string | null;
  /** The measure scenarios are ranked and sorted on. */
  metric: MetricKey;
  select: (id: string | null) => void;
  setMetric: (metric: MetricKey) => void;
}

export const useScenarioUi = create<ScenarioUi>((set) => ({
  selected: null,
  metric: 'success',
  select: (selected) => set({ selected }),
  setMetric: (metric) => set({ metric }),
}));
