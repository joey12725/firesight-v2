// Does the answer depend on the market model? On request (never by itself) the same plan
// is simulated on the same seed under each of the four market models, at a reduced
// number of lives, and the headline figures are set side by side with their intervals.
// The spread between models is the uncertainty no sampling interval shows.
import { useCallback, useEffect, useRef, useState } from 'react';
import { isAbort, type EnginePool } from '@/engine';
import type { Summary } from '@/engine/output-types';
import type { Profile } from '@/engine/types';
import type { RunState } from '@/state/run-store';
import { Button, Icon, count, moneyCompact } from '@/ui';
import { ChartFrame, SvgLayer, age, linearScale, niceDomain, ofHundred, textWidth, type TableData } from '@/ui/charts';
import { XAxis, type XFormat } from '@/ui/charts/ext/outcomes-XAxis';
import { triangulationTakeaway, type ModelResult } from './text';

export type MarketKind = Profile['assumptions']['market']['kind'];
export type MarketType = MarketKind['type'];

export const MODELS: { type: MarketType; label: string; gist: string }[] = [
  { type: 'bootstrap', label: 'History reshuffled', gist: 'Multi-year blocks of the 1926 to 2026 record, in random order' },
  { type: 'regime', label: 'Calm and turbulent regimes', gist: 'A fitted model that switches between calm and turbulent markets' },
  { type: 'lognormal', label: 'Textbook random walk', gist: 'Independent months with history’s averages: no crashes that cluster' },
  { type: 'historical', label: 'History as it happened', gist: 'Every starting month of the record, replayed in order' },
];

/** The same plan under another market model, everything else untouched. */
export function withMarket(profile: Profile, type: MarketType): Profile {
  const current = profile.assumptions.market.kind;
  const kind: MarketKind = type === current.type ? current : type === 'bootstrap' ? { type: 'bootstrap', mean_block_months: 60, from_year: null, joins: 'uniform' } : ({ type } as MarketKind);
  return { ...profile, assumptions: { ...profile.assumptions, market: { ...profile.assumptions.market, kind } } };
}

/** Lives per model: enough for a ±2 in 100 interval, few enough to finish in seconds. */
export const triangulationPaths = (total: number) => Math.max(200, Math.min(2000, total || 2000));

export interface TriangulationState {
  status: 'idle' | 'running' | 'done' | 'error';
  /** The plan the results belong to: a different object means the plan has changed since. */
  profile: Profile | null;
  paths: number;
  results: Partial<Record<MarketType, { summary: Summary; done: number }>>;
  error: string | null;
}

const IDLE: TriangulationState = { status: 'idle', profile: null, paths: 0, results: {}, error: null };

export function useTriangulation(profile: Profile, run: RunState, pool: EnginePool) {
  const [state, setState] = useState<TriangulationState>(IDLE);
  const controller = useRef<AbortController | null>(null);

  const cancel = useCallback(() => {
    controller.current?.abort();
    controller.current = null;
  }, []);
  // Leaving the view abandons the work.
  useEffect(() => cancel, [cancel]);

  const start = useCallback(() => {
    cancel();
    const abort = new AbortController();
    controller.current = abort;
    const paths = triangulationPaths(run.total);
    setState({ status: 'running', profile, paths, results: {}, error: null });
    pool
      .runMany(
        MODELS.map((m) => ({ id: m.type, profile: withMarket(profile, m.type), paths })),
        {
          seed: run.seed,
          signal: abort.signal,
          progressive: true,
          onProgress: (id, p) => setState((s) => (s.profile === profile && s.status === 'running' ? { ...s, results: { ...s.results, [id]: { summary: p.summary, done: p.done } } } : s)),
        },
      )
      .then((results) => {
        if (abort.signal.aborted) return;
        setState((s) => (s.profile === profile ? { ...s, status: 'done', results: Object.fromEntries(MODELS.flatMap((m) => (results.get(m.type) ? [[m.type, { summary: (results.get(m.type) as { summary: Summary }).summary, done: paths }]] : []))) } : s));
      })
      .catch((error: unknown) => {
        if (isAbort(error) || abort.signal.aborted) return;
        setState((s) => (s.profile === profile ? { ...s, status: 'error', error: error instanceof Error ? error.message : 'The comparison could not be run.' } : s));
      });
  }, [cancel, pool, profile, run.seed, run.total]);

  const stop = useCallback(() => {
    cancel();
    setState((s) => (s.status === 'running' ? { ...s, status: Object.keys(s.results).length ? 'done' : 'idle' } : s));
  }, [cancel]);

  return { state, start, stop, outdated: state.profile !== null && state.profile !== profile };
}

interface Metric {
  key: string;
  title: string;
  format: XFormat;
  read: (s: Summary) => { value: number; lo: number; hi: number } | null;
  text: (v: number) => string;
  zero?: boolean;
}

const METRICS: Metric[] = [
  { key: 'success', title: 'Lives in which the plan holds, of 100', format: (v) => String(Math.round(v * 100)), read: (s) => s.headline.success_rate, text: (v) => ofHundred(v).n },
  {
    key: 'fi',
    title: 'Age work becomes optional, middle life',
    format: (v) => String(v),
    read: (s) => {
      const f = s.headline.fi_age;
      return f.p50 === null ? null : { value: f.p50, lo: f.p50_lo ?? f.p50, hi: f.p50_hi ?? f.p50 };
    },
    text: (v) => age(v),
  },
  {
    key: 'wealth',
    title: 'Net worth at the end, middle life',
    format: 'money',
    read: (s) => {
      const w = s.headline.terminal_net_worth;
      return w.p50 === null ? null : { value: w.p50, lo: w.p50_lo ?? w.p50, hi: w.p50_hi ?? w.p50 };
    },
    text: moneyCompact,
    zero: true,
  },
];

export function modelResults(state: TriangulationState): ModelResult[] {
  return MODELS.flatMap((m) => {
    const r = state.results[m.type];
    return r ? [{ kind: m.type, label: m.label, success: r.summary.headline.success_rate }] : [];
  });
}

export function triangulationTable(state: TriangulationState): TableData {
  const pct = (v: number) => `${(v * 100).toFixed(1)}%`;
  return {
    caption: `The same plan under each market model, ${count(state.paths)} lives each on the same seed`,
    columns: [
      { key: 'model', label: 'Market model', align: 'left' },
      { key: 'lives', label: 'Lives simulated' },
      { key: 'success', label: 'Plan holds', format: pct },
      { key: 'success_lo', label: '95% low', format: pct },
      { key: 'success_hi', label: '95% high', format: pct },
      { key: 'fi', label: 'Median age work optional', format: (v) => age(v) },
      { key: 'wealth', label: 'Median net worth at the end', format: moneyCompact },
      { key: 'wealth_p10', label: '10th percentile', format: moneyCompact },
    ],
    rows: MODELS.flatMap((m) => {
      const r = state.results[m.type];
      if (!r) return [];
      const h = r.summary.headline;
      return [{ model: m.label, lives: r.summary.paths, success: h.success_rate.value, success_lo: h.success_rate.lo, success_hi: h.success_rate.hi, fi: h.fi_age.p50, wealth: h.terminal_net_worth.p50, wealth_p10: h.terminal_net_worth.p10 }];
    }),
  };
}

function MetricPanel({ metric, state, current, labels }: { metric: Metric; state: TriangulationState; current: MarketType; labels: boolean }) {
  const rows = MODELS.map((m) => ({ model: m, value: state.results[m.type] ? metric.read((state.results[m.type] as { summary: Summary }).summary) : null }));
  const values = rows.flatMap((r) => (r.value ? [r.value.lo, r.value.hi, r.value.value] : []));
  return (
    <div className="min-w-0">
      <h4 className="text-xs font-medium text-ink-2">{metric.title}</h4>
      <ChartFrame height={(w) => (labels && w < 440 ? 42 : 30) * rows.length + 34} margin={{ top: 6, right: 54, bottom: 24, left: 8 }} aria-label={`${metric.title}, under each market model`}>
        {({ plot, theme, width }) => {
          if (values.length === 0) return null;
          // Model names sit in a left column when there is room, else above each row.
          const side = labels && width >= 440 ? 178 : 0;
          const above = labels && width < 440;
          const ROW = above ? 42 : 30;
          const lo = Math.min(...values);
          const hi = Math.max(...values);
          const pad = (hi - lo || Math.abs(hi) * 0.05 || 1) * 0.25;
          const domain = niceDomain(metric.key === 'success' ? Math.max(0, lo - pad) : lo - pad, metric.key === 'success' ? Math.min(1, hi + pad) : hi + pad, 3);
          const x = linearScale(domain, [plot.x + side, plot.right]);
          return (
            <SvgLayer>
              <XAxis x={x} format={metric.format} count={3} grid from={plot.x + side} />
              {rows.map((r, i) => {
                const cy = plot.y + ROW * i + (above ? 29 : ROW / 2);
                const mine = r.model.type === current;
                const name = labels && (
                  <text x={plot.x} y={above ? cy - 13 : cy + 4} fontSize={11.5} fontWeight={mine ? 600 : 400} fill={mine ? theme.ink.primary : theme.ink.secondary}>
                    {r.model.label}
                    {mine ? ' · this plan' : ''}
                  </text>
                );
                if (!r.value)
                  return (
                    <g key={r.model.type}>
                      {name}
                      <text x={plot.x + side + 4} y={cy + 4} fontSize={11} fill={theme.ink.tertiary}>
                        {state.results[r.model.type] ? 'no value' : 'waiting…'}
                      </text>
                    </g>
                  );
                const color = mine ? theme.baseline.line : theme.ink.secondary;
                const label = metric.text(r.value.value);
                const px = x(r.value.value);
                return (
                  <g key={r.model.type}>
                    {name}
                    <line x1={x(r.value.lo)} x2={x(r.value.hi)} y1={cy} y2={cy} stroke={color} strokeWidth={2} strokeLinecap="round" />
                    <circle cx={px} cy={cy} r={mine ? 5 : 4} fill={color} stroke={theme.surface} strokeWidth={2} />
                    <text x={Math.min(plot.right + 50 - textWidth(label, 11), Math.max(x(r.value.hi), px) + 9)} y={cy + 4} fontSize={11} fontWeight={mine ? 600 : 400} fill={theme.ink.primary} style={{ fontVariantNumeric: 'tabular-nums' }}>
                      {label}
                    </text>
                  </g>
                );
              })}
            </SvgLayer>
          );
        }}
      </ChartFrame>
    </div>
  );
}

/** The strip: one panel per headline figure, a row per model, each a dot with its 95% interval. Side by side when there is room. */
export function TriangulationStrip({ state, current }: { state: TriangulationState; current: MarketType }) {
  return (
    <>
      <div className="grid grid-cols-1 gap-y-6 @4xl:hidden">
        {METRICS.map((metric) => (
          <MetricPanel key={metric.key} metric={metric} state={state} current={current} labels />
        ))}
      </div>
      <div className="hidden grid-cols-[minmax(0,7fr)_minmax(0,4fr)_minmax(0,4fr)] gap-x-6 @4xl:grid">
        {METRICS.map((metric, i) => (
          <MetricPanel key={metric.key} metric={metric} state={state} current={current} labels={i === 0} />
        ))}
      </div>
    </>
  );
}

export function TriangulationIntro({ state, run, onStart, onStop, outdated }: { state: TriangulationState; run: RunState; onStart: () => void; onStop: () => void; outdated: boolean }) {
  const paths = triangulationPaths(run.total);
  const busy = state.status === 'running';
  const doneLives = MODELS.reduce((n, m) => n + (state.results[m.type]?.done ?? 0), 0);
  return (
    <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
      {busy ? (
        <Button size="sm" icon="x" onClick={onStop}>
          Stop
        </Button>
      ) : (
        <Button size="sm" variant={state.status === 'idle' ? 'primary' : 'secondary'} icon={state.status === 'idle' ? 'layers' : 'rotate'} onClick={onStart} disabled={run.status === 'blocked' || run.status === 'idle'} data-testid="triangulate">
          {state.status === 'idle' ? 'Run the plan under all four models' : 'Run again'}
        </Button>
      )}
      <p className="min-w-0 flex-1 basis-64 text-xs leading-relaxed text-ink-3" role="status">
        {busy
          ? `Simulating ${count(doneLives)} of ${count(paths * MODELS.length)} lives…`
          : state.status === 'error'
            ? state.error
            : outdated
              ? 'The plan has changed since these were run. Run again to bring them up to date.'
              : state.status === 'done'
                ? `${count(state.paths)} lives per model, the same seed as the plan, so the non-market luck (careers, health, lifespans) is identical in all four.`
                : `Simulates ${count(paths)} lives under each of the four market models (${count(paths * MODELS.length)} in all, a few seconds) on the plan’s seed. Nothing in the plan changes.`}
      </p>
    </div>
  );
}

export function ModelGlossary() {
  return (
    <dl className="mt-4 grid grid-cols-1 gap-x-8 gap-y-2 text-xs @2xl:grid-cols-2">
      {MODELS.map((m) => (
        <div key={m.type} className="flex gap-2">
          <Icon name="minus" size={12} className="mt-0.5 shrink-0 text-ink-3" />
          <div>
            <dt className="inline font-medium text-ink-2">{m.label}. </dt>
            <dd className="inline text-ink-3">{m.gist}.</dd>
          </div>
        </div>
      ))}
    </dl>
  );
}

export function triangulationSentence(state: TriangulationState, current: MarketType): string {
  const results = modelResults(state);
  if (state.status === 'idle') return 'Every figure on this page comes from one model of how markets behave. Whether the answer survives a change of model is the uncertainty no sampling interval shows.';
  if (results.length < MODELS.length) return 'Simulating the plan under each market model…';
  const wealth = MODELS.flatMap((m) => {
    const p50 = state.results[m.type]?.summary.headline.terminal_net_worth.p50;
    return p50 === null || p50 === undefined ? [] : [p50];
  });
  const [low, high] = [Math.min(...wealth), Math.max(...wealth)];
  const endings = wealth.length === MODELS.length && low > 0 && high / low >= 1.15 ? ` The middle life’s net worth at the end runs from ${moneyCompact(low)} to ${moneyCompact(high)} across them.` : '';
  return `${triangulationTakeaway(results, current)}${endings}`;
}
