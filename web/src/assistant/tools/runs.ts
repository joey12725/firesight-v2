// Simulation results the assistant has produced this session, so that a baseline is
// simulated once however many scenarios are compared with it, and so charts and life
// inspections can refer back to a run by id. Held in memory only.
import { engineInput, type EnginePool, type RunResult } from '@/engine/pool';
import type { Comparison, Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';

export interface RunRecord {
  id: string;
  label: string;
  profile: Profile;
  paths: number;
  seed: number;
  summary: Summary;
  acc: Uint8Array;
  /** Identifies profile + seed (the engine's own cache key). */
  key: string;
}

export interface RunJob {
  label: string;
  profile: Profile;
}

const MAX_RECORDS = 32;

export class RunRegistry {
  private readonly byKey = new Map<string, RunRecord>();
  private readonly byId = new Map<string, RunRecord>();
  /** Results other tools produced that a chart can draw (lever rankings, grids, traces). */
  private readonly data = new Map<string, unknown>();
  /** Paired comparisons already computed, keyed by the two run ids (A then B). */
  private readonly comparisons = new Map<string, Promise<Comparison>>();
  private counter = 0;
  private dataCounter = 0;
  /** The most recent run of the plan as it stood, for tools that default to "the baseline". */
  latestBaseline: string | null = null;

  get(id: string): RunRecord | undefined {
    return this.byId.get(id);
  }

  ids(): string[] {
    return [...this.byId.keys()];
  }

  putData(kind: string, value: unknown): string {
    const id = `${kind}_${++this.dataCounter}`;
    this.data.set(id, value);
    if (this.data.size > MAX_RECORDS) this.data.delete(this.data.keys().next().value as string);
    return id;
  }

  /** Stores under a key the caller chooses (traces, keyed by run and path). */
  setData(key: string, value: unknown) {
    this.data.set(key, value);
    if (this.data.size > MAX_RECORDS) this.data.delete(this.data.keys().next().value as string);
  }

  getData(id: string): unknown {
    return this.data.get(id);
  }

  /**
   * Core's paired comparison of two remembered runs (B minus A), computed once per pair:
   * a chart of a comparison reuses what `try_scenario` already asked for.
   */
  compare(engine: { compare(a: RunRecord, b: RunRecord): Promise<Comparison> }, a: RunRecord, b: RunRecord): Promise<Comparison> {
    const key = `${a.id}|${b.id}`;
    const hit = this.comparisons.get(key);
    if (hit) return hit;
    const pending = engine.compare(a, b);
    this.comparisons.set(key, pending);
    pending.catch(() => this.comparisons.delete(key));
    if (this.comparisons.size > MAX_RECORDS) this.comparisons.delete(this.comparisons.keys().next().value as string);
    return pending;
  }

  clear() {
    this.byKey.clear();
    this.byId.clear();
    this.data.clear();
    this.comparisons.clear();
    this.latestBaseline = null;
  }

  /**
   * Simulates every job on the same seed and path indices (common random numbers), as
   * one fairly scheduled batch. Jobs already simulated with the same inputs are reused.
   * `remember: false` keeps bulk work (grid cells, solver probes) out of the registry.
   */
  async simulate(
    pool: Pick<EnginePool, 'runMany'>,
    jobs: RunJob[],
    settings: { paths: number; seed: number; signal: AbortSignal; remember?: boolean },
  ): Promise<RunRecord[]> {
    const { paths, seed, signal } = settings;
    const remember = settings.remember ?? true;
    const keys = jobs.map((job) => `${engineInput(job.profile, seed).key}|${paths}`);
    const missing = new Map<string, RunJob>();
    keys.forEach((key, i) => {
      if (!this.byKey.has(key) && !missing.has(key)) missing.set(key, jobs[i] as RunJob);
    });
    const fresh = new Map<string, RunRecord>();
    if (missing.size > 0) {
      const batch = [...missing.entries()].map(([key, job]) => ({ id: key, profile: job.profile, paths }));
      const results = await pool.runMany(batch, { seed, signal, progressive: false });
      for (const [key, job] of missing) {
        const r = results.get(key) as RunResult;
        const record: RunRecord = { id: `run_${++this.counter}`, label: job.label, profile: job.profile, paths, seed, summary: r.summary, acc: r.acc, key };
        fresh.set(key, record);
        if (remember) this.remember(record);
      }
    }
    return keys.map((key) => (this.byKey.get(key) ?? fresh.get(key)) as RunRecord);
  }

  private remember(record: RunRecord) {
    this.byKey.set(record.key, record);
    this.byId.set(record.id, record);
    while (this.byId.size > MAX_RECORDS) {
      const oldest = [...this.byId.values()].find((r) => r.id !== this.latestBaseline) as RunRecord;
      this.byId.delete(oldest.id);
      this.byKey.delete(oldest.key);
    }
  }
}
