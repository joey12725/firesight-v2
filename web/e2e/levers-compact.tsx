// Harness for e2e/levers-compact.mjs: mounts the assistant's compact tornado and heat map
// at chat width, fed by real sweeps of the open plan converted to the stored shapes of
// the assistant's rank_levers and sweep tools. Loaded by URL inside the running app.
import { createRoot } from 'react-dom/client';
import { getPool, levers as engineLevers } from '@/engine';
import type { LeverRanking, SweepGrid } from '@/assistant/tools/explore';
import { useProfileStore } from '@/state/profile-store';
import Heatmap from '@/views/what-matters/compact/Heatmap';
import Tornado from '@/views/what-matters/compact/Tornado';
import { buildLevers, defaultAxes, effectOf, variablesFor } from '@/views/what-matters/model';
import { GRID_STAGES, GridSweeper, LeverSweeper } from '@/views/what-matters/sweeps';

function settled<S>(store: { subscribe: (l: () => void) => () => void; get: () => S }, done: (s: S) => boolean): Promise<S> {
  return new Promise((resolve) => {
    const check = () => done(store.get()) && (off(), resolve(store.get()));
    const off = store.subscribe(check);
    check();
  });
}

export async function mount() {
  const { profile, run } = useProfileStore.getState();
  if (!profile) throw new Error('no plan is open');
  const pool = getPool();
  const defs = buildLevers(profile, engineLevers(profile));
  const sweeper = new LeverSweeper(pool);
  sweeper.request(profile, run.seed, defs);
  const result = (await settled(sweeper, (s) => Boolean(s.run?.final) || s.error !== null)).run;
  if (!result) throw new Error('the lever sweep failed');
  const ranking: LeverRanking = {
    metric: 'success_rate',
    base: result.base.success_rate.value,
    paths: result.paths,
    seed: result.seed,
    levers: defs.map((l) => {
      const settings = l.sides.flatMap((s) => {
        const entry = result.sides[s.key];
        if (!entry) return [];
        const e = effectOf('holds', entry.paired, result.base, entry.headline);
        return [{ label: s.label, value: e.value, delta: e.delta, std_error: e.lo !== null && e.hi !== null ? (e.hi - e.lo) / 3.92 : null, within_noise: e.state === 'noise' }];
      });
      return { id: l.id, label: l.label, settings, swing: Math.max(0, ...settings.map((s) => Math.abs(s.delta ?? 0))) };
    }),
  };

  const vars = variablesFor(profile);
  const axes = defaultAxes(vars);
  const x = vars.find((v) => v.id === axes?.x);
  const y = vars.find((v) => v.id === axes?.y);
  if (!x || !y) throw new Error('no axes');
  const grids = new GridSweeper(pool);
  grids.request(profile, run.seed, x, y);
  const g = (await settled(grids, (s) => (s.run?.stage ?? 0) >= GRID_STAGES.length || s.error !== null)).run;
  if (!g) throw new Error('the grid sweep failed');
  const pick = <T,>(a: T[]) => a.filter((_, i) => i % 2 === 0);
  const unit = (id: string) => (id === 'spending' ? 'dollars' : id === 'withdrawal_rate' ? 'rate' : 'number');
  const grid: SweepGrid = {
    metric: 'success_rate',
    x: { label: x.label, unit: unit(x.id), values: pick(g.xs) },
    y: { label: y.label, unit: unit(y.id), values: pick(g.ys) },
    cells: pick(g.cells).map((row) => pick(row).map((c) => c?.levels.holds.value ?? null)),
    paths: Math.min(...g.cells.flat().map((c) => c?.paths ?? Infinity)),
    seed: g.seed,
  };
  const line: SweepGrid = { ...grid, y: null, cells: [grid.cells[Math.floor(grid.cells.length / 2)] ?? []] };

  const host = document.createElement('div');
  host.id = 'levers-compact';
  host.style.cssText = 'position:fixed;inset:0;z-index:9999;overflow:auto;padding:24px;display:flex;gap:24px;align-items:flex-start;background:var(--color-surface-0, Canvas)';
  document.body.append(host);
  const common = { compact: true as const, caption: '', summary: null, profile };
  createRoot(host).render(
    <>
      {[<Tornado key="t" kind="tornado" {...common} ranking={ranking} />, <Heatmap key="h" kind="heatmap" {...common} grid={grid} />, <Heatmap key="l" kind="heatmap" {...common} grid={line} />].map((chart, i) => (
        <div key={i} style={{ width: 320, flex: 'none' }}>
          {chart}
        </div>
      ))}
    </>,
  );
}
